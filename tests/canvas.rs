//! Reads the canvas pieces back out of a rendered buffer, dot by dot, in
//! both kinds of dot: octant and braille.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use shakmaty::{Color, File, Rank, Role, Square};
use tui_tui::games::chess::app::App;
use tui_tui::games::chess::canvas::Dots;
use tui_tui::games::chess::ui::{self, Geometry, PieceStyle, canvas_colour};

/// 11x5 squares.
const BIG: (u16, u16) = (140, 52);
/// 9x4 squares, the smallest the canvas styles are drawn at.
const MID: (u16, u16) = (120, 40);

/// The canvas styles, and the dots each draws with.
const STYLES: [(PieceStyle, Dots); 2] = [
    (PieceStyle::Octant, Dots::Octant),
    (PieceStyle::Braille, Dots::Braille),
];

fn app(style: PieceStyle) -> App {
    let mut app = App::local();
    app.piece_style = style;
    app
}

fn render(app: &App, (w, h): (u16, u16)) -> (Buffer, Geometry) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (buf, Geometry::new(Rect::new(0, 0, w, h)))
}

/// The cells of one square, in rows.
fn cells(g: &Geometry, sq: Square) -> Vec<Vec<(u16, u16)>> {
    let (cw, ch) = g.cell;
    let col = u16::from(sq.file() as u8);
    let row = 7 - u16::from(sq.rank() as u8);
    (0..ch)
        .map(|y| {
            (0..cw)
                .map(|x| (g.grid.x + col * cw + x, g.grid.y + row * ch + y))
                .collect()
        })
        .collect()
}

/// The dots drawn on one square: `#` a dot, `.` none.
fn dots(buf: &Buffer, g: &Geometry, sq: Square, kind: Dots) -> Vec<String> {
    let (cw, ch) = g.cell;
    let mut grid = vec![vec!['.'; usize::from(2 * cw)]; usize::from(4 * ch)];
    for (cy, row) in cells(g, sq).iter().enumerate() {
        for (cx, &pos) in row.iter().enumerate() {
            let bits = kind.decode(buf[pos].symbol()).unwrap_or(0);
            for k in 0..8 {
                if bits & 1 << k != 0 {
                    grid[cy * 4 + k / 2][cx * 2 + k % 2] = '#';
                }
            }
        }
    }
    grid.into_iter().map(String::from_iter).collect()
}

fn count(grid: &[String]) -> usize {
    grid.iter()
        .flat_map(|r| r.chars())
        .filter(|&c| c == '#')
        .count()
}

fn square(file: u32, rank: u32) -> Square {
    Square::from_coords(File::new(file), Rank::new(rank))
}

/// Every starting piece of `side`, by the square it sits on.
fn all_roles(side: Color) -> [(Role, Square); 6] {
    let (back, pawns) = if side == Color::White { (0, 1) } else { (7, 6) };
    [
        (Role::Rook, square(0, back)),
        (Role::Knight, square(1, back)),
        (Role::Bishop, square(2, back)),
        (Role::Queen, square(3, back)),
        (Role::King, square(4, back)),
        (Role::Pawn, square(0, pawns)),
    ]
}

#[test]
fn octants_are_the_default() {
    assert_eq!(App::local().piece_style, PieceStyle::Octant);
}

/// Octants and braille share a grid, so the same piece is the same dots.
#[test]
fn both_kinds_draw_the_same_dots() {
    for size in [BIG, MID] {
        let (octant, g) = render(&app(PieceStyle::Octant), size);
        let (braille, _) = render(&app(PieceStyle::Braille), size);
        for side in [Color::White, Color::Black] {
            for (role, sq) in all_roles(side) {
                assert_eq!(
                    dots(&octant, &g, sq, Dots::Octant),
                    dots(&braille, &g, sq, Dots::Braille),
                    "{side:?} {role:?} at {size:?}"
                );
            }
        }
    }
}

