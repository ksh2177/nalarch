//! Transaction history, and rollback.
//!
//! One single source: `/var/log/pacman.log`. That is deliberate. nala keeps its
//! own journal, which only sees what went through nala; on Arch, everything
//! that touches packages goes through libalpm and lands in that file — pacman,
//! paru, a dependency pulled in by an install script. Reading it therefore
//! gives a complete, retroactive history, including what happened before
//! nalarch existed, with nothing to record ourselves.
//!
//! A rollback "undoes" nothing: it builds the inverse transaction and has
//! pacman carry it out from the packages still present in the caches. What a
//! scriptlet or a hook wrote outside the package manager — a database
//! migration, a rewritten configuration — is not covered.

use std::path::{Path, PathBuf};

const LOG: &str = "/var/log/pacman.log";
const CACHE_PACMAN: &str = "/var/cache/pacman/pkg";

/// pacman options whose value follows, and must therefore not be taken for a
/// target: `--ignore firefox` leaves firefox out, it is not what is installed.
const OPTIONS_WITH_VALUE: [&str; 14] = [
    "--ignore",
    "--ignoregroup",
    "--assume-installed",
    "--overwrite",
    "--dbpath",
    "--root",
    "--sysroot",
    "--cachedir",
    "--config",
    "--logfile",
    "--gpgdir",
    "--hookdir",
    "--arch",
    "--print-format",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Installed,
    Upgraded,
    Downgraded,
    Removed,
    Reinstalled,
}

impl Act {
    pub fn symbol(self) -> &'static str {
        match self {
            Act::Installed => "+",
            Act::Upgraded => "↑",
            Act::Downgraded => "↓",
            Act::Removed => "−",
            Act::Reinstalled => "⟳",
        }
    }

    pub fn label(self) -> &'static str {
        crate::i18n::t(match self {
            Act::Installed => "installed",
            Act::Upgraded => "upgraded",
            Act::Downgraded => "downgraded",
            Act::Removed => "removed",
            Act::Reinstalled => "reinstalled",
        })
    }

    /// Agreed with the count. English past participles do not inflect, but
    /// French ones do, so the plural is a message of its own.
    pub fn label_n(self, n: usize) -> String {
        if n <= 1 {
            return self.label().to_string();
        }
        crate::i18n::t(match self {
            Act::Installed => "plural|installed",
            Act::Upgraded => "plural|upgraded",
            Act::Downgraded => "plural|downgraded",
            Act::Removed => "plural|removed",
            Act::Reinstalled => "plural|reinstalled",
        })
        .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub act: Act,
    pub name: String,
    /// Version before the operation. Absent for an installation.
    pub before: Option<String>,
    /// Version after. Absent for a removal.
    pub after: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    /// Raw timestamp from the log, verbatim: `2026-08-18T00:38:46+0200`.
    pub timestamp: String,
    /// Seconds since the epoch, for the relative display.
    pub instant: Option<i64>,
    /// Command line behind the transaction, when the log carries it.
    pub command: Option<String>,
    pub operations: Vec<Operation>,
    pub warnings: Vec<String>,
    /// Time between opening and closing, in seconds.
    pub duration: Option<i64>,
    /// False when the log carries no close: the transaction was interrupted.
    pub completed: bool,
}

impl Transaction {
    pub fn count(&self, a: Act) -> usize {
        self.operations.iter().filter(|o| o.act == a).count()
    }

