//! The sliding piece, checked by reading frames out of a rendered buffer.
//! The clock is pinned so each frame is exact rather than a race. Each check
//! runs for every style that animates: octants, braille, and the half-block
//! sprites.

use std::time::Instant;

use chess_p2p::app::{App, SLIDE};
use chess_p2p::canvas::Dots;
use chess_p2p::ui::{self, Geometry, PieceStyle, canvas_colour, piece_ink};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use shakmaty::{Color, Square};

const W: u16 = 120;
const H: u16 = 40;

/// The styles whose pieces travel, rather than just arriving.
const ANIMATED: [PieceStyle; 3] = [PieceStyle::Octant, PieceStyle::Braille, PieceStyle::Blocks];

/// Whether any of `side`'s ink appears inside one square: a canvas dot, or a
/// half-block pixel in its colours.
fn occupied(app: &App, sq: Square, side: Color) -> bool {
    let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let g = Geometry::new(Rect::new(0, 0, W, H));
    let (cw, ch) = g.cell;
    let (fill, line) = piece_ink(PieceStyle::Blocks, side);
    let dotted = canvas_colour(side);
    let col = u16::from(sq.file() as u8);
    let row = 7 - u16::from(sq.rank() as u8);

    (0..cw).any(|x| {
        (0..ch).any(|y| {
            let cell = &buf[(g.grid.x + col * cw + x, g.grid.y + row * ch + y)];
            let has_dots = [Dots::Octant, Dots::Braille]
                .iter()
                .any(|d| d.decode(cell.symbol()).is_some_and(|bits| bits != 0));
            (cell.symbol() == "▀" && [fill, line].contains(&cell.fg))
                || (has_dots && cell.fg == dotted)
        })
    })
}

/// Plays one move through `App`, with the clock pinned to the move's start.
fn play(from: Square, to: Square, style: PieceStyle) -> (App, Instant) {
    let mut app = App::local();
    app.piece_style = style;
    let start = Instant::now();
    app.clock = Some(start);
    app.game.cursor = from;
    app.game.activate(None);
    app.game.cursor = to;
    // Go through App, so the slide is set up the way a real move would be.
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    (app, start)
}

#[test]
fn a_move_starts_a_slide_that_ends_on_its_own() {
    let (mut app, start) = play(Square::G1, Square::F3, PieceStyle::Octant);
    let (slide, t) = app.slide_at().expect("the knight should be travelling");
    assert_eq!((slide.from, slide.to), (Square::G1, Square::F3));
    assert_eq!(t, 0.0);

    app.clock = Some(start + SLIDE / 2);
    assert!(app.is_animating());

    app.clock = Some(start + SLIDE);
    assert!(!app.is_animating(), "the slide should be over");
}

#[test]
fn the_piece_leaves_one_square_and_arrives_at_the_other() {
    for style in ANIMATED {
        leaves_and_arrives(style);
    }
}

fn leaves_and_arrives(style: PieceStyle) {
    let (mut app, start) = play(Square::G1, Square::F3, style);

    // At the start it is still on g1, and f3 is empty despite the move.
    assert!(occupied(&app, Square::G1, Color::White));
    assert!(!occupied(&app, Square::F3, Color::White));

    // Once the slide is over the board is drawn normally again.
    app.clock = Some(start + SLIDE);
    assert!(!occupied(&app, Square::G1, Color::White));
    assert!(occupied(&app, Square::F3, Color::White));
}

#[test]
fn the_piece_passes_through_the_squares_between() {
    for style in ANIMATED {
        passes_between(style);
    }
}

fn passes_between(style: PieceStyle) {
    // e2-e4 travels over e3, which is empty, so anything drawn there is the
    // pawn in flight and nothing else.
    let (mut app, start) = play(Square::E2, Square::E4, style);
    let white = Color::White;

    assert!(
        !occupied(&app, Square::E3, white),
        "nothing on e3 at the start"
    );
    assert!(occupied(&app, Square::E2, white), "the pawn starts on e2");

    app.clock = Some(start + SLIDE / 2);
    assert!(
        occupied(&app, Square::E3, white),
        "halfway, the pawn is over e3"
    );
    assert!(!occupied(&app, Square::E2, white), "and has left e2");
    assert!(!occupied(&app, Square::E4, white), "without arriving yet");

    app.clock = Some(start + SLIDE);
    assert!(occupied(&app, Square::E4, white), "the pawn lands on e4");
    assert!(!occupied(&app, Square::E3, white), "and e3 is empty again");
}

#[test]
fn an_opponents_move_animates_too() {
    for style in ANIMATED {
        opponent_animates(style);
    }
}

fn opponent_animates(style: PieceStyle) {
    let mut app = App::local();
    app.piece_style = style;
    let start = Instant::now();
    app.clock = Some(start);
    app.game.play_uci("e2e4").unwrap();
    app.on_net(chess_p2p::net::NetEvent::Move("e7e5".into()));

    let (slide, _) = app.slide_at().expect("a received move should travel");
    assert_eq!((slide.from, slide.to), (Square::E7, Square::E5));
    assert!(occupied(&app, Square::E7, Color::Black));
    assert!(!occupied(&app, Square::E5, Color::Black));
}
