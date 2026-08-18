//! Running paru inside nalarch, through a pseudo terminal.
//!
//! Why a PTY and not a plain stream redirection: paru and pacman both
//! check whether their output is a terminal. On a pipe they turn off colours
//! and progress bars, and `sudo` flatly refuses to read a
//! password from one. With a PTY, paru behaves exactly as it would in a real
//! terminal, so its complete output comes back verbatim.
//!
//! The stream then goes through a terminal emulator (vt100) because that output
//! is not linear text: progress bars rewrite themselves with carriage returns,
//! and pacman moves the cursor around to fit several parallel downloads on
//! screen. Only an emulator reproduces the resulting screen faithfully.

use crate::journal::{Journal, Phase};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub struct Session {
    pub command: String,
    parser: vt100::Parser,
    rx: Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    /// Exit code once the process has finished.
    pub exit_code: Option<u32>,
    rows: u16,
    cols: u16,
    /// Rows scrolled back into history (0 = bottom of the screen).
    scroll: usize,
    /// Reconstruction of the operation from the stream, independent of what is
    /// still displayed on screen.
    journal: Journal,
    /// True once a Ctrl-C has been forwarded: the non-zero exit code that
    /// follows is then explained by the interruption, not by an error.
    pub interrupted: bool,
    splitter: LineSplitter,
    /// Start of the stopwatch shown next to the bar.
    started: Instant,
    /// End. The stopwatch has to stop with the process, otherwise it keeps
    /// climbing on a screen that says "finished".
    ended: Option<Instant>,
}

/// Strips ANSI escape sequences from a line before analysing it.
fn strip_ansi(s: &str) -> String {
    let mut output = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            output.push(c);
            continue;
        }
        match chars.peek().copied() {
            // CSI: ESC [ parameters… final byte between @ and ~.
            Some('[') => {
                chars.next();
                for f in chars.by_ref() {
                    if ('@'..='~').contains(&f) {
                        break;
                    }
                }
            }
            // Character set designation: ESC ( B and friends. Two bytes follow,
            // not one — dropping only the first left the `B` on screen, which is
            // how every makepkg message came out as "Checking sources...B". It
            // is the tail of `tput sgr0`, so it appeared on all of them.
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
                chars.next();
            }
            // Strings terminated by BEL or ST: OSC and its cousins.
            Some(']') | Some('P') | Some('_') | Some('^') | Some('X') => {
                chars.next();
                while let Some(f) = chars.next() {
                    if f == '\x07' {
                        break;
                    }
                    if f == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Everything else is a single-character escape.
            _ => {
                chars.next();
            }
        }
    }
    output
}

/// Rebuilds lines of text from a byte stream.
///
/// Two traps it isolates:
///
/// - a read on the pseudo terminal can cut in the middle of a multi-byte
///   character, so bytes are accumulated and only a whole line is decoded —
///   converting byte by byte produced Latin-1 and displayed "paquetâ¦" instead
///   of "paquet…";
/// - a carriage return ends a line just as a line feed does, because that is
///   how pacman rewrites its progress bars: it is where the successive
///   percentages live.
#[derive(Default)]
struct LineSplitter {
    buffer: Vec<u8>,
}

impl LineSplitter {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut rows = Vec::new();
        for &o in bytes {
            match o {
                b'\n' | b'\r' => {
                    let raw = std::mem::take(&mut self.buffer);
                    if !raw.is_empty() {
                        rows.push(strip_ansi(&String::from_utf8_lossy(&raw)));
                    }
                }
                // Safety net: a line with no end must not grow the buffer
                // indefinitely.
                _ if self.buffer.len() < 8192 => self.buffer.push(o),
                _ => {}
            }
        }
        rows
    }
}

/// Progress in a form the screen can show.
pub struct Progress {
    /// Fraction done, absent when nothing is measurable (an AUR build).
    pub fraction: Option<f64>,
    /// Phase name, kept for the displays that have no access to the journal.
    #[allow(dead_code)]
    pub label: String,
    /// Current item over total, when pacman gives it.
    pub counter: Option<(u32, u32)>,
}

