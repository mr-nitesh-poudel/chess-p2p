//! Peer-to-peer transport over iroh.
//!
//! One QUIC bi-directional stream carries newline-delimited text: a UCI move
//! per line, plus a couple of control words. The host accepts the stream, the
//! joiner opens it.

use anyhow::{Context, Result};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const ALPN: &[u8] = b"chess-p2p/1";

/// Opens the stream. Carries the protocol version for future use.
const HELLO: &str = "hello 1\n";

/// Something the peer did, delivered to the UI loop.
#[derive(Debug)]
pub enum NetEvent {
    /// The endpoint has published its address and can be dialled.
    Online,
    Connected(EndpointId),
    Move(String),
    Resign,
    Draw,
    Disconnected(String),
}

/// Something to tell the peer.
#[derive(Debug)]
pub enum Out {
    Move(String),
    Resign,
    Draw,
}

impl Out {
    fn line(&self) -> String {
        match self {
            Out::Move(uci) => format!("move {uci}\n"),
            Out::Resign => "resign\n".into(),
            Out::Draw => "draw\n".into(),
        }
    }
}

pub struct Net {
    pub id: EndpointId,
    pub out: UnboundedSender<Out>,
    endpoint: Endpoint,
}

impl Net {
    /// Our full address, including the sockets we can be reached on directly.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    pub async fn shutdown(self) {
        self.endpoint.close().await;
    }
}

/// Bind an endpoint and wait for one opponent to dial in.
pub async fn host(events: UnboundedSender<NetEvent>) -> Result<Net> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("binding iroh endpoint")?;
    let id = endpoint.id();
    let (out_tx, out_rx) = unbounded_channel();

    tokio::spawn({
        let endpoint = endpoint.clone();
        let events = events.clone();
        async move {
            endpoint.online().await;
            let _ = events.send(NetEvent::Online);

            let result = async {
                let incoming = endpoint.accept().await.context("endpoint closed")?;
                let conn = incoming.await.context("accepting connection")?;
                let peer = conn.remote_id();
                let (send, recv) = conn.accept_bi().await.context("accepting stream")?;
                let _ = events.send(NetEvent::Connected(peer));
                pump(send, recv, out_rx, events.clone()).await
            }
            .await;
            report(result, &events);
        }
    });

    Ok(Net {
        id,
        out: out_tx,
        endpoint,
    })
}

/// Dial a host. Given a bare [`EndpointId`], discovery resolves the rest.
pub async fn join(peer: impl Into<EndpointAddr>, events: UnboundedSender<NetEvent>) -> Result<Net> {
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("binding iroh endpoint")?;
    let id = endpoint.id();
    let (out_tx, out_rx) = unbounded_channel();
    let peer = peer.into();

    tokio::spawn({
        let endpoint = endpoint.clone();
        let events = events.clone();
        async move {
            let _ = events.send(NetEvent::Online);
            let result = async {
                let peer_id = peer.id;
                let conn = endpoint
                    .connect(peer, ALPN)
                    .await
                    .context("could not reach that opponent")?;
                let (mut send, recv) = conn.open_bi().await.context("opening stream")?;
                // QUIC does not put a stream on the wire until it carries data,
                // so the host's `accept_bi` only returns once we say something.
                send.write_all(HELLO.as_bytes())
                    .await
                    .context("greeting host")?;
                let _ = events.send(NetEvent::Connected(peer_id));
                pump(send, recv, out_rx, events.clone()).await
            }
            .await;
            report(result, &events);
        }
    });

    Ok(Net {
        id,
        out: out_tx,
        endpoint,
    })
}

fn report(result: Result<()>, events: &UnboundedSender<NetEvent>) {
    let msg = match result {
        Ok(()) => "opponent disconnected".to_string(),
        Err(e) => format!("{e:#}"),
    };
    let _ = events.send(NetEvent::Disconnected(msg));
}

/// Shuttle lines both ways until one side hangs up.
async fn pump(
    mut send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    mut out_rx: UnboundedReceiver<Out>,
    events: UnboundedSender<NetEvent>,
) -> Result<()> {
    let mut lines = BufReader::new(recv).lines();
    loop {
        tokio::select! {
            outgoing = out_rx.recv() => {
                let Some(msg) = outgoing else { return Ok(()) };
                send.write_all(msg.line().as_bytes()).await.context("writing to peer")?;
            }
            incoming = lines.next_line() => {
                let Some(line) = incoming.context("reading from peer")? else { return Ok(()) };
                let (word, rest) = line.trim_end().split_once(' ').unwrap_or((line.trim_end(), ""));
                let event = match word {
                    "move" => NetEvent::Move(rest.to_string()),
                    "resign" => NetEvent::Resign,
                    "draw" => NetEvent::Draw,
                    // Unknown verbs are ignored so the protocol can grow.
                    _ => continue,
                };
                if events.send(event).is_err() {
                    return Ok(());
                }
            }
        }
    }
}
