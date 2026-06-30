# Conformance Test Roadmap

A staged plan for growing the `sl-conformance` live-grid suite from its current
four behaviours (`login-handshake`, `inventory-fetch`, `asset-decode`,
`region-info`) to comprehensive coverage of the protocol surface and the
higher-level flows built on top of it (chat sessions, inventory, teleport,
groups, ...). Every case runs against live grids — local **OpenSim** and the
Second Life **Aditi** beta grid — not as `cargo test` units.

This file is a plan, not test code. Future sessions implement one phase (or one
case) at a time, run it live, commit the generated record, and tick the box
here. For *how* the harness works and how runs are recorded, see the book:
`book/src/conformance/{overview,runner,records}.md`.

## How to add a case (recap)

Three mechanical steps, modelled on the existing cases under
`sl-conformance/src/cases/`:

1. Add `sl-conformance/src/cases/<name>.rs` with a unit struct that implements
   `GridTest` (`sl-conformance/src/registry.rs`): `name()` (kebab-case, also the
   record file stem), `description()`, `grids()`, optional `accounts()`, and the
   async `run()` body.
2. Add `pub mod <name>;` to `sl-conformance/src/cases.rs`.
3. Add `Box::new(crate::cases::<name>::<Struct>)` to `registry()` in
   `sl-conformance/src/registry.rs`.

Inside `run()`, drive the live session(s):

- `ctx.primary()` / `ctx.secondary()` (and, once added, `ctx.tertiary()`) yield
  `Session` handles.
- `session.wait_for_region(timeout).await?` gates on the region handshake.
- `session.send(Command::...).await?` issues a command.
- `session.wait_for(timeout, |event| match event { ... })` awaits a typed
  `Event`. The `Command`/`Event` surface lives in `sl-client-tokio/src/lib.rs`;
  the state machines (teleport phases, sit, chat sessions, inventory) live in
  `sl-proto/src/session.rs`.
- `ctx.metrics().set("k", v)` / `.set_timing("k_secs", secs)` record values.
- Fail an assertion with `Err(TestFailure::Assertion("...".to_owned()))`.
- `ctx.mark_partial("reason")` flags a legitimately incomplete dataset instead
  of failing (e.g. a grid that omits a field).

Run and record:

```sh
sl-conformance run --grid opensim <name>
sl-conformance run --grid aditi  <name> --force   # --force skips cooldown
sl-conformance-report                              # green = Current
```

## Legend & conventions

- Grid gating: `[both]`, `[opensim]` (OpenSim only), `[aditi]` (SL only).
- Account count: `1av`, `2av`, `3av` (see Phase 0 and Phase Z).
- Status: `[ ]` todo, `[x]` done (tick when the live record is committed green).
- Prefer asserting an observable protocol effect (a field value, a state
  transition) over only timing it. Keep a timing metric anyway — the reporter
  tracks regressions.
- Record meaningful metrics: counts, timings (`*_secs`), codec/format names.
- Use `mark_partial` (not failure) when a grid legitimately returns less data.
- Keep timeouts generous for Aditi (network + MFA + load).
- Respect the Aditi 120 s per-avatar cooldown; serialise multi-avatar Aditi
  logins and expect long wall-clock.

## Grid capability differences (for gating)

- **SL only** (`[aditi]`): Experiences, Display Names, Voice provisioning,
  god-bit enforcement, modern CAPS-only flows where OpenSim has no equivalent.
- **OpenSim only** (`[opensim]`): OpenRegionInfo limits bag, Hypergrid
  teleport, per-estate physics/scripting restriction.
- **Auto-selected** (write once, runs on both): inventory fetch picks CAPS
  `FetchInventoryDescendents2` vs UDP `FetchInventoryDescendents` per region.
- Several OpenSim features are **OFF by default** and need a config/module step
  before they can be tested — see the Setup-cost appendix.

---

## Phase 0 — Test utilities & helpers (do first)

Pure code, no new avatars. Build the shared scaffolding so later cases stay
short and consistent.

- [x] `cases/common.rs` (or a `support` module) with: standard timeout
  constants, a `send-then-await-matching-event` combinator, a grid-gating
  helper, and metric-name helpers. (`sl-conformance/src/support.rs`:
  `REGION_TIMEOUT`/`REPLY_TIMEOUT`/`LONG_TIMEOUT`, `send_then_wait`,
  `is_opensim`/`is_aditi`, `secs_metric`/`count_metric`.)
- [x] An assertion helper wrapping `TestFailure::Assertion` with a clear,
  formatted message so cases can assert field values, not just measure.
  (`support::check` / `support::check_eq`.)
- [x] A fixtures helper for well-known ids (the default plywood texture UUID
  already used by `asset-decode`; the default region UUID; the estate-owner
  avatar label). (`support::fixtures`: `PLYWOOD_TEXTURE` /
  `plywood_texture()`, `OPENSIM_DEFAULT_REGION`, `ESTATE_OWNER_LABEL`;
  `asset-decode` now uses the fixture.)
- [x] **Tertiary-avatar harness support** (prerequisite for any `3av` case):
  a `--tertiary` resolver mirroring `resolve_secondary`, a `ctx.tertiary()`
  accessor, a third Aditi cooldown guard, and bumping `accounts()` handling to
  accept `3`. Two-avatar plumbing already exists in
  `sl-conformance/src/context.rs` (`accounts()` + `--secondary`); this extends
  it. OpenSim `3av` cases can run as soon as this lands; Aditi `3av` waits on a
  3rd Aditi avatar (Phase Z). (Resolver picks an avatar distinct from both
  primary and secondary; conventional credentials label `tertiary`.)

---

## Phase 1 — Session lifecycle & circuit `[both] 1av`

- [x] `logout-clean` — request logout, assert clean `LogoutReply` / shutdown.
  SL replies cleanly (`complete`); OpenSim never transmits the reply (queued
  then dropped by an unimplemented `LLUDPServer.Flush` + outbox-clearing
  `Shutdown`), so it logs out via the 5 s timeout fallback and is recorded
  `partial`. Our client is conformant on both.
