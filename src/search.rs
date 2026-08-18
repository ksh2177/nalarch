//! Finding a package, in the repositories and in the AUR.
//!
//! Two sources, queried differently for good reason. The repositories are
//! already on disk: libalpm searches them itself, with no subprocess and no
//! output to parse. The AUR is a web service, so it goes through its RPC
//! endpoint — the same one paru uses — rather than through `paru -Ss`, whose
//! output would have to be parsed back into the structure the API already
//! returns.
//!
//! Both run on one background thread. A repository search is instant, an AUR
//! query is not, and freezing the interface on a slow DNS lookup would be a
//! poor way to answer a question the user can still change their mind about.

use serde_json::Value;
use std::sync::mpsc::{channel, Receiver};

/// One result, whatever it came from.
#[derive(Debug, Clone)]
pub struct Hit {
    pub name: String,
    pub version: String,
    /// A repository name, or `aur`.
    pub repo: String,
    pub description: String,
    /// Version currently installed, when it is.
    pub installed: Option<String>,
    /// AUR only: how many people voted for it, and how used it is.
    pub votes: Option<u32>,
    pub popularity: Option<f64>,
    /// AUR only: flagged out of date by a user, which is a warning worth having.
    pub out_of_date: bool,
    /// AUR only: absent means the package is orphaned — nobody maintains it.
    pub maintainer: Option<String>,
    pub url: Option<String>,
}

impl Hit {
    pub fn is_aur(&self) -> bool {
        self.repo == "aur"
    }
}

pub enum State {
    /// Nothing asked yet.
    Idle,
    Running,
    Done(Vec<Hit>),
    Failed(String),
}

pub struct Search {
    /// What the user typed, kept between searches so it can be refined.
    pub query: String,
    pub state: State,
    pending: Option<Receiver<Result<Vec<Hit>, String>>>,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            query: String::new(),
            state: State::Idle,
            pending: None,
        }
    }
}

impl Search {
    /// Starts a search. Anything shorter than two characters is refused: the
    /// AUR would answer with thousands of results, and the wait would buy
    /// nothing useful.
    pub fn start(&mut self, query: &str, installed: Vec<(String, String)>) {
        let q = query.trim().to_string();
        if q.chars().count() < 2 {
            self.state = State::Failed(
                crate::i18n::t("Type at least two characters to search.").to_string(),
            );
            return;
        }
        self.query = q.clone();
        self.state = State::Running;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(run(&q, &installed));
        });
        self.pending = Some(rx);
    }

    /// Collects the result if it has arrived. True when the screen changed.
    pub fn pump(&mut self) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(hits)) => {
                self.state = State::Done(hits);
                self.pending = None;
                true
            }
            Ok(Err(e)) => {
                self.state = State::Failed(e);
                self.pending = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state = State::Failed(crate::i18n::t("Search interrupted.").to_string());
                self.pending = None;
                true
            }
            Err(_) => false,
        }
    }

    pub fn hits(&self) -> &[Hit] {
        match &self.state {
            State::Done(h) => h,
            _ => &[],
        }
    }
}

fn run(query: &str, installed: &[(String, String)]) -> Result<Vec<Hit>, String> {
    let mut hits = repositories(query);
    // An AUR outage must not throw away results that are already in hand.
    match aur(query) {
        Ok(mut a) => hits.append(&mut a),
        Err(e) if hits.is_empty() => return Err(e),
        Err(_) => {}
    }

    for h in &mut hits {
        h.installed = installed
            .iter()
            .find(|(n, _)| n == &h.name)
            .map(|(_, v)| v.clone());
    }

    hits.sort_by_key(|h| rank(h, query));
    hits.truncate(200);
    Ok(hits)
}

/// Sort key: what was probably meant, first.
///
/// Relevance comes before origin. Ranking every repository package above every
/// AUR one sounds prudent, but searching "yazi" then buries the AUR package of
/// that very name under an unrelated `libyazi` from the repositories. Within the
/// same relevance, the reviewed source wins, then the AUR's own popularity —
/// the only quality signal it has.
fn rank(h: &Hit, query: &str) -> (u8, bool, i64, String) {
    let q = query.to_lowercase();
    let name = h.name.to_lowercase();
    let relevance = if name == q {
        0
    } else if name.starts_with(&q) {
        1
    } else if name.contains(&q) {
        2
    } else {
        // Matched on the description alone.
        3
    };
    (
        relevance,
        h.is_aur(),
        -(h.popularity.unwrap_or(0.0) * 1000.0) as i64,
        h.name.clone(),
    )
}

