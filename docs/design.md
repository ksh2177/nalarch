# nalarch — how it works

The README says what nalarch does and how to run it. This says why it works the
way it does: the screens in detail, the parsing behind them, and the reasoning
behind the choices that are not obvious from the code.

*[Version française](design.fr.md) · [back to the README](../README.md).*

## The three screens

**Table** → **Summary** → **Run**. Nothing launches without going through the summary.

### The summary screen

This is nalarch's reason to exist. A package manager does print everything worth knowing,
but mixed into hundreds of log lines: one ends up reading none of it and approving in hope.
Here it all fits on one screen.


**New packages** are the least visible and most useful addition: `checkupdates` only shows
already-installed packages that have a newer version, never the dependencies an upgrade
pulls in along the way. nalarch gets them from `pacman -Sup --print-format`, run against
`checkupdates`' temporary database — fresh data, no privilege, no writes.

The categories mirror nala's (`Operation` in `src/libnala/transaction.rs`), adapted to
pacman: updates, new packages, downgrades, removals, cascade removals, and frozen packages —
the equivalent of its `Held`.

**Cascade removals** are the removal-side counterpart of new dependencies: `-Rns` takes
along dependencies nothing needs any more, and that is often the bulk of the operation.
Removing `asciiquarium` takes `perl-term-animation` and `perl-curses` with it — 789 KiB
instead of the checked package's 28 KiB. The list comes from `pacman -Rsp`, a dry run with
no privilege. (`-n` and `-p` are incompatible in pacman; omitting it does not change the
list, since `-n` only concerns configuration files.)

**Points of attention** are computed, not decorative. Detected: AUR builds (a PKGBUILD is
unreviewed code running under your account), partial upgrades, kernel updates and DKMS
modules to rebuild, essential system components, downgrades, new dependencies, updates
frozen by `IgnorePkg`. For a removal: packages still needed by others, and the reminder that
plugins loaded at run time are invisible.

The rule followed: only announce what is **verified**. A vague warning is quickly ignored,
and a list that gets ignored protects nothing.

### The run screen

Laid out in blocks, like nala's: what is downloading, what is running, then what is left to
know. Each block carries its own bar, because there is no single measure of progress —
downloading and installing are two separate counts, and mixing them would give a number that
means nothing.


Before any of that, paru may have a resolution of its own to show. Installing an AUR package
is the case where nalarch's plan is knowingly incomplete: pacman cannot resolve `plakar-git`,
so the plan says "1 package, sizes unknown" while paru works out that `go` has to come along
to build it. paru prints that in a table and asks to proceed — and until it was read, the
transcript said `Operations · 0` and the confirmation asked to approve something nothing on
screen had shown. It is now retold like the rest, with build-only dependencies marked as
such:

```
┌ paru resolved · 2 package(s) ────────────────────────────────────────────┐
│ ⚒ extra      go                    2:1.26.6-1  to build only             │
│ + aur        plakar-git            1.0.3.r384.gd77c14a2-1                │
└──────────────────────────────────────────────────────────────────────────┘
```

Targets are handed to paru qualified by their repository — `aur/plakar`, `extra/ripgrep` —
and with `--noprovides`.

The prefix alone is not enough. `paru -S plakar` is ambiguous because `plakar-git` provides
`plakar` too, and `aur/plakar` scopes the repository without touching the search for
providers: paru still asks which one is meant, after the choice has been made by picking a
row. `--provides` covers "targets and missing packages"; the targets here always come from a
list of real package names, so that half of it is pure noise.

The other half is the cost, and it is stated rather than hidden: a dependency that no package
satisfies by name now fails to resolve instead of offering a menu. That failure is loud and
lands in the error list, where the question was silent and happened on every ambiguous
install.

