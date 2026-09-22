//! Drawing the lobby, and the geometry its clicks are tested against.

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::{Entry, Field, Item, Lobby, Row};
use crate::games::Kind;
use crate::ui::{CAPTURE, CURSOR, MUTED, SELECTED, centred};

/// Where the lobby's pieces sit. [`LobbyGeometry::rows`] lines up with
/// [`Lobby::rows`], so what is drawn and what is clicked cannot drift apart.
pub struct LobbyGeometry {
    pub panel: Rect,
    pub rows: Vec<Rect>,
    pub friends_heading: Rect,
    /// Where to say how friends get here, while there are none.
    pub no_friends: Option<Rect>,
    pub input: Rect,
    pub hint: Rect,
    pub footer: Rect,
}

const LOBBY_W: u16 = 46;
const PROMPT: &str = "code ";

/// A label and a blurb for most items; a single line for Quit and a friend.
fn row_height(row: Row) -> u16 {
    match row {
        Row::Friend(_) | Row::Item(Item::Game | Item::Quit) => 1,
        Row::Item(_) => 2,
    }
}

impl LobbyGeometry {
    pub fn new(area: Rect, rows: &[Row]) -> Self {
        // Top to bottom inside the panel first, then placed on screen once
        // the panel's height is known.
        let has_friends = rows.iter().any(|r| matches!(r, Row::Friend(_)));
        let mut y = 1;
        let mut heading = 0;
        let mut no_friends = None;
        let mut placed = Vec::with_capacity(rows.len());
        for (i, &row) in rows.iter().enumerate() {
            let starts_friends = match row {
                Row::Friend(_) => i == 0 || !matches!(rows[i - 1], Row::Friend(_)),
                Row::Item(Item::Name) => !has_friends,
                _ => false,
            };
            if starts_friends {
                heading = y + 1;
                y += 2;
                if !has_friends {
                    no_friends = Some(y);
                    y += 1;
                }
            }
            // A gap under the game, which the rest of the menu is for, and
            // above the name, which is about the player rather than a game.
            if matches!(row, Row::Item(Item::Host | Item::Name)) {
                y += 1;
            }
            placed.push((y, row_height(row)));
            y += row_height(row);
        }
        let (input, hint) = (y + 1, y + 2);
        let inner_h = y + 4;

        let [main, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        let panel = centred(main, LOBBY_W, inner_h + 2);
        let at = |y: u16, h: u16| {
            Rect {
                x: panel.x + 2,
                y: panel.y + 1 + y,
                width: panel.width.saturating_sub(4),
                height: h,
            }
            .intersection(panel)
        };
        Self {
            panel,
            rows: placed.into_iter().map(|(y, h)| at(y, h)).collect(),
            friends_heading: at(heading, 1),
            no_friends: no_friends.map(|y| at(y, 1)),
            input: at(input, 1),
            hint: at(hint, 1),
            footer,
        }
    }

    /// The row under a screen position, if any.
    pub fn row_at(&self, x: u16, y: u16) -> Option<usize> {
        self.rows.iter().position(|r| r.contains(Position { x, y }))
    }
}

pub fn draw_lobby(f: &mut Frame, lobby: &Lobby) {
    let rows = lobby.rows();
    let g = LobbyGeometry::new(f.area(), &rows);
    let muted = Style::default().fg(MUTED);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(muted)
        .title(Line::from(" tui-tui ").centered());
    f.render_widget(block, g.panel);

    f.render_widget(
        Paragraph::new(Line::styled("friends", muted)),
        g.friends_heading,
    );
    if let Some(area) = g.no_friends {
        f.render_widget(
            Paragraph::new(Line::styled("  anyone you play turns up here", muted)),
            area,
        );
    }

    for (i, (&row, &area)) in rows.iter().zip(&g.rows).enumerate() {
        let (mark, label) = if i == lobby.selected {
            (
                "▸ ",
                Style::default().fg(CURSOR).add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default())
        };
        let lines = match row {
            Row::Friend(n) => {
                let friend = &lobby.friends[n];
                let games = match friend.games {
                    1 => "1 game".to_string(),
                    n => format!("{n} games"),
                };
                let name: String = friend.name.chars().take(18).collect();
                vec![Line::from(vec![
                    Span::styled(mark, label),
                    Span::styled(format!("{name:<19}"), label),
                    Span::styled(format!("{games} · {}", ago(friend.last_played)), muted),
                ])]
            }
            Row::Item(Item::Game) => {
                // The arrows say it can be changed, once there is a choice.
                let arrows = if Kind::ALL.len() > 1 { label } else { muted };
                vec![Line::from(vec![
                    Span::styled(mark, label),
                    Span::styled(format!("{:<12}", Item::Game.label()), label),
                    Span::styled("‹ ", arrows),
                    Span::styled(
                        lobby.game().name(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ›", arrows),
                ])]
            }
            Row::Item(item) => {
                let mut lines = vec![Line::from(vec![
                    Span::styled(mark, label),
                    Span::styled(item.label(), label),
                ])];
                let blurb = match item {
                    Item::Host => Line::styled("  get a code to send your opponent", muted),
                    Item::Join => Line::styled("  type in the code your opponent sent", muted),
                    Item::Local => Line::styled("  two players taking turns", muted),
                    Item::Name if lobby.editing == Some(Field::Name) => Line::from(vec![
                        Span::raw("  "),
                        Span::styled(lobby.name_input.as_str(), Style::default().fg(CURSOR)),
                    ]),
                    Item::Name if lobby.guest => Line::styled(
                        format!("  {} (guest: another copy has your profile)", lobby.name),
                        muted,
                    ),
                    Item::Name => Line::from(vec![
                        Span::raw("  "),
                        Span::raw(lobby.name.as_str()),
                        Span::styled(" — what friends see", muted),
                    ]),
                    Item::Game | Item::Quit => Line::raw(""),
                };
                lines.push(blurb);
                lines
            }
        };
        f.render_widget(Paragraph::new(lines), area);
    }

    let joining = lobby.editing == Some(Field::Code);
    let input = if lobby.input.is_empty() && !joining {
        Line::from(vec![
            Span::styled(PROMPT, muted),
            Span::styled("42-tiger-marble-ocean", muted),
        ])
    } else {
        let typed = if joining {
            Style::default().fg(CURSOR)
        } else {
            Style::default()
        };
        // The rest of the word Tab would fill in, greyed out after the cursor.
        let ghost = lobby.completion().map_or("", |word| {
            let typed = lobby.input.rsplit('-').next().map_or(0, str::len);
            &word[typed..]
        });
        Line::from(vec![
            Span::styled(PROMPT, muted),
            Span::styled(lobby.input.as_str(), typed),
            Span::styled(ghost, muted),
        ])
    };
    f.render_widget(Paragraph::new(input), g.input);

    match lobby.editing {
        Some(Field::Code) => set_cursor(f, g.input, PROMPT.len() + lobby.input.len()),
        Some(Field::Name) => {
            if let Some(blurb) = g.rows.get(lobby.selected) {
                let line = Rect {
                    y: blurb.y + 1,
                    height: 1,
                    ..*blurb
                };
                set_cursor(f, line, 2 + lobby.name_input.chars().count());
            }
        }
        None => {}
    }

    let hint = if let Some(i) = lobby.forgetting {
        let name = lobby.friends.get(i).map_or("them", |f| f.name.as_str());
        Line::styled(
            format!("forget {name}? y to confirm"),
            Style::default().fg(CAPTURE),
        )
    } else if lobby.editing == Some(Field::Name) {
        Line::styled("enter saves, esc cancels", muted)
    } else if joining {
        match lobby.entry() {
            Entry::Empty => Line::styled("type or paste the code you were sent", muted),
            Entry::Typing if lobby.completion().is_some() => {
                Line::styled("tab finishes the word", muted)
            }
            Entry::Typing => Line::raw(""),
            Entry::Ready(_) => Line::styled("enter to join", Style::default().fg(SELECTED)),
            Entry::Bad(why) => Line::styled(why, Style::default().fg(CAPTURE)),
        }
    } else if let Some(notice) = &lobby.notice {
        Line::styled(notice.as_str(), Style::default().fg(CURSOR))
    } else {
        Line::raw("")
    };
    f.render_widget(Paragraph::new(hint), g.hint);

    let on_friend = matches!(rows.get(lobby.selected), Some(Row::Friend(_)));
    let on_game = lobby.on_game();
    let keys = if lobby.invite.is_some() {
        "y accept   n decline"
    } else if joining {
        "enter join   tab complete   ctrl-u clear   esc back"
    } else if lobby.editing == Some(Field::Name) {
        "enter save   esc cancel"
    } else if on_game {
        "←/→ change game   ↑/↓ choose   q quit"
    } else if on_friend {
        "enter challenge   x forget   ↑/↓ choose   q quit"
    } else {
        "↑/↓ choose   enter select   or type a code   q quit"
    };
    f.render_widget(
        Paragraph::new(Line::styled(keys, muted)).centered(),
        g.footer,
    );

    if let Some(invite) = &lobby.invite {
        let area = centred(g.panel, 40, 5);
        f.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CURSOR))
            .title(Line::from(" invite ").centered());
        let lines = vec![
            Line::raw(""),
            Line::styled(
                format!(
                    "{} wants to play {}",
                    invite.name,
                    invite.game.name().to_lowercase()
                ),
                Style::default().add_modifier(Modifier::BOLD),
            )
            .centered(),
            Line::styled("y accept   n decline", muted).centered(),
        ];
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn set_cursor(f: &mut Frame, line: Rect, offset: usize) {
    let x = line.x + offset as u16;
    f.set_cursor_position(Position {
        x: x.min(line.right().saturating_sub(1)),
        y: line.y,
    });
}

/// How long ago a Unix time was, in the loosest terms that still help.
fn ago(then: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let s = now.saturating_sub(then);
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * HOUR;
    const TWO_DAYS: u64 = 2 * DAY;
    match s {
        0..60 => "just now".into(),
        60..HOUR => format!("{}m ago", s / 60),
        HOUR..DAY => format!("{}h ago", s / HOUR),
        DAY..TWO_DAYS => "yesterday".into(),
        _ if s < 14 * DAY => format!("{}d ago", s / DAY),
        _ => format!("{}w ago", s / (7 * DAY)),
    }
}