/// Repository search, straight through libalpm.
///
/// The handle is opened here rather than shared from the main state: it lives
/// on this thread, which sidesteps the question of moving a raw handle across
/// threads entirely, and opening it costs a few milliseconds.
fn repositories(query: &str) -> Vec<Hit> {
    let Ok(handle) = alpm::Alpm::new("/", "/var/lib/pacman") else {
        return Vec::new();
    };
    for repo in crate::data::configured_repos() {
        let _ = handle.register_syncdb(repo.as_str(), alpm::SigLevel::USE_DEFAULT);
    }
    let mut out = Vec::new();
    for db in handle.syncdbs() {
        let repo = db.name().to_string();
        let Ok(found) = db.search([query].iter().copied()) else {
            continue;
        };
        for p in found.iter() {
            out.push(Hit {
                name: p.name().to_string(),
                version: p.version().to_string(),
                repo: repo.clone(),
                description: p.desc().unwrap_or("").to_string(),
                installed: None,
                votes: None,
                popularity: None,
                out_of_date: false,
                maintainer: None,
                url: p.url().map(String::from),
            });
        }
    }
    out
}

/// AUR search through the RPC endpoint.
///
/// `by=name-desc` is what the web search does, and what people expect: matching
/// on the name alone misses most of what one is looking for.
fn aur(query: &str) -> Result<Vec<Hit>, String> {
    let url = format!(
        "https://aur.archlinux.org/rpc/v5/search/{}?by=name-desc",
        urlencode(query)
    );
    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "12", "-H", "User-Agent: nalarch", &url])
        .output()
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    if body.trim().is_empty() {
        return Err(crate::i18n::t("The AUR did not answer (network?).").to_string());
    }
    let v: Value =
        serde_json::from_str(&body).map_err(|_| crate::i18n::t("Unreadable AUR answer.").to_string())?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(err.to_string());
    }
    let Some(Value::Array(items)) = v.get("results") else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|r| {
            Some(Hit {
                name: r.get("Name")?.as_str()?.to_string(),
                version: r.get("Version")?.as_str().unwrap_or("").to_string(),
                repo: "aur".to_string(),
                description: r
                    .get("Description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                installed: None,
                votes: r.get("NumVotes").and_then(Value::as_u64).map(|v| v as u32),
                popularity: r.get("Popularity").and_then(Value::as_f64),
                out_of_date: r.get("OutOfDate").map(|v| !v.is_null()).unwrap_or(false),
                maintainer: r
                    .get("Maintainer")
                    .and_then(Value::as_str)
                    .map(String::from),
                url: r.get("URL").and_then(Value::as_str).map(String::from),
            })
        })
        .collect())
}

/// Percent-encoding for the query. Package names are tame, but a search term is
/// whatever someone typed, and a space would cut the URL in half.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_encoded_before_reaching_the_url() {
        assert_eq!(urlencode("gtk3"), "gtk3");
        // A space would end the URL and silently search for the first word only.
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("c++"), "c%2B%2B");
    }

    fn hit(name: &str, repo: &str, popularity: f64) -> Hit {
        Hit {
            name: name.into(),
            version: "1-1".into(),
            repo: repo.into(),
            description: String::new(),
            installed: None,
            votes: None,
            popularity: Some(popularity),
            out_of_date: false,
            maintainer: None,
            url: None,
        }
    }

    /// Relevance first, then the reviewed source, then popularity.
    #[test]
    fn results_are_ordered_by_what_was_probably_meant() {
        let mut hits = vec![
            hit("libyazi", "extra", 0.0),
            hit("yazi-git", "aur", 9.0),
            hit("yazi", "aur", 1.0),
            hit("yazi", "extra", 0.0),
        ];
        hits.sort_by_key(|h| rank(h, "yazi"));
        let vu: Vec<(&str, &str)> = hits
            .iter()
            .map(|h| (h.name.as_str(), h.repo.as_str()))
            .collect();
        assert_eq!(
            vu,
            vec![
                // The exact name, from the reviewed source.
                ("yazi", "extra"),
                // The exact name again, from the AUR.
                ("yazi", "aur"),
                // A prefix match beats a mere substring, whatever its origin —
                // otherwise searching "yazi" buries the AUR package of that name
                // under an unrelated `libyazi`.
                ("yazi-git", "aur"),
                ("libyazi", "extra"),
            ]
        );
    }
}
