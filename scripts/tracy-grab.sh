#!/usr/bin/env bash
#
# tracy-grab.sh — capture a bounded Tracy window from the running viewer and
# export it to machine-readable TSV (for eyeballing, grepping, or handing to a
# tool/LLM instead of the graphical timeline).
#
# The viewer must be built and running with `--features profile-tracy` (see
# book/src/tools/profiling.md). Tracy accepts only ONE profiler connection at a
# time, so DISCONNECT the Tracy GUI before capturing — or, to keep the GUI, use
# its File -> Save trace and run `tracy-csvexport` on the file yourself.
#
# The `profile-tracy` build runs Tracy in on-demand mode (the `ondemand`
# feature), so it collects nothing until a profiler connects and discards on
# disconnect — safe to leave running and connect to only when capturing.
#
# Tools: `tracy-capture` and `tracy-csvexport` are taken from $PATH if present,
# else from $TRACY_DIR (default ~/devel/3rdparty/tracy), where they build with
#   cmake -S capture   -B capture/build   -DCMAKE_BUILD_TYPE=Release
#   cmake -S csvexport -B csvexport/build -DCMAKE_BUILD_TYPE=Release
#   cmake --build capture/build --build csvexport/build
#
# Usage: scripts/tracy-grab.sh [seconds] [outdir]
#   seconds : capture duration (default 10)
#   outdir  : output directory (default: tracy-grab-<seconds>s in the CWD)
set -euo pipefail

SECS="${1:-10}"
OUT="${2:-tracy-grab-${SECS}s}"
SEP=$'\t'

TRACY_DIR="${TRACY_DIR:-${HOME}/devel/3rdparty/tracy}"
CAP="$(command -v tracy-capture || true)"
CSV="$(command -v tracy-csvexport || true)"
[[ -n "${CAP}" ]] || CAP="${TRACY_DIR}/capture/build/tracy-capture"
[[ -n "${CSV}" ]] || CSV="${TRACY_DIR}/csvexport/build/tracy-csvexport"

for tool in "${CAP}" "${CSV}"; do
  if [[ ! -x "${tool}" ]]; then
    echo "error: not found or not executable: ${tool}" >&2
    echo "build the Tracy utilities (see the header of this script) or set \$TRACY_DIR." >&2
    exit 1
  fi
done

mkdir -p "${OUT}"
TRACE="${OUT}/trace.tracy"

echo "Capturing ${SECS}s -> ${TRACE} (the Tracy GUI must be disconnected)…"
"${CAP}" -o "${TRACE}" -s "${SECS}" -f

# Sort the aggregate exports by total time (column 4) descending, keeping the
# header row on top. Tab-separated so commas inside zone names stay intact.
sort_by_total() { {
  IFS= read -r h
  printf '%s\n' "${h}"
  sort -t"${SEP}" -k4 -nr
}; }

# Self time — time in each zone excluding children. This is the view that
# surfaces which systems actually burn the frame (main-thread stalls included).
"${CSV}" -e -s "${SEP}" "${TRACE}" | sort_by_total >"${OUT}/zones-self.tsv"

# Inclusive time — each zone including its children (parent schedules/stages).
"${CSV}" -s "${SEP}" "${TRACE}" | sort_by_total >"${OUT}/zones-inclusive.tsv"

# Log messages (chat, warnings, our own tracing events).
"${CSV}" -m -s "${SEP}" "${TRACE}" >"${OUT}/messages.tsv" || true

echo "Wrote:"
echo "  ${OUT}/zones-self.tsv        self time, sorted by total"
echo "  ${OUT}/zones-inclusive.tsv   inclusive time, sorted by total"
echo "  ${OUT}/messages.tsv          log messages"
echo
echo "Top 15 self-time zones:"
column -t -s"${SEP}" "${OUT}/zones-self.tsv" | head -16

# Per-event timeline for one zone (huge — always filtered):
#   tracy-csvexport -u -f composite_minimap "${TRACE}"
