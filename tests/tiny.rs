//! Every screen, at every terminal size down to nothing: none may panic.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tui_tui::games::chess::App;
use tui_tui::games::chess::ui::PieceStyle;
use tui_tui::games::{Conn, Kind, Play, Seat, Table};
use tui_tui::lobby::{self, Field, Invited, Lobby};

/// Every small size, where the arithmetic is tightest, then a spread of
/// bigger ones up to a maximised window.
fn sizes() -> impl Iterator<Item = (u16, u16)> {
    let widths = (0..=40).chain((41..=250).step_by(13));
    widths.flat_map(|w| (0..=20).chain((21..=70).step_by(7)).map(move |h| (w, h)))
}

/// The corners, the middle, and the edges between, of a `w` by `h` screen.
fn spots(w: u16, h: u16) -> Vec<(u16, u16)> {
    let (x1, y1) = (w.saturating_sub(1), h.saturating_sub(1));
    vec![
        (0, 0),
        (x1, 0),
        (0, y1),
        (x1, y1),
        (w / 2, h / 2),
        (w / 2, 0),
        (0, h / 2),
    ]
}

fn mouse(kind: MouseEventKind, (column, row): (u16, u16)) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

const GESTURES: [MouseEventKind; 4] = [
    MouseEventKind::Moved,
    MouseEventKind::Down(MouseButton::Left),
    MouseEventKind::Up(MouseButton::Left),
    MouseEventKind::Down(MouseButton::Right),
];

fn games() -> Vec<(&'static str, App)> {
    let mut out = vec![];
    for style in std::iter::successors(Some(PieceStyle::Octant), |s| Some(s.next())).take(7) {
        let mut app = App::local();
        app.piece_style = style;
        app.game.cursor = shakmaty::Square::E2;
        app.game.activate(None);
        out.push(("selected", app));
    }
    let mut mated = App::local();
    for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        mated.game.play_uci(m).unwrap();
    }
    mated.ended = Some(std::time::Instant::now() - std::time::Duration::from_secs(10));
    out.push(("mated", mated));
    let mut promo = App::local();
    for m in [
        "a2a4", "b7b5", "a4b5", "a7a6", "b5a6", "c8b7", "a6a7", "b7c6",
    ] {
        promo.game.play_uci(m).unwrap();
    }
    promo.game.cursor = shakmaty::Square::A7;
    promo.game.activate(None);
    promo.game.cursor = shakmaty::Square::B8;
    promo.game.activate(None);
    out.push(("promoting", promo));
    let mut hosting = App::new(Seat::Host, Table::new(Conn::Waiting, None));
    hosting.table.share = Some("42-tiger-marble-ocean".into());
    out.push(("hosting", hosting));
    let mut quitting = App::local();
    quitting.confirm_quit = true;
    out.push(("quitting", quitting));
    out
}

#[test]
fn games_draw_at_any_size() {
    for (label, mut app) in games() {
        for (w, h) in sizes() {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            app.set_area(Rect::new(0, 0, w, h));
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                t.draw(|f| app.draw(f)).unwrap();
            }));
            assert!(r.is_ok(), "{label} at {w}x{h}");
        }
    }
}

#[test]
fn lobbies_draw_at_any_size() {
    let mut plain = Lobby::new();
    plain.name = "ace".into();
    let mut invited = Lobby::new();
    invited.invite = Some(Invited {
        name: "a very long name".into(),
        game: Kind::Chess,
    });
    let mut typing = Lobby::new();
    typing.editing = Some(Field::Code);
    typing.input = "42-tiger-mar".into();
    let mut naming = Lobby::new();
    naming.editing = Some(Field::Name);
    naming.selected = naming.rows().len() - 2;
    naming.name_input = "someone".into();
    for (label, lobby) in [
        ("plain", plain),
        ("invited", invited),
        ("typing", typing),
        ("naming", naming),
    ] {
        for (w, h) in sizes() {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                t.draw(|f| lobby::draw_lobby(f, &lobby)).unwrap();
            }));
            assert!(r.is_ok(), "{label} at {w}x{h}");
        }
    }
}

#[test]
fn clicks_land_safely_at_any_size() {
    for (w, h) in sizes() {
        // Each game takes the whole run of clicks, as a player's would.
        for (label, mut app) in games() {
            app.set_area(Rect::new(0, 0, w, h));
            for at in spots(w, h) {
                for kind in GESTURES {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        app.on_mouse(mouse(kind, at));
                    }));
                    assert!(r.is_ok(), "{label}: {kind:?} at {at:?} on {w}x{h}");
                }
            }
        }
        let mut lobby = Lobby::new();
        lobby.area = Rect::new(0, 0, w, h);
        for at in spots(w, h) {
            for kind in GESTURES {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    lobby.on_mouse(mouse(kind, at));
                }));
                assert!(r.is_ok(), "lobby: {kind:?} at {at:?} on {w}x{h}");
            }
        }
    }
}
