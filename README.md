# nalarch

A **TUI** package manager for Arch Linux, in the spirit of [nala](https://github.com/volitank/nala)
on the Debian side: see what is about to change, move around in it, decide, then apply.

*[Version française](README.fr.md).*

![nalarch in use: the plan screen states what a transaction will do and what deserves a look, the run screen retells each download, verification, upgrade and hook as it happens, then an update's changelog is opened — the verdict, Arch's packaging log, the upstream release notes — before the installed, orphan, history, search and cache tabs are toured.](docs/demo.gif)

## Principle

**libalpm for reading, paru for writing.**

No dependency resolution and no AUR build logic is reimplemented. nalarch reads the database
through `libalpm` — the same library pacman and paru use — shows the state, then delegates
every action to `paru`, which stays the sole owner of the transaction. It cannot diverge
from paru's behaviour, because paru is what acts.

paru does not run *beside* nalarch: it runs **inside** it, in a pseudo terminal. The
interface is never left, and paru's output is parsed and retold rather than relayed.

```
             ┌─ libalpm ────────────► state, plan, sizes, reverse dependencies
   nalarch ──┤
             └─ PTY ──► paru ──► vt100 ──► framed panel, step, prompts
```

Updates are detected with `checkupdates` rather than `pacman -Sy`: it syncs into a temporary
database, so there is no risk of leaving the system in a *partial upgrade*.

**[How it works](docs/design.md)** goes through the screens, the parsing and the reasoning in
detail — why the locale is pinned, how the changelog is found, what the transcript is made of.

## Features

| Tab | Contents | Action (`u`) |
|---|---|---|
| **Updates** | repositories + AUR, old → new version, download size | `paru -Syu` |
| **Installed** | explicit packages nothing depends on (≈ `pacman -Qett`) | `paru -Rns` |
| **Orphans** | pulled in as a dependency, needed by nothing now (= `pacman -Qdt`) | `paru -Rns` |
| **History** | every past transaction, and what a rollback would restore | `pacman -U` from the cache |
| **Search** | repositories and AUR in one list, with votes and maintainer | `paru -S` |
| **Cache** | volume, old versions, uninstalled packages | `paccache -rk<N>` / `U` → `-ruk0` |

Nothing runs without going through an approval screen first. It states what the transaction
will **really** do — including the dependencies it drags in, which `checkupdates` never
shows, and the removals `-Rns` cascades — next to the points that deserve a look: an AUR
build, a partial upgrade, a kernel update, a package still needed by something that stays.

The detail panel always shows **what depends on the selected package** (`Required by` /
`Optional for`) — the thing to look at before any removal.

When [rebuild-detector](https://archlinux.org/packages/extra/any/rebuild-detector/) is
installed, the Updates tab also flags **foreign packages broken by a library upgrade** —
the `-git` package that stops launching after a Qt or boost update, with no new version to
install because nothing changed upstream. `checkupdates` and `paru -Qua` are structurally
blind to this case: the fix is not an update but a rebuild, and `b` opens exactly that plan
(`paru -S --rebuild`).

**Installed** deliberately leaves out dependencies: 261 packages out of 1883 on a normal
machine. That is what makes it a list of *your* applications rather than a dump of the
system — but it also means searching it for a dependency finds nothing. The status line says
so, and names the package when it is installed after all. To ask "is this here?" about
anything at all, the Search tab covers every package, installed or not.

## History and rollback

The **History** tab reads `/var/log/pacman.log` rather than a journal of its own. Every
package operation on Arch goes through libalpm and lands in that file — `pacman`, `paru`, a
dependency pulled in by a script — so the history is complete and retroactive, including
what happened before nalarch existed. The `/` filter matches package names: it answers "when
did *this* package change, and to what?".

`u` builds the **inverse transaction** from the local caches: what was installed is removed,
what was upgraded goes back down, what was removed comes back. Nothing is downloaded. The
verdict — how many packages are still restorable — appears before the list of operations,
because on a five-hundred-package upgrade that is the part that decides.

Three things a package rollback does not do, and that the plan screen states:

- **It does not roll the system back.** The files the packages laid down go back down; what
  a scriptlet or a hook wrote since — a database migration, a rewritten config — stays as it
  is. For a real state rollback you need a snapshot (snapper/Btrfs).
- **It does not hold.** The next full upgrade brings the restored versions back up, unless
  `IgnorePkg` says otherwise.
- **It can manufacture an untested state.** When less than half of a large transaction is
  recoverable, the system ends up with a mix of both versions, never shipped nor tested that
  way. That is flagged as a serious risk, at the top of the list.

## Searching, and installing

`/` types a query, `Enter` runs it. The repositories are searched through libalpm, with no
subprocess to parse; the AUR through its RPC endpoint, on a background thread so a slow
lookup never freezes the interface.

Results are ordered by what was probably meant: the exact name first, then a prefix match,
then a substring. Within the same relevance the reviewed source wins, then the AUR's own
popularity. Ranking every repository package above every AUR one sounds prudent, but
searching `yazi` then buries the AUR package of that very name under an unrelated `libyazi`.

An AUR result carries what the AUR itself uses to judge one: votes, popularity, whether a
user flagged it out of date, and whether anyone still maintains it — **no maintainer means
orphaned**, which is the single most useful thing to know before building a PKGBUILD under
your own account.

`space` checks a result, `u` opens its plan. That plan separates what you **asked for** from
what comes along with it: asking for one package routinely brings a dozen, and that is
usually shown as a wall of names right before a confirmation prompt.

## The protection list

`~/.config/nalarch/keep.list`

libalpm only knows about **declared** dependencies. A package loaded dynamically — a Qt
plugin, a Wayland backend, a GStreamer plugin — therefore looks like an orphan while being
vital. On a Hyprland machine, `qt6-wayland` is the textbook case: `pacman -Qdtq` lists it,
and a blind `pacman -Rns $(pacman -Qdtq)` breaks the graphical shell.

Packages in this list show in yellow with a `[·]` mark and **cannot be checked for removal**.
The `p` key adds or removes protection on the selected package. The file is created on first
run with `qt6-wayland` and `qt6-avif-image-plugin` already protected.

## Keys

| Key | Effect |
|---|---|
| `↑` `↓` / `j` `k` | navigate (`PgUp`/`PgDn` by 10, `g`/`G` start/end) |
| `←` `→` / `h` `l` / `Tab` | change tab |
| `space` | check / uncheck |
| `a` / `n` | check all / uncheck all |
| `p` | protect / unprotect |
| `/` | filter (name and description; History: by package; Search: the query), `Esc` cancels |
| `c` | see what the selected update changes |
| `b` | (Updates tab) rebuild the foreign packages broken by a library upgrade |
| `u` | open the plan for the tab's action (History: the rollback) |
| `U` | (Cache tab) purge uninstalled packages |
| `r` | reload the state |
| `q` | quit |

On the plan screen: `Enter` launches, `Esc` cancels, `↑` `↓` walk the detail, and `v`
shows the AUR recipes (PKGBUILDs) from paru's clone cache — worth knowing because paru
only offers its own review when a recipe *changed* since the last build: an unchanged
PKGBUILD sails straight to "Proceed?" with nothing shown.

During a run, keystrokes go to paru, `Ctrl-C` included — its questions get answered and the
sudo password typed without leaving the interface. The movement keys are the exception: they
scroll whichever view is showing, before and after the run, and `j` switches between the
transcript and paru's raw output.

The transcript scrolls back through every operation, not just the ones that fit: a
seventy-five package upgrade is precisely when one wants to look at what happened earlier.
While following, new operations push the view along; once scrolled back it stays put, and
`End` starts following again.

## Icons

On by default, from the Material Design set carried by Nerd Fonts.

The one place they cannot work is a bare TTY — no patched font is loaded there and every
glyph comes out an empty box, which is also a moment nalarch is meant for. That case is
detected rather than left to the user: `TERM=linux` and its kin mean a console, and a console
cannot draw them whatever anyone configures. A multiplexer is not a console, so `tmux` and
`screen` keep them.

Outside a console the font cannot be interrogated, so `--no-icons` or `icons = false` in
`~/.config/nalarch/config` remain for anyone without a patched one. They assume a
**single-width (Mono)** Nerd Font variant — the double-width ones render across two cells
while the layout counts one, which shifts every column that follows.

Nothing they show carries meaning on its own: each glyph sits next to the word it decorates,
on the tabs and in the repository column. Turning them off loses decoration, not information.

## Language

English by default, French when the environment asks for it: `LC_ALL`, `LC_MESSAGES` and
`LANG` are consulted in that order, and `--lang en` / `--lang fr` overrides them.

English is the source language; `src/i18n.rs` holds the French table, and a test scans the
source tree for call sites with no entry, so a French interface cannot quietly grow English
sentences. This is unrelated to the locale forced on paru — see
[How it works](docs/design.md).

## Installing

From the AUR, once published (registration is temporarily paused upstream):

```bash
paru -S nalarch          # or nalarch-git
```

Until then, straight from this repository — the PKGBUILD pins the tag tarball digest:

```bash
git clone https://github.com/ksh2177/nalarch && cd nalarch/packaging/nalarch
makepkg -si
```

From source:

```bash
cargo build --release
install -Dm755 target/release/nalarch ~/.local/bin/nalarch
```

System dependencies: `pacman`, `paru`, `pacman-contrib` (for `checkupdates` and `paccache`),
`sudo`. The PKGBUILDs live in [`packaging/`](packaging/README.md).

Two modes help when there is nothing to upgrade: `nalarch --demo` replays a paru-shaped
session without touching the system, and `nalarch --dump` renders one screen as plain text
with no TTY. Both are described in [How it works](docs/design.md).

## Known limitations

- The rollback only covers packages. On a Btrfs system, `snap-pac` + `snapper` cover the
  complete filesystem state, which no package manager can do.
- What is no longer in the caches cannot be recovered: the `paccache` retention sets the
  real depth of usable history, however long the log is.
- A package absent from every configured repository is labelled `aur`; it may in fact have
  been built locally (`pacman -Qm` lists that set).
- The download size is only shown when the sync database already carries the target version;
  otherwise the field is omitted rather than showing a wrong value.

## License

MIT. See [LICENSE](LICENSE).

## How this was built

nalarch is a collaboration between a human and an AI, and I'd rather say it plainly
than let you find out from a commit trailer. I designed the tool, made the
architectural and UX decisions, review the code, run it daily on my own machine, and
maintain it. Most of the code itself was written in pair with Claude (Anthropic),
under that direction.

Tools in this space deserve scrutiny — nalarch sits in front of your package manager.
That is also why its design is deliberately conservative: libalpm is used read-only,
every write goes through paru running in a pseudo-terminal (no dependency resolution
and no build logic is reimplemented here), and partial-upgrade traps like a bare
`pacman -Sy` are avoided by design, with comments in the source explaining why.

Judge it like any other tool: by its code, its issue tracker, and its history from
here on. Bug reports are welcome.
