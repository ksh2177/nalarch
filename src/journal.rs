//! Reconstruction of what paru does, as a stream of events.
//!
//! Until now nalarch kept only a counter and a phase: enough to fill
//! a bar, nothing to tell the story of the operation. An install came down to
//! "done", with no way to know what had been downloaded, verified, replaced,
//! or which hooks had run.
//!
//! This module turns the stream into a sequence of typed events. The interface
//! uses them to retell the operation in its own vocabulary, with the detail
//! pacman produces but drowns.
//!
//! **The child process's locale is forced to C.** Parsing translated output
//! would be unmanageable: every language changes the verbs, and a translation
//! update would break the parser silently. In English the vocabulary is stable
//! and comes straight from pacman's own strings. None of that English reaches
//! the screen verbatim: it all goes back through the translation layer.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Syncing,
    Downloading,
    Verifying,
    Installing,
    Hooks,
    Building,
}

impl Phase {
    pub fn label(self) -> &'static str {
        crate::i18n::t(match self {
            Phase::Starting => "preparing",
            Phase::Syncing => "syncing repositories",
            Phase::Downloading => "downloading",
            Phase::Verifying => "verifying",
            Phase::Installing => "installing",
            Phase::Hooks => "post-transaction hooks",
            Phase::Building => "building",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Downloaded,
    Verified,
    Installed,
    Upgraded,
    Downgraded,
    Reinstalled,
    Removed,
    Hook,
    Built,
}

impl Action {
    pub fn verb(self) -> &'static str {
        crate::i18n::t(match self {
            Action::Downloaded => "Downloaded",
            Action::Verified => "Verified",
            Action::Installed => "Installed",
            Action::Upgraded => "Upgraded",
            Action::Downgraded => "Downgraded",
            Action::Reinstalled => "Reinstalled",
            Action::Removed => "Removed",
            Action::Hook => "Hook",
            Action::Built => "Building",
        })
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Action::Downloaded => "⤓",
            Action::Verified => "✓",
            Action::Installed => "+",
            Action::Upgraded => "↑",
            Action::Downgraded => "↓",
            Action::Reinstalled => "⟳",
            Action::Removed => "−",
            Action::Hook => "⚙",
            Action::Built => "⚒",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub action: Action,
    pub target: String,
    /// Version, size, or extra detail depending on the action.
    pub detail: String,
}

/// One line of the table paru prints before asking to proceed.
///
/// It is the only place the AUR side of a transaction is ever spelled out:
/// pacman cannot resolve an AUR package, so nalarch's own plan says "1 package,
/// sizes unknown" while paru has just worked out that it also needs `go` to
/// build it. Letting that table scroll past would leave the confirmation prompt
/// answering a question nothing on screen has asked.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub name: String,
    /// `extra`, `aur`, … as paru prefixes it.
    pub repo: String,
    pub from: Option<String>,
    pub to: String,
    /// A build dependency: installed to compile something, not because it was
    /// wanted. paru can remove it again afterwards.
    pub make_only: bool,
}

/// One of the numbered answers paru offers.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub number: u32,
    pub name: String,
    /// Where it comes from — `AUR`, `extra`, … as paru groups them.
    pub group: String,
}

/// A numbered question paru is waiting on.
///
/// Two AUR packages can provide the same thing, and paru asks which one. The
/// answer decides what gets built under your account, so it deserves better
/// than a screen saying nothing is expected.
#[derive(Debug, Clone)]
pub struct Choice {
    /// What is being chosen for, as paru names it.
    pub about: String,
    pub candidates: Vec<Candidate>,
    /// The number that applies if one just presses Enter.
    pub default: Option<u32>,
}

/// What is known about download progress.
#[derive(Default)]
pub struct Downloads {
    /// Names of packages whose file is complete.
    pub finished: Vec<String>,
    pub last: String,
    /// Bytes already fetched, accumulated over completed files.
    pub bytes: i64,
    /// Rate announced by pacman on the last line read.
    pub speed: Option<String>,
}

