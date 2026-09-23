//! The table a game is played at: the connection to the opponent, and
//! everything about playing that is the same whatever the game.

use iroh::EndpointId;
use ratatui::Frame;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use super::{Handled, Kind, Play};
use crate::clipboard::Copied;
use crate::net::{Net, NetEvent};
use crate::session::{Code, MAX_WRONG_CODES, Progress};

/// How the connection to the opponent is going.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conn {
    /// Hot-seat: no network at all.
    Local,
    /// Hosting, putting the code on the DHT.
    Publishing,
    /// Hosting, with the code published and nobody in yet.
    Waiting,
    /// Joining, finding the host behind the code.
    LookingUp,
    /// Joining, found the host and pairing with it.
    Dialling,
    /// Waiting for this friend to answer our invite.
    Inviting(String),
    Playing,
    Lost(String),
}

/// Where a game is going once the player is done with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leave {
    /// Back to the lobby.
    Lobby,
    /// Out of the program altogether.
    Exit,
}

/// What a game can see and use beyond itself: the opponent, the connection,
/// and the parts of the screen every game shares.
pub struct Ctx {
    pub conn: Conn,
    net: Option<Net>,
    pub peer: Option<EndpointId>,
    /// What the peer calls itself, cleaned when it arrived.
    pub peer_name: Option<String>,
    /// The code to show for someone to join with, when hosting.
    pub share: Option<String>,
    /// Text waiting for the main loop to put on the clipboard.
    pub copy_request: Option<String>,
    /// How the last copy of the share code went.
    pub copied: Option<Copied>,
    /// Something to tell the player, until their next key.
    pub note: Option<String>,
    /// The terminal's size, for turning clicks into places on the screen.
    pub area: Rect,
    /// Whether the terminal reports the mouse to us. Doing so takes away its
    /// own text selection, so the player can turn it off.
    pub mouse: bool,
    /// Asked to leave a game still in play, and not answered yet.
    pub confirm_leave: bool,
}

impl Ctx {
    /// Nobody to connect to: both players are here.
    pub fn local() -> Self {
        Self::new(Conn::Local, None)
    }

    /// Someone who is not connected yet. `code` is what to show for them to
    /// join with, when hosting.
    pub fn new(conn: Conn, code: Option<Code>) -> Self {
        Self {
            conn,
            net: None,
            peer: None,
            peer_name: None,
            share: code.map(|c| c.to_string()),
            copy_request: None,
            copied: None,
            note: None,
            area: Rect::new(0, 0, 80, 24),
            mouse: true,
            confirm_leave: false,
        }
    }

    /// Whether there is an opponent on the other end of a connection.
    pub fn is_networked(&self) -> bool {
        self.net.is_some()
    }

    /// Tells the opponent something, if there is one. One message a line: a
    /// line with a newline in it would arrive as two, so it is not sent.
    pub fn send(&self, line: String) {
        debug_assert!(!line.contains('\n'), "a message is one line: {line:?}");
        if line.contains('\n') {
            return;
        }
        if let Some(net) = &self.net {
            net.send(line);
        }
    }

    /// How to refer to the opponent: their name, else a short id.
    pub fn peer_label(&self) -> String {
        if let Some(name) = self.peer_name.as_ref().filter(|n| !n.is_empty()) {
            return name.clone();
        }
        self.peer
            .map(|p| p.fmt_short().to_string())
            .unwrap_or_else(|| "—".into())
    }

    /// Asks for the share code to go on the clipboard, while it is still of
    /// use.
    pub fn copy_share(&mut self) {
        if matches!(self.conn, Conn::Publishing | Conn::Waiting) {
            self.copy_request = self.share.clone();
        }
    }

    /// The opponent is in.
    fn attach(&mut self, net: Net, name: &str) {
        self.peer = Some(net.peer);
        self.peer_name = Some(name.to_string());
        self.net = Some(net);
        self.conn = Conn::Playing;
    }

