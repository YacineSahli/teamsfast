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

launch() { # launch <profile> <display>
  local p="$1" disp="$2"
  local dir="/tmp/tf-$p"
  mkdir -p "$dir/cfg" "$dir/st" "$dir/data"
  Xvfb ":$disp" -screen 0 1100x760x24 >/dev/null 2>&1 &
  eval "XV_$p=\$!"
  env -u WAYLAND_DISPLAY DISPLAY=":$disp" \
    TEAMSFAST_PROFILE="$p" \
    XDG_CONFIG_HOME="$dir/cfg" XDG_STATE_HOME="$dir/st" XDG_DATA_HOME="$dir/data" \
    $BIN >/dev/null 2>&1 &
  eval "APP_$p=\$!"
}

kill_profile() { # kill_profile <profile>
  local p="$1"
  eval "kill \$APP_$p 2>/dev/null; sleep 0.3; kill \$XV_$p 2>/dev/null"
}

alive() { # alive <profile> — ctl ping reachable
  [ "$(ctl "$1" ping)" = "ok" ]
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
  step "B ended"             wait_log "$B_P" "CALLQA phase=ended" 15
  echo "== A trace =="; grep -a "CALLQA" "/tmp/tf-$A_P/st/teamsfast/teamsfast.log" | tail -10
  echo "== B trace =="; grep -a "CALLQA" "/tmp/tf-$B_P/st/teamsfast/teamsfast.log" | tail -10
  kill_profile "$A_P"; kill_profile "$B_P"
  [ $fail -eq 0 ] && echo "PAIR PASS" || echo "PAIR FAIL"
  exit $fail
  ;;
*)
  sed -n '2,16p' "$0"; exit 2 ;;
esac
