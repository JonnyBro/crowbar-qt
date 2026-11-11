# Releasing

How to publish a new version. `X.Y.Z` below is the new version, e.g. `1.2.1`.

## While working

- Write commits as [Conventional Commits](https://www.conventionalcommits.org/): `feat: ...`, `fix: ...`,
  `refactor: ...`, `docs: ...`, `perf: ...`, `chore: ...`, `ci: ...`, optionally with a scope, `feat(pack): ...`.
  Only these end up in the release notes; messages like `lint` or `wip` are left out.
- Every push and pull request runs `.github/workflows/ci.yml` (build and tests on Arch). Keep `main` green.
- If you changed user-visible text, refresh the translation and fill in the new Russian strings
  (Qt Linguist: `linguist6 translations/crowbar-qt_ru.ts`):

  ```bash
  cmake --build build --target update_translations
  ```

## 1. Bump the version

Pick the number by [SemVer](https://semver.org/): `Z` for fixes, `Y` for new features, `X` for breaking changes.
Set it in both places:

- `CMakeLists.txt`: `project(crowbar-qt VERSION X.Y.Z LANGUAGES CXX)`
- `packaging/aur/crowbar-qt-bin/PKGBUILD`: `pkgver=X.Y.Z`, and reset `pkgrel=1`

`packaging/aur/crowbar-qt-git/PKGBUILD` works its version out by itself; its `pkgver=` line only needs to match
for `.SRCINFO`, so bump it too.

## 2. Commit and tag

Use `chore(release):` so the release commit itself stays out of the notes.

```bash
git commit -am "chore(release): vX.Y.Z"
```

```bash
git tag -s vX.Y.Z -m "vX.Y.Z"
```

```bash
git push origin main vX.Y.Z
```

The tag must be `v` plus exactly the version in `CMakeLists.txt`, or the release workflow stops with an error.

## 3. Wait for the release workflow

`.github/workflows/release.yml` runs on the tag (Actions tab on GitHub):

1. `appimage` builds `Crowbar_Qt-X.Y.Z-x86_64.AppImage` on Ubuntu 22.04.
2. `arch` builds `crowbar-qt-X.Y.Z-x86_64.tar.zst` against Arch's Qt; `crowbar-qt-bin` installs this.
3. `release` writes the notes with git-cliff (`cliff.toml`) and publishes both files plus `SHA256SUMS`.

Check the release page: both files and `SHA256SUMS` are attached, and the notes list the commits since the
previous tag.

### If it fails

Fix the problem, then move the tag to the fixed commit. Delete the GitHub release first if one was created.

```bash
git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z
```

Then commit the fix and repeat step 2 from `git tag`.

## 4. Update the AUR packages

### crowbar-qt-bin (every release)

The checksum can only be filled in once the release exists:

```bash
cd packaging/aur/crowbar-qt-bin && updpkgsums && makepkg --printsrcinfo > .SRCINFO
```

Try it locally, then commit `PKGBUILD` and `.SRCINFO` here:

```bash
makepkg -si
```

Copy `PKGBUILD` and `.SRCINFO` into your clone of `ssh://aur@aur.archlinux.org/crowbar-qt-bin.git`, commit there
and push. First time only, clone it:

```bash
git clone ssh://aur@aur.archlinux.org/crowbar-qt-bin.git
```

### crowbar-qt-git (only when its PKGBUILD changes)

It always builds the newest commit, so a release needs nothing here. Push it to
`ssh://aur@aur.archlinux.org/crowbar-qt-git.git` only after changing the PKGBUILD itself (dependencies, build
steps), together with a fresh `.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO`).

### Changing only a PKGBUILD

If a package needs fixing without a new program version (a missing dependency, say), raise `pkgrel` by one
instead of `pkgver`, regenerate `.SRCINFO` and push to the AUR.

## Checklist

- [ ] Version bumped in `CMakeLists.txt` and both PKGBUILDs (`pkgrel=1`)
- [ ] Translations up to date
- [ ] `chore(release): vX.Y.Z` committed, `vX.Y.Z` tagged and pushed
- [ ] Release workflow green; AppImage, `.tar.zst` and `SHA256SUMS` attached
- [ ] `crowbar-qt-bin`: `updpkgsums`, `.SRCINFO`, `makepkg -si`, pushed here and to the AUR
