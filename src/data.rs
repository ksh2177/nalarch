//! Read layer. The principle: libalpm for everything that is metadata (fast,
//! structured, no parsing), and delegation to checkupdates/paru for the
//! authoritative list of pending updates.
//!
//! `pacman -Sy` is deliberately NOT used to detect updates: it would leave the
//! database out of step with the system (a partial upgrade). `checkupdates`
//! syncs into a temporary database, which carries no such risk.

use alpm::{Alpm, PackageReason, SigLevel};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Repo,
    Aur,
}

#[derive(Debug, Clone)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    /// Only set for packages that have an update.
    pub target_version: Option<String>,
    pub repo: String,
    pub description: String,
    pub installed_size: i64,
    /// Download size, known only if the sync database is up to date.
    pub download_size: Option<i64>,
    /// Installed size of the target version, same condition.
    pub target_size: Option<i64>,
    pub explicit: bool,
    pub required_by: Vec<String>,
    pub optional_for: Vec<String>,
    pub depends_on: Vec<String>,
    pub origin: Origin,
}

impl Pkg {
    /// A package is an orphan if it was pulled in as a dependency and nothing
    /// needs it any more. Careful: that status is NOT enough to allow removal
    /// (see qt6-wayland, needed by quickshell with no alpm link).
    pub fn is_orphan(&self) -> bool {
        !self.explicit && self.required_by.is_empty() && self.optional_for.is_empty()
    }

    /// A package installed on purpose that nothing hard-depends on: the top of
    /// the tree, and therefore what the user can uninstall.
    ///
    /// Equivalent to `pacman -Qett`, not `pacman -Qet`: a package you chose
    /// stays your application even if it appears as an *optional* dependency of
    /// another. That optional link is shown in the detail panel rather than
    /// hiding the package.
    pub fn is_root(&self) -> bool {
        self.explicit && self.required_by.is_empty()
    }
}

/// An update that is available but held back by `IgnorePkg`.
#[derive(Debug, Clone)]
pub struct Ignore {
    pub name: String,
    pub from_version: String,
    pub to_version: String,
}

/// Complete snapshot of the system state.
pub struct State {
    pub installed: Vec<Pkg>,
    pub updates: Vec<Pkg>,
    /// Updates held back by IgnorePkg: paru mentions them in a warning, so they
    /// may as well be shown outright rather than left invisible.
    pub ignored: Vec<Ignore>,
    pub cache_bytes: u64,
    pub cache_files: usize,
    /// Reclaimable by keeping only `cache_keep` versions of each package.
    pub cache_prunable: u64,
    /// Reclaimable by purging packages that are no longer installed at all.
    pub cache_uninstalled: u64,
    /// Retention policy read from /etc/conf.d/pacman-contrib.
    pub cache_keep: u32,
    /// name -> (available version, download size, installed size). Kept so that
    /// packages not yet installed can still be sized.
    pub sync: HashMap<String, (String, i64, i64)>,
    /// Dependency strings of the SYNC (target) version of each available update.
    /// Feeds the selection closure: upgrading ncmpcpp alone while --ignoring
    /// boost-libs is unresolvable when the rebuild wants the new sonames.
    pub update_deps: HashMap<String, Vec<String>>,
    /// provide name -> update package name, for the same closure (sonames:
    /// "libboost_locale.so" is provided by boost-libs, not a package name).
    pub update_provides: HashMap<String, String>,
}

/// A package as pacman would resolve it in the transaction.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub version: String,
    pub repo: String,
}

/// Temporary database maintained by `checkupdates`.
///
/// It is a copy of the repository databases, synced without touching the
/// system's own. Querying it allows reasoning about fresh data with no
/// privileges and no risk of a partial-upgrade state.
fn checkupdates_db() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let path = match std::env::var("CHECKUPDATES_DB") {
        Ok(v) => std::path::PathBuf::from(v),
        Err(_) => {
            let uid = std::fs::metadata("/proc/self").ok()?.uid();
            let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(tmp).join(format!("checkup-db-{uid}"))
        }
    };
    path.is_dir().then_some(path)
}

