//! The first screen: host, join with a code, or share a keyboard.
//!
//! Like [`App`](crate::app::App), this only holds state and decides what a key
//! or click means; `ui` draws it and `main` acts on the [`Choice`] it returns.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::session::Code;
use crate::session::code::{complete, is_prefix, is_word};
use crate::ui::LobbyGeometry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Host,
    Join,
    Local,
    Quit,
}

pub const ITEMS: [Item; 4] = [Item::Host, Item::Join, Item::Local, Item::Quit];

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::Host => "Host a game",
            Item::Join => "Join a game",
            Item::Local => "Play on one keyboard",
            Item::Quit => "Quit",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Item::Host => "get a code to send your opponent",
            Item::Join => "type in the code your opponent sent",
            Item::Local => "two players taking turns",
            Item::Quit => "",
        }
    }
}

/// What the player settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Host,
    Join(Code),
    Local,
    Quit,
}

/// How the code typed so far is looking, for the line under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Empty,
    Typing,
    Ready(Code),
    Bad(String),
}

pub struct Lobby {
    pub selected: usize,
    /// Typing a code in, rather than choosing from the menu.
    pub joining: bool,
    pub input: String,
    /// Last known terminal size, for turning clicks into items.
    pub area: Rect,
}

impl Default for Lobby {
    fn default() -> Self {
        Self::new()
    }
}

impl Lobby {
    pub fn new() -> Self {
        Self {
            selected: 0,
            joining: false,
            input: String::new(),
            area: Rect::new(0, 0, 80, 24),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Choice> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Some(Choice::Quit);
        }
        if self.joining {
            return self.entry_key(key.code, ctrl);
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = (self.selected + ITEMS.len() - 1) % ITEMS.len();
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = (self.selected + 1) % ITEMS.len();
            }
            KeyCode::Enter | KeyCode::Char(' ') => return self.activate(self.selected),
            KeyCode::Char('q') | KeyCode::Esc => return Some(Choice::Quit),
            // Every code starts with its number, so typing one is as good as
            // choosing to join.
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.start_joining();
                self.push(c);
            }
            _ => {}
        }
        None
    }

    fn entry_key(&mut self, code: KeyCode, ctrl: bool) -> Option<Choice> {
        match code {
            KeyCode::Esc => self.joining = false,
            KeyCode::Enter => {
                if let Entry::Ready(code) = self.entry() {
                    return Some(Choice::Join(code));
                }
            }
            KeyCode::Tab | KeyCode::Right => self.accept_completion(),
            KeyCode::Char('u') if ctrl => self.input.clear(),
            KeyCode::Char('w') if ctrl => {
                let trimmed = self.input.trim_end_matches('-');
                let keep = trimmed.rfind('-').map_or(0, |i| i + 1);
                self.input.truncate(keep);
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.push(c),
            _ => {}
        }
        None
    }

    pub fn on_paste(&mut self, text: &str) {
        self.start_joining();
        self.input.clear();
        // People paste the whole command as often as the code.
        let text = text.rsplit_once("join").map_or(text, |(_, rest)| rest);
        for c in text.chars() {
            self.push(c);
        }
        self.input = self.input.trim_end_matches('-').to_string();
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) -> Option<Choice> {
        let g = LobbyGeometry::new(self.area);
        let hit = g.item_at(ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Moved if !self.joining => {
                if let Some(i) = hit {
                    self.selected = i;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = hit {
                    self.selected = i;
                    return self.activate(i);
                }
                if g.input.contains((ev.column, ev.row).into()) {
                    self.start_joining();
                }
            }
            _ => {}
        }
        None
    }

    fn activate(&mut self, i: usize) -> Option<Choice> {
        match ITEMS.get(i)? {
            Item::Host => Some(Choice::Host),
            Item::Join => {
                self.start_joining();
                None
            }
            Item::Local => Some(Choice::Local),
            Item::Quit => Some(Choice::Quit),
        }
    }

    fn start_joining(&mut self) {
        self.joining = true;
        self.selected = ITEMS.iter().position(|&i| i == Item::Join).unwrap_or(0);
    }

    /// Typed text goes in the way it will be read back: lower case, with any
    /// separator turned into a single dash.
    fn push(&mut self, c: char) {
        if c.is_ascii_alphanumeric() {
            self.input.push(c.to_ascii_lowercase());
        } else if (c == '-' || c == '_' || c == '.' || c.is_whitespace())
            && !self.input.is_empty()
            && !self.input.ends_with('-')
        {
            self.input.push('-');
        }
    }

    fn parts(&self) -> Vec<&str> {
        self.input.split('-').collect()
    }

    /// What the word being typed would become if Tab were pressed now.
    pub fn completion(&self) -> Option<&'static str> {
        let parts = self.parts();
        // The first part is the number, which has nothing to complete.
        let last = *parts.last()?;
        if parts.len() < 2 || last.is_empty() {
            return None;
        }
        complete(last).filter(|w| w.len() > last.len())
    }

    fn accept_completion(&mut self) {
        if let Some(word) = self.completion() {
            let typed = self.parts().last().map_or(0, |p| p.len());
            self.input.push_str(&word[typed..]);
        }
        // Move on to the next part, but only past one that is right.
        let parts = self.parts();
        let last = parts[parts.len() - 1];
        let valid = if parts.len() == 1 {
            last.parse::<u8>().is_ok_and(|n| n < 100)
        } else {
            is_word(last)
        };
        if valid && parts.len() < 4 {
            self.input.push('-');
        }
    }

    pub fn entry(&self) -> Entry {
        if self.input.is_empty() {
            return Entry::Empty;
        }
        if let Ok(code) = self.input.parse() {
            return Entry::Ready(code);
        }
        let parts = self.parts();
        if parts.len() > 4 {
            return Entry::Bad("a code is a number and three words".into());
        }
        // Only judge parts the player has moved on from.
        let done = parts.len() - 1;
        if done >= 1 && !parts[0].parse::<u8>().is_ok_and(|n| n < 100) {
            return Entry::Bad(format!("{:?} should be a number from 0 to 99", parts[0]));
        }
        if let Some(bad) = parts[1..done.max(1)].iter().find(|w| !is_word(w)) {
            return Entry::Bad(format!("{bad:?} is not one of the code words"));
        }
        // A last word that nothing starts with will never become one.
        let last = parts[done];
        if done >= 1 && !is_prefix(last) {
            return Entry::Bad(format!("no code word starts with {last:?}"));
        }
        Entry::Typing
    }
}
