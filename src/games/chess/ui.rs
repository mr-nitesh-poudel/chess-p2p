//! Drawing a game of chess, plus the geometry that mouse clicks are tested
//! against. Both come from [`Geometry`], so what you see and what you can
//! click on cannot drift apart.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use shakmaty::{Color as Side, File, Piece, Rank, Role, Square};

use super::app::{App, Ending, Finale, Slide};
use super::canvas::{self, Dots};
use super::rules::PROMOTION_ROLES;
use crate::clipboard::Copied;
use crate::games::Conn;
use crate::ui::{CAPTURE, CURSOR, MUTED, SELECTED, blend, centred};

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
        if y <= self.promo.y || y >= self.promo.bottom() - 1 {
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

pub fn draw(f: &mut Frame, app: &App) {
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
    draw_sidebar(f, g.sidebar, app);
    draw_footer(f, g.footer, app);

    if app.game.promotion.is_some() {
        draw_promotion(f, &g, app);
    }
    if let Some(fin) = app.finale()
        && let Some(reveal) = fin.banner
    {
        draw_verdict(f, &g, app, &fin, reveal);
    }
}

fn draw_board(f: &mut Frame, g: &Geometry, app: &App) {
    let (cw, ch) = g.cell;
    let game = &app.game;
    let targets = game.targets();
    let style = app.piece_style;
    let mut lines = Vec::with_capacity(usize::from(8 * ch + 1));

    // Whatever is sliding is drawn on top afterwards, not in place.
    let travelling = app.slide_at().map(|(s, _)| s.to);
    let finale = app.finale();
    let check = app.check_glow();

    for row in 0..8u32 {
        let rank = if app.flipped { row } else { 7 - row };

        for sub in 0..ch {
            // The rank digit goes on the row the pieces' middles sit on.
            let label = if sub == ch / 2 {
                format!("{} ", Rank::new(rank).char())
            } else {
                " ".repeat(GUTTER.into())
            };
            let mut spans = vec![Span::styled(label, Style::default().fg(MUTED))];

            for col in 0..8u32 {
                let file = if app.flipped { 7 - col } else { col };
                let sq = Square::from_coords(File::new(file), Rank::new(rank));
                let piece = if travelling == Some(sq) {
                    None
                } else {
                    game.piece_at(sq)
                };
                let dark = (file + rank) % 2 == 0;
                let mut bg = match (dark, game.last.is_some_and(|(a, b)| a == sq || b == sq)) {
                    (true, false) => DARK,
                    (false, false) => LIGHT,
                    (true, true) => DARK_LAST,
                    (false, true) => LIGHT_LAST,
                };
                if game.selected == Some(sq) {
                    bg = blend(bg, SELECTED, 0.8);
                }
                if sq == game.cursor {
                    bg = blend(bg, CURSOR, 0.5);
                }
                if let Some((king, glow)) = check
                    && sq == king
                {
                    bg = blend(bg, MATE_RED, glow);
                }
                if let Some(fin) = &finale {
                    bg = finale_bg(fin, sq, bg);
                }

                match piece {
                    Some(p) => spans.extend(piece_cell(p, sub, cw, ch, style, bg)),
                    None => {
                        spans.push(Span::styled(" ".repeat(cw.into()), Style::default().bg(bg)))
                    }
                }
            }
            lines.push(Line::from(spans));
        }
    }

    let mut labels = vec![Span::raw(" ".repeat(GUTTER.into()))];
    for col in 0..8u32 {
        let file = if app.flipped { 7 - col } else { col };
        labels.push(Span::styled(
            centre(&File::new(file).char().to_string(), cw),
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::from(labels));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Line::from(" chess ").centered());
    f.render_widget(Paragraph::new(lines).block(block), g.board);

    if let Some(dots) = canvas_dots(style, cw, ch) {
        let mut pieces = Vec::new();
        for row in 0..8u16 {
            for col in 0..8u16 {
                let sq = screen_square(col, row, app.flipped);
                let Some(piece) = game.piece_at(sq).filter(|_| travelling != Some(sq)) else {
                    continue;
                };
                let mut drawn = canvas_piece(piece, (col, row), (0.0, 0.0), g.cell);
                if let Some(fin) = &finale {
                    if sq == fin.king {
                        drawn.at.0 += fin.shake;
                        drawn.fallen = fin.fallen;
                    } else if !fin.checkers.contains(&sq) {
                        // Everything but the king and its killers fades back
                        // with the board.
                        drawn.colour = blend(drawn.colour, NIGHT, fin.dim * 0.6);
                    }
                }
                pieces.push(drawn);
            }
        }
        canvas::stamp(f.buffer_mut(), g.grid, &pieces, dots);
    }

    let dots = (style == PieceStyle::Octant).then_some(Dots::Octant);
    for sq in targets {
        if travelling == Some(sq) {
            continue;
        }
        let capture = game.piece_at(sq).is_some();
        draw_mark(f.buffer_mut(), g, app.flipped, sq, capture, dots);
    }
}

/// Marks a square the selected piece can move to: a dot in the middle of an
/// empty square, or its corners filled in round a circle if there is a piece
/// there to capture. A ring would run through the piece, which fills its
/// square top to bottom; the corners are always free.
///
/// The mark is drawn in octants where the terminal has them and half blocks
/// where it may not, a shade darker than the square. It leaves alone any cell
/// a piece is already drawn in.
fn draw_mark(
    buf: &mut Buffer,
    g: &Geometry,
    flipped: bool,
    sq: Square,
    capture: bool,
    dots: Option<Dots>,
) {
    let (cw, ch) = g.cell;
    let (file, rank) = (sq.file() as u16, sq.rank() as u16);
    let (col, row) = if flipped {
        (7 - file, rank)
    } else {
        (file, 7 - rank)
    };
    let (sx, sy) = match dots {
        Some(_) => canvas::DOTS,
        None => (1, 2),
    };

    // Measured in cell widths, taking a cell to be twice as tall as it is
    // wide, so the mark comes out round rather than squashed.
    let (w, h) = (f64::from(cw), 2.0 * f64::from(ch));
    let size = w.min(h);
    let pixel = (1.0 / f64::from(sx)).max(2.0 / f64::from(sy));
    let inside = |x: f64, y: f64| {
        let d = (x - w / 2.0).hypot(y - h / 2.0);
        if capture {
            d >= 0.58 * size
        } else {
            d <= (0.18 * size).max(0.6 * pixel)
        }
    };

    for cy in 0..ch {
        for cx in 0..cw {
            let at = (g.grid.x + col * cw + cx, g.grid.y + row * ch + cy);
            if !buf.area.contains(at.into()) || buf[at].symbol() != " " {
                continue;
            }
            let mut bits = 0u8;
            for j in 0..sy {
                for i in 0..sx {
                    let x = f64::from(cx) + (f64::from(i) + 0.5) / f64::from(sx);
                    let y = 2.0 * (f64::from(cy) + (f64::from(j) + 0.5) / f64::from(sy));
                    if inside(x, y) {
                        bits |= 1 << (i + sx * j);
                    }
                }
            }
            if bits == 0 {
                continue;
            }
            let symbol = match dots {
                Some(dots) => dots.encode(bits),
                None => [' ', '▀', '▄', '█'][usize::from(bits)],
            };
            let cell = &mut buf[at];
            let shade = blend(cell.bg, NIGHT, MARK_SHADE);
            cell.set_char(symbol).set_fg(shade);
        }
    }
}

/// A square's background during the checkmate finale: the king's square
/// burns red, the checkers' squares glow with it, and the rest goes dark.
fn finale_bg(fin: &Finale, sq: Square, bg: Color) -> Color {
    let bg = if sq == fin.king {
        blend(bg, MATE_RED, fin.red)
    } else if fin.checkers.contains(&sq) {
        blend(bg, MATE_RED, 0.3)
    } else {
        blend(bg, NIGHT, fin.dim)
    };
    blend(bg, FLASH, fin.impact)
}

/// Which dots to draw the pieces in, if a canvas style is in use and the
/// square has room for one. Below 9x4 the detail goes (the king's cross
/// shrinks to a dot), so smaller squares get the half-block sprites instead.
fn canvas_dots(style: PieceStyle, cw: u16, ch: u16) -> Option<Dots> {
    let dots = match style {
        PieceStyle::Octant => Dots::Octant,
        PieceStyle::Braille => Dots::Braille,
        _ => return None,
    };
    (cw >= 9 && ch >= 4).then_some(dots)
}

/// The square at a column and row of the grid as it is drawn.
fn screen_square(col: u16, row: u16, flipped: bool) -> Square {
    let (col, row) = (u32::from(col), u32::from(row));
    let file = if flipped { 7 - col } else { col };
    let rank = if flipped { row } else { 7 - row };
    Square::from_coords(File::new(file), Rank::new(rank))
}

/// A canvas piece in the square at `(col, row)` of a grid of `cell`-sized
/// squares, nudged by `offset` dots.
fn canvas_piece(
    piece: Piece,
    (col, row): (u16, u16),
    offset: (f64, f64),
    cell: (u16, u16),
) -> canvas::Piece {
    let square = (cell.0 * canvas::DOTS.0, cell.1 * canvas::DOTS.1);
    canvas::Piece {
        role: piece.role,
        colour: canvas_colour(piece.color),
        at: (
            f64::from(col * square.0) + offset.0,
            f64::from(row * square.1) + offset.1,
        ),
        square,
        fallen: 0.0,
    }
}

/// The colour a side's canvas pieces are drawn in. Public so a test can
/// read the pieces back out of a rendered buffer.
pub fn canvas_colour(side: Side) -> Color {
    match side {
        Side::White => Color::Rgb(255, 255, 255),
        Side::Black => Color::Rgb(20, 18, 16),
    }
}

/// Draws the travelling piece over the board it was already drawn onto.
///
/// Working straight on the buffer keeps the piece free of the square grid, so
/// it can sit halfway between two squares.
fn draw_slide(buf: &mut Buffer, g: &Geometry, app: &App, slide: &Slide, t: f32) {
    let (cw, ch) = g.cell;
    // Ease in and out, so the piece does not start and stop abruptly.
    let e = t * t * (3.0 - 2.0 * t);

    if let Some(dots) = canvas_dots(app.piece_style, cw, ch) {
        // Canvas pieces move a dot at a time, and may stop between dots.
        let place = |sq: Square| {
            let (file, rank) = (sq.file() as u16, sq.rank() as u16);
            if app.flipped {
                (7 - file, rank)
            } else {
                (file, 7 - rank)
            }
        };
        let (from, to) = (place(slide.from), place(slide.to));
        let square = (cw * canvas::DOTS.0, ch * canvas::DOTS.1);
        let offset = (
            f64::from(e) * (f64::from(to.0) - f64::from(from.0)) * f64::from(square.0),
            f64::from(e) * (f64::from(to.1) - f64::from(from.1)) * f64::from(square.1),
        );
        let piece = canvas_piece(slide.piece, from, offset, g.cell);
        canvas::stamp(buf, g.grid, &[piece], dots);
        return;
    }

    // Character styles have nothing to interpolate; they just arrive.
    let Some(sprite) = sprite_for(app.piece_style, cw, ch, slide.piece.role) else {
        return;
    };
    let (x0, y0) = sprite_origin(&sprite, cw, ch);

    // A square's sprite origin, in pixels from the grid's top-left corner.
    let origin = |sq: Square| {
        let (file, rank) = (i32::from(sq.file() as u8), i32::from(sq.rank() as u8));
        let (col, row) = if app.flipped {
            (7 - file, rank)
        } else {
            (file, 7 - rank)
        };
        (
            col * i32::from(cw) + i32::from(x0),
            row * 2 * i32::from(ch) + i32::from(y0),
        )
    };
    let (fx, fy) = origin(slide.from);
    let (tx, ty) = origin(slide.to);

    let lerp = |a: i32, b: i32| a + (((b - a) as f32) * e).round() as i32;
    let (x, y) = (lerp(fx, tx), lerp(fy, ty));

    let ink = ink(app.piece_style, slide.piece.color);
    let colour = |p: Pixel| match p {
        Pixel::Line => ink.line,
        Pixel::Fill => ink.fill,
    };

    let rows = i32::from(sprite.height());
    for cy in y.div_euclid(2)..=(y + rows - 1).div_euclid(2) {
        if cy < 0 || cy >= i32::from(g.grid.height) {
            continue;
        }
        for cx in x..x + i32::from(sprite.width()) {
            if cx < 0 || cx >= i32::from(g.grid.width) {
                continue;
            }
            let sx = (cx - x) as u16;
            let pixel = |py: i32| u16::try_from(py - y).ok().and_then(|sy| sprite.at(sx, sy));
            let (top, bottom) = (pixel(cy * 2), pixel(cy * 2 + 1));
            if top.is_none() && bottom.is_none() {
                continue;
            }

            // Keep whatever the board already put behind the transparent half.
            let pos = (g.grid.x + cx as u16, g.grid.y + cy as u16);
            let cell = &buf[pos];
            let (was_top, was_bottom) = if cell.symbol() == "▀" {
                (cell.fg, cell.bg)
            } else {
                (cell.bg, cell.bg)
            };
            let fg = top.map_or(was_top, colour);
            let bg = bottom.map_or(was_bottom, colour);
            buf[pos].set_symbol("▀").set_fg(fg).set_bg(bg);
        }
    }
}

/// The rows of text that make up a piece, given how much room the square has.
fn piece_rows(role: Role, style: PieceStyle, cell_h: u16) -> Vec<String> {
    if style == PieceStyle::Art && cell_h >= 3 {
        return art(role).iter().map(|s| s.to_string()).collect();
    }
    let single = match style {
        PieceStyle::Letter | PieceStyle::BigLetter => letter(role),
        // Art that cannot fit falls back to a figurine rather than vanishing.
        _ => figurine(role),
    };
    vec![single.to_string()]
}

/// A pixel of a sprite: `#` is the outline, `o` the body, anything else lets
/// the square show through.
enum Pixel {
    Line,
    Fill,
}

struct Sprite {
    rows: &'static [&'static str],
    /// Grows a one-pixel border in the outline colour around whatever is
    /// drawn. Lets a thin letterform read on any square without anyone having
    /// to hand-draw its border.
    outline: bool,
}

impl Sprite {
    const fn solid(rows: &'static [&'static str]) -> Self {
        Self {
            rows,
            outline: false,
        }
    }

    const fn outlined(rows: &'static [&'static str]) -> Self {
        Self {
            rows,
            outline: true,
        }
    }

    fn pad(&self) -> u16 {
        u16::from(self.outline)
    }

    fn width(&self) -> u16 {
        self.rows[0].len() as u16 + 2 * self.pad()
    }

    fn height(&self) -> u16 {
        self.rows.len() as u16 + 2 * self.pad()
    }

    /// The glyph as written, before any outline is grown around it.
    fn raw(&self, x: i32, y: i32) -> Option<Pixel> {
        let row = usize::try_from(y).ok().and_then(|y| self.rows.get(y))?;
        match row.as_bytes().get(usize::try_from(x).ok()?)? {
            b'#' => Some(Pixel::Line),
            b'o' => Some(Pixel::Fill),
            _ => None,
        }
    }

    /// Out-of-range coordinates read as transparent, which is what lets a
    /// sprite be dropped into a larger square without any bounds juggling.
    fn at(&self, x: u16, y: u16) -> Option<Pixel> {
        let pad = i32::from(self.pad());
        let (gx, gy) = (i32::from(x) - pad, i32::from(y) - pad);
        if let Some(pixel) = self.raw(gx, gy) {
            return Some(pixel);
        }
        if !self.outline {
            return None;
        }
        let touching = (-1..=1).any(|dy| (-1..=1).any(|dx| self.raw(gx + dx, gy + dy).is_some()));
        touching.then_some(Pixel::Line)
    }
}

