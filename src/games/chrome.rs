//! What every game's screen shares: where the connection stands, and the
//! footer with the table's own questions in it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{Conn, Ctx};
use crate::clipboard::Copied;
use crate::ui::{CAPTURE, CURSOR, MUTED, SELECTED};

/// A few lines on the connection, for a game's sidebar: the code to share
/// while hosting, who we are waiting on, who we are playing, or what went
/// wrong. Nothing at all in hot-seat.
pub fn connection_lines(ctx: &Ctx) -> Vec<Line<'static>> {
    let muted = Style::default().fg(MUTED);
    let mut lines = Vec::new();
    match &ctx.conn {
        Conn::Local => {}
        Conn::Publishing | Conn::Waiting => {
            lines.push(Line::styled("share this code:", muted));
            lines.push(Line::styled(
                ctx.share.clone().unwrap_or_default(),
                Style::default().fg(CURSOR),
            ));
            match ctx.copied {
                Some(Copied::Clipboard) => {
                    lines.push(Line::styled("copied ✓", Style::default().fg(SELECTED)));
                }
                // Nothing reports back whether the terminal did it, so say
                // what to do if it did not.
                Some(Copied::Terminal) => {
                    lines.push(Line::styled(
                        "copied via the terminal",
                        Style::default().fg(SELECTED),
                    ));
                    if ctx.mouse {
                        lines.push(Line::styled("(no? m, then select it)", muted));
                    }
                }
                None => lines.push(Line::styled("c copies it", muted)),
            }
            if ctx.conn == Conn::Publishing {
                lines.push(Line::styled("publishing it…", muted));
            }
        }
        Conn::LookingUp => lines.push(Line::styled("looking up code…", muted)),
        Conn::Dialling => lines.push(Line::styled("connecting…", muted)),
        Conn::Inviting(name) => {
            lines.push(Line::styled(
                format!("waiting for {name} to accept…"),
                muted,
            ));
        }
        Conn::Playing => lines.push(Line::from(vec![
            Span::styled("peer ", muted),
            Span::raw(ctx.peer_label()),
        ])),
        Conn::Lost(why) => lines.push(Line::styled(why.clone(), Style::default().fg(CAPTURE))),
    }
    lines
}

/// The bottom line of the screen. The table's own question comes first, then
/// the chat's keys while the player is typing, then the game's question, and
/// with none of those the game's key hints.
pub fn footer(f: &mut Frame, area: Rect, ctx: &Ctx, question: Option<&str>, hints: &str) {
    // A question waiting on an answer stands out from the usual key list.
    let asking = Style::default().fg(CAPTURE).add_modifier(Modifier::BOLD);
    let line = if ctx.confirm_leave {
        Line::styled("leave this game?   y leave   n stay", asking)
    } else if ctx.chat.focused {
        let send = if ctx.is_networked() {
            "enter send   "
        } else {
            ""
        };
        Line::styled(
            format!("{send}↑/↓ scroll   esc back to the game"),
            Style::default().fg(MUTED),
        )
    } else if let Some(question) = question {
        Line::styled(question.to_string(), asking)
    } else {
        Line::styled(hints.to_string(), Style::default().fg(MUTED))
    };
    f.render_widget(Paragraph::new(line.centered()), area);
}
