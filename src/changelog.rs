//! What an update changes, before downloading it.
//!
//! A version transition says nothing about what it brings. Yet "what does this
//! change?" has no simple answer on Arch: packages almost never ship a
//! changelog (`pacman -Qc` is empty most of the time). The information exists,
//! but elsewhere, in two complementary places:
//!
//! - Arch's **packaging log**, which says why the package moved — a new
//!   upstream release, or a plain rebuild against a library. That is often the
//!   most useful answer: a rebuild brings no feature and explains an update
//!   that looked gratuitous;
//! - the **upstream release notes**, which say what the software actually
//!   changes. Fetched when the project is hosted on GitHub, which covers most
//!   of Arch.
//!
//! Fetching goes through `curl`, already present and already used elsewhere,
//! rather than an embedded HTTP stack: that avoids carrying a TLS
//! implementation for two optional requests.

use serde_json::Value;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};

pub struct Commit {
    pub date: String,
    pub title: String,
}

#[derive(Default)]
pub struct Content {
    pub url: Option<String>,
    pub packaging: Vec<Commit>,
    /// Upstream release notes, already split into lines.
    pub upstream: Vec<String>,
    pub upstream_tag: Option<String>,
    /// What could not be obtained, said out loud rather than left blank.
    pub gaps: Vec<String>,
}

pub enum State {
    Loading,
    Ready(Box<Content>),
}

pub struct Changelog {
    pub package: String,
    pub from_version: String,
    pub to_version: String,
    pub state: State,
    pending: Option<Receiver<Content>>,
}

impl Changelog {
    pub fn spawn(package: &str, from_version: &str, to_version: &str, aur: bool) -> Self {
        let (tx, rx) = channel();
        let (p, n) = (package.to_string(), to_version.to_string());
        // Network requests live on their own thread: the interface has to stay
        // responsive, and a slow DNS lookup must not freeze everything.
        std::thread::spawn(move || {
            let _ = tx.send(fetch(&p, &n, aur));
        });
        Self {
            package: package.to_string(),
            from_version: from_version.to_string(),
            to_version: to_version.to_string(),
            state: State::Loading,
            pending: Some(rx),
        }
    }

    /// Collects the result if it has arrived. True when the screen changed.
    pub fn pump(&mut self) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        match rx.try_recv() {
            Ok(c) => {
                self.state = State::Ready(Box::new(c));
                self.pending = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state = State::Ready(Box::default());
                self.pending = None;
                true
            }
            Err(_) => false,
        }
    }
}

fn curl(url: &str) -> Option<String> {
    let output = Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "12",
            "-H",
            "User-Agent: nalarch",
            url,
        ])
        .output()
        .ok()?;
    let body = String::from_utf8(output.stdout).ok()?;
    (!body.trim().is_empty()).then_some(body)
}

/// Upstream project URL, as declared by the package.
fn upstream_url(package: &str) -> Option<String> {
    let output = Command::new("pacman")
        .args(["-Si", package])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("URL"))
        .and_then(|v| v.split_once(':'))
        .map(|(_, u)| u.trim().to_string())
        .filter(|u| !u.is_empty())
}

fn fetch(package: &str, to_version: &str, aur: bool) -> Content {
    let mut c = Content {
        url: upstream_url(package),
        ..Default::default()
    };

    if aur {
        // The AUR has no packaging repository: the PKGBUILD is the only source,
        // and reading it is exactly what paru offers to do.
        c.gaps.push(crate::i18n::t(
            "AUR package: no packaging log. paru will show the PKGBUILD before building.",
        ).to_string());
    } else {
        c.packaging = arch_packaging(package, &mut c.gaps);
    }

    if let Some(url) = c.url.clone() {
        // GitHub covers a bit under half of what is installed here; the GitLab
        // instances — GNOME, freedesktop, KDE's invent — most of the rest that
        // is hosted anywhere at all. The two APIs answer the same question, so
        // supporting both costs one more request shape.
        let fetched = match forge(&url) {
            Some(Forge::GitHub { owner, repo }) => {
                Some(github_release_notes(&owner, &repo, to_version))
            }
            Some(Forge::GitLab { host, path }) => {
                Some(gitlab_release_notes(&host, &path, to_version))
            }
            None => None,
        };
        match fetched {
            Some((notes, tag)) => {
                c.upstream = notes;
                c.upstream_tag = tag;
                if c.upstream.is_empty() {
                    c.gaps
                        .push(crate::i18n::t("No release notes published for this tag.").to_string());
                }
            }
            // Plenty of projects publish through a website rather than a forge.
            // Saying where beats claiming nothing exists.
            None => c.gaps.push(crate::i18n::tf(
                "Not hosted on GitHub or GitLab. Release notes, if any, are at {0}",
                &[&url],
            )),
        }
    }
    c
}

