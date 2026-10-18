#!/bin/bash
# Settle-frame layout-shift QA: open ONE chat, capture the conversation
# body at three times (T0=cached paint, T1/T2=network settled), crop to
# the message region (excludes the caret-blinking composer and header),
# and byte-compare. Identical pixels -> identical PNG bytes, so
# `cmp` is the verdict.
#
# usage: qa_settlediff.sh CHAT_ID [OUTDIR] [extra env as KEY=VAL...]
# evidence lands in OUTDIR (default /tmp/qasd): t0/t1/t2.png + crops +
# cmp verdicts + the app log.
cd "$(dirname "$0")"
CHAT="$1"; OUT="${2:-/tmp/qasd}"; shift 2 2>/dev/null || shift $#
SIZE=1100x760
mkdir -p "$OUT"
# Conversation body crop at 1100x760: right of the sidebar (272), below
# the chat header (96), above the composer (560). Animated pixels (caret,
# typing indicator, hover bars) live outside this rect.
CROP="crop=812:440:280:100"

Xvfb :78 -screen 0 "${SIZE}"x24 >/dev/null 2>&1 &
XV=$!
sleep 2
env -u WAYLAND_DISPLAY DISPLAY=:78 TEAMSFAST_SIZE="$SIZE" \
  TEAMSFAST_OPEN="$CHAT" TEAMSFAST_SCROLL=none "$@" \
  ./target/debug/teamsfast >/dev/null 2>&1 &
APP=$!

shot() { ffmpeg -y -loglevel error -f x11grab -video_size "${SIZE}" -i :78 \
  -frames:v 1 "$1" 2>/dev/null; }

# Load-robust timing (run-7): the first content frame of a DEBUG build
# can take ~3s under desktop load (user's Firefox at 200% CPU stretched
# it past the old 5s sample — three false DIFFERS verdicts, each with a
# blank t1 body). 8s/12s clears that while still catching real drift.
sleep 0.7; shot "$OUT/t0.png"   # cached archive paint
sleep 7.3; shot "$OUT/t1.png"   # network page settled
sleep 4.0; shot "$OUT/t2.png"   # still settled (idle)
kill "$APP" 2>/dev/null; sleep 0.3; kill "$XV" 2>/dev/null
cp ~/.local/state/teamsfast/teamsfast.log "$OUT/app.log" 2>/dev/null

for t in t0 t1 t2; do
  ffmpeg -y -loglevel error -i "$OUT/$t.png" -vf "$CROP" "$OUT/${t}_crop.png" 2>/dev/null
done
if cmp -s "$OUT/t1_crop.png" "$OUT/t2_crop.png"; then
  echo "SETTLEDIFF t1-vs-t2: IDENTICAL (no drift between settled frames)"
else
  echo "SETTLEDIFF t1-vs-t2: DIFFERS (layout still moving when idle)"
fi
if cmp -s "$OUT/t0_crop.png" "$OUT/t1_crop.png"; then
  echo "SETTLEDIFF t0-vs-t1: IDENTICAL (cached paint == settled layout)"
else
  echo "SETTLEDIFF t0-vs-t1: DIFFERS (cached->network relayout)"
fi
