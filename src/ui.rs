//! Rendu ratatui.

use crate::app::{App, Severity, Mode, Tab};
use crate::i18n::{t, tf};
use crate::data::{Origin, Pkg};
use crate::plan::Kind;
use crate::risks::Level;
use crate::theme::{self, human_size};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

pub fn draw(f: &mut Frame, app: &mut App) {
    let zone = f.area();
    // No background is painted: the terminal's own must stay visible so that
    // the light/dark switch applies inside nalarch too.

    match app.mode {
        Mode::Table => table_screen(f, app, zone),
        Mode::Plan => plan_screen(f, app, zone),
        Mode::Running => run_screen(f, app, zone),
        Mode::Changelog => changelog_screen(f, app, zone),
    }
}

fn table_screen(f: &mut Frame, app: &mut App, zone: Rect) {
    // Two rows at the bottom: the legend stays visible at all times, and the
    // line above carries the selection state or the last message. Merging them
    // made the shortcuts vanish as soon as a package was checked.
    let [top, middle, statut, legende] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(zone);

    header(f, app, top);

    if app.current_tab() == Tab::Cache {
        cache_panel(f, app, middle);
    } else if app.current_tab() == Tab::Search {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                .areas(middle);
        search_list(f, app, left);
        hit_detail(f, app, right);
    } else if app.current_tab() == Tab::History {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
                .areas(middle);
        transaction_list(f, app, left);
        transaction_detail(f, app, right);
    } else {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                .areas(middle);
        package_list(f, app, left);
        details_panel(f, app, right);
    }

    status_line(f, app, statut);
    legend_line(f, app, legende);
}

fn header(f: &mut Frame, app: &App, zone: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|o| {
            let count = match o {
                Tab::Updates => app.state.updates.len(),
                Tab::Installed => app.state.installed.iter().filter(|p| p.is_root()).count(),
                Tab::Orphans => {
                    app.state.installed.iter().filter(|p| p.is_orphan()).count()
                }
                Tab::History => app.history.len(),
                Tab::Search => app.search.hits().len(),
                Tab::Cache => app.state.cache_files,
            };
            Line::from(vec![
                Span::raw(format!("{}{}", crate::icons::tab(*o), o.title())),
                Span::styled(
                    format!(" {count}"),
                    Style::default().fg(theme::DIM),
                ),
            ])
        })
        .collect();

    let summary = format!(
        " {} ",
        tf(
            "{0} packages · cache {1}",
            &[
                &app.state.installed.len().to_string(),
                &human_size(app.state.cache_bytes as i64)
            ]
        )
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::DIM))
        .title(Span::styled(
            " nalarch ",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .title_top(Line::from(Span::styled(summary, Style::default().fg(theme::DIM))).right_aligned());

    let tabs_widget = Tabs::new(titles)
        .block(block)
        .select(app.tab)
        .style(Style::default().fg(theme::FG))
        .highlight_style(
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("│", Style::default().fg(theme::DIM)));

    f.render_widget(tabs_widget, zone);
}

/// One list row: checkbox, name, version transition, repository, size.
fn package_row(app: &App, p: &Pkg, width: u16) -> ListItem<'static> {
    let coche = app.checked.contains(&p.name);
    let protege = app.is_protected(&p.name);

    let case = if protege {
        Span::styled("[·] ", Style::default().fg(theme::YELLOW))
    } else if coche {
        Span::styled("[x] ", Style::default().fg(theme::GREEN))
    } else {
        Span::styled("[ ] ", Style::default().fg(theme::DIM))
    };

    // Name width = whatever is left once the fixed columns are subtracted.
    // 52 = 2 borders + 1 selection symbol + 4 checkbox + 15 version + 2 arrow
    //      + 15 target + 12 repository + 1 space. The repository gets 12 so that
    //      third-party repositories fit (chaotic-aur, endeavouros…), plus the
    //      icon column when glyphs are on.
    let name_width = ((width as usize).saturating_sub(52 + crate::icons::repo_width()))
        .clamp(12, 40);
    let name = if p.name.chars().count() > name_width {
        format!("{} ", truncate(&p.name, name_width))
    } else {
        format!("{:<width$} ", p.name, width = name_width)
    };

    let mut spans = vec![
        case,
        Span::styled(
            name,
            Style::default()
                .fg(if protege { theme::YELLOW } else { theme::FG })
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(target) = &p.target_version {
        spans.push(Span::styled(
            format!("{:>14} ", truncate(&p.version, 14)),
            Style::default().fg(theme::DIM),
        ));
        spans.push(Span::styled("→ ", Style::default().fg(theme::ACCENT)));
        spans.push(Span::styled(
            format!("{:<14} ", truncate(target, 14)),
            Style::default().fg(theme::GREEN),
        ));
    } else {
        spans.push(Span::styled(
            format!("{:<16} ", truncate(&p.version, 16)),
            Style::default().fg(theme::DIM),
        ));
        spans.push(Span::styled(
            format!("{:>9} ", human_size(p.installed_size)),
            Style::default().fg(theme::DIM),
        ));
    }

    spans.push(Span::styled(
        format!("{}{:<12}", crate::icons::repo(&p.repo), truncate(&p.repo, 12)),
        Style::default().fg(theme::repo_color(&p.repo)),
    ));

    ListItem::new(Line::from(spans))
}

/// Truncates to `n` columns on character boundaries, never byte ones: an
/// accented version or name would make a raw byte slice panic.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let gardes: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{gardes}…")
    } else {
        s.to_string()
    }
}

fn package_list(f: &mut Frame, app: &mut App, zone: Rect) {
    let rows = app.rows();
    let title = if app.filter.is_empty() {
        format!(" {} ", app.current_tab().title())
    } else {
        format!(" {} · {} ", app.current_tab().title(), tf("filter “{0}”", &[&app.filter]))
    };

    if rows.is_empty() {
        let msg = t(match app.current_tab() {
            Tab::Updates => "System is up to date.",
            Tab::Orphans => "No orphans.",
            _ => "Nothing to show.",
        });
        let p = Paragraph::new(Line::from(Span::styled(
            msg,
            Style::default().fg(theme::GREEN),
        )))
        .alignment(Alignment::Center)
        .block(framed(&title));
        f.render_widget(p, zone);
        return;
    }

    let items: Vec<ListItem> = rows
        .iter()
        .map(|p| package_row(app, p, zone.width))
        .collect();

    let list = List::new(items)
        .block(framed(&title))
        .highlight_style(theme::selected())
        .highlight_symbol("▌");

    f.render_stateful_widget(list, zone, &mut app.list);
}

fn framed(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::DIM))
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(theme::ACCENT),
        ))
}