impl Session {
    pub fn spawn(cmd: &[String], rows: u16, cols: u16) -> Result<Self> {
        let (rows, cols) = (rows.max(4), cols.max(20));

        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("opening the pseudo terminal")?;

        let mut builder = CommandBuilder::new(&cmd[0]);
        for a in &cmd[1..] {
            builder.arg(a);
        }
        // A rich terminal is advertised: that is what vt100 can interpret, and
        // it stops paru from falling back to a degraded mode.
        builder.env("TERM", "xterm-256color");
        builder.env("COLORTERM", "truecolor");
        // Locale pinned to C: the output is rewritten by nalarch, and parsing
        // translated labels would be untenable — every language changes the
        // verbs, and a translation update would break parsing silently. None of
        // that English reaches the screen unmediated.
        builder.env("LC_ALL", "C");
        builder.env("LANG", "C");
        if let Ok(dir) = std::env::current_dir() {
            builder.cwd(dir);
        }

        let child = pair
            .slave
            .spawn_command(builder)
            .with_context(|| format!("launching {}", cmd[0]))?;
        // The slave must be closed on the parent side, otherwise reading the
        // master would never see end-of-file when the process exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("reading the pseudo terminal")?;
        let writer = pair
            .master
            .take_writer()
            .context("writing to the pseudo terminal")?;

        // Reading blocks, so it lives on its own thread, and the interface
        // loop picks the bytes up without ever freezing.
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            command: cmd.join(" "),
            parser: vt100::Parser::new(rows, cols, 5000),
            rx,
            writer,
            child,
            master: pair.master,
            exit_code: None,
            rows,
            cols,
            scroll: 0,
            journal: Journal::default(),
            interrupted: false,
            started: Instant::now(),
            ended: None,
            splitter: LineSplitter::default(),
        })
    }

    /// Consumes the available bytes and updates the emulated screen.
    /// True when something changed and a redraw is needed.
    pub fn pump(&mut self) -> bool {
        let mut change = false;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => {
                    self.absorber(&bytes);
                    self.parser.process(&bytes);
                    change = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if self.exit_code.is_none() {
            if let Ok(Some(statut)) = self.child.try_wait() {
                self.exit_code = Some(statut.exit_code());
                self.ended = Some(Instant::now());
                change = true;
            }
        }
        change
    }

    /// Splits the raw stream into lines and extracts progress from them.
    fn absorber(&mut self, bytes: &[u8]) {
        for line in self.splitter.push(bytes) {
            self.journal.analyze(&line);
        }
    }

    /// Progress to display, or None if the process has announced nothing yet.
    ///
    /// Once the process has finished successfully, progress is 100 % whatever
    /// the phases said: the last one paru announces is a plain search for AUR
    /// updates, with no counter, and leaving the bar in that state gave a
    /// "finished" run with a bar still spinning
    /// encore.
    pub fn progress(&self) -> Option<Progress> {
        if self.exit_code == Some(0) {
            return Some(Progress {
                fraction: Some(1.0),
                label: crate::i18n::t("finished").into(),
                counter: None,
            });
        }
        let j = &self.journal;
        // The phase counter gives measurable progress; the percentage only
        // refines the position inside the current item. On its own it is worth
        // nothing: during retrieval pacman shows one per file, and the bar
        // announced 100 % before any installation had started.
        if let Some((n, m)) = j.counter {
            if m > 0 {
                let within = j.percent.unwrap_or(0) as f64 / 100.0;
                return Some(Progress {
                    fraction: Some((((n as f64 - 1.0) + within) / m as f64).clamp(0.0, 1.0)),
                    label: j.phase.label().to_string(),
                    counter: Some((n, m)),
                });
            }
        }
        Some(Progress {
            fraction: None,
            label: match &j.compilation {
                Some(c) if j.phase == Phase::Building => c.clone(),
                _ => j.phase.label().to_string(),
            },
            counter: None,
        })
    }

    /// Step shown in the header. Afterwards, the last announced phase is no
    /// longer a step in progress: presenting it as one suggests
    /// there is still work left.
    pub fn step_text(&self) -> String {
        match self.exit_code {
            None => self
                .step()
                .unwrap_or_else(|| crate::i18n::t("starting…").into()),
            Some(0) => crate::i18n::t("every step succeeded").into(),
            Some(_) => match self.step() {
                Some(e) => format!("interrompu pendant : {e}"),
                None => "interrompu".into(),
            },
        }
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if bytes.contains(&0x03) {
            self.interrupted = true;
        }
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(4), cols.max(20));
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Time elapsed since launch, shown next to the bar.
    pub fn duration(&self) -> Duration {
        match self.ended {
            Some(f) => f.duration_since(self.started),
            None => self.started.elapsed(),
        }
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn running(&self) -> bool {
        self.exit_code.is_none()
    }

    /// Current major phase, as pacman or paru announced it.
    ///
    /// It comes from tracking the stream, not from the screen: during an AUR
    /// build makepkg's output scrolls the "::" announcements out of view within
    /// seconds, and the step then stayed stuck on "starting…".
    pub fn step(&self) -> Option<String> {
        Some(self.journal.phase.label().to_string())
    }

    /// Scrolls up or down through the emulated terminal's history.
    pub fn scroll_by(&mut self, delta: isize) {
        let n = (self.scroll as isize + delta).max(0) as usize;
        self.scroll = n;
        self.parser.screen_mut().set_scrollback(n);
    }

    /// Back to the bottom of the history. Called as soon as anything is written
    /// to paru: what answers the keystroke is certainly what one wants to see.
    /// Number of rows scrolled back into history.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.scroll != 0 {
            self.scroll = 0;
            self.parser.screen_mut().set_scrollback(0);
        }
    }

    /// Detects that a process is waiting for input.
    ///
    /// The cursor position is trusted rather than a pattern searched across the
    /// whole screen: when paru asks a question, the cursor sits right after it.
    /// A question already answered stays displayed higher up but no longer holds
    /// the cursor, so it raises no false positive.
    pub fn prompt(&self) -> Option<Prompt> {
        if !self.running() {
            return None;
        }
        let screen = self.screen();
        let (line, _) = screen.cursor_position();
        let text = screen.contents_between(line, 0, line, self.cols);
        let t = text.trim();
        if t.is_empty() {
            return None;
        }

        let bottom = t.to_lowercase();
        // sudo does not echo the password being typed: that has to be signalled
        // so the user does not think the interface has frozen.
        if bottom.contains("password") || bottom.contains("mot de passe") {
            return Some(Prompt {
                text: t.to_string(),
                masked: true,
            });
        }
        if is_question(&bottom) {
            return Some(Prompt {
                text: t.to_string(),
                masked: false,
            });
        }
        None
    }

}

