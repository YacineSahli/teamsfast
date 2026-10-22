#!/bin/bash
# E2E call rig harness. Drives TeamsFast instances via the single-instance
# control channel (src/control.rs, `teamsfast --ctl <verb>`) — no keyboard
# or mouse injection exists in this sandbox.
#
# usage:
#   qa_call_e2e.sh selftest            # ONE signed-in instance (default
#                                      # profile = the real account):
#                                      # ping → echo call → connected →
#                                      # mute → hangup → ended
#   qa_call_e2e.sh login <profile>     # launch <profile> signed-out and
#                                      # screenshot the device-code sign-in
#                                      # (one-time provisioning; tokens land
#                                      # in keyring tokens-<profile>)
#   qa_call_e2e.sh pair <a_prof> <a_upn> <b_prof> <b_upn>
#                                      # A calluser B → B rings → B answers
#                                      # → both connected → B mutes →
#                                      # A hangs up → both ended
#   qa_call_e2e.sh pairvideo <a_prof> <a_upn> <b_prof> <b_upn> [<thread>]
#                                      # A videocall B (consumer BUNDLE) →
#                                      # B rings video=true → answer →
#                                      # both connected + VIDEO decoded on
#                                      # both sides → A hangs up → ended.
#                                      # <thread> overrides the callee
#                                      # thread (default: auto-resolved —
#                                      # the newest 1:1 BOTH archives
#                                      # carry; falls back to explicit).
#   qa_call_e2e.sh pairshare <a_prof> <a_upn> <b_prof> <b_upn> [<thread>]
#                                      # pairvideo PLUS the share path:
#                                      # A auto-shares the synthetic
#                                      # 1280x720 source, B must DECODE
#                                      # it, A stops + re-shares
#                                      # mid-call (encoder re-init),
#                                      # B decodes again, clean hang-up.
#   qa_call_e2e.sh pairdecline <a_prof> <a_upn> <b_prof> <b_upn>
#                                      # A calls B → B declines (603) →
#                                      # A ends on the decline → A is
#                                      # immediately re-callable (slot
#                                      # hygiene + ring-dedupe guard).
#
# Every instance runs on its own Xvfb display with per-profile XDG dirs
# under /tmp/tf-<profile> (files) + TEAMSFAST_PROFILE (keyring slots).
set -u
cd "$(dirname "$0")"
BIN=./target/debug/teamsfast
MODE="${1:-}"

ctl() { # ctl <profile> <verb...>
  local p="$1"; shift
  TEAMSFAST_PROFILE="$p" $BIN --ctl "$*" 2>&1
}

wait_log() { # wait_log <profile> <pattern> <timeout_s>
  local p="$1" pat="$2" tmo="${3:-30}"
  # launch() redirects EVERY profile's XDG state (default included), so
  # the harness always reads the redirected copy — never the user's own
  # ~/.local/state log.
  local log="/tmp/tf-$p/st/teamsfast/teamsfast.log"
  local waited=0
  while [ $waited -lt $tmo ]; do
    if grep -aq "$pat" "$log" 2>/dev/null; then return 0; fi
    sleep 1; waited=$((waited+1))
  done
  echo "TIMEOUT waiting for '$pat' in $log (${tmo}s)"
  return 1
}

count_log() { # count_log <profile> <pattern> — matching lines in the rig log
  local n
  n=$(grep -ac "$2" "/tmp/tf-$1/st/teamsfast/teamsfast.log" 2>/dev/null) || true
  echo "${n:-0}"
}

wait_count_gt() { # wait_count_gt <profile> <pattern> <baseline> <timeout_s>
  local p="$1" pat="$2" base="$3" tmo="${4:-30}" waited=0 n
  while [ "$waited" -lt "$tmo" ]; do
    n=$(count_log "$p" "$pat")
    [ "$n" -gt "$base" ] && return 0
    sleep 1; waited=$((waited+1))
  done
  echo "TIMEOUT waiting for '$pat' count > $base in /tmp/tf-$p log (${tmo}s)"
  return 1
}

