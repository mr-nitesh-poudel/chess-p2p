//! The opponent is on the other end of a network connection, running code we
//! did not build. Nothing they send is taken on trust.

use shakmaty::{Color, Position};
use tui_tui::games::chess::App;
use tui_tui::games::{Seat, Table};
use tui_tui::net::NetEvent;

/// A networked game in which we play white.
fn as_white() -> Table<App> {
    Table::local(App::new(Seat::Host))
}

fn peer_says(app: &mut Table<App>, line: &str) {
    app.on_net(NetEvent::Line(line.into()));
}

#[test]
fn the_peer_cannot_move_our_pieces_on_our_turn() {
    let mut app = as_white();
    let before = app.play.game.pos.clone();
    // e2e4 is perfectly legal — for us. It is our turn, so it is not theirs
    // to play.
    peer_says(&mut app, "move e2e4");
    assert_eq!(app.play.game.pos, before, "the board must not change");
    assert_eq!(app.play.game.turn(), Color::White);
    assert!(
        app.ctx
            .note
            .as_deref()
            .is_some_and(|n| n.contains("out of turn"))
    );
}

#[test]
fn the_peer_moves_on_their_own_turn() {
    let mut app = as_white();
    app.play.game.play_uci("e2e4").unwrap();
    peer_says(&mut app, "move e7e5");
    assert_eq!(
        app.play.game.turn(),
        Color::White,
        "black's reply was played"
    );
}

#[test]
fn nonsense_and_illegal_moves_change_nothing() {
    let mut app = as_white();
    app.play.game.play_uci("e2e4").unwrap();
    let before = app.play.game.pos.clone();
    for line in [
        "move e7e2",
        "move zz99",
        "move",
        "",
        "resign please",
        "castle-sideways",
    ] {
        peer_says(&mut app, line);
        assert_eq!(app.play.game.pos, before, "{line:?}");
    }
    assert!(app.play.game.resigned.is_none());
}

#[test]
fn a_finished_game_takes_no_more_messages() {
    let mut app = as_white();
    for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        app.play.game.play_uci(m).unwrap();
    }
    assert!(app.play.game.pos.is_checkmate());
    // Black has already won. A resignation now would rewrite the result.
    peer_says(&mut app, "resign");
    assert!(app.play.game.resigned.is_none(), "checkmate stands");
    peer_says(&mut app, "draw");
    assert!(!app.play.draw_offered && !app.play.draw_agreed);
}

#[test]
fn hot_seat_ignores_the_network_entirely() {
    let mut app = Table::local(App::local());
    let before = app.play.game.pos.clone();
    peer_says(&mut app, "move e2e4");
    peer_says(&mut app, "resign");
    assert_eq!(app.play.game.pos, before);
    assert!(app.play.game.resigned.is_none());
}
