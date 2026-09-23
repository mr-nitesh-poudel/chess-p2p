//! Drawing a game of chess: the board and its pieces (`board`, `pieces`),
//! the panels around it (`panels`), and the geometry that mouse clicks are
//! tested against.
//!
//! What is drawn and what can be clicked both come from [`Geometry`], so the
//! two cannot drift apart.

mod board;
mod panels;
mod pieces;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::*;

use shakmaty::{Color as Side, File, Rank, Square};

use board::{draw_board, draw_slide};
use panels::{draw_footer, draw_promotion, draw_sidebar, draw_tray, draw_verdict};

use super::app::App;
use super::rules::PROMOTION_ROLES;
use crate::games::Ctx;
use crate::ui::centred;

pub use board::canvas_colour;
pub use pieces::piece_ink;

const LIGHT: Color = Color::Rgb(214, 194, 162);
const DARK: Color = Color::Rgb(137, 99, 73);
const LIGHT_LAST: Color = Color::Rgb(206, 204, 122);
const DARK_LAST: Color = Color::Rgb(163, 150, 71);
const PIECE_WHITE: Color = Color::Rgb(252, 250, 245);
const PIECE_BLACK: Color = Color::Rgb(20, 18, 16);
/// How far the dot on a square you can move to darkens the square beneath.
const MARK_SHADE: f32 = 0.22;
/// A king in check or mated, and the verdict.
const MATE_RED: Color = Color::Rgb(222, 52, 44);
const NIGHT: Color = Color::Rgb(0, 0, 0);
const FLASH: Color = Color::Rgb(255, 255, 255);
const VERDICT_BG: Color = Color::Rgb(22, 16, 14);

/// How a piece is drawn. Each falls back to the next when a square is too
/// small to carry it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceStyle {
    /// Silhouettes drawn on a [`Canvas`](ratatui::widgets::canvas::Canvas)
    /// in octants: eight solid dots to a cell. See [`super::canvas`].
    Octant,
    /// The same silhouettes in braille, for terminals without octants.
    Braille,
    /// Half-block sprites. Every cell drawn as `▀` holds two stacked pixels —
    /// its foreground on top, its background below — which buys the vertical
    /// resolution a recognisable piece needs.
    Blocks,
    /// The piece's letter as a 5x5 bitmap, blown up through the same half
    /// blocks the sprites use, with an outline grown around it.
    BigLetter,
    /// Three-row line art. Needs a tall square; falls back on its own.
    Art,
    /// A single chess figurine, ♞.
    Figurine,
    /// A single letter, for terminals that render figurines double-width.
    Letter,
}

impl PieceStyle {
    pub fn next(self) -> Self {
        match self {
            PieceStyle::Blocks => PieceStyle::BigLetter,
            PieceStyle::BigLetter => PieceStyle::Art,
            PieceStyle::Art => PieceStyle::Figurine,
            PieceStyle::Figurine => PieceStyle::Letter,
            PieceStyle::Letter => PieceStyle::Octant,
            PieceStyle::Octant => PieceStyle::Braille,
            PieceStyle::Braille => PieceStyle::Blocks,
        }
    }
}

/// Square sizes we are willing to draw, smallest first. Widths are odd so a
/// single glyph sits dead centre.
const CELL_SIZES: [(u16, u16); 5] = [(3, 1), (5, 2), (7, 3), (9, 4), (11, 5)];
/// Rank digit plus a space, down the left of the board.
const GUTTER: u16 = 2;
const SIDEBAR_MIN: u16 = 24;
/// Past this the sidebar stops growing, and the room left over goes to
/// either side of the board and sidebar instead.
const SIDEBAR_MAX: u16 = 40;
/// The fewest rows the sidebar needs: the status panel and a few moves.
const SIDEBAR_MIN_H: u16 = 15;

/// Where everything sits this frame.
pub struct Geometry {
    pub board: Rect,
    /// The 8x8 playing area, inside the border and to the right of the gutter.
    pub grid: Rect,
    pub cell: (u16, u16),
    pub top_tray: Rect,
    pub bottom_tray: Rect,
    pub sidebar: Rect,
    pub footer: Rect,
    pub promo: Rect,
    pub promo_cell: u16,
}

