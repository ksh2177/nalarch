//! Points of attention in a transaction.
//!
//! pacman's output holds everything needed to judge whether an operation is
//! harmless, but buried in the middle of everything else: an `IgnorePkg`
//! warning looks exactly like a download line, and nothing signals that a
//! PKGBUILD is about to run under your account. This module surfaces those and
//! states them plainly.
//!
//! The rule followed here: only announce what is **verified** from the data. A
//! vague warning is quickly ignored, and a list that gets ignored protects
//! nothing.

use crate::data::State;
use crate::i18n::{t, tf};
use crate::plan::{Kind, Plan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Worth knowing, without consequence.
    Info,
    /// Deserves a look before approving.
    Caution,
    /// Can break the system, or run code nobody reviewed.
    Serious,
}

pub struct Risk {
    pub level: Level,
    pub title: String,
    /// One sentence explaining the concrete consequence, not the mechanism.
    pub detail: String,
}

/// Packages whose upgrade only takes effect after a reboot.
const KERNELS: [&str; 6] = [
    "linux",
    "linux-lts",
    "linux-zen",
    "linux-hardened",
    "linux-rt",
    "linux-rt-lts",
];

/// Building blocks the whole rest of the system depends on.
const CRITICAL: [&str; 6] = ["glibc", "systemd", "pacman", "openssl", "gcc-libs", "bash"];

/// Packages in the plan whose upgrade only takes effect after a reboot.
///
/// pacman does not say so: the system keeps running on the old kernel, whose
/// modules are no longer on disk. The symptom shows up much later — a device
/// that refuses to mount — and no longer connects back to the upgrade.
pub fn needs_reboot(plan: &Plan) -> Vec<String> {
    plan.rows
        .iter()
        .filter(|l| KERNELS.contains(&l.name.as_str()) || CRITICAL.contains(&l.name.as_str()))
        .map(|l| l.name.clone())
        .collect()
}

pub fn analyze(plan: &Plan, state: &State, excluded: &[String], removal: bool) -> Vec<Risk> {
    let mut r = Vec::new();

    if removal {
        analyze_removal(plan, state, &mut r);
    } else {
        analyze_upgrade(plan, state, excluded, &mut r);
    }

    // Most severe first: that is what gets read when only one line is read.
    r.sort_by_key(|x| match x.level {
        Level::Serious => 0,
        Level::Caution => 1,
        Level::Info => 2,
    });
    r
}

fn analyze_upgrade(plan: &Plan, state: &State, excluded: &[String], r: &mut Vec<Risk>) {
    // — Unreviewed code running under your account.
    let aur: Vec<&str> = plan
        .rows
        .iter()
        .filter(|l| l.aur)
        .map(|l| l.name.as_str())
        .collect();
    if !aur.is_empty() {
        r.push(Risk {
            level: Level::Serious,
            title: tf("{0} package(s) built from the AUR: {1}", &[&aur.len().to_string(), &aur.join(", ")]),
            detail: t("A PKGBUILD is a script run under your account, reviewed by nobody. paru will offer to show it before building: that is the moment to read it.").into(),
        });
    }

    // — Partial upgrade: the Arch manual advises against it explicitly, because
    //   packages are built against one another.
    if !excluded.is_empty() {
        r.push(Risk {
            level: Level::Serious,
            title: tf("Partial upgrade: {0} package(s) left out", &[&excluded.len().to_string()]),
            detail: tf(
                "Left out: {0}. Arch packages are compiled against one another; keeping old ones alongside new ones can break programs with no apparent connection.",
                &[&excluded.join(", ")],
            ),
        });
    }

    // — Kernel: the running kernel's modules disappear from disk, which shows
    //   up much later and is bewildering.
    let kernels: Vec<&str> = plan
        .rows
        .iter()
        .filter(|l| KERNELS.contains(&l.name.as_str()))
        .map(|l| l.name.as_str())
        .collect();
    if !kernels.is_empty() {
        let dkms = state
            .installed
            .iter()
            .filter(|p| p.name.ends_with("-dkms"))
            .count();
        let mut detail = t("Until you reboot, plugging in a device or mounting an unusual filesystem may fail: the running kernel's modules are no longer on disk.").to_string();
        if dkms > 0 {
            detail.push(' ');
            detail.push_str(&tf(
                "{0} DKMS module(s) will be rebuilt, which lengthens the operation.",
                &[&dkms.to_string()],
            ));
        }
        r.push(Risk {
            level: Level::Caution,
            title: tf("Kernel upgraded ({0}): reboot required", &[&kernels.join(", ")]),
            detail,
        });
    }

    // — Critical system pieces: an interruption here leaves the machine in a
    //   state that is hard to recover from.
    let critical: Vec<&str> = plan
        .rows
        .iter()
        .filter(|l| CRITICAL.contains(&l.name.as_str()))
        .map(|l| l.name.as_str())
        .collect();
    if !critical.is_empty() {
        r.push(Risk {
            level: Level::Caution,
            title: tf("Essential system components: {0}", &[&critical.join(", ")]),
            detail: t("Do not interrupt the operation once started. A cut here can stop the machine from booting.").into(),
        });
    }

    // — Downgrades: rare, and almost always unintended.
    let rollbacks: Vec<&str> = plan
        .rows
        .iter()
        .filter(|l| l.is_downgrade)
        .map(|l| l.name.as_str())
        .collect();
    if !rollbacks.is_empty() {
        r.push(Risk {
            level: Level::Caution,
            title: tf("Back to an earlier version: {0}", &[&rollbacks.join(", ")]),
            detail: t("The offered version is older than the installed one. Normal after a deliberate rollback, suspicious otherwise.").into(),
        });
    }

    // — New dependencies: what pacman's output drowns and checkupdates does not
    //   show at all.
    let new_count = plan.count(Kind::New);
    if new_count > 0 {
        let names: Vec<&str> = plan
            .rows
            .iter()
            .filter(|l| l.kind == Kind::New)
            .map(|l| l.name.as_str())
            .take(6)
            .collect();
        r.push(Risk {
            level: Level::Info,
            title: tf("{0} new package(s) pulled in as dependencies", &[&new_count.to_string()]),
            detail: tf(
                "You did not ask for them; they arrive because something you asked for needs them. {0}{1}",
                &[
                    &names.join(", "),
                    if new_count > names.len() { "…" } else { "" },
                ],
            ),
        });
    }

    // — Frozen: invisible otherwise, though they explain why an expected update
    //   never arrives.
    if !state.ignored.is_empty() {
        let frozen: Vec<String> = state
            .ignored
            .iter()
            .map(|i| format!("{} {} → {}", i.name, i.from_version, i.to_version))
            .collect();
        r.push(Risk {
            level: Level::Info,
            title: tf("{0} update(s) available but frozen", &[&frozen.len().to_string()]),
            detail: tf(
                "{0}. Held back by IgnorePkg in /etc/pacman.conf: they will not be installed while that line is there.",
                &[&frozen.join(", ")],
            ),
        });
    }
}

