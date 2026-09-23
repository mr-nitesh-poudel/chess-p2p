//! Leaving a game in play asks first; leaving one that is over does not.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_tui::games::chess::app::App;
use tui_tui::games::{Leave, Table};

fn press(app: &mut Table<App>, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn footer(app: &Table<App>) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, buf.area.height - 1)].symbol())
        .collect()
}

#[test]
fn q_and_esc_ask_before_leaving_a_game_in_play() {
    for first in [KeyCode::Char('q'), KeyCode::Esc] {
        for confirm in [KeyCode::Char('y'), KeyCode::Char('q'), KeyCode::Enter] {
            let mut app = Table::local(App::local());
            press(&mut app, first);
            assert_eq!(app.leaving(), None, "{first:?} leaves without asking");
            assert!(footer(&app).contains("leave this game?"), "{first:?}");
            press(&mut app, confirm);
            assert_eq!(
                app.leaving(),
                Some(Leave::Lobby),
                "{first:?} then {confirm:?}"
            );
        }
    }
}

#[test]
fn anything_else_stays_in_the_game() {
    for answer in [KeyCode::Char('n'), KeyCode::Esc, KeyCode::Char('f')] {
        let mut app = Table::local(App::local());
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, answer);
        assert_eq!(app.leaving(), None, "{answer:?}");
        assert!(!app.ctx.confirm_leave, "{answer:?}");
        assert!(
            !app.play.flipped,
            "the answer is not also taken as a command"
        );
        assert!(!footer(&app).contains("leave this game?"));
    }
}

#[test]
fn esc_drops_a_selected_piece_before_it_asks_to_leave() {
    let mut app = Table::local(App::local());
    app.play.game.cursor = shakmaty::Square::E2;
    press(&mut app, KeyCode::Enter);
    assert!(app.play.game.selected.is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.play.game.selected.is_none());
    assert!(!app.ctx.confirm_leave);
}

#[test]
fn a_finished_game_is_left_at_once() {
    let mut app = Table::local(App::local());
    for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        app.play.game.play_uci(m).unwrap();
    }
    app.play.banner_hidden = true;
    press(&mut app, KeyCode::Char('q'));
    assert_eq!(app.leaving(), Some(Leave::Lobby));
}
