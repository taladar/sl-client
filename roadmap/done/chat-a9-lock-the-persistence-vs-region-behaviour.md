---
id: chat-a9
title: Lock the persistence-vs-region behaviour
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A9. Lock the persistence-vs-region behaviour.** Chat sessions,
history, and presence are **grid-level** and **persist** across teleport
(`begin_handover`, `TeleportLocal`), neighbour crossing
(`promote_child_to_root`), and `DisableSimulator` — explicitly **not** reset
(the inverse of the `SitState` reset at those same sites). It is not cleared
even on logout — it survives into the `Closed` state so the final pre-logout
conversation stays readable, vanishing only when the `Session` is dropped
(revised 2026-06-27, below). Persistence **beyond** a single session **is** in
scope — the optional local chat-log files (**A13**); the sans-IO `Session`
state itself stays in-memory and A13's *runtime* file layer is the long-term
store. A9 locks the in-memory region-behaviour; A13 owns the on-disk
behaviour.
**Done — see § Persistence & region reference (from A9) + task B10 in
§ Phase B.** Decided: the three chat/presence stores (`chat_sessions` /
`friends` / `online`) are **grid-level**, so — unlike `sit` / `script_grants`,
which are reset at the four region-boundary sites — they are touched at
**none** of `begin_handover` (`methods.rs:760`, which resets `sit` at `:800`
and drops in-world grants at `:803`), `promote_child_to_root` (`:897`),
`TeleportLocal` (`:2237`, `sit` reset at `:2243`), or the child-circuit
`DisableSimulator` (`:1206`). The **rule is "add no clear at those sites"** —
there is no positive code to write, only the guard that B2/B1's stores are
never wired into those handlers. **Logout never clears them either — they
survive into the `Closed` state for post-logout inspection** (revised
2026-06-27 on user request: a user may still want to read the messages from
immediately before logout). `close` (`:9599`) / `LogoutReply` (`:3548`) / the
logout-timeout (`:3597`) only set `SessionState::Closed` (terminal —
`is_closed`, `:9594`) and emit the disconnect event; **no field is cleared in
place**, so the read accessors (`history` / `chat_sessions` / `friends` /
`is_online`) stay valid on a closed `Session` and the final
conversation/roster/presence remain readable until the driver **drops** the
struct. The stores vanish only by that **discard**: a relogin builds a
**fresh** `Session::new(login)` (`:151`, a `const fn`) whose stores start
empty — so **no `close` hook, no reset code** (the A2/A3 convention, now
doubly justified: clearing on close would *destroy* the post-logout history
the user wants). The chat fields slot into the `const fn` constructor beside
`sit: SitState::NotSitting` (`:165`) / `script_grants: BTreeMap::new()`
(`:167`) as `chat_sessions` / `friends: BTreeMap::new()` + `online:
BTreeSet::new()` — all const-constructible, no `const fn` regression. B10 is a
**verification + guard** task: the cross-region persistence tests are the
**inverse** of `teleport_clears_seat` (`tests/lifecycle.rs:1716`) — after a
teleport / crossing / `DisableSimulator`, a seeded chat session / history /
roster / presence entry must **still** be present. Cross-session (relogin)
persistence is **out** of the in-memory scope — that is A13's optional on-disk
file layer.

## Persistence & region reference (from A9)

Where the chat/presence stores sit relative to the *region* lifecycle. The whole
system (`chat_sessions` A2, `friends` / `online` A3) is **grid-level** — routed
by the grid's IM / group / presence services, not the region simulator — so it
behaves as the **inverse** of the region-local `SitState` and the
per-in-world-object script-permission grants: those reset at every region
boundary, the chat/presence stores never do. A9 produces **no new state and no
new code path** — it *locks* a behaviour by fixing where the B2/B1 stores are
(and are not) wired, and pins the verification.

**The four region-boundary reset sites (where chat/presence must NOT appear).**
Each already resets the region-local state; the chat stores are absent from all
four and must stay absent:

| Site | What it resets today | Chat/presence |
|------|----------------------|---------------|
| `begin_handover` (`methods.rs:760`, retarget teleport) | `children` / `child_seeds` / `objects` / `terrain` / `regions` / `time_dilation` cleared; `sit = NotSitting` (`:800`); `drop_inworld_grants()` (`:803`) | **untouched** |
| `promote_child_to_root` (`:897`, neighbour crossing) | rebuilds the root circuit; **keeps** the seat (a vehicle carries the agent across — `:796`) | **untouched** |
| `TeleportLocal` (`:2237`, intra-region) | `sit = NotSitting` (`:2243`); `drop_inworld_grants()` (`:2244`) | **untouched** |
| `DisableSimulator` (`:1206`, child-circuit retire) | drops the child circuit / seed; `forget_sim_objects` | **untouched** |

The rule is therefore **"add no clear at those sites"**: when B2/B1 land the
stores, none of these four handlers gains a `chat_sessions` / `friends` /
`online` clear. There is no positive code — A9 is the guard that the grid-level
stores never get accidentally wired into the region-reset path (the easy mistake
is to "mirror" the `objects.clear()` line). The contrast is exact: `sit` and the
script grants are *region* facts (a seat is region-local; a grant is per
in-world object left behind), so they reset; a chat session / buddy presence is
a *grid* fact that the same teleport leaves wholly intact.

**Logout keeps them in memory — discard, never in-place clear** (revised
2026-06-27 on user request). Logout is terminal, not a reset: `close` (`:9599`),
`LogoutReply` (`:3548`), and the logout-timeout (`:3597`) each only set
`state = SessionState::Closed` (terminal — `is_closed`, `:9594`) and emit the
disconnect/`LoggedOut` event; **no field is cleared**. This is deliberate and
now load-bearing for the chat stores: a user logging out may still want to
**inspect the messages from immediately before logout**, so the chat sessions,
their history, the rosters, and the friend/presence stores **remain readable on
the `Closed` session** — the read accessors (`history` / `chat_sessions` /
`friends` / `is_online`) must **not** gate on `state` (they are pure getters, so
they already don't; B10 asserts it). The stores die only when the driver
**drops** the `Session`; a relogin constructs a **fresh** `Session::new(login)`
(`:151`) that starts empty. This is the A2/A3 "constructor rebuild, no `close`
hook" convention — **no logout-time clearing code, no reset hook** — and adding
one would now be a *regression*, destroying the post-logout history the user
wants (it mirrors how `sit` / `objects` / `script_grants` are *not* cleared on
close either — they too just vanish with the discarded struct).

**The constructor slot.** `Session::new` is a **`const fn`** (`:151`). The chat
fields go in beside `sit: SitState::NotSitting` (`:165`) and
`script_grants: BTreeMap::new()` (`:167`) as `chat_sessions: BTreeMap::new()` /
`friends: BTreeMap::new()` / `online: BTreeSet::new()` — all
const-constructible, so the constructor stays `const`. (B2/B1 add the fields;
A9 only fixes that they seed empty here and nowhere else.)

**Verification — the inverse of `teleport_clears_seat`.** The existing
`tests/lifecycle.rs:1716` `teleport_clears_seat` asserts the seat is **gone**
after a teleport; the A9 persistence test is its mirror image — seed a chat
session (+ history / roster) and a presence entry, drive a teleport / neighbour
crossing / `DisableSimulator`, and assert every chat/presence entry is **still
present** and unchanged. This is the single most load-bearing A9 check and is
listed in A11's strategy.

**Boundary with A13.** A9 governs the **in-memory** region behaviour only:
within one logged-in session the stores survive every region change and die with
the session. Persistence **across** logins (long-term scrollback) is **out** of
this in-memory model — it is A13's optional, default-off, runtime **on-disk**
chat-log file layer (the sans-IO `Session` does no I/O). A9 = in-memory /
region; A13 = on-disk / cross-session.