- [x] `keepalive-ping` — observe start/complete ping round-trip over the
  circuit; record RTT. The session now sends a periodic `StartPingCheck` on
  every circuit — root and child (the reference viewer's ~5 s circuit ping) —
  and surfaces each `CompletePingCheck` as `Event::Ping { sim, child, rtt }`.
  The case asserts the root ping (`child: false`, the "ping to sim"); recorded
  RTT ≈ 1.2 ms on loopback OpenSim, ≈ 170 ms on Aditi.
- [x] `throttle-set` — apply a `Throttle` preset and confirm it is accepted.
  `AgentThrottle` is fire-and-forget (no protocol reply), so acceptance is the
  *absence* of failure: the reliable packet is acked by the sim rather than
  retransmitted to exhaustion (which would close the circuit). The case applies
  the 500 kbps preset and watches the circuit past the retransmit budget (~9 s)
  via keep-alive pings; a healthy ping past that point plus no `AgentThrottle`
  reply-missing diagnostic confirms acceptance. Green on both grids.
- [x] `draw-distance` — set draw distance; confirm no error and any echoed
  state. The draw distance rides the `Far` field of the unreliable keep-alive
  `AgentUpdate` (no reply), so the simulator folds it into the agent's interest
  list and enables the neighbouring regions it reaches, each surfaced as
  `Event::NeighborDiscovered`. The case applies a 512 m draw distance (double
  the 256 m default), then observes the circuit for a window: a keep-alive ping
  that still round-trips is the "no error" signal, and the neighbour
  announcements are the echoed state. OpenSim is a 2×2 block of adjacent
  regions, so 512 m always reaches its neighbours — green with
  `neighbors_count = 3`. Aditi's landing region had no neighbours within reach,
  recorded `partial` (`neighbors_count = 0`) with the circuit healthy.

## Phase 2 — Local chat `[both]`

- [x] `chat-self-echo` — `say` on a channel and observe own
  `ChatFromSimulator`. `1av`, runs on Aditi today. A normal `say` on the
  public channel (`0`) is broadcast back to the speaker, so the case sends a
  marker message tagged with the avatar's own agent id, then awaits the
  matching `Event::ChatReceived` attributed to its own agent — asserting the
  echoed text, source, and `Normal` chat type. Green on both grids; echo RTT
  ≈ 18 ms on loopback OpenSim, ≈ 177 ms on Aditi.
- [x] `chat-hear-other` — second avatar says, primary hears. `2av`
  (OpenSim now; Aditi deferred → Phase Z). The first multi-avatar case: the
  secondary (`Friend Tester`) `say`s a marker tagged with its own agent id on
  the public channel, and the primary (`Avatar Tester`) — a separate session
  sharing the region — receives the matching `Event::ChatReceived` attributed to
  the secondary's agent, `ChatAudible::Fully`, `Normal` volume. Proves the
  simulator *relays* local chat between distinct agents (vs `chat-self-echo`'s
  self-echo). Green on OpenSim; relay RTT ≈ 1 ms on loopback.
- [x] `chat-whisper-shout-range` — verify whisper/shout reach vs normal. `2av`
  (OpenSim now; Aditi deferred → Phase Z). OpenSim drops an out-of-range message
  outright (it never marks it less audible), so reach is simply whether the
  relayed `ChatFromSimulator` arrives. The case anchors the primary and
  teleports the secondary (an intra-region `Command::Teleport`, so the gap is
  exact regardless of where each logged in) to two separations: at **15 m**
  (between whisper's 10 m and say's 20 m) a normal say is heard but a whisper is
  not, and at **60 m** (between say's 20 m and shout's 100 m) a shout is heard
  but a say is not — establishing whisper < say < shout. At each gap the
  secondary says the out-of-range message immediately followed by a louder
  in-range sentinel; hearing the sentinel but never the out-of-range marker
  (with a short grace against reordering) confirms the drop. Both avatars are
  placed at a high Z so the teleport is not clamped to terrain and the
  separation is purely horizontal. Green on OpenSim; say RTT ≈ 1–3 ms, shout
  RTT ≈ 1 ms on loopback.
- [x] **Runtime refactor (do before the remaining cases, not a test case):**
  surface session/region state in the **bevy** runtime the idiomatic ECS way so
  later cases (and features) can query it at any tick instead of catching a
  one-shot event or threading flat accessors. Move the globally-unique login
  facts now carried by the fire-once `SlIdentity` event (agent id, session id,
  circuit code, seed capability — plus the region handle the
  `chat-whisper-shout-range` work added to the tokio `Client`) into a Bevy
  **`Resource`**, and put per-region state (region handle, sim address, region
  info, neighbours, parcels, …) on **`Component`s of region entities**. The
  motivation: `chat-whisper-shout-range` had to bolt one more flat accessor
  (`region_handle`) onto `Session`/`Client`; doing the structured split early
  means future tests extend a model that already knows where new global vs
  per-region facts belong, rather than accreting ad-hoc `SlIdentity` fields and
  `Client` accessors. Keep tokio-side parity in mind (the
  `agent_id`/`session_id`/`circuit_code`/`seed_capability`/`region_handle`
  accessors are the flat precursor this supersedes on the bevy side). Done in
  `sl-client-bevy/src/world.rs`: `SlIdentity` is now a `Resource` (agent/
  session/circuit/seed + current `region_handle`, read with `Res<SlIdentity>`
  and `is_changed`-gated), no longer an event. Per-region state lives on region
  entities — `SlRegion { handle, sim }` for the login region and every
  `EnableSimulator` neighbour, marked `SlCurrentRegion` / `SlNeighbor`, with
  `SlRegionIdentity` / `SlRegionLimits` components and child `SlParcel`
  entities. A `maintain_world` system (chained after `drive`) folds the
  `SlEvent` stream into this model: spawning the current region on
  `CircuitEstablished`, neighbours on `NeighborDiscovered`, moving the current
  marker and updating the global handle on `RegionChanged`, and clearing it on
  logout/disconnect. `sl-repl-bevy` now reads the resource; covered by unit
  tests in `world.rs`. Tokio keeps its flat accessors for parity.
- [x] `typing-indicator` — `set_typing` start/stop observed by the other. `2av`
  (OpenSim now; Aditi deferred → Phase Z). The local-chat typing indicator is a
  `ChatFromViewer` with no text and a `StartTyping`/`StopTyping` chat type (the
  animation trigger a viewer fires while editing the chat bar); the simulator
  broadcasts it to nearby avatars, surfaced as `Event::ChatTyping`. The
  secondary (`Friend Tester`) sends `Command::Typing(true)` then
  `Command::Typing(false)`, and the primary — a separate session sharing the
  region — observes `typing: true` then `typing: false`, both attributed to the
  secondary's agent id. Where `chat-hear-other` proves the simulator relays a
  spoken *message*, this proves it relays the typing *signal*. Unlike a spoken
  `say` (gated by say/whisper/shout distance), OpenSim delivers typing with no
  distance check, so the relay does not depend on how close the avatars logged
  in. Green on OpenSim; start RTT ≈ 7 ms, stop RTT ≈ 1 ms on loopback.

## Phase 3 — Instant messaging & chat sessions `[both]`

- [x] `im-1to1` — send IM, peer receives; reply back. `2av`
  (OpenSim now; Aditi deferred → Phase Z). A direct IM is an
  `ImprovedInstantMessage` with the `IM_NOTHING_SPECIAL` dialog
  (`ImDialog::Message`), routed by the grid's IM service rather than broadcast
  to the region like local chat. The primary `Command::InstantMessage`s the
  secondary (`Friend Tester`), which — a separate session — observes the
  matching `Event::InstantMessageReceived` attributed to the primary, then
  replies with its own IM, and the primary observes the matching reply. Each
  direction tags its text with the sender's agent id so the predicate ignores
  unrelated background IM; the case asserts `ImDialog::Message` plus
  `from_agent_id`/`to_agent_id` in both directions, proving the service
  delivers a *targeted* message (vs `chat-hear-other`'s proximity broadcast)
  and that the reply travels back the same way. Green on OpenSim; deliver RTT
  ≈ 14 ms, reply RTT ≈ 0.4 ms on loopback.
- [x] `im-typing` — IM typing start/stop. `2av`
  (OpenSim now; Aditi deferred → Phase Z). IM typing is an
  `ImprovedInstantMessage` with an `IM_TYPING_START`/`IM_TYPING_STOP` dialog and
  the literal text `"typing"`, routed by the grid's IM service to one named
  recipient and carrying the canonical 1:1 session id (`agent_id XOR
  to_agent_id`) — the IM-session analogue of `typing-indicator`'s local-chat
  broadcast. The primary `Command::ImTyping`s `typing: true` then
  `typing: false` to the secondary (`Friend Tester`), which — a separate
  session — observes the matching `Event::ImTyping`s attributed to the primary
  on that session. The predicate matches the primary's `from_agent_id`, and the
  case asserts the observed `session_id` equals the canonical id of the
  secondary's `Direct` session with the primary, plus the `typing` flag in each
  direction — proving the signal arrived on the targeted 1:1 session, not as a
  stray broadcast. Where `im-1to1` proves the IM service relays a targeted
  *message*, this proves it relays the typing *signal* over the same session.
  Green on OpenSim; start RTT ≈ 0.7 ms, stop RTT ≈ 0.4 ms on loopback.
- [x] `group-session-message` — open a group session, send, leave. `2av`
  (needs Groups; OpenSim requires Groups V2). The first Phase 3 case to need a
  setup beyond the avatars: it creates a throwaway open-enrollment group (the
  primary becomes founder) and has the secondary `JoinGroup` it, so it depends
  only on Groups V2 being enabled, not on any pre-existing group. Both avatars
  then `StartGroupSession`, the primary `SendGroupMessage`s a marker tagged with
  its own agent id, and the secondary — a fellow member — observes it. OpenSim's
  `GroupsMessagingModule` delivers to a member who is already a session
  participant as a UDP `IM_SESSION_SEND` (`Event::GroupSessionMessage`, the
  canonical path the secondary's pre-join takes), but to a not-yet-joined member
  as a CAPS `ChatterBoxInvitation` carrying the first message inline
  (`Event::ConferenceInvited` with `from_group`); the predicate accepts either,
  so a lost join/send race proves delivery rather than flaking, recording the
  path taken. `LeaveGroupSession` has no observable OpenSim effect (the module
  ignores the `SessionDrop` dialog over UDP), so the case confirms only that the
  circuit survives the leave (a keep-alive ping still round-trips) — the
  "acceptance = absence of failure" shape `throttle-set` uses. Green on OpenSim;
  deliver RTT ≈ 9 ms loopback, via the `group-session-message` path. Required a
  harness fix: each session's events are now forwarded off the run loop's
  bounded channel into an unbounded one, so events that go unread (the
  non-awaited avatar) never stalls its run loop — without it the primary's
  just-queued `SendGroupMessage` sat untransmitted for ~30 s. `[opensim]` only;
  Aditi deferred → Phase Z.
- [x] `chat-invite-accept-decline` — `AcceptChatInvite` /
  `DeclineChatInvite` and the CAPS `ChatSessionRequest` path on SL. `2av`
  (OpenSim now; Aditi deferred → Phase Z). A pending invite is a chat-session
  registry entry whose lifecycle is `Invited`; accepting promotes it to `Joined`
  and declining removes it. To provoke a *real* invitation the primary creates a
  throwaway open-enrollment group, the secondary joins it as a member, and the
  primary opens the group session and sends one message — which the secondary,
  not yet a session participant, receives as a CAPS `ChatterBoxInvitation`
  (`Event::ConferenceInvited` with `from_group`, the same not-yet-joined path
  `group-session-message` documents). The case does this twice (one group to
  accept, one to decline, since a second message in the same group arrives as a
  plain session message), then drives accept / decline on the secondary and
  asserts the registry via `QueryChatSessions`: `Invited` (inviter = primary,
  text channel) before answering, `Joined` after `AcceptChatInvite`, and gone
  after `DeclineChatInvite`. OpenSim exposes no `ChatSessionRequest` capability,
  so there the accept is the optimistic local join and the decline a UDP
  `SessionLeave` the module ignores — both observable only as the client-side
  registry transition; asserting the cap POST and its reply roster is the Aditi
  Phase Z variant. Surfaced and fixed an over-promotion bug: OpenSim sends a
  `ChatterBoxSessionAgentListUpdates` (an informational voice roster push)
  alongside the invitation, and the handler used the *promoting*
  `chat_session_mut`, so the `Invited` window collapsed to `Joined` before any
  accept; it now folds the roster via the non-promoting `chat_session_get_mut`,
  keeping the lifecycle until an explicit accept or real session traffic. Green
  on OpenSim; invite RTT ≈ 70–100 ms loopback. `[opensim]` only.
- [x] `session-mark-read` — unread → mark-read transition. `2av`
  (OpenSim now; Aditi deferred → Phase Z). Each chat session carries an unread
  counter, bumped on every inbound message that is not our own echo and cleared
  either by our own outbound send or explicitly by `Command::MarkSessionRead`
  (the viewer "marking a conversation read" without replying). The secondary
  sends two 1:1 IMs to the primary — each tagged with the secondary's agent id
  so the predicate ignores unrelated background IM — and the primary, a separate
  session, observes both `Event::InstantMessageReceived` and so accumulates
  `unread == 2` on its `Direct { peer: secondary }` session. The primary then
  `MarkSessionRead`s that session and the transition is asserted against its
  registry via `QueryChatSessions`: before marking, the session is a `Joined`
  1:1 with `unread == 2` (two messages, proving the counter *counts* rather than
  flips a has-unread flag); after marking it is still present and still `Joined`
  (mark-read clears the badge, it does not close the conversation) but reads
  `unread == 0`. `MarkSessionRead` is a purely local registry operation (no wire
  send), identical on both grids; only the seeding IMs touch the wire (plain
  LLUDP `ImprovedInstantMessage`). Green on OpenSim; deliver RTT ≈ 5 ms on
  loopback. `[opensim]` only.
- [x] `offline-msg-fetch` — fetch offline IMs (CAPS `ReadOfflineMsgs` vs UDP
  `RetrieveInstantMessages`). `2av` (OpenSim now; Aditi deferred → Phase Z).
  The store-and-forward counterpart of `im-1to1`: an IM sent while the recipient
  is *offline* is stored by the grid and replayed as an offline message when the
  recipient returns and fetches it. The flow needs the recipient absent at send
  time, so the case drives a mid-run logout/login on the primary via new harness
  support (`Session::disconnect` tears the run loop down but keeps the identity;
  `Session::relogin` logs the same avatar back in, inheriting the OpenSim
  "already logged in" retry that evicts the stale presence the disconnect
  leaves, and *waiting out* the aditi login cooldown rather than bypassing it so
  the same flow is safe on aditi). Sequence: the primary (recipient)
  disconnects; the secondary (sender) IMs the now-offline primary; the grid
  cannot deliver it, so it stores the message and replies to the sender with a
  "… Message saved" system IM (from the recipient's id) — the synchronisation
  point proving the message reached offline storage; the primary relogs in and
  issues `Command::RetrieveInstantMessages` (the client never auto-requests
  offline IMs), observing the marker replayed as an
  `Event::InstantMessageReceived` with `offline == true`. The replayed IM is
  matched on the sender's id and exact text, and `offline` distinguishes a
  replayed stored message from a live one. Requires the "Offline Message Module
  V2" on the test grid (SQLite has no offline-IM data provider, so its storage
  points at the throwaway `os_groups` MariaDB on `:3307`); OpenSim has no
  `ReadOfflineMsgs` capability, so this is the UDP path. `Session::relogin` is
  now cooldown-aware, so the remaining Aditi work is only branching the fetch to
  the CAPS `ReadOfflineMsgs` path and a second Aditi avatar (Phase Z). Green on
  OpenSim; store-confirm ≈ 96 ms, fetch RTT ≈ 35 ms loopback. `[opensim]` only.
- [ ] `conference-roster` — start an ad-hoc conference; verify it is distinct
  from a 1:1 (multi-party roster, `SessionAdd`/`SessionLeave`). **`3av`,
  `[aditi]` only → fully deferred to Phase Z.** Investigated 2026-06-30:
  OpenSim has **no ad-hoc (non-group) conference support whatsoever**, so the
  whole behaviour this case verifies is unobservable there. An ad-hoc
  conference is `IM_SESSION_CONFERENCE_START`
  (`ImDialog::SessionConferenceStart` = 16) with conference
  `SessionSend`/`SessionAdd`/`SessionLeave` carrying `from_group = false`.
  `InstantMessageModule.OnInstantMessage` relays only
  `MessageFromAgent`/`StartTyping`/`StopTyping`/`BusyAutoResponse`/
  `MessageFromObject` and drops everything else (`default: return;`);
  `GroupsMessagingModule` acts only on `fromGroup == true`; no module emits a
  `ChatterBoxInvitation` / roster `SessionAdd`/`SessionLeave` for a non-group
  session, and there is no conference module to enable. So on OpenSim the
  conference start reaches no invitee and yields no server roster — only the
  client-side `Conference`-kind registry entry, which is a session-model unit
  test, not a live-grid check. The genuine multi-party-roster behaviour is
  Second Life only and needs both a 2nd and a 3rd Aditi avatar — see Phase Z.

## Phase 4 — Friends & presence `[both] 2av`

- [x] `friendship-offer-accept` — offer, accept, confirm both friend lists.
  `2av` (OpenSim now; Aditi deferred → Phase Z). A friendship offer is an
  `ImprovedInstantMessage` with the `IM_FRIENDSHIP_OFFERED` dialog
  (`ImDialog::FriendshipOffered`), routed by the grid's friends service to the
  named recipient (not broadcast like local chat). The primary
  `OfferFriendship`s the secondary, which — a separate session — observes the
  matching `Event::InstantMessageReceived` (`FriendshipOffered`, attributed to
  the primary) and answers with `AcceptFriendship` quoting the offer's
  transaction id (the IM's `id`, which OpenSim sets to the offerer's agent id).
  The grid stores the symmetric friendship and notifies the offerer with a
  `FriendshipAccepted` IM, which the primary observes (and which adds the
  secondary to its buddy cache); the accepter adds the offerer on its own
  accept. The case then confirms both buddy lists via `QueryFriends` /
  `Event::FriendsSnapshot`. OpenSim rejects an offer to an *existing* friend
  outright ("This person is already your friend", forwarding nothing), so the
  case pre-cleans any leftover friendship with a best-effort
  `TerminateFriendship` (a no-op when not friends) plus a short settle before
  offering, and terminates again at the end so re-runs start clean — verified
  idempotent across back-to-back runs. OpenSim ignores the calling-card folder
  in `AcceptFriendship`, so the case passes the nil folder. Green on OpenSim;
  offer RTT ≈ 5–13 ms, accept RTT ≈ 10–27 ms loopback. `[opensim]` only.
- [x] `friendship-terminate` — terminate, confirm removal.
  `2av` (OpenSim now; Aditi deferred → Phase Z). The case first forms a clean
  friendship (the `friendship-offer-accept` flow: pre-clean, offer, accept,
  confirm both buddy lists) so there is a real friendship to tear down, then the
  primary `TerminateFriendship`s the secondary. `TerminateFriendship` names the
  former friend (`ExBlock.OtherID`); OpenSim's `RemoveFriendship` deletes the
  symmetric record and sends a `TerminateFriendship` back to *both* parties —
  `client.SendTerminateFriend` echoing the removal to the terminator, plus
  `LocalFriendshipTerminated` → `friendClient.SendTerminateFriend` informing the
  dropped friend. Each side's `Session` surfaces this as
  `Event::FriendshipTerminated` and drops the peer from its buddy cache (the
  terminator does *not* remove locally on send — it relies on the grid echo).
  The case asserts the primary observes its own `FriendshipTerminated` (naming
  the secondary), the secondary observes the matching one (naming the primary),
  and a follow-up `QueryFriends` on each side reports the other gone. Green on
  OpenSim; echo RTT ≈ 13 ms, notify RTT ≈ 13 ms loopback. `[opensim]` only.
- [x] `presence-online-offline` — observe `OnlineNotification` /
  `OfflineNotification` as the peer logs in/out. `2av` (OpenSim now; Aditi
  deferred → Phase Z). Presence flows over `OnlineNotification` /
  `OfflineNotification`, which the grid's friends service sends only to friends
  granted the see-online right; OpenSim grants `CanSeeOnline` in *both*
  directions on a fresh friendship (`FriendsModule.AddFriendship`), so a clean
  friendship is the only rights setup needed. Both avatars are already logged in
  when a case starts, so the case drives the transitions via the mid-run
  logout/login support `offline-msg-fetch` introduced: it first establishes a
  clean friendship (pre-clean, offer, accept, confirm the grid's
  `FriendshipAccepted`), then the secondary `disconnect`s and the primary — a
  see-online friend — observes `Event::FriendsOffline` naming it
  (`StatusChange(_, false)` from OpenSim's `OnClientClosed`), then the secondary
  `relogin`s (inheriting the "already logged in" retry that evicts the stale
  presence) and the primary observes `Event::FriendsOnline` naming it
  (`StatusChange(_, true)`, fired once the returning agent is a root agent).
  Each observation matches the secondary's id inside the notification's id list
  so an unrelated friend's presence change cannot satisfy it. Where
  `friendship-offer-accept` proves the friendship forms, this proves the
  presence channel it opens carries both edges of the transition. The offline
  notification is emitted as the grid tears the circuit down (inside the
  disconnect's logout sequence), so it is already buffered by the time the
  primary looks — observed near-instantly (≈ 0.04 ms); the online notification
  follows the relogin at ≈ 84 ms loopback. `[opensim]` only.
- [x] `grant-user-rights` — grant see-online / map / modify rights; confirm.
  `2av` (OpenSim now; Aditi deferred → Phase Z). A friendship is born with only
  `CAN_SEE_ONLINE` granted both ways; a client raises a friend's rights with
  `GrantUserRights`. OpenSim's `FriendsModule.GrantRights` persists the new
  bitfield then **always echoes it to the grantor**
  (`SendChangeUserRights(requester, friend, rights)`) and notifies the friend
  (`LocalGrantRights` → `SendChangeUserRights(requester, friend, rights)`); the
  two `ChangeUserRights` packets carry the same `AgentData.AgentID` (the
  grantor), so the session tells them apart by direction — the grantor sees its
  own id (`granted_to_us = false`, updating `rights_granted`), the friend sees a
  foreign id (`granted_to_us = true`, updating `rights_received`). The case
  forms a clean friendship (the offer/accept flow), then the primary grants the
  secondary the full `CAN_SEE_ONLINE | CAN_SEE_ON_MAP | CAN_MODIFY_OBJECTS` set,
  both sessions observe the matching `Event::FriendRightsChanged`, and a
  `QueryFriends` on each side confirms the cached friendship now reflects it
  (primary's `rights_granted` to the secondary is the full set; secondary's
  `rights_received` from the primary is the full set; the reverse direction
  stays at the default). Surfaced a grid-side timing dependency: `GrantRights`
  only acts on a friend present in the *grantor's* server-side friends cache,
  which `RecacheFriends` refreshes asynchronously and races the
  `FriendshipAccepted` IM — granting the instant the IM lands finds the cache
  still empty and echoes nothing, so the case settles ~3 s after the accept
  before granting (`GRANT_SETTLE`). Keeping the see-online bit set means the
  grant toggles no presence, so it provokes no spurious online/offline
  notification. Green on OpenSim; echo RTT ≈ 4.6 ms, notify RTT ≈ 4.7 ms
  loopback. `[opensim]` only.
- [x] `calling-card` — offer/accept calling card. `2av` (OpenSim now, partial;
  Aditi deferred → Phase Z). A calling card is a reference card to an avatar,
  filed in the recipient's Calling Cards folder; offering one is *not* a
  friendship request. The primary `OfferCallingCard`s the secondary with a fresh
  correlation id; the secondary observes the matching
  `Event::CallingCardOffered` attributed to the primary and
  `AcceptCallingCard`s, quoting the offer's transaction id. Contrary to this
  roadmap's earlier guess, OpenSim **does** surface the offer when both avatars
  share a region: `XCallingCardModule.OnOfferCallingCard` finds the recipient
  in-region, creates the calling-card inventory item, and pushes it with
  `SendOfferCallingCard(from, itemID)` — so the secondary's `CallingCardOffered`
  carries the *new card's item id* as its transaction (the in-region path
  discards the offerer's chosen transaction entirely), and the case asserts the
  offer is attributed to the primary rather than that the transaction
  round-trips. The run is partial because OpenSim's `OnAcceptCallingCard` is an
  empty no-op (the card was already filed at offer time), so it sends the
  offerer **nothing** back — the offerer-side `Event::CallingCardAccepted`
  confirmation has no OpenSim path to observe. The full
  offer→accept→offerer-confirm round-trip is Second Life only → Phase Z (aditi).
  Green-partial on OpenSim; offer RTT ≈ 62 ms loopback. `[opensim]` only.
- All Aditi variants deferred to Phase Z.

## Phase 5 — Inventory (deep) `[both]`

- [x] `inventory-tree-crawl` — background/full-tree fetch beyond the root;
  record folder/item totals. `1av`. Where `inventory-fetch` proves the single
  root folder answers, this proves the recursive descent into the whole tree.
  The case crawls breadth-first from the agent root, issuing a
  `RequestFolderContents` per folder and following every sub-folder reported in
  the `InventoryDescendents` reply (deduping folders and items by id, bounded by
  a `MAX_FOLDERS` safety cap), until the queue drains. It is the same per-folder
  fetch the client's automatic background crawl issues, here pumped explicitly
  by the test so completion is deterministic (the background-crawl flag is a
  client-construction option, not a `Command` the harness can drive); the
  library still routes each fetch to the modern CAPS
  `FetchInventoryDescendents2` where the region advertises it (Second Life) or
  the legacy UDP
  `FetchInventoryDescendents` where it does not (OpenSim). It records the
  folder/item totals and the deepest level reached, and asserts the crawl went
  *beyond* the root — more than one folder and depth ≥ 1 — since a stock agent
  inventory's root holds the standard system sub-folders. Green on OpenSim: 26
  folders, 30 items, max depth 3, crawl ≈ 2.6 s loopback. `[both]`.
- [x] `ais3-folder-lifecycle` — create / rename / move / remove / purge a
  folder (CAPS AIS3 on SL; gate vs UDP on OpenSim). `1av`. Where
  `inventory-tree-crawl` proves the *read* side, this proves the *write* side:
  every structural folder mutation a viewer performs, gated on the per-grid
  path — Second Life carries them over the modern **AIS3** (`InventoryAPIv3`)
  CAPS REST endpoint (`Ais3CreateFolder`/`Ais3RenameFolder`/`Ais3MoveFolder`/
  `Ais3PurgeFolder`/`Ais3RemoveFolder`), OpenSim over the legacy UDP messages
  (`CreateInventoryFolder`/`UpdateInventoryFolder`/`MoveInventoryFolder`/
  `PurgeInventoryDescendents`/`RemoveInventoryFolder`). The UDP mutations are
  fire-and-forget (OpenSim sends no reply, the client caches optimistically),
  so the case never trusts that optimistic cache: after every step it re-fetches
  the affected parent over `RequestFolderContents` and asserts against the
  grid's authoritative `InventoryDescendents` reply, polling to absorb OpenSim's
  fire-and-forget descendents/purge workers. The lifecycle creates a destination
  and a subject under the agent root, renames the subject, gives it a child (so
  purge has something to empty), moves it under the destination (re-parent
  asserted on both edges — present under the new parent, gone from the old),
  then sends it to **Trash** before purging and removing. That Trash step is not
  incidental: both grids only let a folder be purged or deleted once it lives
  under Trash (the viewer's delete = move-to-trash-then-empty flow; OpenSim
  enforces it with an `onlyIfTrash` guard in `XInventoryService`, so a purge or
  remove of a folder outside Trash is silently a no-op — the bug the first run
  surfaced). Purge then empties the subject (child gone, subject survives) and
  remove deletes it; the destination is sent to Trash and removed at the end so
  re-runs start clean. Green on OpenSim via the `udp` path; create ≈ 0.1 s,
  rename ≈ 0.1 s, move ≈ 0.8 s, purge ≈ 0.8 s, remove ≈ 0.7 s, full lifecycle
  ≈ 2.9 s loopback. `[both]`; the AIS3 (`aditi`) path is written but not yet
  run live (no aditi record produced this session).
- [x] `inventory-item-ops` — create / copy / move / link an item. `1av`. Where
  `ais3-folder-lifecycle` proves the write side for *folders*, this proves
  it for the *items* inside them. All four operations ride the UDP messages
  on both grids (the reference viewer still creates, copies, moves, and
  links items over UDP even where AIS3 exists — AIS3 carries folder
  mutations and item *metadata* edits), so this is a single `[both]` path
  with no per-grid branching. Create, copy, and link each draw a direct
  reply that allocates the new item's server id: `CreateInventoryItem` /
  `LinkInventoryItem` answer with an `UpdateCreateInventoryItem`
  (`Event::InventoryItemCreated`), and a copy answers the same way on
  OpenSim (its `CopyInventoryItem` routes through the same
  `CreateNewInventoryItem` → `SendInventoryItemCreateUpdate` path) or as a
  `BulkUpdateInventory` (`Event::InventoryBulkUpdate`) elsewhere, so the
  copy predicate accepts either. The case captures the new id from that
  reply, then — never trusting the optimistic local cache — re-fetches the
  affected folder over `RequestFolderContents` and asserts against the
  grid's authoritative `InventoryDescendents` item list, polling to absorb
  OpenSim's fire-and-forget descendents worker. The lifecycle creates a
  `src` and a `dst` folder under the agent root, then: **create** a notecard
  in `src`; **copy** it to a second name in `src` (the original must survive
  — a copy, not a move); **move** the original to `dst` (asserted on both
  edges — present under `dst`, gone from `src`, copy untouched in `src`);
  **link** to the moved original, filing the link in `src` (asserting the
  link's target still lives in `dst` — a pointer, not a relocation). Created
  items are deleted (item deletion is not Trash-gated) and both working
  folders sent to Trash + removed at the end so re-runs start clean. Green
  on OpenSim; create ≈ 0.1 s, copy ≈ 0.1 s, move ≈ 0.8 s, link ≈ 0.2 s, full
  lifecycle ≈ 1.2 s loopback. `[both]`; the aditi run is deferred with the
  batch (no aditi record produced this session). Required re-exporting
  `NewInventoryLink` from both runtime crates (the only API addition).
- [x] `library-tree-fetch` — fetch the read-only Library tree. `1av`. Where
  `inventory-tree-crawl` walks the agent's *own* tree, this walks the grid-owned
  read-only **Library** — a second inventory tree owned by a distinct Library
  owner, surfaced alongside the agent root by `QueryInventoryRoots`. The crawl
  is the same breadth-first descent over `RequestFolderContents` /
  `InventoryDescendents`, but every Library folder is filed under the Library
  owner, so the library routes each fetch to `FetchLibDescendents2` where the
  region advertises it (Second Life) or the legacy UDP
  `FetchInventoryDescendents` addressed to the Library owner where it does not
  (OpenSim) — automatically, a single `[both]` path. It asserts the Library is a
  *separate* tree (its root is distinct from the agent root) and that the
  descent went beyond the root (folders > 1, depth ≥ 1, since a stock Library
  holds the standard system sub-folders). A grid with no Library is recorded
  `partial` rather than failed. **Surfaced & fixed a real client bug**
  (behavioural, in `sl-proto` so both runtimes get it): OpenSim emits a single
  nil-id placeholder `FolderData` block for an empty folder (an LLUDP "stuffing"
  quirk a real viewer ignores), which the `InventoryDescendents` fold passed
  through verbatim — so the crawl tried to fetch the phantom nil folder (OpenSim
  never answers) and hung, and the background crawl would have marked it
  `Fetching` forever, never reaching `fully_loaded(Library)`. The UDP and CAPS
  folds now drop nil-id folders/items, matching the existing
  `bulk_update_inventory_from_llsd` filter; regression test
  `inventory_descendents_drops_nil_placeholder_subfolder` in
  `sl-proto/tests/lifecycle.rs`. Every OpenSim Library leaf carries this
  stuffing block (the agent tree did not, which is why `inventory-tree-crawl`
  never hit it). Green on OpenSim: 7 folders (root + 6), 17 items, depth 1,
  crawl ≈ 0.5 s
  loopback. `[both]`; the aditi run is deferred with the batch (no aditi record
  produced this session).
- [x] `inventory-cache-skip` — refetch with matching version is skipped. `1av`.
      Where the other Phase 5 cases prove the inventory tree can be *fetched*
      and *mutated*, this proves it need not be fetched **again**: the runtime's
      inventory disk cache lets a relogin restore version-unchanged folders
      straight from `<agent-uuid>.inv.llsd.gz` instead of refetching them. The
      runtime loads the cache before the login skeleton and reconciles it
      (Firestorm's `loadSkeleton`): a cached folder whose version equals the
      skeleton's keeps its loaded contents (`FolderState::Loaded`); a mismatch
      is invalidated and requeued. The case drives the cache directory through
      new harness support (a cleared per-case, per-grid dir under the gitignored
      `.sl-conformance/`, opted into by a `GridTest::inventory_cache()` hook, so
      the first login is genuinely cold) and the mid-run
      `Session::disconnect`/`relogin` cycle `offline-msg-fetch` introduced
      (disconnect writes the cache on logout; relogin reads it back). It asserts
      the version-matching skip directly from the held model via
      `Command::QueryInventoryFolder`: the agent root's child folders are
      **`Unknown`** before the crawl (cold — nothing loaded) and
      **`Loaded` at the same version** after the relogin (warm), with no refetch
      issued that session. The cache load/merge keys only on the login skeleton
      (which both grids send), so it is a single `[both]` path; only the
      underlying per-folder crawl picks CAPS vs UDP per region. Green on
      OpenSim: all 24 agent-root child folders went cold-`Unknown` →
      warm-`Loaded` at the identical version (24/24 version-matched, 29 folders
      cached), crawl ≈ 2.9 s, relogin ≈ 5.1 s loopback. `[both]`; the aditi run
      is deferred with the batch (no aditi record this session).
- [x] `give-inventory` — give an item to another avatar; peer accepts.
  `2av` (OpenSim now; Aditi deferred → Phase Z). The cross-avatar hand-off:
  where `inventory-item-ops` proves a single avatar manipulates its *own*
  items, this proves one avatar gives an item to another. A give is an
  `ImprovedInstantMessage` with the `IM_INVENTORY_OFFERED` dialog whose binary
  bucket carries the offered asset's type byte and id, routed by the grid's IM
  service to the named recipient (not broadcast like local chat); the recipient
  decodes the offer and replies `IM_INVENTORY_ACCEPTED`, which the grid relays
  back to the giver. Sequence: the primary creates a transferable notecard in
  its own Notecards folder, `GiveInventory`s it to the secondary with a fresh
  correlation id, the secondary observes the matching `Event::InstantMessage
  Received` (`InventoryOffered`, attributed to the primary) and decodes the
  `InventoryOffer`, then `AcceptInventoryOffer`s filing it into its Notecards
  folder; the primary observes the matching `InventoryAccepted` IM (the
  round-trip confirmation `calling-card`'s no-op accept lacks), and the case
  re-fetches the recipient's Notecards folder and asserts the item's copy is
  present — never trusting the optimistic cache. OpenSim's `InventoryTransfer
  Module` files a *copy* into the recipient at offer time (in the default folder
  for the asset type) and rewrites the offer bucket to carry the new copy's id,
  so the decoded offer's `item_id` is the recipient's copy, not the giver's
  original; the original keeps its Copy permission so the grid leaves it behind.
  Green on OpenSim; offer RTT ≈ 7.9 ms, accept RTT ≈ 0.6 ms loopback.
  `[opensim]` only.

## Phase 6 — Groups `[both]`

OpenSim requires Groups V2 enabled (see appendix).

- [x] `group-create-activate` — create a group, activate it. `1av`. The group
  lifecycle entry point: `CreateGroup` makes the primary the founder/owner,
  then `ActivateGroup` sets the agent's active group. OpenSim auto-activates a
  group at creation time (`GroupsService.CreateGroup` stamps the founder's
  principal record with the new group as active), so a bare "activate then check
  active == group" would not exercise the command — creation already left it
  active. To make the activation a genuine, observable transition, the case
  first *clears* the active group with `ActivateGroup(None)` and confirms the
  grid reports no active group, then activates the new group and confirms it is
  reported active (`Event::ActiveGroupChanged`) with the founder's non-zero
  powers and the group's name. This also drove an idiomatic API change:
  `Command::ActivateGroup` / `Session::activate_group` now take an
  `Option<GroupKey>` (`None` clears, sent as the nil group id on the wire),
  mirroring the read side where `ActiveGroup::active_group_id` is already an
  `Option`; the REPL gained an `Args::opt_uuid` so `activate_group` with no
  argument clears. Green on OpenSim: create ≈ 0.39 s, clear ≈ 0.05 s, activate
  ≈ 0.06 s loopback; owner powers `0x000ffffffffffffe`. `[both]`; the Aditi run
  is deferred with the batch (no aditi record this session).
- [x] `group-join-leave` — join and leave. `2av`. Plain membership churn,
  the complement to `group-create-activate`'s founder lifecycle: the primary
  owns a throwaway open-enrollment group while the **secondary** does the
  join/leave the case actually tests. It must be the secondary, not the
  primary: the founder is the group's last owner and a grid will not let the
  last owner drop the group out from under it, so `2av` is intrinsic, not
  incidental. Both ends are observable on OpenSim: a join replies
  `JoinGroupReply` (`Event::JoinGroupResult { success }`); a leave is a
  two-event transition — `GroupsModule.LeaveGroupRequest` sends
  `LeaveGroupReply` (`Event::LeaveGroupResult { success }`) *and then*
  `AgentDropGroup` (`Event::DroppedFromGroup`), the membership-list update
  that proves the agent is genuinely out, not merely acked. The case asserts
  both so the leave is a real transition rather than a bare reply. Green on
  OpenSim (local secondary `Friend Tester`, Groups V2 enabled): create
  ≈ 0.20 s, join ≈ 0.12 s, leave ≈ 0.11 s loopback. **Pre-made-group reuse
  (new support):** group creation on Second Life costs **L$100**, an emptied
  group purges only ~48 h after dropping below two members, and the founder
  holds a group slot per created group — so creating per run on SL spends L$
  and marches the founder toward the ~42-group cap. The group cases that do not
  themselves test creation therefore take their group(s) from
  `support::membership_group`, which reuses pre-made groups listed (by
  position) in a gitignored `fixtures.<grid>.toml` when present (the SL path)
  and otherwise creates throwaways (the OpenSim default, free and disposable).
  `group-join-leave` and `group-session-message` use the first fixture group;
  `chat-invite-accept-decline` uses the first two (it needs two distinct pending
  sessions). Each leaves any group it joined, so a reused fixture is restored to
  its founder-only state for the next run (a fresh join is also what makes the
  invitation case fire). The reuse path was verified end to end against two
  primary-owned OpenSim groups, confirming join/leave/cleanup leave the
  fixtures clean. Aditi deferred to Phase Z pending a second Aditi avatar and
  configured pre-made groups.
- [x] `group-roster` — fetch members / roles / titles / profile. `1av`. The
  **read** side of a group, complementing `group-create-activate`'s lifecycle
  and `group-join-leave`'s membership churn: the five roster queries a viewer
  issues on opening a group's profile floater — `RequestGroupProfile`
  (`Event::GroupProfileReceived`), `RequestGroupMembers`
  (`Event::GroupMembers`), `RequestGroupRoles` (`Event::GroupRoleData`),
  `RequestGroupRoleMembers` (`Event::GroupRoleMembers`), and
  `RequestGroupTitles` (`Event::GroupTitles`).
  Rather than assert each reply in isolation, the case cross-checks them so the
  run proves they describe the *same*, self-consistent group: the profile names
  a founder and an owner role; the member roster must then carry that founder
  flagged as an owner; the role list must contain that owner role; and the
  role↔member pairings must pair the founder with the owner role — catching a
  stale or mismatched roster, not merely an empty one. One title is the agent's
  currently selected title. The group comes from `support::membership_group`
  (index 0): a throwaway created per run on OpenSim (the primary becomes
  founder/owner), or a reused pre-made group on Second Life (avoiding the
  per-run L$100 and a founder slot); the case only reads the group, leaving it
  exactly as found. On the created path the founder is the primary itself, so
  the case also pins the reported founder to the primary's own agent id. Green
  on OpenSim: 1 member, 3 default roles (Everyone/Officers/Owners), 2
  role-member pairs, 1 selected title; profile ≈ 23 ms, members ≈ 15 ms, roles
  ≈ 51 ms, role-members ≈ 10 ms, titles ≈ 15 ms loopback. `[both]`; the Aditi
  run is deferred with the batch (needs a configured pre-made group to avoid the
  L$ cost; no aditi record this session).
- [ ] `group-notice` — send and receive a group notice. `2av`.
- [ ] `group-accounting` — account summary / details / transactions. `1av`.
- [ ] `group-proposal-vote` — start a proposal, cast a ballot. `2av`.
- [ ] `group-admin` — eject member / change role members. `2av`; a
  multi-member role/roster assertion wants **`3av`** (Aditi deferred).

## Phase 7 — Avatar profile & social `[both]`

OpenSim needs the UserProfiles fix (see appendix) for profile/picks paths.

- [ ] `avatar-properties` — request another avatar's properties. `1av`.
- [ ] `profile-edit-roundtrip` — update profile / interests; read back. `1av`.
- [ ] `picks-classifieds` — request and edit picks / classifieds. `1av`.
- [ ] `avatar-notes` — write and read avatar notes. `1av`.
- [ ] `display-names` — CAPS `GetDisplayNames`. `[aditi] 1av`.
- [ ] `mute-list` — mute / unmute and fetch the mute list. `1av`.

## Phase 8 — Objects & scene graph `[both]`

Most cases need a rezzed (and some a scripted) object — see appendix for the
OAR / XEngine setup.

- [ ] `object-update-decode` — receive and decode the object-update stream;
  count primitives. `1av`.
- [ ] `object-properties` — request properties and properties-family. `1av`.
- [ ] `object-touch-grab` — touch and grab/degrab an object. `1av`.
- [ ] `object-rez-derez` — rez from inventory, then derez/delete. `1av`.
- [ ] `object-link-delink` — link and delink a set. `1av`.
- [ ] `object-edit` — set name / desc / flags / shape / material /
  permissions / for-sale. `1av`.
- [ ] `task-inventory` — request and update a prim's task inventory. `1av`.

## Phase 9 — Scripting & permissions `[both]`

Needs XEngine + a scripted-object OAR (appendix). Note SL enforces god-bit;
OpenSim may not.

- [ ] `script-dialog` — receive a `ScriptDialog`, reply. `1av`.
- [ ] `script-permissions` — request / grant / revoke script permissions. `1av`.
- [ ] `script-running` — query and toggle script running, reset. `1av`.

## Phase 10 — Parcel & land `[both]`

Edits need the estate-owner avatar.

- [ ] `parcel-properties` — request parcel properties (note the CAPS
  EventQueue path on SL vs UDP). `1av`.
- [ ] `parcel-info-dwell` — parcel info and dwell. `1av`.
- [ ] `parcel-access-list` — read and update the access list. `1av`.
- [ ] `modify-land` — raise/lower terrain, then undo. `1av`.
- [ ] `parcel-divide-join` — divide then join parcels. `1av`.
- [ ] `parcel-object-owners` — request object owners / return objects. `1av`.

## Phase 11 — Region, estate & map `[both]`

- [ ] `simulator-features` — request simulator features. `1av`.
- [ ] `environment` — request environment settings. `1av`.
- [ ] `open-region-info` — OpenRegionInfo limits bag. `[opensim] 1av`.
- [ ] `estate-info` — request estate info / covenant. `1av` (estate owner).
- [ ] `estate-access` — update estate access list. `1av` (estate owner).
- [ ] `map-blocks-items` — request map blocks / items / layer. `1av`.

## Phase 12 — Teleport (state machine) `[both]`

- [ ] `teleport-local-phases` — local teleport; assert the phase sequence
  Starting → Progress → Landing → Complete. `1av`.
- [ ] `teleport-failed` — provoke a failed teleport; assert `TeleportFailed`.
  `1av`.
- [ ] `teleport-cross-region` — cross-region with child circuits (OpenSim
  multi-region on ports 9001-9003 already configured). `1av`.
- [ ] `teleport-offer-accept` — offer a lure, peer accepts. `2av` (Aditi
  deferred).

## Phase 13 — Asset & texture pipeline `[both]`

- [ ] `texture-fetch-http` — HTTP CAPS texture fetch + decode (extends
  `asset-decode`). `1av`.
- [ ] `mesh-fetch-http` — HTTP CAPS mesh fetch + decode. `1av`.
- [ ] `asset-transfer-udp` — legacy UDP asset transfer fallback. `1av`.
- [ ] `asset-upload` — upload via UDP and via CAPS
  `NewFileAgentInventory`. `1av`.
- [ ] `baked-texture-upload` — upload a baked texture (CAPS). `1av`.

## Phase 14 — Appearance, attachments & animations `[both]`

- [ ] `wearables-request` — request current wearables. `1av`.
- [ ] `set-appearance` — set appearance / cached textures. `1av`.
- [ ] `attach-detach` — rez attachment, then detach into inventory. `1av`.
- [ ] `animation-play-stop` — play and stop an animation. `1av`.
- [ ] `gestures` — activate / deactivate gestures. `1av`.

## Phase 15 — Money & economy `[both]`

OpenSim needs BetaGridLikeMoneyModule; balance is hardcoded 0 there, so assert
the message flow, not amounts.

- [ ] `money-balance` — request balance; observe reply. `1av`.
- [ ] `economy-data` — request economy data. `1av`.
- [ ] `money-transfer` — send a transfer (mark partial where no real backend).
  `2av`.

## Phase 16 — Directory & search `[both]`

- [ ] `dir-find-people-groups-events` — `DirFindQuery` across types. `1av`.
- [ ] `dir-places-land-classified` — places / land / classified queries. `1av`.
- [ ] `avatar-picker` — avatar picker request. `1av`.
- [ ] `event-info` — event info / notification add-remove. `1av`.

## Phase 17 — Voice signalling `[aditi] 1av`

Signalling and session state only — no audio transport (out of scope).

- [ ] `voice-account` — provision a voice account. `1av`.
- [ ] `parcel-voice-info` — request parcel voice info. `1av`.
- [ ] `voice-signaling` — exchange voice signalling. `1av`/`2av`.

## Phase 18 — Experiences `[aditi] 1av`

- [ ] `experience-info` — info / find by name. `1av`.
- [ ] `experience-permissions` — request / set experience permission. `1av`.
- [ ] `experience-admin-contributor` — admin / contributor / owned / region
  queries. `1av`.

## Phase 19 — Error handling & recovery `[both]`

Some are easier to provoke on OpenSim.

- [ ] `server-error` — provoke and assert `Error` / `FeatureDisabled`. `1av`.
- [ ] `kick-user` — observe `KickUser` handling. `1av`.
- [ ] `agent-alert` — observe `AgentAlertMessage` / `AlertMessage`. `1av`.
- [ ] `reliable-retransmit` — exercise reliable resend under loss. `1av`.

## Phase 20 — Server side (SimSession) — stretch, no grid

Optional final tier: in-process client ↔ `SimSession` round-trips for messages
that are hard to provoke against a live grid. Complements
`sl-proto/tests/sim_session.rs`. These are not grid-gated.

- [ ] `simsession-roundtrip` — drive a representative set of messages both ways
  through `SimSession` and assert symmetric decode/encode.

---

## Phase Z — Deferred: multi-avatar Aditi work

Collects every multi-avatar case that needs additional **Aditi** avatars, so it
does not block Phases 1-19. Each item is the Aditi variant of a case already
listed in its functional-area phase.

Provisioning needed:

- A **2nd Aditi avatar** unblocks all Aditi `2av` cases: chat (Phase 2),
  IM/sessions (3), friends/presence (4), give-inventory (5), group join/leave
  /notice/proposal (6), teleport offer (12), money transfer (15). Existing
  follow-up: see memory `sl-conformance-harness` and `SL_REPL_ROAD_MAP.md` E3.
- A **3rd Aditi avatar** is needed ONLY for the `3av` cases:
  `conference-roster` (Phase 3) and the multi-member `group-admin` roster
  assertion (Phase 6).

OpenSim `2av`/`3av` equivalents do NOT wait on Aditi — the local secondary
`Friend Tester` already exists, extra console avatars are cheap, and the Phase 0
tertiary-avatar harness support is the only prerequisite for OpenSim `3av`.

- [ ] Provision a 2nd Aditi avatar; add it to `credentials.aditi.toml`.
- [ ] Provision a 3rd Aditi avatar (for conference / group-roster only).
- [ ] Add `[aditi]` variants of the deferred cases as the avatars land.

---

## Setup-cost appendix (OpenSim)

What must be enabled before a feature can be live-tested locally. Each points at
a memory note with the full procedure.

| Area / phases | Default | To enable | Memory note |
| --- | --- | --- | --- |
| Movement / physics | ubODE off | set `physics = ubODE` in OpenSim.ini | `opensim-needs-real-physics-for-movement` |
| Profiles, picks (7) | off / wrong URL | enable UserProfilesService, fix ProfileServiceURL to :9000 | `sl-client-opensim-profiles-setup` |
| Scripting (9), scripted objects (8) | XEngine off | enable XEngine, load a scripted OAR, restart, match region | `sl-client-opensim-scripted-object-testing` |
| Groups (3, 6) | Groups V2 off | podman MariaDB 10.6 on :3307, MessageOnlineUsersOnly + GridUser config | `sl-client-opensim-groups-v2-setup` |
| Money (15) | off | enable BetaGridLikeMoneyModule (balance hardcoded 0, transfers need a real backend) | `sl-client-opensim-money-module-setup` |

General live-test setup (start `opensim.service`, test avatars, console output
in the journal): memory `sl-client-test-avatar-and-smoke-tests`. Aditi login
(rustls, `credentials.aditi.toml`, YubiKey TOTP): memory
`sl-client-aditi-live-testing`.
