//! Transaction plan.
//!
//! The table paru prints is never re-parsed. The same information is available
//! in structured form: versions and sizes come from alpm, and the exact makeup
//! of the transaction — new dependencies included — from
//! `pacman -Sup --print-format`, which needs no privilege and writes nothing.

use crate::data::{self, State, Origin};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Already installed, moving to a newer version.
    Upgrade,
    /// Not installed: arrives as a dependency of something being upgraded.
    New,
    /// Back to an earlier version.
    Downgrade,
    /// Removal asked for explicitly.
    Removal,
    /// Removal pulled in by the `-s` cascade: a dependency nothing needs now.
    AutoRemoval,
}

impl Kind {
    pub fn symbol(self) -> &'static str {
        match self {
            Kind::Upgrade => "↑",
            Kind::New => "+",
            Kind::Downgrade => "↓",
            Kind::Removal => "−",
            Kind::AutoRemoval => "⌫",
        }
    }

    pub fn label(self) -> &'static str {
        crate::i18n::t(match self {
            Kind::Upgrade => "Updates",
            Kind::New => "New packages",
            Kind::Downgrade => "Downgrades",
            Kind::Removal => "Removals",
            Kind::AutoRemoval => "Removed as a cascade",
        })
    }

    /// Every kind, in the order they read.
    pub const ALL: [Kind; 5] = [
        Kind::Upgrade,
        Kind::New,
        Kind::Downgrade,
        Kind::Removal,
        Kind::AutoRemoval,
    ];
}

pub struct PlanRow {
    pub name: String,
    pub repo: String,
    pub from_version: String,
    pub to_version: String,
    pub dl: Option<i64>,
    /// Change in disk space. Unknown when the sync database does not yet carry
    /// the target version — the normal case for the AUR, which gets built.
    pub net: Option<i64>,
    pub aur: bool,
    pub kind: Kind,
    /// The offered version is older than the installed one.
    pub is_downgrade: bool,
}

pub struct Plan {
    pub rows: Vec<PlanRow>,
    pub total_dl: i64,
    pub total_installed: i64,
    pub net: i64,
    pub aur_count: usize,
    /// Packages whose size is unknown: the totals are therefore a lower bound.
    pub unknown: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn count(&self, k: Kind) -> usize {
        self.rows.iter().filter(|r| r.kind == k).count()
    }

    /// True when at least one package has known sizes. When the whole plan is
    /// AUR, the totals are zero not because there is nothing to do but because
    /// nothing is measurable before building: showing them would be a lie.
    pub fn sizes_known(&self) -> bool {
        self.unknown < self.rows.len()
    }
}

/// Builds the upgrade command matching a selection.
///
/// The central trap: `paru -Syu <package>` does not upgrade that package alone.
/// `-u` asks for a full system upgrade, and the named package is merely one
/// more target added to the transaction — naming `elfutils` therefore also
/// upgraded the other five, plus the AUR. The only way to narrow it down is to
/// exclude the others with `--ignore`, the option pacman provides for it, which
/// paru applies to AUR packages as well.
pub fn upgrade_command(available: &[String], targets: &[String], aur: bool) -> Vec<String> {
    let mut cmd = vec!["paru".to_string(), "-Syu".to_string()];
    for name in available.iter().filter(|n| !targets.contains(n)) {
        cmd.push("--ignore".to_string());
        cmd.push(name.clone());
    }
    // With no AUR package the transaction is fully known: it comes from the
    // same pacman resolution as the one shown in the summary. Asking again
    // would make the user re-read a table they have just approved, in a less
    // readable form.
    //
    // As soon as an AUR package is involved, paru keeps its own questions:
    // reading the PKGBUILD, picking a provider, importing PGP keys. Those are
    // decisions nalarch cannot make on their behalf, and suppressing them would
    // remove the one chance to read the script that is about to run.
    if !aur {
        cmd.push("--noconfirm".to_string());
    }
    cmd
}

/// Packages left out of an upgrade, derived from what is checked.
pub fn exclusions(available: &[String], targets: &[String]) -> Vec<String> {
    available
        .iter()
        .filter(|n| !targets.contains(n))
        .cloned()
        .collect()
}

/// A plan with no per-package detail, for actions that have none (cache).
pub fn empty() -> Plan {
    Plan {
        rows: Vec::new(),
        total_dl: 0,
        total_installed: 0,
        net: 0,
        aur_count: 0,
        unknown: 0,
    }
}