pub struct Journal {
    pub phase: Phase,
    /// Events in order, bounded so they do not grow without end on a talkative
    /// build.
    pub events: Vec<Event>,
    pub downloads: Downloads,
    /// `(N/M)` counter of the current phase.
    pub counter: Option<(u32, u32)>,
    /// Percentage of the current item, never of the transaction.
    pub percent: Option<u8>,
    /// Last step announced by makepkg.
    pub compilation: Option<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    /// Configuration files laid down next to yours, waiting to be merged.
    pub pacnew: Vec<String>,
    /// What paru resolved, read from the table it prints before asking.
    pub resolution: Vec<Resolution>,
    /// True while that table is being read.
    in_resolution: bool,
    /// A numbered question paru is currently waiting on.
    pub choice: Option<Choice>,
    /// Group heading the numbered answers are currently under.
    group: String,
    /// Known sizes by file name, to accumulate the bytes fetched.
    sizes: HashMap<String, i64>,
}

const MAX_EVENTS: usize = 2000;

impl Default for Journal {
    fn default() -> Self {
        Self {
            phase: Phase::Starting,
            events: Vec::new(),
            downloads: Downloads::default(),
            counter: None,
            percent: None,
            compilation: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            pacnew: Vec::new(),
            resolution: Vec::new(),
            in_resolution: false,
            choice: None,
            group: String::new(),
            sizes: HashMap::new(),
        }
    }
}

