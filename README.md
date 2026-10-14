# Legion Toolkit

A native GTK 4 / libadwaita control panel for Lenovo Legion laptops.

Written in Rust, on GTK 4 and libadwaita — the toolkits that give a Linux
desktop application its native look and behaviour, so this reads as a
first-class citizen of the GNOME desktop rather than a port of a Windows
application.

**[Project plan](docs/PLAN.md)** · **[Hardware compatibility](docs/hardware-compatibility.md)** · **[Architecture](docs/architecture.md)** · **[Hardware validation](docs/hardware-validation.md)**

---

## What it does

A dashboard that shows what your laptop can actually do, and lets you control it:

| Section   | Controls |
|-----------|----------|
| **Power**      | Power mode (quiet / balanced / performance / custom), maximum fan speed |
| **Thermal**    | CPU and GPU temperature, fan RPM |
| **GPU**        | Working mode (hybrid / integrated only / hybrid auto), G-Sync |
| **Lighting**   | Keyboard backlight, Y-logo light, IO-port light |
| **Battery**    | Charge level and status, battery mode (normal / conservation / rapid) |
| **System**     | Fn-lock, touchpad, camera power, always-on USB charging, Windows-key lock, Instant Boot |
| **Automation** | Pipelines that apply settings on AC/battery, a timer, or a power-mode change |

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

`make install` builds the application and its privileged helper, installs the
polkit policy, desktop entry, icons and a udev rule, and places the helper
exactly where the polkit policy names it. The Makefile owns every destination, so
it is reversible with `make uninstall` and relocatable with a different `PREFIX`.

### 1. Prerequisites

GTK 4.22 / libadwaita 1.9 and a Rust toolchain (1.85 or newer):

```sh
# Fedora
sudo dnf install gtk4-devel libadwaita-devel pango-devel \
    gdk-pixbuf2-devel cairo-gobject-devel graphene-devel

# Ubuntu / Debian — 25.10 "questing" or newer; older releases lack GTK 4.22
sudo apt install libgtk-4-dev libadwaita-1-dev libpango1.0-dev \
    libgdk-pixbuf-2.0-dev libcairo2-dev libgraphene-1.0-dev
```

### 2. Install

```sh
git clone https://github.com/adeerkhan/LenovoLegionLinuxToolkit
cd LenovoLegionLinuxToolkit
sudo make install
legiontoolkit            # or launch it from your app menu
```

Run `make distcheck` first to print the install layout without touching the
system, and use `make install PREFIX=~/.local` to install into your home
directory instead.

`make install` also installs the udev rule that lets your user open the RGB
keyboard's hidraw node, so the **Keyboard RGB** card works unprivileged. A
`$HOME` install cannot write `/etc/udev/rules.d`; install that one file manually:

```sh
sudo install -Dm644 packaging/99-legiontoolkit-hidraw.rules \
    /etc/udev/rules.d/99-legiontoolkit-hidraw.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

### 3. Privileged writes

Reads need no permissions. Each write escalates through polkit to the helper: the
first write in a session prompts for your password and polkit then remembers the
authorisation (`auth_admin_keep`), so you are not prompted per change.

To drop the prompt for a user sitting at the machine, install the no-password rule
at the same time as the application:

```sh
sudo make install LEGIONTOOLKIT_NO_PASSWORD=1
```

That installs `49-legiontoolkit.rules`, which lets an `active` and `local`
session use the helper without authenticating. It grants nothing beyond the
capabilities this application already exposes — the helper's allowlist and its
destination-root check still apply — and it is reversible:

```sh
sudo rm /etc/polkit-1/rules.d/49-legiontoolkit.rules
```

### 4. Optional: the kernel module

`legion_laptop` from [LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux)
adds the keyboard backlight, Y-logo and IO-port LEDs, GPU working mode, G-Sync,
Windows-key lock and Instant Boot. Everything backed by `ideapad_acpi` — battery,
Fn-lock, touchpad, camera, always-on USB — and the whole Home sensor row work
without it.

```sh
cd Reference/LenovoLegionLinux/kernel_module
make
sudo make reloadmodule
```

### 5. Uninstall

```sh
sudo make uninstall        # binaries, policy, desktop entry, icons and rules
```

### Packages and Flatpak

Not yet in any distribution's repositories. `packaging/` holds an RPM spec (build
with `make rpm`), a Debian `debian/` directory and an AUR `PKGBUILD`. Flatpak is
deliberately not used: it cannot write to `/sys`, and this privilege model needs a
root helper outside the sandbox.

---

## Building from source

```sh
git clone https://github.com/adeerkhan/LenovoLegionLinuxToolkit.git
cd LenovoLegionLinuxToolkit
cargo build --release
```

`make install` is the supported route; `sudo cargo install` cannot install the
polkit action or the udev rule that the helper and the RGB card need.

Run without installing — reading hardware needs no privileges:

```sh
cargo run --release
```

### Useful commands

```sh
# Render the UI with no hardware, using the mock backend.
LEGIONTOOLKIT_MOCK=1 cargo run --release

# The capability, path-resolution, policy and RGB-protocol tests.
# Needs no display installed.
cargo test --lib --no-default-features

# The full gate this project must pass.
make check

# Print the capability report for this machine, including what is missing.
cargo test --lib --no-default-features capability_report -- --nocapture

# Build a distribution RPM from the current commit.
make rpm

# Render a page to a PNG offline (no display needed).
LEGIONTOOLKIT_SHOT=/tmp/home.png LEGIONTOOLKIT_PAGE=home cargo run --release
```

---

## Current status

Working: the capability layer, the read-only dashboard, per-write privilege
escalation, presets, battery charge mode, the 24-zone RGB keyboard (colour,
effects and presets), settings, and the four-card Home dashboard.

Not implemented: fan-curve editing (this machine's firmware has no fan curve —
the suppression path is built instead), automation, and the firmware's own
`Fn+Space` lighting slots, which the USB protocol does not expose.

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

  The copy vendored under `Reference/` in this repository is theirs.

- **[keyRGB](https://github.com/Rainexn0b/keyRGB)** by Rainexn0b — the Linux
  tray app whose reverse engineering of the Lenovo Gen10 24-zone ITE keyboard
  controller is the basis of this project's own RGB implementation
  (`src/rgb.rs`). No keyRGB code is copied; the USB report layout is
  reimplemented from its published backend, and this is our thanks.

Also thanks to legendk95 on Discord for reverse-engineering how to talk to the
Legion keyboard in the first place, which is what makes the RGB probe useful to
anyone other than the person who wrote it.

---

## License

MIT — see [LICENSE](LICENSE).

The two projects above are licensed GPL-2.0 and GPL-3.0 respectively. No code
is copied from either; this project reimplements interfaces and behaviour, and
communicates across documented boundaries.
