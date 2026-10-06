#!/bin/bash
# Manual-test launcher: builds, runs TeamsFast, and preserves the session
# log on exit so it survives the next launch (the log file truncates at
# every startup).
#
# usage: ./test.sh            # normal run
#        ./test.sh --verbose  # args pass through to the app
set -u
cd "$(dirname "$0")"

cargo build 2>&1 | grep -E "^error" && { echo "build failed"; exit 1; }

./target/debug/teamsfast "$@"
STATUS=$?

LOG="$HOME/.local/state/teamsfast/teamsfast.log"
cp "$LOG" "$HOME/.local/state/teamsfast/last-session.log" 2>/dev/null
echo
echo "session ended (exit $STATUS)."
echo "log preserved: $HOME/.local/state/teamsfast/last-session.log"
exit $STATUS
