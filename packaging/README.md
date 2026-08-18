# Packaging

Two PKGBUILDs, both targeting the AUR:

| Directory | Package | Source |
|---|---|---|
| `nalarch/` | `nalarch` | the release tarball of a `vX.Y.Z` tag |
| `nalarch-git/` | `nalarch-git` | the git repository, built from `HEAD` |

They are kept here, in the source repository, rather than only in the AUR repos:
that way a change to the build recipe travels with the change that made it
necessary. The AUR repositories stay the place they get published *to*.

## Before publishing

The AUR fetches sources over the public internet, so both files point at GitHub,
which is where the code has to be reachable from:

```
https://github.com/ksh2177/nalarch
```

Until that repository exists and carries a `v0.1.0` tag, neither PKGBUILD can be
built by anyone else. `nalarch-git` will work as soon as the repository is public;
`nalarch` also needs the tag and its release tarball.

## Publishing a release

```bash
# 1. Tag the release; the version must match Cargo.toml.
git tag -a v0.1.0 -m 'nalarch 0.1.0'
git push origin v0.1.0

# 2. Replace the placeholder digest with the real one.
cd packaging/nalarch
updpkgsums            # pacman-contrib

# 3. Check the recipe end to end, in a clean chroot rather than on your machine.
makepkg --syncdeps --cleanbuild --check

# 4. Regenerate the metadata the AUR indexes.
makepkg --printsrcinfo > .SRCINFO

# 5. Push to the AUR repository (separate from this one).
git clone ssh://aur@aur.archlinux.org/nalarch.git aur-nalarch
cp PKGBUILD .SRCINFO aur-nalarch/
cd aur-nalarch && git commit -am 'nalarch 0.1.0' && git push
```

`sha256sums=('SKIP')` is a placeholder, not a choice: a published PKGBUILD must
carry the real digest of its release tarball. `SKIP` is only legitimate for the
git source, whose integrity comes from the commit hash.

## Dependencies, and why

- **`pacman>=7.0`** — provides `libalpm.so`, which the `alpm` crate links against.
  The soname follows the pacman release: `alpm` 5.x wants `libalpm.so.16`, shipped
  by pacman 7.x. A bare `pacman` would let an incompatible version satisfy it.
- **`pacman-contrib`** — `checkupdates` (updates without touching the real
  database), `paccache` (cache figures and pruning), `pacdiff` (merging `.pacnew`).
- **`paru`** — every write action is delegated to it. Not optional: without paru,
  updating and removing do nothing.
- **`sudo`** — cache cleaning and rollbacks are invoked through it.

## Checking the build

`check()` runs the test suite. Those tests read no system state: they replay
captured pacman output and scan the source tree for untranslated strings. They are
safe in a clean chroot, which is why the step is kept rather than skipped.
