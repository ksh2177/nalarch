# Recording of the README demo, replayed by vhs (`vhs docs/demo.tape`).
#
# It opens on `--demo`, whose script only replays paru-shaped output: pressing
# Enter on the plan screen in a recording must never touch a real system. Once
# the run is over, Enter returns to the table and the tabs get toured — reading
# the machine's state is harmless, and so is searching.
#
# Icons are on here because a recording is where they are worth showing. They
# are off by default, and the README says why.
#
# The theme is spelled out rather than left to the terminal so the GIF comes out
# identical wherever it is generated. nalarch itself takes its colours from the
# terminal's ANSI slots — that is the point of the theme section in the README —
# so any palette would do; this one just keeps the recording reproducible.

Output docs/demo.gif

Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 15
Set Width 1500
Set Height 680
Set Padding 20
Set Framerate 24
Set TypingSpeed 60ms

Set Theme { "background": "#1f1f22", "foreground": "#e4e1e5", "cursor": "#bdc4f0", "selection": "#353437", "black": "#424247", "brightBlack": "#55555c", "red": "#ffb4ab", "brightRed": "#ffd0c9", "green": "#96d5b8", "brightGreen": "#aff5b4", "yellow": "#ebcb8b", "brightYellow": "#f2cc60", "blue": "#bdc4f0", "brightBlue": "#d2d8ff", "magenta": "#dba0c4", "brightMagenta": "#b48ead", "cyan": "#88c0d0", "brightCyan": "#a5d6ff", "white": "#c6c5cf", "brightWhite": "#e4e1e5" }

# ── The plan: what the transaction will really do ──────────────────────────
Hide
Type "nalarch --lang en --icons --demo"
Enter
Sleep 2s
Show

# Long enough to read the points of attention, which is the whole idea, and no
# longer: this is also the frame the loop comes back to.
Sleep 3200ms

Enter
Sleep 1800ms

# The demo script asks for a password the way sudo would.
Type "········"
Enter
Sleep 2000ms

# paru's own resolution table, retold in nalarch's vocabulary, and the
# confirmation it asks for.
Sleep 2800ms
Type "Y"
Enter

# Downloads, verifications, the install, then the hooks. The script takes about
# nine seconds from here — measured, not guessed, so the finished screen gets a
# beat rather than five seconds of nothing.
Sleep 10500ms

# ── Back to the table: what is on the machine ──────────────────────────────
Enter
Sleep 1200ms

# What an update actually changes, before downloading it: the verdict first,
# then Arch's packaging log and the upstream release notes.
Type "c"
Sleep 2800ms
# The upstream release notes sit below the packaging log.
PageDown
Sleep 2500ms
Escape
Sleep 800ms

# Installed: explicit packages nothing depends on.
Tab
Sleep 1200ms

# Orphans: the two yellow dots are the protection list at work.
Tab
Sleep 1800ms

# History: every transaction pacman ever ran, and what a rollback could undo.
Tab
Sleep 1800ms

# Search: repositories and AUR in one list.
Tab
Sleep 1200ms
Type "/"
Type "yazi"
Enter
Sleep 3500ms

# Cache: what makes a rollback possible in the first place.
Tab
Sleep 1500ms

Hide
Type "q"
Sleep 500ms
