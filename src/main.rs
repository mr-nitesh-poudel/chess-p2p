//! chess-p2p — terminal chess over a direct peer-to-peer connection.
//!
//!     chess-p2p              two players, one keyboard
//!     chess-p2p host         wait for an opponent, play white
//!     chess-p2p join <code>  dial an opponent, play black

use anyhow::Result;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
use shakmaty::Color;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use chess_p2p::app::{App, Conn};
use chess_p2p::net::{self, NetEvent};
use chess_p2p::ui;

/// Roughly 60 frames a second while something is moving.
const FRAME: std::time::Duration = std::time::Duration::from_millis(16);

const USAGE: &str = "\
chess-p2p — terminal chess over iroh

usage:
  chess-p2p                 play locally, two players on one keyboard
  chess-p2p host            host a game and print a code to share
  chess-p2p join <code>     join a game with the code your opponent sent
";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (net_tx, mut net_rx) = unbounded_channel();

    let app = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => App::local(),
        ["host"] => {
            let net = net::host(net_tx.clone()).await?;
            eprintln!("your code: {}", net.id);
            App::networked(net, Color::White, Conn::Waiting)
        }
        ["join", code] => {
            let peer: iroh::EndpointId = code
                .parse()
                .map_err(|_| anyhow::anyhow!("{code:?} is not a valid code"))?;
            let net = net::join(peer, net_tx.clone()).await?;
            App::networked(net, Color::Black, Conn::Dialling)
        }
        ["-h" | "--help" | "help"] => {
            print!("{USAGE}");
            return Ok(());
        }
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };

    let mut app = app;
    let key_rx = spawn_input();
    let result = run(&mut app, key_rx, &mut net_rx).await;

    if let Some(net) = app.net.take() {
        net.shutdown().await;
    }
    result
}

async fn run(
    app: &mut App,
    mut key_rx: tokio::sync::mpsc::UnboundedReceiver<Event>,
    net_rx: &mut tokio::sync::mpsc::UnboundedReceiver<NetEvent>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut capturing = false;
    let result = async {
        loop {
            // Mouse reporting steals the terminal's own text selection, so it
            // is a setting the player can turn off to copy their share code.
            if app.mouse != capturing {
                capturing = app.mouse;
                let _ = if capturing {
                    execute!(std::io::stdout(), EnableMouseCapture)
                } else {
                    execute!(std::io::stdout(), DisableMouseCapture)
                };
            }

            let size = terminal.size()?;
            app.area = Rect::new(0, 0, size.width, size.height);
            terminal.draw(|f| ui::draw(f, app))?;
            if app.quit {
                return Ok(());
            }
            // While a piece is travelling we redraw on a timer; the rest of
            // the time the loop sits idle waiting for someone to do something.
            let animating = app.is_animating();
            tokio::select! {
                // The event arms only ever redraw, so a closed channel just
                // means that source is done, not that the game is over.
                Some(ev) = key_rx.recv() => match ev {
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
    .await;
    if capturing {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    result
}

/// crossterm's reader is blocking, so it lives on its own thread and feeds the
/// async loop through a channel.
fn spawn_input() -> tokio::sync::mpsc::UnboundedReceiver<Event> {
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
