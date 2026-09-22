//! Game state: the position, the cursor, and everything the UI needs to draw it.
//!
//! Move legality is enforced here and nowhere else. Both peers run this same
//! code over their own copy of the position, so a peer that sends an illegal
//! move simply gets it rejected rather than corrupting the game.

use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, File, Move, Piece, Position, Rank, Role, Square};

pub const CASTLING: CastlingMode = CastlingMode::Standard;

/// The square a move visually lands on.
///
/// `Move::to` reports the *rook* square for castling (the Chess960 convention),
/// which is not where a player aims the cursor.
pub fn ui_to(m: Move) -> Square {
    match m {
        Move::Castle { king, rook } => {
            let file = if rook.file() > king.file() {
                File::G
            } else {
                File::C
            };
            Square::from_coords(file, king.rank())
        }
        _ => m.to(),
    }
}

/// A pawn reaching the last rank: same from/to, four different moves.
pub struct Promotion {
    pub from: Square,
    pub to: Square,
    pub choice: usize,
}

pub const PROMOTION_ROLES: [Role; 4] = [Role::Queen, Role::Rook, Role::Bishop, Role::Knight];

pub struct Game {
    pub pos: Chess,
    pub cursor: Square,
    pub selected: Option<Square>,
    /// SAN of every move played, for the move list.
    pub history: Vec<String>,
    /// Squares of the last move, highlighted on the board.
    pub last: Option<(Square, Square)>,
    pub promotion: Option<Promotion>,
    /// Set when a player resigns; overrides the position's own outcome.
    pub resigned: Option<Color>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Self {
            pos: Chess::default(),
            cursor: Square::E2,
            selected: None,
            history: Vec::new(),
            last: None,
            promotion: None,
            resigned: None,
        }
    }

    pub fn turn(&self) -> Color {
        self.pos.turn()
    }

    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.pos.board().piece_at(sq)
    }

    pub fn over(&self) -> bool {
        self.resigned.is_some() || self.pos.is_game_over()
    }

    /// Every legal move starting from `from`.
    pub fn moves_from(&self, from: Square) -> Vec<Move> {
        self.pos
            .legal_moves()
            .into_iter()
            .filter(|m| m.from() == Some(from))
            .collect()
    }

    /// Squares the selected piece can legally reach.
    pub fn targets(&self) -> Vec<Square> {
        match self.selected {
            Some(from) => self.moves_from(from).into_iter().map(ui_to).collect(),
            None => Vec::new(),
        }
    }

    pub fn nudge(&mut self, dfile: i32, drank: i32) {
        let file = (self.cursor.file() as i32 + dfile).clamp(0, 7);
        let rank = (self.cursor.rank() as i32 + drank).clamp(0, 7);
        self.cursor = Square::from_coords(File::new(file as u32), Rank::new(rank as u32));
    }

    /// Act on the square under the cursor: pick a piece up, put it down, or
    /// switch to a different piece of our own.
    ///
    /// Returns the move to broadcast, or `None` if this only changed the
    /// selection (or opened the promotion prompt).
    pub fn activate(&mut self, controls: Option<Color>) -> Option<Move> {
        if self.over() {
            return None;
        }
        // In a network game we only touch our own pieces, and only on our turn.
        if controls.is_some_and(|me| me != self.turn()) {
            return None;
        }
        let cursor = self.cursor;

        if let Some(from) = self.selected {
            let matching: Vec<Move> = self
                .moves_from(from)
                .into_iter()
                .filter(|m| ui_to(*m) == cursor)
                .collect();
            match matching.len() {
                0 => {}
                1 => {
                    self.selected = None;
                    let m = matching[0];
                    self.play(m);
                    return Some(m);
                }
                // Four moves share this from/to: it is a promotion.
                _ => {
                    self.selected = None;
                    self.promotion = Some(Promotion {
                        from,
                        to: cursor,
                        choice: 0,
                    });
                    return None;
                }
            }
        }

        // Selecting (or reselecting) a piece of the side to move.
        match self.piece_at(cursor) {
            Some(p) if p.color == self.turn() => self.selected = Some(cursor),
            _ => self.selected = None,
        }
        None
    }

    /// Finish a promotion the player has chosen a piece for.
    pub fn promote(&mut self, role: Role) -> Option<Move> {
        let p = self.promotion.take()?;
        let m = self
            .moves_from(p.from)
            .into_iter()
            .find(|m| ui_to(*m) == p.to && m.promotion() == Some(role))?;
        self.play(m);
        Some(m)
    }

    /// Apply a move already known to be legal.
    pub fn play(&mut self, m: Move) {
        let san = SanPlus::from_move(self.pos.clone(), m).to_string();
        self.history.push(san);
        self.last = Some((m.from().unwrap_or_else(|| m.to()), ui_to(m)));
        self.pos.play_unchecked(m);
        self.selected = None;
        self.promotion = None;
        // Park the cursor where the move landed, so a reply starts from there.
        self.cursor = ui_to(m);
    }

    /// Apply a move received from the peer. Rejects anything not legal here.
    pub fn play_uci(&mut self, s: &str) -> Result<Move, String> {
        let uci: UciMove = s.parse().map_err(|_| format!("unparseable move {s:?}"))?;
        let m = uci
            .to_move(&self.pos)
            .map_err(|_| format!("illegal move {s:?}"))?;
        self.play(m);
        Ok(m)
    }

    pub fn to_uci(&self, m: Move) -> String {
        UciMove::from_move(m, CASTLING).to_string()
    }

    /// Pieces of `color` that have been captured, for the material tray.
    pub fn captured(&self, color: Color) -> Vec<Role> {
        const START: [(Role, usize); 5] = [
            (Role::Queen, 1),
            (Role::Rook, 2),
            (Role::Bishop, 2),
            (Role::Knight, 2),
            (Role::Pawn, 8),
        ];
        let board = self.pos.board();
        let mut out = Vec::new();
        for (role, n) in START {
            let alive = board.by_piece(role.of(color)).count();
            for _ in alive..n {
                out.push(role);
            }
        }
        out
    }

    /// Material balance in pawns, from white's perspective.
    pub fn material_edge(&self) -> i32 {
        let value = |r: Role| match r {
            Role::Pawn => 1,
            Role::Knight | Role::Bishop => 3,
            Role::Rook => 5,
            Role::Queen => 9,
            Role::King => 0,
        };
        let sum = |c| self.captured(c).into_iter().map(value).sum::<i32>();
        sum(Color::Black) - sum(Color::White)
    }
}
