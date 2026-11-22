#!/bin/bash
# Record docs/assets/demo.gif: record the demo window with ffmpeg x11grab,
# then build a 15 fps Catppuccin GIF. Requires DISPLAY=:0, i3, ffmpeg, xdotool.
# Usage: scripts/record-demo-gif.sh
# Fresh GIF recorder: places one jr window, verifies focus/geometry, records the
# X11 region with ffmpeg, then builds the GIF. No reuse of older scripts.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT_MKV=/tmp/jiaoren-demo.mkv
OUT_GIF="$REPO/docs/assets/demo.gif"
TMP_GIF="$OUT_GIF.tmp.gif"
WIDTH=900
HEIGHT=540
FPS=15
DURATION=26

export DISPLAY=:0
cd "$REPO"

pkill -x jr 2>/dev/null || true
sleep 0.6

i3-msg 'workspace 9' >/dev/null
sleep 0.6

./target/debug/jr examples/demo.md >/tmp/jr-gif.log 2>&1 </dev/null &
JR_PID=$!

W=""
for _ in $(seq 1 40); do
  W=$(xdotool search --pid "$JR_PID" 2>/dev/null | tail -1 || true)
  [ -z "$W" ] && W=$(xdotool search --name jiaoren 2>/dev/null | tail -1 || true)
  [ -n "$W" ] && break
  sleep 0.25
done
if [ -z "$W" ]; then
  echo "jr window did not appear" >&2
  kill "$JR_PID" 2>/dev/null || true
  exit 1
fi

# Give i3 time to manage the new window before changing its state.
sleep 2
for _ in $(seq 1 12); do
  i3-msg "[id=$W] floating enable" >/dev/null 2>&1 || true
  sleep 0.3
  xdotool windowsize "$W" 900 540
  xdotool windowmove "$W" 120 120
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
  xdotool windowactivate --sync "$W" 2>/dev/null || true
  focused=$(xdotool getactivewindow 2>/dev/null || true)
  [ "$focused" = "$W" ] && break
  sleep 0.2
done
if [ "$focused" != "$W" ]; then
  echo "window did not take focus (active=$focused)" >&2
  kill "$JR_PID" 2>/dev/null || true
  exit 1
fi
sleep 0.8

# Content coordinates (inside the 900x540 design) to absolute screen points.
mx() {
  local ax ay
  ax=$(awk -v g="$GX" -v x="$1" -v s="$SX" 'BEGIN{printf "%d", g + x*s}')
  ay=$(awk -v g="$GY" -v y="$2" -v s="$SY" 'BEGIN{printf "%d", g + y*s}')
  xdotool mousemove "$ax" "$ay"
}

pkill -x import 2>/dev/null || true
ffmpeg -y -loglevel error \
  -f x11grab -draw_mouse 0 -framerate "$FPS" -video_size "${GW}x${GH}" -i ":0.0+${GX},${GY}" \
  -t "$DURATION" -c:v libx264 -qp 0 -pix_fmt yuv444p "$OUT_MKV" &
REC=$!
sleep 0.8

# 1. select the intro phrase and copy it
mx 72 112
xdotool mousedown 1
for x in 140 210 280 350 420 455; do mx "$x" 112; sleep 0.08; done
xdotool mouseup 1
sleep 0.5
xdotool key --clearmodifiers y
sleep 1.2
xdotool key --clearmodifiers Escape
sleep 0.3

# 2. search with a space, then a normal search with Ctrl+W
xdotool key --clearmodifiers slash
sleep 0.3
xdotool type --clearmodifiers --delay 80 "markdown viewer"
sleep 0.9
xdotool key Return
sleep 0.7
xdotool key --clearmodifiers Escape
sleep 0.3
xdotool key --clearmodifiers slash
sleep 0.3
xdotool type --clearmodifiers --delay 80 "markdown vie"
sleep 0.6
xdotool key --clearmodifiers ctrl+w
sleep 0.4
xdotool key --clearmodifiers BackSpace
sleep 0.3
xdotool key Return
sleep 0.8
xdotool key n
sleep 1.0

# 3. table: hover a row, then click the copy icon
mx 300 242
sleep 1.2
mx 843 216
xdotool click 1
sleep 1.2
xdotool key --clearmodifiers Escape
sleep 0.3

# 4. bottom code block: hover one line
xdotool key --clearmodifiers G
sleep 1.0
mx 400 215
sleep 1.4

kill -INT "$REC" 2>/dev/null || true
wait "$REC" 2>/dev/null || true
xdotool key --clearmodifiers q
sleep 0.4
pkill -x jr 2>/dev/null || true

test -s "$OUT_MKV" || { echo "capture missing" >&2; exit 1; }
ffmpeg -v error -y -i "$OUT_MKV" -vf "fps=15,scale=900:540:flags=lanczos" -c:v libx264 -qp 0 -pix_fmt yuv444p /tmp/jiaoren-scaled.mkv
ffmpeg -v error -y -i /tmp/jiaoren-scaled.mkv -vf "palettegen=max_colors=128" /tmp/jiaoren-palette.png
ffmpeg -v error -y -i /tmp/jiaoren-scaled.mkv -i /tmp/jiaoren-palette.png \
  -lavfi "paletteuse=dither=none" -loop 0 "$TMP_GIF"
mv "$TMP_GIF" "$OUT_GIF"
ls -l "$OUT_GIF"
ffprobe -v error -show_entries format=duration -of default=nw=1 "$OUT_GIF"
