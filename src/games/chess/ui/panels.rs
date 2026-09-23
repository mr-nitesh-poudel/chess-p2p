//! Everything around the board: the trays of captured pieces, the sidebar,
//! the key hints, the promotion prompt and the verdict.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use shakmaty::{Color as Side, Piece};

use super::board::{canvas_dots, canvas_piece};
use super::pieces::{piece_cell, piece_rows};
use super::{
    FLASH, GUTTER, Geometry, LIGHT, MATE_RED, PIECE_BLACK, PIECE_WHITE, PieceStyle, VERDICT_BG,
    side_name,
};
use crate::games::chess::app::{App, Ending, Finale};
use crate::games::chess::canvas;
use crate::games::chess::rules::PROMOTION_ROLES;
use crate::games::{Ctx, chrome};
use crate::ui::{CURSOR, MUTED, blend, centred};

/// The pieces `side` has captured, plus their material edge if they have one.
pub(super) fn draw_tray(f: &mut Frame, area: Rect, app: &App, side: Side) {
    let taken = app.game.captured(!side);
    let style = match app.piece_style {
        PieceStyle::Letter | PieceStyle::BigLetter => PieceStyle::Letter,
        _ => PieceStyle::Figurine,
    };
    let mut text: String = taken
        .iter()
        .map(|r| piece_rows(*r, style, 1).remove(0))
        .collect();

    let edge = app.game.material_edge();
    let ahead = if side == Side::White { edge } else { -edge };
    if ahead > 0 {
        text.push_str(&format!("  +{ahead}"));
    }

    // The tray holds the opponent's pieces, so it takes the opponent's colour.
    let fg = if side == Side::White {
        PIECE_BLACK
    } else {
        PIECE_WHITE
    };
    let line = Line::from(vec![
        Span::raw(" ".repeat(GUTTER.into())),
        Span::styled(text, Style::default().fg(fg)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub(super) fn draw_sidebar(f: &mut Frame, area: Rect, app: &App, ctx: &Ctx) {
    let [status, moves] =
        Layout::vertical([Constraint::Length(12), Constraint::Min(3)]).areas(area);

    let mut lines = Vec::new();
    match app.me {
        Some(c) => lines.push(Line::from(vec![
            Span::styled("you  ", Style::default().fg(MUTED)),
            Span::raw(side_name(c)),
        ])),
        None => lines.push(Line::from(vec![
            Span::styled("mode ", Style::default().fg(MUTED)),
            Span::raw("hot-seat"),
        ])),
    }

    lines.push(Line::from(vec![
        Span::styled("turn ", Style::default().fg(MUTED)),
        Span::raw(side_name(app.game.turn())),
    ]));

    let (state, style) = app.state_line(ctx);
    lines.push(Line::from(vec![
        Span::raw("     "),
        Span::styled(state, style),
    ]));

    lines.push(Line::raw(""));
    lines.extend(chrome::connection_lines(ctx));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Line::from(" game "));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        status,
    );

    draw_moves(f, moves, app);
}

fn draw_moves(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Line::from(" moves "));
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = app
        .game
        .history
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| {
            let black = pair.get(1).map(String::as_str).unwrap_or("");
            Line::from(vec![
                Span::styled(format!("{:>3}. ", i + 1), Style::default().fg(MUTED)),
                Span::raw(format!("{:<8}", pair[0])),
                Span::raw(black.to_string()),
            ])
        })
        .collect();

    // Keep the tail of the game visible.
    if lines.len() > inner_h {
        lines.drain(..lines.len() - inner_h);
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

pub(super) fn draw_footer(f: &mut Frame, area: Rect, app: &App, ctx: &Ctx) {
    let question = app
        .confirm_resign
        .then_some("resign this game?   y resign   n cancel");
    chrome::footer(f, area, ctx, question, footer_keys(app, area.width));
}

fn footer_keys(app: &App, width: u16) -> &'static str {
    if app.game.promotion.is_some() {
        "click a piece, or ←/→ and enter   esc cancel"
    } else if width >= 92 {
        "click or drag to move   arrows/hjkl   f flip   p pieces   m mouse   r resign   d draw   q lobby"
    } else if width >= 62 {
        "click or drag   f flip   p pieces   r resign   d draw   q lobby"
    } else {
        "click to move   f flip   r resign   q lobby"
    }
}

pub(super) fn draw_promotion(f: &mut Frame, g: &Geometry, app: &App) {
    let Some(p) = &app.game.promotion else { return };
    let side = app.game.turn();
    f.render_widget(Clear, g.promo);

    let inner_h = g.promo.height.saturating_sub(2);
    let mut rows: Vec<Line> = Vec::new();
    for sub in 0..inner_h {
        let mut spans = vec![Span::raw(" ")];
        for (i, role) in PROMOTION_ROLES.iter().enumerate() {
            let bg = if i == p.choice { CURSOR } else { LIGHT };
            let piece = Piece {
                color: side,
                role: *role,
            };
            spans.extend(piece_cell(
                piece,
                sub,
                g.promo_cell,
                inner_h,
                app.piece_style,
                bg,
            ));
        }
        rows.push(Line::from(spans));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CURSOR))
        .title(Line::from(" promote to "));
    f.render_widget(
        Paragraph::new(rows).alignment(Alignment::Left).block(block),
        g.promo,
    );

    if let Some(dots) = canvas_dots(app.piece_style, g.promo_cell, inner_h) {
        // Past the border and the one-cell margin the rows start with.
        let area = Rect {
            x: g.promo.x + 2,
            y: g.promo.y + 1,
            width: 4 * g.promo_cell,
            height: inner_h,
        };
        let pieces: Vec<_> = PROMOTION_ROLES
            .iter()
            .enumerate()
            .map(|(i, &role)| {
                let piece = Piece { color: side, role };
                canvas_piece(piece, (i as u16, 0), (0.0, 0.0), (g.promo_cell, inner_h))
            })
            .collect();
        canvas::stamp(f.buffer_mut(), area, &pieces, dots);
    }
}

