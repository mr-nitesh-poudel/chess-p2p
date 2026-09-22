//! Application state and key handling: the bit that decides what a keypress means.

use std::time::{Duration, Instant};

use iroh::EndpointId;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Color as Paint, Modifier, Style};
use shakmaty::{Color, Move, Piece, Position, Role, Square};

use crate::game::{Game, PROMOTION_ROLES, ui_to};
use crate::net::{Net, NetEvent, Out};
use crate::ui::{Geometry, PieceStyle};

/// How long a piece takes to travel between two squares.
pub const SLIDE: Duration = Duration::from_millis(160);

/// A piece on its way from one square to another.
pub struct Slide {
    pub piece: Piece,
    pub from: Square,
    pub to: Square,
    pub start: Instant,
}

pub enum Conn {
    /// Hot-seat: no network at all.
    Local,
    /// Hosting, waiting for someone to dial in.
    Waiting,
    Dialling,
    Playing,
    Lost(String),
}

pub struct App {
    pub game: Game,
    /// The side we are allowed to move, or `None` when sharing a keyboard.
    pub me: Option<Color>,
    pub net: Option<Net>,
    pub peer: Option<EndpointId>,
    pub share: Option<String>,
    pub conn: Conn,
    pub flipped: bool,
    pub piece_style: PieceStyle,
    /// Whether the terminal is reporting mouse events to us.
    pub mouse: bool,
    /// Last known terminal size, for turning clicks into squares.
    pub area: Rect,
    /// The square a press picked a piece up from, for drag-and-drop.
    drag_from: Option<Square>,
    pub slide: Option<Slide>,
    /// Overrides the animation clock, so a test can render an exact frame.
    pub clock: Option<Instant>,
    pub quit: bool,
    pub confirm_resign: bool,
    /// The peer has offered a draw and we have not answered.
    pub draw_offered: bool,
    /// We have offered a draw and are waiting for an answer.
    pub draw_sent: bool,
    pub draw_agreed: bool,
    pub note: Option<String>,
}

impl App {
    pub fn local() -> Self {
        Self {
            game: Game::new(),
            me: None,
            net: None,
            peer: None,
            share: None,
            conn: Conn::Local,
            flipped: false,
            piece_style: PieceStyle::Blocks,
            mouse: true,
            area: Rect::new(0, 0, 80, 24),
            drag_from: None,
            slide: None,
            clock: None,
            quit: false,
            confirm_resign: false,
            draw_offered: false,
            draw_sent: false,
            draw_agreed: false,
            note: None,
        }
    }

    pub fn networked(net: Net, me: Color, conn: Conn) -> Self {
        let share = net.id.to_string();
        Self {
            me: Some(me),
            // Always sit behind your own pieces.
            flipped: me == Color::Black,
            share: Some(share),
            net: Some(net),
            conn,
            ..Self::local()
        }
    }

    pub fn peer_short(&self) -> String {
        self.peer
            .map(|p| p.fmt_short().to_string())
            .unwrap_or_else(|| "—".into())
    }

    fn now(&self) -> Instant {
        self.clock.unwrap_or_else(Instant::now)
    }

    /// The slide in progress, and how far through it is from 0.0 to 1.0.
    pub fn slide_at(&self) -> Option<(&Slide, f32)> {
        let slide = self.slide.as_ref()?;
        let elapsed = self.now().saturating_duration_since(slide.start);
        (elapsed < SLIDE).then(|| (slide, elapsed.as_secs_f32() / SLIDE.as_secs_f32()))
    }

    pub fn is_animating(&self) -> bool {
        self.slide_at().is_some()
    }

    /// Sets a move travelling. The piece is read off the board after the move,
    /// so a promotion slides in as whatever it became.
    fn begin_slide(&mut self, m: Move) {
        let to = ui_to(m);
        let (Some(from), Some(piece)) = (m.from(), self.game.piece_at(to)) else {
            return;
        };
        self.slide = Some(Slide {
            piece,
            from,
            to,
            start: self.now(),
        });
    }

