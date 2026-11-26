#!/bin/bash
# Record docs/assets/demo.gif: start a private Xvfb display, place one jr window,
# record it with ffmpeg x11grab, then build a 15 fps Catppuccin GIF.
# Requires Xvfb, ffmpeg, xdotool. No desktop or window manager needed, so it
# never touches the real session.
# Usage: scripts/record-demo-gif.sh
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT_MKV=/tmp/jiaoren-demo.mkv
OUT_GIF="$REPO/docs/assets/demo.gif"
TMP_GIF="$OUT_GIF.tmp.gif"
WIDTH=900
HEIGHT=540
FPS=15
DURATION=55

cd "$REPO"

# Private headless display: -displayfd lets Xvfb pick a free number.
DISP_FILE=$(mktemp)
Xvfb -displayfd 3 -screen 0 1200x800x24 -nolisten tcp 3>"$DISP_FILE" \
  >/tmp/jiaoren-xvfb.log 2>&1 &
XVFB_PID=$!
CFG_DIR=$(mktemp -d)
cleanup() {
  [ -n "${REC:-}" ] && kill -INT "$REC" 2>/dev/null || true
  [ -n "${JR_PID:-}" ] && kill "$JR_PID" 2>/dev/null || true
  kill "$XVFB_PID" 2>/dev/null || true
  rm -f "$DISP_FILE"
  rm -rf "$CFG_DIR"
}
trap cleanup EXIT
for _ in $(seq 1 50); do
  [ -s "$DISP_FILE" ] && break
  sleep 0.1
done
if [ ! -s "$DISP_FILE" ]; then
  echo "Xvfb did not start" >&2
  exit 1
fi
export DISPLAY=":$(cat "$DISP_FILE")"
rm -f "$DISP_FILE"
# A private config dir: the recording always starts light, at 1x zoom, no
# matter what the real session saved.
export XDG_CONFIG_HOME="$CFG_DIR"
echo "display=$DISPLAY"

./target/debug/jr examples/demo.md >/tmp/jr-gif.log 2>&1 </dev/null &
JR_PID=$!

W=""
for _ in $(seq 1 40); do
  for cand in $(xdotool search --pid "$JR_PID" 2>/dev/null || true) \
              $(xdotool search --name '^jiaoren$' 2>/dev/null || true); do
    if [ "$(xdotool getwindowname "$cand" 2>/dev/null)" = "jiaoren" ] \
       && [ "$(xdotool getwindowpid "$cand" 2>/dev/null || true)" = "$JR_PID" ]; then
      W=$cand
      break
    fi
  done
  [ -n "$W" ] && break
  sleep 0.25
done
if [ -z "$W" ] || [ "$(xdotool getwindowname "$W")" != "jiaoren" ]; then
  echo "jr window did not appear (or matched another window)" >&2
  kill "$JR_PID" 2>/dev/null || true
  exit 1
fi

# No window manager: place and size the window directly.
sleep 2
for _ in $(seq 1 12); do
  xdotool windowsize "$W" 900 540
  xdotool windowmove "$W" 0 0
  sleep 0.5
  GEO=$(xdotool getwindowgeometry --shell "$W")
  GW=$(printf '%s\n' "$GEO" | sed -n 's/^WIDTH=//p')
  GH=$(printf '%s\n' "$GEO" | sed -n 's/^HEIGHT=//p')
  [ "$GW" = "900" ] && [ "$GH" = "540" ] && break
done
GEO=$(xdotool getwindowgeometry --shell "$W")
GX=$(printf '%s\n' "$GEO" | sed -n 's/^X=//p')
GY=$(printf '%s\n' "$GEO" | sed -n 's/^Y=//p')
GW=$(printf '%s\n' "$GEO" | sed -n 's/^WIDTH=//p')
GH=$(printf '%s\n' "$GEO" | sed -n 's/^HEIGHT=//p')
if [ "$GW" -lt 400 ] || [ "$GH" -lt 300 ]; then
  echo "unexpected window size ${GW}x${GH}" >&2
  kill "$JR_PID" 2>/dev/null || true
  exit 1