impl Journal {
    /// Packages the transaction has already handled (downloads aside).
    #[allow(dead_code)]
    pub fn handled(&self) -> impl Iterator<Item = &str> {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    e.action,
                    Action::Installed
                        | Action::Upgraded
                        | Action::Downgraded
                        | Action::Reinstalled
                        | Action::Removed
                )
            })
            .map(|e| e.target.as_str())
    }

    fn push_event(&mut self, action: Action, target: String, detail: String) {
        // Once anything is actually happening, the question has been answered.
        self.choice = None;
        if self.events.len() >= MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(Event {
            action,
            target,
            detail,
        });
    }

    pub fn analyze(&mut self, line: &str) {
        let l = line.trim();
        if l.is_empty() {
            return;
        }

        // paru's own resolution table, printed just before it asks to proceed.
        // It is read before anything else because its rows look like nothing in
        // particular and would otherwise fall through to the download parser.
        if is_resolution_header(l) {
            // A table has several consecutive sections — Repo, then Aur, then
            // their Make variants. Only the first of them starts a new table;
            // clearing on each would leave nothing but the last section.
            if !self.in_resolution {
                self.resolution.clear();
                self.in_resolution = true;
            }
            return;
        }
        if self.in_resolution {
            match read_resolution(l) {
                Some(r) => {
                    self.resolution.push(r);
                    return;
                }
                // Anything that is not a row ends the table.
                None => self.in_resolution = false,
            }
        }

        // A numbered question: its parts arrive over several lines, and the
        // prompt that ends it carries no clue as to what is being chosen.
        if let Some(about) = providers_question(l) {
            self.choice = Some(Choice {
                about,
                candidates: Vec::new(),
                default: None,
            });
            self.group = String::new();
            return;
        }
        if self.choice.is_some() {
            if let Some(group) = group_heading(l) {
                self.group = group;
                return;
            }
            let found = read_candidates(l, &self.group);
            if !found.is_empty() {
                if let Some(c) = self.choice.as_mut() {
                    c.candidates.extend(found);
                }
                return;
            }
            if let Some(n) = read_default(l) {
                if let Some(c) = self.choice.as_mut() {
                    c.default = Some(n);
                }
                return;
            }
        }

        if let Some(rest) = l.strip_prefix(":: ") {
            self.new_phase(rest.trim());
            return;
        }

        // makepkg: its steps have no counter, only markers.
        if let Some(rest) = l.strip_prefix("==>").or_else(|| l.strip_prefix("->")) {
            let step = rest.trim();
            if step.is_empty() {
                return;
            }
            // makepkg raises its own warnings and errors through the same
            // marker. Filing them as build steps buried them among forty others,
            // where they are exactly what one wants surfaced.
            if let Some(w) = step.strip_prefix("WARNING:") {
                self.warning(w.trim());
                return;
            }
            if let Some(e) = step.strip_prefix("ERROR:") {
                let e = e.trim().to_string();
                if !self.errors.contains(&e) {
                    self.errors.push(e);
                }
                return;
            }
            // A build has no measurable progress, and the counter left over from
            // the phase before it does not describe one. Keeping it turned the
            // bar into a flat 0.0 % for the whole compilation, which reads as
            // stuck rather than as unmeasurable.
            if self.phase != Phase::Building {
                self.counter = None;
                self.percent = None;
            }
            self.phase = Phase::Building;
            self.compilation = Some(step.to_string());
            self.push_event(Action::Built, step.to_string(), String::new());
            return;
        }

        if let Some(rest) = l.strip_prefix("warning: ") {
            self.warning(rest.trim());
            return;
        }
        if let Some(rest) = l.strip_prefix("error: ") {
            let e = rest.trim().to_string();
            if !self.errors.contains(&e) {
                self.errors.push(e);
            }
            return;
        }

        if let Some((n, m, label)) = read_counter(l) {
            self.counter = Some((n, m));
            self.percent = read_percent(l);
            self.action_transaction(&label);
            return;
        }

        if let Some(p) = read_percent(l) {
            self.percent = Some(p);
            self.download_line(l, p);
        }
    }

    fn new_phase(&mut self, text: &str) {
        // The labels come from pacman's strings in the C locale.
        let bottom = text.to_ascii_lowercase();
        let phase = if bottom.starts_with("synchronizing package databases") {
            Phase::Syncing
        } else if bottom.starts_with("retrieving packages") {
            Phase::Downloading
        } else if bottom.starts_with("processing package changes") {
            Phase::Installing
        } else if bottom.starts_with("running post-transaction hooks") {
            Phase::Hooks
        } else if bottom.starts_with("checking") || bottom.starts_with("loading") {
            Phase::Verifying
        } else {
            // paru's own phases (AUR search, devel) carry no work of their own:
            // switching to them would wipe the display of what has just been done.
            return;
        };
        if phase != self.phase {
            self.phase = phase;
            self.counter = None;
            self.percent = None;
        }
    }

    fn warning(&mut self, text: &str) {
        // This is how pacman reports configurations it did not dare overwrite.
        // It is the only trace of a file to merge by hand, and it goes unnoticed
        // in the middle of everything else.
        if let Some(ended) = text.find(".pacnew") {
            let path = text[..ended + ".pacnew".len()]
                .rsplit(' ')
                .next()
                .unwrap_or("")
                .to_string();
            if !path.is_empty() && !self.pacnew.contains(&path) {
                self.pacnew.push(path);
                return;
            }
        }
        let a = translate_warning(text);
        if !self.warnings.contains(&a) {
            self.warnings.push(a);
        }
    }

    /// Turns a `(N/M) <verb> <target>` label into an event.
    fn action_transaction(&mut self, label: &str) {
        let l = label.trim();
        let bottom = l.to_ascii_lowercase();

        // Checks: the target is not a package but an operation. pacman's own
        // wording is restated here — its English only serves to recognise the
        // line, and never reaches the screen unmediated.
        // Each wording goes through `t()` right here rather than through a
        // variable: the completeness check reads literals at the call site, and
        // a translated string passed by variable would slip past it.
        for (prefix, wording) in [
            ("checking keys in keyring", crate::i18n::t("keyring keys")),
            ("checking package integrity", crate::i18n::t("package integrity")),
            ("loading package files", crate::i18n::t("loading files")),
            ("checking for file conflicts", crate::i18n::t("file conflicts")),
            ("checking available disk space", crate::i18n::t("available disk space")),
        ] {
            if bottom.starts_with(prefix) {
                let already = self
                    .events
                    .last()
                    .is_some_and(|e| e.action == Action::Verified && e.target == wording);
                if !already {
                    self.push_event(Action::Verified, wording.to_string(), String::new());
                }
                return;
            }
        }

        for (verbe, action) in [
            ("upgrading ", Action::Upgraded),
            ("downgrading ", Action::Downgraded),
            ("reinstalling ", Action::Reinstalled),
            ("installing ", Action::Installed),
            ("removing ", Action::Removed),
        ] {
            if let Some(rest) = bottom.strip_prefix(verbe) {
                let name = l[l.len() - rest.len()..].trim().to_string();
                let deja = self
                    .events
                    .last()
                    .is_some_and(|e| e.action == action && e.target == name);
                if !deja {
                    self.push_event(action, name, String::new());
                }
                return;
            }
        }

        // During hooks, the target is the hook's own name.
        if self.phase == Phase::Hooks {
            let name = l.trim_end_matches('.').trim_end_matches('…').to_string();
            let deja = self
                .events
                .last()
                .is_some_and(|e| e.action == Action::Hook && e.target == name);
            if !deja {
                self.push_event(Action::Hook, name, String::new());
            }
        }
    }

    /// Bar line for a file currently being fetched.
    fn download_line(&mut self, l: &str, percent: u8) {
        let Some(file) = l.split_whitespace().find(|m| m.contains(".pkg.tar")) else {
            return;
        };
        if let Some(size) = read_size(l) {
            self.sizes.insert(file.to_string(), size);
        }
        if let Some(v) = read_speed(l) {
            self.downloads.speed = Some(v);
        }
        if percent < 100 {
            return;
        }
        let Some(name) = name_from_file(file) else {
            return;
        };
        if self.downloads.finished.contains(&name) {
            return;
        }
        self.downloads.bytes += self.sizes.get(file).copied().unwrap_or(0);
        self.downloads.last = file.to_string();
        self.downloads.finished.push(name.clone());
        let size = self.sizes.get(file).copied().unwrap_or(0);
        self.push_event(Action::Downloaded, name, crate::theme::human_size(size));
    }
}