/// The colours one side's pieces are drawn in. Both sides are outlined in
/// near-black; black's body is lifted off it far enough that the outline still
/// reads against the body, and stays dark enough to tell from white at a
/// glance.
struct Ink {
    fill: Color,
    line: Color,
}

const WHITE_INK: Ink = Ink {
    fill: Color::Rgb(242, 239, 232),
    line: Color::Rgb(46, 40, 35),
};
const BLACK_INK: Ink = Ink {
    fill: Color::Rgb(68, 60, 54),
    line: Color::Rgb(14, 12, 11),
};

// A letter is all thin strokes, and the grown outline closes up its counters,
// so the body has to carry the contrast against the outline rather than
// against the square. Black's stroke is lifted well clear of the near-black
// border for that, and still reads as the dark side next to white's.
const WHITE_LETTER_INK: Ink = Ink {
    fill: Color::Rgb(245, 242, 236),
    line: Color::Rgb(18, 16, 14),
};
const BLACK_LETTER_INK: Ink = Ink {
    fill: Color::Rgb(128, 115, 103),
    line: Color::Rgb(18, 16, 14),
};

fn ink(style: PieceStyle, side: Side) -> &'static Ink {
    match (style, side) {
        (PieceStyle::BigLetter, Side::White) => &WHITE_LETTER_INK,
        (PieceStyle::BigLetter, _) => &BLACK_LETTER_INK,
        (_, Side::White) => &WHITE_INK,
        _ => &BLACK_INK,
    }
}