impl Geometry {
    /// Picks the biggest board that leaves room for the sidebar, and centres
    /// it on the screen. The sidebar sits to its right, or pushes it left of
    /// centre where there is not room for both.
    pub fn new(area: Rect) -> Self {
        let [main, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

        let mut cell = CELL_SIZES[0];
        for candidate in CELL_SIZES {
            let (w, h) = (block_w(candidate.0), block_h(candidate.1));
            if w + SIDEBAR_MIN <= main.width && h + 2 <= main.height {
                cell = candidate;
            }
        }
        let (cw, ch) = cell;

        // The board with a tray above and below it.
        let (col_w, col_h) = (
            block_w(cw).min(main.width),
            (block_h(ch) + 2).min(main.height),
        );
        let spare = main.width - col_w;
        let side_w = spare.min(SIDEBAR_MAX);
        let left = Rect {
            x: main.x + (spare / 2).min(spare - side_w),
            y: main.y + (main.height - col_h) / 2,
            width: col_w,
            height: col_h,
        };
        let side_h = col_h.max(SIDEBAR_MIN_H).min(main.height);
        let sidebar = Rect {
            x: left.right(),
            y: main.y + (main.height - side_h) / 2,
            width: side_w,
            height: side_h,
        };
        let [top_tray, board, bottom_tray] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(block_h(ch)),
            Constraint::Length(1),
        ])
        .areas(left);

        let grid = Rect {
            x: board.x + 1 + GUTTER,
            y: board.y + 1,
            width: 8 * cw,
            height: 8 * ch,
        };

        // The prompt shows four pieces at roughly board scale.
        let promo_cell = cw.clamp(5, 9);
        let promo = centred(board, 4 * promo_cell + 2, ch.clamp(3, 4) + 2);

        Self {
            board,
            grid,
            cell,
            top_tray,
            bottom_tray,
            sidebar,
            footer,
            promo,
            promo_cell,
        }
    }

    /// The square under a screen position, if any.
    pub fn square_at(&self, x: u16, y: u16, flipped: bool) -> Option<Square> {
        let (cw, ch) = self.cell;
        let col = x.checked_sub(self.grid.x)? / cw;
        let row = y.checked_sub(self.grid.y)? / ch;
        if col > 7 || row > 7 {
            return None;
        }
        let file = if flipped { 7 - col } else { col };
        let rank = if flipped { row } else { 7 - row };
        Some(Square::from_coords(
            File::new(file.into()),
            Rank::new(rank.into()),
        ))
    }

    /// The promotion choice under a screen position, if any.
    pub fn promo_at(&self, x: u16, y: u16) -> Option<usize> {
        if y <= self.promo.y || y + 1 >= self.promo.bottom() {
            return None;
        }
        let index = (x.checked_sub(self.promo.x + 1)? / self.promo_cell) as usize;
        (index < PROMOTION_ROLES.len()).then_some(index)
    }
}

fn block_w(cell_w: u16) -> u16 {
    8 * cell_w + GUTTER + 2
}

/// Eight ranks, the file labels, and the border.
fn block_h(cell_h: u16) -> u16 {
    8 * cell_h + 1 + 2
}

pub fn draw(f: &mut Frame, app: &App, ctx: &Ctx) {
    let g = Geometry::new(f.area());

    let bottom_side = if app.flipped {
        Side::Black
    } else {
        Side::White
    };
    draw_tray(f, g.top_tray, app, !bottom_side);
    draw_board(f, &g, app);
    if let Some((slide, t)) = app.slide_at() {
        draw_slide(f.buffer_mut(), &g, app, slide, t);
    }
    draw_tray(f, g.bottom_tray, app, bottom_side);
    draw_sidebar(f, g.sidebar, app, ctx);
    draw_footer(f, g.footer, app, ctx);

    if app.game.promotion.is_some() {
        draw_promotion(f, &g, app);
    }
    if let Some(fin) = app.finale()
        && let Some(reveal) = fin.banner
    {
        draw_verdict(f, &g, app, ctx, &fin, reveal);
    }
}

/// Pads `s` to `width` columns with the content centred.
fn centre(s: &str, width: u16) -> String {
    let width = usize::from(width);
    let len = s.chars().count();
    if len >= width {
        return s.chars().take(width).collect();
    }
    let left = (width - len) / 2;
    format!(
        "{}{}{}",
        " ".repeat(left),
        s,
        " ".repeat(width - len - left)
    )
}

fn side_name(c: Side) -> &'static str {
    if c == Side::White { "white" } else { "black" }
}