/// Restates pacman's common warnings as plain sentences.
///
/// The child's locale is pinned to C so parsing stays reliable; the terse
/// English it produces is rewritten here and goes through the translation layer
/// like everything else on screen. Unrecognised messages pass through as they
/// are: an English sentence beats a lost one.
pub fn translate_warning(text: &str) -> String {
    use crate::i18n::tf;
    if let Some((package, rest)) = text.split_once(": ignoring package upgrade (") {
        let versions = rest.trim_end_matches(')').replace("=>", "→");
        return tf("{0}: upgrade skipped ({1})", &[package, &versions]);
    }
    if let Some(path) = text.strip_prefix("directory permissions differ on ") {
        return tf("permissions differ on {0}", &[path]);
    }
    if let Some(rest) = text.strip_prefix("could not get file information for ") {
        return tf("file information unreadable: {0}", &[rest]);
    }
    if text.ends_with("is up to date -- skipping") {
        let package = text.split_whitespace().next().unwrap_or(text);
        return tf("{0}: already up to date, skipped", &[package]);
    }
    text.to_string()
}

/// `:: There are 2 providers available for plakar:`
fn providers_question(l: &str) -> Option<String> {
    let rest = l.strip_prefix(":: ")?;
    let rest = rest.strip_prefix("There are ")?;
    let (_count, rest) = rest.split_once(' ')?;
    let target = rest.strip_prefix("providers available for ")?;
    Some(target.trim_end_matches(':').to_string())
}

/// `:: Repository AUR:` — the source the next answers belong to.
fn group_heading(l: &str) -> Option<String> {
    let rest = l.strip_prefix(":: ")?;
    let rest = rest.strip_prefix("Repository ")?;
    Some(rest.trim_end_matches(':').trim().to_string())
}

/// `    1) plakar  2) plakar-git` — several answers may share a line.
fn read_candidates(l: &str, group: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for word in l.split_whitespace() {
        match word.strip_suffix(')').and_then(|n| n.parse::<u32>().ok()) {
            Some(number) => out.push(Candidate {
                number,
                name: String::new(),
                group: group.to_string(),
            }),
            // Package names carry no spaces, but joining is cheap insurance.
            None => {
                if let Some(last) = out.last_mut() {
                    if !last.name.is_empty() {
                        last.name.push(' ');
                    }
                    last.name.push_str(word);
                }
            }
        }
    }
    out.retain(|c| !c.name.is_empty());
    out
}

/// `Enter a number (default=1):`
///
/// Public because the prompt itself rarely reaches this parser: a prompt is
/// written without a newline, so the line splitter holds it in its buffer and
/// never emits it. The run screen reads the same shape off the emulated cursor
/// line instead, where the prompt does sit.
pub fn read_default(l: &str) -> Option<u32> {
    let (_, rest) = l.split_once("(default=")?;
    let (n, _) = rest.split_once(')')?;
    n.trim().parse().ok()
}

/// `Repo (1)`, `Aur Make (2)`, … the heading of one section of paru's table.
///
/// The count in parentheses is what tells it apart from a package whose name
/// happens to start the same way.
fn is_resolution_header(l: &str) -> bool {
    for prefix in ["Repo", "Aur"] {
        let Some(rest) = l.strip_prefix(prefix) else {
            continue;
        };
        let rest = rest.strip_prefix(" Make").unwrap_or(rest);
        if rest.trim_start().starts_with('(') {
            return true;
        }
    }
    false
}

