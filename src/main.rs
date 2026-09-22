//! tui-tui — terminal chess over a direct peer-to-peer connection.
//!
//!     tui-tui              the lobby: host, join, challenge a friend
//!     tui-tui local        two players, one keyboard
//!     tui-tui host         wait for an opponent, play white
//!     tui-tui join <code>  join with an opponent's code, play black

use std::time::Duration;

use anyhow::Result;
use iroh::{Endpoint, EndpointId};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
use shakmaty::Color;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use tui_tui::app::{App, Conn};
use tui_tui::clipboard;
use tui_tui::lobby::{Choice, Lobby, Row};
use tui_tui::net::{self, CHESS, NetEvent};
use tui_tui::profile::Profile;
use tui_tui::session::{self, Code, Incoming, Invite, Link, Listener, Target};
use tui_tui::ui;

/// Roughly 60 frames a second while something is moving.
const FRAME: Duration = Duration::from_millis(16);

/// The longest quitting waits for connections to close cleanly.
const SHUTDOWN: Duration = Duration::from_secs(2);

const USAGE: &str = "\
tui-tui — terminal chess over iroh

usage:
  tui-tui                 open the lobby to host, join, or challenge a friend
  tui-tui local           play locally, two players on one keyboard
  tui-tui host            host a game and get a code to share
  tui-tui join <code>     join a game with the code your opponent sent,
                          e.g. tui-tui join 42-tiger-marble-ocean

Your profile (identity, name, friends) is kept in tui-tui under your
config directory, or in $TUI_TUI_HOME if that is set.
";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Settled before the terminal is taken over, so a bad code or a typo in
    // the command is reported on a normal screen.
    let start = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => None,
        ["local"] => Some(Choice::Local),
        ["host"] => Some(Choice::Host),
        // Spaces are fine too, as in `join 42 tiger marble ocean`.
        ["join", words @ ..] if !words.is_empty() => Some(Choice::Join(words.join("-").parse()?)),
        ["-h" | "--help" | "help"] => {
            print!("{USAGE}");
            return Ok(());
        }
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };

    let (profile, notice) = match Profile::load() {
        Ok(profile) => (profile, None),
        Err(e) => (Profile::guest(), Some(format!("playing as a guest: {e:#}"))),
    };
    let endpoint = session::bind(profile.secret.clone()).await?;

    let (wake, mut woken) = unbounded_channel();
    spawn_input(wake.clone());
    let (heard, mut hearing) = unbounded_channel();
    let listener = Listener::start(endpoint.clone(), &[CHESS], heard);
    tokio::spawn({
        let wake = wake.clone();
        async move {
            while let Some(incoming) = hearing.recv().await {
                let _ = wake.send(Wake::Heard(incoming));
            }
        }
    });

    let mut hub = Hub {
        profile,
        endpoint: endpoint.clone(),
        listener,
        wake,
        next_match: 0,
        invite: None,
        notice,
    };
    let mut term = Term::new();
    let result = hub.run(&mut term, &mut woken, start).await;
    term.restore();
    // Dropping the hub ends any game, which says goodbye to the peer on its
    // way out; closing the endpoint gives that a moment to be sent.
    drop(hub);
    let _ = tokio::time::timeout(SHUTDOWN, endpoint.close()).await;
    result
}

/// Everything that can wake the main loop.
enum Wake {
    Input(Event),
    Heard(Incoming),
    /// A code joined, or a friend invited, for the match with this id.
    Paired(u64, Result<Link, String>),
    Net(u64, NetEvent),
}

enum Screen {
    Lobby(Lobby),
    Match(Box<Match>),
}

/// One game, from the moment it is chosen in the lobby until it is left.
struct Match {
    /// Tells this match's events from those of matches already left.
    id: u64,
    app: App,
    /// Whether the listener's next [`Incoming::Paired`] is for us.
    by_listener: bool,
    /// Our own dialling out, if that is how this match is being paired.
    dialling: Option<JoinHandle<()>>,
}

impl Drop for Match {
    fn drop(&mut self) {
        if let Some(task) = self.dialling.take() {
            task.abort();
        }
    }
}