/// The transaction pacman would actually carry out for this selection.
///
/// `checkupdates` only lists already-installed packages that have a newer
/// version. It says nothing about the **new dependencies** an upgrade pulls in
/// along the way — exactly the kind of surprise worth announcing before
/// launching anything.
///
/// `pacman -Sup --print-format` gives that complete list, needs no privilege
/// and writes nothing. It is pointed at the `checkupdates` database so that it
/// reasons about the same versions as everything else.
pub fn resolved_transaction(excluded: &[String]) -> Vec<Resolved> {
    let mut args: Vec<String> = vec![
        "-Sup".into(),
        "--print-format".into(),
        "%n|%v|%r".into(),
    ];
    if let Some(db) = checkupdates_db() {
        args.push("--dbpath".into());
        args.push(db.to_string_lossy().into_owned());
    }
    for e in excluded {
        args.push("--ignore".into());
        args.push(e.clone());
    }

    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = harvest(spawn("pacman", &refs));

    raw.lines()
        .filter_map(|l| {
            let mut it = l.trim().split('|');
            Some(Resolved {
                name: it.next()?.to_string(),
                version: it.next()?.to_string(),
                repo: it.next().unwrap_or("").to_string(),
            })
        })
        .filter(|r| !r.name.is_empty())
        .collect()
}

/// The transaction pacman would carry out to install these packages.
///
/// Same mechanism as `resolved_transaction`, with `-Sp` instead of `-Sup`: the
/// starting point is a list of names rather than the whole system. It resolves
/// the dependencies too, which is the part worth seeing before installing —
/// asking for one package routinely brings a dozen.
///
/// Names pacman does not know (AUR) simply produce no line: paru handles those,
/// and nothing here has to guess.
pub fn install_transaction(names: &[String]) -> Vec<Resolved> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut args: Vec<String> = vec!["-Sp".into(), "--print-format".into(), "%n|%v|%r".into()];
    if let Some(db) = checkupdates_db() {
        args.push("--dbpath".into());
        args.push(db.to_string_lossy().into_owned());
    }
    args.extend(names.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    harvest(spawn("pacman", &refs))
        .lines()
        .filter_map(|l| {
            let mut it = l.trim().split('|');
            Some(Resolved {
                name: it.next()?.to_string(),
                version: it.next()?.to_string(),
                repo: it.next().unwrap_or("").to_string(),
            })
        })
        .filter(|r| !r.name.is_empty())
        .collect()
}

/// Packages `-Rns` really removes, cascade included.
///
/// Removing one package often takes others along: `-s` removes dependencies
/// nothing needs any more. Asking to remove `asciiquarium` also removes
/// `perl-term-animation` and `perl-curses`. Showing only what was checked would
/// hide most of the operation.
///
/// `-p` is a dry run: the list is computed then printed, with no privilege and
/// nothing touched.
pub fn removal_cascade(noms: &[String]) -> Vec<Resolved> {
    if noms.is_empty() {
        return Vec::new();
    }
    // `-n` (--nosave) and `-p` (--print) are incompatible, so it is dropped for
    // the dry run. No consequence: `-n` concerns configuration files, not the
    // list of removed packages, which is identical either way.
    let mut args: Vec<String> = vec!["-Rsp".into(), "--print-format".into(), "%n|%v".into()];
    args.extend(noms.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    harvest(spawn("pacman", &refs))
        .lines()
        .filter_map(|l| {
            let mut it = l.trim().split('|');
            Some(Resolved {
                name: it.next()?.to_string(),
                version: it.next()?.to_string(),
                repo: String::new(),
            })
        })
        .filter(|r| !r.name.is_empty())
        .collect()
}

/// Packages frozen by `IgnorePkg` in pacman.conf. The directive accepts several
/// names per line and may be repeated.
fn ignored_packages() -> HashSet<String> {
    let Ok(contents) = std::fs::read_to_string("/etc/pacman.conf") else {
        return HashSet::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("IgnorePkg"))
        .filter_map(|r| r.trim_start().strip_prefix('='))
        .flat_map(|v| v.split_whitespace().map(String::from))
        .collect()
}

/// Reads the repository names declared in pacman.conf (any section but [options]).
pub fn configured_repos() -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string("/etc/pacman.conf") else {
        return Vec::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix('[').and_then(|l| l.strip_suffix(']')))
        .filter(|s| *s != "options")
        .map(String::from)
        .collect()
}