/// The upstream part of a version: no epoch, no package release.
///
/// `1:6.29.0-2` and `6.29.0-1` are the same software; only the packaging
/// differs. Telling them apart is the whole of the verdict below.
pub fn upstream_version(v: &str) -> String {
    let v = v.split_once(':').map_or(v, |(_, rest)| rest);
    match v.rsplit_once('-') {
        Some((base, _release)) => base.to_string(),
        None => v.to_string(),
    }
}

/// What an update actually brings, in one line.
///
/// The most common answer, and the one hardest to read off a list of commits:
/// nothing new. A package release bumped on its own means a rebuild, a fix to
/// the packaging, or a dependency change — never a feature. Saying so up front
/// beats leaving it to be inferred from `6.29.0-1 → 6.29.0-2`.
pub fn verdict(from: &str, to: &str) -> String {
    let (a, b) = (upstream_version(from), upstream_version(to));
    if a == b {
        crate::i18n::tf(
            "Same upstream version ({0}) — a packaging change: a rebuild, a fix, or a dependency bump.",
            &[&b],
        )
    } else {
        crate::i18n::tf("New upstream version: {0} → {1}", &[&a, &b])
    }
}

/// Where a project's releases can be asked for.
enum Forge {
    GitHub { owner: String, repo: String },
    GitLab { host: String, path: String },
}

