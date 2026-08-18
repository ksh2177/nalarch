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
        if let Some((owner, repo)) = github(&url) {
            let (notes, tag) = github_release_notes(&owner, &repo, to_version);
            c.upstream = notes;
            c.upstream_tag = tag;
            if c.upstream.is_empty() {
                c.gaps
                    .push(crate::i18n::t("No release notes published for this tag.").to_string());
            }
        } else {
            c.gaps.push(crate::i18n::tf(
                "Upstream notes cannot be fetched automatically: {0}",
                &[&url],
            ));
        }
    }
    c
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