/// The `(body, outline)` colours a side's pieces are drawn in. Public so a
/// test can read sprites back out of a rendered buffer.
pub fn piece_ink(style: PieceStyle, side: Side) -> (Color, Color) {
    let ink = ink(style, side);
    (ink.fill, ink.line)
}

/// Nine by eight, drawn inside the 11x10 pixels of an 11x5 square.
fn sprite_big(role: Role) -> Sprite {
    Sprite::solid(match role {
        Role::King => &[
            "....o....",
            "...ooo...",
            "....o....",
            "..#ooo#..",
            ".#ooooo#.",
            "..#ooo#..",
            ".#ooooo#.",
            ".#######.",
        ],
        Role::Queen => &[
            "o.o.o.o.o",
            "#ooooooo#",
            ".#ooooo#.",
            "..#ooo#..",
            "..#ooo#..",
            ".#ooooo#.",
            "#ooooooo#",
            ".#######.",
        ],
        Role::Rook => &[
            ".o.o.o.o.",
            ".#######.",
            ".#ooooo#.",
            "..#ooo#..",
            "..#ooo#..",
            ".#ooooo#.",
            ".#ooooo#.",
            ".#######.",
        ],
        Role::Bishop => &[
            "....o....",
            "...#o#...",
            "..#ooo#..",
            "..#o#o#..",
            "..#ooo#..",
            ".#ooooo#.",
            ".#ooooo#.",
            ".#######.",
        ],
        Role::Knight => &[
            ".....oo..",
            "...#oooo.",
            "..#ooooo#",
            ".#oooooo#",
            "#o#ooooo#",
            "##.#oooo#",
            "...#oooo#",
            ".#######.",
        ],
        Role::Pawn => &[
            ".........",
            "...###...",
            "..#ooo#..",
            "...#o#...",
            "..#ooo#..",
            ".#ooooo#.",
            ".#ooooo#.",
            ".#######.",
        ],
    })
}

