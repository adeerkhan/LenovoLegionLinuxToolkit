# Contributing

Thanks for taking the time. This is a small project with exactly one laptop to
hand, so reports and patches from other machines are worth more than usual.

## Reporting a bug

Open an [issue](https://github.com/adeerkhan/LenovoLegionLinuxToolkit/issues)
with:

- what you did, what you expected, and what happened instead;
- your **machine** and **driver** — the About page's *Machine*, *Driver* and
  *Backend* rows each have a copy button, so paste them straight in;
- a log:

  ```sh
  RUST_LOG=debug legiontoolkit 2>&1 | tee legion-toolkit.log
  ```

## Reporting your machine (compatibility)

The list of machines this has been verified on is one laptop long, and the only
way it grows is with reports. The [compatibility
section](README.md#compatibility) of the README has the full checklist — in
short: run it, note what works, what does not and what crashes, and open an issue
with the machine and driver strings plus a log.

## Building and running

```sh
sudo dnf install gtk4-devel libadwaita-devel pango-devel \
    gdk-pixbuf2-devel cairo-gobject-devel graphene-devel   # or the apt equivalents
cargo build --release
LEGIONTOOLKIT_MOCK=1 ./target/release/legiontoolkit          # no hardware needed
```

`LEGIONTOOLKIT_MOCK=1` drives the whole UI — including the unavailable-control
path — on any machine, so a change to a page can be seen without the laptop
attached. `--shot` renders a page to a PNG with no display: see the README's
*Usage*.

## Pull requests

- **Run the gate.** It must pass:

  ```sh
  make check       # cargo fmt --check, clippy with -D warnings, and the tests
  ```

- **Keep the library half GTK-free.** `cargo test --no-default-features` builds
  the capability, path-resolution, policy and RGB-protocol layers with no GTK, so
  they stay testable on a machine with no display. A change to `src/lib.rs` and
  its modules should not reach for a widget.

- **Match the code around you.** The comments in this project explain *why* a
  thing is done, not what the line says; a change is expected to keep that up,
  including the reasoning for a non-obvious choice.

- **Commit messages** follow [Conventional
  Commits](https://www.conventionalcommits.org/): `feat(lighting): …`,
  `fix(battery): …`, `docs: …`.

- **Reads are unprivileged; each write escalates.** A new writable control goes
  through the capability model and the helper's allowlist — never by giving the
  process more privilege.

## License

Contributions are accepted under the project's licence, [MIT](LICENSE).
