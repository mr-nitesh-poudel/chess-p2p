//! Chess's own wire protocol, spoken over a [`session`](crate::session) link.
//!
//! Pairing, codes and invites all happen in the session layer; by the time
//! this module sees the connection, both sides know who the other is and have
//! agreed to play chess. What is left is newline-delimited text: a UCI move
//! per line, plus a few control words.

use std::time::Duration;

use anyhow::{Context, Result};
use iroh::EndpointId;
use iroh::endpoint::SendStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::session::{Game, Link, Progress, Reader};

pub const CHESS: Game = Game {
    name: "chess",
    version: 1,
};

/// Something the peer did, delivered to the UI loop.
#[derive(Debug)]
pub enum NetEvent {
    /// Pairing by code moved along.
    Progress(Progress),
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

/// A game in progress with a peer. Dropping it leaves the game, and the peer
/// is told so.
pub struct Net {
    pub peer: EndpointId,
    pub out: UnboundedSender<Out>,
}

/// Play over `link` until one side hangs up.
pub fn play(link: Link, events: UnboundedSender<NetEvent>) -> Net {
    let (out_tx, out_rx) = unbounded_channel();
    let peer = link.peer;
    tokio::spawn(async move {
        let msg = match pump(link.send, link.lines, out_rx, &events).await {
            Ok(Ended::ByUs) => return,
            Ok(Ended::ByThem) => "your opponent left".to_string(),
            Err(e) => format!("lost the connection to your opponent ({e:#})"),
        };
        let _ = events.send(NetEvent::Disconnected(msg));
    });
    Net { peer, out: out_tx }
}

enum Ended {
    ByUs,
    ByThem,
}

/// Shuttle lines both ways until one side hangs up.
async fn pump(
    mut send: SendStream,
    mut lines: Reader,
    mut out_rx: UnboundedReceiver<Out>,
    events: &UnboundedSender<NetEvent>,
) -> Result<Ended> {
    loop {
        tokio::select! {
            outgoing = out_rx.recv() => {
                let Some(msg) = outgoing else {
                    // Say goodbye rather than just vanishing, and give it a
                    // moment to arrive before the connection goes.
                    let _ = send.write_all(b"bye\n").await;
                    if send.finish().is_ok() {
                        let _ = tokio::time::timeout(Duration::from_secs(1), send.stopped()).await;
                    }
                    return Ok(Ended::ByUs);
                };
                send.write_all(msg.line().as_bytes()).await.context("writing to peer")?;
            }
            incoming = lines.next_line() => {
                let Some(line) = incoming.context("reading from peer")? else {
                    return Ok(Ended::ByThem);
                };
                let (word, rest) = line.trim_end().split_once(' ').unwrap_or((line.trim_end(), ""));
                let event = match word {
                    "move" => NetEvent::Move(rest.to_string()),
                    "resign" => NetEvent::Resign,
                    "draw" => NetEvent::Draw,
                    "bye" => return Ok(Ended::ByThem),
                    // Unknown verbs are ignored so the protocol can grow.
                    _ => continue,
                };
                if events.send(event).is_err() {
                    return Ok(Ended::ByUs);
                }
            }
        }
    }
}
