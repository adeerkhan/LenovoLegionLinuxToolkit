# Legion Toolkit

A native GTK 4 / libadwaita control panel for Lenovo Legion laptops.

Written in Rust, on GTK 4 and libadwaita — the toolkits that give a Linux
desktop application its native look and behaviour, so this reads as a
first-class citizen of the GNOME desktop rather than a port of a Windows
application.

**[Project plan](docs/PLAN.md)** · **[Hardware compatibility](docs/hardware-compatibility.md)** · **[Architecture](docs/architecture.md)**

---

## What it does

A dashboard that shows what your laptop can actually do, and lets you control it:

| Section   | Controls |
|-----------|----------|
| **Power**      | Power mode (quiet / balanced / performance / custom), maximum fan speed |
| **Thermal**    | CPU and GPU temperature, fan RPM |
| **Lighting**   | Keyboard backlight, Y-logo light |
| **Battery**    | Charge level and status, battery conservation |
| **System**     | Fn-lock, touchpad, camera power, always-on USB charging |

Readings are unprivileged. Individual writes escalate through polkit to a small
helper binary, so the application itself never runs as root.

### Capability-first, not model-first

Legion hardware differs enormously between models. This laptop, a Legion 5
15IRX10, has **no fan curve and no power limits at all**. Rather than show a
broken page, the application builds its UI from a capability report generated
at runtime, and a control that is unsupported explains *why* rather than
greying out mysteriously.

That is why the same binary works across the whole Legion range. Adding a
laptop that supports something new means adding a capability that reports
itself available — there is no `match model` anywhere in the presentation layer.

---

## Requirements

- Linux with **GTK 4.22 or newer and libadwaita 1.9 or newer** — the versions
  the `gtk4`/`libadwaita` crates are pinned to (Fedora 44 ships both)
- The `legion_laptop` kernel module (`LenovoLegionLinux`) for full control
- `polkit` for privileged writes

Anything backed by `ideapad_acpi` — battery conservation, Fn-lock, touchpad,
camera, always-on USB — works **without** the kernel module, so the dashboard
shows live data out of the box.

## Installation

### Any Linux — local install

The quickest route to a working, testable installation today:

```sh
git clone https://github.com/adeerkhan/legion-toolkit.git
cd legion-toolkit
make install
```

That builds both binaries and installs the polkit policy, desktop entry and
icon, with the helper placed exactly where the policy names it. The Makefile
owns every destination, so `make install PREFIX=~/.local` works too, and
`make uninstall` reverses it exactly.

`make distcheck` prints the layout without touching the system — the fastest
way to see where things will land before you commit to it.

### Fedora

Prerequisites, then the commands above:

```sh
sudo dnf install \
    gtk4-devel libadwaita-devel pango-devel \
    gdk-pixbuf2-devel cairo-gobject-devel graphene-devel
```

### Ubuntu / Debian

```sh
sudo apt install \
    libgtk-4-dev libadwaita-1-dev libpango1.0-dev \
    libgdk-pixbuf-2.0-dev libcairo2-dev libgraphene-1.0-dev
```

### Arch Linux

Not packaged yet — use `make install` below. An AUR `PKGBUILD` is planned.

### From a distribution package

Not yet in any distribution's repositories. An RPM for Fedora and a DEB for
Ubuntu are the next step; until then, `make install` above is the supported
route.

On Flatpak: it would be a single artifact for every distribution, but the
sandbox cannot write to `/sys`, and the privilege model here needs a root
helper outside that sandbox. For a hardware control panel, native packages are
the correct answer, so that is where the effort goes.

### Hardware permissions

Reading hardware needs no special permissions. Writing does, and the
application handles it by escalating per write: the first write in a session
prompts for your password, and polkit remembers the authorisation for the rest
of the session (`auth_admin_keep`).

To remove the prompt entirely for a user sitting at the machine, install the
no-password polkit rule:

```sh
sudo make install LEGIONTOOLKIT_NO_PASSWORD=1
```

That installs `49-legiontoolkit.rules`, which lets an `active` and `local`
session use the helper without authenticating. It grants nothing beyond the
capabilities this application already exposes — the helper's allowlist and its
destination-root check still apply — and it is reversible by deleting the file.

Alternatively, a udev rule can grant group write on the keyboard-backlight
attribute alone:

```sh
# /etc/udev/rules.d/99-legion-toolkit.rules
ACTION=="add|change", SUBSYSTEM=="leds", KERNEL=="platform::kbd_backlight*", MODE="0666"
```

then reload:

```sh
sudo udevadm control --reload-rules && sudo udevadm trigger
```

---

## Building from source

```sh
git clone https://github.com/adeerkhan/legion-toolkit.git
cd legion-toolkit
cargo build --release
sudo cargo install --path . --root /usr/local
```

Run without installing:

```sh
cargo run --release
```

### Useful commands

```sh
# Show the full capability set for this machine, including what is missing,
# using a mock backend — no hardware required.
LEGIONTOOLKIT_MOCK=1 cargo run --release

# Run the capability, path-resolution and security-policy tests.
# Needs no display installed.
cargo test --lib --no-default-features

# Determine whether this keyboard supports four-zone RGB. Read-only: it only
# ever issues HIDIOCGFEATURE, never a write.
sudo ./target/debug/legiontoolkit-keyboard-probe

# Check the privileged helper's allowlist.
./target/release/legiontoolkit-helper set <capability-key> <value>
```

---

## Current status

Early development. The read-only dashboard, the capability layer and the
privilege escalation path are complete and tested. Fan-curve editing is
architectured but not implemented, because the development machine does not
support fan curves — the honest thing was to build the suppression path first.

Verified on a Lenovo Legion 5 15IRX10 (83LY, BIOS QNCN38WW), kernel 7.2,
Fedora 44.

---

## Thanks

This project stands on the shoulders of a lot of reverse engineering done by
other people, and would not exist without either of these:

- **[LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux)** by
  John Fan — the kernel driver and userspace toolkit that makes every
  capability this application exposes possible. The footnotes in its
  `doc/FEATURES_AND_TESTING.md` were the single most useful reference in
  building the hardware layer here, and its documentation of the DMI allowlist
  and the fan-curve capability matrix is what made writing correct code on an
  undocumented machine possible.

- **[Lenovo Legion Toolkit](https://github.com/LenovoLegionToolkit-Team/LenovoLegionToolkit)** —
  the Windows original, and the reference for the feature set, information
  architecture and visual language this project reproduces on Linux. Its
  card-based design system and its habit of hiding unsupported controls rather
  than breaking them are the two ideas borrowed most directly.

  It was originally created by
  [Bartosz Cichecki](https://github.com/BartoszCichecki), whose repository has
  since been archived, and is now developed by the
  **LenovoLegionToolkit-Team**. The copy vendored under `Reference/` in this
  repository is theirs.

Also thanks to legendk95 on Discord for reverse-engineering how to talk to the
Legion keyboard in the first place, which is what makes the RGB probe useful to
anyone other than the person who wrote it.

---

## License

MIT — see [LICENSE](LICENSE).

The two projects above are licensed GPL-2.0 and GPL-3.0 respectively. No code
is copied from either; this project reimplements interfaces and behaviour, and
communicates across documented boundaries.