// The pushes are sequential rather than one `vec![]` because the list carries
// on with conditional entries just below; splitting it in two would read worse.
#[allow(clippy::vec_init_then_push)]
fn details_panel(f: &mut Frame, app: &App, zone: Rect) {
    let Some(p) = app.selected() else {
        f.render_widget(framed(&format!(" {} ", t("Details"))), zone);
        return;
    };

    let mut l: Vec<Line> = Vec::new();

    l.push(Line::from(Span::styled(
        p.name.clone(),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    l.push(Line::from(Span::styled(
        p.description.clone(),
        Style::default().fg(theme::FG),
    )));
    l.push(Line::raw(""));

    l.push(field(
        t("Repository"),
        &format!("{}{}", crate::icons::repo(&p.repo), p.repo),
        theme::repo_color(&p.repo),
    ));
    l.push(field(t("Version"), &p.version, theme::FG));
    if let Some(c) = &p.target_version {
        l.push(field(t("Target"), c, theme::GREEN));
        if p.origin == Origin::Aur {
            l.push(field(t("Note"), t("built from source"), theme::YELLOW));
        }
        if let Some(dl) = p.download_size {
            l.push(field(t("Download"), &human_size(dl), theme::FG));
        }
    }
    l.push(field(t("Size"), &human_size(p.installed_size), theme::FG));
    l.push(field(
        t("Installed"),
        if p.explicit {
            t("explicitly")
        } else {
            t("as a dependency")
        },
        theme::FG,
    ));

    if app.is_protected(&p.name) {
        l.push(Line::raw(""));
        l.push(Line::from(Span::styled(
            t("🔒 protected — removal blocked"),
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // The block that decides any removal: who needs this package.
    l.push(Line::raw(""));
    if p.required_by.is_empty() {
        l.push(Line::from(Span::styled(
            t("Required by: nothing"),
            Style::default().fg(theme::DIM),
        )));
    } else {
        l.push(Line::from(Span::styled(
            tf("Required by ({0}):", &[&p.required_by.len().to_string()]),
            Style::default()
                .fg(theme::RED)
                .add_modifier(Modifier::BOLD),
        )));
        for n in p.required_by.iter().take(8) {
            l.push(Line::from(Span::styled(
                format!("  {n}"),
                Style::default().fg(theme::FG),
            )));
        }
        if p.required_by.len() > 8 {
            l.push(Line::from(Span::styled(
                format!("  {}", tf("… and {0} others", &[&(p.required_by.len() - 8).to_string()])),
                Style::default().fg(theme::DIM),
            )));
        }
    }

    if !p.optional_for.is_empty() {
        l.push(Line::from(Span::styled(
            tf("Optional for ({0}):", &[&p.optional_for.len().to_string()]),
            Style::default().fg(theme::YELLOW),
        )));
        for n in p.optional_for.iter().take(5) {
            l.push(Line::from(Span::styled(
                format!("  {n}"),
                Style::default().fg(theme::FG),
            )));
        }
    }

    l.push(Line::raw(""));
    l.push(Line::from(Span::styled(
        tf("Depends on: {0} package(s)", &[&p.depends_on.len().to_string()]),
        Style::default().fg(theme::DIM),
    )));

    let para = Paragraph::new(l)
        .block(framed(&format!(" {} ", t("Details"))))
        .wrap(Wrap { trim: true });
    f.render_widget(para, zone);
}

fn field(key: &str, value: &str, colour: ratatui::style::Color) -> Line<'static> {
    field_w(key, value, colour, 15)
}

/// A "label  value" line with an explicit label width. The trailing space
/// guarantees a separation even when the label exceeds the requested width.
fn field_w(key: &str, value: &str, colour: ratatui::style::Color, l: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key:<l$} ", l = l),
            Style::default().fg(theme::DIM),
        ),
        Span::styled(value.to_string(), Style::default().fg(colour)),
    ])
}

fn cache_panel(f: &mut Frame, app: &App, zone: Rect) {
    let e = &app.state;
    const L: usize = 21;
    let rows = vec![
        Line::raw(""),
        field_w(t("Location"), "/var/cache/pacman/pkg", theme::FG, L),
        field_w(t("Files"), &e.cache_files.to_string(), theme::FG, L),
        field_w(t("Total size"), &human_size(e.cache_bytes as i64), theme::YELLOW, L),
        field_w(
            t("Retention"),
            &tf("{0} versions per package", &[&e.cache_keep.to_string()]),
            theme::FG,
            L,
        ),
        field_w(
            t("Old versions"),
            &human_size(e.cache_prunable as i64),
            if e.cache_prunable > 0 { theme::GREEN } else { theme::DIM },
            L,
        ),
        field_w(
            t("Uninstalled packages"),
            &human_size(e.cache_uninstalled as i64),
            if e.cache_uninstalled > 0 { theme::GREEN } else { theme::DIM },
            L,
        ),
        Line::raw(""),
        Line::from(Span::styled(
            t("The cache is what makes a rollback possible: old versions stay"),
            Style::default().fg(theme::DIM),
        )),
        Line::from(Span::styled(
            t("reinstallable there through pacman -U. Do not empty it entirely —"),
            Style::default().fg(theme::DIM),
        )),
        Line::from(Span::styled(
            t("it is the only safety net short of a Btrfs snapshot."),
            Style::default().fg(theme::DIM),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("u", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(
                    "  paccache -rk{k}  {}",
                    tf(
                        "— applies the configured retention ({0} versions)",
                        &[&e.cache_keep.to_string()]
                    ),
                    k = e.cache_keep
                ),
                Style::default().fg(theme::FG),
            ),
        ]),
        Line::from(vec![
            Span::styled("U", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  paccache -ruk0  {}", t("— purges packages that are no longer installed")),
                Style::default().fg(theme::FG),
            ),
        ]),
    ];
    let p = Paragraph::new(rows)
        .block(framed(&format!(" {} ", t("Package cache"))))
        .wrap(Wrap { trim: true });
    f.render_widget(p, zone);
}

/// Search results: what matched, where it comes from, and whether it is already
/// here. The repository column carries the same colour code as everywhere else,
/// which is what makes an AUR result recognisable at a glance.
fn search_list(f: &mut Frame, app: &mut App, zone: Rect) {
    use crate::search::State;
    let title = if app.search.query.is_empty() {
        format!(" {} ", t("Search"))
    } else {
        format!(" {} ", tf("Search · “{0}”", &[&app.search.query]))
    };

    let message = match &app.search.state {
        State::Idle => Some(t("Press / to type a query, Enter to search the repositories and the AUR.").to_string()),
        State::Running => Some(tf("Searching for “{0}”…", &[&app.search.query])),
        State::Failed(e) => Some(e.clone()),
        State::Done(h) if h.is_empty() => Some(tf("Nothing matches “{0}”.", &[&app.search.query])),
        State::Done(_) => None,
    };
    if let Some(msg) = message {
        let colour = match &app.search.state {
            State::Failed(_) => theme::YELLOW,
            _ => theme::DIM,
        };
        let p = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(colour))))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(framed(&title));
        f.render_widget(p, zone);
        return;
    }

    let width = zone.width.saturating_sub(2) as usize;
    // 4 checkbox + 13 version + 12 repository + 12 for the marker, the rest to
    // the name. The version gets one column more than it uses so a truncated one
    // never runs into the repository, and the marker column is reserved even
    // when empty — otherwise "installed" is what falls off the right edge, and
    // that is the one word worth reading on the row.
    let repo_w = 12 + crate::icons::repo_width();
    let name_w = width.saturating_sub(4 + 13 + repo_w + 12 + 2).clamp(12, 40);

    let items: Vec<ListItem> = app
        .search
        .hits()
        .iter()
        .map(|h| {
            let checked = app.checked.contains(&h.name);
            let mut spans = vec![
                Span::styled(
                    if checked { "[x] " } else { "[ ] " },
                    Style::default().fg(if checked { theme::GREEN } else { theme::DIM }),
                ),
                Span::styled(
                    format!("{:<name_w$}", truncate(&h.name, name_w)),
                    Style::default()
                        .fg(if h.installed.is_some() { theme::GREEN } else { theme::FG })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<13}", truncate(&h.version, 12)),
                    Style::default().fg(theme::DIM),
                ),
                Span::styled(
                    format!(
                        "{}{:<w$}",
                        crate::icons::repo(&h.repo),
                        truncate(&h.repo, 12),
                        w = 12
                    ),
                    Style::default().fg(theme::repo_color(&h.repo)),
                ),
            ];
            // Two facts worth more than the description on a crowded row: it is
            // already here, or someone flagged it as stale. Out of date wins the
            // column when both apply — it is the one that calls for a decision.
            let (mark, colour) = if h.out_of_date {
                (t("out of date"), theme::RED)
            } else if h.installed.is_some() {
                (t("installed"), theme::GREEN)
            } else {
                ("", theme::DIM)
            };
            spans.push(Span::styled(
                format!(" {}", truncate(mark, 11)),
                Style::default().fg(colour),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(framed(&title))
        .highlight_style(theme::selected())
        .highlight_symbol("▌");
    f.render_stateful_widget(list, zone, &mut app.list);
}

/// Detail of the selected result. For an AUR package it carries what the AUR
/// itself uses to judge one: how many people voted, how used it is, whether
/// anyone still maintains it.
fn hit_detail(f: &mut Frame, app: &App, zone: Rect) {
    let Some(h) = app.selected_hit() else {
        f.render_widget(framed(&format!(" {} ", t("Details"))), zone);
        return;
    };
    const L: usize = 15;
    let mut l = vec![
        Line::from(Span::styled(
            h.name.clone(),
            Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            h.description.clone(),
            Style::default().fg(theme::FG),
        )),
        Line::raw(""),
        field(t("Repository"), &format!("{}{}", crate::icons::repo(&h.repo), h.repo), theme::repo_color(&h.repo)),
        field(t("Version"), &h.version, theme::FG),
    ];
    if let Some(v) = &h.installed {
        l.push(field(t("Installed"), v, theme::GREEN));
    }
    if let Some(u) = &h.url {
        l.push(field(t("Upstream"), u, theme::CYAN));
    }

    if h.is_aur() {
        l.push(Line::raw(""));
        if let (Some(v), Some(p)) = (h.votes, h.popularity) {
            l.push(field(
                t("Votes"),
                &tf("{0} · popularity {1}", &[&v.to_string(), &format!("{p:.2}")]),
                theme::FG,
            ));
        }
        // An orphaned AUR package is nobody's responsibility any more: that is
        // the single most useful thing to know before building it.
        match &h.maintainer {
            Some(name) => l.push(field(t("Maintainer"), name, theme::FG)),
            None => l.push(field(t("Maintainer"), t("none — orphaned"), theme::YELLOW)),
        }
        if h.out_of_date {
            l.push(Line::raw(""));
            l.push(Line::from(Span::styled(
                t("⚑ Flagged out of date by a user."),
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            )));
        }
        l.push(Line::raw(""));
        for chunk in wrap_indent(
            t("Built from a PKGBUILD, a script that runs under your account. paru will offer to show it before building."),
            zone.width.saturating_sub(2) as usize,
            "",
        ) {
            l.push(Line::from(Span::styled(chunk, Style::default().fg(theme::DIM))));
        }
    }

    let _ = L;
    let para = Paragraph::new(l)
        .block(framed(&format!(" {} ", t("Details"))))
        .wrap(Wrap { trim: true });
    f.render_widget(para, zone);
}

/// The list of past transactions, most recent at the top.
fn transaction_list(f: &mut Frame, app: &mut App, zone: Rect) {
    let transactions = app.filtered_history();
    let title = if app.filter.is_empty() {
        format!(" {} ", t("History"))
    } else {
        format!(" {} · {} ", t("History"), tf("filter “{0}”", &[&app.filter]))
    };

    if transactions.is_empty() {
        let msg = if app.filter.is_empty() {
            t("No transaction in /var/log/pacman.log.")
        } else {
            t("No transaction touches this package.")
        };
        let p = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(theme::DIM))))
            .alignment(Alignment::Center)
            .block(framed(&title));
        f.render_widget(p, zone);
        return;
    }

    let width = zone.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = transactions
        .iter()
        .map(|tx| {
            let date = crate::history::short_date(&tx.timestamp);
            let relative = crate::history::relative_time(tx.instant);
            // Row 1: when, and how long ago.
            let mut first_line = vec![
                Span::styled(date, Style::default().fg(theme::FG)),
                Span::styled(format!("  {relative}"), Style::default().fg(theme::DIM)),
            ];
            if !tx.completed {
                first_line.push(Span::styled(
                    format!("  {}", t("interrupted")),
                    Style::default().fg(theme::RED),
                ));
            }
            // Row 2: the count by kind, with the plan's symbols.
            let mut second_line = vec![Span::raw("  ")];
            for (act, colour) in [
                (crate::history::Act::Upgraded, theme::CYAN),
                (crate::history::Act::Installed, theme::GREEN),
                (crate::history::Act::Downgraded, theme::YELLOW),
                (crate::history::Act::Removed, theme::RED),
                (crate::history::Act::Reinstalled, theme::DIM),
            ] {
                let n = tx.count(act);
                if n > 0 {
                    second_line.push(Span::styled(
                        format!("{} {n}  ", act.symbol()),
                        Style::default().fg(colour),
                    ));
                }
            }
            second_line.push(Span::styled(
                truncate(&tx.trigger(), width.saturating_sub(20)),
                Style::default().fg(theme::DIM),
            ));
            ListItem::new(vec![Line::from(first_line), Line::from(second_line)])
        })
        .collect();

    let list = List::new(items)
        .block(framed(&title))
        .highlight_style(theme::selected())
        .highlight_symbol("▌");

    f.render_stateful_widget(list, zone, &mut app.list);
}