fn analyze_removal(plan: &Plan, state: &State, r: &mut Vec<Risk>) {
    // — The cascade is the first thing to see: you check one package, several
    //   often leave.
    let cascade: Vec<&str> = plan
        .rows
        .iter()
        .filter(|l| l.kind == Kind::AutoRemoval)
        .map(|l| l.name.as_str())
        .collect();
    if !cascade.is_empty() {
        let preview: Vec<&str> = cascade.iter().take(8).copied().collect();
        r.push(Risk {
            level: Level::Serious,
            title: tf(
                "{0} extra package(s) will be removed as a cascade",
                &[&cascade.len().to_string()],
            ),
            detail: tf(
                "You did not check {0}{1}: they leave because nothing will need them once your selection is gone.",
                &[
                    &preview.join(", "),
                    if cascade.len() > preview.len() { "…" } else { "" },
                ],
            ),
        });
    }

    // — A package still needed by something that stays: that is a real problem.
    //   Only count requirers outside the plan, otherwise every cascade raises
    //   the alarm — a dependency is by definition needed by the package that
    //   pulled it in, and that one leaves with it.
    let in_plan: Vec<&str> = plan.rows.iter().map(|l| l.name.as_str()).collect();
    let mut still_needed: Vec<(String, usize)> = Vec::new();
    for l in &plan.rows {
        if let Some(p) = state.installed.iter().find(|p| p.name == l.name) {
            let outside = p
                .required_by
                .iter()
                .filter(|d| !in_plan.contains(&d.as_str()))
                .count();
            if outside > 0 {
                still_needed.push((p.name.clone(), outside));
            }
        }
    }
    if !still_needed.is_empty() {
        let list: Vec<String> = still_needed
            .iter()
            .map(|(n, c)| format!("{n} ({c})"))
            .collect();
        r.push(Risk {
            level: Level::Serious,
            title: tf(
                "{0} package(s) still needed by packages that stay: {1}",
                &[&still_needed.len().to_string(), &list.join(", ")],
            ),
            detail: t("Packages outside this removal depend on them. pacman will refuse the operation, or take along whatever depends on them. Check \"Required by\" in the detail panel before approving.").into(),
        });
    }

    // — Plugins loaded at run time are declared nowhere: that is the entire
    //   reason the protection list exists.
    r.push(Risk {
        level: Level::Caution,
        title: t("Dependencies loaded at run time are invisible").into(),
        detail: t("Qt plugins, Wayland backends, GStreamer modules: nothing declares them, so nothing protects them but keep.list. When in doubt, protect rather than remove.").into(),
    });

    // — -Rns takes the configuration with it.
    r.push(Risk {
        level: Level::Info,
        title: t("Configuration files will be deleted").into(),
        detail: t("The -Rns option also removes dependencies nothing needs any more, along with the package's configuration files. Your own files under ~/ are untouched.").into(),
    });
}

/// Lists a few names, then counts the rest. Three hundred packages dumped into
/// a warning makes it unreadable, and therefore useless.
fn list_some(names: &[String], max: usize) -> String {
    if names.len() <= max {
        return names.join(", ");
    }
    tf("{0}, and {1} others", &[&names[..max].join(", "), &(names.len() - max).to_string()])
}

