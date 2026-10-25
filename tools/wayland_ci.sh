#!/usr/bin/env bash
# Runs PinShot inside a headless sway (wlroots) Wayland session with xdg-desktop-portal-wlr, the way Ubuntu's
# Wayland session behaves: portal screenshot, native Wayland windows, XWayland for the monitor list.
# usage: tools/wayland_ci.sh <path-to-pinshot-binary>   (artifacts land in $OUT, default ./wayland-out)
set -euo pipefail

BIN="$(realpath "${1:?path to the pinshot binary}")"
OUT="$(realpath -m "${OUT:-wayland-out}")"
mkdir -p "$OUT"

export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR="$(mktemp -d)"
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=sway
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export HOME="${HOME_OVERRIDE:-$(mktemp -d)}"
mkdir -p "$HOME/.config/xdg-desktop-portal"
printf '[preferred]\ndefault=wlr\n' > "$HOME/.config/xdg-desktop-portal/sway-portals.conf"
export PINSHOT_LOG_STDERR=1 PINSHOT_DEBUG=1

cat > "$OUT/sway.conf" <<'CONF'
output HEADLESS-1 resolution 1280x720 position 0 0
default_border none
CONF

pids=()
dump() {
  echo "::group::diagnostics"
  for f in "$OUT"/*.log; do echo "--- $f"; tail -n 40 "$f" || true; done
  swaymsg -t get_tree 2>/dev/null | head -c 4000 || true
  echo "::endgroup::"
}
cleanup() { kill "${pids[@]}" 2>/dev/null || true; }
trap 'rc=$?; [ $rc -ne 0 ] && dump; cleanup; exit $rc' EXIT

start() { local name=$1; shift; "$@" > "$OUT/$name.log" 2>&1 & pids+=($!); }
wait_for() { # wait_for <description> <command...>
  local what=$1; shift
  for _ in $(seq 1 50); do "$@" >/dev/null 2>&1 && return 0; sleep 0.2; done
  echo "timeout waiting for $what" >&2; return 1
}

start sway sway --unsupported-gpu -c "$OUT/sway.conf"
wait_for "wayland socket" bash -c 'ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null'
export WAYLAND_DISPLAY
WAYLAND_DISPLAY="$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -1)")"
export SWAYSOCK
SWAYSOCK="$(ls "$XDG_RUNTIME_DIR"/sway-ipc.* | head -1)"
wait_for "sway ipc" swaymsg -t get_outputs

# XWayland provides the monitor list (RandR) exactly as on a real desktop
start xwayland Xwayland :9 -rootless
export DISPLAY=:9
wait_for "xwayland" test -S /tmp/.X11-unix/X9

eval "$(dbus-launch --sh-syntax)"
for p in /usr/libexec/xdg-desktop-portal-wlr /usr/lib/xdg-desktop-portal-wlr; do
  [ -x "$p" ] && start portal-wlr "$p" && break
done
sleep 1
for p in /usr/libexec/xdg-desktop-portal /usr/lib/xdg-desktop-portal; do
  [ -x "$p" ] && start portal "$p" -r && break
done
sleep 2

# 1. self-test: monitors via XWayland, screenshot through the portal, OCR, ffmpeg
"$BIN" --selftest | tee "$OUT/selftest.txt"

# 2. the app itself: tray + windows on Wayland
start pinshot "$BIN"
wait_for "pinshot to start" grep -q "started" "$OUT/pinshot.log"
"$BIN" --settings
wait_for "settings window" bash -c "swaymsg -t get_tree | grep -q 'Settings'"
grim "$OUT/settings.png"
"$BIN" --about
wait_for "about window" bash -c "swaymsg -t get_tree | grep -q 'About'"
grim "$OUT/about.png"

# 3. capture overlay: opens fullscreen on every output and shows the frozen desktop
"$BIN" --capture
sleep 3
swaymsg -t get_tree | grep -q '"fullscreen_mode": 1' || { echo "overlay is not fullscreen" >&2; exit 1; }
grim "$OUT/overlay.png"
# the overlay must not be an empty (single colour) image
python3 - "$OUT/overlay.png" <<'PY'
import sys
from PIL import Image
img = Image.open(sys.argv[1]).convert("RGB")
colours = img.getcolors(maxcolors=1 << 20)
assert colours is None or len(colours) > 8, "overlay screenshot is blank"
print("overlay colours:", "many" if colours is None else len(colours))
PY

"$BIN" --quit || true
echo "wayland checks passed"
