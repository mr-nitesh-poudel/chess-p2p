//! The lobby: choosing from the menu, and typing a code in.

use chess_p2p::app::{App, Conn};
use chess_p2p::lobby::{Choice, Entry, ITEMS, Item, Lobby};
use chess_p2p::session::Code;
use chess_p2p::ui::LobbyGeometry;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

fn key(lobby: &mut Lobby, code: KeyCode) -> Option<Choice> {
    lobby.on_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn typed(lobby: &mut Lobby, text: &str) -> Option<Choice> {
    text.chars().find_map(|c| key(lobby, KeyCode::Char(c)))
}

fn code(s: &str) -> Code {
    s.parse().unwrap()
}

fn index(item: Item) -> usize {
    ITEMS.iter().position(|&i| i == item).unwrap()
}

#[test]
fn the_menu_picks_what_is_selected() {
    let mut lobby = Lobby::new();
    assert_eq!(key(&mut lobby, KeyCode::Enter), Some(Choice::Host));

    let mut lobby = Lobby::new();
    key(&mut lobby, KeyCode::Down);
    key(&mut lobby, KeyCode::Down);
    assert_eq!(key(&mut lobby, KeyCode::Enter), Some(Choice::Local));

    // Wrapping upwards from the top lands on the last item.
    let mut lobby = Lobby::new();
    key(&mut lobby, KeyCode::Up);
    assert_eq!(key(&mut lobby, KeyCode::Enter), Some(Choice::Quit));

    assert_eq!(key(&mut Lobby::new(), KeyCode::Char('q')), Some(Choice::Quit));
}

#[test]
fn joining_opens_the_code_box_and_esc_backs_out() {
    let mut lobby = Lobby::new();
    key(&mut lobby, KeyCode::Down);
    assert_eq!(key(&mut lobby, KeyCode::Enter), None);
    assert!(lobby.joining);

    // In the box, q is part of a word, not a way out.
    assert_eq!(key(&mut lobby, KeyCode::Char('q')), None);
    assert_eq!(lobby.input, "q");

    key(&mut lobby, KeyCode::Esc);
    assert!(!lobby.joining);
}

#[test]
fn typing_a_number_from_the_menu_starts_a_code() {
    let mut lobby = Lobby::new();
    assert_eq!(typed(&mut lobby, "42 Tiger marble ocean"), None);
    assert!(lobby.joining);
    assert_eq!(lobby.selected, index(Item::Join));
    assert_eq!(lobby.input, "42-tiger-marble-ocean");
    assert_eq!(
        key(&mut lobby, KeyCode::Enter),
        Some(Choice::Join(code("42-tiger-marble-ocean")))
    );
}

#[test]
fn enter_does_nothing_until_the_code_is_whole() {
    let mut lobby = Lobby::new();
    typed(&mut lobby, "42-tiger-marble");
    assert_eq!(key(&mut lobby, KeyCode::Enter), None);
    assert!(lobby.joining);
}

#[test]
fn tab_finishes_a_word_and_moves_on() {
    let mut lobby = Lobby::new();
    typed(&mut lobby, "42");
    key(&mut lobby, KeyCode::Tab);
    assert_eq!(lobby.input, "42-");

    typed(&mut lobby, "tig");
    assert_eq!(lobby.completion(), Some("tiger"));
    key(&mut lobby, KeyCode::Tab);
    assert_eq!(lobby.input, "42-tiger-");

    // "mar" could be marble, march, margin...: nothing to finish, and no
    // dash past a word that is not one yet.
    typed(&mut lobby, "mar");
    assert_eq!(lobby.completion(), None);
    key(&mut lobby, KeyCode::Tab);
    assert_eq!(lobby.input, "42-tiger-mar");
}

#[test]
fn bad_parts_are_flagged_as_you_go() {
    let entry = |text: &str| {
        let mut lobby = Lobby::new();
        typed(&mut lobby, text);
        lobby.entry()
    };
    assert_eq!(entry("4"), Entry::Typing);
    assert_eq!(entry("42-tig"), Entry::Typing);
    assert!(matches!(entry("42-xylo"), Entry::Bad(_)));
    assert!(matches!(entry("420-tiger"), Entry::Bad(_)));
    assert!(matches!(entry("42-tigr-marble"), Entry::Bad(_)));
    assert_eq!(
        entry("42-tiger-marble-ocean"),
        Entry::Ready(code("42-tiger-marble-ocean"))
    );
}

#[test]
fn ctrl_w_drops_the_last_part() {
    let mut lobby = Lobby::new();
    typed(&mut lobby, "42-tiger-marb");
    lobby.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(lobby.input, "42-tiger-");
    lobby.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(lobby.input, "42-");
}

#[test]
fn pasting_takes_the_code_or_the_whole_command() {
    for pasted in [
        "42-tiger-marble-ocean",
        "  42 Tiger Marble Ocean\n",
        "chess-p2p join 42-tiger-marble-ocean\n",
    ] {
        let mut lobby = Lobby::new();
        lobby.on_paste(pasted);
        assert!(lobby.joining, "{pasted:?}");
        assert_eq!(
            lobby.entry(),
            Entry::Ready(code("42-tiger-marble-ocean")),
            "{pasted:?}"
        );
    }
}

#[test]
fn clicking_an_item_chooses_it() {
    let mut lobby = Lobby::new();
    lobby.area = Rect::new(0, 0, 80, 24);
    let g = LobbyGeometry::new(lobby.area);

    let at = |i: usize| (g.items[i].x + 2, g.items[i].y);
    let mouse = |kind, (column, row)| MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    // Hovering selects, clicking chooses.
    lobby.on_mouse(mouse(MouseEventKind::Moved, at(index(Item::Local))));
    assert_eq!(lobby.selected, index(Item::Local));
    let click = MouseEventKind::Down(MouseButton::Left);
    assert_eq!(
        lobby.on_mouse(mouse(click, at(index(Item::Host)))),
        Some(Choice::Host)
    );
    // The blurb under an item is part of it.
    let blurb = (g.items[index(Item::Local)].x, g.items[index(Item::Local)].y + 1);
    assert_eq!(lobby.on_mouse(mouse(click, blurb)), Some(Choice::Local));

    assert_eq!(lobby.on_mouse(mouse(click, (g.input.x, g.input.y))), None);
    assert!(lobby.joining);
}

#[test]
fn c_copies_the_code_only_while_it_is_needed() {
    let mut app = App::local();
    app.share = Some("42-tiger-marble-ocean".into());
    let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);

    app.on_key(c);
    assert_eq!(app.copy_request, None, "hot-seat has nothing to share");

    app.conn = Conn::Waiting;
    app.on_key(c);
    assert_eq!(app.copy_request.as_deref(), Some("42-tiger-marble-ocean"));

    app.copy_request = None;
    app.conn = Conn::Playing;
    app.on_key(c);
    assert_eq!(app.copy_request, None, "the opponent is already in");
}
