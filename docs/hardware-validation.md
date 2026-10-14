# Hardware validation — loading the module and testing writes

This is the procedure for the one thing that cannot be verified from a sandbox:
loading the out-of-tree `legion_laptop` module on the target laptop and writing
to real hardware. It is written to be run by a person at the machine, one step
at a time, because a wrong write on this EC generation can leave the fans at
maximum until a reboot.

## What is already verified (no root)

- **The module builds against the running kernel.** On kernel
  `7.2.9-200.fc44` the tree produces `legion-laptop.ko` (1.7 MB) with
  `vermagic: 7.2.9-200.fc44.x86_64`, so it is loadable as-is:

  ```sh
  cd Reference/LenovoLegionLinux/kernel_module && make
  ```

- **Our sysfs names match the module's.** Every attribute the resolver in
  `src/paths.rs` uses is present in the built module: `igpumode`,
  `issupportigpumode`, `gsync`, `issupportgsync`, `winkey`, `overdrive`,
  `instant_boot_ac`, `instant_boot_usb_pd`, `rapidcharge`,
  `battery_conservation`, `fan_fullspeed`.

## What is verified now (2026-10-14)

- **The module loads.** `sudo make reloadmodule` bound `QNCN`, read EC `0x5508`,
  and initialised the keyboard / Y-logo / IO-port LED drivers. The capability
  report went from **38 to 47** readings.
- **Two bugs loading it surfaced are fixed**: the touchpad was wrongly
  attributed to `ideapad` (it is module/WMI here), and `issupport…` probes were
  read as `== 1` when they return non-zero (`issupportigpumode` = 3,
  `issupportgsync` = 2).

## What is not verified

**Every write.** Writes need root and touch the embedded controller; the write
path has not been exercised on hardware. Step 4 below is the procedure for that.

## Step 0 — prerequisites

- `sudo`.
- Kernel headers for the running kernel (present on Fedora as
  `kernel-devel`).
- The module built (above).

## Step 1 — load, and confirm it bound

```sh
cd Reference/LenovoLegionLinux/kernel_module
sudo make reloadmodule
sudo dmesg | grep -i legion
```

*Verified working on the reference laptop: config `QNCN`, EC id `0x5508`, no
"not in allowlist", LED drivers initialised.* If it refuses, that is the
allowlist working as intended; report the model/BIOS rather than forcing it.

## Step 2 — read-only checks, before any write

```sh
sensors
ls /sys/devices/platform/legion/legion/ | sort
cat /sys/firmware/acpi/platform_profile
```

Then the toolkit's own report, which needs no display:

```sh
cargo test --lib --no-default-features capability_report -- --nocapture
```

Expect the module-gated controls to move from `UNAVAILABLE` to readable. On the
reference this adds `winkey`, `overdrive`, `gsync`, `igpumode`,
`instant_boot_ac`, `instant_boot_usb_pd`, and the keyboard backlight / Y-logo /
IO-port LEDs.

## Step 3 — the application against real hardware

```sh
cargo run --release
```

- **System** should gain Windows Key, Display Overdrive and the two Instant Boot
  toggles.
- **Processors & GPU** should gain GPU Working Mode and G-Sync, *if*
  `issupportigpumode` / `issupportgsync` report `1`; if they report `0`, the
  cards correctly say the firmware does not back them.
- **Lighting** should gain the keyboard backlight, Y-logo and IO-port controls.
- **About** should report `legion_laptop loaded`.
- The escalation banner should be *absent* if the helper is installed.

Confirm the privileged path directly, as root:

```sh
sudo ./target/release/legiontoolkit-helper --check
```

It should print the helper path, the resolved `platform_profile` file, and
`writable yes` when run as root.

## Step 4 — writes, safest first, one at a time

Write one control, read it back, and confirm before moving on. Stop at the first
unexpected result.

1. **LEDs** — keyboard backlight, Y-logo, IO-port. Purely visual; nothing
   thermal. Good first proof that the whole pkexec → helper → sysfs chain works.
2. **Charging** — battery mode (normal / conservation / rapid). Reversible;
   the Fast/Long_Life exclusivity is enforced by the kernel.
3. **System toggles** — Windows key, touchpad, camera, always-on USB, Instant
   Boot. Reversible, no thermal effect.
4. **GPU working mode / G-Sync** — may need a reboot, and LLT documents that
   switching them repeatedly is unwise. Change once and observe.
5. **`fan_fullspeed` — last, and with care.** On this EC generation a write
   through the wrong path can hold the fans at maximum until the machine is
   rebooted. Only on AC, only with a reboot ready, and never as an automation
   step.

If a write reports failure, the message under the control carries pkexec's own
stderr and the helper's exit-code explanation — start there, not with a reboot.

## Step 5 — unload

```sh
sudo rmmod legion_laptop
```

To cycle it, `sudo make reloadmodule` from the module directory reloads it.

## Recording the result

`docs/hardware-compatibility.md` carries the verified matrix. When a control is
confirmed on real hardware, move it from "needs the module" to "verified", with
the BIOS and date, so the next reader knows what was measured rather than
assumed.