enum Next {
    Stay,
    Go(Screen),
    Exit,
}

struct Hub {
    profile: Profile,
    endpoint: Endpoint,
    listener: Listener,
    wake: UnboundedSender<Wake>,
    next_match: u64,
    /// A friend's invite, while the lobby asks about it.
    invite: Option<Invite>,
    /// Something for the next lobby to say.
    notice: Option<String>,
}

impl Hub {
    async fn run(
        &mut self,
        term: &mut Term,
        woken: &mut UnboundedReceiver<Wake>,
        start: Option<Choice>,
    ) -> Result<()> {
        let mut screen = match start {
            None => self.lobby(None),
            Some(choice) => match self.choose(choice, &mut Lobby::new()) {
                Next::Go(screen) => screen,
                Next::Stay => self.lobby(None),
                Next::Exit => return Ok(()),
            },
        };

        loop {
            let size = term.terminal.size()?;
            let area = Rect::new(0, 0, size.width, size.height);
            match &mut screen {
                Screen::Lobby(lobby) => {
                    term.set_mouse(true);
                    lobby.area = area;
                    term.terminal.draw(|f| ui::draw_lobby(f, lobby))?;
                }
                Screen::Match(m) => {
                    if m.app.exit {
                        return Ok(());
                    }
                    if m.app.quit {
                        let last = m.app.peer;
                        screen = self.lobby(last);
                        continue;
                    }
                    // Mouse reporting steals the terminal's own text
                    // selection, so it is a setting the player can turn off.
                    term.set_mouse(m.app.mouse);
                    if let Some(text) = m.app.copy_request.take() {
                        m.app.copied = Some(clipboard::copy(&text));
                    }
                    m.app.area = area;
                    term.terminal.draw(|f| ui::draw(f, &m.app))?;
                }
            }

            // While a piece is travelling we redraw on a timer; the rest of
            // the time the loop sits idle waiting for something to happen.
            let animating = matches!(&screen, Screen::Match(m) if m.app.is_animating());
            let wake = tokio::select! {
                wake = woken.recv() => match wake {
                    Some(wake) => wake,
                    None => return Ok(()),
                },
                _ = tokio::time::sleep(FRAME), if animating => continue,
            };

            let next = match (wake, &mut screen) {
                (Wake::Input(ev), Screen::Lobby(lobby)) => {
                    let choice = match ev {
                        Event::Key(key) => lobby.on_key(key),
                        Event::Mouse(mouse) => lobby.on_mouse(mouse),
                        Event::Paste(text) => {
                            lobby.on_paste(&text);
                            None
                        }
                        _ => None,
                    };
                    match choice {
                        Some(choice) => self.choose(choice, lobby),
                        None => Next::Stay,
                    }
                }
                (Wake::Input(ev), Screen::Match(m)) => {
                    match ev {
                        Event::Key(key) => m.app.on_key(key),
                        Event::Mouse(mouse) => m.app.on_mouse(mouse),
                        _ => {}
                    }
                    Next::Stay
                }
                (Wake::Heard(incoming), screen) => {
                    self.heard(incoming, screen);
                    Next::Stay
                }
                (Wake::Paired(id, result), Screen::Match(m)) if m.id == id => {
                    m.dialling = None;
                    match result {
                        Ok(link) => self.play(m, link),
                        Err(why) => m.app.conn = Conn::Lost(why),
                    }
                    Next::Stay
                }
                (Wake::Net(id, ev), Screen::Match(m)) if m.id == id => {
                    m.app.on_net(ev);
                    Next::Stay
                }
                // Left over from a match that has been left.
                _ => Next::Stay,
            };
            match next {
                Next::Stay => {}
                Next::Go(next) => screen = next,
                Next::Exit => return Ok(()),
            }
        }
    }