/// Seven by seven, drawn inside the 9x8 pixels of a 9x4 square.
fn sprite_mid(role: Role) -> Sprite {
    Sprite::solid(match role {
        Role::King => &[
            "...o...", "..ooo..", "...o...", ".#ooo#.", ".#ooo#.", "#ooooo#", "#######",
        ],
        Role::Queen => &[
            "o.o.o.o", "#ooooo#", ".#ooo#.", ".#ooo#.", ".#ooo#.", "#ooooo#", "#######",
        ],
        Role::Rook => &[
            ".o.o.o.", ".#####.", ".#ooo#.", "..#o#..", "..#o#..", "#ooooo#", "#######",
        ],
        Role::Bishop => &[
            "...o...", "..#o#..", ".#ooo#.", ".#o#o#.", ".#ooo#.", "#ooooo#", "#######",
        ],
        Role::Knight => &[
            "...oo..", ".#oooo#", "#ooooo#", "#o#ooo#", "##.#oo#", "..#ooo#", "#######",
        ],
        Role::Pawn => &[
            "..###..", ".#ooo#.", "..#o#..", ".#ooo#.", ".#ooo#.", "#ooooo#", "#######",
        ],
    })
}

/// Five by five, drawn inside the 7x6 pixels of a 7x3 square.
fn sprite_tiny(role: Role) -> Sprite {
    Sprite::solid(match role {
        Role::King => &["..o..", ".ooo.", "..o..", "#ooo#", "#####"],
        Role::Queen => &["o.o.o", "#ooo#", ".#o#.", "#ooo#", "#####"],
        Role::Rook => &["o.o.o", ".###.", ".#o#.", "#ooo#", "#####"],
        Role::Bishop => &["..o..", ".#o#.", "#o#o#", "#ooo#", "#####"],
        Role::Knight => &["..oo.", ".#oo#", "#ooo#", "##oo#", "#####"],
        Role::Pawn => &[".###.", "#ooo#", ".#o#.", "#ooo#", "#####"],
    })
}

