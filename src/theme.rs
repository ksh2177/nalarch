//! Colours.
//!
//! Nothing is an absolute value: everything goes through the terminal's ANSI
//! slots. That is what makes the theme follow.
//!
//! The first version hard-coded one dark palette in RGB. nalarch then painted
//! its own background, so under a light theme only the borders changed — the
//! rest stayed dark. Adding a light palette would only have moved the problem:
//! it would have required detecting the active theme, and starting over with
//! every new one.
//!
//! Leaning on the slots instead, the colours come from whatever the terminal
//! has loaded: any theme worth the name defines color0–15, and a live switch
//! applies them at once. nalarch therefore changes theme along with the
//! terminal, while knowing nothing about it.

use ratatui::style::{Color, Modifier, Style};

/// Foreground and background: the terminal's own, never imposed.
pub const FG: Color = Color::Reset;

pub const DIM: Color = Color::Indexed(8); // bright black
pub const ACCENT: Color = Color::Indexed(4); // blue
pub const GREEN: Color = Color::Indexed(2);
pub const YELLOW: Color = Color::Indexed(3);
pub const RED: Color = Color::Indexed(1);
pub const CYAN: Color = Color::Indexed(6);
pub const MAGENTA: Color = Color::Indexed(5);

/// Solid colour badge, readable either way round.
///
/// Reverse video is the only way to get a coloured background with contrasting
/// text without knowing the theme: the terminal brings its own background
/// colour to the foreground. Hard-coding a colour would give black on dark
/// green in a light theme — unreadable.
pub fn badge(colour: Color) -> Style {
    Style::default()
        .fg(colour)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

/// Highlight for the selected row. Same reasoning: reversing adapts, where a
/// fixed grey background would make dim text unreadable (grey on grey) as soon
/// as the theme changes.
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// Colour for a repository, so the AUR stands out at a glance.
pub fn repo_color(repo: &str) -> Color {
    match repo {
        "aur" => MAGENTA,
        "core" => RED,
        "extra" => CYAN,
        "multilib" => YELLOW,
        _ => DIM,
    }
}

/// Formats a byte count in readable units.
pub fn human_size(bytes: i64) -> String {
    let units: [&str; 5] = [
        crate::i18n::t("B"),
        crate::i18n::t("KiB"),
        crate::i18n::t("MiB"),
        crate::i18n::t("GiB"),
        crate::i18n::t("TiB"),
    ];
    let neg = bytes < 0;
    let mut v = bytes.unsigned_abs() as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if i == 0 {
        format!("{v:.0} {}", units[i])
    } else {
        format!("{v:.1} {}", units[i])
    };
    if neg {
        format!("-{s}")
    } else {
        s
    }
}
