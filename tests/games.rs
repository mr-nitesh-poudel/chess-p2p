//! The registry of games, and chess's messages.

use tui_tui::games::chess::protocol::Msg;
use tui_tui::games::chess::{self, App};
use tui_tui::games::{Conn, Kind, Leave, Play, Seat, Table};
use tui_tui::session::{self, Progress};

#[test]
fn every_game_is_known_by_what_it_says_on_the_wire() {
    for &kind in Kind::ALL {
        assert_eq!(Kind::from_wire(kind.wire()), Some(kind));
    }
    assert_eq!(Kind::all_wire().len(), Kind::ALL.len());
    let stranger = session::Game {
        name: "go",
        version: 1,
    };
    assert_eq!(Kind::from_wire(stranger), None);
    let newer = session::Game {
        version: chess::GAME.version + 1,
        ..chess::GAME
    };
    assert_eq!(Kind::from_wire(newer), None, "a version we cannot speak");
}

#[test]
fn the_seat_decides_the_side() {
    let host = App::new(Seat::Host, Table::local());
    let guest = App::new(Seat::Guest, Table::local());
    let local = App::new(Seat::Local, Table::local());
    assert_eq!(host.me, Some(shakmaty::Color::White));
    assert_eq!(guest.me, Some(shakmaty::Color::Black));
    assert!(guest.flipped, "black sits behind black's pieces");
    assert_eq!(local.me, None);
}

#[test]
fn a_started_game_is_the_kind_asked_for() {
    let game = Kind::Chess.start(Seat::Local, Table::local());
    assert_eq!(game.kind(), Kind::Chess);
    assert_eq!(game.leaving(), None);
}

#[test]
fn chess_messages_survive_the_trip() {
    for msg in [Msg::Move("e2e4".into()), Msg::Resign, Msg::Draw] {
        assert_eq!(Msg::parse(&msg.line()), Some(msg.clone()), "{msg:?}");
    }
    assert_eq!(
        Msg::parse("castle-sideways"),
        None,
        "unknown words are ignored"
    );
}

#[test]
fn progress_moves_the_connection_along() {
    let mut table = Table::new(Conn::Publishing, None);
    assert_eq!(table.on_progress(Progress::Listed, Kind::Chess), None);
    assert_eq!(table.conn, Conn::Waiting);
    let note = table.on_progress(Progress::Declined, Kind::Chess);
    assert_eq!(
        note.as_deref(),
        Some("someone joined who cannot play chess")
    );
}

#[test]
fn quitting_is_reported_as_leaving() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut game: Box<dyn Play> = Kind::Chess.start(Seat::Local, Table::local());
    let press = |g: &mut Box<dyn Play>, c| g.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    press(&mut game, KeyCode::Char('q'));
    press(&mut game, KeyCode::Char('y'));
    assert_eq!(game.leaving(), Some(Leave::Lobby));

    let mut game = Kind::Chess.start(Seat::Local, Table::local());
    game.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(game.leaving(), Some(Leave::Exit));
}