    /// One-line summary: what the transaction changed, in numbers.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for act in [
            Act::Upgraded,
            Act::Installed,
            Act::Removed,
            Act::Downgraded,
            Act::Reinstalled,
        ] {
            let n = self.count(act);
            if n > 0 {
                parts.push(format!("{} {}", n, act.label_n(n)));
            }
        }
        if parts.is_empty() {
            crate::i18n::t("no change").into()
        } else {
            parts.join(" · ")
        }
    }

    /// What triggered the transaction, described plainly.
    ///
    /// Copying the command line out does not inform: it is often unreadable
    /// (thirty `--ignore` flags when it comes from nalarch) or full of absolute
    /// paths when paru installs what it has just built. The intent is extracted
    /// instead; the raw command stays visible in the detail panel for anyone
    /// who wants to check.
    pub fn trigger(&self) -> String {
        let Some(cmd) = &self.command else {
            return crate::i18n::t("unknown origin").into();
        };
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let flags: String = words
            .iter()
            .filter(|m| m.starts_with('-'))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let short = |c: char| {
            words.iter().any(|m| {
                m.starts_with('-') && !m.starts_with("--") && m.contains(c)
            })
        };
        let long = |n: &str| words.contains(&n);

        // The targets: neither the options, nor the value following `--ignore`.
        let mut targets = Vec::new();
        let mut skip_next = false;
        for m in words.iter().skip(1) {
            if skip_next {
                skip_next = false;
                continue;
            }
            if OPTIONS_WITH_VALUE.contains(m) {
                skip_next = true;
                continue;
            }
            if m.starts_with('-') || *m == "--" {
                continue;
            }
            // An absolute path that is not a package is the value of an option
            // we do not know about yet: it is not a target.
            if m.starts_with('/') && !m.contains(".pkg.tar") {
                continue;
            }
            // A package path: only the name is shown, never the path.
            targets.push(package_from_path(m));
        }

        let sync = short('S') || long("--sync");
        let upgrade = short('u') || long("--upgrade") || long("--sysupgrade");
        let from_file = short('U') || long("--upgrade") && !sync;
        let removing = short('R') || long("--remove");

        let what = if sync && upgrade && targets.is_empty() {
            crate::i18n::t("system upgrade").to_string()
        } else if sync && upgrade {
            crate::i18n::tf("system upgrade · {0}", &[&list(&targets)])
        } else if from_file {
            crate::i18n::tf("install from file · {0}", &[&list(&targets)])
        } else if removing {
            crate::i18n::tf("removal · {0}", &[&list(&targets)])
        } else if sync {
            crate::i18n::tf("install · {0}", &[&list(&targets)])
        } else if !targets.is_empty() {
            list(&targets)
        } else if flags.is_empty() {
            crate::i18n::t("unknown origin").to_string()
        } else {
            format!("pacman {flags}")
        };
        what
    }

    /// True when the transaction mentions the searched text (a package name or
    /// the command): this is the view's filter.
    pub fn matches_text(&self, filter: &str) -> bool {
        let f = filter.to_lowercase();
        self.operations.iter().any(|o| o.name.to_lowercase().contains(&f))
            || self
                .command
                .as_deref()
                .map(|c| c.to_lowercase().contains(&f))
                .unwrap_or(false)
    }
}

/// `x`, `x y`, or `x y … (+3)` — a list row has no room for the rest.
fn list(noms: &[String]) -> String {
    match noms.len() {
        0 => "—".to_string(),
        1..=2 => noms.join(" "),
        _ => format!("{} … (+{})", noms[..2].join(" "), noms.len() - 2),
    }
}

/// `/var/cache/pacman/pkg/fastfetch-2.66.0-1-x86_64.pkg.tar.zst` → `fastfetch`.
fn package_from_path(target: &str) -> String {
    if !target.contains(".pkg.tar") {
        return target.to_string();
    }
    let base = target.rsplit('/').next().unwrap_or(target);
    match version_key(base) {
        // `name-version-pkgrel`: the last two segments are dropped.
        Some(key) => key
            .rsplitn(3, '-')
            .nth(2)
            .map(|s| s.to_string())
            .unwrap_or(key),
        None => base.to_string(),
    }
}

/// `fastfetch-2.66.0-1-x86_64.pkg.tar.zst` → `fastfetch-2.66.0-1`, the key that
/// identifies one precise version of a package.
fn version_key(fichier: &str) -> Option<String> {
    let sans_extension = fichier.split(".pkg.tar").next()?;
    // The last segment is the architecture.
    let (base, _arch) = sans_extension.rsplit_once('-')?;
    Some(base.to_string())
}

/// Reads pacman's log and extracts the transactions, most recent first.
/// Unrecognised lines are ignored: the log format is not an API, and showing
/// less beats reporting something false.
pub fn load() -> Vec<Transaction> {
    let text = std::fs::read_to_string(LOG).unwrap_or_default();
    analyze(&text)
}