#[test]
fn every_piece_is_drawn_and_stands_on_its_square() {
    for ((style, kind), size) in STYLES.into_iter().flat_map(|s| [(s, BIG), (s, MID)]) {
        let (buf, g) = render(&app(style), size);
        for side in [Color::White, Color::Black] {
            for (role, sq) in all_roles(side) {
                let grid = dots(&buf, &g, sq, kind);
                assert!(
                    count(&grid) > 20,
                    "{side:?} {role:?} is nearly blank at {size:?}: {grid:#?}"
                );
                // The plinth is the widest thing on the lowest inked row.
                let base = grid.iter().rev().find(|r| r.contains('#')).unwrap();
                assert!(
                    base.contains("#####"),
                    "{side:?} {role:?} has no base at {size:?}: {grid:#?}"
                );
                // Clear of every edge of the square.
                let edges = [
                    grid[0].contains('#'),
                    // Two rows clear below.
                    grid[grid.len() - 2..].iter().any(|r| r.contains('#')),
                    grid.iter().any(|r| r.starts_with('#') || r.ends_with('#')),
                ];
                assert_eq!(
                    edges, [false; 3],
                    "{side:?} {role:?} touches its square's edge at {size:?}: {grid:#?}"
                );
            }
        }
    }
}

#[test]
fn the_six_pieces_are_told_apart() {
    for ((style, kind), size) in STYLES.into_iter().flat_map(|s| [(s, BIG), (s, MID)]) {
        let (buf, g) = render(&app(style), size);
        for side in [Color::White, Color::Black] {
            let drawn: Vec<_> = all_roles(side)
                .into_iter()
                .map(|(role, sq)| (role, dots(&buf, &g, sq, kind)))
                .collect();
            for (i, (a_role, a)) in drawn.iter().enumerate() {
                for (b_role, b) in &drawn[i + 1..] {
                    assert_ne!(
                        a, b,
                        "{side:?} {a_role:?} and {b_role:?} look the same at {size:?}"
                    );
                }
            }
        }
    }
}

/// Both sides are the same solid silhouette, in their own colour.
#[test]
fn both_sides_share_a_shape_in_their_own_colour() {
    for ((style, kind), size) in STYLES.into_iter().flat_map(|s| [(s, BIG), (s, MID)]) {
        let (buf, g) = render(&app(style), size);
        for ((role, white_sq), (_, black_sq)) in all_roles(Color::White)
            .into_iter()
            .zip(all_roles(Color::Black))
        {
            assert_eq!(
                dots(&buf, &g, white_sq, kind),
                dots(&buf, &g, black_sq, kind),
                "{role:?} differs between the sides at {size:?}"
            );
            for (sq, side) in [(white_sq, Color::White), (black_sq, Color::Black)] {
                let inked: Vec<_> = cells(&g, sq)
                    .into_iter()
                    .flatten()
                    .filter(|&pos| buf[pos].symbol() != " ")
                    .collect();
                assert!(
                    inked.iter().all(|&pos| buf[pos].fg == canvas_colour(side)),
                    "{side:?} {role:?} is not all in {side:?}'s colour"
                );
            }
        }
    }
}

/// The dots go over the square, not over a patch of their own colour.
#[test]
fn the_square_shows_between_the_dots() {
    for (style, _) in STYLES {
        square_shows(style);
    }
}

fn square_shows(style: PieceStyle) {
    let mut app = app(style);
    app.game.cursor = square(4, 4);
    let (buf, g) = render(&app, BIG);
    for (_, sq) in all_roles(Color::White)
        .into_iter()
        .chain(all_roles(Color::Black))
    {
        let rows = cells(&g, sq);
        let bg = buf[rows[0][0]].bg;
        assert!(
            rows.iter().flatten().all(|&pos| buf[pos].bg == bg),
            "{sq} has cells in more than one background"
        );
    }
}

#[test]
fn small_squares_fall_back_to_sprites() {
    for (style, kind) in STYLES {
        falls_back(style, kind);
    }
}

fn falls_back(style: PieceStyle, kind: Dots) {
    // 100x30 gives 7x3 squares, too few dots for the pieces' detail.
    let (buf, g) = render(&app(style), (100, 30));
    assert_eq!(g.cell, (7, 3));
    let symbols: String = cells(&g, square(4, 0))
        .into_iter()
        .flatten()
        .map(|pos| buf[pos].symbol().to_string())
        .collect();
    assert!(
        symbols.contains('▀'),
        "expected a half-block sprite: {symbols:?}"
    );
    // Octants include the half blocks, so check for nothing but them.
    assert!(
        symbols.chars().all(|c| c == ' ' || c == '▀'),
        "and nothing drawn in {kind:?}: {symbols:?}"
    );
}

#[test]
fn p_reaches_every_style() {
    let mut style = PieceStyle::Octant;
    let mut seen = vec![style];
    loop {
        style = style.next();
        if style == PieceStyle::Octant {
            break;
        }
        assert!(!seen.contains(&style), "{style:?} comes round twice");
        seen.push(style);
    }
    assert_eq!(seen.len(), 7, "every style is reachable: {seen:?}");
}