fi
SX=$(awk -v a="$GW" -v b=900 'BEGIN{printf "%.5f", a/b}')
SY=$(awk -v a="$GH" -v b=540 'BEGIN{printf "%.5f", a/b}')
echo "window=$W at ${GX},${GY} ${GW}x${GH} scale=${SX}x${SY}"

focused=""
for _ in $(seq 1 20); do
  xdotool windowfocus --sync "$W" 2>/dev/null || true
  focused=$(xdotool getwindowfocus 2>/dev/null || true)
  [ "$focused" = "$W" ] && break
  sleep 0.2
done
if [ "$focused" != "$W" ]; then
  echo "window did not take focus (focused=$focused)" >&2
  exit 1
fi
xdotool windowraise "$W"
xdotool windowfocus --sync "$W"
sleep 0.8

ensure_focus() {
  xdotool windowraise "$W"
  xdotool windowfocus --sync "$W" 2>/dev/null || true
  focused=$(xdotool getwindowfocus 2>/dev/null || true)
  if [ "$focused" != "$W" ]; then
    echo "focus moved away from jr (focused=$focused); aborting" >&2
    exit 1
  fi
}
# Content coordinates (inside the 900x540 design) to absolute screen points.
mx() {
  local ax ay
  ax=$(awk -v g="$GX" -v x="$1" -v s="$SX" 'BEGIN{printf "%d", g + x*s}')
  ay=$(awk -v g="$GY" -v y="$2" -v s="$SY" 'BEGIN{printf "%d", g + y*s}')
  xdotool mousemove "$ax" "$ay"
}

rm -f "$OUT_MKV" /tmp/jiaoren-scaled.mkv
pkill -x import 2>/dev/null || true
REC=""
for attempt in 1 2 3; do
  rm -f "$OUT_MKV"
  ffmpeg -y -loglevel error \
    -f x11grab -draw_mouse 0 -framerate "$FPS" -video_size "${GW}x${GH}" -i "${DISPLAY}.0+${GX},${GY}" \
    -t "$DURATION" -c:v libx264 -qp 0 -pix_fmt yuv444p "$OUT_MKV" &
  REC=$!
  sleep 0.6
  if kill -0 "$REC" 2>/dev/null; then
    break
  fi
  echo "x11grab attempt $attempt failed" >&2
  sleep 0.5
done
if ! kill -0 "$REC" 2>/dev/null; then
  echo "could not start x11grab" >&2
  kill "$JR_PID" 2>/dev/null || true
  exit 1
fi

# 1. select the intro phrase and copy it
ensure_focus
mx 72 112
sleep 0.15
xdotool mousedown 1
sleep 0.2
for x in 140 210 280 350 420 455; do mx "$x" 112; sleep 0.1; done
xdotool mouseup 1
# Releasing the drag already copies; keep the pause so the chip is visible.
sleep 1.2
if ! kill -0 "$JR_PID" 2>/dev/null; then
  echo "jr exited during selection" >&2
  exit 1
fi
# Esc clears the kept selection; a second Esc would quit.
xdotool key --clearmodifiers Escape
sleep 0.4

# 2. search with a space, then a nested quote
ensure_focus
xdotool key --clearmodifiers slash
sleep 0.3
xdotool type --clearmodifiers --delay 80 "markdown viewer"
sleep 0.9
xdotool key Return
sleep 0.8
xdotool key --clearmodifiers Escape
sleep 0.3
xdotool key --clearmodifiers slash
sleep 0.2
xdotool type --clearmodifiers --delay 80 "Quote"
sleep 0.4
xdotool key Return
sleep 1.2
xdotool key --clearmodifiers Escape
sleep 0.3

