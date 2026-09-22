//! Chess: the rules ([`rules`]), the state and what keys mean ([`app`]), the
//! board ([`ui`], with pieces drawn by [`canvas`]), and what the two sides
//! say to each other ([`protocol`]).

pub mod app;
pub mod canvas;
pub mod protocol;
pub mod rules;
pub mod ui;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;

pub use app::App;
pub use protocol::GAME;

use super::{Kind, Leave, Play, Table};
use crate::net::NetEvent;

impl Play for App {
    fn kind(&self) -> Kind {
        Kind::Chess
    }

    fn table(&self) -> &Table {
        &self.table
    }

    fn table_mut(&mut self) -> &mut Table {
        &mut self.table
    }

    fn on_key(&mut self, key: KeyEvent) {
        App::on_key(self, key);
    }

    fn on_mouse(&mut self, ev: MouseEvent) {
        App::on_mouse(self, ev);
    }

    fn on_net(&mut self, ev: NetEvent) {
        App::on_net(self, ev);
    }

    fn draw(&self, f: &mut Frame) {
        ui::draw(f, self);
    }

    fn set_area(&mut self, area: Rect) {
        self.area = area;
    }

    fn is_animating(&self) -> bool {
        App::is_animating(self)
    }

    fn wants_mouse(&self) -> bool {
        self.mouse
    }

    fn leaving(&self) -> Option<Leave> {
        if self.exit {
            Some(Leave::Exit)
        } else if self.quit {
            Some(Leave::Lobby)
        } else {
            None
        }
    }
}