/// The letters of CHECKMATE and RESIGNED, five blocks square.
fn glyph(c: char) -> [&'static str; 5] {
    match c {
        'C' => [" ████", "█    ", "█    ", "█    ", " ████"],
        'H' => ["█   █", "█   █", "█████", "█   █", "█   █"],
        'E' => ["█████", "█    ", "████ ", "█    ", "█████"],
        'K' => ["█   █", "█  █ ", "███  ", "█  █ ", "█   █"],
        'M' => ["█   █", "██ ██", "█ █ █", "█   █", "█   █"],
        'A' => [" ███ ", "█   █", "█████", "█   █", "█   █"],
        'T' => ["█████", "  █  ", "  █  ", "  █  ", "  █  "],
        'R' => ["████ ", "█   █", "████ ", "█  █ ", "█   █"],
        'S' => [" ████", "█    ", " ███ ", "    █", "████ "],
        'I' => ["█████", "  █  ", "  █  ", "  █  ", "█████"],
        'G' => [" ████", "█    ", "█  ██", "█   █", " ████"],
        'N' => ["█   █", "██  █", "█ █ █", "█  ██", "█   █"],
        'D' => ["████ ", "█   █", "█   █", "█   █", "████ "],
        _ => ["     "; 5],
    }
}

/// The verdict across the middle of the board, spelled out a letter at a
/// time as `reveal` goes from 0 to 1. Each letter lands white hot and cools
/// to red.
pub(super) fn draw_verdict(
    f: &mut Frame,
    g: &Geometry,
    app: &App,
    ctx: &Ctx,
    fin: &Finale,
    reveal: f32,
) {
    let word = match fin.how {
        Ending::Checkmate => "CHECKMATE",
        Ending::Resignation => "RESIGNED",
    };
    let letters = word.len() as f32;
    let shown = reveal * letters;
    let colour = |i: usize| {
        let age = shown - i as f32;
        if age < 1.0 {
            blend(FLASH, MATE_RED, age)
        } else {
            MATE_RED
        }
    };

    let big_w = word.len() as u16 * 6 - 1;
    let big = g.board.width >= big_w + 6 && g.board.height >= 13;
    let mut lines = vec![Line::raw("")];
    if big {
        for row in 0..5 {
            let mut spans = Vec::new();
            for (i, c) in word.chars().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let text = if (i as f32) < shown {
                    glyph(c)[row]
                } else {
                    "     "
                };
                spans.push(Span::styled(text, Style::default().fg(colour(i))));
            }
            lines.push(Line::from(spans).centered());
        }
    } else {
        let spans: Vec<Span> = word
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let text = if (i as f32) < shown { c } else { ' ' };
                Span::styled(format!("{text} "), Style::default().fg(colour(i)).bold())
            })
            .collect();
        lines.push(Line::from(spans).centered());
    }
    lines.push(Line::raw(""));

    let done = reveal >= 1.0;
    if done {
        let loser = app.game.resigned.unwrap_or(app.game.turn());
        let verdict = match (fin.how, app.me) {
            (Ending::Checkmate, Some(me)) if me == loser => {
                format!("{} wins", ctx.peer_label())
            }
            (Ending::Checkmate, Some(_)) => "you win".to_string(),
            (Ending::Checkmate, None) => format!("{} wins", side_name(!loser)),
            (Ending::Resignation, Some(me)) if me == loser => "you resigned".to_string(),
            (Ending::Resignation, Some(_)) => {
                format!("{} resigned · you win", ctx.peer_label())
            }
            (Ending::Resignation, None) => {
                format!("{} resigns · {} wins", side_name(loser), side_name(!loser))
            }
        };
        lines.push(Line::styled(verdict, Style::default().fg(FLASH).bold()).centered());
        lines.push(Line::styled("any key to see the board", Style::default().fg(MUTED)).centered());
    }

    let width = if big {
        big_w + 6
    } else {
        2 * word.len() as u16 + 8
    }
    .max(30);
    let height = lines.len().max(if big { 10 } else { 5 }) as u16 + 2;
    // In the half of the board away from the king, so the mate stays in
    // view, if there is room there.
    let half = g.grid.height / 2;
    let king_row = app.finale().map_or(0, |fin| {
        let row = 7 - fin.king.rank() as u16;
        if app.flipped { 7 - row } else { row }
    });
    let away = Rect {
        x: g.board.x,
        y: if king_row < 4 {
            g.grid.y + half
        } else {
            g.grid.y
        },
        width: g.board.width,
        height: half,
    };
    let area = centred(if height <= half { away } else { g.board }, width, height);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(MATE_RED))
        .style(Style::default().bg(VERDICT_BG));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
