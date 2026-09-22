//! Renders the UI to an in-memory terminal and prints it, so the layout can be
//! checked without a human at a keyboard. `cargo run --example render`

use chess_p2p::app::{App, Conn};
use chess_p2p::clipboard::Copied;
use chess_p2p::lobby::{Field, Lobby};
use chess_p2p::profile::Contact;
use chess_p2p::ui::{self, Geometry, PieceStyle};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color as Paint;
use shakmaty::{Color, Square};

fn dump(label: &str, app: &App, w: u16, h: u16) {
    print_frame(label, w, h, |f| ui::draw(f, app));
}

fn friend(name: &str, games: u32, last_played: u64) -> Contact {
    Contact {
        id: iroh::SecretKey::generate().public(),
        name: name.into(),
        games,
        last_played,
    }
}

fn dump_lobby(label: &str, lobby: &Lobby, w: u16, h: u16) {
    print_frame(label, w, h, |f| ui::draw_lobby(f, lobby));
}

fn print_frame(label: &str, w: u16, h: u16, draw: impl FnOnce(&mut ratatui::Frame)) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer().clone();

    println!("\n=== {label} ({w}x{h}) ===");
    for y in 0..h {
        let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
        println!("|{}|", row.trim_end());
    }
}

/// Reads the half-block sprites back out of the rendered buffer, so what is
/// printed here is what the terminal was actually told to draw.
fn sprites(app: &App, w: u16, h: u16, squares: &[Square]) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let g = Geometry::new(Rect::new(0, 0, w, h));
    let (cw, ch) = g.cell;

    let ink = |c: Paint| match c {
        Paint::Rgb(242, 239, 232) | Paint::Rgb(40, 36, 33) => 'O',
        Paint::Rgb(46, 40, 35) | Paint::Rgb(201, 194, 182) => '#',
        _ => '.',
    };

    println!("\n=== sprites at {cw}x{ch} ({w}x{h}) ===");
    let mut grids: Vec<Vec<String>> = Vec::new();
    for sq in squares {
        let col = u16::from(sq.file() as u8);
        let row = 7 - u16::from(sq.rank() as u8);
        let mut rows = Vec::new();
        for sub in 0..ch {
            let (mut top, mut bottom) = (String::new(), String::new());
            for x in 0..cw {
                let cell = &buf[(g.grid.x + col * cw + x, g.grid.y + row * ch + sub)];
                if cell.symbol() == "▀" {
                    top.push(ink(cell.fg));
                    bottom.push(ink(cell.bg));
                } else {
                    top.push('.');
                    bottom.push('.');
                }
            }
            rows.push(top);
            rows.push(bottom);
        }
        grids.push(rows);
    }
    for i in 0..grids[0].len() {
        let line: Vec<&str> = grids.iter().map(|gr| gr[i].as_str()).collect();
        println!("  {}", line.join("  "));
    }
}

fn main() {
    let mut app = App::local();
    for m in [
        "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "g8f6", "e1g1", "f6e4",
    ] {
        app.game.play_uci(m).unwrap();
    }
    app.game.cursor = Square::D2;
    app.game.activate(None);

    let back = [
        Square::A1,
        Square::B1,
        Square::C1,
        Square::D1,
        Square::E1,
        Square::F1,
        Square::A2,
    ];
    sprites(&App::local(), 120, 40, &back);
    sprites(&App::local(), 100, 30, &back);

    let mut lettered = App::local();
    lettered.piece_style = PieceStyle::BigLetter;
    sprites(&lettered, 120, 40, &back);

    // The plain-text dumps only make sense with character pieces.
    app.piece_style = PieceStyle::Art;
    dump("big terminal", &app, 120, 40);
    dump("classic 80x24", &app, 80, 24);

    let mut hosting = App::local();
    hosting.piece_style = PieceStyle::Art;
    hosting.me = Some(Color::White);
    hosting.conn = Conn::Waiting;
    hosting.share = Some("42-tiger-marble-ocean".into());
    hosting.copied = Some(Copied::Terminal);
    dump("hosting, waiting", &hosting, 100, 30);

    let mut lobby = Lobby::new();
    lobby.name = "ace".into();
    dump_lobby("lobby, first run", &lobby, 80, 30);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    lobby.set_friends(vec![
        friend("alice", 3, now - 3 * 3600),
        friend("bob", 1, now - 30 * 3600),
        friend("a very long name indeed, really", 12, now - 20 * 86400),
    ]);
    lobby.selected = 3;
    dump_lobby("lobby with friends", &lobby, 80, 30);
    lobby.invite = Some("alice".into());
    dump_lobby("an invite", &lobby, 80, 30);
    lobby.invite = None;

    lobby.set_friends(vec![]);
    lobby.editing = Some(Field::Code);
    lobby.selected = 1;
    lobby.input = "42-tiger-mar".into();
    dump_lobby("lobby, typing a code", &lobby, 80, 24);
    lobby.input = "42-tiger-xylo".into();
    dump_lobby("lobby, a word that is not one", &lobby, 60, 20);

    let mut promo = App::local();
    promo.piece_style = PieceStyle::Art;
    for m in [
        "d2d4", "e7e5", "d4e5", "d7d5", "e5d6", "g8f6", "d6c7", "a7a6",
    ] {
        promo.game.play_uci(m).unwrap();
    }
    promo.game.cursor = Square::C7;
    promo.game.activate(None);
    promo.game.cursor = Square::B8;
    promo.game.activate(None);
    dump("promotion prompt", &promo, 100, 30);
}
