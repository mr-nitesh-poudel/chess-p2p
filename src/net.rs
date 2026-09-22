//! Chess's own wire protocol, spoken over a [`session`](crate::session) link.
//!
//! Pairing, the code and the handshake all happen in the session layer; by the
//! time this module sees the connection, both sides have proved they hold the
//! same code and agreed to play chess. What is left is newline-delimited text:
//! a UCI move per line, plus a couple of control words.

use anyhow::{Context, Result};
use iroh::endpoint::SendStream;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::session::{self, Code, Game, Link, Progress, Reader, Target};

pub const CHESS: Game = Game {
    name: "chess",
    version: 1,
};

/// Something the peer did, delivered to the UI loop.
#[derive(Debug)]
pub enum NetEvent {
    /// Pairing moved along.
    Progress(Progress),
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

    /// Resolves once the endpoint has reached a relay and knows its address.
    pub async fn online(&self) {
        self.endpoint.online().await;
    }

    pub async fn shutdown(self) {
        self.endpoint.close().await;
    }
}

/// Wait for an opponent who knows `code`. With `publish` the code goes on the
/// DHT so it can be looked up; without it the opponent must dial our address.
pub async fn host(code: Code, publish: bool, events: UnboundedSender<NetEvent>) -> Result<Net> {
    let endpoint = session::bind().await?;
    let progress = reporter(&events);
    let pairing = {
        let endpoint = endpoint.clone();
        async move { session::host(&endpoint, code, CHESS, publish, progress).await }
    };
    Ok(start(endpoint, pairing, events))
}

/// Pair with the host behind `code`.
pub async fn join(code: Code, target: Target, events: UnboundedSender<NetEvent>) -> Result<Net> {
    let endpoint = session::bind().await?;
    let progress = reporter(&events);
    let pairing = {
        let endpoint = endpoint.clone();
        async move { session::join(&endpoint, code, &[CHESS], target, progress).await }
    };
    Ok(start(endpoint, pairing, events))
}

fn reporter(events: &UnboundedSender<NetEvent>) -> impl Fn(Progress) + Send + 'static {
    let events = events.clone();
    move |p| {
        let _ = events.send(NetEvent::Progress(p));
    }
}

/// Pair in the background, then play over the link until one side hangs up.
fn start(
    endpoint: Endpoint,
    pairing: impl Future<Output = Result<Link>> + Send + 'static,
    events: UnboundedSender<NetEvent>,
) -> Net {
    let (out_tx, out_rx) = unbounded_channel();
    let id = endpoint.id();
    tokio::spawn(async move {
        let result = async {
            let link = pairing.await?;
            let _ = events.send(NetEvent::Connected(link.peer));
            pump(link.send, link.lines, out_rx, events.clone()).await
        }
        .await;
        report(result, &events);
    });
    Net {
        id,
        out: out_tx,
        endpoint,
    }
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
    mut send: SendStream,
    mut lines: Reader,
    mut out_rx: UnboundedReceiver<Out>,
    events: UnboundedSender<NetEvent>,
) -> Result<()> {
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