pub struct Prompt {
    pub text: String,
    /// True for a password entry: nothing must appear.
    pub masked: bool,
}

/// Recognises a question put to the user.
///
/// Three shapes, and the third is the one that matters. pacman and paru print
/// the possible answers between brackets, with the capital marking the default —
/// that covers `[Y/n]` in any language. Some questions merely end in a question
/// mark. But `Enter a number (default=1):` has neither, and missing it left
/// nalarch announcing that nothing was expected while paru sat blocked on a
/// provider choice.
///
/// A trailing colon is enough here only because of what calls this: the line the
/// cursor is sitting on. Everything pacman prints ends with a newline, so the
/// cursor only stays put on something written without one — which is what a
/// prompt is.
fn is_question(bottom: &str) -> bool {
    ["[y/n]", "[o/n]", "[n/y]", "[n/o]"]
        .iter()
        .any(|m| bottom.contains(m))
        || bottom.ends_with('?')
        || bottom.ends_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_sequences_disappear() {
        assert_eq!(strip_ansi("\x1b[1;32m==>\x1b[0m Cleaning…"), "==> Cleaning…");
        assert_eq!(strip_ansi("no colour"), "no colour");
    }

    /// `tput sgr0` ends with `ESC ( B`, a two-byte charset designation. Dropping
    /// only the first byte left the `B` behind, and makepkg resets its colours
    /// on every message — so every build step read "Checking sources...B".
    #[test]
    fn a_charset_designation_does_not_leave_its_letter_behind() {
        assert_eq!(
            strip_ansi("\x1b[1m\x1b[34m==>\x1b(B\x1b[m Checking sources...\x1b(B\x1b[m"),
            "==> Checking sources..."
        );
    }

    /// A window title is a string escape, terminated by BEL or ST rather than by
    /// a final byte in a range.
    #[test]
    fn a_string_escape_is_swallowed_whole() {
        assert_eq!(strip_ansi("\x1b]0;building\x07done"), "done");
        assert_eq!(strip_ansi("\x1b]0;building\x1b\\done"), "done");
    }

    /// The stream arrives in byte chunks of arbitrary size: a read may well cut
    /// in the middle of a "…". Converting byte by byte produced Latin-1, hence
    /// the "paquetâ¦" seen on screen.
    #[test]
    fn a_cut_inside_a_character_does_not_break_the_accents() {
        let whole = ":: Création du paquet…\n".as_bytes();
        // 26 bytes: "é" sits at positions 5-6, "…" at 22-24.
        for cut in [6, 7, 23, 24] {
            let mut d = LineSplitter::default();
            let mut rows = d.push(&whole[..cut]);
            rows.extend(d.push(&whole[cut..]));
            assert_eq!(
                rows,
                vec![":: Création du paquet…".to_string()],
                "cut after {cut} bytes"
            );
        }
    }

    #[test]
    fn a_carriage_return_ends_a_line_just_like_a_line_feed() {
        let mut d = LineSplitter::default();
        // What a progress bar rewriting itself in place produces.
        let rows = d.push(b"thing [##--]  40%\rthing [####]  80%\n");
        assert_eq!(rows, vec!["thing [##--]  40%", "thing [####]  80%"]);
    }

    #[test]
    fn a_question_is_recognised_whatever_the_case() {
        assert!(is_question("procéder à l'installation ? [o/n]"));
        assert!(is_question(":: proceed with installation? [y/n]"));
        assert!(!is_question("(1/3) upgrading fastfetch"));
    }

    /// The shape that was missed: no brackets, no question mark. paru sat
    /// waiting on it while the footer said nothing was expected.
    #[test]
    fn a_prompt_ending_in_a_colon_is_a_question_too() {
        assert!(is_question("enter a number (default=1):"));
        assert!(is_question("[sudo] password for ksh:"));
        // A progress bar also leaves the cursor on its line, and must not be
        // mistaken for one.
        assert!(!is_question("(1/3) upgrading fastfetch [###] 100%"));
        assert!(!is_question(" fastfetch-2.67.1-1-x86_64.pkg.tar.zst  638.5 kib"));
    }
}
