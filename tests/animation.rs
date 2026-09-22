//! The sliding piece, checked by reading frames out of a rendered buffer.
//! The clock is pinned so each frame is exact rather than a race. Each check
//! runs for every style that animates: octants, braille, and the half-block
//! sprites.

use std::time::Instant;

use chess_p2p::app::{App, CHECK_PULSE, Ending, FINALE, RESIGN_FINALE, SHUDDER, SLIDE, TOPPLE};
use chess_p2p::canvas::Dots;
use chess_p2p::ui::{self, Geometry, PieceStyle, canvas_colour, piece_ink};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use shakmaty::{Color, Position, Square};

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

/// Fool's mate, with the queen's move played through `App` so it slides in.
fn fools_mate() -> (App, Instant) {
    let mut app = App::local();
    for m in ["f2f3", "e7e5", "g2g4"] {
        app.game.play_uci(m).unwrap();
    }
    let start = Instant::now();
    app.clock = Some(start);
    app.game.cursor = Square::D8;
    app.game.activate(None);
    app.game.cursor = Square::H4;
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    (app, start)
}

fn screen_text(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
        .collect()
}

#[test]
fn checkmate_plays_out_once_the_mating_piece_lands() {
    let (mut app, start) = fools_mate();
    let land = start + SLIDE;
    assert!(
        app.finale().is_none(),
        "not while the queen is still moving"
    );

    app.clock = Some(land);
    let fin = app
        .finale()
        .expect("the finale starts when the queen lands");
    assert_eq!(fin.king, Square::E1);
    assert_eq!(fin.checkers, vec![Square::H4]);
    assert_eq!(fin.banner, None, "the verdict waits for the king to fall");
    assert!(app.is_animating());

    app.clock = Some(land + SHUDDER + TOPPLE / 2);
    assert!(
        app.finale().unwrap().fallen.abs() > 0.0,
        "the king is going over"
    );

    app.clock = Some(land + FINALE);
    let fin = app.finale().unwrap();
    assert!(!app.is_animating(), "and then it all holds still");
    assert_eq!(fin.banner, Some(1.0));
    // The queen is to the king's right, so it falls to the left.
    assert!(
        (fin.fallen + 1.0).abs() < 1e-6,
        "flat on its side: {}",
        fin.fallen
    );
    assert!(screen_text(&app).contains("any key to see the board"));
}

#[test]
fn keys_skip_the_finale_then_clear_the_verdict() {
    let (mut app, start) = fools_mate();
    app.clock = Some(start + SLIDE);
    let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

    app.on_key(key('f'));
    assert!(!app.flipped, "the first key only skips ahead");
    assert!(!app.is_animating());
    assert!(screen_text(&app).contains("any key to see the board"));

    app.on_key(key('f'));
    assert!(!app.flipped, "the second clears the verdict");
    let fin = app.finale().unwrap();
    assert_eq!((fin.banner, fin.dim), (None, 0.0));
    assert!(!screen_text(&app).contains("any key to see the board"));
    assert!(fin.fallen != 0.0, "the king stays down");

    app.on_key(key('f'));
    assert!(app.flipped, "and after that keys work as usual");
}

#[test]
fn q_leaves_even_mid_finale() {
    let (mut app, start) = fools_mate();
    app.clock = Some(start + SLIDE);
    app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.quit);
}

#[test]
fn an_ordinary_move_has_no_finale() {
    let (mut app, start) = play(Square::E2, Square::E4, PieceStyle::Octant);
    app.clock = Some(start + FINALE);
    assert!(app.finale().is_none());
    assert!(app.ended.is_none());
}

#[test]
fn check_pulses_then_glows_until_it_is_answered() {
    let mut app = App::local();
    // 1. e4 f6 2. d4 g5, and then Qh5+ played through App.
    for m in ["e2e4", "f7f6", "d2d4", "g7g5"] {
        app.game.play_uci(m).unwrap();
    }
    let start = Instant::now();
    app.clock = Some(start);
    app.game.cursor = Square::D1;
    app.game.activate(None);
    app.game.cursor = Square::H5;
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.game.pos.is_checkmate(),
        "Qh5 is mate here, which is its own finale"
    );
    assert!(app.check_glow().is_none());

    // A plain check: 1. e4 f5 2. Qh5+.
    let mut app = App::local();
    for m in ["e2e4", "f7f5"] {
        app.game.play_uci(m).unwrap();
    }
    app.clock = Some(start);
    app.game.cursor = Square::D1;
    app.game.activate(None);
    app.game.cursor = Square::H5;
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.game.pos.is_check() && !app.game.pos.is_checkmate());
    assert!(app.check_glow().is_none(), "not until the queen arrives");

    let land = start + SLIDE;
    app.clock = Some(land + CHECK_PULSE / 4);
    let (king, peak) = app.check_glow().unwrap();
    assert_eq!(king, Square::E8);
    assert!(app.is_animating());

    app.clock = Some(land + CHECK_PULSE);
    let (_, steady) = app.check_glow().unwrap();
    assert!(peak > steady, "it pulses brighter than it settles");
    assert!(!app.is_animating(), "and the steady glow needs no redraws");
    assert!(app.finale().is_none());

    app.game.play_uci("g7g6").unwrap();
    assert!(app.check_glow().is_none(), "blocking the check puts it out");
}

#[test]
fn resigning_lays_the_king_down() {
    let mut app = App::local();
    let start = Instant::now();
    app.clock = Some(start);
    let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    app.on_key(key('r'));
    app.on_key(key('y'));

    let fin = app.finale().expect("resigning starts the finale at once");
    assert_eq!((fin.how, fin.king), (Ending::Resignation, Square::E1));
    assert_eq!((fin.impact, fin.shake), (0.0, 0.0), "quietly");
    assert!(fin.checkers.is_empty());

    app.clock = Some(start + RESIGN_FINALE);
    let fin = app.finale().unwrap();
    assert!(!app.is_animating());
    assert!((fin.fallen - 1.0).abs() < 1e-6, "laid flat: {}", fin.fallen);
    let text = screen_text(&app);
    assert!(text.contains("white resigns · black wins"), "{text}");
}

#[test]
fn reachable_squares_have_no_dots() {
    for style in [PieceStyle::Octant, PieceStyle::Letter] {
        let mut app = App::local();
        app.piece_style = style;
        app.game.cursor = Square::E2;
        app.game.activate(None);
        let text = screen_text(&app);
        assert!(!text.contains('●') && !text.contains('•'), "{style:?}");
    }
}