/// A transaction's detail: what it did, and what a rollback could restore.
/// Recoverability is computed here rather than at approval time: it is the
/// information that decides whether the rollback makes sense at all.
fn transaction_detail(f: &mut Frame, app: &App, zone: Rect) {
    let Some(tx) = app.selected_transaction() else {
        f.render_widget(framed(&format!(" {} ", t("Detail"))), zone);
        return;
    };
    let rollbacks = crate::history::rollback_plan(&app.caches, tx);
    let width = zone.width.saturating_sub(2) as usize;

    let mut rows: Vec<Line> = Vec::new();
    const L: usize = 12;
    rows.push(field_w(
        crate::i18n::t("Date"),
        &crate::history::short_date(&tx.timestamp),
        theme::FG,
        L,
    ));
    if let Some(cmd) = &tx.command {
        for (i, chunk) in wrap_indent(cmd, width.saturating_sub(L), "")
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                rows.push(field_w(t("Command"), chunk.trim(), theme::CYAN, L));
            } else {
                rows.push(Line::from(Span::styled(
                    format!("{:L$}{}", "", chunk.trim()),
                    Style::default().fg(theme::CYAN),
                )));
            }
        }
    }
    if let Some(d) = tx.duration {
        let duration = if d < 1 {
            crate::i18n::t("less than a second").to_string()
        } else if d < 60 {
            format!("{d} s")
        } else {
            format!("{} min {} s", d / 60, d % 60)
        };
        rows.push(field_w(crate::i18n::t("Duration"), &duration, theme::DIM, L));
    }
    if !tx.completed {
        rows.push(field_w(
            crate::i18n::t("State"),
            crate::i18n::t("interrupted — the log carries no close"),
            theme::RED,
            L,
        ));
    }

    // The verdict before the evidence: on a five-hundred-package upgrade, what
    // matters is knowing whether the rollback is possible, not scrolling the
    // list to the bottom to find out.
    let missing = rollbacks.iter().filter(|r| !r.is_possible()).count();
    let recoverable = rollbacks.len() - missing;
    rows.push(Line::raw(""));
    rows.push(section(t("Rollback")));
    if rollbacks.is_empty() {
        rows.push(Line::from(Span::styled(
            format!("   {}", t("Nothing to undo: this transaction only reinstalled.")),
            Style::default().fg(theme::DIM),
        )));
    } else if recoverable == 0 {
        rows.push(Line::from(Span::styled(
            format!(
                "   {}",
                t("Impossible: none of the versions involved is still in cache.")
            ),
            Style::default().fg(theme::RED),
        )));
    } else {
        let (text, colour) = if missing == 0 {
            (
                format!(
                    "   {}",
                    tf(
                        "{0} package(s) restorable — a complete rollback is possible.",
                        &[&recoverable.to_string()]
                    )
                ),
                theme::GREEN,
            )
        } else {
            (
                format!(
                    "   {}",
                    tf(
                        "{0} restorable of {1} — {2} out of cache, left as they are.",
                        &[
                            &recoverable.to_string(),
                            &rollbacks.len().to_string(),
                            &missing.to_string()
                        ]
                    )
                ),
                theme::YELLOW,
            )
        };
        rows.push(Line::from(Span::styled(text, Style::default().fg(colour))));
        rows.push(Line::from(vec![
            Span::styled(
                "   u",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", t("build the inverse transaction")),
                Style::default().fg(theme::DIM),
            ),
        ]));
    }

    if !tx.warnings.is_empty() {
        rows.push(Line::raw(""));
        rows.push(section(t("pacman warnings")));
        for a in &tx.warnings {
            for chunk in wrap_indent(a, width, "   ") {
                rows.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(theme::YELLOW),
                )));
            }
        }
    }

    rows.push(Line::raw(""));
    rows.push(section(&tf("Operations · {0}", &[&tx.summary()])));

    // An operation whose version is no longer cached is flagged in place: it is
    // what will keep the rollback from being complete.
    let not_cached: std::collections::HashSet<&str> = rollbacks
        .iter()
        .filter(|r| !r.is_possible())
        .map(|r| r.name.as_str())
        .collect();
    const NAME_W: usize = 24;
    const MARK_W: usize = 12;
    for op in &tx.operations {
        let colour = act_color(op.act);
        let versions = match (&op.before, &op.after) {
            (Some(a), Some(b)) => format!("{a} → {b}"),
            (None, Some(b)) => b.clone(),
            (Some(a), None) => a.clone(),
            (None, None) => String::new(),
        };
        let absent = not_cached.contains(op.name.as_str());
        // The remaining room is computed, not guessed: without that a long
        // version gets cut by the panel edge and the "out of cache" mark that
        // follows disappears — that is, the most useful information.
        let room = width.saturating_sub(3 + NAME_W + 1 + if absent { MARK_W } else { 0 });
        let mut spans = vec![
            Span::styled(format!(" {} ", op.act.symbol()), Style::default().fg(colour)),
            Span::styled(
                format!("{:<NAME_W$}", truncate(&op.name, NAME_W)),
                Style::default().fg(theme::FG),
            ),
            Span::styled(truncate(&versions, room), Style::default().fg(theme::DIM)),
        ];
        if absent {
            spans.push(Span::styled(
                format!("  {}", t("out of cache")),
                Style::default().fg(theme::RED),
            ));
        }
        rows.push(Line::from(spans));
    }

    // Scrolling is bounded by the overflow: without that, one keypress too many
    // would scroll the panel into the void.
    let height = zone.height.saturating_sub(2) as usize;
    let overflow = rows.len().saturating_sub(height) as u16;
    let offset = app.detail_scroll.min(overflow);
    let title = if overflow > 0 {
        format!(
            " {} ",
            tf(
                "Detail · lines {0}-{1} of {2}",
                &[
                    &(offset as usize + 1).to_string(),
                    &(offset as usize + height).min(rows.len()).to_string(),
                    &rows.len().to_string()
                ]
            )
        )
    } else {
        format!(" {} ", t("Transaction detail"))
    };
    let p = Paragraph::new(rows).block(framed(&title)).scroll((offset, 0));
    f.render_widget(p, zone);
}

