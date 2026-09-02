#!/bin/sh
# Starts the standalone fake grid on a *fixed* port with a *named* scenario and
# prints, once it actually answers, the two forms a viewer needs to reach it.
#
# The port is fixed rather than ephemeral because both viewers of a cross-check
# run are configured before either starts, and Firestorm caches a grid in its
# grid manager between runs.
#
# The printed host is the IPv4 literal 127.0.0.1, never `localhost`: that name
# resolves to ::1 first and this grid listens IPv4-only, so a viewer told
# `localhost` fails to connect for a reason that looks nothing like the cause.
#
# Runs in the foreground; Ctrl-C stops the grid.
set -eu

cd "$(dirname "$0")/.."

port=9100
scenario=catalogue
target=release

usage() {
  cat <<'USAGE'
Usage: scripts/fake-grid.sh [--port N] [--scenario NAME] [--debug]
                            [-- | ...sl-fake-grid arguments]

  --port N          the fixed TCP port to serve login, CAPS and get_grid_info
                    on (default 9100)
  --scenario NAME   the named scene every region shows (default "catalogue";
                    `sl-fake-grid --help` lists the names)
  --debug           run the debug build instead of the release one

Every other argument is passed to sl-fake-grid unchanged, so `--account
First:Last:password` and `--region Name@X,Y` work as they do there. A literal
`--` stops this script's own option parsing.
USAGE
}

# Parse by rotating the positional parameters past a sentinel: what survives to
# the end of the loop is exactly the pass-through argument list, with the quoting
# of a region name that contains a space intact.
sentinel='--sl-fake-grid-sh-end--'
set -- "$@" "${sentinel}"
while [ "$1" != "${sentinel}" ]; do
  case "$1" in
  --port | --scenario)
    if [ "$#" -lt 2 ] || [ "$2" = "${sentinel}" ]; then
      echo "$0: $1 wants a value" >&2
      exit 2
    fi
    case "$1" in
    --port) port="$2" ;;
    *) scenario="$2" ;;
    esac
    shift 2
    ;;
  --debug)
    target=debug
    shift
    ;;
  --help | -h)
    usage
    exit 0
    ;;
  --)
    shift
    while [ "$1" != "${sentinel}" ]; do
      set -- "$@" "$1"
      shift
    done
    ;;
  *)
    set -- "$@" "$1"
    shift
    ;;
  esac
done
shift

if ! command -v curl >/dev/null 2>&1; then
  echo "$0: curl is needed to wait for the grid to answer" >&2
  exit 1
fi

# Refuse a port something already answers on. Without this check a leftover grid
# from an earlier run answers the readiness probe below, the banner claims a grid
# that is not ours is ready, and the viewer then logs into last run's scene.
# curl's exit 7 is "could not connect", which is the only answer that means free.
preflight=0
curl --silent --max-time 2 --output /dev/null "http://127.0.0.1:${port}/" || preflight=$?
if [ "${preflight}" -ne 7 ]; then
  echo "$0: something is already listening on 127.0.0.1:${port}" >&2
  echo "$0: stop it (an earlier fake grid?) or pass --port" >&2
  exit 1
fi

# Build first, so a compile error is reported as a compile error rather than as
# a grid that never came up.
if [ "${target}" = release ]; then
  cargo build --release -p sl-fake-grid --bin sl-fake-grid
else
  cargo build -p sl-fake-grid --bin sl-fake-grid
fi

"target/${target}/sl-fake-grid" --http-port "${port}" --scenario "${scenario}" "$@" &
grid_pid=$!

# Ask the grid to stop rather than killing it outright: a session the simulator
# still believes is logged in makes the *next* run fail to log in, and that
# failure looks exactly like a viewer bug.
trap 'kill -INT "${grid_pid}" 2>/dev/null || true' INT TERM

# `kill -0` alone says yes to a process that has exited and not been reaped, so
# a grid that dies during startup would otherwise look alive for the whole
# timeout. Ask for its process state too.
grid_alive() {
  kill -0 "${grid_pid}" 2>/dev/null || return 1
  case "$(ps -o stat= -p "${grid_pid}" 2>/dev/null)" in
  *Z*) return 1 ;;
  *) return 0 ;;
  esac
}

# Wait for the grid to actually answer before printing the banner.
# `get_grid_info` is the document Firestorm fetches before it will even show a
# login screen, so answering it is the honest definition of ready.
waited=0
while :; do
  if ! grid_alive; then
    echo "$0: the grid exited before it was ready; its own log says why" >&2
    wait "${grid_pid}" || true
    exit 1
  fi
  if curl --silent --fail --max-time 2 "http://127.0.0.1:${port}/get_grid_info" \
    >/dev/null 2>&1; then
    break
  fi
  if [ "${waited}" -ge 60 ]; then
    echo "$0: the grid did not answer get_grid_info within 30s" >&2
    kill -INT "${grid_pid}" 2>/dev/null || true
    wait "${grid_pid}" || true
    exit 1
  fi
  waited=$((waited + 1))
  sleep 0.5
done

cat <<EOF

  fake grid ready on 127.0.0.1:${port}, scenario "${scenario}"

    this workspace's viewer   SL_LOGIN_URI=http://127.0.0.1:${port}/
    Firestorm                 --grid 127.0.0.1:${port} --multiple
                              (and FIRESTORM_X64_USER_DIR=<a fresh temp dir>,
                               or the run shares settings, cache, logs and the
                               credential store with your real session)

  Never "localhost": it resolves to ::1 first and this grid is IPv4-only.
  Ctrl-C stops the grid.

EOF

status=0
wait "${grid_pid}" || status=$?
exit "${status}"