launch() { # launch <profile> <display> [extra env assignments...]
  local p="$1" disp="$2"; shift 2
  local dir="/tmp/tf-$p"
  mkdir -p "$dir/cfg" "$dir/st" "$dir/data"
  Xvfb ":$disp" -screen 0 1100x760x24 >/dev/null 2>&1 &
  eval "XV_$p=\$!"
  env -u WAYLAND_DISPLAY DISPLAY=":$disp" \
    TEAMSFAST_PROFILE="$p" \
    XDG_CONFIG_HOME="$dir/cfg" XDG_STATE_HOME="$dir/st" XDG_DATA_HOME="$dir/data" \
    "$@" $BIN >/dev/null 2>&1 &
  eval "APP_$p=\$!"
}

kill_profile() { # kill_profile <profile>
  local p="$1"
  eval "kill \$APP_$p 2>/dev/null; sleep 0.3; kill \$XV_$p 2>/dev/null"
}

alive() { # alive <profile> — ctl ping reachable
  [ "$(ctl "$1" ping)" = "ok" ]
}

# Shared 1:1 threads of two profiles, newest first (intersection of the
# two encrypted archives; falls back to explicit <thread> args when the
# keyring keys or sqlcipher are unavailable). Emits "time id" lines.
pair_chats() { # pair_chats <profile>
  local p="$1" key
  local db="/tmp/tf-$p/st/teamsfast/archive.db"
  [ -r "$db" ] || return 0
  key=$(secret-tool lookup service teamsfast username "archive-key-$p" 2>/dev/null)
  [ -n "$key" ] || return 0
  # NOTE: sqlcipher prints "ok" before result rows and uses "|" as the
  # column separator — drop the former, translate the latter (thread
  # ids contain neither).
  sqlcipher "$db" "PRAGMA key=\"$key\"; SELECT last_message_time, id FROM chats WHERE is_group=0;" 2>/dev/null \
    | grep -v '^ok$' | tr '|' ' '
}

resolve_pair_thread() { # resolve_pair_thread <a_prof> <b_prof> — newest thread id BOTH archives carry
  # pair_chats emits "time id"; join -j 2 re-orders to "id time_a time_b"
  join -j 2 <(pair_chats "$1" | sort -k2,2) <(pair_chats "$2" | sort -k2,2) 2>/dev/null \
    | sort -k2,2r | head -1 | cut -d' ' -f1
}

case "$MODE" in
selftest)
  if pgrep -x teamsfast >/dev/null; then
    echo "REFUSE: a TeamsFast instance is already running (default profile would hand off to it)"; exit 1
  fi
  launch default 79
  sleep 8
  fail=0
  step() { echo "== $1 =="; shift; "$@" || fail=1; }
  step "ping"          alive default
  step "echo call"     [ "$(ctl default echo)" = "ok" ]
  step "connected"     wait_log default "CALLQA phase=connected" 45
  step "mute on"       [ "$(ctl default 'mute on')" = "ok" ]
  step "mute applied"  wait_log default "CALLQA mute=true" 5
  step "hangup"        [ "$(ctl default hangup)" = "ok" ]
  step "ended"         wait_log default "CALLQA phase=ended" 15
  echo "== CALLQA trace =="
  grep -a "CALLQA" /tmp/tf-default/st/teamsfast/teamsfast.log | tail -12
  kill_profile default
  [ $fail -eq 0 ] && echo "SELFTEST PASS" || echo "SELFTEST FAIL"
  exit $fail
  ;;
