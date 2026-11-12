#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

SPEC=$(echo packaging/*.spec)
NAME=$(basename "$SPEC" .spec)
VERSION=$(awk '/^Version:/ {print $2; exit}' "$SPEC")
SOURCE_DIR="$HOME/.local/rpm/sources/$NAME"
mkdir -p "$SOURCE_DIR" "$HOME/.local/rpm/specs"

ln -sf "$PWD/$SPEC" "$HOME/.local/rpm/specs/$NAME.spec"
git archive --prefix="$NAME-$VERSION/" -o "$SOURCE_DIR/$NAME-$VERSION.tar.gz" HEAD

# mock/COPR builds are offline: crates ship as Source1
VENDOR_TMP=$(mktemp -d)
trap 'rm -rf "$VENDOR_TMP"' EXIT
git archive HEAD Cargo.toml Cargo.lock | tar -x -C "$VENDOR_TMP"
mkdir -p "$VENDOR_TMP/src" && touch "$VENDOR_TMP/src/main.rs"
(cd "$VENDOR_TMP" && cargo vendor --locked -q vendor >/dev/null)
tar -czf "$SOURCE_DIR/$NAME-$VERSION-vendor.tar.gz" -C "$VENDOR_TMP" vendor

mx-rpm --no-git-commit "$@" "$NAME"