    /// A fresh lobby, with the last opponent picked out for a rematch.
    fn lobby(&mut self, last: Option<EndpointId>) -> Screen {
        self.listener.idle();
        let mut lobby = Lobby::new();
        lobby.name = self.profile.name.clone();
        lobby.guest = self.profile.is_guest();
        lobby.set_friends(self.profile.contacts.clone());
        lobby.notice = self.notice.take();
        if let Some(i) = last.and_then(|id| self.profile.contacts.iter().position(|c| c.id == id)) {
            let rows = lobby.rows();
            if let Some(row) = rows.iter().position(|&r| r == Row::Friend(i)) {
                lobby.selected = row;
            }
        }
        Screen::Lobby(lobby)
    }

    fn choose(&mut self, choice: Choice, lobby: &mut Lobby) -> Next {
        // Starting anything means we cannot also answer a friend.
        let starts_match = matches!(
            choice,
            Choice::Host
                | Choice::Join(_)
                | Choice::Local
                | Choice::Challenge(_)
                | Choice::AcceptInvite
        );
        if starts_match
            && !matches!(choice, Choice::AcceptInvite)
            && let Some(invite) = self.invite.take()
        {
            invite.refuse("busy");
        }

        match choice {
            Choice::Quit => Next::Exit,
            Choice::Local => {
                self.listener.busy();
                Next::Go(Screen::Match(Box::new(self.new_match(App::local(), false))))
            }
            Choice::Host => Next::Go(Screen::Match(Box::new(self.host()))),
            Choice::Join(code) => Next::Go(Screen::Match(Box::new(self.join(code)))),
            Choice::Challenge(id) => Next::Go(Screen::Match(Box::new(self.challenge(id)))),
            Choice::AcceptInvite => {
                let Some(invite) = self.invite.take() else {
                    return Next::Stay;
                };
                self.listener.busy();
                invite.accept(&self.profile.name);
                let app = App::networked(Color::White, Conn::Dialling, None);
                Next::Go(Screen::Match(Box::new(self.new_match(app, true))))
            }
            Choice::DeclineInvite => {
                if let Some(invite) = self.invite.take() {
                    invite.refuse("declined");
                }
                Next::Stay
            }
            Choice::Rename(name) => {
                match self.profile.set_name(&name) {
                    Ok(()) => lobby.name = self.profile.name.clone(),
                    Err(e) => lobby.notice = Some(format!("{e:#}")),
                }
                Next::Stay
            }
            Choice::Forget(id) => {
                let name = self.profile.contact(id).map(|c| c.name.clone());
                if let Err(e) = self.profile.forget(id) {
                    lobby.notice = Some(format!("{e:#}"));
                } else if let Some(name) = name {
                    lobby.notice = Some(format!("forgot {name}"));
                }
                lobby.set_friends(self.profile.contacts.clone());
                Next::Stay
            }
        }
    }

    fn new_match(&mut self, app: App, by_listener: bool) -> Match {
        self.next_match += 1;
        Match {
            id: self.next_match,
            app,
            by_listener,
            dialling: None,
        }
    }

    fn host(&mut self) -> Match {
        let code = Code::generate();
        self.listener.host(code, CHESS, &self.profile.name, true);
        let mut app = App::networked(Color::White, Conn::Publishing, Some(code));
        // Hosting is for sending the code to someone, so have it ready.
        app.copy_share();
        self.new_match(app, true)
    }

    fn join(&mut self, code: Code) -> Match {
        self.listener.busy();
        let app = App::networked(Color::Black, Conn::LookingUp, None);
        let mut m = self.new_match(app, false);
        let (id, endpoint, wake, name) = (
            m.id,
            self.endpoint.clone(),
            self.wake.clone(),
            self.profile.name.clone(),
        );
        m.dialling = Some(tokio::spawn(async move {
            let progress = {
                let wake = wake.clone();
                move |p| {
                    let _ = wake.send(Wake::Net(id, NetEvent::Progress(p)));
                }
            };
            let result =
                session::join(&endpoint, code, &[CHESS], Target::Lookup, &name, progress).await;
            let result = result.map_err(|e| session::explain(&e, "your opponent"));
            let _ = wake.send(Wake::Paired(id, result));
        }));
        m
    }

