# Packaging

Every route is built from the same `Makefile` and `install.sh` a local
`make install` uses. `install.sh` derives every destination from `PREFIX` and
stages into `DESTDIR`, so no packaging recipe carries its own copy of the file
layout — and the helper always lands where the polkit action names it.

Before any of these, the version in `Cargo.toml` and the packaging files must
agree.

| Route | Source here | Published as |
|---|---|---|
| Fedora / RHEL | `rpm/legiontoolkit.spec` | a COPR project |
| Ubuntu / Debian | `debian/` | a PPA |
| Arch | `aur/PKGBUILD` | the AUR |
| Any Linux | `flatpak/` | a Flatpak repository |

A tagged release builds the RPM and the DEB and attaches them to the GitHub
release — see `.github/workflows/release.yml`.

## Fedora / RHEL — RPM and COPR

```sh
make rpm
```

This archives `HEAD` to a source tarball, then runs `rpmbuild`. The result lands
in `target/rpmbuild/RPMS/`. The spec builds the helper without GTK (the
`Makefile` target does that) so a root process never links a GUI runtime.

**COPR** builds the same spec from git, which is what makes the README's route
work:

```sh
sudo dnf copr enable adeerkhan/legion-toolkit
sudo dnf install legiontoolkit
```

One-time setup:

1. A [Fedora account](https://accounts.fedoraproject.org/).
2. Create the project at <https://copr.fedorainfracloud.org/coprs/add/> with:
   - **SCM**: `https://github.com/adeerkhan/LenovoLegionLinuxToolkit.git`
   - **Spec file**: `packaging/rpm/legiontoolkit.spec`
   - **Build for**: `fedora-44-x86_64` (and newer)
   - Enable the GitHub webhook so a tag builds automatically.

## Ubuntu / Debian — PPA

`packaging/debian/` is the source package; Launchpad builds it and serves it as a
PPA:

```sh
sudo add-apt-repository ppa:adeerkhan/legion-toolkit
sudo apt install legiontoolkit
```

`ppa/README.md` has the one-time Launchpad setup and the per-release
`debuild`/`dput` steps. To build locally instead:

```sh
dpkg-buildpackage -b -us -uc
```

`debian/rules` sets `PREFIX=/usr` and `DESTDIR` and delegates to the upstream
`Makefile`. Note the GTK requirement: the build needs **GTK 4.22 / libadwaita
1.9**, which Ubuntu ships from 25.10 (questing) onward, so target that or newer.

## Arch — AUR (`aur/PKGBUILD`)

```sh
cd packaging/aur
makepkg -si
```

`makepkg` builds from the release tarball and installs into `$pkgdir` via
`DESTDIR`. Update `pkgver` (and regenerate `.SRCINFO`) for each release, then
push to the AUR.

## Flatpak (`flatpak/`)

The manifest, an AppStream file and a `.flatpakref`:

```sh
./packaging/flatpak/generate-cargo-sources.sh   # once, and after a dependency change
make flatpak                                     # builds and exports target/flatpak-repo
```

`flatpak/README.md` covers the runtime, the sandbox's limits, and publishing the
repository (GitHub Pages works). The privilege model is the interesting part: the
sandbox reads `/sys` and drives the RGB keyboard, and reaches the **host** helper
through `flatpak-spawn --host` for writes.

## What every package installs

- `legiontoolkit`, `legiontoolkit-keyboard-probe` → `/usr/bin`
- `legiontoolkit-helper` → `/usr/libexec`
- `org.legiontoolkit.helper.policy` → `/usr/share/polkit-1/actions`
- `legiontoolkit.desktop`, and the icons under the hicolor theme
- `99-legiontoolkit-hidraw.rules` → `/etc/udev/rules.d` (grants the session the
  RGB keyboard's hidraw node)

The helper's path is baked into the GUI at build time and named in the polkit
action; both derive from `PREFIX`, so a package that sets `PREFIX=/usr` keeps them
in agreement. The optional no-password polkit rule
(`LEGIONTOOLKIT_NO_PASSWORD=1`) is for a manual `make install`, not packages.