login)
  P="${2:?usage: qa_call_e2e.sh login <profile>}"
  launch "$P" 79
  sleep 8
  ffmpeg -y -loglevel error -f x11grab -video_size 1100x760 -i :79 -frames:v 1 "/tmp/tf-$P-login.png" 2>/dev/null
  echo "sign-in screen: /tmp/tf-$P-login.png — complete the device code"
  echo "(keyring tokens-$P fills once approved). Polling for sign-in…"
  waited=0
  while [ $waited -lt 300 ]; do
    # The chat-list endpoint line only appears once tokens work.
    if wait_log "$P" "endpoint hint" 1; then
      echo "SIGNED IN after ${waited}s"; break
    fi
    sleep 5; waited=$((waited+5))
  done
  kill_profile "$P"
  ;;
pair)
  A_P="${2:?a profile}" A_UPN="${3:?a upn}" B_P="${4:?b profile}" B_UPN="${5:?b upn}"
  launch "$A_P" 81; launch "$B_P" 82
  echo "== waiting for both instances =="
  sleep 10
  fail=0
  step() { echo "== $1 =="; shift; "$@" || fail=1; }
  step "A alive" alive "$A_P"
  step "B alive" alive "$B_P"
  step "A calls B (calluser)" [ "$(ctl "$A_P" "calluser $B_UPN")" = "ok" ]
  step "B rings"             wait_log "$B_P" "CALLQA phase=ringing" 30
  step "B answers"           [ "$(ctl "$B_P" answer)" = "ok" ]
  step "B accepted"          wait_log "$B_P" "CALLQA action=accept" 5
  step "A connected"         wait_log "$A_P" "CALLQA phase=connected" 45
  step "B connected"         wait_log "$B_P" "CALLQA phase=connected" 45
  sleep 5   # hold the call
  step "B mutes"             [ "$(ctl "$B_P" 'mute on')" = "ok" ]
  sleep 2
  step "A hangs up"          [ "$(ctl "$A_P" hangup)" = "ok" ]
  step "A ended"             wait_log "$A_P" "CALLQA phase=ended" 15
  # Consumer callees learn of the hang-up via the 30s frozen-recv
  # watchdog (the CC delivers no call/end) — 50s like pairvideo.
  step "B ended"             wait_log "$B_P" "CALLQA phase=ended" 50
  echo "== A trace =="; grep -a "CALLQA" "/tmp/tf-$A_P/st/teamsfast/teamsfast.log" | tail -10
  echo "== B trace =="; grep -a "CALLQA" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" | tail -10
  kill_profile "$A_P"; kill_profile "$B_P"
  [ $fail -eq 0 ] && echo "PAIR PASS" || echo "PAIR FAIL"
  exit $fail
  ;;
pairvideo)
  A_P="${2:?a profile}" A_UPN="${3:?a upn}" B_P="${4:?b profile}" B_UPN="${5:?b upn}"
  THREAD="${6:-}"
  if [ -z "$THREAD" ]; then
    THREAD=$(resolve_pair_thread "$A_P" "$B_P")
    if [ -n "$THREAD" ]; then
      echo "NOTE: auto-resolved pair thread from both archives"
    else
      echo "NOTE: thread auto-resolve unavailable (keyring/sqlcipher/archive) — pass the pair 1:1 thread id"
      exit 2
    fi
  fi
  launch "$A_P" 81; launch "$B_P" 82
  echo "== waiting for both instances =="
  sleep 10
  fail=0
  step() { echo "== $1 =="; shift; "$@" || fail=1; }
  step "A alive" alive "$A_P"
  step "B alive" alive "$B_P"
  step "A video-calls B"   [ "$(ctl "$A_P" "videocall $THREAD")" = "ok" ]
  step "B rings (video)"   wait_log "$B_P" "CALLQA phase=ringing caller=.* video=true" 30
  step "B answers"         [ "$(ctl "$B_P" answer)" = "ok" ]
  step "B accepted"        wait_log "$B_P" "CALLQA action=accept" 5
  step "A connected"       wait_log "$A_P" "CALLQA phase=connected" 45
  step "B connected"       wait_log "$B_P" "CALLQA phase=connected" 45
  # The BUNDLE video evidence: the callee answered mid1 for real and BOTH
  # sides decode frames within the hold window.
  sleep 8   # hold + let video establish
  step "B answered video"  wait_log "$B_P" "BUNDLE video answered" 5
  step "B decodes video"   wait_log "$B_P" "Recv video frame decoded" 10
  step "A decodes video"   wait_log "$A_P" "Recv video frame decoded" 10
  step "A hangs up"        [ "$(ctl "$A_P" hangup)" = "ok" ]
  step "A ended"           wait_log "$A_P" "CALLQA phase=ended" 15
  # The end summary carries both directions' video counters.
  step "A audio+video"     wait_log "$A_P" "video [1-9][0-9]* sent / [1-9][0-9]* received" 5
  step "B ended"           wait_log "$B_P" "CALLQA phase=ended" 50
  echo "== A trace =="; grep -a "CALLQA" "/tmp/tf-$A_P/st/teamsfast/teamsfast.log" | tail -10
  echo "== B video =="; grep -aE "BUNDLE video answered|Recv video frame decoded|Audio stats" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" | tail -6
  kill_profile "$A_P"; kill_profile "$B_P"
  [ $fail -eq 0 ] && echo "PAIRVIDEO PASS" || echo "PAIRVIDEO FAIL"
  exit $fail
  ;;
