//! Rules and wire-format checks that do not need a terminal.

use shakmaty::{Color, Position, Role, Square};

use chess_p2p::game::{Game, ui_to};

/// Walk a game in through the same path a peer's moves take.
fn play(g: &mut Game, moves: &[&str]) {
    for m in moves {
        g.play_uci(m).unwrap_or_else(|e| panic!("{m}: {e}"));
    }
}

#[test]
fn rejects_illegal_and_malformed_moves() {
    let mut g = Game::new();
    assert!(g.play_uci("e2e5").is_err(), "pawns do not jump three");
    assert!(g.play_uci("hello").is_err());
    assert!(g.play_uci("e7e5").is_err(), "not black's turn");
    assert_eq!(
        g.history.len(),
        0,
        "a rejected move must not touch the game"
    );
    g.play_uci("e2e4").unwrap();
    assert_eq!(g.history, ["e4"]);
}

#[test]
fn castling_lands_on_the_king_square() {
    let mut g = Game::new();
    play(&mut g, &["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5"]);

    let castle = g
        .moves_from(Square::E1)
        .into_iter()
        .find(|m| matches!(m, shakmaty::Move::Castle { .. }))
        .expect("white can castle short here");

    // shakmaty reports the rook square; the board must show the king square.
    assert_eq!(castle.to(), Square::H1);
    assert_eq!(ui_to(castle), Square::G1);
    assert!(g.targets().is_empty());

    g.cursor = Square::E1;
    g.activate(None);
    assert!(g.targets().contains(&Square::G1));
    g.cursor = Square::G1;
    let played = g.activate(None).expect("selecting g1 castles");
    assert_eq!(g.to_uci(played), "e1g1");
    assert_eq!(g.history.last().unwrap(), "O-O");
}

#[test]
fn promotion_asks_before_committing() {
    let mut g = Game::new();
    // A pawn walks d4-e5, takes en passant on d6, then eats its way to c7.
    play(
        &mut g,
        &[
            "d2d4", "e7e5", "d4e5", "d7d5", "e5d6", "g8f6", "d6c7", "a7a6",
        ],
    );

    g.cursor = Square::C7;
    g.activate(None);
    g.cursor = Square::B8; // capture the knight, promoting
    assert!(
        g.activate(None).is_none(),
        "ambiguous move must not auto-play"
    );
    assert!(g.promotion.is_some());
    assert_eq!(g.history.len(), 8);

    let m = g.promote(Role::Knight).unwrap();
    assert_eq!(g.to_uci(m), "c7b8n");
    assert_eq!(g.history.last().unwrap(), "cxb8=N");
}

#[test]
fn en_passant_and_uci_round_trip() {
    let mut g = Game::new();
    play(&mut g, &["e2e4", "a7a6", "e4e5", "d7d5"]);
    let m = g
        .moves_from(Square::E5)
        .into_iter()
        .find(|m| m.is_en_passant())
        .unwrap();
    assert_eq!(g.to_uci(m), "e5d6");
    g.play(m);
    assert_eq!(g.history.last().unwrap(), "exd6");
    assert!(g.piece_at(Square::D5).is_none(), "the passed pawn is gone");
}

#[test]
fn scholars_mate_ends_the_game() {
    let mut g = Game::new();
    play(
        &mut g,
        &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"],
    );
    assert!(g.pos.is_checkmate());
    assert!(g.over());
    assert_eq!(g.turn(), Color::Black, "black is mated");
    assert!(g.activate(None).is_none(), "no moves after mate");
}

#[test]
fn a_player_only_moves_their_own_pieces() {
    let mut g = Game::new();
    g.cursor = Square::E2;
    assert!(g.activate(Some(Color::Black)).is_none());
    assert!(g.selected.is_none(), "black may not pick up a white pawn");

    g.activate(Some(Color::White));
    assert_eq!(g.selected, Some(Square::E2));
}

#[test]
fn captured_tray_tracks_material() {
    let mut g = Game::new();
    assert!(g.captured(Color::White).is_empty());
    play(&mut g, &["e2e4", "d7d5", "e4d5"]);
    assert_eq!(g.captured(Color::Black), [Role::Pawn]);
    assert_eq!(g.material_edge(), 1, "white is a pawn up");
}
