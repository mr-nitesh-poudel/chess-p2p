//! Everything that draws, plus the geometry that mouse clicks are tested
//! against. Both come from [`Geometry`], so what you see and what you can
//! click on cannot drift apart.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use shakmaty::{Color as Side, File, Piece, Rank, Role, Square};

use crate::app::{App, Conn, Slide};
use crate::clipboard::Copied;
use crate::game::PROMOTION_ROLES;
use crate::lobby::{Entry, Field, Item, Lobby, Row};

const LIGHT: Color = Color::Rgb(214, 194, 162);
const DARK: Color = Color::Rgb(137, 99, 73);
const LIGHT_LAST: Color = Color::Rgb(206, 204, 122);
const DARK_LAST: Color = Color::Rgb(163, 150, 71);
const PIECE_WHITE: Color = Color::Rgb(252, 250, 245);
const PIECE_BLACK: Color = Color::Rgb(20, 18, 16);
const CURSOR: Color = Color::Rgb(246, 205, 82);
const SELECTED: Color = Color::Rgb(124, 176, 95);
const CAPTURE: Color = Color::Rgb(204, 96, 78);
const MUTED: Color = Color::Rgb(128, 128, 128);
/// Washed over a square you can move to, rather than replacing its colour, so
/// the board underneath still reads.
const TARGET: Color = Color::Rgb(106, 196, 84);
const MARKER: Color = Color::Rgb(38, 92, 30);

/// How a piece is drawn. Each falls back to the next when a square is too
/// small to carry it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceStyle {
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
            PieceStyle::Letter => PieceStyle::Blocks,
        }
    }
}