/// One row of that table: `extra/go  2:1.26.6-1  Yes`.
///
/// Columns are aligned rather than delimited, so the fields are read by shape:
/// the first carries `repo/name`, a trailing `Yes`/`No` is the make-only flag,
/// and what remains is one or two versions — one for an install, two for an
/// upgrade.
fn read_resolution(l: &str) -> Option<Resolution> {
    let mut fields: Vec<&str> = l.split_whitespace().collect();
    if fields.len() < 2 {
        return None;
    }
    let (repo, name) = fields.remove(0).split_once('/')?;
    if repo.is_empty() || name.is_empty() {
        return None;
    }

    let make_only = match fields.last() {
        Some(&"Yes") => {
            fields.pop();
            true
        }
        Some(&"No") => {
            fields.pop();
            false
        }
        _ => false,
    };

    let (from, to) = match fields.len() {
        1 => (None, fields[0].to_string()),
        2 => (Some(fields[0].to_string()), fields[1].to_string()),
        _ => return None,
    };
    Some(Resolution {
        name: name.to_string(),
        repo: repo.to_string(),
        from,
        to,
        make_only,
    })
}

/// Recovers a package name from the file being downloaded.
///
/// The convention is `name-version-release-architecture.pkg.tar.*`. The name
/// may itself contain dashes (`python-pbs-installer`), so the last three
/// segments are dropped; it is never cut at the first dash.
pub fn name_from_file(word: &str) -> Option<String> {
    let base = word.split(".pkg.tar").next()?;
    let parts: Vec<&str> = base.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    Some(parts[..parts.len() - 3].join("-"))
}

/// Extracts a counter of the form "(3/7) label", tolerating the padding pacman
/// applies to large numbers — "( 3/17)".
pub fn read_counter(l: &str) -> Option<(u32, u32, String)> {
    let rest = l.strip_prefix('(')?;
    let (inside, after) = rest.split_once(')')?;
    let (n, m) = inside.split_once('/')?;
    // The progress bar follows the label on the same line. It is cut off: it has
    // its own representation, and copying it would truncate the package name,
    // the only useful information here.
    let label = after.split('[').next().unwrap_or(after).trim().to_string();
    Some((n.trim().parse().ok()?, m.trim().parse().ok()?, label))
}

/// Extracts the percentage from a pacman bar, recognisable by its brackets.
pub fn read_percent(l: &str) -> Option<u8> {
    if !l.contains('[') {
        return None;
    }
    let before = l.trim_end().strip_suffix('%')?;
    let chiffres: String = before
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    chiffres.parse().ok().filter(|p| *p <= 100)
}

fn unite_en_octets(u: &str) -> Option<f64> {
    match u {
        "B" => Some(1.0),
        "KiB" => Some(1024.0),
        "MiB" => Some(1024f64.powi(2)),
        "GiB" => Some(1024f64.powi(3)),
        _ => None,
    }
}

/// Size announced on a download line ("638.5 KiB").
pub fn read_size(l: &str) -> Option<i64> {
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, m) in words.iter().enumerate() {
        // The rate reads "1863 KiB/s": its unit carries a suffix, which is what
        // tells it apart from a size.
        let Some(mult) = unite_en_octets(m) else {
            continue;
        };
        if let Some(v) = i
            .checked_sub(1)
            .and_then(|j| words.get(j))
            .and_then(|v| v.replace(',', ".").parse::<f64>().ok())
        {
            return Some((v * mult) as i64);
        }
    }
    None
}