/// What paru does with build dependencies once the build is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveMake {
    /// They are removed again.
    Yes,
    /// They stay installed.
    No,
    /// paru asks.
    Ask,
}

/// Reads `RemoveMake` from paru's configuration.
///
/// Worth the read rather than a guess: a build dependency can be `go`, six
/// hundred megabytes pulled in to compile one small program. Whether it goes
/// away afterwards is the difference between a passing inconvenience and a
/// permanent one, and it is a per-user setting — saying either without looking
/// would be wrong on half the machines.
///
/// paru reads one file, not a merge: the user's if it exists, `/etc/paru.conf`
/// otherwise. The default is off, which is why an unset option means `No`.
pub fn remove_make() -> RemoveMake {
    let user = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .map(|b| b.join("paru").join("paru.conf"));

    let contents = match user {
        Ok(p) if p.exists() => std::fs::read_to_string(p),
        _ => std::fs::read_to_string("/etc/paru.conf"),
    };
    let Ok(contents) = contents else {
        return RemoveMake::No;
    };

    for line in contents.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("RemoveMake") else {
            continue;
        };
        // Bare `RemoveMake` means yes; `RemoveMake = ask` is its own answer.
        return match rest.trim_start().strip_prefix('=').map(str::trim) {
            None => RemoveMake::Yes,
            Some("ask") => RemoveMake::Ask,
            Some("no") => RemoveMake::No,
            Some(_) => RemoveMake::Yes,
        };
    }
    RemoveMake::No
}