/// The piece's letter as a 5x5 bitmap. An outline is grown around it at draw
/// time, so it occupies 7x7 pixels and reads on either square colour.
fn font(role: Role) -> Sprite {
    Sprite::outlined(match role {
        Role::King => &["o...o", "o..o.", "ooo..", "o..o.", "o...o"],
        Role::Queen => &[".ooo.", "o...o", "o...o", "o..o.", ".oo.o"],
        Role::Rook => &["oooo.", "o...o", "oooo.", "o..o.", "o...o"],
        Role::Bishop => &["oooo.", "o...o", "oooo.", "o...o", "oooo."],
        Role::Knight => &["o...o", "oo..o", "o.o.o", "o..oo", "o...o"],
        Role::Pawn => &["oooo.", "o...o", "oooo.", "o....", "o...."],
    })
}

/// The biggest sprite this square can hold with a margin left around it, if a
/// pixel style was asked for.
fn sprite_for(style: PieceStyle, cw: u16, ch: u16, role: Role) -> Option<Sprite> {
    if style == PieceStyle::BigLetter {
        // 7x7 once outlined, so it needs the same room as the mid sprite.
        return (cw >= 9 && ch >= 4).then(|| font(role));
    }
    // The canvas styles hand small squares over to the sprites.
    if !matches!(
        style,
        PieceStyle::Blocks | PieceStyle::Octant | PieceStyle::Braille
    ) {
        return None;
    }
    match (cw, ch) {
        (w, h) if w >= 11 && h >= 5 => Some(sprite_big(role)),
        (w, h) if w >= 9 && h >= 4 => Some(sprite_mid(role)),
        (w, h) if w >= 7 && h >= 3 => Some(sprite_tiny(role)),
        _ => None,
    }
}

/// Where a sprite sits inside its square, in pixels from the square's corner.
/// Centred across, and sunk towards the bottom so the piece stands on the
/// square rather than floating in the middle of it.
fn sprite_origin(sprite: &Sprite, cw: u16, ch: u16) -> (u16, u16) {
    (
        (cw - sprite.width()) / 2,
        (2 * ch - sprite.height()).div_ceil(2),
    )
}

