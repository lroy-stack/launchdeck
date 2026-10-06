#!/bin/sh
# Runs the physical acceptance suite on a Launchpad Mini MK3, or else a
# Launchpad X, plugged into this computer (LAUNCHPAD_MINI_MK3_PLAN.md
# section 15; docs/devices/hardware-testing.md).
#
# Close Ableton Live, Novation Components and anything else that uses the
# Launchpad first. You answer on the unit: the top right > button is yes, the
# bottom right button (Stop/Solo/Mute) is no; a question unanswered for 60 s
# is skipped. Without a unit the suite says NOT HARDWARE VERIFIED and passes.
#
#   LAUNCHDECK_ACCEPTANCE_REPORT  where the report goes (default: a new file
#                                 in the system's temporary folder)
#   LAUNCHDECK_ACCEPTANCE_DAW=1   also send the inquiry to the DAW interface
#   LAUNCHDECK_CARGO              the cargo to run (default: cargo)
#
# Never part of `npm run check` or CI: it needs a person and a unit.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "$LAUNCHDECK_ACCEPTANCE_REPORT" ]; then
  LAUNCHDECK_ACCEPTANCE_REPORT="${TMPDIR:-/tmp}"
  LAUNCHDECK_ACCEPTANCE_REPORT="${LAUNCHDECK_ACCEPTANCE_REPORT%/}/launchdeck-acceptance-$(date +%Y%m%d-%H%M%S).md"
fi
export LAUNCHDECK_ACCEPTANCE_REPORT
echo "Report: $LAUNCHDECK_ACCEPTANCE_REPORT"
cd "$root/src-tauri"
status=0
"${LAUNCHDECK_CARGO:-cargo}" test --lib launchpad_acceptance -- --ignored --nocapture --test-threads=1 || status=$?
echo "Report: $LAUNCHDECK_ACCEPTANCE_REPORT"
exit $status