/// How many versions paccache is meant to keep, read from the system
/// configuration rather than hard-coded.
///
/// `paccache.timer` prunes the cache periodically according to `PACCACHE_ARGS`
/// in `/etc/conf.d/pacman-contrib`. Offering a different value would make the
/// cache oscillate between two policies on every run.
pub fn paccache_keep() -> u32 {
    const DEFAULT: u32 = 3; // paccache's own default
    let Ok(contents) = std::fs::read_to_string("/etc/conf.d/pacman-contrib") else {
        return DEFAULT;
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix("PACCACHE_ARGS="))
        .and_then(|v| {
            let v = v.trim_matches(['\'', '"']);
            v.split_whitespace()
                .find_map(|a| a.strip_prefix("-k"))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(DEFAULT)
}

/// Starts a command without waiting for it. None if the binary is missing.
fn spawn(cmd: &str, args: &[&str]) -> Option<Child> {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Waits for a command started by `spawn` and returns its standard output.
///
/// The exit code is ignored on purpose: `checkupdates` exits 2 when there is no
/// update, which is not an error. Empty output simply yields zero parsed lines.
fn harvest(child_process: Option<Child>) -> String {
    child_process
        .and_then(|c| c.wait_with_output().ok())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

/// Parses the "name old -> new" lines of checkupdates and paru -Qua.
fn parse_updates(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let _ancienne = it.next()?;
            let fleche = it.next()?;
            if fleche != "->" {
                return None;
            }
            let to_version = it.next()?;
            Some((name.to_string(), to_version.to_string()))
        })
        .collect()
}

pub fn load() -> Result<State> {
    // The four subprocesses are independent and two of them are network-bound.
    // Start them all first, do the alpm work while they run, and harvest at the
    // end.
    let keep = paccache_keep();
    let keep_arg = format!("-dk{keep}");
    let p_depot = spawn("checkupdates", &[]);
    let p_aur = spawn("paru", &["-Qua"]);
    let p_anciennes = spawn("paccache", &[&keep_arg]);
    let p_desinstalles = spawn("paccache", &["-duk0"]);

    let handle = Alpm::new("/", "/var/lib/pacman").context("opening the alpm database")?;

    // The sync databases provide the originating repository and the download
    // size. Their possible staleness is harmless: target versions come from
    // checkupdates, not from here.
    for repo in configured_repos() {
        let _ = handle.register_syncdb(repo.as_str(), SigLevel::USE_DEFAULT);
    }

    // Locating each package's repository through the sync databases. This is the
    // bulk of the alpm work, and it happens while the subprocesses run.
    let mut repo_of: HashMap<String, String> = HashMap::new();
    // name -> (available version, download size, installed size)
    let mut sync_of: HashMap<String, (String, i64, i64)> = HashMap::new();
    for db in handle.syncdbs() {
        let db_name = db.name().to_string();
        for p in db.pkgs() {
            let n = p.name().to_string();
            repo_of.entry(n.clone()).or_insert_with(|| db_name.clone());
            sync_of.entry(n).or_insert_with(|| {
                (p.version().to_string(), p.download_size(), p.isize())
            });
        }
    }

    // Harvest: by now the commands have had the whole alpm scan to finish.
    let maj_depot = parse_updates(&harvest(p_depot));
    let maj_aur = parse_updates(&harvest(p_aur));

    let local = handle.localdb();
    let mut installed = Vec::new();
    let mut updates = Vec::new();

    for p in local.pkgs() {
        let name = p.name().to_string();
        let version = p.version().to_string();

        let (target_version, origin) = match (maj_depot.get(&name), maj_aur.get(&name)) {
            (Some(v), _) => (Some(v.clone()), Origin::Repo),
            (None, Some(v)) => (Some(v.clone()), Origin::Aur),
            (None, None) => (None, Origin::Repo),
        };

        let repo = match origin {
            Origin::Aur => "aur".to_string(),
            Origin::Repo => repo_of
                .get(&name)
                .cloned()
                .unwrap_or_else(|| "aur".to_string()), // in no repository = AUR/local
        };

        // Sizes are only trustworthy if the sync database already carries the
        // version about to be installed; otherwise showing nothing beats showing
        // something wrong.
        let cible_sync = target_version
            .as_ref()
            .and_then(|c| sync_of.get(&name).filter(|(v, _, _)| v == c));
        let download_size = cible_sync.map(|(_, dl, _)| *dl);
        let target_size = cible_sync.map(|(_, _, isize)| *isize);

        let pkg = Pkg {
            name,
            version,
            target_version,
            repo,
            description: p.desc().unwrap_or("").to_string(),
            installed_size: p.isize(),
            download_size,
            target_size,
            explicit: p.reason() == PackageReason::Explicit,
            required_by: p.required_by().iter().map(String::from).collect(),
            optional_for: p.optional_for().iter().map(String::from).collect(),
            depends_on: p.depends().iter().map(|d| d.name().to_string()).collect(),
            origin,
        };

        if pkg.target_version.is_some() {
            updates.push(pkg.clone());
        }
        installed.push(pkg);
    }

    installed.sort_by(|a, b| a.name.cmp(&b.name));
    // AUR updates last: they mean a build, and therefore a cost of a different
    // nature from a plain download.
    updates.sort_by(|a, b| {
        (a.origin == Origin::Aur)
            .cmp(&(b.origin == Origin::Aur))
            .then(a.name.cmp(&b.name))
    });

    // Frozen packages: checkupdates excludes them, so without this they would be
    // entirely invisible in nalarch even though paru mentions them.
    let frozen = ignored_packages();
    let mut ignored: Vec<Ignore> = installed
        .iter()
        .filter(|p| frozen.contains(&p.name))
        .filter_map(|p| {
            let (dispo, _, _) = sync_of.get(&p.name)?;
            (dispo != &p.version).then(|| Ignore {
                name: p.name.clone(),
                from_version: p.version.clone(),
                to_version: dispo.clone(),
            })
        })
        .collect();
    ignored.sort_by(|a, b| a.name.cmp(&b.name));

    let (cache_bytes, cache_files) = mesurer_cache();

    // Dependencies and provides of the TARGET version of each update, read from
    // the sync databases (a handful of lookups). See State::update_deps.
    let mut update_deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut update_provides: HashMap<String, String> = HashMap::new();
    for u in &updates {
        for db in handle.syncdbs() {
            if let Ok(p) = db.pkg(u.name.as_str()) {
                update_deps.insert(
                    u.name.clone(),
                    p.depends().iter().map(|d| d.to_string()).collect(),
                );
                for prov in p.provides() {
                    update_provides.insert(prov.name().to_string(), u.name.clone());
                }
                break;
            }
        }
    }

    Ok(State {
        installed,
        updates,
        ignored,
        cache_bytes,
        cache_files,
        cache_prunable: paccache_dry_run(&harvest(p_anciennes)),
        cache_uninstalled: paccache_dry_run(&harvest(p_desinstalles)),
        cache_keep: keep,
        sync: sync_of,
        update_deps,
        update_provides,
    })
}

fn mesurer_cache() -> (u64, usize) {
    let mut total = 0;
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir("/var/cache/pacman/pkg") {
        for e in entries.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total += m.len();
                    n += 1;
                }
            }
        }
    }
    (total, n)
}

