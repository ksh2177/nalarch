//! Glyphs, on by default.
//!
//! They come from the Material Design set carried by Nerd Fonts, and asking
//! everyone to pass a flag for them would be a poor trade — a tool should look
//! like itself out of the box.
//!
//! The one place they cannot work is a bare TTY after a botched upgrade, which
//! is also a moment nalarch is meant for: no Nerd Font is loaded there and every
//! glyph comes out an empty box. That case is detected rather than delegated to
//! the user — `TERM=linux` and its kin mean a console, and a console cannot draw
//! them whatever anyone configures.
//!
//! Outside a console the font cannot be interrogated, so `--no-icons` or
//! `icons = false` remain for anyone without a patched one. They also assume a
//! single-width (Mono) Nerd Font variant: the double-width ones render these
//! glyphs across two cells while the layout counts one, which shifts every
//! column that follows.
//!
//! Nothing here carries meaning on its own: every icon sits next to the word it
//! decorates. Turning them off loses decoration, not information.

use std::sync::OnceLock;

static ON: OnceLock<bool> = OnceLock::new();

/// Decides once, at startup, whether glyphs are drawn.
///
/// `--no-icons` beats `--icons`, which beats the environment, which beats the
/// config file, which beats the console check. A flag is how one checks
/// quickly; the config file is how one settles it. An explicit yes is honoured
/// even in a console — being told is better than being clever.
pub fn init(args: &[String]) {
    let value = if args.iter().any(|a| a == "--no-icons") {
        false
    } else if args.iter().any(|a| a == "--icons") {
        true
    } else if let Ok(v) = std::env::var("NALARCH_ICONS") {
        matches!(v.as_str(), "1" | "true" | "yes")
    } else {
        match from_config() {
            Some(v) => v,
            None => !in_console(),
        }
    };
    let _ = ON.set(value);
}

/// True in a terminal that cannot draw a patched font, whatever is installed.
///
/// Only the certain cases are listed. `screen` and `tmux` are left out on
/// purpose: they run inside whatever terminal launched them, and that one may
/// well have the font.
fn in_console() -> bool {
    let term = std::env::var("TERM").unwrap_or_default();
    term.is_empty() || term == "linux" || term == "dumb" || term.starts_with("vt")
}

pub fn enabled() -> bool {
    *ON.get().unwrap_or(&false)
}

/// `~/.config/nalarch/config`, one `key = value` per line.
///
/// Hand-parsed rather than pulling in a TOML crate: there is one key, and the
/// project already reads pacman.conf and PACCACHE_ARGS the same way.
///
/// `None` when the key is absent, which is not the same as `false`: an unset
/// option leaves the console check to decide, a `false` one does not.
fn from_config() -> Option<bool> {
    let contents = std::fs::read_to_string(config_path()?).ok()?;
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "icons")
        .map(|(_, v)| matches!(v.trim(), "1" | "true" | "yes"))
}

pub fn config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok()?;
    Some(base.join("nalarch").join("config"))
}

/// An icon plus its trailing space, or nothing at all.
fn glyph(c: char) -> String {
    if enabled() {
        format!("{c} ")
    } else {
        String::new()
    }
}

pub fn tab(t: crate::app::Tab) -> String {
    use crate::app::Tab;
    glyph(match t {
        Tab::Updates => '\u{f06b0}',   // a clock with a refresh arrow
        Tab::Installed => '\u{f03d7}', // a closed box
        Tab::Orphans => '\u{f02a0}',   // a ghost
        Tab::History => '\u{f02da}',   // a clock running backwards
        Tab::Search => '\u{f0349}',    // a magnifier
        Tab::Cache => '\u{f01bc}',     // stacked discs
    })
}

/// Repository marker. Unknown repositories get no icon rather than a wrong one:
/// third-party repositories are common, and inventing a glyph for each would
/// say something the tool does not know.
pub fn repo(name: &str) -> String {
    match name {
        "core" => glyph('\u{f0498}'),   // a shield: the base of the system
        "extra" => glyph('\u{f03d6}'),  // a package
        "multilib" => glyph('\u{f0487}'), // a chip: the 32-bit side
        "aur" => glyph('\u{f08c7}'),    // the Arch logo, for what the users build
        _ => String::new(),
    }
}

/// Width the repository column has to reserve, so that turning icons on does
/// not shift everything to its right.
pub fn repo_width() -> usize {
    if enabled() {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_drawn_when_icons_are_off() {
        // `init` was never called in the test binary, so nothing is drawn.
        assert!(!enabled());
        assert_eq!(repo("core"), "");
        assert_eq!(repo_width(), 0);
    }

    /// The one case a patched font cannot be present, whatever is installed.
    #[test]
    fn a_console_cannot_draw_them() {
        let console = |t: &str| {
            t.is_empty() || t == "linux" || t == "dumb" || t.starts_with("vt")
        };
        assert!(console("linux"));
        assert!(console("vt220"));
        assert!(console(""));
        // A multiplexer runs inside a terminal that may well have the font.
        assert!(!console("tmux-256color"));
        assert!(!console("screen-256color"));
        assert!(!console("xterm-256color"));
    }

    #[test]
    fn an_unknown_repository_gets_no_icon() {
        // Third-party repositories are common; a guessed glyph would claim
        // knowledge the tool does not have.
        assert_eq!(repo("chaotic-aur"), "");
    }
}
