//! Optional glyphs, off by default.
//!
//! The icons come from the Material Design set carried by Nerd Fonts. They are
//! opt-in for a reason that matters more here than in most tools: the moment
//! nalarch is most needed is a bare TTY after a botched upgrade, where a
//! Nerd Font is not loaded and every glyph would come out as an empty box. A
//! package manager that becomes unreadable exactly when it is needed would be a
//! poor trade for some polish.
//!
//! They also assume a single-width (Mono) Nerd Font variant. The double-width
//! variants render these glyphs across two cells while the layout counts one,
//! which shifts every column that follows.
//!
//! Nothing here carries meaning on its own: every icon sits next to the word it
//! decorates. Turning them off loses nothing but the decoration.

use std::sync::OnceLock;

static ON: OnceLock<bool> = OnceLock::new();

/// Decides once, at startup, whether glyphs are drawn.
///
/// `--no-icons` beats `--icons`, which beats the environment, which beats the
/// config file. A flag is how one checks quickly; the config file is how one
/// settles it.
pub fn init(args: &[String]) {
    let value = if args.iter().any(|a| a == "--no-icons") {
        false
    } else if args.iter().any(|a| a == "--icons") {
        true
    } else if let Ok(v) = std::env::var("NALARCH_ICONS") {
        matches!(v.as_str(), "1" | "true" | "yes")
    } else {
        from_config()
    };
    let _ = ON.set(value);
}

pub fn enabled() -> bool {
    *ON.get().unwrap_or(&false)
}

/// `~/.config/nalarch/config`, one `key = value` per line.
///
/// Hand-parsed rather than pulling in a TOML crate: there is one key, and the
/// project already reads pacman.conf and PACCACHE_ARGS the same way.
fn from_config() -> bool {
    let Some(path) = config_path() else {
        return false;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "icons")
        .map(|(_, v)| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
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
        // `init` was never called in the test binary, so the default applies.
        assert!(!enabled());
        assert_eq!(repo("core"), "");
        assert_eq!(repo_width(), 0);
    }

    #[test]
    fn an_unknown_repository_gets_no_icon() {
        // Third-party repositories are common; a guessed glyph would claim
        // knowledge the tool does not have.
        assert_eq!(repo("chaotic-aur"), "");
    }
}
