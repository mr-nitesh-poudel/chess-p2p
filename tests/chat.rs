//! Talking across the table: what goes over the wire, where keys and clicks
//! go while the player is typing, and where the panel sits.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tui_tui::games::chat::{MESSAGE_MAX, Said};
use tui_tui::games::chess::App;
use tui_tui::games::chess::ui::Geometry;
use tui_tui::games::{Conn, Ctx, Seat, Table};
use tui_tui::net::{Net, NetEvent};

/// A game against someone, and what they are sent.
fn connected() -> (Table<App>, UnboundedReceiver<String>) {
    let (out, sent) = unbounded_channel();
    let net = Net {
        peer: iroh::SecretKey::generate().public(),
        out,
    };
    let mut t = Table::new(Ctx::new(Conn::Dialling, None), App::new(Seat::Host));
    t.attach(net, "bob");
    t.set_area(Rect::new(0, 0, 160, 50));
    (t, sent)
}

fn press(t: &mut Table<App>, code: KeyCode) {
    t.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_in(t: &mut Table<App>, text: &str) {
    for c in text.chars() {
        press(t, KeyCode::Char(c));
    }
}

fn click(t: &mut Table<App>, (column, row): (u16, u16)) {
    t.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn chat_area(t: &Table<App>) -> Rect {
    Geometry::of(t.ctx.area, &t.ctx)
        .chat
        .expect("room for the chat")
}

#[test]
fn chat_sits_right_of_the_board_with_the_game_to_its_left() {
    let (t, _) = connected();
    let g = Geometry::of(t.ctx.area, &t.ctx);
    let chat = g.chat.unwrap();
    assert!(g.sidebar.right() <= g.board.x, "game and moves on the left");
    assert!(chat.x >= g.board.right(), "chat on the right");
    assert!(chat.right() <= t.ctx.area.right());
}

#[test]
fn hot_seat_has_nobody_to_talk_to() {
    let mut t = Table::local(App::local());
    assert_eq!(Geometry::of(t.ctx.area, &t.ctx).chat, None);
    press(&mut t, KeyCode::Char('t'));
    assert!(!t.ctx.chat.focused);
}

#[test]
fn typing_goes_to_the_chat_not_the_game_and_enter_sends_it() {
    let (mut t, mut sent) = connected();
    press(&mut t, KeyCode::Char('t'));
    assert!(t.ctx.chat.focused);

    let flipped = t.play.flipped;
    // f flips the board, and q leaves, when the game has the keys.
    type_in(&mut t, "gl hf  ");
    type_in(&mut t, "fq");
    assert_eq!(t.play.flipped, flipped);
    assert!(!t.ctx.confirm_leave);

    press(&mut t, KeyCode::Enter);
    assert_eq!(sent.try_recv().unwrap(), "chat gl hf fq");
    assert_eq!(
        t.ctx.chat.log.back(),
        Some(&Said {
            mine: true,
            text: "gl hf fq".into()
        })
    );
    assert!(t.ctx.chat.input.is_empty());

    // Nothing to say sends nothing.
    type_in(&mut t, "   ");
    press(&mut t, KeyCode::Enter);
    assert!(sent.try_recv().is_err());

    // Esc goes back to the game, rather than leaving it.
    press(&mut t, KeyCode::Esc);
    assert!(!t.ctx.chat.focused);
    assert!(!t.ctx.confirm_leave);
    press(&mut t, KeyCode::Char('f'));
    assert_ne!(t.play.flipped, flipped);
}

#[test]
fn a_message_stops_at_the_longest_either_side_keeps() {
    let (mut t, mut sent) = connected();
    press(&mut t, KeyCode::Char('t'));
    type_in(&mut t, &"a".repeat(MESSAGE_MAX + 20));
    assert_eq!(t.ctx.chat.input.chars().count(), MESSAGE_MAX);
    press(&mut t, KeyCode::Enter);
    assert_eq!(sent.try_recv().unwrap().len(), "chat ".len() + MESSAGE_MAX);
}

#[test]
fn clicking_the_chat_starts_typing_and_clicking_the_board_stops() {
    let (mut t, _) = connected();
    let area = chat_area(&t);
    click(&mut t, (area.x + 2, area.bottom() - 2));
    assert!(t.ctx.chat.focused);

    // The click on the board is still a click on the board.
    let g = Geometry::of(t.ctx.area, &t.ctx);
    let (cw, ch) = g.cell;
    // e2, with white at the bottom.
    click(
        &mut t,
        (g.grid.x + 4 * cw + cw / 2, g.grid.y + 6 * ch + ch / 2),
    );
    assert!(!t.ctx.chat.focused);
    assert_eq!(t.play.game.selected, Some(shakmaty::Square::E2));
}

#[test]
fn the_peers_chat_is_the_tables_and_arrives_clean() {
    let (mut t, _) = connected();
    t.on_net(NetEvent::Line("chat hi\x1b[2J there".into()));
    t.on_net(NetEvent::Line("chat \t ".into()));
    assert_eq!(
        t.ctx
            .chat
            .log
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>(),
        ["hi [2J there"]
    );
    assert!(!t.ctx.chat.log[0].mine);
    assert_eq!(t.ctx.chat.unread, 1);
    // Anything else is still the game's.
    t.on_net(NetEvent::Line("draw".into()));
    assert!(t.play.draw_offered);

    press(&mut t, KeyCode::Char('t'));
    assert_eq!(t.ctx.chat.unread, 0, "seen once the chat is open");
    t.on_net(NetEvent::Line("chat and again".into()));
    assert_eq!(t.ctx.chat.unread, 0, "nor while it is");
}

#[test]
fn nothing_is_sent_before_anyone_joins() {
    let mut t = Table::new(Ctx::new(Conn::Waiting, None), App::new(Seat::Host));
    press(&mut t, KeyCode::Char('t'));
    type_in(&mut t, "anyone?");
    press(&mut t, KeyCode::Enter);
    assert!(t.ctx.chat.log.is_empty());
    assert_eq!(t.ctx.chat.input, "anyone?", "kept for when they do");
}
