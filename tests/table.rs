//! The table works for any game, not just chess. This one keeps a tally and
//! wants a single key, which is all it takes to see what the table does for
//! a game and what it keeps to itself.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::{Frame, Terminal, backend::TestBackend};
use tui_tui::games::{Conn, Ctx, Handled, Kind, Leave, Play, Table, chess, chrome};
use tui_tui::net::NetEvent;
use tui_tui::session::Progress;

/// Counts `+` presses, remembers what the opponent said, and is over once it
/// reaches three.
#[derive(Default)]
struct Tally {
    count: u32,
    keys_seen: Vec<KeyCode>,
    heard: Vec<String>,
    clicks: u32,
}

impl Play for Tally {
    fn kind(&self) -> Kind {
        // The table asks only for a name to put in notes; any kind will do.
        chess::KIND
    }

    fn on_key(&mut self, key: KeyEvent, _: &mut Ctx) -> Handled {
        self.keys_seen.push(key.code);
        if key.code == KeyCode::Char('+') {
            self.count += 1;
            Handled::Used
        } else {
            Handled::Unused
        }
    }

    fn on_mouse(&mut self, _: MouseEvent, _: &mut Ctx) {
        self.clicks += 1;
    }

    fn on_line(&mut self, line: &str, _: &mut Ctx) {
        self.heard.push(line.to_string());
    }

    fn draw(&self, f: &mut Frame, ctx: &Ctx) {
        chrome::footer(f, f.area(), ctx, None, &[("+", "counts"), ("q", "leaves")]);
    }

    fn in_play(&self) -> bool {
        self.count < 3
    }
}

fn press(t: &mut Table<Tally>, code: KeyCode) {
    t.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn screen(t: &Table<Tally>) -> String {
    let mut term = Terminal::new(TestBackend::new(60, 1)).unwrap();
    term.draw(|f| t.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
}

#[test]
fn the_game_gets_its_keys_and_the_table_gets_the_rest() {
    let mut t = Table::local(Tally::default());
    press(&mut t, KeyCode::Char('+'));
    press(&mut t, KeyCode::Char('m'));
    assert_eq!(t.play.count, 1);
    assert!(!t.wants_mouse(), "m, unused by the game, toggled the mouse");
    // The game saw m first, and chose not to use it.
    assert_eq!(t.play.keys_seen, [KeyCode::Char('+'), KeyCode::Char('m')]);
}

#[test]
fn leaving_a_game_in_play_asks_and_the_game_never_sees_the_answer() {
    let mut t = Table::local(Tally::default());
    press(&mut t, KeyCode::Char('q'));
    assert!(t.ctx.confirm_leave);
    assert!(screen(&t).contains("leave this game?"));

    let seen = t.play.keys_seen.len();
    press(&mut t, KeyCode::Char('+'));
    assert_eq!(t.play.keys_seen.len(), seen, "the answer is the table's");
    assert_eq!(t.play.count, 0, "and not also a move");
    assert_eq!(t.leaving(), None, "anything but yes stays");

    press(&mut t, KeyCode::Esc);
    press(&mut t, KeyCode::Char('y'));
    assert_eq!(t.leaving(), Some(Leave::Lobby));
}

#[test]
fn a_decided_game_is_left_without_asking() {
    let mut t = Table::local(Tally::default());
    for _ in 0..3 {
        press(&mut t, KeyCode::Char('+'));
    }
    assert!(!t.play.in_play());
    press(&mut t, KeyCode::Char('q'));
    assert_eq!(t.leaving(), Some(Leave::Lobby));
}

#[test]
fn ctrl_c_quits_and_never_reaches_the_game() {
    let mut t = Table::local(Tally::default());
    t.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(t.leaving(), Some(Leave::Exit));
    assert!(t.play.keys_seen.is_empty());
}

#[test]
fn only_key_presses_arrive() {
    let mut t = Table::local(Tally::default());
    let mut release = KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    t.on_key(release);
    assert!(t.play.keys_seen.is_empty());
}

#[test]
fn the_opponents_lines_go_to_the_game_and_the_connection_stays_with_the_table() {
    let mut t = Table::new(Ctx::new(Conn::Publishing, None), Tally::default());
    t.on_net(NetEvent::Progress(Progress::Listed));
    t.on_net(NetEvent::Line("hello".into()));
    t.on_net(NetEvent::Disconnected("they left".into()));
    assert_eq!(t.play.heard, ["hello"]);
    assert_eq!(t.ctx.conn, Conn::Lost("they left".into()));
}

#[test]
fn a_click_while_the_table_is_asking_only_dismisses_the_question() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    let mut t = Table::local(Tally::default());
    press(&mut t, KeyCode::Char('q'));
    t.on_mouse(click);
    assert!(!t.ctx.confirm_leave);
    assert_eq!(t.play.clicks, 0);
    t.on_mouse(click);
    assert_eq!(t.play.clicks, 1);
}

#[test]
fn any_game_can_sit_behind_the_trait() {
    // What the main loop holds: it never learns which game this is.
    let mut t: Box<Table<dyn Play>> = Box::new(Table::local(Tally::default()));
    t.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(t.ctx.confirm_leave);
}
