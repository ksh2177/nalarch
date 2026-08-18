//! Demo mode.
//!
//! An up-to-date system leaves no way to exercise the run screen: with no
//! pending update there is no progress bar to watch, no question to answer, no
//! build output to check. This mode replays a paru-like session, with its
//! pauses, its bars rewritten in place by carriage returns, and its prompts.
//!
//! It is not a mock of the interface: the script really runs in the pseudo
//! terminal and goes through the same emulator and the same stream analysis as
//! paru. Only the source of the text differs. No pacman command is invoked,
//! nothing is installed or removed.

use anyhow::{Context, Result};
use std::path::PathBuf;

const SCRIPT: &str = r#"#!/bin/sh
# Replays paru output in the C locale, exactly as nalarch parses it.
# Invokes no pacman command.

ligne() { printf '%s\n' "$1"; sleep "${2:-0.3}"; }

# Bar rewritten in place with carriage returns, like pacman's own.
barre() { # $1 libelle  $2 n  $3 m  $4 pas
  i=0
  while [ "$i" -le 100 ]; do
    pleins=$((i / 5))
    f=$(printf "%${pleins}s" '' | tr ' ' '#')
    v=$(printf "%$((20 - pleins))s" '' | tr ' ' '-')
    printf '\r(%s/%s) %s [%s%s] %3s%%' "$2" "$3" "$1" "$f" "$v" "$i"
    i=$((i + ${4:-20}))
    sleep 0.08
  done
  printf '\n'
}

# Download bar: file name, size, rate.
dl() { # $1 fichier  $2 taille  $3 unite
  i=0
  while [ "$i" -le 100 ]; do
    pleins=$((i / 5))
    f=$(printf "%${pleins}s" '' | tr ' ' '#')
    v=$(printf "%$((20 - pleins))s" '' | tr ' ' '-')
    printf '\r %s   %s %s  1863 KiB/s 00:00 [%s%s] %3s%%' "$1" "$2" "$3" "$f" "$v" "$i"
    i=$((i + 20))
    sleep 0.09
  done
  printf '\n'
}

printf '[sudo] password for %s: ' "$(id -un)"
stty -echo 2>/dev/null
read -r _motdepasse
stty echo 2>/dev/null
printf '\n'

ligne ':: Synchronizing package databases...'
ligne ' core is up to date' 0.15
ligne ' extra is up to date' 0.15
ligne ' multilib is up to date' 0.15
ligne 'warning: kitty: ignoring package upgrade (0.45.0-4 => 0.48.2-1)' 0.3

ligne ':: Retrieving packages...'
dl 'fastfetch-2.67.1-1-x86_64.pkg.tar.zst' '638.5' 'KiB'
dl 'bat-0.26.1-2-x86_64.pkg.tar.zst' '2.4' 'MiB'
dl 'libfoo-1.4.2-1-x86_64.pkg.tar.zst' '240.0' 'KiB'

ligne ':: Checking keyring...'
barre 'checking keys in keyring' 1 3 34
ligne ':: Checking integrity...'
barre 'checking package integrity' 2 3 34
ligne ':: Loading packages...'
barre 'loading package files' 3 3 34

ligne ':: Processing package changes...'
barre 'upgrading fastfetch' 1 3 25
barre 'upgrading bat' 2 3 25
barre 'installing libfoo' 3 3 25
ligne 'warning: /etc/pacman.conf installed as /etc/pacman.conf.pacnew' 0.3

ligne ':: Running post-transaction hooks...'
barre 'Arming ConditionNeedsUpdate' 1 3 50
barre 'Updating icon theme caches' 2 3 50
barre 'Updating the desktop file MIME type cache' 3 3 50

ligne ':: Looking for AUR upgrades...' 0.4
ligne ':: Looking for devel upgrades...' 0.4
sleep 0.5
"#;

/// The plan matching the script, so that the totals on screen mean something.
///
/// Without it the download block compares against a zero denominator and
/// announces "3/0".
pub fn plan() -> crate::plan::Plan {
    use crate::plan::{Kind, PlanRow};
    let line = |name: &str, from: &str, to: &str, dl: i64, kind| PlanRow {
        name: name.into(),
        repo: "extra".into(),
        from_version: from.into(),
        to_version: to.into(),
        dl: Some(dl),
        net: Some(dl / 4),
        aur: false,
        kind,
        is_downgrade: false,
    };
    let mut p = crate::plan::empty();
    p.rows = vec![
        line("fastfetch", "2.66.0-1", "2.67.1-1", 653_824, Kind::Upgrade),
        line("bat", "0.26.1-1", "0.26.1-2", 2_516_582, Kind::Upgrade),
        line("libfoo", "", "1.4.2-1", 245_760, Kind::New),
    ];
    p.total_dl = 3_416_166;
    p.net = 854_041;
    p
}

/// Writes the script to a temporary file and returns the command to spawn.
///
/// Going through a file rather than `sh -c` avoids escaping a whole script
/// inside one argument, and keeps `ps` output readable.
pub fn command() -> Result<Vec<String>> {
    let path: PathBuf = std::env::temp_dir().join("nalarch-demo.sh");
    std::fs::write(&path, SCRIPT).context("writing the demo script")?;
    Ok(vec![
        "sh".to_string(),
        path.to_string_lossy().into_owned(),
    ])
}