fn act_color(a: crate::history::Act) -> Color {
    use crate::history::Act;
    match a {
        Act::Upgraded => theme::CYAN,
        Act::Installed => theme::GREEN,
        Act::Downgraded => theme::YELLOW,
        Act::Removed => theme::RED,
        Act::Reinstalled => theme::DIM,
    }
}


/// Status line: an ongoing search, a transient message, or a summary of the
/// selection. It never carries shortcuts — those have a line of their own.
fn status_line(f: &mut Frame, app: &App, zone: Rect) {
    if app.search_mode {
        let p = Paragraph::new(Line::from(vec![
            Span::styled(" /", Style::default().fg(theme::ACCENT)),
            Span::styled(app.filter.clone(), Style::default().fg(theme::FG)),
            Span::styled("▏", Style::default().fg(theme::ACCENT)),
            Span::styled(
                format!("   {}", t("Enter confirms · Esc cancels")),
                Style::default().fg(theme::DIM),
            ),
        ]));
        f.render_widget(p, zone);
        return;
    }

    if let Some((msg, severity)) = &app.message {
        let colour = match severity {
            Severity::Info => theme::CYAN,
            Severity::Success => theme::GREEN,
            Severity::Warning => theme::YELLOW,
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {msg}"),
                Style::default().fg(colour),
            ))),
            zone,
        );
        return;
    }

    if let Some(summary) = app.selection_summary() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● ", Style::default().fg(theme::GREEN)),
                Span::styled(
                    summary,
                    Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                ),
            ])),
            zone,
        );
        return;
    }

    // The history reaches as far back as pacman's log: saying so avoids the
    // belief that nalarch only recorded its own transactions.
    if app.current_tab() == Tab::History {
        // The date alone is enough here: the time of the system's very first
        // transaction teaches nothing.
        let since = app
            .history
            .last()
            .and_then(|t| t.timestamp.get(..10).map(|d| d.to_string()))
            .unwrap_or_default();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● ", Style::default().fg(theme::ACCENT)),
                Span::styled(
                    tf(
                        "{0} transactions since {1} — everything that went through pacman, nalarch or not",
                        &[&app.history.len().to_string(), &since],
                    ),
                    Style::default().fg(theme::DIM),
                ),
            ])),
            zone,
        );
        return;
    }

    // Nothing selected: the frozen packages are recalled here, otherwise the
    // line would stay empty and that information would appear nowhere.
    if app.current_tab() == Tab::Updates && !app.state.ignored.is_empty() {
        let names: Vec<&str> = app.state.ignored.iter().map(|i| i.name.as_str()).collect();
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    " {}",
                    tf(
                        "{0} update(s) frozen by IgnorePkg: {1}",
                        &[&names.len().to_string(), &names.join(", ")]
                    )
                ),
                Style::default().fg(theme::YELLOW),
            ))),
            zone,
        );
    }
}

// ─────────────────────────── plan screen ───────────────────────────

/// Approval screen: what is about to happen, and what deserves a look.
///
/// This is nalarch's reason to exist. A package manager's output does contain
/// this information, but mixed into hundreds of log lines; one ends up reading
/// none of it and approving in hope. Here everything that matters fits on one
/// screen: how many packages and of what nature, what it costs, and the points
/// of attention in plain words.
fn plan_screen(f: &mut Frame, app: &mut App, zone: Rect) {
    let Some(intent) = &app.intent else {
        return;
    };

    // Width of the right column, known in advance: the risk text has to be
    // wrapped before knowing how many rows to reserve.
    const SUMMARY_W: u16 = 38;
    let risks_width = zone.width.saturating_sub(SUMMARY_W + 4) as usize;
    let risks = risk_lines(intent, risks_width);

    // The points of attention take precedence over the detail: they claim the
    // room they need, as long as a few packages remain checkable.
    let ceiling = zone.height.saturating_sub(11).max(8);
    let height = (risks.len() as u16 + 2)
        .max(summary_height(intent, app.state.ignored.len()))
        .min(ceiling);

    let [top, summary, detail, bottom] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(height),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(zone);

    plan_header(f, intent, top);

    // Summary on the left, attention points on the right: one reads the
    // figures first, then what qualifies them.
    let [left, right] =
        Layout::horizontal([Constraint::Length(SUMMARY_W), Constraint::Fill(1)]).areas(summary);
    summary_block(f, intent, &app.state.ignored, left);

    let clipped = risks.len() as u16 > height.saturating_sub(2);
    let title = if clipped {
        format!(" {} ", t("Points of attention · PgUp/PgDn to scroll"))
    } else {
        format!(" {} ", t("Points of attention"))
    };
    f.render_widget(
        Paragraph::new(risks)
            .scroll((app.risks_scroll, 0))
            .block(framed(&title)),
        right,
    );

    detail_list(f, app, detail);

    let help = Line::from(vec![
        Span::styled(format!(" {} ", t("Enter")), theme::badge(theme::GREEN)),
        Span::styled(format!(" {}   ", t("launch")), Style::default().fg(theme::FG)),
        Span::styled(format!(" {} ", t("Esc")), theme::badge(theme::DIM)),
        Span::styled(format!(" {}   ", t("cancel")), Style::default().fg(theme::FG)),
        Span::styled(t("↑↓ walk the detail"), Style::default().fg(theme::DIM)),
    ]);
    f.render_widget(Paragraph::new(help), bottom);
}

fn plan_header(f: &mut Frame, intent: &crate::app::Intent, zone: Rect) {
    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(" {}", intent.title),
            Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(format!(" {} ", t("command:")), Style::default().fg(theme::DIM)),
            Span::styled(
                intent
                    .display_command
                    .clone()
                    .unwrap_or_else(|| intent.cmd.join(" ")),
                Style::default().fg(theme::CYAN),
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT)),
    );
    f.render_widget(header, zone);
}

