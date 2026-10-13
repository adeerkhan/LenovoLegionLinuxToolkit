# Legion Toolkit — local build and install.
#
# The Makefile owns the install paths. install.sh, and later the RPM and DEB
# specs, read them from here, so there is exactly one place to change where a
# file lands.
#
# The variable that matters most is HELPER_PATH. The polkit action declares the
# same path in `org.freedesktop.policykit.exec.path`, and polkit matches it
# exactly: if the helper is installed anywhere else, every privileged write is
# refused and each control looks broken for reasons the user cannot see. The
# installed policy is therefore rendered from this variable, not shipped as a
# fixed file.

PREFIX       ?= /usr/local
BINDIR       ?= $(PREFIX)/bin
LIBEXECDIR   ?= $(PREFIX)/libexec
DATADIR      ?= $(PREFIX)/share
DESKTOPDIR   ?= $(DATADIR)/applications
ICONDIR      ?= $(DATADIR)/icons/hicolor/128x128/apps
POLKITDIR    ?= $(DATADIR)/polkit-1/actions

# Absolute path of the helper. Must match the polkit policy's exec.path
# annotation; `make distcheck` verifies the shape.
HELPER_PATH  ?= $(PREFIX)/libexec/legiontoolkit-helper

CARGO       ?= cargo
INSTALL     ?= install
SED         ?= sed

.PHONY: all check build install uninstall clean distcheck print-paths icon

all: build
build:
	# Built explicitly rather than with --bins. `--bins` shares one feature
	# resolution across targets, which left the helper stale when only the
	# GUI changed — a silent "make install ships the old binary" bug. The
	# helper and probe are also built without GTK so they cannot pull a GUI
	# runtime into a root process.
	#
	# LEGIONTOOLKIT_HELPER_PATH is baked into the GUI binary so it agrees with
	# wherever `install` puts the helper. The polkit policy declares the same
	# path, and polkit matches it exactly — if the two diverge every write is
	# refused in a way that looks like the feature is broken.
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

# Emit the path variables as shell assignments for install.sh to eval.
print-paths:
	@echo "PREFIX='$(PREFIX)'"
	@echo "BINDIR='$(BINDIR)'"
	@echo "LIBEXECDIR='$(LIBEXECDIR)'"
	@echo "DATADIR='$(DATADIR)'"
	@echo "DESKTOPDIR='$(DESKTOPDIR)'"
	@echo "ICONDIR='$(ICONDIR)'"
	@echo "POLKITDIR='$(POLKITDIR)'"
	@echo "HELPER_PATH='$(HELPER_PATH)'"

install: build
	./install.sh install

uninstall:
	./install.sh uninstall

icon:
	python3 packaging/make-icon.py

# Catch path mistakes before they become silent write failures on a user's
# machine.
distcheck: build
	@echo "PREFIX      = $(PREFIX)"
	@echo "HELPER_PATH = $(HELPER_PATH)"
	@echo "POLKITDIR   = $(POLKITDIR)"
	@test -f packaging/org.legiontoolkit.helper.policy || \
		{ echo "missing packaging/org.legiontoolkit.helper.policy"; exit 1; }
	@case "$(HELPER_PATH)" in \
	    */legiontoolkit-helper) ;; \
	    *) echo "HELPER_PATH must end in /legiontoolkit-helper"; exit 1 ;; \
	esac
	@echo "layout ok"

clean:
	$(CARGO) clean
