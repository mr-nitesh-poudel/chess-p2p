//! A game of chess in progress, and what a key, click or message does to it.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Color as Paint, Modifier, Style};
use shakmaty::{Color, Move, Piece, Position, Role, Square};

use super::protocol::Msg;
use super::rules::{Game, PROMOTION_ROLES, ui_to};
use super::ui::{Geometry, PieceStyle};
use crate::games::{Conn, Kind, Seat, Table};
use crate::net::NetEvent;

/// How long a piece takes to travel between two squares.
pub const SLIDE: Duration = Duration::from_millis(160);

/// Checkmate plays out in three acts once the mating piece lands: the king
/// shudders while its square flashes red, then topples as the board goes
/// dark, and then the verdict is spelled out across the board.
pub const SHUDDER: Duration = Duration::from_millis(700);
pub const TOPPLE: Duration = Duration::from_millis(600);
pub const REVEAL: Duration = Duration::from_millis(600);
pub const FINALE: Duration = SHUDDER.saturating_add(TOPPLE).saturating_add(REVEAL);

/// A resignation is quieter: a moment's stillness, then the king lays itself
/// down slowly, and the verdict follows.
pub const STILL: Duration = Duration::from_millis(250);
pub const LAY_DOWN: Duration = Duration::from_millis(1100);
pub const RESIGN_FINALE: Duration = STILL.saturating_add(LAY_DOWN).saturating_add(REVEAL);

/// How long a king's square pulses red when it is put in check.
pub const CHECK_PULSE: Duration = Duration::from_millis(900);

/// How the game was lost, which sets how the finale plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Checkmate,
    Resignation,
}

impl Ending {
    /// The length of each act: the first beat, the fall, and the reveal.
    fn acts(self) -> (Duration, Duration, Duration) {
        match self {
            Ending::Checkmate => (SHUDDER, TOPPLE, REVEAL),
            Ending::Resignation => (STILL, LAY_DOWN, REVEAL),
        }
    }

    pub fn length(self) -> Duration {
        let (a, b, c) = self.acts();
        a + b + c
    }
}

/// Where the finale is at, for drawing one frame of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Finale {
    pub how: Ending,
    /// The king that has lost.
    pub king: Square,
    /// The pieces giving the check.
    pub checkers: Vec<Square>,
    /// How strongly the king's square is washed red, 0 to 1.
    pub red: f32,
    /// A white flash over the board the moment the mate lands, fading fast.
    pub impact: f32,
    /// How far the rest of the board has gone dark, 0 to 1.
    pub dim: f32,
    /// How far the king is knocked sideways by its shudder, in dots.
    pub shake: f64,
    /// How far the king has toppled: 0 upright, 1 flat on its right side,
    /// -1 on its left. It falls away from whatever mated it.
    pub fallen: f64,
    /// How much of the verdict has been spelled out, 0 to 1, once it shows.
    pub banner: Option<f32>,
}

/// A piece on its way from one square to another.
pub struct Slide {
    pub piece: Piece,
    pub from: Square,
    pub to: Square,
    pub start: Instant,
}

pub struct App {
    pub game: Game,
    /// The side we are allowed to move, or `None` when sharing a keyboard.
    pub me: Option<Color>,
    /// The opponent, and how the connection to them is going.
    pub table: Table,
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
    /// Done with this game: back to the lobby.
    pub quit: bool,
    /// Done with the program altogether.
    pub exit: bool,
    pub confirm_resign: bool,
    /// Asked to leave a game still in play, and not yet sure.
    pub confirm_quit: bool,
    /// The peer has offered a draw and we have not answered.
    pub draw_offered: bool,
    /// We have offered a draw and are waiting for an answer.
    pub draw_sent: bool,
    pub draw_agreed: bool,
    pub note: Option<String>,
    /// When the finale began: the mating piece landed, or someone resigned.
    pub ended: Option<Instant>,
    /// When the last check landed, for the pulse on the king's square.
    pub checked: Option<Instant>,
    /// The verdict has been waved away, to look at the board.
    pub banner_hidden: bool,
}

impl App {
    pub fn local() -> Self {
        Self {
            game: Game::new(),
            me: None,
            table: Table::local(),
            flipped: false,
            piece_style: PieceStyle::Octant,
            mouse: true,
            area: Rect::new(0, 0, 80, 24),
            drag_from: None,
            slide: None,
            clock: None,
            quit: false,
            exit: false,
            confirm_resign: false,
            confirm_quit: false,
            draw_offered: false,
            draw_sent: false,
            draw_agreed: false,
            note: None,
            ended: None,
            checked: None,
            banner_hidden: false,
        }
    }

