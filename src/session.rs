//! Getting two players connected, whatever they are going to play.
//!
//! The host makes up a short [`Code`] and publishes its address under it on the
//! DHT ([`rendezvous`]). The joiner types the code in, looks the address up and
//! dials. Then both prove they hold the same code and agree on the game
//! ([`handshake`]), and the caller gets back an authenticated [`Link`] to speak
//! its own protocol over.

pub mod code;
pub mod handshake;
pub mod rendezvous;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::endpoint::{SendStream, presets};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::io::{AsyncBufReadExt, BufReader};

pub use code::{Code, CodeError};
pub use handshake::{Declined, Reader, WrongCode};

/// One protocol for every game: which one is being played is settled inside
/// the handshake rather than by ALPN.
pub const ALPN: &[u8] = b"chess-p2p/session/1";

/// How long one attempt at the handshake may take before it is dropped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to wait for the endpoint to reach a relay, without which there is
/// nothing worth publishing.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(30);

/// Wrong codes the host puts up with before it stops listening. Each is one
/// guess at the code, so this bounds the odds of an attacker getting in.
pub const MAX_WRONG_CODES: u32 = 3;

const PUBLISH_ATTEMPTS: u32 = 3;

/// A game and the version of its wire protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Game {
    pub name: &'static str,
    pub version: u32,
}

/// How pairing is going, for the UI to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The host's code is on the DHT and can be looked up.
    Listed,
    /// The joiner has found the host and is dialling it.
    Found,
    /// Someone dialled the host with the wrong code.
    WrongCode { attempts: u32 },
    /// Someone got in but could not play what the host is playing.
    Declined,
}

/// A connected, authenticated peer, ready for the game's own protocol.
pub struct Link {
    pub peer: EndpointId,
    pub game: Game,
    pub send: SendStream,
    pub lines: Reader,
}

/// Where the joiner finds the host.
pub enum Target {
    /// Look the code up on the DHT.
    Lookup,
    /// Dial this address directly; the code is still needed for the handshake.
    Addr(EndpointAddr),
}

pub async fn bind() -> Result<Endpoint> {
    Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("binding iroh endpoint")
}

/// Wait for one opponent who knows `code`, publishing it first if `publish`.
pub async fn host(
    endpoint: &Endpoint,
    code: Code,
    game: Game,
    publish: bool,
    progress: impl Fn(Progress),
) -> Result<Link> {
    if publish {
        tokio::time::timeout(ONLINE_TIMEOUT, endpoint.online())
            .await
            .context("could not get online — check your connection")?;
        publish_with_retries(code, &endpoint.addr()).await?;
        progress(Progress::Listed);
    }

    let me = endpoint.id();
    let mut wrong = 0;
    loop {
        let incoming = endpoint.accept().await.context("endpoint closed")?;
        let attempt = async {
            let conn = incoming.await.context("accepting connection")?;
            let peer = conn.remote_id();
            let (mut send, recv) = conn.accept_bi().await.context("accepting stream")?;
            let mut lines = BufReader::new(recv).lines();
            handshake::host(&mut send, &mut lines, code, game, (me, peer)).await?;
            anyhow::Ok(Link {
                peer,
                game,
                send,
                lines,
            })
        };
        // A failed attempt does not end the wait: the real opponent may still
        // be on the way.
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, attempt).await {
            Ok(Ok(link)) => return Ok(link),
            Ok(Err(e)) if e.is::<WrongCode>() => {
                wrong += 1;
                progress(Progress::WrongCode { attempts: wrong });
                if wrong >= MAX_WRONG_CODES {
                    bail!("too many wrong codes — start a new game for a fresh one");
                }
            }
            Ok(Err(e)) if e.is::<Declined>() => progress(Progress::Declined),
            _ => {}
        }
    }
}

async fn publish_with_retries(code: Code, addr: &EndpointAddr) -> Result<()> {
    let mut attempt = 1;
    loop {
        match rendezvous::publish(code, addr).await {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= PUBLISH_ATTEMPTS => return Err(e),
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Find the host behind `code` and pair with it, accepting any of `games`.
pub async fn join(
    endpoint: &Endpoint,
    code: Code,
    games: &[Game],
    target: Target,
    progress: impl Fn(Progress),
) -> Result<Link> {
    let addr = match target {
        Target::Lookup => rendezvous::resolve(code).await?,
        Target::Addr(addr) => addr,
    };
    progress(Progress::Found);

    let host = addr.id;
    let conn = endpoint
        .connect(addr, ALPN)
        .await
        .context("found the game, but could not reach your opponent")?;
    let (mut send, recv) = conn.open_bi().await.context("opening stream")?;
    let mut lines = BufReader::new(recv).lines();
    let game = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        handshake::join(&mut send, &mut lines, code, games, (host, endpoint.id())),
    )
    .await
    .context("your opponent stopped answering")??;
    Ok(Link {
        peer: host,
        game,
        send,
        lines,
    })
}