pub fn analyze(text: &str) -> Vec<Transaction> {
    let mut transactions: Vec<Transaction> = Vec::new();
    let mut courante: Option<Transaction> = None;
    let mut derniere_commande: Option<String> = None;

    for line in text.lines() {
        let Some((timestamp, rest)) = bracketed(line) else {
            continue;
        };
        let Some((etiquette, contents)) = bracketed(rest) else {
            continue;
        };

        if etiquette == "PACMAN" {
            if let Some(cmd) = contents.strip_prefix("Running ") {
                derniere_commande = Some(cmd.trim_matches('\'').to_string());
            }
            continue;
        }
        if etiquette != "ALPM" {
            continue;
        }

        if contents == "transaction started" {
            // A transaction still open here was never closed: the log stops in
            // the middle, which means an interruption.
            if let Some(t) = courante.take() {
                transactions.push(t);
            }
            courante = Some(Transaction {
                instant: epoch(timestamp),
                timestamp: timestamp.to_string(),
                command: derniere_commande.take(),
                operations: Vec::new(),
                warnings: Vec::new(),
                duration: None,
                completed: false,
            });
            continue;
        }

        let Some(t) = courante.as_mut() else {
            continue;
        };

        if contents == "transaction completed" {
            t.completed = true;
            t.duration = match (t.instant, epoch(timestamp)) {
                (Some(a), Some(b)) => Some(b - a),
                _ => None,
            };
            transactions.push(courante.take().unwrap());
            continue;
        }

        if let Some(av) = contents.strip_prefix("warning: ") {
            t.warnings.push(av.to_string());
            continue;
        }

        if let Some(op) = operation(contents) {
            t.operations.push(op);
        }
    }

    if let Some(t) = courante.take() {
        transactions.push(t);
    }

    // Transactions with no operation at all are noise: a bare `-Sy`, or a
    // transaction cancelled before writing anything.
    transactions.retain(|t| !t.operations.is_empty());
    transactions.reverse();
    transactions
}

/// `[value] rest` → `(value, rest)`.
fn bracketed(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let ended = rest.find(']')?;
    Some((&rest[..ended], rest[ended + 1..].trim_start()))
}

/// `upgraded fastfetch (2.66.0-1 -> 2.67.1-1)` → the matching operation.
fn operation(contents: &str) -> Option<Operation> {
    let (verbe, rest) = contents.split_once(' ')?;
    let act = match verbe {
        "installed" => Act::Installed,
        "upgraded" => Act::Upgraded,
        "downgraded" => Act::Downgraded,
        "removed" => Act::Removed,
        "reinstalled" => Act::Reinstalled,
        _ => return None,
    };
    let (name, versions) = rest.split_once(" (")?;
    let versions = versions.strip_suffix(')')?;
    let (before, after) = match versions.split_once(" -> ") {
        Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
        None => match act {
            Act::Removed => (Some(versions.to_string()), None),
            _ => (None, Some(versions.to_string())),
        },
    };
    Some(Operation {
        act,
        name: name.to_string(),
        before,
        after,
    })
}

/// `2026-08-18T00:38:46+0200` → seconds since the epoch.
fn epoch(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let nombre = |a: usize, b: usize| ts.get(a..b)?.parse::<i64>().ok();
    let (an, mois, jour) = (nombre(0, 4)?, nombre(5, 7)?, nombre(8, 10)?);
    let (h, mi, s) = (nombre(11, 13)?, nombre(14, 16)?, nombre(17, 19)?);
    let mut seconds = days_from_civil(an, mois, jour) * 86_400 + h * 3600 + mi * 60 + s;
    // Time zone offset: `+0200` means the time read is two hours ahead of UTC,
    // so it is subtracted to recover the absolute instant.
    if let Some(rest) = ts.get(19..) {
        let sign = match rest.chars().next() {
            Some('+') => 1,
            Some('-') => -1,
            _ => 0,
        };
        if sign != 0 {
            let hh: i64 = rest.get(1..3).and_then(|v| v.parse().ok()).unwrap_or(0);
            let mm: i64 = rest.get(3..5).and_then(|v| v.parse().ok()).unwrap_or(0);
            seconds -= sign * (hh * 3600 + mm * 60);
        }
    }
    Some(seconds)
}

/// Days between 1970-01-01 and the given date (Hinnant's algorithm).
fn days_from_civil(an: i64, mois: i64, jour: i64) -> i64 {
    let an = an - if mois <= 2 { 1 } else { 0 };
    let ere = if an >= 0 { an } else { an - 399 } / 400;
    let annee_ere = an - ere * 400;
    let jour_annee = (153 * (mois + if mois > 2 { -3 } else { 9 }) + 2) / 5 + jour - 1;
    let jour_ere = annee_ere * 365 + annee_ere / 4 - annee_ere / 100 + jour_annee;
    ere * 146_097 + jour_ere - 719_468
}