    /// A game at `seat`. The host plays white.
    pub fn new(seat: Seat, table: Table) -> Self {
        let me = match seat {
            Seat::Local => {
                return Self {
                    table,
                    ..Self::local()
                };
            }
            Seat::Host => Color::White,
            Seat::Guest => Color::Black,
        };
        Self {
            me: Some(me),
            // Always sit behind your own pieces.
            flipped: me == Color::Black,
            table,
            ..Self::local()
        }
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
        let now = self.now();
        let finale = self
            .ended
            .zip(self.ending())
            .is_some_and(|(at, how)| now < at + how.length());
        let check =
            self.check_glow().is_some() && self.checked.is_some_and(|at| now < at + CHECK_PULSE);
        self.slide_at().is_some() || finale || check
    }

    /// How the game was lost, if it was.
    pub fn ending(&self) -> Option<Ending> {
        if self.game.resigned.is_some() {
            Some(Ending::Resignation)
        } else if self.game.pos.is_checkmate() {
            Some(Ending::Checkmate)
        } else {
            None
        }
    }

    /// The king in check, and how strongly its square glows red: a few
    /// quick pulses as the check lands, then a steady warning. `None` when
    /// there is no check, or it is mate, which has its own finale.
    pub fn check_glow(&self) -> Option<(Square, f32)> {
        if !self.game.pos.is_check() || self.ending().is_some() {
            return None;
        }
        let king = self.game.pos.board().king_of(self.game.turn())?;
        let now = self.now();
        let t = match self.checked {
            // Not until the checking piece has arrived.
            Some(at) if now < at => return None,
            Some(at) => (now - at).as_secs_f32() / CHECK_PULSE.as_secs_f32(),
            None => 1.0,
        };
        let glow = if t < 1.0 {
            0.3 + 0.4 * (t * 2.0 * std::f32::consts::PI).sin().abs()
        } else {
            0.3
        };
        Some((king, glow))
    }

    /// The finale as of now: `None` until the game is lost and, for a
    /// checkmate, the mating piece has landed.
    pub fn finale(&self) -> Option<Finale> {
        let at = self.ended?;
        let how = self.ending()?;
        let now = self.now();
        if now < at {
            return None;
        }
        let elapsed = now - at;
        let loser = self.game.resigned.unwrap_or(self.game.turn());
        let king = self.game.pos.board().king_of(loser)?;
        let checkers: Vec<Square> = match how {
            Ending::Checkmate => self.game.pos.checkers().into_iter().collect(),
            Ending::Resignation => Vec::new(),
        };

        // Each act's progress, from 0 to 1.
        let act = |start: Duration, len: Duration| {
            (elapsed.saturating_sub(start).as_secs_f32() / len.as_secs_f32()).min(1.0)
        };
        let (first, fall_len, reveal_len) = how.acts();
        let shudder = act(Duration::ZERO, first);
        let topple = act(first, fall_len);
        let reveal = act(first + fall_len, reveal_len);

        if how == Ending::Resignation {
            // Laid down gently, easing in and out, with no drama but the dark.
            let t = f64::from(topple);
            let eased = t * t * (3.0 - 2.0 * t);
            let away = if self.flipped { -1.0 } else { 1.0 };
            let hidden = self.banner_hidden;
            return Some(Finale {
                how,
                king,
                checkers,
                red: 0.4 * topple,
                impact: 0.0,
                dim: if hidden { 0.0 } else { topple * 0.55 },
                shake: 0.0,
                fallen: away * eased,
                banner: (!hidden && elapsed >= first + fall_len).then_some(reveal),
            });
        }

        // Three pulses of red that settle on a steady glow.
        let pulse = (shudder * 3.0 * std::f32::consts::PI).sin().abs();
        let red = if shudder < 1.0 {
            0.35 + 0.5 * pulse
        } else {
            0.7
        };
        let impact = (1.0 - elapsed.as_secs_f32() / 0.15).max(0.0) * 0.45;
        // A fast rattle that dies away.
        let shake = if shudder < 1.0 {
            1.5 * f64::from(1.0 - shudder) * (f64::from(shudder) * 45.0).sin()
        } else {
            0.0
        };
        // Falling picks up speed, like something heavy tipping over, then
        // bounces once where it lands.
        let fall = f64::from(topple);
        let fallen = if fall < 0.8 {
            (fall / 0.8).powi(2)
        } else {
            1.0 - 0.08 * ((fall - 0.8) / 0.2 * std::f64::consts::PI).sin()
        };
        // Away from the checker, or to the right if it is straight above.
        let away = match checkers.first() {
            Some(c) if c.file() > king.file() => -1.0,
            _ => 1.0,
        };
        // Seen from black's side the board is mirrored, and so is the fall.
        let away = if self.flipped { -away } else { away };

        let hidden = self.banner_hidden;
        Some(Finale {
            how,
            king,
            checkers,
            red,
            impact,
            dim: if hidden { 0.0 } else { topple * 0.55 },
            shake,
            fallen: away * fallen,
            banner: (!hidden && elapsed >= first + fall_len).then_some(reveal),
        })
    }

