//! Reads the half-block sprites back out of a rendered buffer. A piece that
//! looks wrong on screen fails here.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use shakmaty::{Color, Role, Square};
use tui_tui::games::Table;
use tui_tui::games::chess::app::App;
use tui_tui::games::chess::ui::{Geometry, PieceStyle, piece_ink};

/// The pixel grid actually drawn for one square: `O` body, `#` outline,
/// `.` the square showing through.
fn silhouette(app: &Table<App>, w: u16, h: u16, sq: Square, side: Color) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let buf = terminal.backend().buffer().clone();

    let g = Geometry::new(Rect::new(0, 0, w, h));
    let (cw, ch) = g.cell;
    let (fill, line) = piece_ink(app.play.piece_style, side);
    let col = u16::from(sq.file() as u8);
    let row = 7 - u16::from(sq.rank() as u8);

    let mut rows = Vec::new();
    for sub in 0..ch {
        let (mut top, mut bottom) = (String::new(), String::new());
        for x in 0..cw {
            let cell = &buf[(g.grid.x + col * cw + x, g.grid.y + row * ch + sub)];
            let ink = |c| {
                if c == fill {
                    'O'
                } else if c == line {
                    '#'
                } else {
                    '.'
                }
            };
            if cell.symbol() == "▀" {
                top.push(ink(cell.fg));
                bottom.push(ink(cell.bg));
            } else {
                top.push('.');
                bottom.push('.');
            }
        }
        rows.push(top);
        rows.push(bottom);
    }
    rows
}

/// An app drawing half-block sprites, which braille would otherwise replace.
fn sprites() -> Table<App> {
    let mut app = Table::local(App::local());
    app.play.piece_style = PieceStyle::Blocks;
    app
}

/// Every starting piece of `side`, by the square it sits on.
fn all_roles(side: Color) -> [(Role, Square); 6] {
    let back = if side == Color::White { 0 } else { 7 };
    let pawns = if side == Color::White { 1 } else { 6 };
    let sq = |file: u32, rank: u32| {
        Square::from_coords(shakmaty::File::new(file), shakmaty::Rank::new(rank))
    };
    [
        (Role::Rook, sq(0, back)),
        (Role::Knight, sq(1, back)),
        (Role::Bishop, sq(2, back)),
        (Role::Queen, sq(3, back)),
        (Role::King, sq(4, back)),
        (Role::Pawn, sq(0, pawns)),
    ]
}

#[test]
fn every_piece_is_drawn_and_stands_on_its_square() {
    for (w, h) in [(100, 30), (120, 40)] {
        let app = sprites();
        for (role, sq) in all_roles(Color::White) {
            let grid = silhouette(&app, w, h, sq, Color::White);
            let inked = grid
                .iter()
                .flat_map(|r| r.chars())
                .filter(|c| *c != '.')
                .count();
            assert!(inked > 10, "{role:?} is nearly blank at {w}x{h}: {grid:#?}");

            let base = grid.last().unwrap();
            assert!(
                base.contains("###"),
                "{role:?} has no base to stand on at {w}x{h}: {base:?}"
            );
        }
    }
}

#[test]
fn the_six_pieces_are_told_apart() {
    for (w, h) in [(100, 30), (120, 40)] {
        let app = sprites();
        let drawn: Vec<(Role, Vec<String>)> = all_roles(Color::White)
            .into_iter()
            .map(|(role, sq)| (role, silhouette(&app, w, h, sq, Color::White)))
            .collect();

        for (i, (a_role, a)) in drawn.iter().enumerate() {
            for (b_role, b) in &drawn[i + 1..] {
                assert_ne!(a, b, "{a_role:?} and {b_role:?} look identical at {w}x{h}");
            }
        }
    }
}

#[test]
fn both_sides_share_a_shape() {
    let app = sprites();
    for ((role, white_sq), (_, black_sq)) in all_roles(Color::White)
        .into_iter()
        .zip(all_roles(Color::Black))
    {
        let white = silhouette(&app, 120, 40, white_sq, Color::White);
        let black = silhouette(&app, 120, 40, black_sq, Color::Black);
        assert_eq!(white, black, "{role:?} differs between the two sides");
    }
}

