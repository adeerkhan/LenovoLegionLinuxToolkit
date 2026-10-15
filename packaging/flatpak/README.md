# Flatpak

A Flatpak build, so the application can be installed from a repository — or from
a single `.flatpakref` click — instead of from source.

Two files carry it:

- `org.legiontoolkit.Ltk.yml` — the manifest. It builds the application, the
  privileged helper and the keyboard probe from a git checkout and installs
  them under `/app`.
- `org.legiontoolkit.Ltk.flatpakref` — the one-click install file. It names a
  published repository, so a user runs

  ```sh
  flatpak install --from https://…/org.legiontoolkit.Ltk.flatpakref
  ```

  or just opens the file in GNOME Software.

## Prerequisites

```sh
flatpak install flathub org.gnome.Sdk//50 org.gnome.Platform//50 \
    org.freedesktop.Sdk.Extension.rust-stable
sudo dnf install flatpak-builder            # or: sudo apt install flatpak-builder
python3 -m pip install --user aiohttp       # for the crate-source generator
```

The runtime must ship **GTK 4.22 / libadwaita 1.9** — `org.gnome.Platform` 50
does. Older branches fail the gtk4 version check at build time.

## Build and try it

```sh
./packaging/flatpak/generate-cargo-sources.sh   # once, and after a dependency change
make flatpak                                    # builds and exports to target/flatpak-repo

flatpak --user remote-add --no-gpg-verify legiontoolkit file://$PWD/target/flatpak-repo
flatpak --user install legiontoolkit org.legiontoolkit.Ltk
flatpak run org.legiontoolkit.Ltk
```

`generate-cargo-sources.sh` writes `cargo-sources.json`, the offline crate list
Flatpak builds need. Commit it: a Flatpak build has no network.

## Publish it

1. Build into a repository (`make flatpak` already does the export).
2. Sign it, so installs are verified:

   ```sh
   flatpak build-update-repo --generate-static-deltas --gpg-sign=KEYID target/flatpak-repo
   flatpak build-sign target/flatpak-repo org.legiontoolkit.Ltk --gpg-sign=KEYID --update-appstream
   ```

3. Host `target/flatpak-repo` anywhere static — GitHub Pages works:

   ```
   https://adeerkhan.github.io/LenovoLegionLinuxToolkit/flatpak/repo/
   ```

4. Put the base64 public key in `GPGKey=` of the `.flatpakref` (or drop the file
   and hand users `flatpak remote-add --if-not-exists legiontoolkit <url>` plus
   `flatpak install legiontoolkit org.legiontoolkit.Ltk`).

See [Flatpak repositories](https://docs.flatpak.org/en/latest/repositories.html)
for the repository layout and
[Flatpak file formats](https://docs.flatpak.org/en/latest/flatpakref-files.html)
for the `.flatpakref` keys.

## What the sandbox can and cannot do

The application reads hardware and writes hardware, and the two are different
problems in a sandbox.

**Reads work fully.** Every reading comes from `/sys` and `/proc`, and the
manifest grants `--filesystem=host:ro` for exactly that. The Home sensor row,
the Battery page and the temperature and fan readings are all there.

**The RGB keyboard works fully.** The keyboard speaks over `/dev/hidraw*`, which
`--device=all` exposes, and the project's udev rule already grants the session
access to that node — no privilege involved.

**Privileged writes need the helper on the host.** Every writable sysfs
attribute is `0644 root`, so nothing in the sandbox can write it, root or not.
The application already escalates per write to a host helper through `pkexec`;
inside the Flatpak it reaches that same host helper through
`flatpak-spawn --host` (see `src/services/privilege.rs`). Install the helper
once with any of the native packages — the RPM, the DEB, the AUR `PKGBUILD`, or
`sudo make install` — and the Flatpak's toggles, sliders and the RGB editor all
work. Without it the application still runs, reads everything, and says plainly
that writes are unavailable.

So a complete install is:

```sh
flatpak install --from org.legiontoolkit.Ltk.flatpakref   # the application
git clone https://github.com/adeerkhan/LenovoLegionLinuxToolkit.git
cd LenovoLegionLinuxToolkit && sudo make install          # once: the host helper
```

The second step installs the helper, its polkit action and the udev rule from
source; it can be an RPM, DEB or AUR package instead. It is a one-time step per
machine.

**GPU telemetry from `nvidia-smi` is unavailable.** `nvidia-smi` is the host's
NVIDIA driver, not something the sandbox has. AMD GPUs are read from sysfs and
are unaffected; on NVIDIA the GPU card reads blank.
