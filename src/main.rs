//! tui-tui — terminal games over a direct peer-to-peer connection.
//!
//!     tui-tui                     the lobby: pick a game, host, join, challenge a friend
//!     tui-tui [game] local        two players, one keyboard
//!     tui-tui [game] host         wait for an opponent, and go first
//!     tui-tui join <code>         join with an opponent's code
//!
//! Without a game named, the first there is; the lobby starts on whichever is.

mod hub;

use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use hub::{Hub, Term, Wake};
use tui_tui::games::Kind;
use tui_tui::lobby::Choice;
use tui_tui::profile::Profile;
use tui_tui::session;

/// The longest quitting waits for connections to close cleanly.
const SHUTDOWN: Duration = Duration::from_secs(2);

const USAGE: &str = "\
tui-tui — terminal games over iroh

usage:
  tui-tui                    open the lobby to pick a game, host, join, or
                             challenge a friend
  tui-tui [game] local       play locally, two players on one keyboard
  tui-tui [game] host        host a game and get a code to share
  tui-tui join <code>        join a game with the code your opponent sent,
                             e.g. tui-tui join 42-tiger-marble-ocean

games: {games}

Your profile (identity, name, friends) is kept in tui-tui under your
config directory, or in $TUI_TUI_HOME if that is set.
";

/// The usage text, with the games this build can play filled in.
fn usage() -> String {
    // The first is the one played when none is named.
    let games: Vec<String> = Kind::ALL
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let name = k.name().to_lowercase();
            if i == 0 {
                format!("{name} (default)")
            } else {
                name
            }
        })
        .collect();
    USAGE.replace("{games}", &games.join(", "))
}

/// A game named on the command line.
fn named_game(word: &str) -> Option<Kind> {
    Kind::ALL
        .iter()
        .copied()
        .find(|k| k.name().eq_ignore_ascii_case(word))
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let game = match args.first().and_then(|a| named_game(a)) {
        Some(game) => {
            args.remove(0);
            game
        }
        None => Kind::ALL[0],
    };
    // Settled before the terminal is taken over, so a bad code or a typo in
    // the command is reported on a normal screen.
    let start = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => None,
        ["local"] => Some(Choice::Local),
        ["host"] => Some(Choice::Host),
        // Spaces are fine too, as in `join 42 tiger marble ocean`.
        ["join", words @ ..] if !words.is_empty() => Some(Choice::Join(words.join("-").parse()?)),
        ["-h" | "--help" | "help"] => {
            print!("{}", usage());
            return Ok(());
        }
        _ => {
            eprint!("{}", usage());
            std::process::exit(2);
        }
    };

    let (profile, notice) = match Profile::load() {
        Ok(profile) => (profile, None),
        Err(e) => (Profile::guest(), Some(format!("playing as a guest: {e:#}"))),
    };
    let endpoint = session::bind(profile.secret.clone()).await?;

    let (wake, mut woken) = unbounded_channel();
    spawn_input(wake.clone());
    let mut hub = Hub::new(profile, endpoint.clone(), wake, notice);
    let mut term = Term::new();
    let result = hub.run(&mut term, &mut woken, game, start).await;
    term.restore();
    // Dropping the hub ends any game, which says goodbye to the peer on its
    // way out; closing the endpoint gives that a moment to be sent.
    drop(hub);
    let _ = tokio::time::timeout(SHUTDOWN, endpoint.close()).await;
    result
}

/// crossterm's reader is blocking, so it lives on its own thread and feeds the
/// async loop through the same channel as everything else.
fn spawn_input(wake: UnboundedSender<Wake>) {
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if wake.send(Wake::Input(ev)).is_err() {
                break;
            }
        }
    });
}