/// A rollback plan built from a transaction in the log.
///
/// Sizes stay unknown: the packages come from local caches, there is nothing to
/// download, and the space used afterwards cannot be read anywhere without
/// opening every archive. Showing zeros would suggest a transaction with no
/// effect.
pub fn from_rollback(rollbacks: &[crate::history::Rollback]) -> Plan {
    use crate::history::Source;
    let mut rows = Vec::new();
    for r in rollbacks {
        let kind = match (&r.source, &r.current) {
            (Source::Missing, _) => continue,
            (Source::Remove, _) => Kind::Removal,
            // The package had been removed: the rollback reinstalls it.
            (Source::File(_), None) => Kind::New,
            (Source::File(_), Some(_)) => Kind::Downgrade,
        };
        // Where the file comes from is decided before translating it: the flag
        // must not depend on the interface language.
        let from_paru = matches!(&r.source,
            Source::File(p) if p.to_string_lossy().contains("/.cache/paru/"));
        let repo = match &r.source {
            Source::File(_) if from_paru => crate::i18n::t("paru cache"),
            Source::File(_) => crate::i18n::t("cache"),
            _ => crate::i18n::t("installed"),
        };
        rows.push(PlanRow {
            name: r.name.clone(),
            repo: repo.to_string(),
            from_version: r.current.clone().unwrap_or_else(|| "—".into()),
            to_version: r.target.clone().unwrap_or_else(|| "—".into()),
            dl: None,
            net: None,
            aur: from_paru,
            kind,
            is_downgrade: kind == Kind::Downgrade,
        });
    }
    with_totals(Plan {
        rows,
        total_dl: 0,
        total_installed: 0,
        net: 0,
        aur_count: 0,
        unknown: 0,
    })
}

fn with_totals(mut p: Plan) -> Plan {
    p.total_dl = p.rows.iter().filter_map(|l| l.dl).sum();
    p.net = p.rows.iter().filter_map(|l| l.net).sum();
    p.aur_count = p.rows.iter().filter(|l| l.aur).count();
    p.unknown = p.rows.iter().filter(|l| l.net.is_none()).count();
    p
}

/// Plan of a removal: nothing to download, and "net" is the space handed back
/// to the disk, therefore negative.
pub fn removal_plan(state: &State, names: &[String]) -> Plan {
    // The cascade is authoritative: `-s` takes along dependencies nothing needs
    // any more, and those are often the bulk of the operation.
    let cascade = data::removal_cascade(names);
    let source: Vec<String> = if cascade.is_empty() {
        names.to_vec()
    } else {
        cascade.into_iter().map(|r| r.name).collect()
    };

    let rows = source
        .iter()
        .filter_map(|name| state.installed.iter().find(|p| &p.name == name))
        .map(|p| PlanRow {
            name: p.name.clone(),
            repo: p.repo.clone(),
            from_version: p.version.clone(),
            to_version: String::new(),
            dl: None,
            net: Some(-p.installed_size),
            aur: p.origin == Origin::Aur,
            kind: if names.contains(&p.name) {
                Kind::Removal
            } else {
                Kind::AutoRemoval
            },
            is_downgrade: false,
        })
        .collect();

    let mut p = with_totals(Plan {
        rows,
        ..empty()
    });
    p.total_installed = 0;
    p
}

/// Immediate estimate, without a subprocess.
///
/// Feeds the table's status line, redrawn on every keystroke: running a full
/// resolution there would restart pacman continuously. It therefore only knows
/// about packages already listed, without the new dependencies.
pub fn quick_plan(state: &State, names: &[String]) -> Plan {
    let rows = names
        .iter()
        .filter_map(|name| state.updates.iter().find(|p| &p.name == name))
        .map(|p| PlanRow {
            name: p.name.clone(),
            repo: p.repo.clone(),
            from_version: p.version.clone(),
            to_version: p.target_version.clone().unwrap_or_default(),
            dl: p.download_size,
            net: p.target_size.map(|c| c - p.installed_size),
            aur: p.origin == Origin::Aur,
            kind: Kind::Upgrade,
            is_downgrade: false,
        })
        .collect();

    let mut p = with_totals(Plan { rows, ..empty() });
    p.total_installed = 0;
    p
}