/// "3 h ago", "2 d ago" — an absolute date on its own forces the reader to do
/// the arithmetic just to know whether it is recent.
pub fn relative_time(instant: Option<i64>) -> String {
    let Some(t) = instant else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(t);
    let d = now - t;
    if d < 0 {
        return String::new();
    }
    use crate::i18n::tf;
    match d {
        0..=59 => crate::i18n::t("just now").into(),
        60..=3599 => tf("{0} min ago", &[&(d / 60).to_string()]),
        3600..=86_399 => tf("{0} h ago", &[&(d / 3600).to_string()]),
        86_400..=2_591_999 => tf("{0} d ago", &[&(d / 86_400).to_string()]),
        _ => tf("{0} months ago", &[&(d / 2_592_000).to_string()]),
    }
}

/// Readable date, without the time zone or the seconds.
pub fn short_date(timestamp: &str) -> String {
    timestamp
        .get(..16)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| timestamp.to_string())
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Source {
    /// The package file for the version to restore, found in a local cache.
    File(PathBuf),
    /// Nothing to reinstall: removing what was added is enough.
    Remove,
    /// Version not found: this part of the rollback cannot happen.
    Missing,
}

#[derive(Debug, Clone)]
pub struct Rollback {
    pub name: String,
    /// Version currently installed, according to the transaction.
    pub current: Option<String>,
    /// Version the rollback would restore.
    pub target: Option<String>,
    pub source: Source,
}

impl Rollback {
    pub fn is_possible(&self) -> bool {
        !matches!(self.source, Source::Missing)
    }
}

/// Inverse of a past transaction.
///
/// The direction is strictly mechanical: what was installed is removed, what
/// was upgraded goes back down, what was removed comes back. A reinstall has no
/// inverse and is skipped.
pub fn rollback_plan(caches: &Caches, t: &Transaction) -> Vec<Rollback> {
    let mut rollbacks = Vec::new();
    for op in &t.operations {
        let rollback = match op.act {
            Act::Installed => Rollback {
                name: op.name.clone(),
                current: op.after.clone(),
                target: None,
                source: Source::Remove,
            },
            Act::Upgraded | Act::Downgraded | Act::Removed => {
                let target = op.before.clone();
                let source = match target.as_deref().and_then(|v| caches.find(&op.name, v)) {
                    Some(p) => Source::File(p),
                    None => Source::Missing,
                };
                Rollback {
                    name: op.name.clone(),
                    current: op.after.clone(),
                    target,
                    source,
                }
            }
            Act::Reinstalled => continue,
        };
        rollbacks.push(rollback);
    }
    rollbacks
}

/// Index of the versions present in the local caches.
///
/// Built once, not on every frame: the question "is this version still
/// recoverable?" is asked for every package of every transaction, and walking
/// six thousand files per frame would be absurd.
///
/// Two locations, because they do not hold the same thing: pacman's cache keeps
/// what comes from the repositories, paru's keeps the AUR packages it built —
/// and those never pass through `/var/cache/pacman/pkg`.
#[derive(Debug, Default, Clone)]
pub struct Caches {
    versions: std::collections::HashMap<String, PathBuf>,
}

impl Caches {
    pub fn index() -> Self {
        let mut versions = std::collections::HashMap::new();
        index_dir(Path::new(CACHE_PACMAN), &mut versions);
        if let Ok(home) = std::env::var("HOME") {
            let clone = PathBuf::from(home).join(".cache/paru/clone");
            if let Ok(entries) = std::fs::read_dir(&clone) {
                for e in entries.flatten() {
                    index_dir(&e.path(), &mut versions);
                }
            }
        }
        Self { versions }
    }

    pub fn find(&self, name: &str, version: &str) -> Option<PathBuf> {
        self.versions.get(&format!("{name}-{version}")).cloned()
    }
}

fn index_dir(dir: &Path, index: &mut std::collections::HashMap<String, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        // The signature travels with the package but is not installed.
        if name.ends_with(".sig") || !name.contains(".pkg.tar") {
            continue;
        }
        if let Some(key) = version_key(&name) {
            // pacman's cache was indexed first: a repository package wins over a
            // locally built namesake, since that is the one pacman would reinstall.
            index.entry(key).or_insert_with(|| e.path());
        }
    }
}