    /// Moves the connection along, and says anything worth telling the
    /// player.
    fn on_progress(&mut self, progress: Progress, game: Kind) {
        match progress {
            Progress::Listed => self.conn = Conn::Waiting,
            Progress::Found => self.conn = Conn::Dialling,
            Progress::WrongCode { attempts } => {
                self.note = Some(format!(
                    "someone tried a wrong code ({attempts} of {MAX_WRONG_CODES})"
                ));
            }
            Progress::Declined => {
                self.note = Some(format!(
                    "someone joined who cannot play {}",
                    game.name().to_lowercase()
                ));
            }
        }
    }
}

/// A game, and the table it is played at.
///
/// The main loop holds a `Box<Table<dyn Play>>` and never needs to know which
/// game is on it. A test can hold a `Table` of a particular game instead, to
/// look at that game's state while still going through the table.
pub struct Table<G: ?Sized> {
    pub ctx: Ctx,
    leaving: Option<Leave>,
    pub play: G,
}

impl<G: Play> Table<G> {
    pub fn new(ctx: Ctx, play: G) -> Self {
        Self {
            ctx,
            leaving: None,
            play,
        }
    }

    /// Both players at this keyboard, with nobody to connect to.
    pub fn local(play: G) -> Self {
        Self::new(Ctx::local(), play)
    }
}

impl<G: Play + ?Sized> Table<G> {
    pub fn kind(&self) -> Kind {
        self.play.kind()
    }

    /// Where the player is going, once they have decided.
    pub fn leaving(&self) -> Option<Leave> {
        self.leaving
    }

    pub fn wants_mouse(&self) -> bool {
        self.ctx.mouse
    }

    pub fn is_animating(&self) -> bool {
        self.play.is_animating()
    }

    pub fn set_area(&mut self, area: Rect) {
        self.ctx.area = area;
    }

    /// The opponent is in.
    pub fn attach(&mut self, net: Net, name: &str) {
        self.ctx.attach(net, name);
    }

    /// The connection could not be made, or went.
    pub fn lost(&mut self, why: String) {
        self.ctx.conn = Conn::Lost(why);
    }

    pub fn draw(&self, f: &mut Frame) {
        self.play.draw(f, &self.ctx);
    }

    /// A key goes to the table's own question if it is asking one, then to
    /// the game, and then — if the game had no use for it — to the table.
    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.leaving = Some(Leave::Exit);
            return;
        }
        if self.ctx.confirm_leave {
            self.ctx.confirm_leave = false;
            // q again confirms, so a double tap still gets out quickly.
            if matches!(key.code, KeyCode::Char('y' | 'q') | KeyCode::Enter) {
                self.leaving = Some(Leave::Lobby);
            }
            return;
        }
        self.ctx.note = None;
        if self.play.on_key(key, &mut self.ctx) == Handled::Used {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.leave(),
            KeyCode::Char('m') => self.ctx.mouse = !self.ctx.mouse,
            KeyCode::Char('c') => self.ctx.copy_share(),
            _ => {}
        }
    }

    /// A click while the table is asking a question dismisses it; anything
    /// else is the game's.
    pub fn on_mouse(&mut self, ev: MouseEvent) {
        if self.ctx.confirm_leave {
            if matches!(ev.kind, MouseEventKind::Down(_)) {
                self.ctx.confirm_leave = false;
            }
            return;
        }
        self.play.on_mouse(ev, &mut self.ctx);
    }

    pub fn on_net(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Progress(p) => {
                let kind = self.kind();
                self.ctx.on_progress(p, kind);
            }
            NetEvent::Line(line) => self.play.on_line(&line, &mut self.ctx),
            NetEvent::Disconnected(why) => self.lost(why),
        }
    }

    /// Back to the lobby, asking first if there is still a game to lose.
    fn leave(&mut self) {
        if self.play.in_play() {
            self.ctx.confirm_leave = true;
        } else {
            self.leaving = Some(Leave::Lobby);
        }
    }
}