/// One square's worth of one piece, on one row of the board.
///
/// Always returns exactly `cw` columns, so callers can lay squares out
/// side by side without measuring.
fn piece_cell(
    piece: Piece,
    sub: u16,
    cw: u16,
    ch: u16,
    style: PieceStyle,
    bg: Color,
) -> Vec<Span<'static>> {
    // Canvas pieces are stamped over the board once it is drawn.
    if canvas_dots(style, cw, ch).is_some() {
        return vec![Span::styled(" ".repeat(cw.into()), Style::default().bg(bg))];
    }
    let ink = ink(style, piece.color);

    if let Some(sprite) = sprite_for(style, cw, ch, piece.role) {
        let (x0, y0) = sprite_origin(&sprite, cw, ch);
        let colour = |p: Option<Pixel>| match p {
            Some(Pixel::Line) => ink.line,
            Some(Pixel::Fill) => ink.fill,
            None => bg,
        };

        return (0..cw)
            .map(|x| {
                let sx = x.wrapping_sub(x0);
                let top = sprite.at(sx, (2 * sub).wrapping_sub(y0));
                let bottom = sprite.at(sx, (2 * sub + 1).wrapping_sub(y0));
                if top.is_none() && bottom.is_none() {
                    Span::styled(" ", Style::default().bg(bg))
                } else {
                    // The upper half block paints the top pixel in the
                    // foreground and leaves the bottom one as background.
                    Span::styled("▀", Style::default().fg(colour(top)).bg(colour(bottom)))
                }
            })
            .collect();
    }

    let rows = piece_rows(piece.role, style, ch);
    let top = (ch - rows.len() as u16).div_ceil(2);
    let content = match sub.checked_sub(top) {
        Some(i) if (i as usize) < rows.len() => centre(&rows[i as usize], cw),
        _ => " ".repeat(cw.into()),
    };
    let mut cell = Style::default().fg(ink.fill).bg(bg);
    if piece.color == Side::White {
        cell = cell.add_modifier(Modifier::BOLD);
    }
    vec![Span::styled(content, cell)]
}

fn art(role: Role) -> [&'static str; 3] {
    match role {
        Role::King => ["\\+/", "(K)", "/_\\"],
        Role::Queen => ["\\o/", "(Q)", "/_\\"],
        Role::Rook => ["|-|", "(R)", "/_\\"],
        Role::Bishop => [".^.", "(B)", "/_\\"],
        Role::Knight => ["/^)", "(N)", "/_\\"],
        Role::Pawn => [" o ", "(P)", "/_\\"],
    }
}

fn figurine(role: Role) -> char {
    match role {
        Role::King => '♚',
        Role::Queen => '♛',
        Role::Rook => '♜',
        Role::Bishop => '♝',
        Role::Knight => '♞',
        Role::Pawn => '♟',
    }
}

