//! The games, and what each has to offer the rest of the program.
//!
//! Everything outside this module treats a game as a [`Play`]: something that
//! takes keys, clicks and the peer's messages, and draws itself. Pairing,
//! invites and the lobby are shared, so a new game only needs its own rules,
//! screen and messages, plus an entry in [`Kind`].

pub mod chess;

use iroh::EndpointId;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;

use crate::clipboard::Copied;
use crate::net::{Net, NetEvent};
use crate::session::{self, Code, MAX_WRONG_CODES, Progress};

/// Every game there is to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Chess,
}

impl Kind {
    /// In the order the lobby offers them.
    pub const ALL: &[Kind] = &[Kind::Chess];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Chess => "Chess",
        }
    }

    /// The name and protocol version the two sides agree on when pairing.
    pub fn wire(self) -> session::Game {
        match self {
            Kind::Chess => chess::GAME,
        }
    }

    /// The game a peer offered, if this build can play it.
    pub fn from_wire(game: session::Game) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.wire() == game)
    }

    /// What pairing offers and accepts: every game this build can play.
    pub fn all_wire() -> Vec<session::Game> {
        Kind::ALL.iter().map(|k| k.wire()).collect()
    }

    /// A new game, sat at `seat`.
    pub fn start(self, seat: Seat, table: Table) -> Box<dyn Play> {
        match self {
            Kind::Chess => Box::new(chess::App::new(seat, table)),
        }
    }
}

/// Where a player sits, which decides who goes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    /// Both players at this keyboard.
    Local,
    /// The one who hosted, or was invited.
    Host,
    /// The one who joined, or invited.
    Guest,
}

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

/// Who is across the table, and how to reach them. The same for every game.
pub struct Table {
    pub conn: Conn,
    pub net: Option<Net>,
    pub peer: Option<EndpointId>,
    /// What the peer calls itself.
    pub peer_name: Option<String>,
    /// The code to show for someone to join with, when hosting.
    pub share: Option<String>,
    /// Text waiting for the main loop to put on the clipboard.
    pub copy_request: Option<String>,
    /// How the last copy of the share code went.
    pub copied: Option<Copied>,
}

impl Table {
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
        }
    }

    /// The opponent is in.
    pub fn attach(&mut self, net: Net, name: &str) {
        self.peer = Some(net.peer);
        self.peer_name = Some(name.to_string());
        self.net = Some(net);
        self.conn = Conn::Playing;
    }

    pub fn is_networked(&self) -> bool {
        self.net.is_some()
    }

    /// Tell the opponent something, if there is one.
    pub fn send(&self, line: String) {
        if let Some(net) = &self.net {
            net.send(line);
        }
    }

    /// Ask for the share code to go on the clipboard, while it is still of use.
    pub fn copy_share(&mut self) {
        if matches!(self.conn, Conn::Publishing | Conn::Waiting) {
            self.copy_request = self.share.clone();
        }
    }

    /// How to refer to the opponent: their name, else a short id.
    pub fn peer_label(&self) -> String {
        if let Some(name) = &self.peer_name {
            return name.clone();
        }
        self.peer
            .map(|p| p.fmt_short().to_string())
            .unwrap_or_else(|| "—".into())
    }

    /// Moves the connection along. Returns anything worth telling the
    /// player about.
    pub fn on_progress(&mut self, progress: Progress, game: Kind) -> Option<String> {
        match progress {
            Progress::Listed => self.conn = Conn::Waiting,
            Progress::Found => self.conn = Conn::Dialling,
            Progress::WrongCode { attempts } => {
                return Some(format!(
                    "someone tried a wrong code ({attempts} of {MAX_WRONG_CODES})"
                ));
            }
            Progress::Declined => {
                return Some(format!(
                    "someone joined who cannot play {}",
                    game.name().to_lowercase()
                ));
            }
        }
        None
    }
}

/// Where a game wants to go when it is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leave {
    /// Back to the lobby.
    Lobby,
    /// Out of the program altogether.
    Exit,
}

/// A game in progress, as the main loop sees it.
pub trait Play {
    fn kind(&self) -> Kind;
    fn table(&self) -> &Table;
    fn table_mut(&mut self) -> &mut Table;

    fn on_key(&mut self, key: KeyEvent);
    fn on_mouse(&mut self, ev: MouseEvent);
    fn on_net(&mut self, ev: NetEvent);
    fn draw(&self, f: &mut Frame);
    /// The terminal's size, for turning clicks into places on the screen.
    fn set_area(&mut self, area: Rect);

    /// Whether to redraw on a timer, rather than only when something happens.
    fn is_animating(&self) -> bool {
        false
    }
    /// Mouse reporting steals the terminal's own text selection, so a game
    /// can let the player turn it off.
    fn wants_mouse(&self) -> bool {
        true
    }
    fn leaving(&self) -> Option<Leave>;

    /// The opponent is in.
    fn attach(&mut self, net: Net, name: &str) {
        self.table_mut().attach(net, name);
    }
}
