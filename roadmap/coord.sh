#!/bin/sh
# Coordination between several agents working one repository from separate git
# worktrees. Copy this next to index.py, inside the repo's roadmap/ directory.
#
# Two problems, one script:
#
#   WHO WORKS ON WHAT.  Each agent registers itself, claims a roadmap item, and
#   can see every other agent's claim plus the top-level paths its unmerged
#   commits already touch. That state lives in the *shared* git directory
#   (`git rev-parse --git-common-dir`), which every linked worktree resolves to
#   the same place, so it is visible from everywhere and committed nowhere. It
#   is deliberately ephemeral: the durable record of who did what stays the
#   roadmap item's status directory in git history.
#
#   WHO GETS TO BUILD.  Compiling, linking, testing and committing a large
#   workspace are memory-hungry enough that two at once can exhaust the machine.
#   `heavy` gates such a command behind a small semaphore, a free-memory floor,
#   and a transient systemd scope that confines an out-of-memory kill to the
#   command instead of the agent that started it.
#
# WHY THE SCOPE MATTERS. systemd-oomd kills whole cgroups, not single processes.
# A build started straight from an agent's terminal shares that terminal's
# scope, so killing the memory hog kills the agent and every one of its
# subprocesses with it. Running the build in its own transient scope makes the
# build the candidate and the only casualty.
#
# Every command is safe to run from any worktree and any directory inside it.
# State mutations are serialised with flock(1).
set -eu

# --------------------------------------------------------------------------
# Locations
# --------------------------------------------------------------------------

# This script lives inside roadmap/, the same assumption index.py makes.
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
roadmap_dir="${script_dir}"

if ! toplevel=$(git rev-parse --show-toplevel 2>/dev/null); then
  # As a PreToolUse hook this must never fail loudly: a hook exiting 2 *blocks*
  # the command it was asked about, so a shell command run outside any worktree
  # would become unrunnable. Exit 0 (allow) instead.
  if [ "${1:-}" = 'hook-pretooluse' ]; then
    exit 0
  fi
  echo "coord.sh: not inside a git worktree" >&2
  exit 2
fi

# --path-format=absolute keeps us from having to guess what the bare
# `--git-common-dir` output is relative to: in the primary worktree it is the
# literal string ".git", relative to the *current directory*, not the toplevel.
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)

state_dir="${common_dir}/roadmap-coord"
agents_dir="${state_dir}/agents"
lock_file="${state_dir}/lock"

# --------------------------------------------------------------------------
# Configuration (defaults, then roadmap/coord.conf overrides them)
# --------------------------------------------------------------------------

# How many heavy operations may run at once.
SLOTS=2
# Seconds between polls while waiting for a slot or for memory.
POLL_INTERVAL=5
# The branch unmerged work is measured against. Empty = autodetect.
BASE_BRANCH=''
# Extended regex of commands considered "heavy" by the PreToolUse hook.
HEAVY_PATTERNS='(^|[;&|[:space:]])(cargo|git[[:space:]]+(commit|push)|make|ninja)([[:space:]]|$)'
# Refuse to start a heavy command below this much available memory (MiB).
MIN_AVAIL_MB=0
MIN_AVAIL_EXCLUSIVE_MB=0
# A shell command; success means some other heavy build owns the machine.
EXTERNAL_LOAD_DETECT=''
# Run heavy commands inside a transient systemd scope.
USE_SCOPE=1
SCOPE_MEMORY_MAX='60%'
SCOPE_MEMORY_MAX_EXCLUSIVE='85%'
# Deliberately empty by default. A MemoryHigh far below the real working set
# does not throttle-then-succeed on a swapless machine, it thrashes reclaim
# forever -- which an agent cannot distinguish from a hang. An outright kill is
# more useful. Set this only just below SCOPE_MEMORY_MAX, if at all.
SCOPE_MEMORY_HIGH=''
# Where the slot files live. Override to a fixed path shared by several repos
# if agents work more than one workspace on the same machine -- memory is a
# property of the machine, not of the repository.
SLOT_DIR=''

if [ -f "${roadmap_dir}/coord.conf" ]; then
  # shellcheck source=/dev/null
  . "${roadmap_dir}/coord.conf"