paru also asks numbered questions, and those were worse: `Enter a number (default=1):` has
neither brackets nor a question mark, so it was not recognised as a prompt at all. nalarch
announced that nothing was expected while paru sat blocked on a provider choice. A trailing
colon now counts as a question — safe only because what is inspected is the line the cursor
sits on, and everything pacman prints ends with a newline. The cursor stays put on a prompt
precisely because a prompt has none.

```
┌ paru is asking · which plakar? ──────────────────────────────────────────┐
│ 1 plakar             AUR    ← Enter takes this one                       │
│ 2 plakar-git         AUR                                                 │
└──────────────────────────────────────────────────────────────────────────┘
```

The question is held rather than recomputed each frame, but its text is read live. Holding
the text too froze the question at the instant it appeared, so an answer being typed went
nowhere on screen: four presses of `1` looked exactly like none, and the only way to see them
was paru's raw output — the view the transcript exists to replace. A password is not echoed at
all, so its line stays the question, which is the honest thing to show. Detection reads the line the
cursor sits on, and the first character of an answer is echoed onto that same line — so the
shape stops matching the moment one starts typing, and "input expected" vanished while paru
was still waiting. It is cleared when a complete line arrives, which only happens once paru
has something new to say.

That same missing newline is why the default is read off the emulated screen rather than from
the stream: the line splitter holds an unterminated line in its buffer and never emits it.

The rows are grouped rather than listed. A flat list left `go` sitting there with no reason
given — as likely a bug as a build dependency, from the reader's side. What becomes of those
build dependencies afterwards is read from paru's own configuration rather than guessed:
`RemoveMake` is a per-user setting, and a build dependency can be six hundred megabytes of
compiler, so the difference between "removed again" and "stays installed" is worth stating
correctly.

The table's columns are aligned rather than delimited, so its rows are read by shape: the
first field carries `repo/name`, a trailing `Yes`/`No` is the make-only flag, and what remains
is one version for an install or two for an upgrade.

The **Worth noting** block gathers what calls for an action on your side and what the output
drowns: `.pacnew` files to merge, a required reboot, warnings, errors. Each entry says what
to do, not only what happened — a `.pacnew` comes with the `sudo pacdiff -s` command and the
reminder that without it, the new configuration never applies.

paru's detailed output stays available through `j`, including once the operation has
finished. The reboot is inferred from the plan — pacman does not signal it, and the symptom
shows up much later, when nothing connects it back to the upgrade any more.

### Pinned locale

The child process runs with `LC_ALL=C`. Parsing translated output would be untenable: every
language changes the verbs, and a translation update would break the parser silently. In
English the vocabulary is stable, taken straight from pacman's own strings.

None of that English reaches the screen unmediated: `src/journal.rs` restates everything —
phases, actions, and the common warnings — and the result goes through the same translation
layer as the rest of the interface. An unrecognised message passes through as it is, because
an English sentence beats a lost one.

makepkg is the noisiest source in the stream and needed three things of its own. Its colour
resets end with `ESC ( B`, a two-byte charset designation: dropping only the first byte left
the `B` behind, so every step read "Checking sources...B". Its warnings and errors arrive
through the same `==>` marker as its steps, and filed as steps they sat buried among forty
others rather than in **Worth noting**. And a build has no measurable progress, so the counter
inherited from the phase before it has to go — kept, it pinned the bar at a flat 0.0 % for
minutes, which reads as stuck rather than as unmeasurable.

That parsing is what makes the transcript possible. nalarch used to keep only a counter and
a phase: enough to fill a bar, not enough to tell the story of the operation.

## Seeing what an update changes

Press `c` on a package in the **Updates** tab.

A version transition says nothing about what it brings, and Arch packages almost never ship
a changelog — `pacman -Qc` is empty most of the time. The information exists elsewhere, in
two complementary places:

```
 fastfetch   2.66.0-1 → 2.67.1-1
 upstream: https://github.com/fastfetch-cli/fastfetch
┌ Changes ────────────────────────────────────────────────────────┐
│ Arch packaging log                                              │
│  ▸ 2026-08-14  2.67.1-1: New upstream release                   │
│  ▸ 2026-08-06  2.67.0-1: New upstream release                   │
│    2026-07-10  2.66.0-1: New upstream release                   │
│    ─── installed version ───                                    │
│                                                                 │
│ Upstream release notes · 2.67.1                                 │
│  Bugfixes:                                                      │
│  • Fixed a `Symbol not found` error on macOS 10.15              │
└─────────────────────────────────────────────────────────────────┘
```

The **packaging log** says *why* the package moved: a new upstream release, or a plain
rebuild against a library. That is often the most useful answer — a rebuild brings no
feature and explains an update that looked gratuitous. Entries marked `▸` are what the update
brings; below the line is already installed.

The line above them is the one worth reading first: whether the update is a new upstream
version at all. A package release bumped on its own — `6.29.0-1 → 6.29.0-2` — means a rebuild,
a packaging fix, or a dependency change, never a feature. That is the most common answer and
the hardest to read off a list of commits, so it is stated rather than left to be inferred.

The **upstream release notes** come from GitHub, which covers a bit under half of what is
installed on a typical machine, and from the GitLab instances — GNOME, freedesktop, KDE's
invent — which cover a good part of the rest. Both APIs answer the same question, so
supporting the second costs one more request shape. A GitLab instance cannot be told from any
other host by name, so only the hosts that actually appear as upstream URLs are matched:
guessing would mean a request to an unrelated server for every package. An AUR package has no packaging repository: the PKGBUILD is the source
of truth, and paru offers to show it before building.

The requests go through `curl` on a separate thread — the interface does not freeze, and it
avoids carrying a TLS stack for two optional requests. Whatever could not be fetched is said
out loud rather than left blank.

## Theme

No colour is an absolute value: nalarch only uses the terminal's ANSI slots and never paints
its own background. It therefore follows the terminal's theme, light or dark, including a
live switch.

Coloured backgrounds (badges, selected row) go through reverse video: that is the only way
to get correct contrast without knowing the active theme. A fixed grey would make dim text
unreadable — grey on grey — as soon as the theme flips.

## `--demo` mode

```bash
nalarch --demo
```

Replays a paru-like session: a password prompt, a `[Y/n]` question, bars rewritten with
carriage returns, counters, then an AUR build with no measurable progress. **No pacman
command is invoked, nothing is installed or removed.**

It is not a mock of the interface: the script really runs in the pseudo terminal and goes
through the same emulator and the same stream analysis as paru. Only the source of the text
differs. Without it the run screen would be untestable as soon as the system is up to date —
that is, most of the time.

## `--dump` mode

```bash
nalarch --dump [screen] [width] [height]
```

Renders the interface into an in-memory buffer and writes it as plain text, with no TTY.
Useful to check the layout, produce a capture, or debug from a script.

| screen | contents |
|---|---|
| `0`–`4` | the five tabs of the table |
| `4` | plan screen, as if everything were checked |
| `5` | run screen (runs `pacman -Qi`, read only) |
| `6` | table with everything checked |
| `7` | run in progress, measurable progress |
| `8` | run in progress, AUR build (not measurable) |
| `9` | run finished in failure |
| `17` | history, on the busiest transaction (4th number = index) |
| `20` | the search tab (`--query <term>`, 4th number = which result) |
| `21` | the install plan for that result, dependencies included |
| `22` | a long transcript (4th number = first event shown, 5th = raw output) |
| `23` | paru's resolution table with its confirmation prompt |
| `24` | paru blocked on a provider choice |
| `25` | an AUR build, with the escape sequences makepkg really emits |
| `26` | the Installed tab filtered on a dependency (`--query <name>`) |
| `27` | a numbered question answered into (4th number: a password prompt) |
| `18` | the rollback plan built from that transaction |
| `19` | paru's raw output at the end of a run (4th number = lines scrolled back) |
