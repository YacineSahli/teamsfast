#!/bin/bash
# Paired idle-RAM + first-paint benchmark (zapfast methodology, adapted
# to this repo's headless Xvfb harness). Compares TeamsFast (release
# build, signed-in default profile) against teams-for-linux v2.26.0 run
# from an extracted AppImage (/tmp/tfl-bench/squashfs-root, fresh HOME).
#
# usage: scripts/bench.sh [runs] [out_dir]
#   runs   — paired runs (default 4; app order alternates within a pair)
#   outdir — docs/assets/benchmarks/<date> by default
#
# Measurements per run:
#   first_paint_ms — monotonic spawn -> first non-black video frame on a
#                    fresh Xvfb display (identical detector for both apps)
#   pss_kb         — SUM of Pss over the app's whole process tree
#                    (/proc/PID/smaps_rollup); median of 5 samples taken
#                    25s after first paint, 1s apart
#   child_procs    — direct children at sampling time
# Privacy: counts/timings/memory only — no contents, no kept screenshots.
set -u
cd "$(dirname "$0")/.."
RUNS="${1:-4}"
DATE=$(date +%F)
OUT="${2:-docs/assets/benchmarks/$DATE}"
mkdir -p "$OUT"
TF_BIN=./target/release/teamsfast
TFL_DIR=/tmp/tfl-bench/squashfs-root
[ -x "$TF_BIN" ] || { echo "missing $TF_BIN (cargo build --release)"; exit 1; }
[ -x "$TFL_DIR/teams-for-linux" ] || { echo "missing $TFL_DIR (extract the AppImage first)"; exit 1; }

TFL_HOME=/tmp/tfl-bench/home; mkdir -p "$TFL_HOME/.config" "$TFL_HOME/.cache"
LINES=$(mktemp); trap 'rm -f "$LINES"' EXIT
DISP_C=80

pss_tree_kb() { # root pid -> summed Pss kB over the process tree
  python3 - "$1" <<'PYEOF'
import sys, os
root = int(sys.argv[1])
total, stack, seen = 0, [root], set()
while stack:
    p = stack.pop()
    if p in seen: continue
    seen.add(p)
    try:
        with open(f"/proc/{p}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    total += int(line.split()[1]); break
        for kid in os.listdir(f"/proc/{p}/task"):
            try:
                with open(f"/proc/{p}/task/{kid}/children") as f:
                    stack += [int(x) for x in f.read().split()]
            except OSError: pass
    except OSError: pass
print(total)
PYEOF
}

# Prints epoch-ms of the first frame whose central region has a lit
# pixel on the given display (10fps gray rawvideo from ffmpeg).
watch_first_paint() { # display W H
  python3 - "$1" "$2" "$3" <<'PYEOF'
import subprocess, sys, time
disp, w, h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
ff = subprocess.Popen(
    ["ffmpeg", "-loglevel", "error", "-f", "x11grab", "-video_size", f"{w}x{h}",
     "-i", f":{disp}", "-vf", "fps=10", "-f", "rawvideo", "-pix_fmt", "gray", "-"],
    stdout=subprocess.PIPE)
frame = w * h
x0, x1, y0, y1 = int(w*0.2), int(w*0.8), int(h*0.2), int(h*0.8)
while True:
    buf = ff.stdout.read(frame)
    if len(buf) < frame:
        print("NEVER"); break
    lit = any(
        any(buf[y*w + x] > 24 for x in range(x0, x1, 4))
        for y in range(y0, y1, 4))
    if lit:
        print(int(time.time()*1000)); break
PYEOF
}

run_one() { # app(tfast|tfl) run_no
  local app=$1 run=$2
  local W=1280 H=800
  DISP_C=$((DISP_C + 1)); local disp=$DISP_C
  Xvfb ":$disp" -screen 0 ${W}x${H}x24 >/dev/null 2>&1 &
  local XV=$!
  sleep 1.5
  local WATCH_OUT; WATCH_OUT=$(mktemp)
  watch_first_paint "$disp" $W $H > "$WATCH_OUT" &
  local WATCH=$!
  local T0=$(date +%s%3N)
  local APP
  if [ "$app" = tfast ]; then
    env -u WAYLAND_DISPLAY DISPLAY=":$disp" "$TF_BIN" >/dev/null 2>&1 &
    APP=$!
  else
    env -u WAYLAND_DISPLAY DISPLAY=":$disp" HOME="$TFL_HOME" \
      XDG_CONFIG_HOME="$TFL_HOME/.config" XDG_CACHE_HOME="$TFL_HOME/.cache" \
      setsid "$TFL_DIR/teams-for-linux" --disable-gpu --no-sandbox >/dev/null 2>&1 &
    APP=$!
  fi
  wait $WATCH 2>/dev/null
  local FIRST=$(cat "$WATCH_OUT"); rm -f "$WATCH_OUT"
  if [ "$FIRST" = NEVER ] || [ -z "$FIRST" ]; then
    echo "app=$app run=$run ERROR=no-first-paint" >> "$LINES"
    kill $APP 2>/dev/null; kill $XV 2>/dev/null; sleep 2; return
  fi
  local FIRST_MS=$(( FIRST - T0 ))
  sleep 25   # settle: our chat list / their login page fully loaded
  local -a pss=()
  for _ in 1 2 3 4 5; do
    pss+=($(pss_tree_kb $APP)); sleep 1
  done
  local nkids=$(ps -o pid= --ppid $APP 2>/dev/null | wc -l)
  kill -TERM -- -$APP 2>/dev/null; kill -TERM $APP 2>/dev/null
  sleep 2; pkill -9 -P $APP 2>/dev/null; kill -9 $APP 2>/dev/null
  kill $XV 2>/dev/null
  local med=$(printf '%s\n' "${pss[@]}" | sort -n | sed -n 3p)
  echo "app=$app run=$run first_paint_ms=$FIRST_MS pss_kb=$med children=$nkids samples=$(printf '%s,' "${pss[@]}")" >> "$LINES"
  echo "run $run/$RUNS $app: first_paint=${FIRST_MS}ms pss=$((med/1024))MB children=$nkids"
}

for r in $(seq 1 "$RUNS"); do
  if [ $((r % 2)) -eq 1 ]; then order="tfast tfl"; else order="tfl tfast"; fi
  for app in $order; do
    run_one "$app" "$r"
    sleep 3
  done
done

python3 - "$LINES" "$OUT" "$DATE" "$RUNS" <<'PYEOF'
import json, sys, re, statistics, subprocess
lines_path, out, date, runs = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
rows = []
for ln in open(lines_path):
    d = dict(re.findall(r"(\w+)=(\S+)", ln.strip().rstrip(',')))
    if "ERROR" in ln or "first_paint_ms" not in d: continue
    d["run"] = int(d["run"]); d["first_paint_ms"] = int(d["first_paint_ms"])
    d["pss_kb"] = int(d["pss_kb"]); d["children"] = int(d["children"])
    d["pss_kb_samples"] = [int(x) for x in d.pop("samples").split(",") if x]
    rows.append(d)
doc = {
    "date": date,
    "machine": subprocess.run(["uname","-r"],capture_output=True,text=True).stdout.strip(),
    "tfast": subprocess.run(["git","describe","--tags"],capture_output=True,text=True).stdout.strip() or "dev",
    "tfl": "2.26.0 (extracted AppImage, --disable-gpu --no-sandbox, fresh HOME)",
    "runs": rows,
}
json.dump(doc, open(f"{out}/measurements.json","w"), indent=1)
for app in ("tfast","tfl"):
    sel = [r for r in rows if r["app"]==app]
    if not sel: continue
    med = statistics.median(r["pss_kb"] for r in sel)
    fp = statistics.median(r["first_paint_ms"] for r in sel)
    ch = max(r["children"] for r in sel)
    print(f"{app}: median idle PSS {med/1024:.1f} MB, median first paint {fp:.0f} ms, max children {ch} ({len(sel)} runs)")
PYEOF
echo "wrote $OUT/measurements.json"
