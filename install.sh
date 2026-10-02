#!/usr/bin/env bash
# Install omaverse for the current user.
set -euo pipefail
cd "$(dirname "$0")"

APP=org.mikefsm.omaverse
BIN="${HOME}/.local/bin"
APPS="${HOME}/.local/share/applications"
ICONS="${HOME}/.local/share/icons/hicolor/scalable/apps"

echo "Building..."
cargo build --release

install -Dm755 target/release/omaverse "${BIN}/omaverse"
install -Dm644 "data/${APP}.desktop" "${APPS}/${APP}.desktop"
install -Dm644 "data/${APP}.svg"     "${ICONS}/${APP}.svg"

update-desktop-database "${APPS}" 2>/dev/null || true
gtk4-update-icon-cache -qtf "${HOME}/.local/share/icons/hicolor" 2>/dev/null \
  || gtk-update-icon-cache -qtf "${HOME}/.local/share/icons/hicolor" 2>/dev/null \
  || true

echo "Installed:"
echo "  ${BIN}/omaverse"
echo "  ${APPS}/${APP}.desktop"
echo "  ${ICONS}/${APP}.svg"

case ":${PATH}:" in
  *":${BIN}:"*) ;;
  *) echo; echo "Note: ${BIN} is not on your PATH, so 'omaverse' will not run from a shell." ;;
esac
