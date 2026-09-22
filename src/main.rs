//! chess-p2p — terminal chess over a direct peer-to-peer connection.
//!
//!     chess-p2p              the lobby: host, join, or share a keyboard
//!     chess-p2p local        two players, one keyboard
//!     chess-p2p host         wait for an opponent, play white
//!     chess-p2p join <code>  join with an opponent's code, play black

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
use shakmaty::Color;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use chess_p2p::app::{App, Conn};
use chess_p2p::clipboard;
use chess_p2p::lobby::{Choice, Lobby};
use chess_p2p::net::{self, NetEvent};
use chess_p2p::session::{Code, Target};
use chess_p2p::ui;

/// Roughly 60 frames a second while something is moving.
const FRAME: std::time::Duration = std::time::Duration::from_millis(16);

/// The longest quitting waits for the connection to close cleanly.
const SHUTDOWN: std::time::Duration = std::time::Duration::from_secs(2);

const USAGE: &str = "\
chess-p2p — terminal chess over iroh

usage:
  chess-p2p                 open the lobby to host, join, or play locally
  chess-p2p local           play locally, two players on one keyboard
  chess-p2p host            host a game and get a code to share
  chess-p2p join <code>     join a game with the code your opponent sent,
                            e.g. chess-p2p join 42-tiger-marble-ocean
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

    let mut input = spawn_input();
    let mut term = Term::new();
    let result = session(&mut term, &mut input, start).await;
    term.restore();
    result
}

/// From the lobby (unless the command line already chose) through one game.
async fn session(
    term: &mut Term,
    input: &mut UnboundedReceiver<Event>,
    start: Option<Choice>,
) -> Result<()> {
    let choice = match start {
        Some(choice) => choice,
        None => lobby(term, input).await?,
    };
    let (net_tx, mut net_rx) = unbounded_channel();
    let mut app = match choice {
        Choice::Quit => return Ok(()),
        Choice::Local => App::local(),
        Choice::Host => {
            let code = Code::generate();
            let net = net::host(code, true, net_tx).await?;
            let mut app = App::networked(net, Color::White, Conn::Publishing, code);
            // Hosting is for sending the code to someone, so have it ready.
            app.copy_share();
            app
        }
        Choice::Join(code) => {
            let net = net::join(code, Target::Lookup, net_tx).await?;
            App::networked(net, Color::Black, Conn::LookingUp, code)
        }
    };

    let result = play(term, &mut app, input, &mut net_rx).await;
    if let Some(net) = app.net.take() {
        // Closing politely tells the peer at once, but someone who has quit
        // should not be kept waiting on the network for it.
        let _ = tokio::time::timeout(SHUTDOWN, net.shutdown()).await;
    }
    result
}

async fn lobby(term: &mut Term, input: &mut UnboundedReceiver<Event>) -> Result<Choice> {
    let mut lobby = Lobby::new();
    term.set_mouse(true);
    loop {
        let size = term.terminal.size()?;
        lobby.area = Rect::new(0, 0, size.width, size.height);
        term.terminal.draw(|f| ui::draw_lobby(f, &lobby))?;

        let Some(ev) = input.recv().await else {
            return Ok(Choice::Quit);
        };
        let choice = match ev {
            Event::Key(key) => lobby.on_key(key),
            Event::Mouse(mouse) => lobby.on_mouse(mouse),
            Event::Paste(text) => {
                lobby.on_paste(&text);
                None
            }
            _ => None,
        };
        if let Some(choice) = choice {
            return Ok(choice);
        }
    }
}

async fn play(
    term: &mut Term,
    app: &mut App,
    input: &mut UnboundedReceiver<Event>,
    net_rx: &mut UnboundedReceiver<NetEvent>,
) -> Result<()> {
    loop {
        // Mouse reporting steals the terminal's own text selection, so it is
        // a setting the player can turn off to copy their share code by hand.
        term.set_mouse(app.mouse);
        if let Some(text) = app.copy_request.take() {
            app.copied = Some(clipboard::copy(&text));
        }

        let size = term.terminal.size()?;
        app.area = Rect::new(0, 0, size.width, size.height);
        term.terminal.draw(|f| ui::draw(f, app))?;
        if app.quit {
            return Ok(());
        }
        // While a piece is travelling we redraw on a timer; the rest of the
        // time the loop sits idle waiting for someone to do something.
        let animating = app.is_animating();
        tokio::select! {
            // The event arms only ever redraw, so a closed channel just means
            // that source is done, not that the game is over.
            Some(ev) = input.recv() => match ev {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                _ => {}
            },
            Some(ev) = net_rx.recv() => app.on_net(ev),
            _ = tokio::time::sleep(FRAME), if animating => {}
            else => return Ok(()),
        }
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
/// async loop through a channel.
fn spawn_input() -> UnboundedReceiver<Event> {
    let (tx, rx): (UnboundedSender<Event>, _) = unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if tx.send(ev).is_err() {
                break;
            }
        }
    });
    rx
}
