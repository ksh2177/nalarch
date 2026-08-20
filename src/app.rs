//! Application state and navigation logic.

use crate::data::{self, State, Pkg};
use crate::exec::Session;
use crate::plan::{self, Plan};
use anyhow::Result;
use ratatui::widgets::ListState;
use std::collections::HashSet;

/// Active screen. The path is always the same: pick in the table, approve a
/// plan, then watch paru work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Table,
    Plan,
    Running,
    /// What an update changes, before launching it.
    Changelog,
    /// The AUR recipes of the plan, before launching it.
    Pkgbuild,
}

/// What the user is about to launch, with everything needed to show it first.
pub struct Intent {
    pub title: String,
    pub cmd: Vec<String>,
    /// Readable form of the command, when the real one is unreadable: a
    /// rollback lists a hundred and eighty cache paths, and showing them helps
    /// nobody understand what is about to run.
    pub display_command: Option<String>,
    pub plan: Plan,
    /// Flips how totals read: space is freed rather than taken.
    pub removal: bool,
    pub notes: Vec<String>,
    /// Points of attention, computed once when the plan is built.
    pub risks: Vec<crate::risks::Risk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Updates,
    Installed,
    Orphans,
    History,
    Search,
    Cache,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Updates,
        Tab::Installed,
        Tab::Orphans,
        Tab::History,
        Tab::Search,
        Tab::Cache,
    ];

    pub fn title(self) -> &'static str {
        crate::i18n::t(match self {
            Tab::Updates => "Updates",
            Tab::Installed => "tab|Installed",
            Tab::Orphans => "Orphans",
            Tab::History => "History",
            Tab::Search => "Search",
            Tab::Cache => "Cache",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
}

pub struct App {
    pub state: State,
    pub keep: HashSet<String>,
    /// Log of past transactions, re-read at startup and after every run: pacman
    /// itself is what writes it.
    pub history: Vec<crate::history::Transaction>,
    /// Versions still present in the local caches: that is what makes a
    /// rollback possible or not.
    pub caches: crate::history::Caches,
    /// Repository and AUR search, with its query and its background state.
    pub search: crate::search::Search,
    /// Scroll inside a transaction's detail: a full upgrade rarely fits in one
    /// panel.
    pub detail_scroll: u16,
    /// Size of the panel that actually holds paru's output, measured at render
    /// time. The pty must be sized from that, not from the window height: the
    /// "Download" and "Worth noting" blocks appear along the way, and the gap
    /// made the last lines produced vanish — exactly the ones one is reading for.
    pub pty_size: std::cell::Cell<(u16, u16)>,
    pub tab: usize,
    pub list: ListState,
    pub plan_list: ListState,
    /// Scroll of the points-of-attention panel, when it overflows.
    pub risks_scroll: u16,
    /// Switches between our view of the run and paru's raw output.
    pub raw_visible: bool,
    /// Where the transcript is pinned: the absolute index of its first visible
    /// event, or `None` to follow the operation as it goes.
    ///
    /// Absolute, not a distance from the tail. A distance drifts: every event
    /// that lands pushes the window forward by one, so a reader who stopped to
    /// look at something watches it slide off the top on its own.
    ///
    /// Cells because only the rendering knows how many rows the block ended up
    /// with, and therefore where the window really landed; it writes both back.
    pub journal_anchor: std::cell::Cell<Option<usize>>,
    /// First event shown by the last frame, so scrolling knows where it starts.
    pub journal_start: std::cell::Cell<usize>,
    pub changelog: Option<crate::changelog::Changelog>,
    pub changelog_scroll: u16,
    /// AUR recipes shown from the plan screen: (package names, concatenated
    /// PKGBUILD text). Read from paru's clone cache — see open_pkgbuild.
    pub pkgbuild: Option<(String, String)>,
    pub pkgbuild_scroll: u16,
    pub checked: HashSet<String>,
    pub filter: String,
    pub search_mode: bool,
    pub message: Option<(String, Severity)>,
    pub mode: Mode,
    pub intent: Option<Intent>,
    pub session: Option<Session>,
    pub quit: bool,
}

impl App {
    pub fn new_app() -> Result<Self> {
        let state = data::load()?;
        let keep = data::load_keep();
        let mut list = ListState::default();
        list.select(Some(0));
        Ok(Self {
            state,
            keep,
            history: crate::history::load(),
            caches: crate::history::Caches::index(),
            search: crate::search::Search::default(),
            detail_scroll: 0,
            pty_size: std::cell::Cell::new((0, 0)),
            tab: 0,
            list,
            plan_list: ListState::default(),
            risks_scroll: 0,
            raw_visible: false,
            journal_anchor: std::cell::Cell::new(None),
            journal_start: std::cell::Cell::new(0),
            changelog: None,
            changelog_scroll: 0,
            pkgbuild: None,
            pkgbuild_scroll: 0,
            checked: HashSet::new(),
            filter: String::new(),
            search_mode: false,
            message: None,
            mode: Mode::Table,
            intent: None,
            session: None,
            quit: false,
        })
    }

    pub fn current_tab(&self) -> Tab {
        Tab::ALL[self.tab]
    }

    /// Packages shown in the current tab, with the filter applied.
    pub fn rows(&self) -> Vec<&Pkg> {
        let source: Vec<&Pkg> = match self.current_tab() {
            Tab::Updates => self.state.updates.iter().collect(),
            Tab::Installed => self.state.installed.iter().filter(|p| p.is_root()).collect(),
            Tab::Orphans => self
                .state
                .installed
                .iter()
                .filter(|p| p.is_orphan())
                .collect(),
            Tab::History | Tab::Search | Tab::Cache => Vec::new(),
        };
        if self.filter.is_empty() {
            return source;
        }
        let f = self.filter.to_lowercase();
        source
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&f) || p.description.to_lowercase().contains(&f)
            })
            .collect()
    }

    pub fn selected(&self) -> Option<&Pkg> {
        let rows = self.rows();
        self.list.selected().and_then(|i| rows.get(i).copied())
    }

    pub fn is_protected(&self, name: &str) -> bool {
        self.keep.contains(name)
    }

    // ---- navigation ----

    /// Number of navigable entries in the current tab. Not every view lists
    /// packages: the history lists transactions.
    pub fn row_count(&self) -> usize {
        match self.current_tab() {
            Tab::History => self.filtered_history().len(),
            Tab::Search => self.search.hits().len(),
            Tab::Cache => 0,
            _ => self.rows().len(),
        }
    }

    /// History transactions, with the filter applied.
    pub fn filtered_history(&self) -> Vec<&crate::history::Transaction> {
        if self.filter.is_empty() {
            return self.history.iter().collect();
        }
        self.history
            .iter()
            .filter(|t| t.matches_text(&self.filter))
            .collect()
    }

    /// The result the cursor sits on, in the search tab.
    pub fn selected_hit(&self) -> Option<&crate::search::Hit> {
        self.list.selected().and_then(|i| self.search.hits().get(i))
    }

    /// Checked results qualified by their repository — `aur/plakar`,
    /// `extra/ripgrep`.
    ///
    /// Without the prefix, `paru -S plakar` is ambiguous: `plakar-git` provides
    /// `plakar` too, so paru asks which one is meant — after the choice has
    /// already been made by picking a row. The prefix is the syntax paru prints
    /// in its own resolution table, and it settles the question before it can be
    /// asked.
    pub fn checked_targets(&self) -> Vec<String> {
        self.search
            .hits()
            .iter()
            .filter(|h| self.checked.contains(&h.name))
            .map(|h| format!("{}/{}", h.repo, h.name))
            .collect()
    }

    /// Names checked in the search tab, split by where they come from: pacman
    /// resolves the repository ones, paru is the only one that knows the rest.
    pub fn checked_hits(&self) -> (Vec<String>, Vec<String>) {
        let mut repos = Vec::new();
        let mut aur = Vec::new();
        for h in self.search.hits() {
            if !self.checked.contains(&h.name) {
                continue;
            }
            if h.is_aur() {
                aur.push(h.name.clone());
            } else {
                repos.push(h.name.clone());
            }
        }
        (repos, aur)
    }

    /// Runs the search for what is currently typed.
    pub fn run_search(&mut self) {
        let installed: Vec<(String, String)> = self
            .state
            .installed
            .iter()
            .map(|p| (p.name.clone(), p.version.clone()))
            .collect();
        let query = self.filter.clone();
        self.search.start(&query, installed);
        self.checked.clear();
        self.go_to(0);
    }

    pub fn selected_transaction(&self) -> Option<&crate::history::Transaction> {
        let list = self.filtered_history();
        self.list.selected().and_then(|i| list.get(i).copied())
    }

    /// Scrolls the transcript. A positive delta goes back in time.
    ///
    /// It starts from where the last frame actually landed, so the first press
    /// after following the tail picks up from what is on screen rather than
    /// from a stale number.
    pub fn scroll_journal(&mut self, delta: i32) {
        let start = self.journal_start.get() as i32;
        self.journal_anchor
            .set(Some((start - delta).max(0) as usize));
    }

    /// Back to following the operation as it goes.
    pub fn follow_journal(&mut self) {
        self.journal_anchor.set(None);
    }

    /// Scrolls the selected transaction's detail.
    pub fn scroll_detail(&mut self, delta: i32) {
        self.detail_scroll = (self.detail_scroll as i32 + delta).max(0) as u16;
    }

    pub fn move_by(&mut self, delta: isize) {
        // Changing transaction resets its detail to the top: keeping the scroll
        // would show the middle of a list that has just been opened.
        self.detail_scroll = 0;
        let n = self.row_count();
        if n == 0 {
            self.list.select(None);
            return;
        }
        let courant = self.list.selected().unwrap_or(0) as isize;
        let new_app = (courant + delta).rem_euclid(n as isize);
        self.list.select(Some(new_app as usize));
    }

    pub fn go_to(&mut self, pos: usize) {
        self.detail_scroll = 0;
        let n = self.row_count();
        if n == 0 {
            self.list.select(None);
        } else {
            self.list.select(Some(pos.min(n - 1)));
        }
    }

    pub fn switch_tab(&mut self, delta: isize) {
        let n = Tab::ALL.len() as isize;
        self.tab = ((self.tab as isize + delta).rem_euclid(n)) as usize;
        self.filter.clear();
        self.go_to(0);
    }

    // ---- selection ----

    pub fn toggle_check(&mut self) {
        // The search tab lists results, not installed packages, so the cursor
        // resolves through a different collection. Everything after is the same.
        let name = match self.current_tab() {
            Tab::Search => match self.selected_hit() {
                Some(h) => h.name.clone(),
                None => return,
            },
            _ => match self.selected() {
                Some(p) => p.name.clone(),
                None => return,
            },
        };
        // Checking a protected package is refused in the removal views: that is
        // the entire point of keep.list.
        if self.is_removal_view() && self.is_protected(&name) {
            self.message = Some((
                crate::i18n::tf("{0} is protected — press p to lift the protection", &[&name]),
                Severity::Warning,
            ));
            return;
        }
        if !self.checked.remove(&name) {
            self.checked.insert(name);
        }
        self.move_by(1);
    }

    pub fn check_all(&mut self) {
        // Checking every search result at once would be a way to install a
        // hundred packages by accident; the search tab is left out on purpose.
        if self.current_tab() == Tab::Search {
            self.message = Some((
                crate::i18n::t("Check search results one by one — there is no “all”.").into(),
                Severity::Info,
            ));
            return;
        }
        let names: Vec<String> = self
            .rows()
            .iter()
            .map(|p| p.name.clone())
            .filter(|n| !(self.is_removal_view() && self.keep.contains(n)))
            .collect();
        let how_many = names.len();
        self.checked.extend(names);
        self.message = Some((
            crate::i18n::tf("{0} package(s) checked", &[&how_many.to_string()]),
            Severity::Info,
        ));
    }

    pub fn uncheck_all(&mut self) {
        self.checked.clear();
        self.message = Some((crate::i18n::t("Selection cleared").into(), Severity::Info));
    }

    /// True when the current tab is for removing packages.
    pub fn is_removal_view(&self) -> bool {
        matches!(
            self.current_tab(),
            Tab::Installed | Tab::Orphans
        )
    }

    /// Numeric summary of what is checked, shown permanently under the list so
    /// that what is about to happen is known without opening the plan.
    pub fn selection_summary(&self) -> Option<String> {
        let names = self.checked_visible();
        if names.is_empty() {
            return None;
        }
        let size = crate::theme::human_size;
        match self.current_tab() {
            Tab::Updates => {
                let p = plan::quick_plan(&self.state, &names);
                let mut s = crate::i18n::tf(
                    "{0} to update · {1} to download",
                    &[&names.len().to_string(), &size(p.total_dl)],
                );
                if p.aur_count > 0 {
                    s.push_str(&crate::i18n::tf(
                        " · {0} from the AUR (built from source)",
                        &[&p.aur_count.to_string()],
                    ));
                }
                Some(s)
            }
            Tab::Installed | Tab::Orphans => {
                let p = plan::removal_plan(&self.state, &names);
                Some(crate::i18n::tf(
                    "{0} to remove · {1} freed",
                    &[&names.len().to_string(), &size(-p.net)],
                ))
            }
            Tab::History | Tab::Search | Tab::Cache => None,
        }
    }

    /// Checked names that are actually present in the current tab.
    pub fn checked_visible(&self) -> Vec<String> {
        self.rows()
            .iter()
            .filter(|p| self.checked.contains(&p.name))
            .map(|p| p.name.clone())
            .collect()
    }

    // ---- protection ----

    pub fn toggle_protection(&mut self) {
        let Some(pkg) = self.selected() else { return };
        let name = pkg.name.clone();
        let protege = !self.keep.contains(&name);
        match data::toggle_keep(&name, protege) {
            Ok(()) => {
                if protege {
                    self.keep.insert(name.clone());
                    self.checked.remove(&name);
                    self.message =
                        Some((crate::i18n::tf("{0} protected", &[&name]), Severity::Success));
                } else {
                    self.keep.remove(&name);
                    self.message =
                        Some((crate::i18n::tf("{0} unprotected", &[&name]), Severity::Warning));
                }
            }
            Err(e) => {
                self.message = Some((
                    crate::i18n::tf("Failed: {0}", &[&e.to_string()]),
                    Severity::Warning,
                ))
            }
        }
    }

    // ---- actions ----

    /// Builds the plan for the requested action and switches to its approval
    /// screen. Nothing runs until the user has confirmed.
    pub fn apply(&mut self) {
        let intent = match self.current_tab() {
            Tab::Updates => {
                let mut targets = self.checked_visible();
                if targets.is_empty() {
                    self.message = Some((
                        crate::i18n::t("No package checked — press a to check them all").into(),
                        Severity::Warning,
                    ));
                    return;
                }
                // A partial selection must stay resolvable: pull in the updates
                // that the new versions of the selection depend on (see plan.rs).
                let pulled = plan::close_over_deps(
                    &mut targets,
                    &self.state.update_deps,
                    &self.state.update_provides,
                );
                let available: Vec<String> =
                    self.state.updates.iter().map(|p| p.name.clone()).collect();
                let excluded = plan::exclusions(&available, &targets);
                let plan = plan::build(&self.state, &targets, &excluded);
                let cmd = plan::upgrade_command(&available, &targets, plan.aur_count > 0);
                let mut notes = Vec::new();
                if !pulled.is_empty() {
                    notes.push(crate::i18n::tf(
                        "Pulled into the selection: {0} — the new versions of the checked packages depend on them.",
                        &[&pulled.join(", ")],
                    ));
                }
                if !excluded.is_empty() {
                    notes.push(crate::i18n::tf(
                        "{0} update(s) excluded through --ignore: {1}.",
                        &[&excluded.len().to_string(), &excluded.join(", ")],
                    ));
                    notes.push(
                        crate::i18n::t("A partial upgrade is not supported on Arch: keep it for one-off cases.")
                            .into(),
                    );
                }
                if plan.aur_count > 0 {
                    notes.push(crate::i18n::tf(
                        "{0} AUR package(s): built from source, with unpredictable duration and size.",
                        &[&plan.aur_count.to_string()],
                    ));
                    notes.push(
                        crate::i18n::t("paru will ask its own questions (reading the PKGBUILD, PGP keys).")
                            .into(),
                    );
                } else {
                    notes.push(
                        crate::i18n::t("paru will run without asking again: this approval is what counts.")
                            .into(),
                    );
                }
                Intent {
                    display_command: None,
                    // The title announces what will really happen. "Full system
                    // upgrade" described the command (paru -Syu) and not the
                    // transaction: in front of a single package to update, it
                    // suggested a major undertaking.
                    title: {
                        let n = plan.rows.len();
                        let new_count = plan.count(crate::plan::Kind::New);
                        let mut t =
                            crate::i18n::tf("Update · {0} package(s)", &[&n.to_string()]);
                        if new_count > 0 {
                            t.push_str(&crate::i18n::tf(
                                ", {0} of them new",
                                &[&new_count.to_string()],
                            ));
                        }
                        if !excluded.is_empty() {
                            t.push_str(&crate::i18n::tf(
                                " · {0} left out",
                                &[&excluded.len().to_string()],
                            ));
                        }
                        t
                    },
                    cmd,
                    risks: crate::risks::analyze(&plan, &self.state, &excluded, false),
                    plan,
                    removal: false,
                    notes,
                }
            }
            Tab::Installed | Tab::Orphans => {
                let targets: Vec<String> = self
                    .checked_visible()
                    .into_iter()
                    .filter(|n| !self.keep.contains(n))
                    .collect();
                if targets.is_empty() {
                    self.message =
                        Some((crate::i18n::t("No package checked").into(), Severity::Warning));
                    return;
                }
                let mut cmd = vec!["paru".into(), "-Rns".into(), "--noconfirm".into()];
                cmd.extend(targets.iter().cloned());
                let plan = plan::removal_plan(&self.state, &targets);
                Intent {
                    title: crate::i18n::tf(
                        "Removal · {0} package(s)",
                        &[&plan.rows.len().to_string()],
                    ),
                    display_command: None,
                    risks: crate::risks::analyze(&plan, &self.state, &[], true),
                    plan,
                    cmd,
                    removal: true,
                    notes: vec![
                        crate::i18n::t("-Rns also removes dependencies nothing needs any more, along with configuration files.").into(),
                    ],
                }
            }
            Tab::History => {
                let Some(intent) = self.rollback_intent() else {
                    return;
                };
                intent
            }
            Tab::Search => {
                let Some(intent) = self.install_intent() else {
                    return;
                };
                intent
            }
            Tab::Cache => {
                // The retention configured for paccache.timer is reused, so the
                // cache does not oscillate between two policies.
                let keep = self.state.cache_keep;
                Intent {
                    title: format!("Nettoyage du cache (garder {keep} versions)"),
                    display_command: None,
                    cmd: vec!["sudo".into(), "paccache".into(), format!("-rk{keep}")],
                    plan: plan::empty(),
                    risks: Vec::new(),
                    removal: true,
                    notes: vec![crate::i18n::tf(
                        "Frees roughly {0} of old versions.",
                        &[&crate::theme::human_size(self.state.cache_prunable as i64)],
                    )],
                }
            }
        };
        self.intent = Some(intent);
        self.mode = Mode::Plan;
    }

    /// Rebuilds the foreign packages that `checkrebuild` flagged: same version,
    /// same source, but linked against the libraries present NOW. This is the
    /// case updates cannot express — after a soname bump (Qt, boost…) the -git
    /// package has no new version to offer, it is simply broken until rebuilt.
    pub fn rebuild(&mut self) {
        if self.state.rebuilds.is_empty() {
            let msg = if self.state.rebuild_checker {
                crate::i18n::t("Nothing to rebuild: no foreign package links against a missing library.")
            } else {
                crate::i18n::t("Detection needs checkrebuild — install the rebuild-detector package.")
            };
            self.message = Some((msg.into(), Severity::Info));
            return;
        }
        let targets = self.state.rebuilds.clone();
        let plan = plan::install_plan(&self.state, &[], &targets);
        let mut cmd = vec![
            "paru".to_string(),
            "-S".to_string(),
            "--rebuild".to_string(),
            "--noprovides".to_string(),
        ];
        cmd.extend(targets.iter().cloned());
        let notes = vec![
            crate::i18n::t(
                "These packages link against libraries that no longer exist (typically after a Qt/boost-style upgrade): same version, rebuilt from source against the new ones.",
            )
            .into(),
            crate::i18n::t("paru will ask its own questions (reading the PKGBUILD, PGP keys).").into(),
        ];
        self.intent = Some(Intent {
            display_command: None,
            title: crate::i18n::tf("Rebuild · {0} package(s)", &[&targets.len().to_string()]),
            cmd,
            risks: crate::risks::analyze(&plan, &self.state, &[], false),
            plan,
            removal: false,
            notes,
        });
        self.mode = Mode::Plan;
    }

    /// Shows the AUR recipes of the current plan, read from paru's clone
    /// cache. The point is knowing what will run BEFORE launching: paru only
    /// offers its own review when the recipe changed since the last build, so
    /// an unchanged PKGBUILD sails straight to "Proceed?" with nothing shown.
    pub fn open_pkgbuild(&mut self) {
        let Some(intent) = &self.intent else { return };
        let aur: Vec<String> = intent
            .plan
            .rows
            .iter()
            .filter(|r| r.aur)
            .map(|r| r.name.clone())
            .collect();
        if aur.is_empty() {
            self.message = Some((
                crate::i18n::t("No AUR package in this plan: repository packages carry no PKGBUILD to read.")
                    .into(),
                Severity::Info,
            ));
            return;
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let mut text = String::new();
        for name in &aur {
            let path = format!("{home}/.cache/paru/clone/{name}/PKGBUILD");
            text.push_str(&format!("──── {name} ─── {path}

"));
            match std::fs::read_to_string(&path) {
                Ok(content) => text.push_str(&content),
                Err(_) => text.push_str(crate::i18n::t(
                    "Not cloned yet: paru fetches it at launch and will offer the review then.",
                )),
            }
            text.push('\n');
        }
        self.pkgbuild = Some((aur.join(", "), text));
        self.pkgbuild_scroll = 0;
        self.mode = Mode::Pkgbuild;
    }

    /// Builds the inverse of the transaction selected in the history. Nothing
    /// runs: it goes through the plan screen, like every other action.
    fn rollback_intent(&mut self) -> Option<Intent> {
        let Some(t) = self.selected_transaction() else {
            self.message = Some((
                crate::i18n::t("No transaction selected").into(),
                Severity::Warning,
            ));
            return None;
        };
        let t = t.clone();
        let rollbacks = crate::history::rollback_plan(&self.caches, &t);
        if rollbacks.is_empty() {
            self.message = Some((
                crate::i18n::t("This transaction has nothing to undo (a reinstall only)").into(),
                Severity::Info,
            ));
            return None;
        }
        let Some(cmd) = crate::history::rollback_command(&rollbacks) else {
            self.message = Some((
                crate::i18n::t("Rollback impossible: none of the versions involved is still in cache").into(),
                Severity::Warning,
            ));
            return None;
        };

        let plan = plan::from_rollback(&rollbacks);
        let missing = rollbacks.iter().filter(|r| !r.is_possible()).count();
        let mut notes = vec![
            crate::i18n::tf(
                "Transaction of {0} — {1}.",
                &[&crate::history::short_date(&t.timestamp), &t.summary()],
            ),
            crate::i18n::t(
                "The packages come from local caches: nothing is downloaded, no repository is queried.",
            )
            .into(),
        ];
        if missing > 0 {
            notes.push(crate::i18n::tf(
                "{0} package(s) will stay as they are, for lack of a cached version.",
                &[&missing.to_string()],
            ));
        }
        let risks = crate::risks::analyze_rollback(&rollbacks, &t, &self.state);
        Some(Intent {
            display_command: Some(crate::history::readable_command(&rollbacks)),
            title: crate::i18n::tf(
                "Rollback · {0} package(s) of {1}",
                &[&plan.rows.len().to_string(), &rollbacks.len().to_string()],
            ),
            cmd,
            plan,
            removal: false,
            notes,
            risks,
        })
    }

    /// Builds the plan for installing what is checked in the search tab.
    fn install_intent(&mut self) -> Option<Intent> {
        let (repos, aur) = self.checked_hits();
        if repos.is_empty() && aur.is_empty() {
            self.message = Some((
                crate::i18n::t("No package checked — space to check one").into(),
                Severity::Warning,
            ));
            return None;
        }
        let plan = plan::install_plan(&self.state, &repos, &aur);
        let targets = self.checked_targets();

        let mut cmd = vec!["paru".to_string(), "-S".to_string()];
        // The targets come from a list of real package names, so paru has
        // nothing to resolve about them. Left on, its provider search asks which
        // of `plakar` and `plakar-git` was meant — after the choice was made by
        // checking a row. Qualifying the target as `aur/plakar` does not settle
        // it: the prefix scopes the repository, not the search for providers.
        //
        // The cost is stated rather than hidden: `--provides` also covers
        // dependencies that no package satisfies by name, and one of those now
        // fails to resolve instead of offering a menu. That failure is loud and
        // lands in the error list; the question was silent and happened on every
        // ambiguous install.
        cmd.push("--noprovides".to_string());
        // As with an upgrade, paru keeps its questions when the AUR is involved:
        // reading the PKGBUILD is the one chance to see what will run.
        if aur.is_empty() {
            cmd.push("--noconfirm".to_string());
        }
        cmd.extend(targets.iter().cloned());

        let pulled = plan.count(plan::Kind::New);
        let mut notes = vec![crate::i18n::tf(
            "{0} asked for, {1} pulled in as dependencies.",
            &[&targets.len().to_string(), &pulled.to_string()],
        )];
        if !aur.is_empty() {
            notes.push(
                crate::i18n::t("AUR packages are built from source: their size and duration cannot be known beforehand.")
                    .into(),
            );
        }
        Some(Intent {
            display_command: None,
            title: crate::i18n::tf("Install · {0} package(s)", &[&plan.rows.len().to_string()]),
            cmd,
            risks: crate::risks::analyze(&plan, &self.state, &[], false),
            plan,
            removal: false,
            notes,
        })
    }

    /// Actually launches the command, in an embedded pseudo terminal.
    pub fn start(&mut self, rows: u16, cols: u16) {
        let Some(intent) = &self.intent else {
            return;
        };
        self.journal_anchor.set(None);
        match Session::spawn(&intent.cmd, rows, cols) {
            Ok(s) => {
                self.session = Some(s);
                self.mode = Mode::Running;
            }
            Err(e) => {
                self.message = Some((
                    crate::i18n::tf("Cannot launch: {0}", &[&e.to_string()]),
                    Severity::Warning,
                ));
                self.mode = Mode::Table;
            }
        }
    }

    /// Back to the table after a run, reloading the state.
    pub fn finish(&mut self) -> Result<()> {
        self.session = None;
        self.intent = None;
        self.mode = Mode::Table;
        self.reload()
    }

    /// Opens the changelog detail for the selected package.
    pub fn open_changelog(&mut self) {
        let Some(p) = self.selected() else { return };
        // Without a target version there is no "change" to show: this is an
        // installed package, not an update.
        let Some(target) = p.target_version.clone() else {
            self.message = Some((
                crate::i18n::t("No pending update for this package").into(),
                Severity::Info,
            ));
            return;
        };
        self.changelog = Some(crate::changelog::Changelog::spawn(
            &p.name,
            &p.version,
            &target,
            p.origin == crate::data::Origin::Aur,
        ));
        self.changelog_scroll = 0;
        self.mode = Mode::Changelog;
    }

    /// Purges from the cache the packages that are no longer installed (Cache tab).
    pub fn purge_uninstalled(&mut self) {
        if self.current_tab() != Tab::Cache {
            return;
        }
        self.intent = Some(Intent {
            display_command: None,
            title: crate::i18n::t("Purge uninstalled packages from the cache").into(),
            cmd: vec!["sudo".into(), "paccache".into(), "-ruk0".into()],
            plan: plan::empty(),
            risks: Vec::new(),
            removal: true,
            notes: vec![crate::i18n::tf(
                "Frees roughly {0} — these packages are no longer installed, but you lose the ability to reinstall them offline.",
                &[&crate::theme::human_size(self.state.cache_uninstalled as i64)],
            )],
        });
        self.mode = Mode::Plan;
    }

    pub fn reload(&mut self) -> Result<()> {
        self.state = data::load()?;
        self.keep = data::load_keep();
        self.history = crate::history::load();
        self.caches = crate::history::Caches::index();
        self.checked.clear();
        self.go_to(0);
        Ok(())
    }
}