    /// The finale is over and its verdict is on the board.
    fn banner_showing(&self) -> bool {
        self.finale().is_some_and(|f| f.banner.is_some())
    }

    /// Sets a move travelling. The piece is read off the board after the move,
    /// so a promotion slides in as whatever it became.
    fn begin_slide(&mut self, m: Move) {
        // The finale, or the check's pulse, starts as the piece lands.
        if self.game.pos.is_checkmate() {
            self.ended = Some(self.now() + SLIDE);
        } else if self.game.pos.is_check() {
            self.checked = Some(self.now() + SLIDE);
        }
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
        self.send(Msg::Move(uci));
    }

    fn send(&self, msg: Msg) {
        self.table.send(msg.line());
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
        // During the finale a key skips to its end, and once the verdict is
        // up a key clears it away to look at the board.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let quitting = key.code == KeyCode::Char('q') || ctrl && key.code == KeyCode::Char('c');
        let finale = self
            .ending()
            .filter(|_| !quitting && self.ended.is_some() && !self.banner_hidden);
        if let Some(how) = finale {
            if self.is_animating() {
                self.slide = None;
                let now = self.now();
                self.ended = Some(now.checked_sub(how.length()).unwrap_or(now));
            } else {
                self.banner_hidden = true;
            }
            return;
        }
        if self.confirm_quit {
            // q again confirms, so a double tap still gets out quickly.
            self.confirm_quit = false;
            match key.code {
                KeyCode::Char('y' | 'q') | KeyCode::Enter => self.quit = true,
                KeyCode::Char('c') if ctrl => self.exit = true,
                _ => {}
            }
            return;
        }
        if self.confirm_resign {
            match key.code {
                KeyCode::Char('y') => {
                    self.confirm_resign = false;
                    let me = self.me.unwrap_or(self.game.turn());
                    self.game.resigned = Some(me);
                    self.ended = Some(self.now());
                    self.send(Msg::Resign);
                }
                _ => self.confirm_resign = false,
            }
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.exit = true,
            KeyCode::Char('q') => self.leave(),
            KeyCode::Esc => {
                if self.game.selected.is_some() {
                    self.game.selected = None;
                } else {
                    self.leave();
                }
            }
            KeyCode::Char('f') => self.flipped = !self.flipped,
            KeyCode::Char('p') => self.piece_style = self.piece_style.next(),
            KeyCode::Char('m') => self.mouse = !self.mouse,
            KeyCode::Char('c') => self.table.copy_share(),
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

    /// Back to the lobby, asking first if there is still a game to lose.
    fn leave(&mut self) {
        if self.game.over() || self.draw_agreed {
            self.quit = true;
        } else {
            self.confirm_quit = true;
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
            self.send(Msg::Draw);
        } else if self.table.is_networked() && !self.draw_sent {
            self.draw_sent = true;
            self.send(Msg::Draw);
        }
    }

    pub fn on_net(&mut self, event: NetEvent) {
        match event {
            NetEvent::Progress(p) => {
                if let Some(note) = self.table.on_progress(p, Kind::Chess) {
                    self.note = Some(note);
                }
            }
            NetEvent::Line(line) => {
                if let Some(msg) = Msg::parse(&line) {
                    self.on_msg(msg);
                }
            }
            NetEvent::Disconnected(why) => self.table.conn = Conn::Lost(why),
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Move(uci) => match self.game.play_uci(&uci) {
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
            Msg::Resign => {
                self.game.resigned = self.me.map(|me| !me);
                self.ended = Some(self.now());
            }
            // A draw message answers our own offer, or starts a new one.
            Msg::Draw => {
                if self.draw_sent {
                    self.draw_sent = false;
                    self.draw_agreed = true;
                } else {
                    self.draw_offered = true;
                }
            }
        }
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        let g = Geometry::new(self.area);
        if self.banner_showing() && matches!(ev.kind, MouseEventKind::Down(_)) {
            self.banner_hidden = true;
            return;
        }
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
        if self.confirm_resign || self.confirm_quit {
            self.confirm_resign = false;
            self.confirm_quit = false;
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
