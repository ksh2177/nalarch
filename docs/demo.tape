# Recording of the README demo, replayed by vhs (`vhs docs/demo.tape`).
#
# It opens on `--demo`, whose script only replays paru-shaped output: pressing
# Enter on the plan screen in a recording must never touch a real system. Once
# the run is over, Enter returns to the table and the tabs get toured — reading
# the machine's state is harmless.
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
Type "nalarch --lang en --demo"
Enter
Sleep 2s
Show

# Long enough to read the points of attention, which is the whole idea.
Sleep 4500ms

Enter
Sleep 1800ms

# The demo script asks for a password the way sudo would.
Type "········"
Enter

# Downloads, verifications, the install, then the hooks.
Sleep 15s

# ── Back to the table: what is on the machine ──────────────────────────────
Enter
Sleep 1800ms

# Installed: explicit packages nothing depends on.
Tab
Sleep 1800ms

# Orphans: the two yellow dots are the protection list at work.
Tab
Sleep 2500ms

# History: every transaction pacman ever ran, and what a rollback could undo.
Tab
Sleep 2500ms

# Cache: what makes a rollback possible in the first place.
Tab
Sleep 2500ms

Hide
Type "q"
Sleep 500ms
