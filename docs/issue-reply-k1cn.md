# Issue reply — Legion 7 16IAX7 (82TD, K1CN48WW) compatibility report

Draft reply to the reporter. Post once the fix release is built; attach the
RPM or point at the release tag.

---

Thank you — this is the most complete report the project has received, and
every claim in it checked out. I verified each item against the toolkit source,
against a live sysfs on the reference machine, and against the
LenovoLegionLinux driver source (`model_k1cn`, `legion-laptop.c`). Findings and
fixes below; everything ships in the next build.

## Confirmed and fixed

| # | Your report | Verdict | Fix |
|---|---|---|---|
| 1 | Fn Lock / Camera / Always-On USB writes refused with "outside the writable roots" | **Confirmed — affects every machine.** The helper's writable root was spelled `.../drivers/ideapad`, but the files live under `.../ideapad_acpi/VPC2004:00` (or `ideapad_laptop`), and the path check compares whole path components, so the root matched nothing | Roots corrected to the two real driver directories; a regression test now fails if a capability's backing file is outside the roots again |
| 2 | Custom power mode: `EINVAL` on `/sys/firmware/acpi/platform_profile` | **Confirmed.** Since kernel 6.14 the legacy file rejects `custom` by design; the value must go to the handler's class device — on your machine the gamezone/legion one under `/sys/class/platform-profile/` | `custom` is now written to the class device of a handler whose `choices` include it; standard modes keep the legacy file |
| 3 | G-Sync toggle snaps back, no helper call in the journal | **Confirmed — a real silent drop in the app.** A click landing inside the once-per-second refresh window was swallowed by the "programmatic update" guard: no write, no error, and nothing in the log to show it | The guard now distinguishes refresh echoes from user clicks by value, so a click during a refresh is applied, not dropped; a failed write puts the toggle back immediately with the error; **every failed helper call is now logged** (key, value, reason), so debug logs capture what red text alone used to carry |
| 4 | Keyboard Backlight reason says "Requires the legion_laptop driver" though the driver is loaded | **Confirmed.** Your keyboard is per-key RGB (ITE `048d:c978`); the driver aborts the zone-backlight LED registration when the WMI brightness read fails, so no LED ever appears. The old reason sent you chasing the wrong fix | The reason now reads "The driver is loaded, but this keyboard registers no zone-backlight LED (per-key RGB keyboards use a separate controller)" |
| 5 | Fan Curve hidden as "not supported by this model's firmware" for every machine | **Confirmed — it was hard-coded.** `model_k1cn` has `access_method_fancurve = ACCESS_METHOD_WMI3`: your machine exposes the standard hwmon `pwm*_auto_point*` interface and writing works | The reason is now probed: on your machine it will say the curve is exposed and an editor is coming; only machines with no curve get the "not supported" text |
| 6 | Camera does not follow the hardware switch | **Confirmed as a limitation, not a bug we can fix here.** `camera_power` is the software power bit; the physical switch state is not reported through sysfs by either driver | The control now says so on the card, so the two can't be confused |
| 7 | Battery labels while charging; conservation "60%" hint; GPU-mode explanations; About export; ANSI in logs; update-check 404 | **All taken** | "Time to Full"/"Charge Rate" retitle themselves while charging; the conservation note now says 60–80% depending on model; GPU Working Mode and G-Sync cards explain the options and their interaction; the About page gained **Copy report** (whole page as text); logs go to stderr and drop colour when piped, and CONTRIBUTING now recommends `NO_COLOR=1`; the 404 is logged at debug level (it just means no release is published yet — harmless) |

## What we need from you

Two outputs to confirm the platform-profile routing on kernel 7.2, since the
handler population is kernel- and driver-version-dependent:

```sh
ls /sys/class/platform-profile/
cat /sys/class/platform-profile/platform-profile-*/choices
cat /sys/class/platform-profile/platform-profile-*/profile
```

## Retest checklist

With the new build (helper and GUI ship together in the RPM — both sides of
the fix matter), please retest:

- [ ] Fn Lock write (and that Fn+Esc still tracks in the app)
- [ ] Camera on/off
- [ ] Always-On USB Charging — including **with the laptop asleep**, which your
      report had marked untested (item 1 blocked it)
- [ ] Battery Conservation via the toggle (the ideapad-backed one, distinct
      from Battery Mode)
- [ ] Custom power mode from the dropdown **and** from a Preset apply
- [ ] G-Sync / Hybrid toggle — after the fix it either applies or shows an
      error under the card (and `journalctl | grep legiontoolkit-helper` shows
      the attempt); if it ever snaps back silently again, that's a bug to
      reopen
- [ ] About page: new Keyboard Backlight and Fan Curve reasons, and the new
      *Copy report* button
- [ ] Your "not tested yet" list: Presets, Settings toggles, suspend/resume

A fresh log with the new build covers the last item:

```sh
NO_COLOR=1 RUST_LOG=debug legiontoolkit 2>&1 | tee legion-toolkit.log
```

## Fan-curve editor

Deliberately its own milestone rather than slipped into this fix set: your
model's WMI fan-table *readback* returns an empty buffer (write works), so the
editor needs a default-curve loader and a write-first flow — worth building
and validating as one piece, on a machine that has a curve. Yours is currently
the only one in the tracker, so you'll hear from us when there's something to
test.