/// The rollback command.
///
/// pacman does not mix installation and removal in one invocation: when the
/// rollback needs both, they are chained. Installation goes first — if a removal
/// then became impossible because the restored version still depends on it,
/// pacman refuses instead of breaking the system.
pub fn rollback_command(rollbacks: &[Rollback]) -> Option<Vec<String>> {
    let files: Vec<String> = rollbacks
        .iter()
        .filter_map(|r| match &r.source {
            Source::File(p) => Some(p.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    let removals: Vec<String> = rollbacks
        .iter()
        .filter(|r| matches!(r.source, Source::Remove))
        .map(|r| r.name.clone())
        .collect();

    let mut steps: Vec<String> = Vec::new();
    if !files.is_empty() {
        steps.push(format!(
            "pacman -U --noconfirm {}",
            files.iter().map(|f| quote(f)).collect::<Vec<_>>().join(" ")
        ));
    }
    if !removals.is_empty() {
        // Plain `-R`, without `-s`: the transaction already listed the
        // dependencies that arrived with the package, so naming them is the exact
        // inverse. A cascade would go further than what is being undone.
        steps.push(format!("pacman -R --noconfirm {}", removals.join(" ")));
    }
    match steps.len() {
        0 => None,
        1 => {
            let mut cmd = vec!["sudo".to_string()];
            cmd.extend(steps[0].split_whitespace().map(|s| s.to_string()));
            Some(cmd)
        }
        _ => Some(vec![
            "sudo".into(),
            "sh".into(),
            "-c".into(),
            steps.join(" && "),
        ]),
    }
}

/// Readable form of the rollback command: cache paths are counted, not listed.
/// The real command is still the one that runs.
pub fn readable_command(rollbacks: &[Rollback]) -> String {
    let files = rollbacks
        .iter()
        .filter(|r| matches!(r.source, Source::File(_)))
        .count();
    let removals: Vec<&str> = rollbacks
        .iter()
        .filter(|r| matches!(r.source, Source::Remove))
        .map(|r| r.name.as_str())
        .collect();
    let mut steps = Vec::new();
    if files > 0 {
        steps.push(crate::i18n::tf(
            "sudo pacman -U ({0} package(s) taken from the local caches)",
            &[&files.to_string()],
        ));
    }
    if !removals.is_empty() {
        steps.push(format!("sudo pacman -R {}", list(&removals.iter().map(|s| s.to_string()).collect::<Vec<_>>())));
    }
    steps.join(" && ")
}

fn quote(path: &str) -> String {
    if path.contains(' ') {
        format!("'{path}'")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXCERPT: &str = "\
[2026-08-18T00:38:45+0200] [PACMAN] Running 'pacman --sync -y -u --noconfirm --'
[2026-08-18T00:38:45+0200] [PACMAN] synchronizing package lists
[2026-08-18T00:38:46+0200] [ALPM] transaction started
[2026-08-18T00:38:46+0200] [ALPM] upgraded fastfetch (2.66.0-1 -> 2.67.1-1)
[2026-08-18T00:38:47+0200] [ALPM] installed libtruc (1.0-1)
[2026-08-18T00:38:48+0200] [ALPM] removed vieux (3.2-1)
[2026-08-18T00:38:49+0200] [ALPM] warning: /etc/foo.conf installed as /etc/foo.conf.pacnew
[2026-08-18T00:38:50+0200] [ALPM] transaction completed
[2026-08-18T00:38:50+0200] [ALPM] running '35-systemd-update.hook'...
";

    #[test]
    fn a_transaction_is_rebuilt_with_its_operations() {
        let t = analyze(EXCERPT);
        assert_eq!(t.len(), 1);
        let t = &t[0];
        assert!(t.completed);
        assert_eq!(t.duration, Some(4));
        assert_eq!(t.operations.len(), 3);
        assert_eq!(t.warnings.len(), 1);
        assert_eq!(t.command.as_deref(), Some("pacman --sync -y -u --noconfirm --"));
    }

    #[test]
    fn versions_are_read_the_right_way_round() {
        let t = &analyze(EXCERPT)[0];
        let updates = &t.operations[0];
        assert_eq!(updates.act, Act::Upgraded);
        assert_eq!(updates.before.as_deref(), Some("2.66.0-1"));
        assert_eq!(updates.after.as_deref(), Some("2.67.1-1"));
        // A removal has no "after": the recorded version is the one that
        // disappeared, and confusing them would invert the rollback.
        let sup = &t.operations[2];
        assert_eq!(sup.act, Act::Removed);
        assert_eq!(sup.before.as_deref(), Some("3.2-1"));
        assert_eq!(sup.after, None);
    }

    #[test]
    fn the_rollback_inverts_every_operation() {
        let t = &analyze(EXCERPT)[0];
        let r = rollback_plan(&Caches::default(), t);
        assert_eq!(r.len(), 3);
        // Upgrade → the previous version is the target.
        assert_eq!(r[0].target.as_deref(), Some("2.66.0-1"));
        // Empty index: the version is nowhere, and the rollback says so instead
        // of letting it be assumed.
        assert!(!r[0].is_possible());
        // Install → a plain removal, no file needed.
        assert!(matches!(r[1].source, Source::Remove));
        assert!(r[1].is_possible());
        // Removal → the vanished version is the target.
        assert_eq!(r[2].target.as_deref(), Some("3.2-1"));
    }

    #[test]
    fn an_interrupted_transaction_stays_marked_incomplete() {
        let truncated = "\
[2026-08-18T00:38:46+0200] [ALPM] transaction started
[2026-08-18T00:38:46+0200] [ALPM] upgraded fastfetch (2.66.0-1 -> 2.67.1-1)
";
        let t = analyze(truncated);
        assert_eq!(t.len(), 1);
        assert!(!t[0].completed);
    }

    #[test]
    fn transactions_with_no_operation_are_not_listed() {
        let empty = "\
[2026-08-18T00:34:21+0200] [PACMAN] Running 'pacman --sync -y -u --'
[2026-08-18T00:34:21+0200] [PACMAN] synchronizing package lists
";
        assert!(analyze(empty).is_empty());
    }

    fn with_command(cmd: &str) -> Transaction {
        Transaction {
            timestamp: String::new(),
            instant: None,
            command: Some(cmd.into()),
            operations: Vec::new(),
            warnings: Vec::new(),
            duration: None,
            completed: true,
        }
    }

    #[test]
    fn the_trigger_does_not_mistake_an_ignore_for_a_target() {
        // `firefox` follows `--ignore`: it is what was left out, certainly not
        // the transaction's target.
        let t = with_command("paru -Syu --ignore firefox elfutils");
        assert_eq!(t.trigger(), "system upgrade · elfutils");
    }

    #[test]
    fn the_trigger_names_the_package_not_the_path() {
        let t = with_command(
            "pacman -U /var/cache/pacman/pkg/fastfetch-2.66.0-1-x86_64.pkg.tar.zst",
        );
        assert_eq!(t.trigger(), "install from file · fastfetch");
    }

    #[test]
    fn a_full_upgrade_says_so_plainly() {
        assert_eq!(
            with_command("pacman --sync -y -u --").trigger(),
            "system upgrade"
        );
        assert_eq!(
            with_command("pacman -R --noconfirm kdeconnect").trigger(),
            "removal · kdeconnect"
        );
    }

    #[test]
    fn the_version_key_ignores_architecture_and_extension() {
        assert_eq!(
            version_key("fastfetch-2.66.0-1-x86_64.pkg.tar.zst").as_deref(),
            Some("fastfetch-2.66.0-1")
        );
        // A package whose name contains dashes must not be truncated.
        assert_eq!(
            version_key("qt6-wayland-6.9.0-2-x86_64.pkg.tar.zst").as_deref(),
            Some("qt6-wayland-6.9.0-2")
        );
    }

    #[test]
    fn the_rollback_command_chains_install_then_removal() {
        let rollbacks = vec![
            Rollback {
                name: "fastfetch".into(),
                current: Some("2.67.1-1".into()),
                target: Some("2.66.0-1".into()),
                source: Source::File("/var/cache/pacman/pkg/f.pkg.tar.zst".into()),
            },
            Rollback {
                name: "libtruc".into(),
                current: Some("1.0-1".into()),
                target: None,
                source: Source::Remove,
            },
        ];
        let cmd = rollback_command(&rollbacks).unwrap();
        assert_eq!(cmd[0], "sudo");
        assert_eq!(cmd[1], "sh");
        let script = &cmd[3];
        assert!(script.starts_with("pacman -U"));
        assert!(script.contains("&& pacman -R --noconfirm libtruc"));
    }

    #[test]
    fn a_timestamp_converts_to_an_absolute_instant() {
        // 2026-08-18T00:38:46+0200 = 2026-08-17T22:38:46Z
        let a = epoch("2026-08-18T00:38:46+0200").unwrap();
        let b = epoch("2026-08-17T22:38:46+0000").unwrap();
        assert_eq!(a, b);
    }
}