fn letter(role: Role) -> char {
    match role {
        Role::King => 'K',
        Role::Queen => 'Q',
        Role::Rook => 'R',
        Role::Bishop => 'B',
        Role::Knight => 'N',
        Role::Pawn => 'P',
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

/// The pieces `side` has captured, plus their material edge if they have one.
fn draw_tray(f: &mut Frame, area: Rect, app: &App, side: Side) {
    let taken = app.game.captured(!side);
    let style = match app.piece_style {
        PieceStyle::Letter | PieceStyle::BigLetter => PieceStyle::Letter,
        _ => PieceStyle::Figurine,
    };
    let mut text: String = taken
        .iter()
        .map(|r| piece_rows(*r, style, 1).remove(0))
        .collect();

    let edge = app.game.material_edge();
    let ahead = if side == Side::White { edge } else { -edge };
    if ahead > 0 {
        text.push_str(&format!("  +{ahead}"));
    }

    // The tray holds the opponent's pieces, so it takes the opponent's colour.
    let fg = if side == Side::White {
        PIECE_BLACK
    } else {
        PIECE_WHITE
    };
    let line = Line::from(vec![
        Span::raw(" ".repeat(GUTTER.into())),
        Span::styled(text, Style::default().fg(fg)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let [status, moves] =
        Layout::vertical([Constraint::Length(12), Constraint::Min(3)]).areas(area);

    let mut lines = Vec::new();
    match app.me {
        Some(c) => lines.push(Line::from(vec![
            Span::styled("you  ", Style::default().fg(MUTED)),
            Span::raw(side_name(c)),
        ])),
        None => lines.push(Line::from(vec![
            Span::styled("mode ", Style::default().fg(MUTED)),
            Span::raw("hot-seat"),
        ])),
    }

    lines.push(Line::from(vec![
        Span::styled("turn ", Style::default().fg(MUTED)),
        Span::raw(side_name(app.game.turn())),
    ]));

    let (state, style) = app.state_line();
    lines.push(Line::from(vec![
        Span::raw("     "),
        Span::styled(state, style),
    ]));

    lines.push(Line::raw(""));
    match &app.table.conn {
        Conn::Local => {}
        Conn::Publishing | Conn::Waiting => {
            lines.push(Line::styled("share this code:", Style::default().fg(MUTED)));
            lines.push(Line::styled(
                app.table.share.clone().unwrap_or_default(),
                Style::default().fg(CURSOR),
            ));
            match app.table.copied {
                Some(Copied::Clipboard) => {
                    lines.push(Line::styled("copied ✓", Style::default().fg(SELECTED)));
                }
                // Nothing reports back whether the terminal did it, so say
                // what to do if it did not.
                Some(Copied::Terminal) => {
                    lines.push(Line::styled(
                        "copied via the terminal",
                        Style::default().fg(SELECTED),
                    ));
                    if app.mouse {
                        lines.push(Line::styled(
                            "(no? m, then select it)",
                            Style::default().fg(MUTED),
                        ));
                    }
                }
                None => lines.push(Line::styled("c copies it", Style::default().fg(MUTED))),
            }
            if matches!(app.table.conn, Conn::Publishing) {
                lines.push(Line::styled("publishing it…", Style::default().fg(MUTED)));
            }
        }
        Conn::LookingUp => lines.push(Line::styled("looking up code…", Style::default().fg(MUTED))),
        Conn::Dialling => lines.push(Line::styled("connecting…", Style::default().fg(MUTED))),
        Conn::Inviting(name) => lines.push(Line::styled(
            format!("waiting for {name} to accept…"),
            Style::default().fg(MUTED),
        )),
        Conn::Playing => lines.push(Line::from(vec![
            Span::styled("peer ", Style::default().fg(MUTED)),
            Span::raw(app.table.peer_label()),
        ])),
        Conn::Lost(why) => lines.push(Line::styled(why.clone(), Style::default().fg(CAPTURE))),
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Line::from(" game "));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        status,
    );

    draw_moves(f, moves, app);
}

fn draw_moves(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Line::from(" moves "));
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = app
        .game
        .history
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| {
            let black = pair.get(1).map(String::as_str).unwrap_or("");
            Line::from(vec![
                Span::styled(format!("{:>3}. ", i + 1), Style::default().fg(MUTED)),
                Span::raw(format!("{:<8}", pair[0])),
                Span::raw(black.to_string()),
            ])
        })
        .collect();

    // Keep the tail of the game visible.
    if lines.len() > inner_h {
        lines.drain(..lines.len() - inner_h);
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    // A question waiting on an answer stands out from the usual key list.
    let asking = Style::default().fg(CAPTURE).add_modifier(Modifier::BOLD);
    let (keys, style) = if app.confirm_quit {
        ("leave this game?   y leave   n stay", asking)
    } else if app.confirm_resign {
        ("resign this game?   y resign   n cancel", asking)
    } else {
        (footer_keys(app, area.width), Style::default().fg(MUTED))
    };
    f.render_widget(Paragraph::new(Line::styled(keys, style).centered()), area);
}

fn footer_keys(app: &App, width: u16) -> &'static str {
    if app.game.promotion.is_some() {
        "click a piece, or ←/→ and enter   esc cancel"
    } else if width >= 92 {
        "click or drag to move   arrows/hjkl   f flip   p pieces   m mouse   r resign   d draw   q lobby"
    } else if width >= 62 {
        "click or drag   f flip   p pieces   r resign   d draw   q lobby"
    } else {
        "click to move   f flip   r resign   q lobby"
    }
}

fn draw_promotion(f: &mut Frame, g: &Geometry, app: &App) {
    let Some(p) = &app.game.promotion else { return };
    let side = app.game.turn();
    f.render_widget(Clear, g.promo);

    let inner_h = g.promo.height.saturating_sub(2);
    let mut rows: Vec<Line> = Vec::new();
    for sub in 0..inner_h {
        let mut spans = vec![Span::raw(" ")];
        for (i, role) in PROMOTION_ROLES.iter().enumerate() {
            let bg = if i == p.choice { CURSOR } else { LIGHT };
            let piece = Piece {
                color: side,
                role: *role,
            };
            spans.extend(piece_cell(
                piece,
                sub,
                g.promo_cell,
                inner_h,
                app.piece_style,
                bg,
            ));
        }
        rows.push(Line::from(spans));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CURSOR))
        .title(Line::from(" promote to "));
    f.render_widget(
        Paragraph::new(rows).alignment(Alignment::Left).block(block),
        g.promo,
    );

    if let Some(dots) = canvas_dots(app.piece_style, g.promo_cell, inner_h) {
        // Past the border and the one-cell margin the rows start with.
        let area = Rect {
            x: g.promo.x + 2,
            y: g.promo.y + 1,
            width: 4 * g.promo_cell,
            height: inner_h,
        };
        let pieces: Vec<_> = PROMOTION_ROLES
            .iter()
            .enumerate()
            .map(|(i, &role)| {
                let piece = Piece { color: side, role };
                canvas_piece(piece, (i as u16, 0), (0.0, 0.0), (g.promo_cell, inner_h))
            })
            .collect();
        canvas::stamp(f.buffer_mut(), area, &pieces, dots);
    }
}

/// The letters of CHECKMATE and RESIGNED, five blocks square.
fn glyph(c: char) -> [&'static str; 5] {
    match c {
        'C' => [" ████", "█    ", "█    ", "█    ", " ████"],
        'H' => ["█   █", "█   █", "█████", "█   █", "█   █"],
        'E' => ["█████", "█    ", "████ ", "█    ", "█████"],
        'K' => ["█   █", "█  █ ", "███  ", "█  █ ", "█   █"],
        'M' => ["█   █", "██ ██", "█ █ █", "█   █", "█   █"],
        'A' => [" ███ ", "█   █", "█████", "█   █", "█   █"],
        'T' => ["█████", "  █  ", "  █  ", "  █  ", "  █  "],
        'R' => ["████ ", "█   █", "████ ", "█  █ ", "█   █"],
        'S' => [" ████", "█    ", " ███ ", "    █", "████ "],
        'I' => ["█████", "  █  ", "  █  ", "  █  ", "█████"],
        'G' => [" ████", "█    ", "█  ██", "█   █", " ████"],
        'N' => ["█   █", "██  █", "█ █ █", "█  ██", "█   █"],
        'D' => ["████ ", "█   █", "█   █", "█   █", "████ "],
        _ => ["     "; 5],
    }
}

/// The verdict across the middle of the board, spelled out a letter at a
/// time as `reveal` goes from 0 to 1. Each letter lands white hot and cools
/// to red.
fn draw_verdict(f: &mut Frame, g: &Geometry, app: &App, fin: &Finale, reveal: f32) {
    let word = match fin.how {
        Ending::Checkmate => "CHECKMATE",
        Ending::Resignation => "RESIGNED",
    };
    let letters = word.len() as f32;
    let shown = reveal * letters;
    let colour = |i: usize| {
        let age = shown - i as f32;
        if age < 1.0 {
            blend(FLASH, MATE_RED, age)
        } else {
            MATE_RED
        }
    };

    let big_w = word.len() as u16 * 6 - 1;
    let big = g.board.width >= big_w + 6 && g.board.height >= 13;
    let mut lines = vec![Line::raw("")];
    if big {
        for row in 0..5 {
            let mut spans = Vec::new();
            for (i, c) in word.chars().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let text = if (i as f32) < shown {
                    glyph(c)[row]
                } else {
                    "     "
                };
                spans.push(Span::styled(text, Style::default().fg(colour(i))));
            }
            lines.push(Line::from(spans).centered());
        }
    } else {
        let spans: Vec<Span> = word
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let text = if (i as f32) < shown { c } else { ' ' };
                Span::styled(format!("{text} "), Style::default().fg(colour(i)).bold())
            })
            .collect();
        lines.push(Line::from(spans).centered());
    }
    lines.push(Line::raw(""));

    let done = reveal >= 1.0;
    if done {
        let loser = app.game.resigned.unwrap_or(app.game.turn());
        let verdict = match (fin.how, app.me) {
            (Ending::Checkmate, Some(me)) if me == loser => {
                format!("{} wins", app.table.peer_label())
            }
            (Ending::Checkmate, Some(_)) => "you win".to_string(),
            (Ending::Checkmate, None) => format!("{} wins", side_name(!loser)),
            (Ending::Resignation, Some(me)) if me == loser => "you resigned".to_string(),
            (Ending::Resignation, Some(_)) => {
                format!("{} resigned · you win", app.table.peer_label())
            }
            (Ending::Resignation, None) => {
                format!("{} resigns · {} wins", side_name(loser), side_name(!loser))
            }
        };
        lines.push(Line::styled(verdict, Style::default().fg(FLASH).bold()).centered());
        lines.push(Line::styled("any key to see the board", Style::default().fg(MUTED)).centered());
    }

    let width = if big {
        big_w + 6
    } else {
        2 * word.len() as u16 + 8
    }
    .max(30);
    let height = lines.len().max(if big { 10 } else { 5 }) as u16 + 2;
    // In the half of the board away from the king, so the mate stays in
    // view, if there is room there.
    let half = g.grid.height / 2;
    let king_row = app.finale().map_or(0, |fin| {
        let row = 7 - fin.king.rank() as u16;
        if app.flipped { 7 - row } else { row }
    });
    let away = Rect {
        x: g.board.x,
        y: if king_row < 4 {
            g.grid.y + half
        } else {
            g.grid.y
        },
        width: g.board.width,
        height: half,
    };
    let area = centred(if height <= half { away } else { g.board }, width, height);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(MATE_RED))
        .style(Style::default().bg(VERDICT_BG));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn side_name(c: Side) -> &'static str {
    if c == Side::White { "white" } else { "black" }
}
