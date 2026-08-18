//! nalarch — a TUI package manager for Arch.
//!
//! Architecture: libalpm for reading, paru for writing. No dependency
//! resolution and no AUR build logic is reimplemented here; the actions are
//! delegated to paru, which stays the sole owner of the transaction.

mod app;
mod changelog;
mod data;
mod demo;
mod exec;
mod history;
mod i18n;
mod icons;
mod journal;
mod plan;
mod risks;
mod search;
mod theme;
mod ui;

use anyhow::Result;
use app::{App, Mode};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // The interface language is settled before anything can print: `t()` reads
    // it through a OnceLock, and a first call made before this one would pin
    // English for the whole run.
    i18n::init(&args);
    icons::init(&args);

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    // Off-terminal rendering: used to check the layout without a TTY.
    // Usage: nalarch --dump [screen] [width] [height]
    if args.iter().any(|a| a == "--dump") {
        return dump(&args);
    }

    let mut app = App::new_app()?;

    // Demo: replays a paru-like session without installing anything. Used to
    // exercise the run screen when the system is already up to date and no real
    // transaction is available.
    if args.iter().any(|a| a == "--demo") {
        let plan = demo::plan();
        // The synthetic plan goes through the real analyser rather than an empty
        // list. An attention panel reading "nothing in particular" on the one
        // screen that justifies the tool misrepresents it — and the analyser has
        // real things to say here, starting with the new dependency the plan
        // drags in and whatever IgnorePkg is holding back on this machine.
        let risks = risks::analyze(&plan, &app.state, &[], false);
        app.intent = Some(app::Intent {
            display_command: None,
            title: crate::i18n::t("Demo — the system is not modified").into(),
            cmd: demo::command()?,
            plan,
            risks,
            removal: false,
            notes: vec![
                crate::i18n::t("Replayed output: no pacman command is invoked.").into(),
            ],
        });
        app.mode = Mode::Plan;
    }

    let mut terminal = ratatui::init();

    let result = run_loop(&mut terminal, &mut app);

    ratatui::restore();
    result
}

/// Renders the interface into an in-memory buffer and writes it as plain text.
/// Usage summary. Deliberately terse: nalarch explains itself on screen, and a
/// wall of text in the terminal would only delay getting there.
fn print_help() {
    println!("nalarch — {}", i18n::t("a TUI package manager for Arch"));
    println!();
    println!("  nalarch                {}", i18n::t("start the interface"));
    println!("  nalarch --lang en|fr   {}", i18n::t("force the interface language"));
    println!("  nalarch --icons        {}", i18n::t("draw Nerd Font glyphs (needs one)"));
    println!("  nalarch --demo         {}", i18n::t("replay a session without touching the system"));
    println!("  nalarch --dump N W H   {}", i18n::t("render one screen as plain text (no TTY)"));
    println!("  nalarch --help         {}", i18n::t("this message"));
}

fn dump(args: &[String]) -> Result<()> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let nums: Vec<usize> = args
        .iter()
        .skip_while(|a| *a != "--dump")
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let tab = nums.first().copied().unwrap_or(0);
    let width = nums.get(1).copied().unwrap_or(120) as u16;
    let height = nums.get(2).copied().unwrap_or(30) as u16;

    let mut app = App::new_app()?;

    match tab {
        // 4: plan screen, filled as if everything were checked in the updates
        // tab.
        4 => {
            app.tab = 0;
            app.check_all();
            app.message = None;
            app.apply();
        }
        // 5: run screen. A read-only command exercises the whole pty → vt100 →
        // render chain without touching the system.
        5 => {
            // A real plan, so the banner shows realistic totals…
            app.tab = 0;
            app.check_all();
            app.message = None;
            app.apply();
            // …but a harmless command: nothing is upgraded just for a render.
            if let Some(i) = app.intent.as_mut() {
                i.cmd = vec!["pacman".into(), "-Qi".into(), "pacman".into()];
            }
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            // Let the process produce its output before taking the snapshot.
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 6: table with everything checked, to verify that the status line and
        // the legend coexist instead of replacing one another.
        6 => {
            app.tab = 0;
            app.check_all();
            app.message = None;
        }
        // 7: run screen in progress, fed by a realistic progress stream. The
        // `sleep` keeps the process alive long enough to render.
        7..=9 => {
            // 9: a failed run, to check that the footer no longer says
            // "Finished" after an interruption or an error.
            let tail = if tab == 9 { "exit 3" } else { "sleep 5" };
            let stream = if tab == 8 {
                // AUR build: no measurable progress.
                r":: Making and installing package...\n==> Making package: infisical-bin\n"
            } else {
                // Measurable progress: a pacman phase and a counter.
                r":: Processing package changes...\n(3/6) upgrading spotify [####----] 50%%\n"
            };
            // A synthetic intent: these modes must render the bar even when the
            // system is up to date and no plan is available.
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Demo").into(),
                cmd: vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf '{stream}'; {tail}"),
                ],
                plan: plan::empty(),
                risks: Vec::new(),
                removal: false,
                notes: vec![crate::i18n::t("demo render").into()],
            });
            app.mode = app::Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 10: a fabricated plan gathering several kinds of risk, to check how
        // the attention block renders without waiting for such a transaction to
        // turn up. The risks go through the real analyser.
        10 => {
            use plan::{Kind, PlanRow};
            let line = |name: &str, repo: &str, anc: &str, nouv: &str, kind, aur, dl| PlanRow {
                name: name.into(),
                repo: repo.into(),
                from_version: anc.into(),
                to_version: nouv.into(),
                dl,
                net: dl.map(|d: i64| d / 3),
                aur,
                kind,
                is_downgrade: false,
            };
            let mut p = plan::empty();
            p.rows = vec![
                line("linux", "core", "6.17.4-1", "6.17.6-1", Kind::Upgrade, false, Some(148_000_000)),
                line("glibc", "core", "2.42-3", "2.42-4", Kind::Upgrade, false, Some(7_300_000)),
                line("libfoo", "extra", "", "1.4.2-1", Kind::New, false, Some(240_000)),
                line("quickshell-git", "aur", "r1842-1", "r1851-1", Kind::Upgrade, true, None),
            ];
            p.total_dl = 155_540_000;
            p.total_installed = 420_000_000;
            p.net = 51_846_666;
            p.aur_count = 1;
            p.unknown = 1;
            let excluded = vec!["spotify".to_string(), "tmux".to_string()];
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Partial upgrade").into(),
                cmd: vec!["paru".into(), "-Syu".into(), "--ignore".into(), "spotify".into()],
                risks: risks::analyze(&p, &app.state, &excluded, false),
                plan: p,
                removal: false,
                notes: Vec::new(),
            });
            app.mode = Mode::Plan;
        }
        // 11: the transcript during a run, fed by a stream naming the packages
        // of the plan fabricated in mode 10.
        11 => {
            use plan::{Kind, PlanRow};
            let line = |name: &str, nouv: &str, aur| PlanRow {
                name: name.into(),
                repo: "core".into(),
                from_version: "1.0".into(),
                to_version: nouv.into(),
                dl: Some(1_000_000),
                net: Some(1000),
                aur,
                kind: Kind::Upgrade,
                is_downgrade: false,
            };
            let mut p = plan::empty();
            p.rows = vec![
                line("linux", "6.17.6-1", false),
                line("linux-firmware", "20260810-1", false),
                line("glibc", "2.42-4", false),
                line("quickshell-git", "r1851-1", true),
            ];
            let stream = r":: Processing package changes...
(1/4) upgrading linux [####] 100%%
(2/4) upgrading glibc [##--] 40%%
";
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Demo").into(),
                cmd: vec!["sh".into(), "-c".into(), format!("printf '{stream}'; sleep 5")],
                plan: p,
                risks: Vec::new(),
                removal: false,
                notes: Vec::new(),
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 12: a run that finished successfully. Checks that no package is left
        // "pending" and that the bar has stopped spinning.
        12 => {
            use plan::{Kind, PlanRow};
            let mut p = plan::empty();
            p.rows = vec![PlanRow {
                name: "fastfetch".into(),
                repo: "extra".into(),
                from_version: "2.66.0-1".into(),
                to_version: "2.67.1-1".into(),
                dl: Some(638_500),
                net: Some(8_300),
                aur: false,
                kind: Kind::Upgrade,
                is_downgrade: false,
            }];
            p.total_dl = 638_500;
            p.net = 8_300;
            // Reproduces the real sequence: the install, then the AUR phases
            // that follow and used to wipe the progress.
            let stream = r":: Processing package changes...
(1/1) upgrading fastfetch [####] 100%%
:: Looking for AUR upgrades...
:: Looking for devel upgrades...
";
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Demo").into(),
                cmd: vec!["sh".into(), "-c".into(), format!("printf '{stream}'")],
                plan: p,
                risks: Vec::new(),
                removal: false,
                notes: Vec::new(),
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    if s.pump() && !s.running() {
                        break;
                    }
                }
            }
        }
        // 13: the package retrieval phase. Checks that the bar does not jump to
        // 100 % on a single file's percentage, and that the fetched package
        // moves to "downloaded" without being called installed.
        13 => {
            use plan::{Kind, PlanRow};
            let line = |name: &str, anc: &str, nouv: &str| PlanRow {
                name: name.into(),
                repo: "extra".into(),
                from_version: anc.into(),
                to_version: nouv.into(),
                dl: Some(638_500),
                net: Some(8_300),
                aur: false,
                kind: Kind::Upgrade,
                is_downgrade: false,
            };
            let mut p = plan::empty();
            p.rows = vec![
                line("fastfetch", "2.66.0-1", "2.67.1-1"),
                line("python-pbs-installer", "2026.08.07-1", "2026.08.14-1"),
            ];
            let stream = concat!(
                r":: Retrieving packages...
",
                r" fastfetch-2.67.1-1-x86_64.pkg.tar.zst   638.5 KiB  1863 KiB/s 00:00 [####] 100%%
",
            );
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Demo").into(),
                cmd: vec!["sh".into(), "-c".into(), format!("printf '{stream}'; sleep 5")],
                plan: p,
                risks: Vec::new(),
                removal: false,
                notes: Vec::new(),
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 14: a removal plan on a package with a real cascade. Nothing runs:
        // only the plan is computed, through a pacman dry run.
        14 => {
            app.tab = 1;
            app.go_to(0);
            app.checked.insert("asciiquarium".to_string());
            app.apply();
        }
        // 15: replays the full demo and snapshots the screen after a delay
        // given by the 4th argument (in tenths of a second).
        15 => {
            use plan::{Kind, PlanRow};
            let line = |name: &str, anc: &str, nouv: &str, dl: i64, g| PlanRow {
                name: name.into(),
                repo: "extra".into(),
                from_version: anc.into(),
                to_version: nouv.into(),
                dl: Some(dl),
                net: Some(dl / 4),
                aur: false,
                kind: g,
                is_downgrade: false,
            };
            let mut p = plan::empty();
            p.rows = vec![
                line("fastfetch", "2.66.0-1", "2.67.1-1", 653_824, Kind::Upgrade),
                line("bat", "0.26.1-1", "0.26.1-2", 2_516_582, Kind::Upgrade),
                line("libfoo", "", "1.4.2-1", 245_760, Kind::New),
            ];
            p.total_dl = 3_416_166;
            p.net = 854_041;
            app.intent = Some(app::Intent {
                display_command: None,
                title: crate::i18n::t("Demo").into(),
                cmd: demo::command()?,
                plan: p,
                risks: Vec::new(),
                removal: false,
                notes: Vec::new(),
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            // The simulated password prompt waits for input, so it is given.
            if let Some(s) = app.session.as_mut() {
                std::thread::sleep(Duration::from_millis(150));
                s.send(b"x\r");
            }
            let tenths = nums.get(3).copied().unwrap_or(30);
            for _ in 0..(tenths * 4) {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 16: the changelog screen for the selected package (needs network).
        // 17: history, positioned on the busiest transaction — that is where the
        // layout can overflow.
        // 18: the rollback plan built from that transaction.
        17 | 18 => {
            app.tab = 3;
            // A fourth number picks the transaction; without it, the busiest
            // one, which puts the layout to the test.
            let biggest = nums.get(3).copied().unwrap_or_else(|| app
                .history
                .iter()
                .take(60)
                .enumerate()
                .max_by_key(|(_, t)| t.operations.len())
                .map(|(i, _)| i)
                .unwrap_or(0));
            app.go_to(biggest);
            if tab == 18 {
                app.apply();
                // The real command does not appear on screen when it lists a
                // hundred and eighty paths, so it is printed here to be checked
                // exactly as it will run.
                if let Some(i) = &app.intent {
                    eprintln!("real command: {:?}", i.cmd);
                }
            }
        }
        // 22: a long transcript, to check that the operations can be walked
        // back. A fourth number scrolls that many rows away from the tail.
        22 => {
            let mut script = String::from(":: Processing package changes...\\n");
            for n in 1..=40 {
                script.push_str(&format!("({n}/40) upgrading package-{n:02} [####] 100%%\\n"));
            }
            script.push_str(":: Running post-transaction hooks...\\n");
            for n in 1..=8 {
                script.push_str(&format!("({n}/8) Hook number {n}...\\n"));
            }
            app.intent = Some(app::Intent {
                display_command: None,
                title: i18n::t("Demo").into(),
                cmd: vec!["sh".into(), "-c".into(), format!("printf '{script}'")],
                plan: plan::empty(),
                risks: Vec::new(),
                removal: false,
                notes: vec![i18n::t("demo render").into()],
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    if s.pump() && !s.running() {
                        break;
                    }
                }
            }
            // A fourth number pins the window to that event index. Scrolling
            // by a delta would need a frame rendered first, since it starts from
            // where the last one landed — in the interface one always is.
            if let Some(&start) = nums.get(3) {
                app.journal_anchor.set(Some(start));
            }
            // A fifth number switches to paru's raw output, the other half of
            // what the footer has to describe.
            app.raw_visible = nums.get(4).is_some_and(|n| *n != 0);
        }
        // 23: paru's resolution table with its confirmation prompt — the exact
        // moment where nothing had been retold and the plan could not know what
        // the AUR side pulls in.
        23 => {
            let script = concat!(
                ":: Resolving dependencies...\\n",
                ":: Calculating conflicts...\\n",
                "Repo (1)        Old Version  New Version  Make Only\\n",
                "extra/go                     2:1.26.6-1   Yes\\n",
                "Aur (1)         Old Version  New Version  Make Only\\n",
                "aur/plakar-git               1.0.3.r384.gd77c14a2-1  No\\n",
                ":: Proceed with installation? [Y/n]: ",
            );
            app.intent = Some(app::Intent {
                display_command: None,
                title: i18n::t("Demo").into(),
                cmd: vec!["sh".into(), "-c".into(), format!("printf '{script}'; sleep 5")],
                plan: plan::empty(),
                risks: Vec::new(),
                removal: false,
                notes: vec![i18n::t("demo render").into()],
            });
            app.mode = Mode::Plan;
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
        }
        // 19: paru's raw output (the "j" key) at the end of a run — used to
        // check that the *last* line produced is really visible.
        19 => {
            app.tab = 0;
            app.check_all();
            app.message = None;
            app.apply();
            if let Some(i) = app.intent.as_mut() {
                i.cmd = vec![
                    "sh".into(),
                    "-c".into(),
                    "for i in $(seq 1 60); do echo \"ligne $i\"; done".into(),
                ];
            }
            app.start(
                height.saturating_sub(ui::RUN_CHROME),
                width.saturating_sub(2),
            );
            app.raw_visible = true;
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(s) = app.session.as_mut() {
                    s.pump();
                }
            }
            // A fourth number scrolls back that many rows, as the up arrow
            // would once the run has finished.
            if let (Some(n), Some(s)) = (nums.get(3), app.session.as_mut()) {
                s.scroll_by(*n as isize);
            }
        }
        // 20: the search tab, with the query passed after the dimensions. The
        // search hits the network, so it is given time to land.
        20 | 21 => {
            app.tab = 4;
            app.filter = std::env::args()
                .skip_while(|a| a != "--query")
                .nth(1)
                .unwrap_or_else(|| "yazi".into());
            app.run_search();
            for _ in 0..200 {
                std::thread::sleep(Duration::from_millis(50));
                if app.search.pump() {
                    break;
                }
            }
            // A fourth number picks the result, to inspect an AUR package's
            // detail panel rather than whichever came first.
            app.go_to(nums.get(3).copied().unwrap_or(0));
            // 21: the install plan for that result, to see what the requested
            // package drags along behind it.
            if tab == 21 {
                app.toggle_check();
                app.message = None;
                app.apply();
            }
        }
        16 => {
            app.tab = 0;
            app.go_to(0);
            app.open_changelog();
            for _ in 0..200 {
                std::thread::sleep(Duration::from_millis(50));
                if let Some(c) = app.changelog.as_mut() {
                    if c.pump() {
                        break;
                    }
                }
            }
        }
        n => {
            app.tab = n.min(4);
            app.go_to(0);
        }
    }

    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;

    let buffer = terminal.backend().buffer().clone();
    let mut output = String::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    // Written in one go, error ignored: piping into `head` closes the pipe
    // partway through, and a println! would panic on that broken pipe.
    use std::io::Write;
    let _ = std::io::stdout().write_all(output.as_bytes());
    Ok(())
}

fn run_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let mut redraw = true;
    let mut last_frame = Instant::now();
    loop {
        // The spinner and the waiting shuttle must keep moving even when paru
        // writes nothing — during a build the output can fall silent for
        // seconds, and the interface would look frozen.
        if app.mode == Mode::Running && last_frame.elapsed() >= Duration::from_millis(90) {
            redraw = true;
        }
        // The pseudo terminal must know its panel's size, otherwise pacman
        // computes its progress bars for a different width.
        if app.mode == Mode::Running {
            let (l, c) = embedded_terminal_size(terminal, app)?;
            if let Some(s) = app.session.as_mut() {
                s.resize(l, c);
            }
        }

        if redraw {
            terminal.draw(|f| ui::draw(f, app))?;
            redraw = false;
            last_frame = Instant::now();
        }

        // Polling is more frequent during a run: that is what makes the progress
        // bars smooth. At rest, slow polling is enough.
        let attente = if app.mode == Mode::Running {
            Duration::from_millis(25)
        } else {
            Duration::from_millis(150)
        };

        if event::poll(attente)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.mode {
                        Mode::Table => table_key(app, key)?,
                        Mode::Plan => plan_key(app, key, terminal)?,
                        Mode::Running => run_key(app, key)?,
                        Mode::Changelog => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') => {
                                app.changelog = None;
                                app.mode = Mode::Table;
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.changelog_scroll =
                                    app.changelog_scroll.saturating_add(1)
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.changelog_scroll =
                                    app.changelog_scroll.saturating_sub(1)
                            }
                            KeyCode::PageDown => {
                                app.changelog_scroll =
                                    app.changelog_scroll.saturating_add(10)
                            }
                            KeyCode::PageUp => {
                                app.changelog_scroll =
                                    app.changelog_scroll.saturating_sub(10)
                            }
                            _ => {}
                        },
                    }
                    redraw = true;
                }
                Event::Resize(_, _) => redraw = true,
                _ => {}
            }
        }

        // The search answer, arriving in the background.
        if app.search.pump() {
            app.go_to(app.list.selected().unwrap_or(0));
            redraw = true;
        }

        // The changelog request's answer, arriving in the background.
        if let Some(c) = app.changelog.as_mut() {
            if c.pump() {
                redraw = true;
            }
        }

        // Bytes produced by paru since the previous round.
        if let Some(s) = app.session.as_mut() {
            if s.pump() {
                redraw = true;
            }
        }

        if app.quit {
            return Ok(());
        }
    }
}

/// Usable size of the panel that holds paru's output: total height minus the
/// two banners, minus the frame's borders.
fn embedded_terminal_size(
    terminal: &ratatui::DefaultTerminal,
    app: &App,
) -> Result<(u16, u16)> {
    // As soon as one frame has been rendered, the panel's exact size is known;
    // the estimate only serves the very first launch.
    let (l, c) = app.pty_size.get();
    if l > 0 && c > 0 {
        return Ok((l, c));
    }
    let t = terminal.size()?;
    Ok((t.height.saturating_sub(ui::RUN_CHROME), t.width.saturating_sub(2)))
}

fn plan_key(
    app: &mut App,
    key: ratatui::crossterm::event::KeyEvent,
    terminal: &ratatui::DefaultTerminal,
) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            // The panel has not been rendered for this run yet: the recorded
            // size is the previous run's, or the estimate.
            app.pty_size.set((0, 0));
            let (l, c) = embedded_terminal_size(terminal, app)?;
            app.start(l, c);
        }
        KeyCode::Esc | KeyCode::Char('q') => {
            app.intent = None;
            app.risks_scroll = 0;
            app.mode = Mode::Table;
        }
        // The points of attention have their own scroll: they can outgrow the
        // available room even though they are what needs reading.
        KeyCode::PageDown => app.risks_scroll = app.risks_scroll.saturating_add(1),
        KeyCode::PageUp => app.risks_scroll = app.risks_scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            let n = app.intent.as_ref().map_or(0, |i| i.plan.rows.len());
            if n > 0 {
                let i = app.plan_list.selected().map_or(0, |i| (i + 1) % n);
                app.plan_list.select(Some(i));
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let n = app.intent.as_ref().map_or(0, |i| i.plan.rows.len());
            if n > 0 {
                let i = app
                    .plan_list
                    .selected()
                    .map_or(0, |i| (i + n - 1) % n);
                app.plan_list.select(Some(i));
            }
        }
        _ => {}
    }
    Ok(())
}

/// During a run, keystrokes go to paru — Ctrl-C included — so that its questions
/// can be answered and the sudo password typed without leaving
/// l'interface.
///
/// Two exceptions: the movement keys always drive our own scrolling, and "j"
/// switches between the transcript and the raw output.
fn run_key(
    app: &mut App,
    key: ratatui::crossterm::event::KeyEvent,
) -> Result<()> {
    // The toggle first: it is precisely once finished that one wants to re-read
    // the detailed output, and the early return used to intercept it.
    if key.code == KeyCode::Char('j') && !key.modifiers.contains(KeyModifiers::CONTROL) {
        app.raw_visible = !app.raw_visible;
        return Ok(());
    }

    // Scrolling before anything else, and unconditionally. Placing it after the
    // early return on `exit_code` made it dead once paru had finished — that is,
    // exactly when the output is opened to be read end to end.
    //
    // Which of the two views scrolls depends on which one is showing. They are
    // not the same thing: the raw screen is what paru printed, the transcript is
    // what nalarch made of it, and each has its own history to walk. Seventy-five
    // operations do not fit on screen, and everything before the tail used to be
    // unreachable without switching to the raw output.
    let step = match key.code {
        KeyCode::Up => Some(1),
        KeyCode::Down => Some(-1),
        KeyCode::PageUp => Some(10),
        KeyCode::PageDown => Some(-10),
        _ => None,
    };
    if let Some(delta) = step {
        if app.raw_visible {
            if let Some(session) = app.session.as_mut() {
                session.scroll_by(delta as isize);
            }
        } else {
            app.scroll_journal(delta);
        }
        return Ok(());
    }
    if matches!(key.code, KeyCode::Home | KeyCode::End) {
        let to_top = key.code == KeyCode::Home;
        match (app.raw_visible, to_top) {
            (true, true) => {
                if let Some(s) = app.session.as_mut() {
                    s.scroll_by(10_000);
                }
            }
            (true, false) => {
                if let Some(s) = app.session.as_mut() {
                    s.scroll_to_bottom();
                }
            }
            (false, true) => app.scroll_journal(i32::MAX / 2),
            (false, false) => app.follow_journal(),
        }
        return Ok(());
    }

    let exit_code = app.session.as_ref().is_some_and(|s| !s.running());
    if exit_code {
        // Once paru has finished, the keys become nalarch's again.
        if matches!(
            key.code,
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q')
        ) {
            app.finish()?;
        }
        return Ok(());
    }

    // The movement keys always drive nalarch's own scrolling and are never
    // forwarded.
    //
    // The previous version only intercepted them during a prompt, and only the
    // vertical ones. Horizontal arrows therefore reached a "[Y/n]" question that
    // cannot interpret them: they were echoed as escape sequences, the line
    // wrapped, the cursor left the line carrying "[Y/n]", prompt detection
    // failed — and from then on every arrow got through. A single keystroke was
    // enough to set it off.
    //
    // The wheel gets the same treatment: in the alternate screen the terminal
    // translates it into up/down arrows.
    let Some(session) = app.session.as_mut() else {
        return Ok(());
    };

    // The vertical ones are handled above; the horizontal ones are left, and
    // they have nothing useful to forward and everything to lose by being.
    if matches!(key.code, KeyCode::Left | KeyCode::Right) {
        return Ok(());
    }

    let bytes = encode_key(key);
    if !bytes.is_empty() {
        // Any forwarded keystroke returns to the bottom of the history: what
        // paru answers is what one wants to see.
        session.scroll_to_bottom();
        session.send(&bytes);
    }
    Ok(())
}

