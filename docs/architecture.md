# Architecture

## Layers

```
GUI binary (src/main.rs)
├── application.rs     AdwApplication, activates the window
├── window.rs          Sidebar navigation, capability-driven pages, refresh loop
└── design/
    ├── mod.rs         Stylesheet, light/dark handling, accent colour
    ├── card.rs        LltCard + LltCardHeader (the content model)
    └── feature_card.rs  Renders one Capability, refreshes itself

library (src/lib.rs) — no GTK dependency
├── capability.rs      Capability descriptor, Kind, Group, Value, Builder
├── paths.rs           sysfs/hwmon/leds resolution, kernel-version branching
├── policy.rs          The privileged helper's allowlist, and its invariant test
├── services/
│   └── privilege.rs   pkexec invocation of the helper
└── backends/
    ├── mod.rs         Backend trait, driver_status()
    ├── sysfs.rs       Native sysfs/hwmon backend
    └── mock.rs        Offline backend mirroring this laptop's capability set

helper binary (src/bin/legiontoolkit-helper.rs)
└── Runs as root via pkexec. Validates against policy, writes one file.
```

The library deliberately has no GTK dependency (see the `gui` feature in
`Cargo.toml`). That buys two things: the capability layer, path resolution and
security policy are testable and usable without a display, and the privileged
helper cannot pull a GTK runtime into a process that runs as root.

## The capability descriptor

```rust
pub struct Capability {
    pub key:             &'static str,   // stable, persisted in profiles
    pub label:           String,
    pub icon:            &'static str,
    pub kind:            Kind,           // Toggle | Enum | Slider | ReadOnly | Custom
    pub group:           Group,          // Power | Thermal | Lighting | Battery | System
    pub values:          Vec<NamedValue>,// enums, from sysfs `choices`
    pub limits:          Option<Limits>, // min / max / step
    pub unit:            Option<&'static str>,
    pub note:            Option<&'static str>,
    pub unavailable_reason: Option<&'static str>,
    read:  Box<dyn Fn() -> Result<Value> + Send + Sync>,
    write: Option<Box<dyn Fn(Value) -> Result<()> + Send + Sync>>,
}
```

The read and write closures capture their own backing path, so a capability is
self-contained. The UI never sees a filesystem path.

`unavailable_reason` is the important field. It is set **before** any widget is
built, so an unsupported capability renders as one sentence of explanation
rather than a disabled control. The distinction it carries is not cosmetic —
`"Requires the legion_laptop driver"` and `"Not supported by this model"` need
completely different actions from the user, and a greyed-out widget cannot tell
them apart.

## Why a `Backend` trait

Three reasons, in order of importance:

1. **Testable UI.** `MockBackend` mirrors this laptop's capability set, so the
   dashboard can be developed and screenshotted without the machine. The mock
   deliberately *includes* the capabilities this laptop lacks (fan curve, power
   limits) rendered as unavailable — so if a control appears in the mock that
   should be suppressed on real hardware, the mock is lying and the bug is
   caught immediately.
2. **Bounded reimplementation.** LLL's `legion.py` holds fan-curve RPM↔PWM
   conversion logic across five different backends, level-clamp tables and YAML
   preset schemas. We are not going to re-derive that from a reference repo.
   `SysfsBackend` covers what is native; when fan curves are needed, an
   `LllCliBackend` bridging to `legion_cli set-feature` can serve them without
   the UI changing at all.
3. **No model branching in the UI.** There is no `match model` anywhere in the
   presentation layer. Adding a laptop that supports X means adding a
   capability that reports itself available.

## Privilege

```
GUI (unprivileged, user session)
  │  writes sysfs files?            no — needs root
  │
  └─→ pkexec legiontoolkit-helper set <key> <value>
          │  auth_admin (polkit prompt)
          ↓
      helper (root)
          ├─ reject if key not in policy::HELPER_WRITABLE
          ├─ reject a value that is not a plausible sysfs token
          ├─ resolve the path through the same code the GUI uses
          └─ write exactly one file
```

`pkexec` is used rather than setuid, so the kernel handles the transition and
polkit handles the authorisation. See `packaging/org.legiontoolkit.helper.policy`
for the action definition, and note its `org.freedesktop.policykit.exec.path`
annotation — it must match the helper's install path exactly or every write
reports as refused.

The policy uses `auth_admin`, not `auth_admin_keep`. Writing to the embedded
controller is not something to silently remember; someone changing a fan curve
can re-authenticate, and someone flipping a toggle from an automation profile
must not end up with a permanently authorised session.

### The invariant that keeps this honest

`src/policy.rs::tests::every_writable_capability_is_allowed` asserts that every
capability the sysfs backend installs a write closure for appears in the helper's
allowlist. Without it, adding a capability to the GUI would silently produce a
control that reports failure for reasons invisible to the user — the worst
failure mode a control panel has.

## Refresh model

Capabilities are read on the GTK main thread by a one-second timer, and once
before the first paint. These are sysfs reads: microseconds, no I/O contention,
nothing to justify a worker thread and the channel that would come with it. When
a step *does* warrant one (a fan-curve write touches ~120 files individually), it
can move to a worker then.

A read failure blanks the value rather than removing the card. A file that
vanished this instant is far more likely to reappear than to mean the hardware
permanently lost the feature.

## Path resolution

`src/paths.rs` is where model-specific knowledge lives, and it is the likeliest
source of "it doesn't work on my machine" bugs. Three things it handles that a
hardcoded path would get wrong:

1. **Kernel 7.0 renamed the platform directory** from
   `drivers/platform:legion/PNP0C09:00` to `drivers/platform:legion/legion`.
   Resolve by kernel version, keep the other as a fallback.
2. **hwmon is globbed**, not named — `hwmonN` is not stable across boots.
3. **Several controls live under `ideapad_acpi`, not legion**: battery
   conservation, Fn-lock, touchpad, camera power, always-on-USB charging.
   Probing only the legion tree reports all five as unsupported on machines
   where they work fine.

## Coding conventions

- No `unwrap()` in library or UI code. Use `?` and let the caller decide; a
  capability read that fails should degrade, not panic the window.
- Every capability `key` is a stable `&'static str` and is persisted in profiles,
  so keys are never renamed without a migration.
- Doc comments explain *why* a decision was made, not what the line does. The
  reference repos are vendored precisely so these decisions can be checked.
- `cargo test --lib --no-default-features` must pass without GTK installed.
