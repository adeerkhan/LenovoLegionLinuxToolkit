#!/usr/bin/env bash
# Install and uninstall Legion Toolkit, driven entirely by the Makefile's path
# variables so there is one definition of where each file lands.
#
#   ./install.sh install   [PREFIX=...]
#   ./install.sh uninstall [PREFIX=...]
#
# `make install` is the usual entry point. This script exists separately so the
# RPM and DEB specs, the local install and the CI can share one implementation.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
mode="${1:-}"

# Read the paths from the Makefile instead of restating them, so a change to the
# Makefile cannot be forgotten here.
eval "$(make -s -C "$here" print-paths)"

render_policy() {
    # The shipped policy names the helper at the default location. When the
    # install prefix differs, substitute the real path — polkit matches
    # `exec.path` exactly, and a mismatch makes every write fail silently.
    "$SED_PATH" "$here/packaging/org.legiontoolkit.helper.policy" \
        -e "s|@POLKIT_ACTION@|org.legiontoolkit.helper|g" \
        -e "s|@HELPER_PATH@|$HELPER_PATH|g"
}

# `sed` is needed by the renderer; take it from the Makefile's variable if set.
SED_PATH="${SED:-sed}"

case "$mode" in
    install)
        for bin in legiontoolkit legiontoolkit-helper legiontoolkit-keyboard-probe; do
            test -x "target/release/$bin" || {
                echo "error: target/release/$bin is missing — run make build" >&2
                exit 1
            }
        done

        install -Dm755 target/release/legiontoolkit \
                          "$BINDIR/legiontoolkit"
        install -Dm755 target/release/legiontoolkit-helper \
                          "$LIBEXECDIR/legiontoolkit-helper"
        install -Dm755 target/release/legiontoolkit-keyboard-probe \
                          "$BINDIR/legiontoolkit-keyboard-probe"
        install -Dm644 packaging/legiontoolkit.desktop \
                          "$DESKTOPDIR/legiontoolkit.desktop"
        install -Dm644 packaging/icons/hicolor/128x128/apps/legiontoolkit.png \
                          "$ICONDIR/legiontoolkit.png"
        # Rendered last: the action id and the helper path must agree with
        # wherever the helper just landed.
        mkdir -p "$POLKITDIR"
        render_policy > "$POLKITDIR/org.legiontoolkit.helper.policy"
        chmod 644 "$POLKITDIR/org.legiontoolkit.helper.policy"

        # polkit reads actions from a fixed directory, so a non-standard prefix
        # needs the user to link it in. Warn rather than fail.
        if [ "$POLKITDIR" != "/usr/share/polkit-1/actions" ] && \
           [ "$POLKITDIR" != "/usr/local/share/polkit-1/actions" ]; then
            echo "note: polkit reads its actions from a fixed directory."
            echo "      link $POLKITDIR/org.legiontoolkit.helper.policy there"
            echo "      or privileged writes will not authorise."
        fi

        echo "installed to $PREFIX"
        echo "  binaries   $BINDIR"
        echo "  helper     $LIBEXECDIR"
        echo "  policy     $POLKITDIR"
        echo "  desktop    $DESKTOPDIR"
        ;;
    uninstall)
        rm -f "$BINDIR/legiontoolkit" \
              "$LIBEXECDIR/legiontoolkit-helper" \
              "$BINDIR/legiontoolkit-keyboard-probe" \
              "$DESKTOPDIR/legiontoolkit.desktop" \
              "$ICONDIR/legiontoolkit.png" \
              "$POLKITDIR/org.legiontoolkit.helper.policy"
        echo "removed from $PREFIX"
        ;;
    *)
        echo "usage: $0 {install|uninstall}" >&2
        exit 64
        ;;
esac
