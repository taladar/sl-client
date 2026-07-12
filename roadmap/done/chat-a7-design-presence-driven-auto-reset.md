---
id: chat-a7
title: Design presence-driven auto-reset
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A7. Design presence-driven auto-reset.** On `FriendsOffline`, for each
offlined friend: clear their typing in every session; mark/close the open
**1:1** session whose peer is that friend; and best-effort update **conference
/ group rosters** where they appear as a participant (drop or mark-left).
State the caveat explicitly: presence is friends-only, so this only covers
friend-participants who grant see-online — **non-friend** participants still
rely on the simulator's `SessionLeave` events. The two signals layer; they do
not replace each other. On `FriendsOnline`: update the presence set (and
optionally clear a stale "peer offline" marker on the 1:1 session); no other
auto-action. Define the exact session transitions.
**Done — see § Presence-driven reset reference (from A7) + task B6 in
§ Phase B.** Decided: on `FriendsOffline`, at A3's `OfflineNotification`
handler where A3 removes the friend from `online`, for each offlined agent
iterate every `ChatSession` and **remove that agent from `typing` and from
`participants`** (the 1:1's `participants` is unmaterialised, so only its
`typing` is touched). **No session is removed and no per-session "offline"
marker is stored** — refining "mark/close": a 1:1 is never removed
(A4), and its peer-offline state is already `!is_online(peer)` from the A3 set
(single source of truth — a stored marker would duplicate it). So
**`FriendsOnline` needs *no* chat action** (no marker to clear; the friend
re-joins via `SessionAdd`/messages) — A3's set-add is the whole effect.
**Caveat:** presence is friends-only / see-online-gated, so the roster
drop covers only friend-participants who grant see-online; **non-friend**
participants are dropped solely by the sim's `SessionLeave` (A6). The two
**layer** — A7 is the fast path for friends (also covers a crash with no
`SessionLeave`), `SessionLeave` covers everyone; both idempotent. Typing is
also cleared by the A6 9 s expiry — A7 just does it immediately. No new event
(the driver already gets `FriendsOffline`).

## Presence-driven reset reference (from A7)

How friend presence (A3) drives chat-session state. When a friend goes offline
the chat state tied to them is cleaned **immediately**, rather than waiting on
the simulator's session events. This is the one place the two subsystems —
presence (A3) and the session registry (A2/A6) — couple; everywhere else they
are independent (A3's invariant). The simulator stays authoritative; A7 is a
fast, best-effort mirror that *layers with*, never replaces, the sim's
`SessionLeave`.

**Trigger: `OfflineNotification` → `FriendsOffline`** (`methods.rs:3514`). A3
already removes each offlined `FriendKey` from `online` here; A7 adds, at the
**same** handler, for each offlined agent `a` (`FriendKey` → `AgentKey`, same
underlying `Key`):

- **Clear their typing everywhere** — for every `ChatSession` in the registry,
  `typing.remove(a)`. A friend who logged out cannot still be typing; do it now
  rather than wait the A6 9 s expiry (which remains the backstop for non-friends
  and crashes).
- **Drop them from group / conference rosters** — for every `ChatSession`,
  `participants.remove(a)`. Logout removes the agent from every IM session, so
  this is correct; the sim will *also* send `SessionLeave` (A6 removes them
  again — idempotent), but A7 is faster and still cleans up if a crash means no
  `SessionLeave` arrives. A `Direct` session has no materialised `participants`,
  so this is a no-op there.

That is the whole fan-out: one pass over `chat_sessions`, removing `a` from each
session's `typing` and `participants`. Cost is O(sessions) per offlined friend —
trivial.

**What A7 does *not* do (the refinements):**

- **No session is removed.** A 1:1 is never removed (A4) — its history must
  survive the peer going offline; group / conference sessions we are in are not
  removed either (only an explicit leave / decline removes — A4). A7 only edits
  the *contents* (`typing` / `participants`), never the registry membership.
- **No per-session "offline" marker.** The sketch said "mark/close the 1:1"; the
  decision is **neither**. A 1:1's peer-offline state is exactly
  `!is_online(peer)`, already kept by A3's `online` set — the single source
  of truth. Storing a marker on the `ChatSession` would duplicate it and risk
  drift. The driver reads presence via `is_online(peer)` (A3) for any session it
  displays.
- **No lifecycle change.** A 1:1 stays `Joined` when its peer goes offline — you
  can still send (it becomes a stored offline IM); "joined the conversation" is
  unrelated to "peer currently online".

**`FriendsOnline` → no chat action.** Because no marker is stored,
there is nothing to clear when a friend comes back: A3 adds them to `online`
(which flips `is_online`), and that is the entire effect. The friend re-appears
in a roster only when the sim re-adds them (`SessionAdd`) or speaks — A7 does
**not** speculatively re-populate rosters. This keeps presence the only driver
the online set and avoids inventing membership.

**The friends-only caveat & layering (explicit).** Presence is **friends-only,
`CAN_SEE_ONLINE`-gated** (A3). So A7's roster/typing cleanup fires **only** for
participants who are our friends *and* grant us see-online. Every other
participant — non-friends, or friends not granting see-online — is cleaned up
**only** by the sim `SessionLeave` (A6 roster fold) and the A6 typing expiry.
The two signals **layer**: A7 is the fast path where presence is visible to us;
`SessionLeave` / expiry is the universal path. Neither replaces the other, and
both are idempotent (removing an already-absent key is a no-op), so a friend who
triggers both just gets removed once.

**Persistence.** A7 is triggered by *presence*, not by region change, so it is
orthogonal to the A9 teleport-persistence rules: presence (and thus the chat
state) survives a teleport because no `FriendsOffline` is synthesised by moving.
A7 fires only on a genuine `OfflineNotification`.
