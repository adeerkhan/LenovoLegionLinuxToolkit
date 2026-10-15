# Legion Toolkit — local build and install.
#
# This Makefile owns the build and the *default* install prefix. install.sh
# derives the per-file destinations from the same PREFIX; the two must stay in
# step, and each says so at the point where it matters.
#
# The variable that matters most is HELPER_PATH. It is baked into the GUI binary
# at build time, and the polkit action declares the same path in
# `org.freedesktop.policykit.exec.path`. polkit matches that annotation
# literally: if the two diverge, every privileged write is refused and each
# control looks broken for reasons the user cannot see.

PREFIX      ?= /usr/local
HELPER_PATH ?= $(PREFIX)/libexec/legiontoolkit-helper

# Read by the `rpm` target; kept in step with Cargo.toml.
VERSION     ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

# Set to 1 to install the polkit rule that lets an active, local session use the
# helper without a password prompt: `sudo make install LEGIONTOOLKIT_NO_PASSWORD=1`.
LEGIONTOOLKIT_NO_PASSWORD ?=

CARGO      ?= cargo

.PHONY: all check build install uninstall clean distcheck icon rpm flatpak

all: build
build:
	# Built explicitly rather than with --bins. `--bins` shares one feature
	# resolution across targets, which left the helper stale when only the
	# GUI changed — a silent "make install ships the old binary" bug. The
	# helper and probe are also built without GTK so they cannot pull a GUI
	# runtime into a root process.
	#
	# LEGIONTOOLKIT_HELPER_PATH must agree with where install.sh puts the
	# helper. Both derive it from PREFIX; see privilege::HELPER_PATH.
	LEGIONTOOLKIT_HELPER_PATH='$(HELPER_PATH)' $(CARGO) build --release --bin legiontoolkit
	$(CARGO) build --release --bin legiontoolkit-helper --no-default-features
	$(CARGO) build --release --bin legiontoolkit-keyboard-probe --no-default-features

# The gates this project must pass before anything ships. The library half is
# tested without GTK so a machine with no display installed can still verify
# the capability, path-resolution and security-policy code.
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test --no-default-features

install: build
	LEGIONTOOLKIT_NO_PASSWORD='$(LEGIONTOOLKIT_NO_PASSWORD)' ./install.sh install

uninstall:
	./install.sh uninstall

icon:
	python3 packaging/make-icon.py

# Build an RPM. The source is archived from git, so rpmbuild sees exactly a
# commit; the spec runs the same Makefile inside that tree. Run `git add` first
# — uncommitted files are not in the archive.
rpm:
	@test -n "$(VERSION)" || { echo "could not read version from Cargo.toml"; exit 1; }
	@git diff --quiet || echo "warning: packaging from HEAD; uncommitted changes are not included"
	git archive --prefix=legiontoolkit-$(VERSION)/ -o packaging/rpm/legiontoolkit-$(VERSION).tar.gz HEAD
	rpmbuild -bb packaging/rpm/legiontoolkit.spec \
	    --define "_topdir $(CURDIR)/target/rpmbuild" \
	    --define "_sourcedir $(CURDIR)/packaging/rpm"
	@echo "RPM written under target/rpmbuild/RPMS/"

# Build a Flatpak and export it to a local repository under target/.
# packaging/flatpak/README.md covers the runtime, the crate sources and publishing.
flatpak:
	@test -f packaging/flatpak/cargo-sources.json || { \
	    echo "packaging/flatpak/cargo-sources.json is missing — run packaging/flatpak/generate-cargo-sources.sh"; \
	    exit 1; }
	flatpak-builder --force-clean --user \
	    --repo=$(CURDIR)/target/flatpak-repo \
	    $(CURDIR)/target/flatpak-build \
	    packaging/flatpak/org.legiontoolkit.Ltk.yml
	flatpak build-update-repo --generate-static-deltas $(CURDIR)/target/flatpak-repo
	@echo "built. Install with:"
	@echo "  flatpak --user remote-add --no-gpg-verify legiontoolkit file://$(CURDIR)/target/flatpak-repo"
	@echo "  flatpak --user install legiontoolkit org.legiontoolkit.Ltk"

# Prints where things will land, without touching the system or compiling.
# The fast way to sanity-check a PREFIX override before committing to it.
distcheck:
	@echo "PREFIX      = $(PREFIX)"
	@echo "HELPER_PATH = $(HELPER_PATH)"
	@echo "policy      = $(PREFIX)/share/polkit-1/actions"
	@echo "binaries    = $(PREFIX)/bin"
	@case "$(HELPER_PATH)" in \
	    */legiontoolkit-helper) ;; \
	    *) echo "HELPER_PATH must end in /legiontoolkit-helper"; exit 1 ;; \
	esac
	@echo "layout ok"

clean:
	$(CARGO) clean
