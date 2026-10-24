#!/usr/bin/env bash
# usage: tools/ci_run.sh "<title>" <command...>
# Runs the command, streams its output, and when it fails publishes the LAST lines as one GitHub annotation, which is
# readable through the public API and on the run summary page (plain "exit code 1" says nothing).
title="$1"; shift
log="$(mktemp)"
"$@" 2>&1 | tee "$log"
code=${PIPESTATUS[0]}
if [ "$code" -ne 0 ]; then
  # annotation messages need %, CR and LF escaped
  msg="$(grep -vE '^\s*$' "$log" | tail -n 45 | sed -e 's/%/%25/g' -e 's/\r//g' | awk '{printf "%s%%0A", $0}')"
  echo "::error title=${title} failed (exit ${code})::${msg}"
fi
exit "$code"