/// Points of attention specific to a rollback.
///
/// These risks are of a different nature from an upgrade's: here the
/// transaction is perfectly known, but it runs against what the system expects,
/// and above all it undoes only part of what was done — the rest is nowhere to
/// be seen.
pub fn analyze_rollback(
    rollbacks: &[crate::history::Rollback],
    transaction: &crate::history::Transaction,
    state: &State,
) -> Vec<Risk> {
    use crate::history::Source;
    let mut r = Vec::new();

    let missing: Vec<&crate::history::Rollback> =
        rollbacks.iter().filter(|x| !x.is_possible()).collect();
    let recoverable = rollbacks.len() - missing.len();
    if !missing.is_empty() {
        let names: Vec<String> = missing
            .iter()
            .map(|x| format!("{} {}", x.name, x.target.clone().unwrap_or_default()))
            .collect();
        r.push(Risk {
            level: Level::Serious,
            title: tf("{0} package(s) not found: partial rollback", &[&missing.len().to_string()]),
            detail: tf(
                "These versions are no longer in any local cache and will stay as they are: {0}. \
                 They were pruned by paccache, which keeps only {1} version(s) per package.",
                &[&list_some(&names, 6), &state.cache_keep.to_string()],
            ),
        });
    }

    // A rollback that recovers only a fraction of a large transaction does not
    // bring back the earlier state: it manufactures a third one, that nobody has
    // ever tested. Say it once, up front, rather than leaving it to be inferred.
    if missing.len() > recoverable && rollbacks.len() > 20 {
        r.push(Risk {
            level: Level::Serious,
            title: t("This rollback rebuilds an untested state, not the earlier one").into(),
            detail: tf(
                "Fewer than half the packages ({0} of {1}) can go back down. The system would end \
                 up with a mix of both versions, never shipped nor tested that way. To really \
                 undo a transaction of this size, you need a filesystem snapshot.",
                &[&recoverable.to_string(), &rollbacks.len().to_string()],
            ),
        });
    }

    let restored: Vec<&str> = rollbacks
        .iter()
        .filter(|x| matches!(x.source, Source::File(_)) && x.current.is_some())
        .map(|x| x.name.as_str())
        .collect();
    if !restored.is_empty() {
        let names: Vec<String> = restored.iter().map(|s| s.to_string()).collect();
        r.push(Risk {
            level: Level::Caution,
            title: t("The next full upgrade will undo this rollback").into(),
            detail: tf(
                "pacman will offer the recent version again on the next -Syu. To freeze it for \
                 good, add IgnorePkg = {0} to /etc/pacman.conf.",
                &[&list_some(&names, 8)],
            ),
        });
    }

    // Only the packages the rollback actually touches: a critical component that
    // is out of cache will not move, announcing it would be a false alarm.
    let critical: Vec<&str> = rollbacks
        .iter()
        .filter(|x| x.is_possible())
        .map(|x| x.name.as_str())
        .filter(|n| CRITICAL.contains(n) || KERNELS.contains(n))
        .collect();
    if !critical.is_empty() {
        r.push(Risk {
            level: Level::Serious,
            title: tf("Critical component downgraded: {0}", &[&critical.join(", ")]),
            detail: t("The whole system is tied to these packages. An earlier version can make \
                       pacman itself unusable — keep rescue media within reach.")
                .into(),
        });
    }

    // An isolated downgrade leaves the rest of the system on the recent
    // versions: that is the definition of a partial upgrade, with the same
    // consequences.
    if !restored.is_empty() {
        r.push(Risk {
            level: Level::Caution,
            title: t("Partial state: the rest of the system does not go back").into(),
            detail: t("Only the packages from this transaction go back. If one of them is linked \
                       against a library upgraded since, pacman will refuse — or the program will \
                       start against a version it does not know.")
                .into(),
        });
    }

    let removals = rollbacks
        .iter()
        .filter(|x| matches!(x.source, Source::Remove))
        .count();
    if removals > 0 {
        r.push(Risk {
            level: Level::Caution,
            title: tf("{0} package(s) will be uninstalled", &[&removals.to_string()]),
            detail: t("These are the ones the transaction had installed. Their configuration \
                       files under /etc are kept, but everything created since (data, enabled \
                       systemd units) stays on disk.")
                .into(),
        });
    }

    if !transaction.warnings.is_empty() {
        r.push(Risk {
            level: Level::Info,
            title: tf(
                "{0} warning(s) during the original transaction",
                &[&transaction.warnings.len().to_string()],
            ),
            detail: transaction.warnings.join(" · "),
        });
    }

    // The most important point, and the one no package manager can fix: undoing
    // a package does not undo what that package did on the next boot.
    r.push(Risk {
        level: Level::Info,
        title: t("Rolling back packages is not rolling back the system").into(),
        detail: t("The files the packages laid down go back. What a scriptlet or a hook has \
                   written since — a database migration, a rewritten configuration, a regenerated \
                   cache — stays as it is. For a real state rollback you need a snapshot \
                   (snapper/Btrfs).")
            .into(),
    });

    r
}