fi

# Environment overrides, for one-off use.
SLOTS=${ROADMAP_COORD_SLOTS:-${SLOTS}}
USE_SCOPE=${ROADMAP_COORD_USE_SCOPE:-${USE_SCOPE}}

if [ -z "${SLOT_DIR}" ]; then
  SLOT_DIR="${state_dir}/slots"
fi

# systemd unit names are global to the user, while the slot numbering is per
# pool, so mix the pool path into the name or two repositories would collide.
unit_prefix="roadmap-coord-$(printf '%s' "${SLOT_DIR}" | md5sum | cut -c1-8)"

# --------------------------------------------------------------------------
# Small helpers
# --------------------------------------------------------------------------

note() {
  echo "coord: $*" >&2
}

die() {
  echo "coord: $*" >&2
  exit 1
}

# The agent id is the systemd-escaped absolute worktree path. That is injective
# over the whole filesystem, so a worktree created under /tmp gets an id as
# distinct as one under ~/devel, and it decodes back to the path it came from.
agent_id() {
  if [ -n "${ROADMAP_COORD_AGENT:-}" ]; then
    printf '%s\n' "${ROADMAP_COORD_AGENT}"
  else
    systemd-escape --path "${toplevel}"
  fi
}

# Reverse of the above, for display.
agent_path() {
  systemd-escape --path --unescape "$1" 2>/dev/null || printf '%s\n' "$1"
}

# A short human label for messages. The full id encodes an absolute path and so
# can be very long; the worktree's own directory name is what a person actually
# recognises.
agent_label() {
  basename "$(agent_path "$1")"
}

# Read one KEY=VALUE line. The key is a literal we control, the value may
# contain spaces, so take everything after the first '='.
meta_get() {
  [ -f "$1" ] || return 0
  sed -n "s/^$2=//p" "$1" | head -n 1
}

mem_available_mb() {
  awk '/^MemAvailable:/ { printf "%d\n", $2 / 1024; exit }' /proc/meminfo
}

# Field 22 of /proc/<pid>/stat. The comm field can contain spaces and
# parentheses, so cut everything through the last ") " first; starttime is then
# the 20th remaining field.
proc_starttime() {
  [ -r "/proc/$1/stat" ] || return 1
  sed 's/.*) //' "/proc/$1/stat" | cut -d' ' -f20
}

# The cgroup of the *session* this script was invoked from -- in practice the
# terminal scope the agent runs in.
#
# This, not a pid, is what an agent's lifetime actually is. Every command an
# agent runs is a fresh short-lived shell, so recording $PPID would mark the
# agent dead the moment the command that registered it returned, and the next
# command would reap its own claim. The enclosing scope, by contrast, lives
# exactly as long as the agent session does.
agent_scope() {
  # Strip our own transient scope if we happen to be inside one, so a
  # registration from within `heavy` still names the session.
  sed -n 's|^0::||p' /proc/self/cgroup |
    sed 's|/roadmap-coord-[^/]*\.scope$||'
}

# An agent is alive when the session cgroup it registered from still exists.
# Falls back to pid liveness (with a start-time check, so a recycled pid cannot
# make a long-dead agent look busy) where no cgroup was recorded.
agent_alive() {
  _al_dir=$1
  _al_cg=$(meta_get "${_al_dir}/meta" cgroup)
  if [ -n "${_al_cg}" ]; then
    [ -d "/sys/fs/cgroup${_al_cg}" ]
    return $?
  fi
  _al_pid=$(meta_get "${_al_dir}/meta" pid)
  [ -n "${_al_pid}" ] || return 1
  kill -0 "${_al_pid}" 2>/dev/null || return 1
  _al_recorded=$(meta_get "${_al_dir}/meta" starttime)
  [ -n "${_al_recorded}" ] || return 0
  _al_now=$(proc_starttime "${_al_pid}" 2>/dev/null || echo '')
  [ "${_al_now}" = "${_al_recorded}" ]
}

# --------------------------------------------------------------------------
# State mutation, always under the mutex
# --------------------------------------------------------------------------

ensure_state() {
  mkdir -p "${agents_dir}" "${SLOT_DIR}"
  [ -f "${lock_file}" ] || : >"${lock_file}"
}

