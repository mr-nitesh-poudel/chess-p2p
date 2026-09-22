//! The wash over squares you can move to. It is mixed here rather than by the
//! terminal, which has no alpha channel, so it is worth checking it lands.

use chess_p2p::app::App;
use chess_p2p::ui::{self, Geometry};
use ratatui::Terminal;
use ratatui::layout::Rect;
use ratatui::style::Color;
use shakmaty::Square;

const W: u16 = 120;
const H: u16 = 40;

/// The square's own colour, read from a corner cell that no piece reaches.
fn square_colour(app: &App, sq: Square) -> (u16, u16, u16) {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let g = Geometry::new(Rect::new(0, 0, W, H));
    let (cw, ch) = g.cell;
    let col = u16::from(sq.file() as u8);
    let row = 7 - u16::from(sq.rank() as u8);

    match buf[(g.grid.x + col * cw, g.grid.y + row * ch)].bg {
        Color::Rgb(r, gr, b) => (r.into(), gr.into(), b.into()),
        other => panic!("expected a true colour on {sq}, got {other:?}"),
    }
}

/// How far the colour leans green, against its strongest other channel.
fn greenness(c: (u16, u16, u16)) -> i32 {
    i32::from(c.1 as i16) - i32::from(c.0.max(c.2) as i16)
}

#[test]
fn reachable_squares_turn_green() {
    let mut app = App::local();
    let plain_dark = square_colour(&app, Square::E3);
    let plain_light = square_colour(&app, Square::E4);

    app.game.cursor = Square::E2;
    app.game.activate(None);

    let lit_dark = square_colour(&app, Square::E3);
    let lit_light = square_colour(&app, Square::E4);

    assert!(greenness(lit_dark) > 12, "e3 is barely green: {lit_dark:?}");
    assert!(
        greenness(lit_light) > 12,
        "e4 is barely green: {lit_light:?}"
    );
    assert!(greenness(lit_dark) > greenness(plain_dark) + 10);
    assert!(greenness(lit_light) > greenness(plain_light) + 10);
}

#[test]
fn the_wash_lets_the_board_show_through() {
    let mut app = App::local();
    app.game.cursor = Square::E2;
    app.game.activate(None);

    // e3 is a dark square and e4 a light one. If the wash replaced the colour
    // outright they would come out identical.
    assert_ne!(
        square_colour(&app, Square::E3),
        square_colour(&app, Square::E4),
        "the wash should tint the square, not paint over it"
    );
}

#[test]
fn a_capture_is_marked_more_strongly_than_an_empty_square() {
    let mut app = App::local();
    for m in ["e2e4", "d7d5"] {
        app.game.play_uci(m).unwrap();
    }
    app.game.cursor = Square::E4;
    app.game.activate(None);

    // Both d5 and e5 are light squares, so the two washes are comparable.
    let capture = square_colour(&app, Square::D5);
    let quiet = square_colour(&app, Square::E5);
    assert!(
        greenness(capture) > greenness(quiet),
        "capture {capture:?} should read stronger than quiet move {quiet:?}"
    );
}

#[test]
fn the_cursor_and_the_wash_stack() {
    let mut app = App::local();
    app.game.cursor = Square::E4;

    // The same square, under the cursor both times: once reachable, once not.
    app.game.selected = None;
    let cursor_only = square_colour(&app, Square::E4);
    app.game.selected = Some(Square::E2);
    let cursor_on_target = square_colour(&app, Square::E4);

    assert_ne!(
        cursor_only, cursor_on_target,
        "a reachable square under the cursor should not look like any other"
    );
    assert!(
        greenness(cursor_on_target) > greenness(cursor_only) + 10,
        "the green should survive the cursor: {cursor_on_target:?} vs {cursor_only:?}"
    );
}