/// The announced rate ("1863 KiB/s"), passed through as is.
pub fn read_speed(l: &str) -> Option<String> {
    let words: Vec<&str> = l.split_whitespace().collect();
    let i = words.iter().position(|m| m.ends_with("/s"))?;
    let unite = words[i].trim_end_matches("/s");
    unite_en_octets(unite)?;
    let value = i.checked_sub(1).and_then(|j| words.get(j))?;
    Some(format!("{value} {unite}/s"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(rows: &[&str]) -> Journal {
        let mut j = Journal::default();
        for l in rows {
            j.analyze(l);
        }
        j
    }

    #[test]
    fn le_nom_se_retrouve_depuis_le_fichier_telecharge() {
        assert_eq!(
            name_from_file("fastfetch-2.67.1-1-x86_64.pkg.tar.zst").as_deref(),
            Some("fastfetch")
        );
        assert_eq!(
            name_from_file("python-pbs-installer-2026.08.14-1-any.pkg.tar.zst").as_deref(),
            Some("python-pbs-installer")
        );
        assert_eq!(name_from_file("not-a-package.txt"), None);
    }

    #[test]
    fn taille_et_vitesse_ne_se_confondent_pas() {
        let l = " fastfetch-2.67.1-1-x86_64.pkg.tar.zst   638.5 KiB  1863 KiB/s 00:00 [###] 100%";
        assert_eq!(read_size(l), Some(653_824));
        assert_eq!(read_speed(l).as_deref(), Some("1863 KiB/s"));
    }

    /// The core of what the run screen has to tell.
    #[test]
    fn a_complete_transaction_produces_its_events() {
        let j = replay(&[
            ":: Synchronizing package databases...",
            ":: Retrieving packages...",
            " fastfetch-2.67.1-1-x86_64.pkg.tar.zst  638.5 KiB  1863 KiB/s 00:00 [###] 100%",
            ":: Checking keyring...",
            "(1/1) checking keys in keyring [###] 100%",
            ":: Processing package changes...",
            "(1/1) upgrading fastfetch [###] 100%",
            ":: Running post-transaction hooks...",
            "(1/2) Arming ConditionNeedsUpdate...",
            "(2/2) Updating the desktop file MIME type cache...",
        ]);

        let verbs: Vec<Action> = j.events.iter().map(|e| e.action).collect();
        assert_eq!(
            verbs,
            vec![
                Action::Downloaded,
                Action::Verified,
                Action::Upgraded,
                Action::Hook,
                Action::Hook
            ]
        );
        assert_eq!(j.downloads.finished, vec!["fastfetch".to_string()]);
        assert_eq!(j.downloads.bytes, 653_824);
        assert_eq!(j.handled().collect::<Vec<_>>(), vec!["fastfetch"]);
        assert_eq!(j.phase, Phase::Hooks);
    }

    /// paru's own phases arrive after the install and must not wipe what has
    /// just been accomplished.
    #[test]
    fn parus_aur_phases_reset_nothing() {
        let j = replay(&[
            ":: Processing package changes...",
            "(1/1) upgrading fastfetch [###] 100%",
            ":: Looking for AUR upgrades...",
            ":: Looking for devel upgrades...",
        ]);
        assert_eq!(j.phase, Phase::Installing);
        assert_eq!(j.handled().collect::<Vec<_>>(), vec!["fastfetch"]);
    }

    /// A .pacnew file is the only trace of a configuration to merge, and it
    /// vanishes in the flood of warnings.
    #[test]
    fn pacnew_files_are_kept_apart_from_warnings() {
        let j = replay(&[
            "warning: /etc/pacman.conf installed as /etc/pacman.conf.pacnew",
            "warning: kitty: ignoring package upgrade (0.45.0-4 => 0.48.2-1)",
        ]);
        assert_eq!(j.pacnew, vec!["/etc/pacman.conf.pacnew".to_string()]);
        assert_eq!(j.warnings.len(), 1);
    }

    /// A build has no measurable progress. Carrying the counter of the phase
    /// before it into the build turned the bar into a flat 0.0 % for minutes,
    /// which reads as stuck rather than as unmeasurable.
    #[test]
    fn a_build_drops_the_counter_it_inherited() {
        let j = replay(&[
            ":: Processing package changes...",
            "(1/1) upgrading fastfetch [###] 100%",
            "==> Making package: plakar-git 1.0.3.r384",
            "==> Starting build()...",
        ]);
        assert_eq!(j.phase, Phase::Building);
        assert_eq!(j.counter, None);
        assert_eq!(j.percent, None);
    }

    /// makepkg raises its own warnings through the same marker as its steps.
    /// Filed as a step, a warning sat buried among forty others.
    #[test]
    fn makepkg_warnings_are_not_build_steps() {
        let j = replay(&[
            "==> Starting build()...",
            "==> WARNING: Using existing $srcdir/ tree",
            "==> ERROR: A failure occurred in build().",
        ]);
        assert_eq!(j.warnings, vec!["Using existing $srcdir/ tree".to_string()]);
        assert_eq!(j.errors, vec!["A failure occurred in build().".to_string()]);
        // Only the real step remains one.
        assert_eq!(j.events.len(), 1);
    }

    /// Two AUR packages can provide the same thing, and paru asks which one.
    /// The prompt that ends the question carries no clue as to what is being
    /// chosen, so the whole exchange has to be read to say anything useful.
    #[test]
    fn a_provider_question_is_read_whole() {
        let j = replay(&[
            ":: Resolving dependencies...",
            ":: There are 2 providers available for plakar:",
            ":: Repository AUR:",
            "    1) plakar  2) plakar-git",
            "Enter a number (default=1):",
        ]);
        let c = j.choice.expect("a question is pending");
        assert_eq!(c.about, "plakar");
        assert_eq!(c.default, Some(1));
        assert_eq!(c.candidates.len(), 2);
        assert_eq!(c.candidates[0].number, 1);
        assert_eq!(c.candidates[0].name, "plakar");
        assert_eq!(c.candidates[0].group, "AUR");
        assert_eq!(c.candidates[1].name, "plakar-git");
    }

    /// Answering it starts the work, and the question stops being one.
    #[test]
    fn the_question_goes_once_something_happens() {
        let j = replay(&[
            ":: There are 2 providers available for plakar:",
            ":: Repository AUR:",
            "    1) plakar  2) plakar-git",
            "Enter a number (default=1):",
            ":: Processing package changes...",
            "(1/1) installing plakar [###] 100%",
        ]);
        assert!(j.choice.is_none());
        assert_eq!(j.handled().collect::<Vec<_>>(), vec!["plakar"]);
    }

    /// paru resolves the AUR side itself, and says so only in the table it
    /// prints before asking to proceed. Reading it is the difference between
    /// "1 package, sizes unknown" and knowing that `go` is about to be pulled in
    /// to build the thing.
    #[test]
    fn parus_resolution_table_is_read() {
        let j = replay(&[
            ":: Resolving dependencies...",
            "Repo (1)        Old Version  New Version  Make Only",
            "extra/go                     2:1.26.6-1   Yes",
            "Aur (1)         Old Version  New Version  Make Only",
            "aur/plakar-git               1.0.3.r384.gd77c14a2-1  No",
            ":: Proceed with installation? [Y/n]:",
        ]);
        assert_eq!(j.resolution.len(), 2);

        let go = &j.resolution[0];
        assert_eq!((go.repo.as_str(), go.name.as_str()), ("extra", "go"));
        assert_eq!(go.from, None);
        assert_eq!(go.to, "2:1.26.6-1");
        // Build-only: it is here to compile something, not because it was wanted.
        assert!(go.make_only);

        let aur = &j.resolution[1];
        assert_eq!(aur.repo, "aur");
        assert!(!aur.make_only);
        assert_eq!(aur.to, "1.0.3.r384.gd77c14a2-1");
    }

    /// An upgrade row carries two versions where an install carries one.
    #[test]
    fn a_resolution_row_with_two_versions_is_an_upgrade() {
        let j = replay(&[
            "Repo (1)        Old Version  New Version  Make Only",
            "extra/foo       1.0-1        2.0-1        No",
        ]);
        assert_eq!(j.resolution[0].from.as_deref(), Some("1.0-1"));
        assert_eq!(j.resolution[0].to, "2.0-1");
    }

    /// The table ends where its rows stop, not on a marker: whatever follows
    /// must go back through the normal parsing.
    #[test]
    fn the_table_ends_when_the_rows_do() {
        let j = replay(&[
            "Repo (1)        Old Version  New Version  Make Only",
            "extra/go                     2:1.26.6-1   Yes",
            ":: Processing package changes...",
            "(1/1) upgrading fastfetch [###] 100%",
        ]);
        assert_eq!(j.resolution.len(), 1);
        assert_eq!(j.phase, Phase::Installing);
        assert_eq!(j.handled().collect::<Vec<_>>(), vec!["fastfetch"]);
    }

    /// The locale is pinned to C to make parsing reliable; letting that terse
    /// English reach the screen would merely move the problem.
    #[test]
    fn warnings_are_restated_as_sentences() {
        let j = replay(&["warning: kitty: ignoring package upgrade (0.45.0-4 => 0.48.2-1)"]);
        assert_eq!(
            j.warnings,
            vec!["kitty: upgrade skipped (0.45.0-4 → 0.48.2-1)".to_string()]
        );
        // An unknown message passes through: better than information lost.
        assert_eq!(translate_warning("something odd"), "something odd");
    }

    #[test]
    fn une_erreur_est_conservee_a_part() {
        let j = replay(&["error: failed to commit transaction"]);
        assert_eq!(j.errors, vec!["failed to commit transaction".to_string()]);
    }
}