    /// A move we made: animate it and tell the opponent.
    fn broadcast(&mut self, m: Move) {
        self.begin_slide(m);
        let uci = self.game.to_uci(m);
        self.send(Out::Move(uci));
    }

    fn send(&self, msg: Out) {
        if let Some(net) = &self.net {
            let _ = net.out.send(msg);
        }
    }

    /// Is it our move? Always true in hot-seat.
    fn my_turn(&self) -> bool {
        self.me.is_none_or(|me| me == self.game.turn())
    }

    /// The one-line description of where the game stands.
    pub fn state_line(&self) -> (String, Style) {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let win = bold.fg(Paint::Rgb(124, 176, 95));
        let warn = Style::default().fg(Paint::Rgb(209, 106, 88));

        if let Some(loser) = self.game.resigned {
            let winner = if loser == Color::White {
                "black"
            } else {
                "white"
            };
            return (format!("{winner} wins by resignation"), win);
        }
        if self.draw_agreed {
            return ("draw agreed".into(), bold);
        }
        if self.game.pos.is_checkmate() {
            let winner = if self.game.turn() == Color::White {
                "black"
            } else {
                "white"
            };
            return (format!("checkmate — {winner} wins"), win);
        }
        if self.game.pos.is_stalemate() {
            return ("stalemate — draw".into(), bold);
        }
        if self.game.pos.is_insufficient_material() {
            return ("draw — insufficient material".into(), bold);
        }
        if let Some(note) = &self.note {
            return (note.clone(), warn);
        }
        if self.draw_offered {
            return ("draw offered — press d to accept".into(), warn);
        }
        if self.draw_sent {
            return (
                "draw offer sent".into(),
                Style::default().fg(Paint::Rgb(128, 128, 128)),
            );
        }
        if self.game.pos.is_check() {
            return ("check".into(), warn);
        }
        if !self.my_turn() {
            return (
                "waiting for opponent".into(),
                Style::default().fg(Paint::Rgb(128, 128, 128)),
            );
        }
        ("your move".into(), Style::default())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.note = None;

        if self.game.promotion.is_some() {
            self.promotion_key(key.code);
            return;
        }
        if self.confirm_resign {
            match key.code {
                KeyCode::Char('y') => {
                    self.confirm_resign = false;
                    let me = self.me.unwrap_or(self.game.turn());
                    self.game.resigned = Some(me);
                    self.send(Out::Resign);
                }
                _ => self.confirm_resign = false,
            }
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => {
                if self.game.selected.is_some() {
                    self.game.selected = None;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('f') => self.flipped = !self.flipped,
            KeyCode::Char('p') => self.piece_style = self.piece_style.next(),
            KeyCode::Char('m') => self.mouse = !self.mouse,
            KeyCode::Char('r') if !self.game.over() => self.confirm_resign = true,
            KeyCode::Char('d') if !self.game.over() => self.draw_key(),
            KeyCode::Left | KeyCode::Char('h') => self.nudge(-1, 0),
            KeyCode::Right | KeyCode::Char('l') => self.nudge(1, 0),
            KeyCode::Up | KeyCode::Char('k') => self.nudge(0, 1),
            KeyCode::Down | KeyCode::Char('j') => self.nudge(0, -1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            _ => {}
        }
    }

    /// Cursor deltas are in screen terms; the board may be upside down.
    fn nudge(&mut self, dfile: i32, drank: i32) {
        if self.flipped {
            self.game.nudge(-dfile, -drank);
        } else {
            self.game.nudge(dfile, drank);
        }
    }

    fn activate(&mut self) {
        if !self.my_turn() {
            return;
        }
        if let Some(m) = self.game.activate(self.me) {
            self.broadcast(m);
        }
    }

    fn promotion_key(&mut self, code: KeyCode) {
        let Some(p) = self.game.promotion.as_mut() else {
            return;
        };
        let chosen = match code {
            KeyCode::Left | KeyCode::Char('h') => {
                p.choice = (p.choice + PROMOTION_ROLES.len() - 1) % PROMOTION_ROLES.len();
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                p.choice = (p.choice + 1) % PROMOTION_ROLES.len();
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => Some(PROMOTION_ROLES[p.choice]),
            KeyCode::Char('q') => Some(Role::Queen),
            KeyCode::Char('r') => Some(Role::Rook),
            KeyCode::Char('b') => Some(Role::Bishop),
            KeyCode::Char('n') => Some(Role::Knight),
            KeyCode::Esc => {
                self.game.promotion = None;
                None
            }
            _ => None,
        };
        if let Some(m) = chosen.and_then(|role| self.game.promote(role)) {
            self.broadcast(m);
        }
    }

    fn draw_key(&mut self) {
        if self.draw_offered {
            self.draw_offered = false;
            self.draw_agreed = true;
            self.send(Out::Draw);
        } else if self.net.is_some() && !self.draw_sent {
            self.draw_sent = true;
            self.send(Out::Draw);
        }
    }

    pub fn on_net(&mut self, event: NetEvent) {
        match event {
            NetEvent::Online => {}
            NetEvent::Connected(peer) => {
                self.peer = Some(peer);
                self.conn = Conn::Playing;
            }
            NetEvent::Move(uci) => match self.game.play_uci(&uci) {
                // Playing on silently declines any outstanding draw offer.
                Ok(m) => {
                    self.begin_slide(m);
                    self.draw_offered = false;
                    self.draw_sent = false;
                }
                // A peer running the same code cannot produce this, so it means
                // the two sides have diverged. Say so rather than guessing.
                Err(why) => self.note = Some(format!("peer sent {why}")),
            },
            NetEvent::Resign => {
                self.game.resigned = self.me.map(|me| !me);
            }
            // A draw message answers our own offer, or starts a new one.
            NetEvent::Draw => {
                if self.draw_sent {
                    self.draw_sent = false;
                    self.draw_agreed = true;
                } else {
                    self.draw_offered = true;
                }
            }
            NetEvent::Disconnected(why) => self.conn = Conn::Lost(why),
        }
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        let g = Geometry::new(self.area);
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.press(&g, ev.column, ev.row),
            MouseEventKind::Up(MouseButton::Left) => self.release(&g, ev.column, ev.row),
            // A right click puts a picked-up piece back down.
            MouseEventKind::Down(MouseButton::Right) => {
                self.game.selected = None;
                self.drag_from = None;
            }
            // Let the pointer lead the cursor, so hovering previews a square.
            MouseEventKind::Moved => {
                if let Some(sq) = g.square_at(ev.column, ev.row, self.flipped) {
                    self.game.cursor = sq;
                }
            }
            _ => {}
        }
    }

    fn press(&mut self, g: &Geometry, x: u16, y: u16) {
        self.note = None;

        if self.game.promotion.is_some() {
            let picked = g
                .promo_at(x, y)
                .and_then(|i| self.game.promote(PROMOTION_ROLES[i]));
            if let Some(m) = picked {
                self.broadcast(m);
            }
            return;
        }
        if self.confirm_resign {
            self.confirm_resign = false;
            return;
        }

        let Some(sq) = g.square_at(x, y, self.flipped) else {
            return;
        };
        self.game.cursor = sq;
        self.activate();
        // Remember the piece we picked up, so a drag can drop it elsewhere.
        self.drag_from = self.game.selected;
    }

    fn release(&mut self, g: &Geometry, x: u16, y: u16) {
        let Some(from) = self.drag_from.take() else {
            return;
        };
        let Some(sq) = g.square_at(x, y, self.flipped) else {
            return;
        };
        // Releasing where we pressed is a click: the piece stays selected.
        if sq == from || self.game.selected != Some(from) {
            return;
        }
        self.game.cursor = sq;
        self.activate();
    }
}
