//! What two chess players say to each other once they are connected: a UCI
//! move per line, plus a few control words.

use crate::session::Game;

pub const GAME: Game = Game {
    name: "chess",
    version: 1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Move(String),
    Resign,
    /// Offers a draw, or accepts the one on offer.
    Draw,
}

impl Msg {
    pub fn line(&self) -> String {
        match self {
            Msg::Move(uci) => format!("move {uci}"),
            Msg::Resign => "resign".into(),
            Msg::Draw => "draw".into(),
        }
    }

    /// `None` for anything unknown, which is ignored so the protocol can grow.
    pub fn parse(line: &str) -> Option<Msg> {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "move" => Some(Msg::Move(rest.to_string())),
            "resign" => Some(Msg::Resign),
            "draw" => Some(Msg::Draw),
            _ => None,
        }
    }
}