/// `paccache -d` is a dry run: it announces what would be deleted without
/// touching anything. The reclaimable volume is extracted from it.
///
/// The message reads:
///   ==> finished dry run: 6 candidates (disk space saved: 2.59 MiB)
/// The closing parenthesis sticks to the unit, hence stripping non-alphabetic
/// characters before comparing. When there is nothing to prune, paccache writes
/// "no candidate packages found for pruning" and this returns 0.
fn paccache_dry_run(out: &str) -> u64 {
    const MARKER: &str = "disk space saved:";
    for line in out.lines() {
        let Some(idx) = line.find(MARKER) else {
            continue;
        };
        let mut it = line[idx + MARKER.len()..].split_whitespace();
        let (Some(val), Some(unit)) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(v) = val.replace(',', ".").parse::<f64>() else {
            continue;
        };
        let unit: String = unit.chars().filter(|c| c.is_ascii_alphabetic()).collect();
        let mult: f64 = match unit.to_ascii_uppercase().as_str() {
            "KIB" | "K" => 1024.0,
            "MIB" | "M" => 1024_f64.powi(2),
            "GIB" | "G" => 1024_f64.powi(3),
            "TIB" | "T" => 1024_f64.powi(4),
            _ => 1.0,
        };
        return (v * mult) as u64;
    }
    0
}

/// Protection list: packages that must not be removed even when they look like
/// orphans. Necessary, because libalpm cannot see dependencies loaded
/// dynamically at run time (Qt plugins, Wayland backends…).
pub fn keep_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    base.join("nalarch").join("keep.list")
}

/// Seed contents of keep.list, written on first run.
///
/// The comments are translated: this file is meant to be read and edited by
/// hand, so it speaks the language of the interface. Package names are not.
fn keep_seed() -> String {
    use crate::i18n::t;
    format!(
        "\
# {0}
# {1}
#
# {2}

# {3}
qt6-wayland
qt6-avif-image-plugin
",
        t("nalarch — packages protected from removal."),
        t("One name per line. They stay visible in the Orphans tab but cannot be checked for removal."),
        t("libalpm only knows about declared dependencies. A package loaded dynamically (a Qt plugin, a Wayland backend, a GStreamer plugin) therefore looks like an orphan while being vital. Hence this list."),
        t("Needed by Qt applications under Wayland, with no alpm dependency link:"),
    )
}

pub fn load_keep() -> HashSet<String> {
    let path = keep_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, keep_seed());
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

pub fn toggle_keep(name: &str, protege: bool) -> Result<()> {
    let path = keep_path();
    let mut contents = std::fs::read_to_string(&path).unwrap_or_default();
    if protege {
        if !contents.ends_with('\n') && !contents.is_empty() {
            contents.push('\n');
        }
        contents.push_str(name);
        contents.push('\n');
    } else {
        contents = contents
            .lines()
            .filter(|l| l.trim() != name)
            .collect::<Vec<_>>()
            .join("\n");
        contents.push('\n');
    }
    std::fs::write(&path, contents).context("writing the protection list")
}