/// Translates a key into the byte sequence a terminal would send.
fn encode_key(key: ratatui::crossterm::event::KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Ctrl-A..Ctrl-Z valent 0x01..0x1a.
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => {
            vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        // A terminal sends a carriage return, not a line feed.
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        // Arrows and page keys are deliberately absent: they drive nalarch's
        // own scrolling and must never leave for the process. See `run_key`.
        _ => Vec::new(),
    }
}

fn table_key(
    app: &mut App,
    key: ratatui::crossterm::event::KeyEvent,
) -> Result<()> {
    if app.search_mode {
        match key.code {
            KeyCode::Esc => {
                app.filter.clear();
                app.search_mode = false;
                app.go_to(0);
            }
            KeyCode::Enter => {
                app.search_mode = false;
                // On the search tab the query is not a filter over rows already
                // in hand: validating it goes and asks the repositories and the
                // AUR.
                if app.current_tab() == app::Tab::Search {
                    app.run_search();
                }
            }
            KeyCode::Backspace => {
                app.filter.pop();
                app.go_to(0);
            }
            KeyCode::Char(c) => {
                app.filter.push(c);
                app.go_to(0);
            }
            _ => {}
        }
        return Ok(());
    }

    // Any keystroke clears the previous message: it must not stay on screen
    // past the next action.
    app.message = None;

    match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.quit = true
                }

                KeyCode::Down | KeyCode::Char('j') => app.move_by(1),
                KeyCode::Up | KeyCode::Char('k') => app.move_by(-1),
                // In the history, jumping ten transactions is less useful than
                // walking through the one being looked at: a full upgrade
                // tient sur cent rows.
                KeyCode::PageDown if app.current_tab() == app::Tab::History => {
                    app.scroll_detail(10)
                }
                KeyCode::PageUp if app.current_tab() == app::Tab::History => {
                    app.scroll_detail(-10)
                }
                KeyCode::PageDown => app.move_by(10),
                KeyCode::PageUp => app.move_by(-10),
                KeyCode::Home | KeyCode::Char('g') => app.go_to(0),
                KeyCode::End | KeyCode::Char('G') => app.go_to(usize::MAX),

                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => app.switch_tab(1),
                KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => app.switch_tab(-1),

                KeyCode::Char(' ') => app.toggle_check(),
                KeyCode::Char('a') => app.check_all(),
                KeyCode::Char('n') => app.uncheck_all(),
                KeyCode::Char('p') => app.toggle_protection(),
                KeyCode::Char('/') => {
                    app.search_mode = true;
                    app.filter.clear();
                }
                KeyCode::Char('r') => {
                    app.reload()?;
                    app.message =
                        Some((crate::i18n::t("State reloaded").into(), app::Severity::Success));
                }
                KeyCode::Char('c') => app.open_changelog(),
                KeyCode::Char('u') => app.apply(),
                KeyCode::Char('U') => app.purge_uninstalled(),
                _ => {}
            }
    Ok(())
}
