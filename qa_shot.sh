#!/bin/bash
# Headless UI QA: run TeamsFast on Xvfb, screenshot, clean up by PID.
# usage: qa_shot.sh OUT.png SIZE [chat_id] [scroll_mode]
cd "$(dirname "$0")"
OUT="$1"; SIZE="$2"; OPEN="$3"; SCROLL="$4"

Xvfb :77 -screen 0 "${SIZE}"x24 >/dev/null 2>&1 &
XV=$!
sleep 2

if [ -n "$OPEN" ]; then
  env -u WAYLAND_DISPLAY DISPLAY=:77 TEAMSFAST_SIZE="$SIZE" \
    TEAMSFAST_OPEN="$OPEN" TEAMSFAST_SCROLL="${SCROLL:-none}" \
    ./target/debug/teamsfast 2>/dev/null &
else
  env -u WAYLAND_DISPLAY DISPLAY=:77 TEAMSFAST_SIZE="$SIZE" \
    TEAMSFAST_SCROLL="${SCROLL:-none}" \
    ./target/debug/teamsfast 2>/dev/null &
fi
APP=$!
sleep 10
# ffmpeg x11grab, not `import` — ImageMagick's import is intercepted by the
# zcode harness appimage on QA machines and fails with "unrecognized option
# `-window'"; ffmpeg's x11grab always works.
ffmpeg -y -loglevel error -f x11grab -video_size "${SIZE}" -i :77 -frames:v 1 "$OUT" 2>/dev/null
kill "$APP" 2>/dev/null
sleep 0.3
kill "$XV" 2>/dev/null
cp ~/.local/state/teamsfast/teamsfast.log /tmp/qa_last_run.log 2>/dev/null
echo "shot: $(wc -c < "$OUT") bytes -> $OUT (log: /tmp/qa_last_run.log)"
