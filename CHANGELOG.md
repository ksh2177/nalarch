# Changelog

All notable changes to nalarch. Dates use YYYY-MM-DD.

## Unreleased

- Updates tab flags foreign packages whose binaries link against libraries that
  no longer exist (the -git package broken by a Qt/boost upgrade, with no new
  version to install). Detection via `checkrebuild` (rebuild-detector) when
  present; `b` opens a `paru -S --rebuild` plan for exactly those packages.

- A partial update selection is now closed over the dependencies of the target
  versions: checking ncmpcpp alone while boost-libs stayed unchecked produced an
  unresolvable transaction (`--ignore` turned it into a partial upgrade). The
  required updates are pulled in automatically and listed in the plan notes.

## [0.2.1] — 2026-08-18

- `--version` answers on stdout instead of opening the interface.

## [0.2.0] — 2026-08-18

- paru prompts are answered from the UI: no more questions in the terminal that
  were already answered by clicking, and what gets typed at paru is shown.
- Update plan explains *what a change does* before showing the evidence.
- Help text no longer crashes when piped.
- Transcript can be walked back entirely, not just its tail.
- Search-and-install flow, and glyph rendering on request.

## [0.1.0] — 2026-08-18

- Initial release: TUI package manager for Arch. libalpm for reading, paru for
  writing — no dependency resolution and no AUR build logic reimplemented here;
  paru runs inside a pseudo-terminal. Updates view, history and rollback,
  protection list, orphan handling, cache retention from system configuration,
  French and English interface.