/// Full plan of an upgrade, as pacman would resolve it.
///
/// Unlike `quick_plan`, this one queries pacman and therefore surfaces the
/// packages nobody asked for but that arrive as dependencies. That is the
/// information most often missed in the usual output: it is there, buried in
/// the middle of everything else.
pub fn build(state: &State, targets: &[String], excluded: &[String]) -> Plan {
    let mut rows = Vec::new();
    let mut total_installed = 0;

    for r in data::resolved_transaction(excluded) {
        let installed = state.installed.iter().find(|p| p.name == r.name);
        let sync = state.sync.get(&r.name);

        // Sizes only hold if the sync database really carries the target version.
        let (dl, target_size) = match sync.filter(|(v, _, _)| *v == r.version) {
            Some((_, d, i)) => (Some(*d), Some(*i)),
            None => (None, None),
        };
        if let Some(i) = target_size {
            total_installed += i;
        }

        let (kind, from_version, current_size) = match installed {
            Some(p) => {
                let backwards = alpm::vercmp(r.version.as_str(), p.version.as_str())
                    == std::cmp::Ordering::Less;
                let k = if backwards { Kind::Downgrade } else { Kind::Upgrade };
                (k, p.version.clone(), p.installed_size)
            }
            None => (Kind::New, String::new(), 0),
        };

        rows.push(PlanRow {
            is_downgrade: installed.is_some_and(|p| {
                alpm::vercmp(r.version.as_str(), p.version.as_str())
                    == std::cmp::Ordering::Less
            }),
            name: r.name,
            repo: r.repo,
            from_version,
            to_version: r.version,
            dl,
            net: target_size.map(|c| c - current_size),
            aur: false,
            kind,
        });
    }

    // pacman ignores the AUR: those packages come from paru and have, by
    // nature, no known size before building.
    for name in targets {
        let Some(p) = state
            .updates
            .iter()
            .find(|p| &p.name == name && p.origin == Origin::Aur)
        else {
            continue;
        };
        rows.push(PlanRow {
            name: p.name.clone(),
            repo: p.repo.clone(),
            from_version: p.version.clone(),
            to_version: p.target_version.clone().unwrap_or_default(),
            dl: None,
            net: None,
            aur: true,
            kind: Kind::Upgrade,
            is_downgrade: false,
        });
    }

    // Updates first, new packages next, AUR last: that is the order it reads
    // in, and the AUR is what deserves the most attention.
    rows.sort_by(|a, b| {
        (a.aur, a.kind == Kind::New, &a.name).cmp(&(b.aur, b.kind == Kind::New, &b.name))
    });

    let mut p = with_totals(Plan { rows, ..empty() });
    p.total_installed = total_installed;
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn checking_everything_gives_a_full_system_upgrade() {
        let available = v(&["elfutils", "libelf", "tmux"]);
        assert_eq!(
            upgrade_command(&available, &available, false),
            v(&["paru", "-Syu", "--noconfirm"])
        );
    }

    /// An AUR package must leave paru its questions: that is the only chance to
    /// read the PKGBUILD before it runs.
    #[test]
    fn an_aur_package_keeps_parus_questions() {
        let available = v(&["elfutils"]);
        let cmd = upgrade_command(&available, &available, true);
        assert!(!cmd.contains(&"--noconfirm".to_string()));
    }

    /// The case that was wrong: `paru -Syu elfutils` upgraded everything.
    #[test]
    fn checking_one_package_excludes_all_the_others() {
        let available = v(&["elfutils", "libelf", "tmux"]);
        let cmd = upgrade_command(&available, &v(&["elfutils"]), true);
        assert_eq!(
            cmd,
            v(&["paru", "-Syu", "--ignore", "libelf", "--ignore", "tmux"])
        );
        // The wanted package must never be passed as a target: it would have no
        // effect here, and would mislead anyone reading the command.
        assert!(!cmd.contains(&"elfutils".to_string()));
    }

    #[test]
    fn checking_nothing_excludes_everything() {
        let available = v(&["elfutils", "tmux"]);
        let cmd = upgrade_command(&available, &[], true);
        assert_eq!(cmd.iter().filter(|a| *a == "--ignore").count(), 2);
    }

    #[test]
    fn aur_packages_are_excluded_like_any_other() {
        let available = v(&["elfutils", "infisical-bin"]);
        let cmd = upgrade_command(&available, &v(&["elfutils"]), true);
        assert_eq!(cmd, v(&["paru", "-Syu", "--ignore", "infisical-bin"]));
    }

    #[test]
    fn exclusions_are_the_complement_of_the_targets() {
        let available = v(&["a", "b", "c"]);
        assert_eq!(exclusions(&available, &v(&["b"])), v(&["a", "c"]));
        assert!(exclusions(&available, &available).is_empty());
    }
}