/// Square sizes we are willing to draw, smallest first. Widths are odd so a
/// single glyph sits dead centre.
const CELL_SIZES: [(u16, u16); 5] = [(3, 1), (5, 2), (7, 3), (9, 4), (11, 5)];
/// Rank digit plus a space, down the left of the board.
const GUTTER: u16 = 2;
const SIDEBAR_MIN: u16 = 24;

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
    /// Picks the biggest board that leaves room for the sidebar.
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

        let [left, sidebar] =
            Layout::horizontal([Constraint::Length(block_w(cw)), Constraint::Min(0)]).areas(main);
        let [top_tray, board, bottom_tray, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(block_h(ch)),
            Constraint::Length(1),
            Constraint::Min(0),
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

/// Where the lobby's pieces sit. [`LobbyGeometry::rows`] lines up with
/// [`Lobby::rows`], so what is drawn and what is clicked cannot drift apart.
pub struct LobbyGeometry {
    pub panel: Rect,
    pub rows: Vec<Rect>,
    pub friends_heading: Rect,
    /// Where to say how friends get here, while there are none.
    pub no_friends: Option<Rect>,
    pub input: Rect,
    pub hint: Rect,
    pub footer: Rect,
}

const LOBBY_W: u16 = 46;
const PROMPT: &str = "code ";

/// A label and a blurb for most items; a single line for Quit and a friend.
fn row_height(row: Row) -> u16 {
    match row {
        Row::Friend(_) | Row::Item(Item::Quit) => 1,
        Row::Item(_) => 2,
    }
}

impl LobbyGeometry {
    pub fn new(area: Rect, rows: &[Row]) -> Self {
        // Top to bottom inside the panel first, then placed on screen once
        // the panel's height is known.
        let has_friends = rows.iter().any(|r| matches!(r, Row::Friend(_)));
        let mut y = 1;
        let mut heading = 0;
        let mut no_friends = None;
        let mut placed = Vec::with_capacity(rows.len());
        for (i, &row) in rows.iter().enumerate() {
            let starts_friends = match row {
                Row::Friend(_) => i == 0 || !matches!(rows[i - 1], Row::Friend(_)),
                Row::Item(Item::Name) => !has_friends,
                _ => false,
            };
            if starts_friends {
                heading = y + 1;
                y += 2;
                if !has_friends {
                    no_friends = Some(y);
                    y += 1;
                }
            }
            if row == Row::Item(Item::Name) {
                y += 1;
            }
            placed.push((y, row_height(row)));
            y += row_height(row);
        }
        let (input, hint) = (y + 1, y + 2);
        let inner_h = y + 4;

        let [main, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        let panel = centred(main, LOBBY_W, inner_h + 2);
        let at = |y: u16, h: u16| {
            Rect {
                x: panel.x + 2,
                y: panel.y + 1 + y,
                width: panel.width.saturating_sub(4),
                height: h,
            }
            .intersection(panel)
        };
        Self {
            panel,
            rows: placed.into_iter().map(|(y, h)| at(y, h)).collect(),
            friends_heading: at(heading, 1),
            no_friends: no_friends.map(|y| at(y, 1)),
            input: at(input, 1),
            hint: at(hint, 1),
            footer,
        }
    }

    /// The row under a screen position, if any.
    pub fn row_at(&self, x: u16, y: u16) -> Option<usize> {
        self.rows.iter().position(|r| r.contains(Position { x, y }))
    }
}

pub fn draw_lobby(f: &mut Frame, lobby: &Lobby) {
    let rows = lobby.rows();
    let g = LobbyGeometry::new(f.area(), &rows);
    let muted = Style::default().fg(MUTED);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(muted)
        .title(Line::from(" chess-p2p ").centered());
    f.render_widget(block, g.panel);

    f.render_widget(
        Paragraph::new(Line::styled("friends", muted)),
        g.friends_heading,
    );
    if let Some(area) = g.no_friends {
        f.render_widget(
            Paragraph::new(Line::styled("  anyone you play turns up here", muted)),
            area,
        );
    }

    for (i, (&row, &area)) in rows.iter().zip(&g.rows).enumerate() {
        let (mark, label) = if i == lobby.selected {
            (
                "▸ ",
                Style::default().fg(CURSOR).add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default())
        };
        let lines = match row {
            Row::Friend(n) => {
                let friend = &lobby.friends[n];
                let games = match friend.games {
                    1 => "1 game".to_string(),
                    n => format!("{n} games"),
                };
                let name: String = friend.name.chars().take(18).collect();
                vec![Line::from(vec![
                    Span::styled(mark, label),
                    Span::styled(format!("{name:<19}"), label),
                    Span::styled(format!("{games} · {}", ago(friend.last_played)), muted),
                ])]
            }
            Row::Item(item) => {
                let mut lines = vec![Line::from(vec![
                    Span::styled(mark, label),
                    Span::styled(item.label(), label),
                ])];
                let blurb = match item {
                    Item::Host => Line::styled("  get a code to send your opponent", muted),
                    Item::Join => Line::styled("  type in the code your opponent sent", muted),
                    Item::Local => Line::styled("  two players taking turns", muted),
                    Item::Name if lobby.editing == Some(Field::Name) => Line::from(vec![
                        Span::raw("  "),
                        Span::styled(lobby.name_input.as_str(), Style::default().fg(CURSOR)),
                    ]),
                    Item::Name if lobby.guest => Line::styled(
                        format!("  {} (guest: another copy has your profile)", lobby.name),
                        muted,
                    ),
                    Item::Name => Line::from(vec![
                        Span::raw("  "),
                        Span::raw(lobby.name.as_str()),
                        Span::styled(" — what friends see", muted),
                    ]),
                    Item::Quit => Line::raw(""),
                };
                lines.push(blurb);
                lines
            }
        };
        f.render_widget(Paragraph::new(lines), area);
    }

    let joining = lobby.editing == Some(Field::Code);
    let input = if lobby.input.is_empty() && !joining {
        Line::from(vec![
            Span::styled(PROMPT, muted),
            Span::styled("42-tiger-marble-ocean", muted),
        ])
    } else {
        let typed = if joining {
            Style::default().fg(CURSOR)
        } else {
            Style::default()
        };
        // The rest of the word Tab would fill in, greyed out after the cursor.
        let ghost = lobby.completion().map_or("", |word| {
            let typed = lobby.input.rsplit('-').next().map_or(0, str::len);
            &word[typed..]
        });
        Line::from(vec![
            Span::styled(PROMPT, muted),
            Span::styled(lobby.input.as_str(), typed),
            Span::styled(ghost, muted),
        ])
    };
    f.render_widget(Paragraph::new(input), g.input);

    match lobby.editing {
        Some(Field::Code) => set_cursor(f, g.input, PROMPT.len() + lobby.input.len()),
        Some(Field::Name) => {
            let blurb = g.rows[lobby.selected];
            let line = Rect {
                y: blurb.y + 1,
                height: 1,
                ..blurb
            };
            set_cursor(f, line, 2 + lobby.name_input.chars().count());
        }
        None => {}
    }

    let hint = if let Some(i) = lobby.forgetting {
        let name = lobby.friends.get(i).map_or("them", |f| f.name.as_str());
        Line::styled(
            format!("forget {name}? y to confirm"),
            Style::default().fg(CAPTURE),
        )
    } else if lobby.editing == Some(Field::Name) {
        Line::styled("enter saves, esc cancels", muted)
    } else if joining {
        match lobby.entry() {
            Entry::Empty => Line::styled("type or paste the code you were sent", muted),
            Entry::Typing if lobby.completion().is_some() => {
                Line::styled("tab finishes the word", muted)
            }
            Entry::Typing => Line::raw(""),
            Entry::Ready(_) => Line::styled("enter to join", Style::default().fg(SELECTED)),
            Entry::Bad(why) => Line::styled(why, Style::default().fg(CAPTURE)),
        }
    } else if let Some(notice) = &lobby.notice {
        Line::styled(notice.as_str(), Style::default().fg(CURSOR))
    } else {
        Line::raw("")
    };
    f.render_widget(Paragraph::new(hint), g.hint);

    let on_friend = matches!(rows.get(lobby.selected), Some(Row::Friend(_)));
    let keys = if lobby.invite.is_some() {
        "y accept   n decline"
    } else if joining {
        "enter join   tab complete   ctrl-u clear   esc back"
    } else if lobby.editing == Some(Field::Name) {
        "enter save   esc cancel"
    } else if on_friend {
        "enter challenge   x forget   ↑/↓ choose   q quit"
    } else {
        "↑/↓ choose   enter select   or type a code   q quit"
    };
    f.render_widget(
        Paragraph::new(Line::styled(keys, muted)).centered(),
        g.footer,
    );

    if let Some(name) = &lobby.invite {
        let area = centred(g.panel, 40, 5);
        f.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CURSOR))
            .title(Line::from(" invite ").centered());
        let lines = vec![
            Line::raw(""),
            Line::styled(
                format!("{name} wants to play chess"),
                Style::default().add_modifier(Modifier::BOLD),
            )
            .centered(),
            Line::styled("y accept   n decline", muted).centered(),
        ];
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn set_cursor(f: &mut Frame, line: Rect, offset: usize) {
    let x = line.x + offset as u16;
    f.set_cursor_position(Position {
        x: x.min(line.right().saturating_sub(1)),
        y: line.y,
    });
}

/// How long ago a Unix time was, in the loosest terms that still help.
fn ago(then: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let s = now.saturating_sub(then);
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * HOUR;
    const TWO_DAYS: u64 = 2 * DAY;
    match s {
        0..60 => "just now".into(),
        60..HOUR => format!("{}m ago", s / 60),
        HOUR..DAY => format!("{}h ago", s / HOUR),
        DAY..TWO_DAYS => "yesterday".into(),
        _ if s < 14 * DAY => format!("{}d ago", s / DAY),
        _ => format!("{}w ago", s / (7 * DAY)),
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
}

fn draw_board(f: &mut Frame, g: &Geometry, app: &App) {
    let (cw, ch) = g.cell;
    let game = &app.game;
    let targets = game.targets();
    let style = app.piece_style;
    let mut lines = Vec::with_capacity(usize::from(8 * ch + 1));

    // Whatever is sliding is drawn on top afterwards, not in place.
    let travelling = app.slide_at().map(|(s, _)| s.to);

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
                let is_target = targets.contains(&sq);

                let dark = (file + rank) % 2 == 0;
                let mut bg = match (dark, game.last.is_some_and(|(a, b)| a == sq || b == sq)) {
                    (true, false) => DARK,
                    (false, false) => LIGHT,
                    (true, true) => DARK_LAST,
                    (false, true) => LIGHT_LAST,
                };
                // Washes stack, so a square that is both reachable and under
                // the cursor still shows that it is both.
                if is_target {
                    let strength = if piece.is_some() { 0.62 } else { 0.42 };
                    bg = blend(bg, TARGET, strength);
                }
                if game.selected == Some(sq) {
                    bg = blend(bg, SELECTED, 0.8);
                }
                if sq == game.cursor {
                    // Light enough that a green wash underneath still reads.
                    bg = blend(bg, CURSOR, 0.5);
                }

                match piece {
                    Some(p) => spans.extend(piece_cell(p, sub, cw, ch, style, bg)),
                    None => {
                        // The legal-move dot sits on the square's middle row.
                        let content = if is_target && sub == ch / 2 {
                            centre(&marker(cw).to_string(), cw)
                        } else {
                            " ".repeat(cw.into())
                        };
                        spans.push(Span::styled(content, Style::default().fg(MARKER).bg(bg)));
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
}

/// Draws the travelling piece over the board it was already drawn onto.
///
/// Working straight on the buffer keeps the piece free of the square grid, so
/// it can sit halfway between two squares.
fn draw_slide(buf: &mut Buffer, g: &Geometry, app: &App, slide: &Slide, t: f32) {
    let (cw, ch) = g.cell;
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

    // Ease in and out, so the piece does not start and stop abruptly.
    let e = t * t * (3.0 - 2.0 * t);
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
    if style != PieceStyle::Blocks {
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

/// Mixes `over` into `base` at `alpha`. Terminals have no alpha channel, so
/// the blend happens here and is handed over as one solid colour.
fn blend(base: Color, over: Color, alpha: f32) -> Color {
    let (Color::Rgb(br, bg, bb), Color::Rgb(or, og, ob)) = (base, over) else {
        return over;
    };
    let mix = |b: u8, o: u8| (f32::from(b) * (1.0 - alpha) + f32::from(o) * alpha).round() as u8;
    Color::Rgb(mix(br, or), mix(bg, og), mix(bb, ob))
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

fn marker(cell_w: u16) -> char {
    if cell_w >= 5 { '●' } else { '•' }
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
    match &app.conn {
        Conn::Local => {}
        Conn::Publishing | Conn::Waiting => {
            lines.push(Line::styled("share this code:", Style::default().fg(MUTED)));
            lines.push(Line::styled(
                app.share.clone().unwrap_or_default(),
                Style::default().fg(CURSOR),
            ));
            match app.copied {
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
            if matches!(app.conn, Conn::Publishing) {
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
            Span::raw(app.peer_label()),
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
    let keys = if app.game.promotion.is_some() {
        "click a piece, or ←/→ and enter   esc cancel"
    } else if app.confirm_resign {
        "y confirm resign   n cancel"
    } else if area.width >= 92 {
        "click or drag to move   arrows/hjkl   f flip   p pieces   m mouse   r resign   d draw   q lobby"
    } else if area.width >= 62 {
        "click or drag   f flip   p pieces   r resign   d draw   q lobby"
    } else {
        "click to move   f flip   r resign   q lobby"
    };
    f.render_widget(
        Paragraph::new(Line::styled(format!(" {keys}"), Style::default().fg(MUTED))),
        area,
    );
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
}

fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn side_name(c: Side) -> &'static str {
    if c == Side::White { "white" } else { "black" }
}
