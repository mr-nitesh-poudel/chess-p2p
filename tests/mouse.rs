//! Click-to-square mapping, and the gestures built on top of it.

use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use shakmaty::{Role, Square};
use tui_tui::games::Table;
use tui_tui::games::chess::app::App;
use tui_tui::games::chess::ui::Geometry;

fn geometry(w: u16, h: u16) -> Geometry {
    Geometry::new(Rect::new(0, 0, w, h))
}

/// The middle of a square on screen — the inverse of `square_at`.
fn centre_of(g: &Geometry, sq: Square, flipped: bool) -> (u16, u16) {
    let (cw, ch) = g.cell;
    let (file, rank) = (sq.file() as u16, sq.rank() as u16);
    let col = if flipped { 7 - file } else { file };
    let row = if flipped { rank } else { 7 - rank };
    (g.grid.x + col * cw + cw / 2, g.grid.y + row * ch + ch / 2)
}

fn press(app: &mut Table<App>, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn release(app: &mut Table<App>, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn click(app: &mut Table<App>, sq: Square) {
    let g = Geometry::new(app.ctx.area);
    let (x, y) = centre_of(&g, sq, app.play.flipped);
    press(app, x, y);
    release(app, x, y);
}

fn app_at(w: u16, h: u16) -> Table<App> {
    let mut app = Table::local(App::local());
    app.ctx.area = Rect::new(0, 0, w, h);
    app
}

#[test]
fn the_board_grows_with_the_terminal() {
    let small = geometry(80, 24).cell;
    let big = geometry(120, 40).cell;
    assert!(
        big.0 > small.0 && big.1 > small.1,
        "{big:?} should beat {small:?}"
    );
    // Even a cramped terminal still gets a whole board rather than a clipped one.
    let tiny = geometry(60, 18);
    assert_eq!(tiny.grid.width % 8, 0);
    assert!(tiny.grid.right() <= 60 && tiny.grid.bottom() <= 18);
}

#[test]
fn clicks_land_on_the_right_square() {
    for (w, h) in [(80, 24), (100, 30), (120, 40), (60, 18)] {
        let g = geometry(w, h);
        for flipped in [false, true] {
            for sq in Square::ALL {
                let (x, y) = centre_of(&g, sq, flipped);
                assert_eq!(
                    g.square_at(x, y, flipped),
                    Some(sq),
                    "{sq} at {w}x{h} flipped={flipped}"
                );
            }
        }
    }
}

#[test]
fn clicks_off_the_grid_hit_nothing() {
    let g = geometry(100, 30);
    // The rank gutter, the border, and the sidebar are not playable.
    assert_eq!(g.square_at(g.board.x, g.grid.y, false), None);
    assert_eq!(g.square_at(g.sidebar.x + 2, g.grid.y, false), None);
    assert_eq!(g.square_at(g.grid.x, g.grid.bottom(), false), None);
    assert_eq!(g.square_at(g.grid.right(), g.grid.y, false), None);
}

#[test]
fn clicking_a_piece_then_a_square_moves_it() {
    let mut app = app_at(100, 30);
    click(&mut app, Square::E2);
    assert_eq!(
        app.play.game.selected,
        Some(Square::E2),
        "a click picks the pawn up"
    );

    click(&mut app, Square::E4);
    assert_eq!(app.play.game.history, ["e4"]);
    assert_eq!(app.play.game.selected, None);
}

#[test]
fn dragging_a_piece_moves_it() {
    let mut app = app_at(120, 40);
    let g = Geometry::new(app.ctx.area);
    let (fx, fy) = centre_of(&g, Square::G1, false);
    let (tx, ty) = centre_of(&g, Square::F3, false);

    press(&mut app, fx, fy);
    release(&mut app, tx, ty);
    assert_eq!(app.play.game.history, ["Nf3"]);
}

#[test]
fn dropping_on_an_illegal_square_plays_nothing() {
    let mut app = app_at(120, 40);
    let g = Geometry::new(app.ctx.area);
    let (fx, fy) = centre_of(&g, Square::E2, false);
    let (tx, ty) = centre_of(&g, Square::E5, false);

    press(&mut app, fx, fy);
    release(&mut app, tx, ty);
    assert!(
        app.play.game.history.is_empty(),
        "e2e5 is not a legal first move"
    );
}

#[test]
fn a_right_click_puts_the_piece_back_down() {
    let mut app = app_at(100, 30);
    click(&mut app, Square::D2);
    assert!(app.play.game.selected.is_some());

    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.play.game.selected, None);
}

#[test]
fn hovering_moves_the_cursor() {
    let mut app = app_at(100, 30);
    let g = Geometry::new(app.ctx.area);
    let (x, y) = centre_of(&g, Square::C6, false);
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.play.game.cursor, Square::C6);
}

#[test]
fn the_promotion_prompt_is_clickable() {
    let mut app = app_at(100, 30);
    for m in [
        "d2d4", "e7e5", "d4e5", "d7d5", "e5d6", "g8f6", "d6c7", "a7a6",
    ] {
        app.play.game.play_uci(m).unwrap();
    }
    click(&mut app, Square::C7);
    click(&mut app, Square::B8);
    assert!(
        app.play.game.promotion.is_some(),
        "the prompt should be open"
    );

    // Third piece across is the bishop.
    let g = Geometry::new(app.ctx.area);
    let x = g.promo.x + 1 + 2 * g.promo_cell + g.promo_cell / 2;
    press(&mut app, x, g.promo.y + 1);

    assert!(app.play.game.promotion.is_none());
    assert_eq!(app.play.game.history.last().unwrap(), "cxb8=B");
    assert_eq!(
        app.play.game.piece_at(Square::B8).unwrap().role,
        Role::Bishop
    );
}