# 3. table: hover a row, then click the copy icon in the header
ensure_focus
xdotool key --clearmodifiers slash
sleep 0.2
xdotool type --clearmodifiers --delay 70 "Copy table"
sleep 0.4
xdotool key Return
sleep 0.6
mx 300 288
sleep 1.0
mx 845 247
xdotool click 1
sleep 1.2
xdotool key --clearmodifiers Escape
sleep 0.3

# 4. diagram popup and zoom
ensure_focus
xdotool key --clearmodifiers slash
sleep 0.2
xdotool type --clearmodifiers --delay 70 "diagram"
sleep 0.4
xdotool key Return
sleep 0.4
xdotool key n
sleep 0.9
mx 430 395
xdotool click 1
sleep 0.8
# Theme toggle stays live inside the overlay (0.5.0), then zoom.
xdotool key --clearmodifiers t
sleep 1.2
xdotool click 4; sleep 0.25; xdotool click 4; sleep 0.25; xdotool click 4
sleep 1.2
xdotool key --clearmodifiers q
sleep 0.6

# 5. bottom code block: hover a wrapped source line (both visual rows)
ensure_focus
xdotool key --clearmodifiers G
sleep 1.0
mx 400 201
sleep 1.5

# 6. next/previous document in the folder
ensure_focus
xdotool key --clearmodifiers bracketright
sleep 2.2
xdotool key --clearmodifiers bracketleft
sleep 1.6

# 7. heading index: fold, unfold and jump
ensure_focus
xdotool key --clearmodifiers Tab
sleep 1.0
xdotool key --clearmodifiers g g
sleep 0.5
xdotool key --clearmodifiers h
sleep 1.8
xdotool key --clearmodifiers l
sleep 1.8
for _ in 1 2 3; do xdotool key --clearmodifiers j; sleep 0.25; done
sleep 0.6
xdotool key Return
sleep 1.2

# 8. file path / progress status line
ensure_focus
xdotool key --clearmodifiers ctrl+g
sleep 2.4

# 9. keyboard-shortcut overlay
ensure_focus
xdotool key --clearmodifiers shift+slash
sleep 2.4
xdotool key --clearmodifiers Escape
sleep 0.6

# 10. zoom and horizontal pan
ensure_focus
xdotool key --clearmodifiers plus
sleep 0.3
xdotool key --clearmodifiers plus
sleep 0.3
xdotool key --clearmodifiers plus
sleep 1.2
xdotool key --clearmodifiers h
sleep 0.25
xdotool key --clearmodifiers h
sleep 1.0
xdotool key --clearmodifiers l
sleep 0.25
xdotool key --clearmodifiers l
sleep 1.0
xdotool key --clearmodifiers a
sleep 1.0

# 11. yy copies the whole document
ensure_focus
xdotool key --clearmodifiers y
sleep 0.2
xdotool key --clearmodifiers y
sleep 2.4

# 12. back to the light theme
xdotool key --clearmodifiers t
sleep 2.2

kill -INT "$REC" 2>/dev/null || true
wait "$REC" 2>/dev/null || true

test -s "$OUT_MKV" || { echo "capture missing" >&2; exit 1; }
ffmpeg -v error -y -i "$OUT_MKV" -vf "fps=15,scale=900:540:flags=lanczos" -c:v libx264 -qp 0 -pix_fmt yuv444p /tmp/jiaoren-scaled.mkv
ffmpeg -v error -y -i /tmp/jiaoren-scaled.mkv -vf "palettegen=max_colors=128" /tmp/jiaoren-palette.png
ffmpeg -v error -y -i /tmp/jiaoren-scaled.mkv -i /tmp/jiaoren-palette.png \
  -lavfi "paletteuse=dither=none" -loop 0 "$TMP_GIF"
mv "$TMP_GIF" "$OUT_GIF"
ls -l "$OUT_GIF"
ffprobe -v error -show_entries format=duration -of default=nw=1 "$OUT_GIF"
