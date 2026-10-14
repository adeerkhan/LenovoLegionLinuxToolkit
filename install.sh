#!/usr/bin/env bash
# Install and uninstall Legion Toolkit.
#
# Paths are derived from PREFIX here. The Makefile derives the same paths (and
# HELPER_PATH, which it bakes into the GUI binary) from the same PREFIX — keep
# the two in step.
#
#   PREFIX=/path ./install.sh install
#   PREFIX=/path ./install.sh uninstall
#
# Or through the Makefile, which forwards PREFIX as an environment variable:
#   make install PREFIX=/path
#
# `make install` is the usual entry point. This script is separate so the RPM
# and DEB specs, the local install and CI can share one implementation.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-/usr/local}"

# Mirrored in the Makefile.
BINDIR="$PREFIX/bin"
LIBEXECDIR="$PREFIX/libexec"
DESKTOPDIR="$PREFIX/share/applications"
ICONTHEMEDIR="$PREFIX/share/icons/hicolor"
POLKITDIR="$PREFIX/share/polkit-1/actions"
HELPER_PATH="$PREFIX/libexec/legiontoolkit-helper"

# Polkit reads rules only from fixed directories under /etc and /usr/share, so
# this one is absolute rather than derived from PREFIX — a rule under
# /usr/local/etc would simply never be read.
POLKITRULESDIR="/etc/polkit-1/rules.d"

POLICY="org.legiontoolkit.helper.policy"
BINARIES="legiontoolkit legiontoolkit-helper legiontoolkit-keyboard-probe"
ICON_SIZES="16 22 24 32 48 64 128 256 512"

die() {
    echo "error: $*" >&2
    exit 1
}

# The policy is rendered to a temporary file and installed with `install`
# rather than written straight into place. A redirect can leave a truncated
# file behind if it fails partway, and a malformed .policy makes polkit log an
# error on every action lookup — a bad state to discover by reading logs.
tmp_policy=""
cleanup() {
    [ -n "$tmp_policy" ] && rm -f "$tmp_policy"
    return 0
}
trap cleanup EXIT

render_policy() {
    # polkit matches exec.path literally. A mismatch refuses every write in a
    # way that looks like the feature is broken, so the rendered file always
    # names exactly where the helper landed.
    sed -e "s|@POLKIT_ACTION@|org.legiontoolkit.helper|g" \
        -e "s|@HELPER_PATH@|$HELPER_PATH|g" \
        "$here/packaging/org.legiontoolkit.helper.policy"
}

mode="${1:-}"

case "$mode" in
    install | uninstall) ;;
    *)
        echo "usage: PREFIX=/path $0 {install|uninstall}" >&2
        exit 64
        ;;
esac

# Refuse stray arguments rather than ignoring them. PREFIX is an environment
# variable, so `./install.sh install PREFIX=/x` looks like it works and then
# installs to /usr/local — which, run as root, succeeds silently in the wrong
# place. Failing here costs one line and prevents that.
shift
if [ $# -gt 0 ]; then
    echo "error: unexpected argument '$1'." >&2
    echo "       PREFIX is an environment variable: PREFIX=$1 $0" >&2
    exit 64
fi

if [ "$mode" = install ]; then
    # Resolved against $here, not the working directory, so running this script
    # from anywhere else still installs what it just checked.
    for bin in $BINARIES; do
        test -x "$here/target/release/$bin" \
            || die "target/release/$bin is missing — run make build"
    done

    install -Dm755 "$here/target/release/legiontoolkit" \
                      "$BINDIR/legiontoolkit"
    install -Dm755 "$here/target/release/legiontoolkit-helper" \
                      "$LIBEXECDIR/legiontoolkit-helper"
    install -Dm755 "$here/target/release/legiontoolkit-keyboard-probe" \
                      "$BINDIR/legiontoolkit-keyboard-probe"

    install -Dm644 "$here/packaging/legiontoolkit.desktop" \
                      "$DESKTOPDIR/legiontoolkit.desktop"
    # Every size the generator produces is installed, so the icon is sharp at
    # each size and gtk-update-icon-cache has something real to index.
    for size in $ICON_SIZES; do
        install -Dm644 "$here/packaging/icons/hicolor/${size}x${size}/apps/legiontoolkit.png" \
                          "$ICONTHEMEDIR/${size}x${size}/apps/legiontoolkit.png"
    done

    # Rendered last: the action id and the helper path must agree with wherever
    # the helper just landed.
    tmp_policy="$(mktemp)"
    render_policy > "$tmp_policy"
    install -Dm644 "$tmp_policy" "$POLKITDIR/$POLICY"

    # Optional: let an active, local session use the helper without a password.
    # Off by default; enabled with `make install LEGIONTOOLKIT_NO_PASSWORD=1`.
    # This grants the org.legiontoolkit.helper action and nothing else, and the
    # helper still enforces its own allowlist and destination-root check.
    if [ "${LEGIONTOOLKIT_NO_PASSWORD:-}" = "1" ]; then
        install -Dm644 "$here/packaging/49-legiontoolkit-no-password.rules" \
                          "$POLKITRULESDIR/49-legiontoolkit.rules"
    fi

    # Best effort: without these the entry and icon can linger until the next
    # login, and a missing tool is not a reason to fail the install.
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$DESKTOPDIR" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -q -t -f "$ICONTHEMEDIR" 2>/dev/null || true
    fi

    echo "installed to $PREFIX"
    echo "  binaries   $BINDIR"
    echo "  helper     $LIBEXECDIR"
    echo "  policy     $POLKITDIR/$POLICY"
    echo "  desktop    $DESKTOPDIR"
    echo "  icons      $ICONTHEMEDIR"
    if [ "${LEGIONTOOLKIT_NO_PASSWORD:-}" = "1" ]; then
        echo "  no-password rule $POLKITRULESDIR/49-legiontoolkit.rules"
    fi
else
    rm -f "$BINDIR/legiontoolkit" \
          "$LIBEXECDIR/legiontoolkit-helper" \
          "$BINDIR/legiontoolkit-keyboard-probe" \
          "$DESKTOPDIR/legiontoolkit.desktop" \
          "$POLKITDIR/$POLICY"
    # The no-password rule lives under /etc, outside PREFIX, and removing it
    # needs root. Failing to remove it (a non-root uninstall) must not abort the
    # rest of the uninstall.
    rm -f "$POLKITRULESDIR/49-legiontoolkit.rules" 2>/dev/null || true
    rm -f "$ICONTHEMEDIR"/*/apps/legiontoolkit.png
    echo "removed from $PREFIX"
fi