pairshare)
  # A videocalls B with the synthetic screen source (caller only —
  # the callee launches WITHOUT the env so decode assertions stay
  # unambiguous): the auto-share fires at call start, B must decode
  # the 1280x720 share, then A stops and RE-starts the share mid-call
  # (encoder re-init + second IDR), B decodes again, clean hang-up.
  A_P="${2:?a profile}" A_UPN="${3:?a upn}" B_P="${4:?b profile}" B_UPN="${5:?b upn}"
  THREAD="${6:-$(resolve_pair_thread "$A_P" "$B_P")}"
  if [ -z "$THREAD" ]; then
    echo "NOTE: thread auto-resolve unavailable — pass the pair 1:1 thread id"
    exit 2
  fi
  launch "$A_P" 81 TEAMSFAST_SCREENSHARE=1 TEAMSFAST_NO_RINGBACK=1
  launch "$B_P" 82 TEAMSFAST_NO_RINGBACK=1
  echo "== waiting for both instances =="
  sleep 10
  fail=0
  step() { echo "== $1 =="; shift; "$@" || fail=1; }
  step "A alive" alive "$A_P"
  step "B alive" alive "$B_P"
  step "A video-calls B"   [ "$(ctl "$A_P" "videocall $THREAD")" = "ok" ]
  step "B rings (video)"   wait_log "$B_P" "CALLQA phase=ringing caller=.* video=true" 30
  step "B answers"         [ "$(ctl "$B_P" answer)" = "ok" ]
  step "B accepted"        wait_log "$B_P" "CALLQA action=accept" 5
  step "A connected"       wait_log "$A_P" "CALLQA phase=connected" 45
  step "B connected"       wait_log "$B_P" "CALLQA phase=connected" 45
  step "A auto-shares"     wait_log "$A_P" "CALLQA share=auto (" 15
  step "B decodes 720p share" wait_log "$B_P" "Recv video frame decoded: 1280x720" 30
  before=$(grep -ac "Recv video frame decoded: 1280x720" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" 2>/dev/null || echo 0)
  step "A stops share"     [ "$(ctl "$A_P" 'share off')" = "ok" ]
  step "A share off logged" wait_log "$A_P" "CALLQA share=off" 5
  sleep 3   # let the leg settle back on camera frames
  step "A re-shares mid-call" [ "$(ctl "$A_P" 'share on')" = "ok" ]
  step "A share on logged"  wait_log "$A_P" "CALLQA share=on" 5
  sleep 8   # encoder re-init + IDR + PLI recovery window
  after=$(grep -ac "Recv video frame decoded: 1280x720" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" 2>/dev/null || echo 0)
  echo "== B 720p decodes: $before -> $after =="
  step "B decodes 720p again" [ "$after" -gt "$before" ]
  step "A hangs up"        [ "$(ctl "$A_P" hangup)" = "ok" ]
  step "A ended"           wait_log "$A_P" "CALLQA phase=ended" 15
  step "A audio+video"     wait_log "$A_P" "video [1-9][0-9]* sent / [1-9][0-9]* received" 5
  step "B ended"           wait_log "$B_P" "CALLQA phase=ended" 50
  echo "== A trace =="; grep -a "CALLQA" "/tmp/tf-$A_P/st/teamsfast/teamsfast.log" | tail -10
  echo "== B share =="; grep -aE "1280x720|share" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" | tail -6
  kill_profile "$A_P"; kill_profile "$B_P"
  [ $fail -eq 0 ] && echo "PAIRSHARE PASS" || echo "PAIRSHARE FAIL"
  exit $fail
  ;;
pairdecline)
  # A calls B (calluser — CreateOneToOne, no thread needed) → B rings →
  # B DECLINES (603 chain) → A's call ends on the decline, B's ring
  # clears → A is immediately re-callable → mid-ring hang-up also
  # clears B's ring. Regression for the ring-dedupe self-decline bug
  # class. NOTE: B never HAS a call task (it declines while ringing),
  # so B's terminal evidence is ring_gone — never "phase=ended".
  A_P="${2:?a profile}" A_UPN="${3:?a upn}" B_P="${4:?b profile}" B_UPN="${5:?b upn}"
  launch "$A_P" 81 TEAMSFAST_NO_RINGBACK=1; launch "$B_P" 82 TEAMSFAST_NO_RINGBACK=1
  echo "== waiting for both instances =="
  sleep 10
  fail=0
  step() { echo "== $1 =="; shift; "$@" || fail=1; }
  step "A alive" alive "$A_P"
  step "B alive" alive "$B_P"
  step "A calls B (calluser)" [ "$(ctl "$A_P" "calluser $B_UPN")" = "ok" ]
  step "B rings"             wait_log "$B_P" "CALLQA phase=ringing" 30
  step "B declines"          [ "$(ctl "$B_P" decline)" = "ok" ]
  step "B decline logged"    wait_log "$B_P" "CALLQA action=decline" 5
  step "B ring clears"       wait_log "$B_P" "CALLQA ring_gone" 20
  # The caller learns of the decline through the CC (run-12 chain).
  step "A ended on decline"  wait_log "$A_P" "CALLQA phase=ended" 30
  ring1=$(count_log "$B_P" "CALLQA phase=ringing")
  end1=$(count_log "$A_P" "CALLQA phase=ended")
  gone1=$(count_log "$B_P" "CALLQA ring_gone")
  step "A re-callable"       [ "$(ctl "$A_P" "calluser $B_UPN")" = "ok" ]
  step "B rings again"       wait_count_gt "$B_P" "CALLQA phase=ringing" "$ring1" 30
  step "cleanup hangup A"    [ "$(ctl "$A_P" hangup)" = "ok" ]
  step "A ended 2nd"         wait_count_gt "$A_P" "CALLQA phase=ended" "$end1" 20
  step "B ring clears 2nd"   wait_count_gt "$B_P" "CALLQA ring_gone" "$gone1" 30
  echo "== A trace =="; grep -a "CALLQA" "/tmp/tf-$A_P/st/teamsfast/teamsfast.log" | tail -8
  echo "== B trace =="; grep -a "CALLQA" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" | tail -8
  kill_profile "$A_P"; kill_profile "$B_P"
  [ $fail -eq 0 ] && echo "PAIRDECLINE PASS" || echo "PAIRDECLINE FAIL"
  exit $fail
  ;;
*)
  sed -n '2,16p' "$0"; exit 2 ;;
esac
