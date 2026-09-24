//! Analysis: talking to an engine, the bar, grading moves, looking back
//! through a game, and keeping the engine out of a game still being played.
//!
//! The engine here is a few lines of shell that speak just enough UCI, so
//! the tests need no Stockfish. It scores every position a little in favour
//! of the side to move, except the one after 3...Nf6??, where it sees the
//! mate.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use shakmaty::{Color, Square};
use tui_tui::games::chess::App;
use tui_tui::games::chess::analysis::{Grade, Thinking};
use tui_tui::games::chess::engine::{Engine, Score};
use tui_tui::games::chess::ui::Geometry;
use tui_tui::games::{Conn, Ctx, Seat, Table};

const FAKE: &str = r#"#!/bin/sh
fen=""
while read -r line; do
  case "$line" in
    uci) echo "id name fake"; echo "uciok" ;;
    isready) echo "readyok" ;;
    "position fen "*) fen="${line#position fen }" ;;
    go*)
      case "$fen" in
        "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w"*)
          echo "info depth 20 score mate 1 pv h5f7"; echo "bestmove h5f7" ;;
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w"*)
          echo "info depth 5 score cp 10 pv d2d4"
          echo "info depth 20 score cp 30 pv e2e4"; echo "bestmove e2e4" ;;
        *) echo "info depth 20 score cp 25"; echo "bestmove 0000" ;;
      esac ;;
    quit) exit 0 ;;
  esac
done
"#;

