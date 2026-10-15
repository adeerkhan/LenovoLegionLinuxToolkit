# PPA — Ubuntu and Debian

`packaging/debian/` builds the source package; Launchpad builds it for each
Ubuntu series and serves it as a PPA. That is what makes the README's route work:

```sh
sudo add-apt-repository ppa:adeerkhan/legion-toolkit
sudo apt install legiontoolkit
```

## One-time setup

1. **A Launchpad account**, with an OpenPGP key uploaded to it. The key signs the
   upload; Launchpad re-signs the packages it publishes.
2. **Create the PPA** at <https://launchpad.net/people/+me/+activate-ppa>, named
   `legion-toolkit`. The URL becomes `ppa:adeerkhan/legion-toolkit`.
3. **Install the tooling**:

   ```sh
   sudo apt install devscripts debhelper dput-ng
   ```

## Each release

1. **Set the version and series** in `debian/changelog`. The series is the Ubuntu
   codename being built for; GTK 4.22 needs `questing` (25.10) or newer, so older
   series are not worth targeting:

   ```sh
   cd /path/to/LenovoLegionLinuxToolkit
   dch --newversion 0.1.0-1 --distribution questing "New upstream release."
   ```

2. **Build and sign the source package**:

   ```sh
   debuild -S -sa -k<YOUR_KEY_ID>
   ```

3. **Upload it**:

   ```sh
   dput ppa:adeerkhan/legion-toolkit ../legiontoolkit_0.1.0-1_source.changes
   ```

Launchpad then builds for every series the PPA is enabled for and publishes the
binary packages. Watch the build at <https://launchpad.net/~adeerkhan/+archive/ubuntu/legion-toolkit>.

## Notes

- `debian/rules` delegates the whole build to the upstream `Makefile` and
  `install.sh`, so there is no second copy of the layout to keep in step.
- The package declares `libgtk-4-1` and `libadwaita-1-0` as dependencies; on a
  series without GTK 4.22 the install will resolve but the binary will refuse to
  start, which is why the PPA targets 25.10+.
- The `debian/` directory builds a **native** source package (`source/format`),
  which is what a PPA expects: there is no separate upstream tarball to carry.