    fn challenge(&mut self, friend: EndpointId) -> Match {
        self.listener.busy();
        let their_name = self
            .profile
            .contact(friend)
            .map_or_else(|| friend.fmt_short().to_string(), |c| c.name.clone());
        let app = App::networked(Color::Black, Conn::Inviting(their_name.clone()), None);
        let mut m = self.new_match(app, false);
        let (id, endpoint, wake, name) = (
            m.id,
            self.endpoint.clone(),
            self.wake.clone(),
            self.profile.name.clone(),
        );
        m.dialling = Some(tokio::spawn(async move {
            let result = session::invite(&endpoint, friend, CHESS, &name).await;
            let result = result.map_err(|e| session::explain(&e, &their_name));
            let _ = wake.send(Wake::Paired(id, result));
        }));
        m
    }

    fn heard(&mut self, incoming: Incoming, screen: &mut Screen) {
        let waiting = match screen {
            Screen::Match(m) if m.by_listener && m.app.net.is_none() => Some(m),
            _ => None,
        };
        match incoming {
            Incoming::Invite(invite) => {
                let Some(friend) = self.profile.contact(invite.peer) else {
                    invite.refuse("unknown");
                    return;
                };
                match screen {
                    Screen::Lobby(lobby) if self.invite.is_none() => {
                        lobby.invite = Some(friend.name.clone());
                        self.invite = Some(invite);
                    }
                    _ => invite.refuse("busy"),
                }
            }
            Incoming::InviteGone(peer) => {
                if self.invite.as_ref().is_some_and(|i| i.peer == peer) {
                    self.invite = None;
                    if let Screen::Lobby(lobby) = screen {
                        let name = lobby.invite.take().unwrap_or_default();
                        lobby.notice = Some(format!("{name}'s invite ran out"));
                    }
                }
            }
            Incoming::Paired(link) => match waiting {
                Some(m) => self.play(m, link),
                // Nobody here is waiting for it any more; dropping it hangs up.
                None => drop(link),
            },
            Incoming::Progress(p) => {
                if let Some(m) = waiting {
                    m.app.on_net(NetEvent::Progress(p));
                }
            }
            Incoming::Closed(why) => {
                if let Some(m) = waiting {
                    m.app.conn = Conn::Lost(why);
                }
            }
        }
    }

    /// The opponent is in: remember them, and start playing.
    fn play(&mut self, m: &mut Match, link: Link) {
        if let Err(e) = self.profile.played(link.peer, &link.peer_name) {
            self.notice = Some(format!("could not save your friends: {e:#}"));
        }
        let name = self
            .profile
            .contact(link.peer)
            .map_or_else(|| link.peer_name.clone(), |c| c.name.clone());

        let (events, mut heard) = unbounded_channel();
        let (id, wake) = (m.id, self.wake.clone());
        tokio::spawn(async move {
            while let Some(ev) = heard.recv().await {
                let _ = wake.send(Wake::Net(id, ev));
            }
        });
        m.app.attach(net::play(link, events), &name);
    }
}

/// The terminal, and the modes we have switched on in it that need switching
/// off again on the way out.
struct Term {
    terminal: DefaultTerminal,
    capturing: bool,
}

impl Term {
    fn new() -> Self {
        let terminal = ratatui::init();
        // A pasted code then arrives as one event rather than as keystrokes,
        // some of which the lobby would take as commands.
        let _ = execute!(std::io::stdout(), EnableBracketedPaste);
        Self {
            terminal,
            capturing: false,
        }
    }

    fn set_mouse(&mut self, on: bool) {
        if on == self.capturing {
            return;
        }
        self.capturing = on;
        let _ = if on {
            execute!(std::io::stdout(), EnableMouseCapture)
        } else {
            execute!(std::io::stdout(), DisableMouseCapture)
        };
    }

    fn restore(mut self) {
        self.set_mouse(false);
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

/// crossterm's reader is blocking, so it lives on its own thread and feeds the
/// async loop through the same channel as everything else.
fn spawn_input(wake: UnboundedSender<Wake>) {
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if wake.send(Wake::Input(ev)).is_err() {
                break;
            }
        }
    });
}