/// The fake engine, written somewhere of its own.
fn fake_engine(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tuitui-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("engine");
    std::fs::write(&path, FAKE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Waits for `ready`, failing loudly rather than hanging.
async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("gave up waiting for {what}");
}

fn press(t: &mut Table<App>, code: KeyCode) {
    t.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn play(app: &mut App, moves: &[&str]) {
    for m in moves {
        app.game.play_uci(m).unwrap();
    }
}

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[tokio::test]
async fn the_engine_says_how_good_a_position_is_and_what_to_play() {
    let engine = Engine::start(&fake_engine("basic"), None).unwrap();
    engine.focus(START.into());
    until("an evaluation", || {
        engine.eval(START).is_some_and(|e| e.depth == 20)
    })
    .await;
    let eval = engine.eval(START).unwrap();
    assert_eq!(eval.score, Score::Cp(30), "the deepest line wins");
    assert_eq!(eval.best.as_deref(), Some("e2e4"));
}

#[tokio::test]
async fn scores_are_for_white_whoever_is_to_move() {
    let mut app = App::local();
    play(&mut app, &["e2e4"]);
    app.engine = Some(Engine::start(&fake_engine("black"), None).unwrap());
    app.feed_engine();
    until("an evaluation", || app.score_at(1).is_some()).await;
    // Good for black, the side to move, so bad for white.
    assert_eq!(app.score_at(1).unwrap().0, Score::Cp(-25));
}

#[tokio::test]
async fn the_bar_leans_to_whoever_is_better_and_the_best_move_is_shown() {
    let mut t = Table::local(App::local());
    t.play.engine = Some(Engine::start(&fake_engine("bar"), None).unwrap());
    t.play.feed_engine();
    until("the best move", || t.play.best_move().is_some()).await;

    let best = t.play.best_move().unwrap();
    assert_eq!((best.from(), best.to()), (Some(Square::E2), Square::E4));
    // The bar sets off towards the score when it is first drawn, and a
    // moment later has settled there, leaning to white.
    let now = std::time::Instant::now();
    t.play.clock = Some(now);
    assert!((t.play.bar_share() - 0.5).abs() < 0.01, "it starts level");
    t.play.clock = Some(now + Duration::from_secs(1));
    let settled = t.play.bar_share();
    assert!(settled > 0.5, "white is better: {settled}");
    assert!(!t.play.bar_moving());

    // The bar is drawn, and has the score on it.
    let area = Rect::new(0, 0, 140, 44);
    t.set_area(area);
    let g = Geometry::for_game(area, &t.ctx, &t.play);
    let bar = g.eval.expect("a bar while analysing");
    assert!(bar.right() <= g.board.x && bar.x >= g.sidebar.right());
    let mut term = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    term.draw(|f| t.draw(f)).unwrap();
    let buf = term.backend().buffer();
    let column: String = (bar.y..bar.bottom())
        .flat_map(|y| (bar.x..bar.right()).map(move |x| (x, y)))
        .map(|at| buf[at].symbol().to_string())
        .collect();
    assert!(column.contains("0.3"), "{column}");
}

#[tokio::test]
async fn a_blunder_is_graded_once_both_sides_of_it_are_known() {
    let mut app = App::local();
    // 3...Nf6?? walks into mate.
    play(&mut app, &["e2e4", "e7e5", "d1h5", "b8c6", "f1c4", "g8f6"]);
    app.engine = Some(Engine::start(&fake_engine("grades"), None).unwrap());
    app.feed_engine();
    until("the game graded", || {
        (0..=6).all(|p| app.score_at(p).is_some())
    })
    .await;
    assert_eq!(app.grade(5), Some(Grade::Blunder), "Nf6");
    assert_eq!(app.grade(4), None, "Bc4 was fine");
}

#[test]
fn mate_is_scored_without_an_engine() {
    let mut app = App::local();
    play(&mut app, &["f2f3", "e7e5", "g2g4", "d8h4"]);
    let (score, _) = app.score_at(4).unwrap();
    assert!(score.white_share() < 0.01, "white is mated: {score:?}");
    assert!(app.score_at(3).is_none(), "the rest needs an engine");
}

#[test]
fn no_engine_is_offered_while_a_game_against_someone_is_on() {
    let mut t = Table::new(Ctx::new(Conn::Playing, None), App::new(Seat::Host));
    assert!(!t.play.can_analyse());
    press(&mut t, KeyCode::Char('a'));
    assert!(t.play.engine.is_none());
    assert_eq!(
        t.ctx.note.as_deref(),
        Some("analysis opens once the game is over")
    );

    // Once it is decided, it is.
    t.play.game.resigned = Some(Color::White);
    assert!(t.play.can_analyse());
    // And hot-seat, with both players here, may ask whenever.
    assert!(App::local().can_analyse());
}

#[tokio::test]
async fn an_engine_that_is_not_there_says_so() {
    let missing = std::env::temp_dir().join("tuitui-no-such-engine/stockfish");
    let why = Engine::start(&missing, None)
        .err()
        .expect("nothing to start");
    assert!(why.starts_with("could not start"), "{why}");
}

#[test]
fn an_engine_needs_somewhere_to_run() {
    // Outside the async runtime, as a test drawing the screen would be.
    let why = Engine::start(&fake_engine("no-runtime"), None).err();
    assert_eq!(why.as_deref(), Some("analysis needs the async runtime"));
}

#[test]
fn looking_back_steps_through_the_game_and_comes_back() {
    let mut t = Table::local(App::local());
    play(&mut t.play, &["e2e4", "e7e5", "g1f3"]);

    // Mid-game the arrows still move the cursor; , and . look back.
    let cursor = t.play.game.cursor;
    press(&mut t, KeyCode::Left);
    assert_ne!(t.play.game.cursor, cursor);
    assert_eq!(t.play.review, None);
    press(&mut t, KeyCode::Char(','));
    press(&mut t, KeyCode::Char(','));
    assert_eq!(t.play.review, Some(1));
    let then = t.play.reviewed().unwrap();
    assert!(then.piece_at(Square::E4).is_some() && then.piece_at(Square::E5).is_none());
    assert_eq!(then.last, Some((Square::E2, Square::E4)));

    // Picking a piece up comes back to the game, and does nothing else.
    press(&mut t, KeyCode::Enter);
    assert_eq!(t.play.review, None);
    assert_eq!(t.play.game.selected, None);

    // Esc comes back too, before it means leaving.
    press(&mut t, KeyCode::Home);
    assert_eq!(t.play.review, Some(0));
    press(&mut t, KeyCode::Esc);
    assert_eq!(t.play.review, None);
    assert!(!t.ctx.confirm_leave);

    // Stepping on past the end is the game as it is.
    press(&mut t, KeyCode::Char(','));
    press(&mut t, KeyCode::Char('.'));
    assert_eq!(t.play.review, None);
}

#[test]
fn once_the_game_is_over_the_arrows_replay_it() {
    let mut t = Table::local(App::local());
    play(&mut t.play, &["f2f3", "e7e5", "g2g4", "d8h4"]);
    t.play.banner_hidden = true;
    press(&mut t, KeyCode::Left);
    assert_eq!(t.play.review, Some(3));
    press(&mut t, KeyCode::Up);
    assert_eq!(t.play.review, Some(0));
    press(&mut t, KeyCode::Right);
    assert_eq!(t.play.review, Some(1));
    press(&mut t, KeyCode::Down);
    assert_eq!(t.play.review, None);
}

#[tokio::test]
async fn the_engine_follows_the_position_on_the_screen() {
    let mut t = Table::local(App::local());
    play(&mut t.play, &["e2e4"]);
    t.play.engine = Some(Engine::start(&fake_engine("follow"), None).unwrap());
    t.play.feed_engine();
    press(&mut t, KeyCode::Char(','));
    assert_eq!(t.play.shown_ply(), 0);
    until("the start searched", || {
        matches!(t.play.thinking(), Some(Thinking::Found { depth: 20, .. }))
    })
    .await;
    assert_eq!(t.play.score_at(0).unwrap().0, Score::Cp(30));
}

/// An engine of its own, from a script.
fn script(name: &str, body: &str) -> PathBuf {
    let path = fake_engine(name);
    std::fs::write(&path, body).unwrap();
    path
}

#[tokio::test]
async fn an_engine_that_dies_ends_analysis_and_says_so() {
    let path = script(
        "dies",
        "#!/bin/sh\nwhile read -r l; do case \"$l\" in uci) echo uciok;; go*) exit 3;; esac; done\n",
    );
    let mut app = App::local();
    app.engine = Some(Engine::start(&path, None).unwrap());
    app.feed_engine();
    until("the engine to be missed", || {
        matches!(app.thinking(), Some(Thinking::Failed(_)))
    })
    .await;
    let Some(Thinking::Failed(why)) = app.thinking() else {
        unreachable!()
    };
    assert!(why.contains("engine"), "{why}");
}

#[tokio::test]
async fn letting_go_of_the_engine_ends_its_process() {
    let pid_file = std::env::temp_dir().join(format!("tuitui-{}-engine.pid", std::process::id()));
    let body = format!(
        "#!/bin/sh\necho $$ > '{}'\nwhile read -r l; do case \"$l\" in uci) echo uciok;; esac; done\n",
        pid_file.display()
    );
    let engine = Engine::start(&script("pid", &body), None).unwrap();
    until("the engine to start", || {
        std::fs::read_to_string(&pid_file).is_ok_and(|s| !s.trim().is_empty())
    })
    .await;
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .to_string();
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .is_ok_and(|s| s.success())
    };
    assert!(alive(&pid));
    drop(engine);
    until("the process to be gone", || !alive(&pid)).await;
}