# with_lock <function> [args...]
with_lock() {
  ensure_state
  # A subshell so the lock is released by the shell exiting the block; the
  # called function only ever writes files, never variables we need back.
  (flock 9 && "$@") 9>"${lock_file}"
}

reap_dead() {
  [ -d "${agents_dir}" ] || return 0
  for _rd_dir in "${agents_dir}"/*; do
    [ -d "${_rd_dir}" ] || continue
    if ! agent_alive "${_rd_dir}"; then
      note "reaping dead agent $(agent_label "$(basename "${_rd_dir}")")"
      rm -rf "${_rd_dir}"
    fi
  done
}

do_register() {
  _dr_dir="${agents_dir}/$(agent_id)"
  mkdir -p "${_dr_dir}"
  {
    printf 'id=%s\n' "$(agent_id)"
    printf 'cgroup=%s\n' "${ROADMAP_COORD_CGROUP:-$(agent_scope)}"
    printf 'pid=%s\n' "${ROADMAP_COORD_PID:-${PPID}}"
    printf 'starttime=%s\n' "$(proc_starttime "${ROADMAP_COORD_PID:-${PPID}}" 2>/dev/null || echo '')"
    printf 'worktree=%s\n' "${toplevel}"
    printf 'branch=%s\n' "$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')"
    printf 'host=%s\n' "$(hostname)"
    printf 'since=%s\n' "$(date -Is)"
  } >"${_dr_dir}/meta"
}

# --------------------------------------------------------------------------
# Unmerged-work awareness
# --------------------------------------------------------------------------

resolve_base() {
  if [ -n "${BASE_BRANCH}" ]; then
    printf '%s\n' "${BASE_BRANCH}"
    return 0
  fi
  if _rb=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null); then
    printf '%s\n' "${_rb}"
    return 0
  fi
  for _rb in master main; do
    if git rev-parse --verify --quiet "${_rb}" >/dev/null 2>&1; then
      printf '%s\n' "${_rb}"
      return 0
    fi
  done
  printf '%s\n' 'HEAD'
}

do_unmerged() {
  _du_dir="${agents_dir}/$(agent_id)"
  mkdir -p "${_du_dir}"
  _du_base=$(resolve_base)
  _du_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')

  if [ "${_du_base}" = 'HEAD' ] || ! git rev-parse --verify --quiet "${_du_base}" >/dev/null 2>&1; then
    {
      printf 'branch=%s\n' "${_du_branch}"
      printf 'base=\n'
      printf 'commits=0\n'
      printf 'paths=\n'
    } >"${_du_dir}/unmerged"
    return 0
  fi

  _du_commits=$(git rev-list --count "${_du_base}..HEAD" 2>/dev/null || echo 0)
  # Top-level path components only: in a cargo workspace that is the crate, and
  # crate-level overlap is what "two agents rewriting the same code" looks like.
  _du_paths=$(git diff --name-only "${_du_base}...HEAD" 2>/dev/null |
    cut -d/ -f1 | sort -u | tr '\n' ' ' | sed 's/ *$//')
  {
    printf 'branch=%s\n' "${_du_branch}"
    printf 'base=%s\n' "${_du_base}"
    printf 'commits=%s\n' "${_du_commits}"
    printf 'paths=%s\n' "${_du_paths}"
  } >"${_du_dir}/unmerged"
}

# --------------------------------------------------------------------------
# Claims
# --------------------------------------------------------------------------

# Ask index.py where an item lives. Absent or unknown id is not fatal here --
# a repo may adopt coord.sh before teaching index.py --locate.
locate_item() {
  [ -x "${roadmap_dir}/index.py" ] || return 1
  python3 "${roadmap_dir}/index.py" --locate "$1" 2>/dev/null
}

claim_holder() {
  [ -d "${agents_dir}" ] || return 0
  for _ch_dir in "${agents_dir}"/*; do
    [ -f "${_ch_dir}/claim" ] || continue
    [ "$(basename "${_ch_dir}")" != "$(agent_id)" ] || continue
    if [ "$(meta_get "${_ch_dir}/claim" task)" = "$1" ]; then
      basename "${_ch_dir}"
      return 0
    fi
  done
}

do_claim() {
  _dc_task=$1
  _dc_subsystem=$2
  _dc_note=$3

  _dc_holder=$(claim_holder "${_dc_task}")
  if [ -n "${_dc_holder}" ]; then
    die "'${_dc_task}' is already claimed by $(agent_label "${_dc_holder}") ($(agent_path "${_dc_holder}"))"
  fi

  # Warn -- never refuse -- when someone else is in the same area. An import
  # conflict is cheap; two rewrites of one subsystem are not.
  if [ -n "${_dc_subsystem}" ]; then
    for _dc_dir in "${agents_dir}"/*; do
      [ -f "${_dc_dir}/claim" ] || continue
      [ "$(basename "${_dc_dir}")" != "$(agent_id)" ] || continue
      if [ "$(meta_get "${_dc_dir}/claim" subsystem)" = "${_dc_subsystem}" ]; then
        note "WARNING: $(agent_label "$(basename "${_dc_dir}")") is also working on '${_dc_subsystem}'"
      fi
    done
  fi

  _dc_dir="${agents_dir}/$(agent_id)"
  mkdir -p "${_dc_dir}"
  {
    printf 'task=%s\n' "${_dc_task}"
    printf 'subsystem=%s\n' "${_dc_subsystem}"
    printf 'note=%s\n' "${_dc_note}"
    printf 'since=%s\n' "$(date -Is)"
  } >"${_dc_dir}/claim"
}

do_release() {
  rm -f "${agents_dir}/$(agent_id)/claim"
}

# --------------------------------------------------------------------------
# The heavy-operation semaphore
# --------------------------------------------------------------------------

held_slot=''
exclusive=0

external_load() {
  [ -n "${EXTERNAL_LOAD_DETECT}" ] || return 1
  sh -c "${EXTERNAL_LOAD_DETECT}" >/dev/null 2>&1
}

slot_is_held() {
  # Probe by trying to take it. Succeeding means it was free, so invert.
  if (flock -n -x 8) 8>"${SLOT_DIR}/slot.$1" 2>/dev/null; then
    return 1
  fi
  return 0
}

release_slot() {
  if [ -n "${held_slot}" ]; then
    rm -f "${SLOT_DIR}/slot.${held_slot}.owner"
  fi
}

wait_for_memory() {
  if [ "${ROADMAP_COORD_NO_MEM_GATE:-0}" = "1" ]; then
    return 0
  fi
  _wm_floor=$1
  if [ "${_wm_floor}" -le 0 ]; then
    return 0
  fi
  _wm_said=0
  while :; do
    _wm_avail=$(mem_available_mb)
    if [ "${_wm_avail}" -ge "${_wm_floor}" ]; then
      return 0
    fi
    if [ "${_wm_said}" -eq 0 ]; then
      note "waiting for memory: ${_wm_avail} MiB available, need ${_wm_floor} MiB"
      _wm_said=1
    fi
    sleep "${POLL_INTERVAL}"
  done
}

# The pool is a readers-writer lock plus N counted slots:
#
#   normal     -- shared lock on the gate, then one numbered slot
#   exclusive  -- exclusive lock on the gate, no numbered slot needed
#
# so an exclusive operation waits for every normal one to drain, and needs no
# second code path to "reduce the slots to one".
acquire() {
  ensure_state
  [ -f "${SLOT_DIR}/gate" ] || : >"${SLOT_DIR}/gate"

  if external_load; then
    note "external build load detected -- taking the machine exclusively"
    exclusive=1
  fi

  exec 7>"${SLOT_DIR}/gate"
  if [ "${exclusive}" -eq 1 ]; then
    if ! flock -n -x 7; then
      note 'waiting for every slot to drain (exclusive)'
      flock -x 7
    fi
    held_slot='exclusive'
    wait_for_memory "${MIN_AVAIL_EXCLUSIVE_MB}"
    return 0
  fi

  if ! flock -n -s 7; then
    note 'waiting: an exclusive operation holds the machine'
    flock -s 7
  fi

  _aq_said=0
  while :; do
    _aq_n=1
    while [ "${_aq_n}" -le "${SLOTS}" ]; do
      exec 8>"${SLOT_DIR}/slot.${_aq_n}"
      if flock -n -x 8; then
        held_slot=${_aq_n}
        wait_for_memory "${MIN_AVAIL_MB}"
        return 0
      fi
      exec 8>&-
      _aq_n=$((_aq_n + 1))
    done
    if [ "${_aq_said}" -eq 0 ]; then
      note "waiting for a free slot (all ${SLOTS} in use)"
      _aq_said=1
    fi
    sleep "${POLL_INTERVAL}"
  done
}

run_command() {
  _rc_unit="${unit_prefix}-${held_slot}"
  _rc_max=${SCOPE_MEMORY_MAX}
  if [ "${exclusive}" -eq 1 ]; then
    _rc_max=${SCOPE_MEMORY_MAX_EXCLUSIVE}
  fi

  # Run through `env --` so a command written with leading VAR=value
  # assignments works. Those are shell syntax, not part of a command's argv, so
  # execing them directly fails with "Failed to find executable VAR=value" --
  # and prefixing a build with RUSTFLAGS/RUSTDOCFLAGS is far too common to make
  # callers rewrite it. With no assignments `env` is a transparent passthrough.
  set -- env -- "$@"

  if [ "${USE_SCOPE}" != "1" ] || ! command -v systemd-run >/dev/null 2>&1; then
    # fds 7 and 8 are closed for the child so a backgrounded grandchild can
    # never keep the slot held after we exit.
    "$@" 7>&- 8>&-
    return $?
  fi

  # A crashed predecessor can leave the unit name behind.
  systemctl --user reset-failed "${_rc_unit}.scope" >/dev/null 2>&1 || true

  if [ -n "${SCOPE_MEMORY_HIGH}" ]; then
    set -- systemd-run --user --scope --collect -u "${_rc_unit}" \
      -p ManagedOOMMemoryPressure=kill \
      -p "MemoryHigh=${SCOPE_MEMORY_HIGH}" \
      -p "MemoryMax=${_rc_max}" \
      -- "$@"
  else
    set -- systemd-run --user --scope --collect -u "${_rc_unit}" \
      -p ManagedOOMMemoryPressure=kill \
      -p "MemoryMax=${_rc_max}" \
      -- "$@"
  fi
  "$@" 7>&- 8>&-
}

do_heavy() {
  _dh_label=$1
  shift
  with_lock do_register
  acquire
  trap release_slot EXIT INT TERM
  {
    printf 'agent=%s\n' "$(agent_id)"
    printf 'pid=%s\n' "$$"
    printf 'label=%s\n' "${_dh_label}"
    printf 'since=%s\n' "$(date -Is)"
  } >"${SLOT_DIR}/slot.${held_slot}.owner"

  _dh_status=0
  run_command "$@" || _dh_status=$?

  if [ "${_dh_status}" -eq 137 ]; then
    note ''
    note "the command was KILLED (exit 137) -- it exceeded MemoryMax=${_rc_max:-?},"
    note 'or systemd-oomd reclaimed its scope under machine-wide memory pressure.'
    note 'Your agent session was NOT affected: the kill was confined to the'
    note "transient scope ${unit_prefix}-${held_slot}.scope."
    note 'Confirm with:  journalctl -u systemd-oomd --since "5 min ago"'
    note 'Retry with --exclusive, or narrow the command (-p <crate>).'
  fi
  return "${_dh_status}"
}

# --------------------------------------------------------------------------
# Reporting
# --------------------------------------------------------------------------

do_status() {
  echo "pool:   ${SLOT_DIR}  (${SLOTS} slot(s))"
  if external_load; then
    echo 'load:   EXTERNAL BUILD ACTIVE -- heavy commands run exclusively'
  fi
  echo "memory: $(mem_available_mb) MiB available"

  _ds_n=1
  while [ "${_ds_n}" -le "${SLOTS}" ]; do
    if slot_is_held "${_ds_n}"; then
      _ds_owner="${SLOT_DIR}/slot.${_ds_n}.owner"
      if [ -f "${_ds_owner}" ]; then
        echo "slot ${_ds_n}: HELD by $(agent_label "$(meta_get "${_ds_owner}" agent)") -- $(meta_get "${_ds_owner}" label) (since $(meta_get "${_ds_owner}" since))"
      else
        echo "slot ${_ds_n}: HELD"
      fi
    else
      echo "slot ${_ds_n}: free"
      rm -f "${SLOT_DIR}/slot.${_ds_n}.owner"
    fi
    _ds_n=$((_ds_n + 1))
  done

  echo ''
  if [ ! -d "${agents_dir}" ] || [ -z "$(ls -A "${agents_dir}" 2>/dev/null)" ]; then
    echo 'agents: none registered'
    return 0
  fi

  for _ds_dir in "${agents_dir}"/*; do
    [ -d "${_ds_dir}" ] || continue
    _ds_id=$(basename "${_ds_dir}")
    echo "agent $(agent_label "${_ds_id}")"
    echo "  worktree: $(agent_path "${_ds_id}")"
    echo "  branch:   $(meta_get "${_ds_dir}/meta" branch)"
    if [ -f "${_ds_dir}/claim" ]; then
      echo "  claim:    $(meta_get "${_ds_dir}/claim" task) [$(meta_get "${_ds_dir}/claim" subsystem)] since $(meta_get "${_ds_dir}/claim" since)"
      _ds_note=$(meta_get "${_ds_dir}/claim" note)
      [ -z "${_ds_note}" ] || echo "  note:     ${_ds_note}"
    else
      echo '  claim:    (idle)'
    fi
    if [ -f "${_ds_dir}/unmerged" ]; then
      echo "  unmerged: $(meta_get "${_ds_dir}/unmerged" commits) commit(s) vs $(meta_get "${_ds_dir}/unmerged" base)"
      _ds_paths=$(meta_get "${_ds_dir}/unmerged" paths)
      [ -z "${_ds_paths}" ] || echo "  touching: ${_ds_paths}"
    fi
  done
}

# --------------------------------------------------------------------------
# The PreToolUse hook
# --------------------------------------------------------------------------

# Reads the Claude Code hook payload on stdin and denies a heavy command that
# is not already wrapped. A hook cannot rewrite the command, and it cannot hold
# a lock past its own exit, so denying with an instruction is the only shape
# that actually serialises anything.
do_hook() {
  if [ "${ROADMAP_COORD_BYPASS:-0}" = "1" ]; then
    exit 0
  fi
  command -v jq >/dev/null 2>&1 || exit 0

  _dh_payload=$(cat)
  _dh_cmd=$(printf '%s' "${_dh_payload}" | jq -r '.tool_input.command // ""')
  [ -n "${_dh_cmd}" ] || exit 0

  # Already wrapped, or is the wrapper itself.
  case "${_dh_cmd}" in
  *coord.sh*heavy*) exit 0 ;;
  *ROADMAP_COORD_BYPASS=1*) exit 0 ;;
  esac

  if ! printf '%s' "${_dh_cmd}" | grep -Eq "${HEAVY_PATTERNS}"; then
    exit 0
  fi

  # Everything after `--` is an argv, not a shell command, so a pipeline or a
  # redirect has to be handed to a shell explicitly or its operators would be
  # passed to the program as literal arguments.
  # SC2016 is a false positive here: these are case *patterns* matching the
  # literal characters in the inspected command string, not text we want the
  # shell to expand -- a literal `$(` is precisely what we are looking for.
  # shellcheck disable=SC2016
  case "${_dh_cmd}" in
  *'|'* | *';'* | *'&'* | *'>'* | *'<'* | *'$('* | *'`'*)
    _dh_suggest="roadmap/coord.sh heavy -- sh -c '<the command, quotes escaped>'"
    ;;
  *)
    _dh_suggest="roadmap/coord.sh heavy -- ${_dh_cmd}"
    ;;
  esac

  _dh_reason="This command is memory-heavy and several agents share this machine.
Run it through the coordinator so it takes a build slot and runs in its own
systemd scope (an OOM kill then loses the build, not your whole session):

  ${_dh_suggest}

Add --exclusive before the -- for a full or release build of the largest crate.
A leading VAR=value assignment is fine as-is. If you really must bypass, prefix
the command with ROADMAP_COORD_BYPASS=1."

  jq -n --arg reason "${_dh_reason}" \
    '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
  exit 0
}

# --------------------------------------------------------------------------
# Command line
# --------------------------------------------------------------------------

usage() {
  cat <<'USAGE'
Usage: roadmap/coord.sh <command> [options]

  register                 record this worktree as an active agent
  status                   show every agent, its claim, and slot occupancy
  claim <id> [--subsystem L] [--note T]
                           claim a roadmap item for this worktree
  release                  drop this worktree's claim
  unmerged                 refresh this worktree's unmerged-work summary
  reap                     drop agents whose process is gone
  heavy [--label T] [--exclusive] -- <command...>
                           run a build/test/commit under the semaphore
  hook-pretooluse          Claude Code PreToolUse hook (reads JSON on stdin)

Environment:
  ROADMAP_COORD_AGENT        override the agent id
  ROADMAP_COORD_SLOTS        override the slot count
  ROADMAP_COORD_NO_MEM_GATE  skip the free-memory floor
  ROADMAP_COORD_USE_SCOPE    0 to run without a systemd scope
  ROADMAP_COORD_BYPASS       1 to make the hook allow anything
USAGE
}

[ $# -ge 1 ] || {
  usage
  exit 2
}

cmd=$1
shift

case "${cmd}" in
register)
  with_lock reap_dead
  with_lock do_register
  do_unmerged
  ;;
status)
  with_lock reap_dead
  do_unmerged 2>/dev/null || true
  do_status
  ;;
claim)
  [ $# -ge 1 ] || die 'claim wants a roadmap item id'
  claim_task=$1
  shift
  claim_subsystem=''
  claim_note=''
  while [ $# -gt 0 ]; do
    case "$1" in
    --subsystem)
      [ $# -ge 2 ] || die '--subsystem wants a value'
      claim_subsystem=$2
      shift 2
      ;;
    --note)
      [ $# -ge 2 ] || die '--note wants a value'
      claim_note=$2
      shift 2
      ;;
    *) die "unknown option '$1'" ;;
    esac
  done

  if located=$(locate_item "${claim_task}"); then
    claim_status=$(printf '%s' "${located}" | cut -f1)
    claim_path=$(printf '%s' "${located}" | cut -f2)
    case "${claim_status}" in
    done | wont-do | deferred)
      die "'${claim_task}' is in ${claim_status}/ -- not a claimable item"
      ;;
    esac
  else
    claim_status=''
    claim_path=''
  fi

  with_lock reap_dead
  with_lock do_register
  do_unmerged
  with_lock do_claim "${claim_task}" "${claim_subsystem}" "${claim_note}"
  note "claimed ${claim_task}"
  if [ -n "${claim_path}" ] && [ "${claim_status}" != 'in-progress' ]; then
    target=$(printf '%s' "${claim_path}" | sed 's|/[^/]*/|/in-progress/|')
    note "move it yourself when you start:  git mv ${claim_path} ${target}"
  fi
  ;;
release)
  with_lock do_release
  do_unmerged
  note 'claim released'
  ;;
unmerged)
  with_lock do_register
  do_unmerged
  ;;
reap)
  with_lock reap_dead
  ;;
heavy)
  heavy_label=''
  while [ $# -gt 0 ]; do
    case "$1" in
    --label)
      [ $# -ge 2 ] || die '--label wants a value'
      heavy_label=$2
      shift 2
      ;;
    --exclusive)
      exclusive=1
      shift
      ;;
    --)
      shift
      break
      ;;
    *) die "unknown option '$1' (did you forget the -- before the command?)" ;;
    esac
  done
  [ $# -ge 1 ] || die 'heavy wants a command after --'
  # Default the label to the first two words of the command. Keeping --label
  # optional matters for permission allow-lists: those match on a command
  # prefix, and a mandatory free-text label between `heavy` and the command
  # would make every wrapped invocation a different, unmatchable prefix.
  if [ -z "${heavy_label}" ]; then
    heavy_label=$(printf '%s %s' "${1:-}" "${2:-}" | sed 's/ *$//')
  fi
  do_heavy "${heavy_label}" "$@"
  ;;
hook-pretooluse)
  do_hook
  ;;
-h | --help | help)
  usage
  ;;
*)
  usage
  exit 2
  ;;
esac
