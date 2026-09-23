//! Player names: what each side calls itself during pairing, and what the
//! other side is prepared to show.
//!
//! A name arrives from a peer we have no reason to trust, so it is cleaned
//! here, where it enters, rather than wherever it happens to be drawn.

/// The longest name either side keeps, in characters.
pub const NAME_MAX: usize = 24;

/// A name as it will be shown to others: one line, no control characters, and
/// short enough to fit the sidebar. `None` if nothing readable is left.
pub fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NAME_MAX)
        .collect();
    cleaned
        .chars()
        .any(char::is_alphanumeric)
        .then(|| cleaned.trim().to_string())
}