#[test]
fn small_squares_fall_back_instead_of_going_blank() {
    // 80x24 gives 5x2 squares, too small for a sprite.
    let app = sprites();
    let grid = silhouette(&app, 80, 24, Square::E1, Color::White);
    assert!(
        grid.iter().all(|r| r.chars().all(|c| c == '.')),
        "no sprite should be drawn at this size"
    );

    // Something must still be on the square, just not made of half blocks.
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let g = Geometry::new(Rect::new(0, 0, 80, 24));
    let (cw, ch) = g.cell;
    let drawn: String = (0..ch)
        .flat_map(|sub| (0..cw).map(move |x| (g.grid.x + 4 * cw + x, g.grid.y + 7 * ch + sub)))
        .map(|pos| buf[pos].symbol().to_string())
        .collect();
    assert!(
        drawn.contains('♚'),
        "expected a figurine fallback, got {drawn:?}"
    );
}

#[test]
fn both_sides_are_outlined_in_near_black() {
    let luma = |c| match c {
        ratatui::style::Color::Rgb(r, g, b) => {
            0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
        }
        other => panic!("expected a true colour, got {other:?}"),
    };
    for side in [Color::White, Color::Black] {
        let (fill, line) = piece_ink(PieceStyle::Blocks, side);
        assert!(
            luma(line) < 50.0,
            "{side:?} outline is not near-black: {line:?}"
        );
        // The body has to sit far enough off the outline for it to show.
        assert!(
            (luma(fill) - luma(line)).abs() > 30.0,
            "{side:?} body and outline are too close: {fill:?} vs {line:?}"
        );
    }
    // And the two sides must still be obvious apart.
    let (white, _) = piece_ink(PieceStyle::Blocks, Color::White);
    let (black, _) = piece_ink(PieceStyle::Blocks, Color::Black);
    assert!(
        luma(white) - luma(black) > 120.0,
        "the two sides look alike"
    );
}

#[test]
fn big_letters_are_drawn_and_told_apart() {
    let mut app = Table::local(App::local());
    app.play.piece_style = PieceStyle::BigLetter;

    let drawn: Vec<(Role, Vec<String>)> = all_roles(Color::White)
        .into_iter()
        .map(|(role, sq)| (role, silhouette(&app, 120, 40, sq, Color::White)))
        .collect();

    for (role, grid) in &drawn {
        let strokes = grid
            .iter()
            .flat_map(|r| r.chars())
            .filter(|c| *c == 'O')
            .count();
        assert!(
            strokes > 8,
            "{role:?} has almost no letter in it: {grid:#?}"
        );
    }
    for (i, (a_role, a)) in drawn.iter().enumerate() {
        for (b_role, b) in &drawn[i + 1..] {
            assert_ne!(a, b, "{a_role:?} and {b_role:?} read the same");
        }
    }
}

#[test]
fn a_black_letter_reads_against_its_border() {
    let luma = |c| match c {
        ratatui::style::Color::Rgb(r, g, b) => {
            0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
        }
        other => panic!("expected a true colour, got {other:?}"),
    };
    let (black, border) = piece_ink(PieceStyle::BigLetter, Color::Black);
    let (white, white_border) = piece_ink(PieceStyle::BigLetter, Color::White);

    // The border stays near-black on both sides, as it is for the sprites.
    assert!(luma(border) < 50.0 && luma(white_border) < 50.0);
    // A letter is thin, so its stroke has to clear its own border.
    assert!(
        luma(black) - luma(border) > 60.0,
        "black strokes vanish into their border: {black:?} on {border:?}"
    );
    // And the two sides still have to be obvious apart.
    assert!(luma(white) - luma(black) > 90.0, "the two sides look alike");
}

#[test]
fn big_letters_fall_back_on_a_small_square() {
    // 80x24 gives 5x2 squares: too small for the 7x7 outlined letter.
    let mut app = Table::local(App::local());
    app.play.piece_style = PieceStyle::BigLetter;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let buf = terminal.backend().buffer().clone();

    let g = Geometry::new(Rect::new(0, 0, 80, 24));
    let (cw, ch) = g.cell;
    let drawn: String = (0..ch)
        .flat_map(|sub| (0..cw).map(move |x| (g.grid.x + 4 * cw + x, g.grid.y + 7 * ch + sub)))
        .map(|pos| buf[pos].symbol().to_string())
        .collect();
    assert!(
        drawn.contains('K'),
        "expected a plain letter fallback, got {drawn:?}"
    );
}
