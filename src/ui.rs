//! What every screen draws with: the palette, and a few small helpers.

use ratatui::layout::Rect;
use ratatui::style::Color;

/// The cursor, and anything else the player's eye should land on.
pub const CURSOR: Color = Color::Rgb(246, 205, 82);
/// Something picked, or ready to go.
pub const SELECTED: Color = Color::Rgb(124, 176, 95);
/// A warning, or a question that needs an answer.
pub const CAPTURE: Color = Color::Rgb(204, 96, 78);
/// Labels, hints and borders.
pub const MUTED: Color = Color::Rgb(128, 128, 128);

/// Mixes `over` into `base` at `alpha`. Terminals have no alpha channel, so
/// the blend happens here and is handed over as one solid colour.
pub fn blend(base: Color, over: Color, alpha: f32) -> Color {
    let (Color::Rgb(br, bg, bb), Color::Rgb(or, og, ob)) = (base, over) else {
        return over;
    };
    let mix = |b: u8, o: u8| (f32::from(b) * (1.0 - alpha) + f32::from(o) * alpha).round() as u8;
    Color::Rgb(mix(br, or), mix(bg, og), mix(bb, ob))
}

/// A `w` by `h` rectangle in the middle of `area`, shrunk to fit.
pub fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}