/// Recognises the two forges whose APIs answer "what is in this release".
///
/// A GitLab instance cannot be told from any other host by its name alone, so
/// only the ones that actually appear as upstream URLs are matched. Guessing
/// would mean a request to an unrelated server for every package.
fn forge(url: &str) -> Option<Forge> {
    if let Some((owner, repo)) = github(url) {
        return Some(Forge::GitHub { owner, repo });
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let (host, path) = rest.split_once('/')?;
    let known = host.starts_with("gitlab.") || host == "invent.kde.org";
    if !known {
        return None;
    }
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .split("/-/")
        .next()?;
    (!path.is_empty()).then(|| Forge::GitLab {
        host: host.to_string(),
        path: path.to_string(),
    })
}

/// Upstream release notes from a GitLab instance.
///
/// Same shape as the GitHub one, including trying the tag with and without its
/// `v`: the project decides, not the packager.
fn gitlab_release_notes(host: &str, path: &str, version: &str) -> (Vec<String>, Option<String>) {
    let base = version.split('-').next().unwrap_or(version);
    let base = base.split_once(':').map_or(base, |(_, v)| v);
    let project = path.replace('/', "%2F");

    for tag in [base.to_string(), format!("v{base}")] {
        let url = format!("https://{host}/api/v4/projects/{project}/releases/{tag}");
        let Some(body) = curl(&url) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        if let Some(notes) = v.get("description").and_then(Value::as_str) {
            let rows: Vec<String> = notes
                .lines()
                .map(|l| l.trim_end().to_string())
                .take(60)
                .collect();
            if !rows.is_empty() {
                return (rows, Some(tag));
            }
        }
    }
    (Vec::new(), None)
}

/// Arch packaging repository log: says *why* the package changed.
fn arch_packaging(package: &str, gaps: &mut Vec<String>) -> Vec<Commit> {
    let url = format!(
        "https://gitlab.archlinux.org/api/v4/projects/archlinux%2Fpackaging%2Fpackages%2F{package}/repository/commits?per_page=15"
    );
    let Some(body) = curl(&url) else {
        gaps.push(crate::i18n::t("Packaging log unreachable (network?).").to_string());
        return Vec::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&body) else {
        gaps.push(crate::i18n::t("This package has no Arch packaging repository.").to_string());
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| {
            Some(Commit {
                date: v.get("created_at")?.as_str()?.get(..10)?.to_string(),
                title: v.get("title")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// Extracts owner and repository from a GitHub URL.
fn github(url: &str) -> Option<(String, String)> {
    let rest = url
        .split_once("github.com/")
        .map(|(_, r)| r)?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut it = rest.split('/');
    let owner = it.next()?.to_string();
    let repo = it.next()?.to_string();
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

/// Upstream release notes for the target version.
///
/// Arch versions carry a package revision (`2.67.1-1`) that upstream tags do
/// not have, and projects may or may not prefix them with `v`. Both forms are
/// tried rather than imposing one.
fn github_release_notes(owner: &str, repo: &str, version: &str) -> (Vec<String>, Option<String>) {
    let base = version.split('-').next().unwrap_or(version);
    // An epoch (`1:1.2.3`) is not part of the upstream tag.
    let base = base.split_once(':').map_or(base, |(_, v)| v);

    for tag in [base.to_string(), format!("v{base}")] {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}");
        let Some(body) = curl(&url) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        if let Some(notes) = v.get("body").and_then(Value::as_str) {
            let rows: Vec<String> = notes
                .lines()
                .map(|l| l.trim_end().to_string())
                .take(60)
                .collect();
            if !rows.is_empty() {
                return (rows, Some(tag));
            }
        }
    }
    (Vec::new(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer people actually want, and the one a list of commits hides.
    #[test]
    fn a_release_bump_alone_is_a_packaging_change() {
        // What prompted this: baloo 6.29.0-1 → 6.29.0-2, "fix lmdb linking".
        let v = verdict("6.29.0-1", "6.29.0-2");
        assert!(v.contains("packaging"), "{v}");
        assert!(v.contains("6.29.0"), "{v}");

        let v = verdict("6.28.0-1", "6.29.0-1");
        assert!(v.contains("6.28.0") && v.contains("6.29.0"), "{v}");
        assert!(!v.contains("packaging"), "{v}");
    }

    #[test]
    fn an_epoch_is_not_part_of_the_upstream_version() {
        assert_eq!(upstream_version("1:26.1.3-2"), "26.1.3");
        assert_eq!(upstream_version("2.67.1-1"), "2.67.1");
        // A version with no release at all still reads.
        assert_eq!(upstream_version("20260810"), "20260810");
    }

    #[test]
    fn a_gitlab_instance_is_recognised_by_its_host() {
        let path = |u: &str| match forge(u) {
            Some(Forge::GitLab { host, path }) => Some(format!("{host}:{path}")),
            _ => None,
        };
        assert_eq!(
            path("https://gitlab.gnome.org/GNOME/gtk"),
            Some("gitlab.gnome.org:GNOME/gtk".into())
        );
        assert_eq!(
            path("https://invent.kde.org/frameworks/baloo"),
            Some("invent.kde.org:frameworks/baloo".into())
        );
        // A browsing URL carries a suffix the API does not want.
        assert_eq!(
            path("https://gitlab.freedesktop.org/mesa/mesa/-/tree/main"),
            Some("gitlab.freedesktop.org:mesa/mesa".into())
        );
        // A project page is not a forge, and guessing would mean a request to an
        // unrelated server for every package.
        assert!(path("https://develop.kde.org/products/frameworks/").is_none());
        assert!(path("https://www.gnu.org/software/bash/").is_none());
    }

    #[test]
    fn a_github_url_is_recognised() {
        assert_eq!(
            github("https://github.com/fastfetch-cli/fastfetch"),
            Some(("fastfetch-cli".into(), "fastfetch".into()))
        );
        assert_eq!(
            github("https://github.com/sharkdp/bat.git"),
            Some(("sharkdp".into(), "bat".into()))
        );
        assert_eq!(github("https://gitlab.com/volian/nala"), None);
    }
}