/// Counts by kind of operation, then cost. Empty categories stay on screen at
/// zero: knowing that *nothing* will be removed is information.
fn summary_block(
    f: &mut Frame,
    intent: &crate::app::Intent,
    app_ignores: &[crate::data::Ignore],
    zone: Rect,
) {
    let p = &intent.plan;
    let mut l = vec![Line::raw("")];

    // Every kind present is listed, plus those that structure the operation
    // even at zero: knowing that *nothing* will be removed counts as much as
    // the rest.
    let rollback = p.count(Kind::Downgrade) > 0 && p.count(Kind::Upgrade) == 0;
    // An install has nothing to say about updates, and an upgrade nothing about
    // what was "requested": each shows the categories that structure it, so a
    // zero on screen means something rather than filling a line.
    let always: &[Kind] = if rollback {
        &[Kind::Downgrade, Kind::Removal]
    } else if intent.removal {
        &[Kind::Removal, Kind::AutoRemoval]
    } else if p.count(Kind::Requested) > 0 {
        &[Kind::Requested, Kind::New]
    } else {
        &[Kind::Upgrade, Kind::New]
    };
    for g in Kind::ALL {
        let n = p.count(g);
        if n == 0 && !always.contains(&g) {
            continue;
        }
        let colour = kind_color(g);
        l.push(Line::from(vec![
            Span::styled(format!("  {}  ", g.symbol()), Style::default().fg(colour)),
            Span::styled(
                format!("{:<22}", g.label()),
                Style::default().fg(if n > 0 { theme::FG } else { theme::DIM }),
            ),
            Span::styled(
                format!("{n:>3}"),
                Style::default()
                    .fg(if n > 0 { colour } else { theme::DIM })
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    // Frozen packages are not part of the transaction, but they explain why an
    // expected update is missing from it. On a rollback, the IgnorePkg list
    // speaks of pending updates: it has nothing to do with the transaction
    // being shown.
    if !intent.removal && !rollback && !app_ignores.is_empty() {
        l.push(Line::from(vec![
            Span::styled("  ⊘  ", Style::default().fg(theme::YELLOW)),
            Span::styled(
                format!("{:<22}", t("Frozen (IgnorePkg)")),
                Style::default().fg(theme::FG),
            ),
            Span::styled(
                format!("{:>3}", app_ignores.len()),
                Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if p.aur_count > 0 {
        l.push(Line::from(vec![
            Span::styled("  ⚙  ", Style::default().fg(theme::MAGENTA)),
            Span::styled(
                format!("{:<22}", t("of them built (AUR)")),
                Style::default().fg(theme::FG),
            ),
            Span::styled(
                format!("{:>3}", p.aur_count),
                Style::default().fg(theme::MAGENTA).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    l.push(Line::raw(""));
    if p.sizes_known() {
        if !intent.removal {
            l.push(cost_line(
                "⤓",
                t("To download"),
                &human_size(p.total_dl),
                theme::CYAN,
            ));
        }
        l.push(cost_line(
            "⛁",
            if intent.removal {
                t("Space freed")
            } else {
                t("Disk space")
            },
            &human_size(p.net),
            if p.net < 0 { theme::GREEN } else { theme::YELLOW },
        ));
        if p.unknown > 0 {
            l.push(Line::from(Span::styled(
                format!("     {}", tf("{0} package(s) cannot be sized", &[&p.unknown.to_string()])),
                Style::default().fg(theme::DIM),
            )));
        }
    } else {
        // Nothing to size, but not for the same reason: the AUR gets built, a
        // rollback draws on packages that are already there.
        l.push(Line::from(Span::styled(
            if rollback {
                format!("  {}", t("Already cached, nothing to download"))
            } else {
                format!("     {}", t("Sizes unknown: built from source"))
            },
            Style::default().fg(theme::MAGENTA),
        )));
    }

    f.render_widget(
        Paragraph::new(l).block(framed(&format!(" {} ", t("Summary")))),
        zone,
    );
}

fn cost_line(icon: &str, label: &str, value: &str, colour: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {icon}  "), Style::default().fg(colour)),
        Span::styled(format!("{label:<18}"), Style::default().fg(theme::DIM)),
        Span::styled(
            format!("{value:>10}"),
            Style::default().fg(colour).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Wraps text to the wanted width, indenting the continuation rows.
///
/// ratatui's own wrapping aligns continuations on the margin, which visually
/// detaches a detail from its title: one can no longer tell which explains
/// which.
fn wrap_indent(text: &str, width: usize, indent: &str) -> Vec<String> {
    let margin = indent.chars().count();
    let usable = width.saturating_sub(margin).max(20);
    let mut rows = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > usable {
            rows.push(format!("{indent}{current}"));
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        rows.push(format!("{indent}{current}"));
    }
    rows
}

/// Points of attention, from the most severe to the most harmless.
fn risk_lines(intent: &crate::app::Intent, width: usize) -> Vec<Line<'static>> {
    if intent.risks.is_empty() {
        return vec![Line::from(Span::styled(
            format!(" {}", t("Nothing in particular to report on this transaction.")),
            Style::default().fg(theme::GREEN),
        ))];
    }

    let mut l = Vec::new();
    for r in &intent.risks {
        let (icon, colour) = match r.level {
            Level::Serious => ("▲", theme::RED),
            Level::Caution => ("▲", theme::YELLOW),
            Level::Info => ("•", theme::CYAN),
        };
        for (i, chunk) in wrap_indent(&r.title, width, "   ").into_iter().enumerate() {
            l.push(if i == 0 {
                Line::from(vec![
                    Span::styled(
                        format!(" {icon} "),
                        Style::default().fg(colour).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        chunk.trim_start().to_string(),
                        Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                Line::from(Span::styled(
                    chunk,
                    Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                ))
            });
        }
        for chunk in wrap_indent(&r.detail, width, "   ") {
            l.push(Line::from(Span::styled(
                chunk,
                Style::default().fg(theme::DIM),
            )));
        }
        l.push(Line::raw(""));
    }
    l.pop();
    l
}

/// One colour per kind of operation, defined once.
fn kind_color(k: Kind) -> Color {
    match k {
        Kind::Upgrade => theme::GREEN,
        Kind::Requested => theme::GREEN,
        Kind::New => theme::CYAN,
        Kind::Downgrade => theme::YELLOW,
        Kind::Removal => theme::RED,
        Kind::AutoRemoval => theme::MAGENTA,
    }
}

/// Per-package detail. Deliberately below the summary: it is a check, not what
/// one reads first.
fn detail_list(f: &mut Frame, app: &mut App, zone: Rect) {
    let Some(intent) = &app.intent else { return };
    let p = &intent.plan;

    if p.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            t("No per-package detail for this action."),
            Style::default().fg(theme::DIM),
        )))
        .alignment(Alignment::Center)
        .block(framed(&format!(" {} ", t("Detail"))));
        f.render_widget(msg, zone);
        return;
    }

    let name_width = ((zone.width as usize).saturating_sub(62)).clamp(14, 36);
    let items: Vec<ListItem> = p
        .rows
        .iter()
        .map(|l| {
            let colour = kind_color(l.kind);
            let mut spans = vec![
                Span::styled(
                    format!(" {} ", l.kind.symbol()),
                    Style::default().fg(colour).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<10}", truncate(&l.repo, 10)),
                    Style::default().fg(theme::repo_color(&l.repo)),
                ),
                Span::styled(
                    format!("{:<name_width$} ", truncate(&l.name, name_width)),
                    Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                ),
            ];
            // A new package has no previous version: showing an arrow starting
            // from nothing would be an artifice.
            if l.to_version.is_empty() {
                spans.push(Span::styled(
                    format!("{:<32} ", truncate(&l.from_version, 32)),
                    Style::default().fg(theme::DIM),
                ));
            } else if l.from_version.is_empty() {
                spans.push(Span::styled(
                    format!("{:<32} ", truncate(&l.to_version, 32)),
                    Style::default().fg(theme::CYAN),
                ));
            } else {
                spans.push(Span::styled(
                    format!("{:>14} ", truncate(&l.from_version, 14)),
                    Style::default().fg(theme::DIM),
                ));
                spans.push(Span::styled(
                    if l.is_downgrade { "↓ " } else { "→ " },
                    Style::default().fg(if l.is_downgrade {
                        theme::YELLOW
                    } else {
                        theme::ACCENT
                    }),
                ));
                spans.push(Span::styled(
                    format!("{:<15} ", truncate(&l.to_version, 15)),
                    Style::default().fg(colour),
                ));
            }
            spans.push(Span::styled(
                match l.dl {
                    Some(d) if d > 0 => format!("{:>10}", human_size(d)),
                    Some(_) => format!("{:>10}", t("cached")),
                    None => format!("{:>10}", "—"),
                },
                Style::default().fg(theme::DIM),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = format!(" {} ", tf("Detail · {0} package(s)", &[&p.rows.len().to_string()]));
    let widget = List::new(items)
        .block(framed(&title))
        .highlight_style(theme::selected())
        .highlight_symbol("▌");
    f.render_stateful_widget(widget, zone, &mut app.plan_list);
}

/// Minimum height imposed by the left column.
fn summary_height(intent: &crate::app::Intent, frozen: usize) -> u16 {
    let p = &intent.plan;
    let rollback = p.count(Kind::Downgrade) > 0 && p.count(Kind::Upgrade) == 0;
    let always: &[Kind] = if rollback {
        &[Kind::Downgrade, Kind::Removal]
    } else if intent.removal {
        &[Kind::Removal, Kind::AutoRemoval]
    } else if p.count(Kind::Requested) > 0 {
        &[Kind::Requested, Kind::New]
    } else {
        &[Kind::Upgrade, Kind::New]
    };
    let categories = Kind::ALL
        .iter()
        .filter(|g| p.count(**g) > 0 || always.contains(g))
        .count() as u16;
    let frozen = u16::from(!intent.removal && frozen > 0);
    // borders + blank line + categories + frozen + AUR + separator + costs
    (2 + 1 + categories + frozen + u16::from(p.aur_count > 0) + 1 + 2 + u16::from(p.unknown > 0))
        .max(8)
}

// ─────────────────────────── run screen ───────────────────────────

/// Heights of the banners framing the pseudo terminal. They are constant so
/// that its size never varies during a run; `main` uses them to compute the
/// dimensions handed to the PTY.
///
/// The top banner holds three rows: the command, the current step, and the
/// plan's totals. The latter stay on screen for the whole run — it is precisely
/// while paru is downloading that one wants to know how much is left to pull.
pub const RUN_TOP: u16 = 5;
pub const RUN_BOTTOM: u16 = 3;
/// Line dedicated to the progress bar, between the banner and the terminal.
pub const RUN_BAR: u16 = 1;
/// Total rows the run screen takes outside the embedded terminal: the two
/// banners, the bar, and the frame's borders.
pub const RUN_CHROME: u16 = RUN_TOP + RUN_BAR + RUN_BOTTOM + 2;

/// Frames of the waiting spinner, shared by the bars. Braille turns smoothly
/// and occupies a single column, which avoids shifting the rest of the line on
/// every frame.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Formats a duration as h:mm:ss.
fn hms(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Numeric summary of the plan, condensed onto one line for the run banner.
fn totals_line(intent: &crate::app::Intent) -> Line<'static> {
    let p = &intent.plan;
    if p.is_empty() {
        return Line::from(vec![
            Span::styled(format!(" {:<8} ", t("plan:")), Style::default().fg(theme::DIM)),
            Span::styled(
                intent.notes.first().cloned().unwrap_or_default(),
                Style::default().fg(theme::FG),
            ),
        ]);
    }

    let mut spans = vec![
        Span::styled(format!(" {:<8} ", t("plan:")), Style::default().fg(theme::DIM)),
        Span::styled(
            tf("{0} package(s)", &[&p.rows.len().to_string()]),
            Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
        ),
    ];
    let mut add = |key: &str, value: String, colour| {
        spans.push(Span::styled(
            format!(" · {key} "),
            Style::default().fg(theme::DIM),
        ));
        spans.push(Span::styled(value, Style::default().fg(colour)));
    };
    if p.sizes_known() {
        if !intent.removal {
            add(t("download"), human_size(p.total_dl), theme::CYAN);
        }
        add(
            if intent.removal { t("freed") } else { t("net") },
            human_size(p.net),
            if p.net < 0 { theme::GREEN } else { theme::YELLOW },
        );
    } else {
        add(
            t("sizes"),
            t("unknown before building").into(),
            theme::MAGENTA,
        );
    }
    if p.aur_count > 0 {
        spans.push(Span::styled(
            format!(" · {} AUR", p.aur_count),
            Style::default().fg(theme::MAGENTA),
        ));
    }
    Line::from(spans)
}

/// The run screen.
///
/// Laid out in blocks like nala's: what is downloading, what is installing,
/// then what is left to know. Each block carries its own bar, because there is
/// no single measure of progress — downloading and installing are two separate
/// counts, and mixing them would give a number that means nothing.
///
/// paru's output is not displayed: it is parsed and rewritten here. It stays
/// available verbatim through `j`.
fn run_screen(f: &mut Frame, app: &mut App, zone: Rect) {
    let Some(session) = &app.session else { return };
    let j = session.journal();
    let prompt = session.prompt();

    // The download is only worth showing once it has started.
    let h_dl = if j.downloads.finished.is_empty() && j.downloads.speed.is_none() {
        0
    } else {
        4
    };
    let notes = note_lines(app, session);
    let h_notes = if notes.is_empty() {
        0
    } else {
        (notes.len() as u16 + 2).min(zone.height / 3)
    };

    let [top, dl, middle, notes_area, bottom] = Layout::vertical([
        Constraint::Length(RUN_TOP),
        Constraint::Length(h_dl),
        Constraint::Fill(1),
        Constraint::Length(h_notes),
        Constraint::Length(RUN_BOTTOM),
    ])
    .areas(zone);

    // The pty will be resized to this on the next loop iteration.
    app.pty_size.set((
        middle.height.saturating_sub(2),
        middle.width.saturating_sub(2),
    ));

    run_header(f, session, app, top);
    if h_dl > 0 {
        download_block(f, app, session, dl);
    }

    // `j` asks explicitly for the raw output; otherwise it is our transcript,
    // unless there is nothing to transcribe.
    if app.raw_visible {
        let program = session.command.split_whitespace().next().unwrap_or("output");
        // Without a marker, having scrolled back is indistinguishable from an
        // output that has simply stopped moving.
        let title = match session.scroll() {
            0 => format!(" {} ", tf("output of {0}", &[program])),
            n => format!(
                " {} ",
                tf(
                    "output of {0} · scrolled back {1} line(s) · End to return",
                    &[program, &n.to_string()]
                )
            ),
        };
        f.render_widget(
            PseudoTerminal::new(session.screen()).block(framed(&title)),
            middle,
        );
    } else {
        journal_block(f, app, session, middle);
    }

    if h_notes > 0 {
        f.render_widget(
            Paragraph::new(notes).block(framed(&format!(" {} ", t("Worth noting")))),
            notes_area,
        );
    }
    run_footer(f, session, prompt, app.raw_visible, bottom);
}

fn run_header(
    f: &mut Frame,
    session: &crate::exec::Session,
    app: &App,
    zone: Rect,
) {
    let (symbol, label, colour) = match session.exit_code {
        None => ("▶", t("running").to_string(), theme::ACCENT),
        Some(0) => ("✓", t("finished").to_string(), theme::GREEN),
        Some(c) if session.interrupted => (
            "✘",
            tf("interrupted (code {0})", &[&c.to_string()]),
            theme::RED,
        ),
        Some(c) => ("✘", tf("failed (code {0})", &[&c.to_string()]), theme::RED),
    };
    let mut rows = vec![
        Line::from(vec![
            Span::styled(format!(" {symbol} "), Style::default().fg(colour)),
            Span::styled(
                session.command.clone(),
                Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   {label}"),
                Style::default().fg(colour).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8} ", t("step:")), Style::default().fg(theme::DIM)),
            Span::styled(session.step_text(), Style::default().fg(theme::CYAN)),
        ]),
    ];
    if let Some(intent) = &app.intent {
        rows.push(totals_line(intent));
    }
    f.render_widget(
        Paragraph::new(rows).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colour)),
        ),
        zone,
    );
}

/// Download block: how many packages, which one has just finished, and where
/// the volume stands. Those are the numbers one looks for when the connection
/// drags.
fn download_block(
    f: &mut Frame,
    app: &App,
    session: &crate::exec::Session,
    zone: Rect,
) {
    let j = session.journal();
    let done = j.downloads.finished.len();
    // With no plan (demo, or an action with no detail) the denominator is 0:
    // showing "3/0" or "0.0 %" would be absurd. What was actually observed is
    // used instead.
    let expected = app
        .intent
        .as_ref()
        .map(|i| i.plan.rows.len())
        .filter(|n| *n > 0)
        .unwrap_or(done);
    let total_bytes = app
        .intent
        .as_ref()
        .map(|i| i.plan.total_dl)
        .filter(|t| *t > 0)
        .unwrap_or(j.downloads.bytes);

    let l1 = Line::from(vec![
        Span::styled(format!(" {:<10}", t("Packages")), Style::default().fg(theme::DIM)),
        Span::styled(
            format!("{done}/{expected}"),
            Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("     {:<10}", t("Latest")), Style::default().fg(theme::DIM)),
        Span::styled(
            truncate(
                if j.downloads.last.is_empty() {
                    "—"
                } else {
                    &j.downloads.last
                },
                46,
            ),
            Style::default().fg(theme::FG),
        ),
    ]);

    let fraction = if expected > 0 {
        (done as f64 / expected as f64).min(1.0)
    } else {
        0.0
    };
    let mut right = vec![
        Span::styled(
            format!("{:>5.1}%", fraction * 100.0),
            Style::default().fg(theme::GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " • {} / {}",
                human_size(j.downloads.bytes),
                human_size(total_bytes)
            ),
            Style::default().fg(theme::FG),
        ),
    ];
    let _ = &right;
    if let Some(v) = &j.downloads.speed {
        right.push(Span::styled(
            format!(" • {v}"),
            Style::default().fg(theme::CYAN),
        ));
    }
    let l2 = rail(" ⤓ ", theme::CYAN, fraction, zone.width, right);

    f.render_widget(
        Paragraph::new(vec![l1, l2]).block(framed(&format!(" {} ", t("Download")))),
        zone,
    );
}

/// Builds an "icon + rail + numbers" line.
fn rail(
    icon: &str,
    tint: Color,
    fraction: f64,
    width: u16,
    right: Vec<Span<'static>>,
) -> Line<'static> {
    let right_width: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let rail_width = (width as usize)
        .saturating_sub(right_width + icon.chars().count() + 6)
        .clamp(10, 90);
    let done = ((fraction * rail_width as f64).round() as usize).min(rail_width);

    let mut spans = vec![
        Span::styled(icon.to_string(), Style::default().fg(tint)),
        Span::styled("━".repeat(done), Style::default().fg(tint)),
        Span::styled(
            "━".repeat(rail_width - done),
            Style::default().fg(theme::DIM),
        ),
        Span::raw("  "),
    ];
    spans.extend(right);
    Line::from(spans)
}

/// The action log, rewritten in our own vocabulary, with the current phase's
/// bar at the foot of the block — like nala's "Running dpkg".
fn journal_block(f: &mut Frame, app: &App, session: &crate::exec::Session, zone: Rect) {
    let j = session.journal();
    // The journal only knows names; the versions come from the plan, so that
    // what was actually laid down can be checked afterwards.
    let version_of = |name: &str| -> String {
        let Some(i) = &app.intent else {
            return String::new();
        };
        match i.plan.rows.iter().find(|l| l.name == name) {
            Some(l) if l.to_version.is_empty() => l.from_version.clone(),
            Some(l) if l.from_version.is_empty() => l.to_version.clone(),
            Some(l) => format!("{} → {}", l.from_version, l.to_version),
            None => String::new(),
        }
    };
    let interieur = zone.height.saturating_sub(2) as usize;
    // One line is reserved for the phase bar.
    let room = interieur.saturating_sub(1);

    // The tail is followed while the reader has not taken over: the current
    // action is what one wants to see. Once they scroll back, the window stays
    // where they put it, because seventy-five operations do not fit on screen
    // and everything before the tail used to be unreachable without `j`.
    let overflow = j.events.len().saturating_sub(room);
    let started = match app.journal_anchor.get() {
        None => overflow,
        // Scrolled back down to the tail: following resumes on its own, which is
        // what one means by scrolling to the bottom.
        Some(a) if a >= overflow => {
            app.journal_anchor.set(None);
            overflow
        }
        Some(a) => a,
    };
    // Published so the next keypress starts from what is actually on screen.
    app.journal_start.set(started);
    let ended = (started + room).min(j.events.len());
    let mut rows: Vec<Line> = j.events[started..ended]
        .iter()
        .map(|e| {
            let teinte = action_color(e.action);
            let mut spans = vec![
                Span::styled(
                    format!(" {} ", e.action.symbol()),
                    Style::default().fg(teinte).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<12}", e.action.verb()),
                    Style::default().fg(teinte),
                ),
                Span::styled(
                    format!("{:<38}", truncate(&e.target, 38)),
                    Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                ),
            ];
            let extra = if e.detail.is_empty() {
                version_of(&e.target)
            } else {
                e.detail.clone()
            };
            if !extra.is_empty() {
                spans.push(Span::styled(
                    extra,
                    Style::default().fg(theme::DIM),
                ));
            }
            Line::from(spans)
        })
        .collect();

    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            format!(" {}", t("Waiting for the first operations…")),
            Style::default().fg(theme::DIM),
        )));
    }
    while rows.len() < room {
        rows.push(Line::raw(""));
    }

    // Bar of the current phase, at the foot of the block.
    let p = session.progress();
    let (fraction, counter) = match &p {
        Some(p) => (p.fraction, p.counter),
        None => (None, None),
    };
    let mut right = vec![Span::styled(
        match fraction {
            Some(fr) => format!("{:>5.1}%", fr * 100.0),
            None => "  ····".to_string(),
        },
        Style::default()
            .fg(if fraction.is_some() {
                theme::GREEN
            } else {
                theme::MAGENTA
            })
            .add_modifier(Modifier::BOLD),
    )];
    right.push(Span::styled(
        format!(" • {}", hms(session.duration())),
        Style::default().fg(theme::FG),
    ));
    if let Some((n, m)) = counter {
        right.push(Span::styled(
            format!(" • {n}/{m}"),
            Style::default().fg(theme::CYAN),
        ));
    }
    let icon = match session.exit_code {
        None => format!(
            " {} ",
            SPINNER[(session.duration().as_millis() / 90) as usize % SPINNER.len()]
        ),
        Some(0) => " ✔ ".to_string(),
        Some(_) => " ✘ ".to_string(),
    };
    let teinte = match session.exit_code {
        None => theme::ACCENT,
        Some(0) => theme::GREEN,
        Some(_) => theme::RED,
    };
    rows.push(rail(
        &icon,
        teinte,
        fraction.unwrap_or(0.0),
        zone.width,
        right,
    ));

    // The block carries its function, not the phase: that already appears in
    // the header and in the download block, and repeating it there gave two
    // frames with the same title.
    // Scrolled back, the title says where: without it, a frozen list is
    // indistinguishable from an operation that has stopped producing events.
    let title = if started < overflow {
        format!(
            " {} ",
            tf(
                "Operations · {0}-{1} of {2} · End to follow again",
                &[
                    &(started + 1).to_string(),
                    &ended.to_string(),
                    &j.events.len().to_string()
                ]
            )
        )
    } else {
        format!(" {} ", tf("Operations · {0}", &[&j.events.len().to_string()]))
    };
    f.render_widget(Paragraph::new(rows).block(framed(&title)), zone);
}

fn action_color(a: crate::journal::Action) -> Color {
    use crate::journal::Action as A;
    match a {
        A::Downloaded => theme::CYAN,
        A::Verified => theme::DIM,
        A::Installed => theme::CYAN,
        A::Upgraded => theme::GREEN,
        A::Downgraded => theme::YELLOW,
        A::Reinstalled => theme::ACCENT,
        A::Removed => theme::RED,
        A::Hook => theme::MAGENTA,
        A::Built => theme::MAGENTA,
    }
}

/// What is left to know once the operation has passed: the things that call
/// for an action on your side and that the output drowns.
fn note_lines(app: &App, session: &crate::exec::Session) -> Vec<Line<'static>> {
    let j = session.journal();
    let mut l = Vec::new();

    for e in &j.errors {
        l.push(Line::from(vec![
            Span::styled(" ✘ ", Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)),
            Span::styled(e.clone(), Style::default().fg(theme::RED)),
        ]));
    }

    if !j.pacnew.is_empty() {
        l.push(Line::from(vec![
            Span::styled(" ▲ ", Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(
                tf(
                    "{0} configuration file(s) to merge: {1}",
                    &[&j.pacnew.len().to_string(), &j.pacnew.join(", ")],
                ),
                Style::default().fg(theme::FG),
            ),
        ]));
        l.push(Line::from(Span::styled(
            format!("   {}", t("Your version was kept; the new one waits beside it, unapplied.")),
            Style::default().fg(theme::DIM),
        )));
        l.push(Line::from(vec![
            Span::styled(format!("   {} ", t("Compare and merge:")), Style::default().fg(theme::DIM)),
            Span::styled(
                "sudo pacdiff -s",
                Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   {}", t("(pacman-contrib) — without it, the new configuration never applies.")),
                Style::default().fg(theme::DIM),
            ),
        ]));
    }

    // The reboot is inferred from the plan, not from the output: pacman does not say so.
    if session.exit_code == Some(0) {
        if let Some(intent) = &app.intent {
            let needed = crate::risks::needs_reboot(&intent.plan);
            if !needed.is_empty() {
                l.push(Line::from(vec![
                    Span::styled(" ▲ ", Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        tf("Reboot required: {0}", &[&needed.join(", ")]),
                        Style::default().fg(theme::FG),
                    ),
                ]));
            }
        }
    }

    for a in j.warnings.iter().take(4) {
        l.push(Line::from(vec![
            Span::styled(" • ", Style::default().fg(theme::YELLOW)),
            Span::styled(a.clone(), Style::default().fg(theme::DIM)),
        ]));
    }
    l
}

fn run_footer(
    f: &mut Frame,
    session: &crate::exec::Session,
    prompt: Option<crate::exec::Prompt>,
    raw_visible: bool,
    zone: Rect,
) {
    if let Some(question) = prompt {
        let rows = vec![
            Line::from(vec![
                Span::styled(format!(" {} ", t("input expected")), theme::badge(theme::YELLOW)),
                Span::styled(format!("  {}", question.text), Style::default().fg(theme::FG)),
            ]),
            Line::from(Span::styled(
                if question.masked {
                    format!(" {}", t("Type your password then Enter — nothing shows, that is normal."))
                } else {
                    format!(" {}", t("Type your answer then Enter · j shows the raw output · Ctrl-C interrupts"))
                },
                Style::default().fg(theme::DIM),
            )),
        ];
        f.render_widget(
            Paragraph::new(rows).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme::YELLOW)),
            ),
            zone,
        );
        return;
    }

    // The hint names the view one would switch *to*, not the one already on
    // screen: offering "paru's raw output" while it is showing said nothing.
    let toggle = if raw_visible {
        t("j back to the transcript")
    } else {
        t("j paru's raw output")
    };
    let (text, tint) = match session.exit_code {
        None => (
            format!(
                " {} · {}",
                toggle,
                t("↑↓ scroll · every other key is forwarded to paru")
            ),
            theme::DIM,
        ),
        Some(0) => (
            format!(
                " {} · {} · {}",
                t("Finished"),
                toggle,
                t("↑↓ PgUp PgDn scroll · Enter to return")
            ),
            theme::GREEN,
        ),
        Some(c) => (
            format!(
                " {}",
                format!(
                    "{0} · {1} · {2}",
                    if session.interrupted {
                        t("Interrupted").to_string()
                    } else {
                        tf("Failed (code {0})", &[&c.to_string()])
                    },
                    toggle,
                    t("↑↓ PgUp PgDn scroll · Enter back to the table")
                )
            ),
            theme::RED,
        ),
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(tint),
        ))),
        zone,
    );
}

/// What an update changes, before even downloading it.
///
/// Two complementary sources. Arch's packaging log says *why* the package
/// moved: a plain rebuild against a library adds no feature, and that is often
/// the explanation for an update that looked gratuitous. The upstream notes say
/// what the software itself changes.
fn changelog_screen(f: &mut Frame, app: &mut App, zone: Rect) {
    let Some(c) = &app.changelog else { return };

    let [top, middle, bottom] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(zone);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                c.package.clone(),
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(c.from_version.clone(), Style::default().fg(theme::DIM)),
            Span::styled(" → ", Style::default().fg(theme::ACCENT)),
            Span::styled(
                c.to_version.clone(),
                Style::default().fg(theme::GREEN).add_modifier(Modifier::BOLD),
            ),
        ]),
        match &c.state {
            crate::changelog::State::Loading => Line::from(Span::styled(
                format!(" {}", t("fetching the packaging log and the release notes…")),
                Style::default().fg(theme::DIM),
            )),
            crate::changelog::State::Ready(contents) => Line::from(vec![
                Span::styled(format!(" {} ", t("upstream:")), Style::default().fg(theme::DIM)),
                Span::styled(
                    contents.url.clone().unwrap_or_else(|| t("unknown").into()),
                    Style::default().fg(theme::CYAN),
                ),
            ]),
        },
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT)),
    );
    f.render_widget(header, top);

    let installed_version = c.from_version.clone();
    let mut rows: Vec<Line> = Vec::new();
    match &c.state {
        crate::changelog::State::Loading => {
            rows.push(Line::from(Span::styled(
                format!(" {}", t("Loading…")),
                Style::default().fg(theme::DIM),
            )));
        }
        crate::changelog::State::Ready(contents) => {
            if !contents.packaging.is_empty() {
                rows.push(section(t("Arch packaging log")));
                // The log is sorted newest first. Everything above the installed
                // version is what the update brings; the rest is already on the
                // machine and only serves as context. Without that distinction
                // one reads a list of commits with no idea which ones bear on
                // the decision at hand.
                let already = contents
                    .packaging
                    .iter()
                    .position(|c| c.title.contains(&installed_version));
                for (i, commit) in contents.packaging.iter().enumerate() {
                    let brings = already.is_none_or(|d| i < d);
                    // The `upgpkg:` prefix is convention noise.
                    let title = commit.title.trim_start_matches("upgpkg:").trim();
                    rows.push(Line::from(vec![
                        Span::styled(
                            if brings { "  ▸ " } else { "    " },
                            Style::default().fg(theme::GREEN),
                        ),
                        Span::styled(
                            format!("{}  ", commit.date),
                            Style::default().fg(theme::DIM),
                        ),
                        Span::styled(
                            title.to_string(),
                            if brings {
                                Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(theme::DIM)
                            },
                        ),
                    ]));
                    if already == Some(i) {
                        rows.push(Line::from(Span::styled(
                            format!("    ─── {} ───", t("installed version")),
                            Style::default().fg(theme::DIM),
                        )));
                    }
                }
                rows.push(Line::raw(""));
            }

            if !contents.upstream.is_empty() {
                rows.push(section(&format!(
                    "{}{}",
                    t("Upstream release notes"),
                    contents
                        .upstream_tag
                        .as_ref()
                        .map(|tag| format!(" · {tag}"))
                        .unwrap_or_default()
                )));
                for l in &contents.upstream {
                    rows.push(markdown_line(l));
                }
                rows.push(Line::raw(""));
            }

            for m in &contents.gaps {
                rows.push(Line::from(vec![
                    Span::styled(" • ", Style::default().fg(theme::YELLOW)),
                    Span::styled(m.clone(), Style::default().fg(theme::DIM)),
                ]));
            }
            if rows.is_empty() {
                rows.push(Line::from(Span::styled(
                    format!(" {}", t("Nothing published for this version.")),
                    Style::default().fg(theme::DIM),
                )));
            }
        }
    }

    f.render_widget(
        Paragraph::new(rows)
            .scroll((app.changelog_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(framed(&format!(" {} ", t("Changes")))),
        middle,
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", t("Esc")), theme::badge(theme::DIM)),
            Span::styled(format!(" {}   ", t("back")), Style::default().fg(theme::FG)),
            Span::styled(t("↑↓ PgUp/PgDn scroll"), Style::default().fg(theme::DIM)),
        ])),
        bottom,
    );
}

fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {title} "),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    ))
}

/// Light Markdown rendering for the release notes: headings and bullets are
/// enough to make the text readable, a full implementation would be off topic.
fn markdown_line(l: &str) -> Line<'static> {
    let body = l.trim_start();
    if let Some(title) = body.strip_prefix("## ").or_else(|| body.strip_prefix("# ")) {
        return Line::from(Span::styled(
            format!("  {title}"),
            Style::default().fg(theme::GREEN).add_modifier(Modifier::BOLD),
        ));
    }
    let indent = l.len() - body.len();
    if let Some(rest) = body.strip_prefix("* ").or_else(|| body.strip_prefix("- ")) {
        return Line::from(vec![
            Span::styled(
                format!("  {}• ", " ".repeat(indent)),
                Style::default().fg(theme::CYAN),
            ),
            Span::styled(rest.to_string(), Style::default().fg(theme::FG)),
        ]);
    }
    Line::from(Span::styled(
        format!("  {l}"),
        Style::default().fg(theme::FG),
    ))
}

fn legend_line(f: &mut Frame, app: &App, zone: Rect) {
    let action = t(match app.current_tab() {
        Tab::Updates => "u update",
        Tab::Installed | Tab::Orphans => "u remove",
        Tab::History => "u roll back",
        Tab::Search => "u install",
        Tab::Cache => "u clean",
    });

    // The history is not checkable: a transaction is undone whole or not at
    // all. Showing "space check" there would be an empty promise.
    let keys: &[(&str, &str)] = if app.current_tab() == Tab::Search {
        &[
            ("↑↓", "navigate"),
            ("←→", "tab"),
            ("/", "query"),
            ("space", "check"),
            ("n", "none"),
            ("r", "reload"),
        ]
    } else if app.current_tab() == Tab::History {
        &[
            ("↑↓", "navigate"),
            ("←→", "tab"),
            ("/", "filter by package"),
            ("r", "reload"),
        ]
    } else {
        &[
            ("↑↓", "navigate"),
            ("←→", "tab"),
            ("space", "check"),
            ("a/n", "all/none"),
            ("p", "protect"),
            ("c", "changes"),
            ("/", "filter"),
            ("r", "reload"),
        ]
    };

    let mut spans = vec![];
    for (key, label) in keys.iter().copied() {
        // The key name goes through the table too: `space` is a word, not a
        // symbol, and stayed English next to its translated label. Actual
        // letter keys (`p`, `c`, `r`) have no entry and pass through untouched.
        spans.push(Span::styled(
            format!(" {} ", t(key)),
            Style::default().fg(theme::ACCENT),
        ));
        spans.push(Span::styled(t(label), Style::default().fg(theme::DIM)));
    }
    spans.push(Span::styled(
        format!("  {action}"),
        Style::default().fg(theme::GREEN).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(format!("  q {}", t("quit")), Style::default().fg(theme::DIM)));

    f.render_widget(Paragraph::new(Line::from(spans)), zone);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrapping must indent the continuation rows, otherwise a detail detaches
    /// visually from the title it explains.
    #[test]
    fn wrapping_indents_the_following_rows() {
        let l = wrap_indent("one two three four five six", 24, "   ");
        assert!(l.len() > 1);
        assert!(l[0].starts_with("   "));
        assert!(l[1].starts_with("   "));
        assert!(l.iter().all(|x| x.chars().count() <= 24));
    }

    #[test]
    fn truncation_never_cuts_a_character_in_half() {
        assert_eq!(truncate("écran", 3), "éc…");
        assert_eq!(truncate("court", 20), "court");
    }
}
