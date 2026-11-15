#!/bin/sh
# Build dost and put the `dost` command into PREFIX/bin (default: ~/.local, where pipx-style
# tools usually live). Nothing else on your machine is touched: no shell config, no aliases.
#
#   ./install.sh               build and install
#   ./install.sh --uninstall   remove the installed command
#   PREFIX=/usr/local ./install.sh
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
TARGET="$BIN_DIR/dost"
HERE="$(cd "$(dirname "$0")" && pwd)"

case "${1:-}" in
  "" ) ;;
  --uninstall)
    if [ -e "$TARGET" ]; then
      rm -f "$TARGET"
      echo "removed $TARGET"
    else
      echo "nothing to remove: $TARGET does not exist"
    fi
    exit 0
    ;;
  -h|--help)
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "install.sh: unknown argument '$1' (try --help)" >&2
    exit 2
    ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo "install.sh: cargo was not found; install Rust from https://rustup.rs first" >&2
  exit 1
fi

echo "building dost (release)..."
(cd "$HERE" && cargo build --release --locked)

mkdir -p "$BIN_DIR"
install -m 755 "$HERE/target/release/dost" "$TARGET"
echo "installed $TARGET"

case ":$PATH:" in
  *":$BIN_DIR:"*) echo "ready: run 'dost doctor'" ;;
  *)
    echo "note: $BIN_DIR is not in your PATH."
    echo "      add this line to your shell config, then open a new terminal:"
    echo "        export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac
