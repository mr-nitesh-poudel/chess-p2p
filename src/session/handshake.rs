//! What the two sides say before the game starts, one line at a time:
//!
//! ```text
//! joiner  pake <hex>        SPAKE2, keyed by the code
//! host    pake <hex>
//! joiner  confirm <hex>     proof we reached the same key
//! host    confirm <hex>     ...or `no wrong-code`
//! host    game chess 1      what the host is playing
//! joiner  ok                ...or `no <why>`
//! ```
//!
//! SPAKE2 gives someone without the code exactly one guess per connection, and
//! the host only allows a few wrong ones in total. The confirmations are keyed
//! over both endpoint ids, which iroh has already authenticated, so a man in
//! the middle relaying the exchange ends up with ids that do not match.

use anyhow::{Context, Result, anyhow, bail};
use iroh::EndpointId;
use iroh::endpoint::{RecvStream, SendStream};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use tokio::io::{BufReader, Lines};

use super::{Code, Game};

pub type Reader = Lines<BufReader<RecvStream>>;

const IDENTITY: &[u8] = b"chess-p2p/session/1";

/// The peer used a different code. The host counts these; anything else going
/// wrong mid-handshake is just a dropped attempt.
#[derive(Debug)]
pub struct WrongCode;

impl std::fmt::Display for WrongCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the code did not match — check it with your opponent")
    }
}

impl std::error::Error for WrongCode {}

/// The joiner got in but cannot play the host's game.
#[derive(Debug)]
pub struct Declined;

impl std::fmt::Display for Declined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("your opponent's build cannot play this game")
    }
}

impl std::error::Error for Declined {}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Role {
    Host = 0,
    Joiner = 1,
}

pub async fn host(
    send: &mut SendStream,
    lines: &mut Reader,
    code: Code,
    game: Game,
    ids: (EndpointId, EndpointId),
) -> Result<()> {
    let (spake, ours) = start(code);
    let theirs = unhex(&hear(lines, "pake").await?)?;
    say(send, &format!("pake {}", hex(&ours))).await?;
    let key = finish(spake, &theirs)?;

    let claimed = hear(lines, "confirm").await?;
    if !matches(&claimed, tag(&key, Role::Joiner, ids)) {
        refuse(send, "wrong-code").await;
        return Err(WrongCode.into());
    }
    say(
        send,
        &format!("confirm {}", tag(&key, Role::Host, ids).to_hex()),
    )
    .await?;

    say(send, &format!("game {} {}", game.name, game.version)).await?;
    hear(lines, "ok").await?;
    Ok(())
}

/// Returns whichever of `games` the host turned out to be playing.
pub async fn join(
    send: &mut SendStream,
    lines: &mut Reader,
    code: Code,
    games: &[Game],
    ids: (EndpointId, EndpointId),
) -> Result<Game> {
    let (spake, ours) = start(code);
    say(send, &format!("pake {}", hex(&ours))).await?;
    let theirs = unhex(&hear(lines, "pake").await?)?;
    let key = finish(spake, &theirs)?;

    say(
        send,
        &format!("confirm {}", tag(&key, Role::Joiner, ids).to_hex()),
    )
    .await?;
    let claimed = hear(lines, "confirm").await?;
    if !matches(&claimed, tag(&key, Role::Host, ids)) {
        return Err(WrongCode.into());
    }

    let offer = hear(lines, "game").await?;
    let game = offer
        .split_once(' ')
        .and_then(|(name, version)| {
            let version = version.parse().ok()?;
            games
                .iter()
                .find(|g| g.name == name && g.version == version)
        })
        .copied();
    match game {
        Some(game) => {
            say(send, "ok").await?;
            Ok(game)
        }
        None => {
            refuse(send, "unsupported-game").await;
            bail!("your opponent is playing {offer}, which this build does not have")
        }
    }
}

fn start(code: Code) -> (Spake2<Ed25519Group>, Vec<u8>) {
    Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.to_string()),
        &Identity::new(IDENTITY),
    )
}

fn finish(spake: Spake2<Ed25519Group>, theirs: &[u8]) -> Result<[u8; 32]> {
    let key = spake
        .finish(theirs)
        .map_err(|e| anyhow!("peer sent a bad key exchange: {e:?}"))?;
    key.try_into()
        .map_err(|_| anyhow!("key exchange gave a key of the wrong size"))
}

fn tag(key: &[u8; 32], role: Role, (host, joiner): (EndpointId, EndpointId)) -> blake3::Hash {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(b"confirm");
    h.update(&[role as u8]);
    h.update(host.as_bytes());
    h.update(joiner.as_bytes());
    h.finalize()
}

/// `blake3::Hash` compares in constant time, so parse and compare as hashes.
fn matches(claimed: &str, expected: blake3::Hash) -> bool {
    blake3::Hash::from_hex(claimed).is_ok_and(|h| h == expected)
}

async fn say(send: &mut SendStream, line: &str) -> Result<()> {
    send.write_all(format!("{line}\n").as_bytes())
        .await
        .context("writing to peer")?;
    Ok(())
}

/// Back out with `no <why>`, and let that land before the caller drops the
/// connection under it: closing a connection throws away unsent data.
async fn refuse(send: &mut SendStream, why: &str) {
    if say(send, &format!("no {why}")).await.is_ok() && send.finish().is_ok() {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), send.stopped()).await;
    }
}

/// The next line, which has to be `verb`, returning whatever follows it. The
/// peer backing out with `no <why>` comes back as the error.
async fn hear(lines: &mut Reader, verb: &str) -> Result<String> {
    let line = lines
        .next_line()
        .await
        .context("reading from peer")?
        .context("peer hung up during the handshake")?;
    let (word, rest) = line.split_once(' ').unwrap_or((&line, ""));
    match word {
        w if w == verb => Ok(rest.to_string()),
        "no" if rest == "wrong-code" => Err(WrongCode.into()),
        "no" if rest == "unsupported-game" => Err(Declined.into()),
        "no" => bail!("peer refused: {rest}"),
        _ => bail!("expected {verb:?} from peer, got {word:?}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    // Checked up front so the byte slicing below cannot split a character.
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        bail!("bad hex from peer");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex from peer"))
        .collect()
}
