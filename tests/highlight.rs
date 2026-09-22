//! The marks on squares you can move to: a dot on an empty square, the corners
//! filled in on a capture. They are drawn into the cells over the square, so
//! it is worth checking they land where they should and nowhere else.

use ratatui::Terminal;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use shakmaty::Square;
use tui_tui::games::chess::app::App;
use tui_tui::games::chess::ui::{self, Geometry, PieceStyle};

const W: u16 = 120;
const H: u16 = 40;

fn render(app: &App) -> Buffer {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// The cell `(dx, dy)` into a square, as seen from white's side.
fn cell(buf: &Buffer, sq: Square, (dx, dy): (u16, u16)) -> Cell {
    let g = Geometry::new(Rect::new(0, 0, W, H));
    let (cw, ch) = g.cell;
    let col = u16::from(sq.file() as u8);
    let row = 7 - u16::from(sq.rank() as u8);
    buf[(g.grid.x + col * cw + dx, g.grid.y + row * ch + dy)].clone()
}

fn middle() -> (u16, u16) {
    let (cw, ch) = Geometry::new(Rect::new(0, 0, W, H)).cell;
    (cw / 2, ch / 2)
}

fn selecting(from: Square, moves: &[&str], style: PieceStyle) -> App {
    let mut app = App::local();
    app.piece_style = style;
    for m in moves {
        app.game.play_uci(m).unwrap();
    }
    app.game.cursor = from;
    app.game.activate(None);
    // Out of the way, so the cursor's own highlight does not get in.
    app.game.cursor = Square::H8;
    app
}

#[test]
fn an_empty_square_you_can_reach_gets_a_dot_in_the_middle() {
    for style in [PieceStyle::Octant, PieceStyle::Letter] {
        let buf = render(&selecting(Square::E2, &[], style));
        let dot = cell(&buf, Square::E4, middle());
        assert_ne!(dot.symbol(), " ", "{style:?}: no dot on e4");
        assert_ne!(dot.fg, dot.bg, "{style:?}: the dot must show");
        assert_eq!(
            cell(&buf, Square::E4, (0, 0)).symbol(),
            " ",
            "{style:?}: a dot stays off the edges"
        );
        assert_eq!(cell(&buf, Square::E5, middle()).symbol(), " ", "{style:?}");
    }
}

#[test]
fn half_blocks_stand_in_where_octants_may_not_draw() {
    let buf = render(&selecting(Square::E2, &[], PieceStyle::Letter));
    let dot = cell(&buf, Square::E4, middle());
    assert!(
        ["▀", "▄", "█"].contains(&dot.symbol()),
        "{:?}",
        dot.symbol()
    );
}

#[test]
fn a_capture_fills_the_corners_and_leaves_the_piece() {
    let mut app = selecting(Square::E4, &["e2e4", "d7d5"], PieceStyle::Octant);
    let buf = render(&app);
    app.game.selected = None;
    let untouched = render(&app);

    let (cw, ch) = Geometry::new(Rect::new(0, 0, W, H)).cell;
    for corner in [(0, 0), (cw - 1, 0), (0, ch - 1), (cw - 1, ch - 1)] {
        assert_ne!(cell(&buf, Square::D5, corner).symbol(), " ", "{corner:?}");
    }
    assert_eq!(
        cell(&buf, Square::D5, middle()),
        cell(&untouched, Square::D5, middle()),
        "the captured piece is drawn as before"
    );
}

#[test]
fn marking_a_square_leaves_its_colour_alone() {
    let plain = render(&App::local());
    let buf = render(&selecting(Square::E2, &[], PieceStyle::Octant));
    for sq in [Square::E3, Square::E4] {
        assert_eq!(
            cell(&buf, sq, (0, 0)).bg,
            cell(&plain, sq, (0, 0)).bg,
            "{sq}"
        );
    }
}
