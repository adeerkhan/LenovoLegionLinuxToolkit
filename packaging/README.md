# Packaging

Three routes to a distribution package, all built on the same `Makefile` and
`install.sh` that a local `make install` uses. `install.sh` derives every
destination from `PREFIX` and stages into `DESTDIR`, so no packaging recipe
carries its own copy of the file layout.

Before any of these, the version in `Cargo.toml` and the packaging files must
agree.

## Fedora / RHEL — RPM (`rpm/legiontoolkit.spec`)

```sh
make rpm
```

This archives `HEAD` to a source tarball, then runs `rpmbuild`. The result lands
in `target/rpmbuild/RPMS/`. The spec builds the helper without GTK (the
`Makefile` target does that) so a root process never links a GUI runtime.

For COPR, point a COPR project at the spec: COPR builds from a `git` SCM source
using `packaging/rpm/legiontoolkit.spec`. No changes are needed for that.

## Debian / Ubuntu — `debian/`

The standard `debian/` directory builds with `dpkg-buildpackage`:

```sh
dpkg-buildpackage -b -us -uc
```

`debian/rules` sets `PREFIX=/usr` and `DESTDIR`, and delegates to the upstream
`Makefile`. Note the GTK requirement: the build needs **GTK 4.22 /
libadwaita 1.9**, which Ubuntu ships from 25.10 (questing) onward, so target
that or newer.

## Arch — AUR (`aur/PKGBUILD`)

```sh
cd packaging/aur
makepkg -si
```

`makepkg` builds from the release tarball and installs into `$pkgdir` via
`DESTDIR`. Update `pkgver` (and regenerate `.SRCINFO`) for each release.

## What every package installs

- `legiontoolkit`, `legiontoolkit-keyboard-probe` → `/usr/bin`
- `legiontoolkit-helper` → `/usr/libexec`
- `org.legiontoolkit.helper.policy` → `/usr/share/polkit-1/actions`
- `legiontoolkit.desktop` and icons under the hicolor theme

The helper's path is baked into the GUI at build time and named in the polkit
action; both derive from `PREFIX`, so a package that sets `PREFIX=/usr` keeps
them in agreement. The optional no-password polkit rule
(`LEGIONTOOLKIT_NO_PASSWORD=1`) is for a manual `make install`, not packages.
