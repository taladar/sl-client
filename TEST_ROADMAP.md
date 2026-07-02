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

    sl-conformance run --grid opensim <name>
    sl-conformance run --grid aditi  <name> --force   # --force skips cooldown
    sl-conformance-report                              # green = Current

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
- [x] `group-notice` — send and receive a group notice. `2av`. The
  group's **one-shot announcement** path, complementing
  `group-session-message`'s live
  conversation and `group-join-leave`'s membership churn: the primary owns the
  group and posts a notice (`Command::SendGroupNotice`, an `IM_GROUP_NOTICE`
  whose subject and body are joined with a `|` on the wire), while the
  **secondary** — having joined the open-enrollment group — is the member that
  receives it. `2av` is intrinsic: send-and-receive needs a receiver distinct
  from the poster (the grid also relays the notice back to the founder, but
  proving delivery to *another* avatar is the point). A freshly joined member
  accepts notices by default (`AcceptNotices = "1"`), so no accept-notices
  toggle is needed. The relayed IM is attributed *from the group* —
  `from_group` set, `from_agent_id` the group id, not the posting avatar — so
  the receive predicate keys on the `GroupNotice` dialog and the exact
  `subject|body` rather than a sender id; its session id (`InstantMessage::id`)
  is the new notice's id. After
  observing the live delivery the case cross-checks persistence: the primary
  fetches the notice history (`RequestGroupNotices` → `Event::GroupNotices`) and
  asserts the just-posted notice is present with the same id and subject and no
  attachment — proving the notice was stored, not merely echoed, exercising the
  `GroupNoticesListReply` read path alongside IM delivery. The group comes from
  `support::membership_group` (index 0): a throwaway created per run on OpenSim,
  or a reused pre-made group on Second Life; the secondary leaves any group it
  joined, restoring a reused fixture to its founder-only state. Green on OpenSim
  (local secondary `Friend Tester`, Groups V2 enabled): create ≈ 0.32 s, join
  ≈ 0.06 s, notice deliver ≈ 44 ms, history fetch ≈ 16 ms loopback (1 listed
  notice). `[both]`; the Aditi run is deferred with the batch (needs a second
  Aditi avatar and a configured pre-made group; no aditi record this session).
- [x] `group-accounting` — account summary / details / transactions. `1av`.
  The three requests a viewer issues for a group's "Land & L$" floater, one per
  tab: `RequestGroupAccountSummary` (→ `Event::GroupAccountSummary`),
  `RequestGroupAccountDetails` (→ `Event::GroupAccountDetails`), and
  `RequestGroupAccountTransactions` (→ `Event::GroupAccountTransactions`). Each
  is a reliable `S32`-parameterised request keyed by a client-chosen `RequestID`
  echoed back for correlation; the case mints a fresh `GroupRequestId` per
  request and pairs every reply by it, with interval parameters matching the
  reference viewer exactly (summary 7-day / details 1-day / transactions 7-day,
  all at current interval 0). The group comes from `support::membership_group`
  (index 0) so the primary holds the group's `Accountable` power. Listed
  `[both]`, but the grids exercise different halves. **OpenSim has no
  group-accounting backend:** `LLClientView` parses and acks all three requests
  and fires `OnGroupAccountSummaryRequest` and siblings, but no region module
  subscribes to those events, so it never replies (the `SendGroupAccounting*`
  methods exist but are dead code) — confirmed across core and optional modules
  including `SampleMoneyModule` (the `BetaGridLikeMoneyModule` config target).
  The OpenSim run therefore proves the client *encodes and transmits* all three
  requests in a form a real simulator accepts — it watches the circuit past the
  reliable-retransmit budget via keep-alive pings (the
  acceptance-by-absence-of-failure check `throttle-set` uses) — then marks the
  dataset partial, since no reply data is observable. Green-partial on OpenSim:
  create ≈ 0.43 s, ping ≈ 0.5 ms loopback, 3 requests sent, 0 replies. The
  **reply assertions are the Second Life variant** (deferred with the Aditi
  batch): wait for all three replies, correlate each by request/group id, and
  assert the echoed interval parameters; the SL run additionally needs the
  primary to hold the configured pre-made group's `Accountable` power.
- [ ] `group-proposal-vote` — start a proposal, cast a ballot. `2av`. **No live
  variant on any grid — group proposals/voting is a removed feature.**
  Investigated 2026-06-30 (and confirmed by the project owner): Second Life
  **removed group voting entirely** ("vote removal", DEV-24856). The reference
  viewer keeps only the power bits `GP_PROPOSAL_START`/`GP_PROPOSAL_VOTE`,
  explicitly marked `_DEPRECATED` "as part of vote removal" in
  `roles_constants.h`; there is no `panel_group_voting` UI, no send code, and
  the four messages are `UDPDeprecated` in `message_template.msg` — so a modern
  SL viewer never starts a proposal or casts a ballot, and there is nothing to
  observe on SL. **OpenSim has no proposal/voting backend either** — more absent
  than group-accounting: `StartGroupProposal` and `GroupProposalBallot` are not
  even parsed by `LLClientView` (no `AddLocalPacketHandler`, no
  `OnStartGroupProposal`/`OnGroupProposalBallotRequest` event), and
  `GroupActiveProposalsRequest`/`GroupVoteHistoryRequest` *are* parsed and fire
  their events but no region module subscribes (the `SendGroupActiveProposals`/
  `SendGroupVoteHistory` methods are dead code, like the accounting ones), so
  none of the four messages yields any reply. The genuine proposal/voting
  behaviour is therefore unobservable on **both** grids and not deferrable to
  Phase Z (no avatar count makes it work) — same document-and-skip outcome as
  `conference-roster`. The client retains the command/event surface
  (`Command::StartGroupProposal`/`RequestGroupActiveProposals`/
  `GroupProposalBallot`/`RequestGroupVoteHistory`,
  `Event::GroupActiveProposals`/`GroupVoteHistory`) for completeness and for the
  server-side `SimSession` encoders; a transmit-only "the client still encodes
  these legacy messages acceptably" check was prototyped but dropped as
  low-value given the feature is dead everywhere.
- [x] `group-admin` — eject member / change role members. `2av`. The
  **admin** side of a group, complementing `group-join-leave`'s self-churn and
  `group-roster`'s read side: the **primary** owns the group and the
  **secondary** — having joined the open-enrollment group — is the member it
  acts on. `2av` is intrinsic: an owner cannot eject itself (it leaves instead),
  and a self role-change would not exercise the cross-member path. Two halves,
  each asserted against the grid's authoritative state rather than the
  optimistic local cache. **Role change** (`Command::ChangeGroupRoleMembers`, a
  `GroupRoleChanges`) draws no direct reply, so after assigning the secondary to
  a non-owner assignable role — the stock "Officers" role, found as the role
  whose id is neither the nil "Everyone" role nor the profile's owner role — the
  case re-requests the role↔member pairings (`Event::GroupRoleMembers`) and
  polls until the new pairing appears, then removes the assignment and polls
  until it is gone (proving a real transition, not a one-way add). **Ejection**
  (`Command::EjectGroupMembers`, an `EjectGroupMemberRequest`) is a two-event
  transition like a voluntary leave: OpenSim replies to the ejector with
  `EjectGroupMemberReply` (`Event::EjectGroupMemberResult { success }`) *and*
  sends the ejectee `AgentDropGroup` (`Event::DroppedFromGroup`), the
  membership-list update proving the member is genuinely out; the case asserts
  both. The ejection also restores a reused pre-made group to its founder-only
  state for the next run. The group comes from `support::membership_group`
  (index 0): a throwaway created per run on OpenSim (the primary becomes
  founder/owner, holding the `RoleAssignMember` + `MemberEject` powers), or a
  reused pre-made group on Second Life. Green on OpenSim (local secondary
  `Friend Tester`, Groups V2 enabled): create ≈ 0.43 s, role-add ≈ 48 ms,
  role-remove ≈ 68 ms, eject ≈ 82 ms loopback. `[opensim]` only; the Aditi
  variant — and a multi-member role/roster assertion that wants a **`3av`**
  third avatar — is deferred to Phase Z pending more Aditi avatars (and a
  configured pre-made group).

## Phase 7 — Avatar profile & social `[both]`

OpenSim needs the UserProfiles fix (see appendix) for profile/picks paths.

- [x] `avatar-properties` — request another avatar's properties. `1av`.
  Where `profile-edit-roundtrip` edits the agent's *own* profile, this reads a
  **different** avatar's — the "open someone's profile floater" lookup.
  `Command::RequestAvatarProperties(target)` draws an `AvatarPropertiesReply`
  (`Event::AvatarProperties`) carrying that avatar's account-level facts
  (account creation date, partner, about text, flags), with an
  `AvatarInterestsReply` (`Event::AvatarInterests`) alongside on grids that send
  it. The target need not be online — profile data is profile-service state, not
  presence — so a single
  logged-in avatar reads any account's profile (`1av`). The point (vs
  `profile-edit-roundtrip`) is that the reply describes *that other avatar*, so
  the case asserts the reply's `avatar_id` equals the requested target and
  differs from the logged-in primary, and that the grid returned real account
  data (a non-empty `born_on`) rather than the "profile not available"
  placeholder. The "other avatar" id is resolved per grid: OpenSim falls back to
  the local secondary test avatar (`Friend Tester`, a fixed-UUID account on the
  workspace grid) so no configuration is needed; Second Life has no built-in
  second avatar, so the aditi run reads the `other_avatar` configured in
  `fixtures.aditi.toml` (a new fixtures field), recording `partial` when that
  fixture is absent. Needs the OpenSim UserProfiles module enabled (appendix).
  Green on OpenSim: properties RTT ≈ 44 ms loopback, interests reply received,
  `born_on` present. `[both]`; the aditi run is deferred with the batch (needs
  the `other_avatar` fixture; no aditi record this session).
- [x] `profile-edit-roundtrip` — update profile / interests; read back. `1av`.
  Where `avatar-properties` reads a **different** avatar's profile, this is the
  "edit my own profile floater" round-trip: it reads the agent's current
  profile and interests, writes a changed copy back, and confirms a fresh read
  reflects the edit. Both `Command::UpdateProfile` (`AvatarPropertiesUpdate`)
  and `Command::UpdateInterests` (`AvatarInterestsUpdate`) *replace the whole
  record*, so the case reads the current values first and edits from there
  rather than blanking unrelated fields (the publish/mature booleans are
  reconstructed from the read profile `flags`). Neither update carries an ack,
  so the edit is verified by polling a fresh `RequestAvatarProperties` read
  until the new value appears; the about-text and interests markers *toggle*
  between two fixed values keyed off what was just read, so every re-run is a
  real, detectable change and an interrupted run self-heals. After asserting the
  edit, the case writes the originals back so it leaves the profile as it found
  it. Live finding: a single `RequestAvatarProperties` yields the properties
  **and** the interests reply that follows, so the read-back must consume both
  together — reading the interests apart races the queued replies and reads a
  stale value. `1av`, `[both]`. When a grid omits interests that half is
  recorded `partial`, not failed. OpenSim needs the UserProfiles module enabled
  (appendix). Green on OpenSim: about-text + interests edits both reflected,
  re-read RTT ≈ 60 ms loopback, profile restored. The aditi run is deferred with
  the batch (no aditi record this session).
- [x] `picks-classifieds` — request and edit picks / classifieds. `1av`.
  The profile "Picks" and "Classifieds" tabs: a create → read → edit → delete
  round-trip over the two per-account lists the profile service keeps, driven on
  the agent's own profile. Picks use `Command::UpdatePick`/`DeletePick` and are
  verified through the replies the simulator *volunteers* after each edit — a
  `PickInfoUpdate` draws back both an `AvatarPicksReply` (the whole list) and a
  `PickInfoReply` (the full record), a `PickDelete` a fresh list — so the case
  asserts the created pick's volunteered detail, sweeps any marker pick a prior
  interrupted run left behind (from that same list), confirms the edited
  description on the next volunteered detail, then deletes and confirms it left
  the list. Classifieds get no volunteered reply, so they are read back with the
  typed `Command::RequestClassifiedInfo` (`ClassifiedInfoRequest`), using a
  fixed id (re-runs edit one record, not piling up) and toggling the description
  so each edit is a detectable change. **Two live OpenSim findings shaped this,
  both worked around rather than fixed:** the `avatarpicksrequest` /
  `avatarclassifiedsrequest` list *queries* (both `GenericMessage`s, correctly
  encoded and session-matched on the wire) go unanswered by stock OpenSim for
  the agent's own profile — hence the volunteered-reply and typed-detail paths
  above, never a bare list query; and OpenSim's `classified_delete` throws a
  data-layer SQLite error and leaves the record, so classified deletion is
  best-effort (recorded, not asserted; the fixed id keeps the leftover to
  one). A
  classified listing costs L$ on Second Life (this case lists at L$0, which
  OpenSim accepts and SL declines), so when the created classified never reads
  back that half is recorded `partial`, not failed. Needs the OpenSim
  UserProfiles module enabled (appendix). Green on OpenSim: pick create RTT
  ≈ 20 ms loopback, pick listed / edited / deleted, classified create RTT
  ≈ 15 ms, classified edited (delete records `false` — the OpenSim bug).
  `[both]`; the aditi run is deferred with the batch (no aditi record this
  session). Added
  a `ClassifiedKey` re-export to both runtime crates (sibling of the existing
  `PickKey`).
- [x] `avatar-notes` — write and read avatar notes. `1av`.
  The private, per-account free-text note a viewer keeps about *another* avatar
  (the profile floater's "My Notes" box) — profile-service state keyed on the
  pair (viewing agent, target), never shown to the target and independent of
  presence, so one logged-in avatar drives the whole round-trip (`1av`). A read
  is `Command::RequestAvatarNotes` (the `avatarnotesrequest` `GenericMessage`)
  answered by an `AvatarNotesReply` (`Event::AvatarNotes`); a write is
  `Command::UpdateAvatarNotes` (`AvatarNotesUpdate`), which carries no ack, so
  the edit is verified by polling a fresh read until the new text appears. The
  note *toggles* between two fixed markers keyed off the last read, so every
  re-run is a real, detectable change and an interrupted run self-heals; after
  asserting, the case writes the original back to leave the profile as it found
  it. The "other avatar" is resolved per grid like `avatar-properties`: OpenSim
  falls back to the local secondary (`Friend Tester`, fixed UUID); Second Life
  reads the `other_avatar` fixture, `partial` if absent. **Live OpenSim finding
  (worked around, not fixed):** stock OpenSim leaves the `avatarnotesrequest`
  query *unanswered* — the same unresponsive-`GenericMessage` class
  `picks-classifieds` documented — and, unlike picks, `AvatarNotesUpdate`
  volunteers no reply either, so the note is never readable back on OpenSim. The
  case detects the silence (the initial read times out), still pushes a write so
  the `AvatarNotesUpdate` encoding is exercised on the wire, and records
  `partial`; the read-back round-trip is only assertable on a grid that answers
  the query. Unlike every prior Phase 7 case this one is `partial` (not green)
  on OpenSim — `notes_read_answered=false`, write exercised, no positive
  read assertion. Needs the OpenSim UserProfiles module enabled (appendix).
  `[both]`; the aditi run — where the full toggle → write → re-read → assert →
  restore round-trip runs green — is deferred with the batch (no aditi record
  this session).
- [x] `display-names` — CAPS `GetDisplayNames`. `1av`. Batch-resolve agent
  ids to their mutable, user-chosen **display names** (layered over the legacy
  `First Last` identity) over the `GetDisplayNames` HTTP capability: the case
  drives [`Command::RequestDisplayNames`], batching the agent's own id with a
  second known avatar into one GET, and asserts the reply
  (`Event::DisplayNames`) resolves the agent's *own* id to a real,
  non-`missing` record with a non-empty username, legacy name, and display
  name — the observable effect of the cap. **The case only ever reads.** The
  client has no set-display-name command at all (the display-name surface is
  the `GetDisplayNames` lookup plus observing the
  CAPS-pushed `DisplayNameUpdate` / `SetDisplayNameReply`), so it structurally
  cannot touch Second Life's multi-day per-avatar display-name-*change* cooldown
  and is safe to re-run freely. **Re-gated `[aditi]` → `[both]`:** the legend
  lists Display Names under SL-only, but stock OpenSim *does* serve
  `GetDisplayNames` whenever its user-management component is present
  (`BunchOfCaps.cs`), returning the legacy name as a default
  (`is_display_name_default = true`) display name — so the read round-trip is
  assertable on both grids. The second avatar (the `other_avatar` fixture on SL,
  the local secondary `Friend Tester` on OpenSim) is added only to exercise
  multi-id batching; its resolution is best-effort (recorded, not asserted),
  because OpenSim resolves only avatars its region user-management component
  already knows — the logged-in agent always, a not-recently-seen fixed-UUID
  account not necessarily — and returns unknown ids in `bad_ids` (or silently
  omits them) rather than failing. Where a grid omits the capability entirely
  the command is a silent no-op and the case records `partial` on the timeout.
  Added a `DisplayName` / `DisplayNameUpdate` / `SetDisplayNameReply` re-export
  to both runtime crates (the display-name event types were reachable via
  `Event` but not nameable). Green on OpenSim: own id resolved, lookup RTT
  ≈ 70 ms loopback,
  `is_display_name_default = true`, secondary unresolved (not region-known).
  `[both]`; the aditi run — where custom display names distinct from the legacy
  name appear — is deferred with the batch (no aditi record this session).
- [x] `mute-list` — mute / unmute and fetch the mute list. `1av`.
  The agent's own private block list: a full add → read-back → remove →
  read-back round-trip over the per-account mute list the simulator keeps.
  Reading is [`Command::RequestMuteList`] (`MuteListRequest` with a zero CRC,
  forcing a fresh download); the simulator answers by uploading the list file
  over the `Xfer` path behind a `MuteListUpdate` (surfaced, once downloaded and
  parsed, as [`Event::MuteList`]) or — for an empty list — with the
  `emptymutelist` `GenericMessage` (also [`Event::MuteList`]`([])`). Adding is
  [`Command::Mute`] (`UpdateMuteListEntry`), removing is [`Command::Unmute`]
  (`RemoveMuteListEntry`); neither carries an ack, so each edit is verified by
  re-requesting the list until the change shows. The case mutes a **fixed
  synthetic target** (a conformance-owned UUID + name, muted as a
  [`MuteType::Agent`] with default mute-everything flags): nothing external is
  touched — a mute is private block-list state, the target need not be a real
  account, and the fixed id means a re-run edits the one marker rather than
  piling up. Because the round-trip *is* add-then-remove it leaves the list as
  it found it (marker absent) with no separate restore step, and an interrupted
  run self-heals since the next run's remove sweeps a leftover marker. That
  makes the case grid-agnostic and free of any `other_avatar` fixture or
  cooldown concern (muting has no display-name-style change cooldown, so it is
  safe to re-run freely). Unlike every prior Phase 7 case this one needed **no
  new client code and no new runtime-crate re-exports** — the mute
  Command/Event/Session surface and the `MuteEntry`/`MuteFlags`/`MuteType`
  re-exports all already existed — so it is a pure new conformance case. Stock
  OpenSim serves the whole round-trip once its `MuteListModule` (+
  `MuteListService`) is enabled (both already on in this workspace's
  `OpenSim.ini`); its SQLite mute-delete works cleanly (no data-layer bug,
  unlike `picks-classifieds`' classified delete). With the module absent the
  simulator's default handler answers a read with an empty list but drops the
  write, so the entry never appears — the case detects that (the add never
  surfaces), records the write as exercised, best-effort cleans up, and marks
  `partial` rather than failing. Green on OpenSim: baseline 0 mutes, entry
  muted (type agent, default flags) then unmuted, `mute_rtt` ≈ 1.0 s /
  `unmute_rtt` ≈ 1.1 s (both poll-interval-bound, not the sub-poll server
  latency). `[both]`; the aditi run — where Second Life serves it natively — is
  deferred with the batch (no aditi record this session).

## Phase 8 — Objects & scene graph `[both]`

Most cases need a rezzed (and some a scripted) object — see appendix for the
OAR / XEngine setup.

- [x] `object-update-decode` — receive and decode the object-update stream;
  count primitives. `1av`. After the region handshake the simulator streams the
  agent's interest list — full `ObjectUpdate`s, `ObjectUpdateCompressed`, and
  `ObjectUpdateCached` digests (whose cache misses this client resolves with a
  `RequestMultipleObjects`, so the full update — and its
  [`Event::ObjectAdded`] — follows a round trip later). The case observes that
  stream for a 20 s window and tallies the first sighting of every region-local
  id ([`Event::ObjectAdded`]) by `PCode`: primitives, avatars, and other
  (trees/grass/…), deduplicated by id. This is the first case that must be
  **co-located with a fixed in-world object**, which the login default of
  `"last"` cannot guarantee — so it introduces a general
  `GridTest::start_location(grid)` hook (default `"last"`, threaded through
  `context::login`/`connect_and_spawn`/`relogin` and stored on the `Session` so
  a relogin lands the same place). This case forces the OpenSim **Default
  Region** (`uri:Default Region&128&128&30`), which holds this workspace's
  rezzed test object, and keeps `"last"` on Second Life (a named OpenSim region
  is meaningless there). Needs a rezzed object in that region (appendix); the
  scripted prim left in Default Region by Phase 9's #8 setup serves. No new
  client code — the `Object`/`pcode`/`ObjectAdded` surface all already existed;
  only the harness login hook plus the new case. Green on OpenSim: 1 primitive
  (the test object) + 1 avatar (self) decoded, `first_object` ≈ 1 ms. On SL the
  landing region's contents are uncontrolled — zero primitives in the window is
  recorded `partial` rather than failed. `[both]`; the aditi run is deferred
  with the batch (no aditi record this session).
- [x] `object-properties` — request properties and properties-family. `1av`.
  The two ways a viewer learns an object's administrative facts, exercised back
  to back against one primitive: the selection-based full path
  ([`Command::RequestObjectProperties`] → `ObjectSelect` → the full
  [`Event::ObjectProperties`] with creator/last-owner/perm-block/task-serial/
  texture-ids) and the selection-free condensed path
  ([`Command::RequestObjectPropertiesFamily`] with no request flags → the hover
  summary [`Event::ObjectPropertiesFamily`]). The case first watches the same
  interest-list stream `object-update-decode` decodes for a primitive, issues
  both requests for it, then `DeselectObjects` to leave the scene as found. It
  asserts the two replies describe the *same* object consistently (identical
  `object_id`, owner, group, sale type, name, and base permission mask) — the
  cross-check that both decode paths agree — plus a non-empty name proving a
  real object rather than a placeholder. Reuses the `start_location` hook to
  force the OpenSim Default Region (no primitive there fails; on SL the
  uncontrolled landing region records `partial`). No new client code — the
  `RequestObjectProperties*`/`ObjectProperties`/`ObjectPropertiesFamily` surface
  all existed; only the new case. Green on OpenSim against the Phase 9 scripted
  prim (`SLClientSoundTester`): both replies matched, RTT ≈ 30 ms. `[both]`;
  the aditi run is deferred with the batch (no aditi record this session).
- [x] `object-touch-grab` — touch and grab/degrab an object. `1av`. The
  two ways a viewer physically interacts with a prim, both exercised against
  one primitive: a **touch** (left-click) via [`Command::TouchObject`] (an
  `ObjectGrab` immediately followed by an `ObjectDeGrab`, the click that fires a
  script's `touch_start`/`touch_end`), and a full **press-drag-release** —
  [`Command::GrabObject`] → [`Command::GrabObjectUpdate`] (keyed by the
  persistent object id, not the region-local id) → [`Command::DegrabObject`].
  All four are unacknowledged at the application layer — the simulator sends no
  reply a viewer waits on (any visible effect is a *script's* reaction, which a
  stock prim need not have) — so, like `draw-distance`'s unreliable
  `AgentUpdate`, "no error" is read from the circuit staying healthy: a
  keep-alive ping still round-tripping after the interaction. The messages are
  reliable, so a failure to encode or enqueue any of them propagates from `send`
  and fails the case first. Reuses the object-find and `start_location`
  machinery of `object-properties`/`object-update-decode` (Default Region on
  OpenSim; a no-primitive region fails there and records `partial` on SL). No
  new client code — the
  `TouchObject`/`GrabObject`/`GrabObjectUpdate`/`DegrabObject` surface all
  existed; only the new case. Green on OpenSim against the Phase 9 scripted
  prim: touch + grab cycle sent, ping RTT ≈ 0.6 ms loopback. `[both]`; the aditi
  run is deferred with the batch (no aditi record this session).
- [x] `object-rez-derez` — rez from inventory, then derez/delete. `1av`.
  The full object lifecycle, each leg confirmed by an object-update event:
  **create** a throwaway cube with [`Command::RezObject`] (`ObjectAdd`, the
  build-tool new-prim path) placed a metre above a reference primitive in
  the region ([`Event::ObjectAdded`] with a region-local id not seen during
  the initial scene settle); **take** it into the agent's Objects folder
  with [`Command::DerezObjects`] /
  [`DeRezDestination::TakeIntoAgentInventory`], which removes the world
  object and materialises the inventory item
  ([`Event::InventoryItemCreated`]); **rez that item from inventory** with
  [`Command::RezObjectFromInventory`] (a second [`Event::ObjectAdded`] with
  a fresh id — the operation the roadmap item names); and **delete** it to
  the Trash with [`Command::DerezObjects`] / [`DeRezDestination::Trash`],
  confirmed by the [`Event::ObjectRemoved`] (`KillObject`), leaving the
  scene as found. This is the first case to construct a
  [`RezObjectParams`] / [`RestoreItem`], so it re-exports both from the two
  runtime crates (as commit `d41e378` did for `ObjectPropertiesFamily`); no
  other new client code — the whole rez/derez surface already existed.
  Force-deleting via `ObjectDelete` ([`Command::DeleteObjects`]) is a no-op
  on stock OpenSim, so the portable delete is the derez-to-Trash; OpenSim
  resolves the caller's own Trash for a `Delete` derez regardless of the
  destination id and looks the source item up by id alone for the rez (the
  payload's permission masks and CRC are not validated), so the round trip
  is self-contained on the local grid. Reuses the `start_location` hook
  (Default Region on OpenSim, where the workspace's rezzed test object is
  the placement reference; a no-primitive landing region records `partial`
  on SL). The take leaves the created item in the Objects folder and the
  final delete a copy in Trash — bounded inventory residue of two items per
  run, acceptable on a throwaway grid. Green on OpenSim: create / take /
  rez / delete all confirmed against distinct created vs rezzed objects,
  RTTs ≈ 15–90 ms. `[both]`; the aditi run is deferred with the batch (no
  aditi record this session).
- [x] `object-link-delink` — link and delink a set. `1av`. Both halves of
  the build-tool link operation, against a self-manufactured set so the round
  trip is self-contained: **create** three throwaway cubes with
  [`Command::RezObject`] (`ObjectAdd`) spaced above a reference primitive —
  one root plus two children, the smallest genuine *set* (as opposed to a
  two-prim pair) — each an [`Event::ObjectAdded`] with a region-local id not
  seen during the initial scene settle; **link** them into one linkset with
  [`Command::LinkObjects`] (root id first, `ObjectLink`), verified by each
  child re-broadcasting as an [`Event::ObjectUpdated`] whose
  [`Object::parent_id`] now points at the root; **delink** with
  [`Command::DelinkObjects`] (`ObjectDelink`), verified by each former child
  re-broadcasting with its parent back to zero; then **clean up** by derezing
  the whole set to Trash ([`Command::DerezObjects`] /
  [`DeRezDestination::Trash`]), each confirmed by an [`Event::ObjectRemoved`]
  (`KillObject`), leaving the scene as found. OpenSim's `ObjectLink` handler
  links only same-owner prims and needs no prior selection, so the fresh
  same-owner set links cleanly; the child's local id is preserved across the
  link (only its `ParentID` changes, no `KillObject`), which is what makes the
  re-parenting observable as an update rather than a remove/re-add. Reuses the
  `start_location` hook and the settle/reference machinery of
  `object-rez-derez` (Default Region on OpenSim, where the workspace's rezzed
  test object is the placement reference; a no-primitive landing region records
  `partial` on SL). No new client code — the
  `LinkObjects`/`DelinkObjects`/`RezObject`/`DerezObjects` surface all existed;
  only the new case. Green on OpenSim: create / link / delink / delete all
  confirmed, link and delink RTTs ≈ 90 ms. The Trash cleanup leaves three
  items per run — bounded inventory residue, acceptable on a throwaway grid.
  `[both]`; the aditi run is deferred with the batch (no aditi record this
  session).
- [x] `object-edit` — set name / desc / flags / shape / material /
  permissions / for-sale. `1av`. The whole build-tool edit surface exercised
  against one self-manufactured cube (as `object-rez-derez` does), each change
  confirmed by the reply that carries it back. The **administrative** edits land
  in the object's extended properties, so they are read back with one
  [`Command::RequestObjectProperties`] at the end: rename
  ([`Command::SetObjectName`]), re-describe ([`Command::SetObjectDescription`]),
  toggle the next-owner *copy* bit ([`Command::SetObjectPermissions`]), and put
  it up for sale as a priced copy ([`Command::SetObjectForSale`]). The
  **geometric / physical** edits re-broadcast the object on the interest-list
  stream, so each is confirmed by an [`Event::ObjectUpdated`] carrying the new
  value: metal material ([`Command::SetObjectMaterial`] → [`Object::material`]),
  phantom ([`Command::SetObjectFlags`] → the `FLAGS_PHANTOM` bit of
  [`Object::update_flags`]), and a hollowed box ([`Command::SetObjectShape`] →
  [`Object::shape`]). Baseline properties are read first so each edit is proven
  a real *change*: the permission edit flips the next-owner copy bit away from
  whatever the grid defaults it to (OpenSim starts a new prim at move+transfer,
  copy clear; Second Life at full). Two client fixes fell out of live testing:
  the `ObjectName`/`ObjectDescription` (and `ObjectImage` media-URL) encoders
  were sending the variable string field without its trailing NUL, so OpenSim
  dropped the last character — now NUL-terminated like every other string
  field; and `Command::SetObjectPermissions` /
  `Session::set_object_permissions` now take a typed [`Permissions`] mask
  instead of a raw `u32` (re-exported from both runtime crates). Reuses the
  `start_location` hook and the settle/reference machinery of `object-rez-derez`
  (Default Region on OpenSim, where the workspace's rezzed test object is the
  placement reference; a no-primitive landing region records `partial` on SL).
  Green on OpenSim: all seven edits applied and confirmed, next-owner copy
  `0x82000` → `0x8a000`, sale 25 L$ (copy), material / flags / shape RTTs ≈
  30–90 ms. The Trash cleanup leaves one item per run — bounded inventory
  residue, acceptable on a throwaway grid. `[both]`; the aditi run is deferred
  with the batch (no aditi record this session).
- [x] `task-inventory` — request/update a prim's task inventory. `1av`.
  A prim carries its own inventory (the scripts / notecards / sounds / objects
  a build drops into it); a viewer learns its contents by requesting them
  ([`Command::RequestTaskInventory`]) and receives an
  [`Event::TaskInventoryReply`] carrying the current *contents serial* (bumped
  on every change, so a client can tell whether a cached listing is stale) plus
  the temporary Xfer filename to download the full listing from — the serial
  alone is enough to observe a write landing. Rather than depend on a
  pre-populated prim the case manufactures a self-contained fixture like
  `object-rez-derez`: it rezs a throwaway **container** cube
  ([`Command::RezObject`]) and a second **donor** cube which it takes into the
  Objects folder ([`Command::DerezObjects`] /
  [`DeRezDestination::TakeIntoAgentInventory`]) to materialise an agent
  inventory item; requests the container's task inventory (serial `0`, empty
  filename — a fresh cube is empty); drops the taken item in with
  [`Command::UpdateTaskInventory`] / [`TaskInventoryKey::Item`] (OpenSim
  resolves the item by id from the agent's own inventory and copies it in);
  requests again and asserts the serial advanced (`0` → `1`) and the filename is
  now non-empty; then derezes the container to Trash ([`Event::ObjectRemoved`])
  to leave the scene as found. The two replies are correlated to the container
  by their task id so a stray reply is skipped. No new client code — the
  `RequestTaskInventory`/`UpdateTaskInventory` surface all existed; this case
  only re-exports [`TaskInventoryKey`] / [`TaskInventoryReply`] from the two
  runtime crates (as commit `d41e378` did for `ObjectPropertiesFamily`), plus
  `ObjectKey` which was missing from both runtime re-export lists. Reuses the
  `start_location` hook and the settle/reference machinery of `object-rez-derez`
  (Default Region on OpenSim, where the workspace's rezzed test object is the
  placement reference; a no-primitive landing region records `partial` on SL).
  Green on OpenSim: serial `0` → `1`, request and update RTTs ≈ 15 ms. The take
  leaves the donor item in the Objects folder and the container's copy of it
  goes to Trash with the container — bounded inventory residue, acceptable on a
  throwaway grid. `[both]`; the aditi run is deferred with the batch (no aditi
  record this session).

## Phase 9 — Scripting & permissions `[both]`

Needs XEngine + a scripted-object OAR (appendix). Note SL enforces god-bit;
OpenSim may not.

- [x] `script-dialog` — receive a `ScriptDialog`, reply. `1av`. A
  script raises a menu on an avatar's viewer with `llDialog` (or a free-text
  prompt with `llTextBox`): the simulator sends a `ScriptDialog` naming the
  object, its owner, the prompt text, the button labels and a hidden negative
  chat channel, and the avatar answers by chatting the chosen button's label on
  that channel —
  a `ScriptDialogReply` the script hears on its `llListen`. The case exercises
  both edges: it waits for the dialog the Default Region's scripted test prim
  (`SLClientScriptTester`, the Phase-8 #8 dialog fixture — `llDialog` on channel
  `-4242` with `Yes`/`No` buttons, fired on a 4 s timer) raises on login,
  asserts the parse (a hidden channel, at least one button), then answers it
  with [`Command::ReplyScriptDialog`] choosing the first button (an `llTextBox`
  prompt would carry typed text in place of a real label). The reply carries no
  application-level acknowledgement — the only observer of a `ScriptDialogReply`
  is the script's own `llListen`, whose reaction a stock prim need not expose —
  so, like `object-touch-grab`, "no error" is read from the circuit staying
  healthy: a keep-alive ping still round-tripping after the reply is enqueued
  (the reliable reply's encode/enqueue failure would propagate from `send`
  first). No new client code — the [`ScriptDialog`](Event::ScriptDialog) event
  and [`ReplyScriptDialog`](Command::ReplyScriptDialog) command
  surface all existed (verified end-to-end in Phase 8's #8 setup); only the new
  case. On OpenSim the avatar is forced into the "Default Region" whose test
  prim guarantees a dialog (its absence fails the case); the fixture prim is
  wiped by any non-merge OAR load, so restoring it is a
  `load oar --merge slclient8.oar` — the ARCHIVER starts its script on load, no
  restart needed (memory `sl-client-opensim-scripted-object-testing`). On Second
  Life no scripted
  object menus this avatar, so a window with no dialog records `partial` rather
  than failed. Green on OpenSim: dialog on channel `-4242`, 2 buttons, reply
  RTT ≈ 0.5 ms ping. `[both]`; the aditi run is deferred with the batch (no
  aditi record this session).
- [x] `script-permissions` — request/grant/revoke a script permission. `1av`.
  A script asks the agent for LSL permissions with `llRequestPermissions`: the
  simulator sends a `ScriptQuestion` naming the holding object, its script item,
  the owner and the requested `PERMISSION_*` bitfield; the agent answers with a
  `ScriptAnswerYes` granting a subset (empty = explicit deny) and may later
  withdraw with `RevokePermissions`. `sl-proto` keeps a **local mirror** of what
  the agent answered — never a security boundary — readable through
  [`Command::QueryScriptPermissions`], which the runtime answers by synthesizing
  an [`Event::ScriptPermissionState`] snapshot (no wire traffic). The case
  exercises all three edges against the Default Region's `SLClientScriptTester`
  prim (the Phase-8 #8 fixture, which calls `llRequestPermissions(av,
  PERMISSION_DEBIT)` on a 4 s timer): it waits for the request, asserts the
  parse (a holder, a script item, `DEBIT` in the requested set), grants exactly
  that subset with [`Command::AnswerScriptPermissions`], queries the mirror and
  asserts the grant is recorded (`Granted`, not `Denied`, carrying `DEBIT`),
  then revokes with [`Command::RevokeScriptPermissions`] and queries once more.
  The revoke is faithful to the documented mirror policy: `RevokePermissions`
  puts the full bitfield on the wire, but the mirror only *follows* the
  animation bits (`TRIGGER_ANIMATION`/`OVERRIDE_ANIMATIONS`) — every other
  permission, `DEBIT` among them, the simulator keeps enforcing, so the
  conservative mirror leaves the grant in place; the assertion records that
  server-enforced behaviour rather than expecting a local clear.
  `RevokePermissions` carries no application-level acknowledgement, so — as with
  `script-dialog` — the circuit staying healthy (a keep-alive ping still
  round-tripping) is read as "no error". No new client code — the
  [`ScriptPermissionRequest`](Event::ScriptPermissionRequest) event and the
  answer/query/revoke commands all existed (verified end-to-end in Phase 8's #8
  setup); only the new case. On OpenSim the avatar is forced into the "Default
  Region" whose test prim guarantees a request (its absence fails the case); the
  fixture prim is wiped by any non-merge OAR load, so restoring it is a
  `load oar --merge slclient8.oar` followed by a restart (the `scripts show`
  console count is XEngine-only and reads 0 for the YEngine fixture —
  "Initialized N script instances" on restart is the real signal; memory
  `sl-client-opensim-scripted-object-testing`). On Second Life no scripted
  object requests permissions from this avatar, so a window with no request
  records `partial` rather than failed. Green on OpenSim: `DEBIT` requested and
  granted, mirror keeps it across the revoke, request RTT ≈ 4.9 s (the timer),
  reply ping ≈ 3.6 ms. `[both]`; the aditi run is deferred with the batch (no
  aditi record this session).
- [x] `script-running` — query/toggle script running, reset. `1av`.
  The three commands that drive a script *after* it compiles: the viewer's
  object-Contents "Running" checkbox reads the state with `GetScriptRunning`
  ([`Command::RequestScriptRunning`] → [`Event::ScriptRunning`]) and writes it
  with `SetScriptRunning`, and the "Reset" button re-initialises it with
  `ScriptReset`. Only the *get* draws a reply — set/reset are fire-and-forget —
  so every mutation is verified by a follow-up query. Rather than borrow a
  fixture prim, the case owns a script it can freely toggle: it rezzes a
  container cube and creates a **new script directly in it** with
  [`Command::RezScript`] + [`RestoreItem::new_script`] (the object-Contents "New
  Script"). OpenSim's `RezNewScript` fills a default body **and starts it**, so
  the script is running the moment it appears in the task inventory. The case
  then: queries → asserts **running** (auto-start); `SetScriptRunning(false)`
  → queries → asserts **stopped**; `SetScriptRunning(true)` → queries → asserts
  **running** again; `ResetScript` → queries → asserts still **running** and the
  circuit healthy (a keep-alive ping still round-trips — the reset leaves a
  running script running and carries no reply, so "no error" is read the way
  `script-dialog` / `script-permissions` read their reply-less commands). Each
  query polls across the engine's asynchronous compile/start/stop:
  `GetScriptRunning` returns *nothing* while the engine has no live instance yet
  (still compiling), so a per-attempt timeout re-queries rather than failing.
  **Surfaced & fixed a real client gap** (behavioural, in `sl-proto` so both
  runtimes get it): OpenSim answers `GetScriptRunning` over the **CAPS event
  queue** (`ScriptRunningReply`, `{ Script: [ { ObjectID, ItemID, Running, Mono
  } ] }`) whenever the region has an event queue — its default, and modern SL —
  rather than the UDP `ScriptRunningReply` the client parsed, so no reply
  reached [`Event::ScriptRunning`]; `handle_caps_event` now decodes that event
  (helper `script_running_from_caps_llsd`, regression test
  `script_running_reply_caps_surfaces_run_state`). The case lives in a prim's
  *task* inventory, reached through the same `RezScript` task-write Second Life
  silently drops (the open investigation tracked with `script-upload` in Phase
  Z), so `[opensim]` only — the SL variant defers with the task-inventory batch.
  No other new client surface (the query/set/reset commands and the event all
  existed). Green on OpenSim: create+query ≈ 69 ms, stop ≈ 100 ms, start
  ≈ 100 ms, reset ping ≈ 0.5 ms loopback. `[opensim]` only.
- [x] `script-upload` — create a script, upload source, read the compile. `1av`.
      **OpenSim green; SL run deferred to Phase Z** (see the SL-task-write note
      there). Editing a script's source is not a plain asset upload: the viewer
      never compiles LSL/Lua locally — it POSTs the raw source to the
      `UpdateScriptAgent` (agent inventory) or `UpdateScriptTask` (task
      inventory) capability with a requested compile `target`
      (`mono`/`lsl2`/`luau`), and the **simulator compiles synchronously**,
      returning a `compiled` flag and an `errors` array; a script can upload as
      an asset yet fail to compile. The case uses the **task-inventory** path
      (only a task upload compiles on OpenSim — its agent path just stores the
      asset): rez a throwaway cube, create a script **directly in it** with
      [`RezScript`](Command::RezScript) + [`RestoreItem::new_script`] (the
      viewer's object-Contents "New Script" — a null-id/null-asset item the sim
      fills with a default body), fetch the listing for the task item id, then
      [`UploadScript`](Command::UploadScript) **valid** source (asserts
      `compiled == true`, no errors) and **invalid** source (asserts
      `compiled == false`, a non-empty error list, and that the first
      [`ScriptCompileError`] parsed a `line`/`column` — the payoff of the
      structured parse). Green on OpenSim: valid compiles, invalid → 3 errors,
      real XEngine format `(4,20) Error: …` parsed to line 4 col 20. New surface
      (all wired through both runtimes + REPL): `ScriptTarget`
      (`#[non_exhaustive]`, `luau` confirmed from LL viewer source),
      `ScriptLanguage` (the item-flags subtype), `ScriptUploadLocation`,
      `ScriptCompileError`, `Command::UploadScript`/`CreateScript`,
      `Event::ScriptUploaded`, the `UpdateScriptTask` cap, and the parent-aware
      CRC helpers `RestoreItem::for_task_drop`/`new_script`; scripts are removed
      from the generic upload commands at the type level (`UpdatableAssetType`)
      so the compile-blind path can't touch them.

## Phase 10 — Parcel & land `[both]`

Edits need the estate-owner avatar.

- [x] `parcel-properties` — request parcel properties (note the CAPS
  EventQueue path on SL vs UDP). `1av`. A single request/reply: send a UDP
  `ParcelPropertiesRequest` ([`Command::RequestParcelProperties`]) for a 4×4 m
  square at the region centre (128, 128) with a distinctive sequence id, then
  await the [`Event::ParcelProperties`] whose *echoed* sequence id matches — so
  the reply is our query's answer, not an unsolicited on-entry one. The reply
  does **not** come back over UDP on a modern region: OpenSim (whenever the
  region has an event queue, its default) and Second Life both enqueue
  `ParcelProperties` on the **CAPS EventQueue**, decoded by the runtime's
  event-queue task via `parcel_info_from_llsd` into the event — the UDP
  request is only the trigger, the UDP `ParcelProperties` message is
  deprecated. So this also exercises the CAPS decode path, not a plain UDP
  round-trip. The query rectangle is region-relative and independent of the
  avatar's exact position, so no `start_location` override is needed. Asserts
  the reply carries real data (`request_result` ≠ `NoData`) with a positive
  area. No new client code — the
  `RequestParcelProperties`/`ParcelProperties`/`ParcelInfo` surface (and its
  CAPS decode) all already existed from the sl-survey parcel work; only the new
  case. Green on OpenSim against the Default Region's single region-wide parcel
  ("Your Parcel", `local_id` 1, area 65536, max_prims / sim_wide_max_prims
  15000), RTT ≈ 48 ms over the event queue. `[both]`; the aditi run is deferred
  with the batch (no aditi record this session).
- [x] `parcel-info-dwell` — parcel info and dwell. `1av`. Exercises the two
  distinct "tell me about this parcel" request/reply pairs against the
  region-centre parcel. First learn the parcel's *region-local* id from a
  `ParcelPropertiesRequest` reply (as in `parcel-properties`), then: (1) request
  its **dwell** with a UDP `ParcelDwellRequest`
  ([`Command::RequestParcelDwell`]) keyed on a [`ScopedParcelId`] — the
  region-local id paired with the **root circuit id** — and await the matching
  [`Event::ParcelDwell`]; and (2) fetch the condensed **info listing** by
  resolving the region-centre location to a *grid-wide* parcel id through the
  `RemoteParcelRequest` **capability** ([`Command::RequestRemoteParcelId`] →
  [`Event::RemoteParcelId`]), then feeding that id to a UDP `ParcelInfoRequest`
  ([`Command::RequestParcelInfo`]) and awaiting the [`Event::ParcelDetails`]
  whose echoed id matches. Asserts the dwell reply echoes the requested
  region-local id, the resolved grid-wide id is non-nil, and the info listing
  carries a region name. New harness plumbing: the runtime's `Client` now
  exposes `root_circuit_id()` (mirroring the existing `region_handle()`
  accessor) and the conformance `Session` seeds/exposes `circuit_id()` from it,
  so a case can build the [`ScopedParcelId`] the scoped parcel commands
  take — infrastructure the later Phase 10 scoped-parcel cases
  (`parcel-access-list`, `parcel-object-owners`, `parcel-divide-join`) reuse.
  Also re-exported `ParcelDetails`/`ParcelKey` from `sl-client-tokio` (both
  appear in public `Event` variants but were missing from the re-export).
  Green on OpenSim's Default Region: the region-wide parcel ("Your Parcel",
  `local_id` 1, area 65536) answers all three requests, dwell tracked by the
  default `DefaultDwellModule`, `RemoteParcelRequest` cap and the two UDP
  replies all RTT ≈ 0.5–1.1 s. `dwell_parcel_id` == `parcel_id` on OpenSim
  (both the FakeID), but the case does not assert that cross-grid. `[both]`;
  the aditi run is deferred with the batch.
- [x] `parcel-access-list` — read and update the access list. `1av`. A
  read-modify-verify-restore cycle on the region-centre parcel's **allow**
  (AL_ACCESS) list, run as the estate-owner avatar (`--avatar estate-owner`),
  who owns the parcel — the first case to use the estate-owner credentials
  label. Learns the parcel's region-local id from a `ParcelPropertiesRequest`
  reply (and asserts the owner is the logged-in avatar), reads both the allow
  and ban lists ([`Command::RequestParcelAccessList`] →
  [`Event::ParcelAccessList`] per [`ParcelAccessScope`]), adds a known other
  avatar to the allow list ([`Command::UpdateParcelAccessList`]), re-reads to
  assert it landed, then restores the list to its original entries and re-reads
  to assert the entry is gone. Surfaced and fixed **two client issues** the
  round-trip needs: (1) `ParcelAccessListUpdate` hard-coded a nil transaction
  id, so the reference simulator (OpenSim `LandObject.UpdateAccessList`) only
  clears-before-adds on the *first* update per list and *appends* thereafter —
  the runtime now mints a fresh transaction id per update
  (`Session::update_parcel_access_list` gained a `transaction_id` param, wired
  through both `sl-client-tokio` and `sl-client-bevy`); and (2) an empty list
  comes back as a single nil-agent placeholder block, which the decode now
  drops (as the reference viewer's `LLParcel::unpackAccessEntries` does), so an
  empty list surfaces as zero entries. Green on OpenSim's Default Region
  ("Your Parcel", `local_id` 1) owned by the estate owner: empty allow/ban
  lists initially, the add leaves one entry, the restore clears back to empty,
  read/update RTT ≈ 4–90 ms / 15 ms. `[both]`; the aditi run is deferred with
  the batch (needs `other_avatar` in `fixtures.aditi.toml`).
- [x] `modify-land` — raise/lower terrain, then undo. `1av`. Runs as the
  estate-owner avatar (`--avatar estate-owner`), who owns the region-wide parcel
  and so has terraform rights; on OpenSim it forces a login at the region centre
  so the avatar is within terrain-streaming range of the edited patch. A
  terraform edit is a `ModifyLand` ([`Command::ModifyLand`]) brush stroke — a
  [`LandEdit`] bundling a [`LandBrushAction`], a [`LandBrushSize`], a strength,
  and the region-local ground rectangle ([`TerraformArea`]); the viewer sends a
  zero-area rectangle at the cursor ([`TerraformArea::point`]) for a click-drag
  brush, whose cos-falloff sphere moves the very centre cell by the full
  strength. There is no reply for a terraform edit — the confirmation is the
  simulator re-broadcasting the affected `LayerData` terrain patch
  ([`Event::TerrainPatch`]) with the new heights; the region centre (128, 128)
  is patch (8, 8) cell (0, 0), exactly the brush peak. Flow: learn the
  region-centre parcel's local id (and confirm we own it) from a
  `ParcelPropertiesRequest`; advertise a `Throttle` so the sim streams terrain
  and drain the login terrain flood; raise the centre and read the raised height
  `H1` off the re-broadcast patch; send `UndoLand` ([`Command::UndoLand`]) and
  watch for the patch to drop back to the baseline `H0`; assert `H1 - H0` is a
  real rise. New client code: only the `LandEdit`/`LandBrushAction`/
  `LandBrushSize`/`TerraformArea` re-exports from `sl-client-tokio` (the
  `ModifyLand`/`UndoLand` command surface and terrain-patch decode all already
  existed) — same re-export gap as `d41e378`. **Green on OpenSim's Default
  Region as partial:** the raise is verified (baseline 24.95 m → 28.02 m, delta
  3.06 m for a 3 m brush; DCT quantisation adds the extra), but stock OpenSim's
  `UndoLand` is a **no-op** (the terrain module's `client_OnLandUndo` is an
  empty stub), so the undo re-broadcasts nothing and the wait times out; the
  case restores the terrain with a `Revert` brush (reverts toward the baked
  heightmap) and reads back the baseline, and marks the run **partial** — undo
  restoration is only assertable on a grid that honours `UndoLand`. Either way
  the region is left as found. `[both]`; the aditi run (which can assert the
  undo restore) is deferred with the batch.
- [x] `parcel-divide-join` — divide then join parcels. `1av`. Runs as the
  estate-owner avatar (`--avatar estate-owner`), who owns the region-wide parcel
  on the local grid, since `ParcelDivide`/`ParcelJoin` need land-divide/join
  rights. A divide-verify-join-verify cycle that leaves the region with exactly
  the single parcel it started with: `ParcelDivide`
  ([`Command::DivideParcel`]) chops a metre `west/south/east/north` rectangle —
  a strict subsection of one parcel — out into a brand-new parcel;
  `ParcelJoin` ([`Command::JoinParcels`]) merges every owned parcel within a
  rectangle back into the largest (survivor). Neither has a direct reply, so
  the case reads the reshaped layout back with `ParcelPropertiesRequest`
  queries (as in `parcel-properties`, each with a distinct echoed sequence id).
  Flow: (0) defensively join the whole region to a single-parcel baseline —
  a no-op if already single, and it heals any parcels a prior interrupted run
  left behind; (1) learn the region-centre parcel's local id, owner (confirm we
  own it), and area `A0`; (2) divide out the SW 64×64 m corner, then assert a
  point inside the corner now resolves to a **new** parcel id whose area is the
  corner's (4096 m²), the region centre still resolves to the **original** id
  with a reduced area, and the two areas sum back to `A0`; (3) join the whole
  region, then assert the region centre is the original id with `A0` restored
  and the corner now resolves to that same id (a single parcel again). No new
  client code — the `ParcelDivide`/`ParcelJoin` command surface all existed;
  only the new case. **Green on OpenSim's Default Region:** single region-wide
  parcel (`local_id` 1, area 65536, owned by the estate owner), corner divides
  out as `local_id` 4 area 4096 leaving 61440, join restores the full 65536
  under `local_id` 1. A fixed ~2 s settle after each edit is needed: the edit
  has no reply and the readback otherwise races the simulator applying it (a
  no-settle readback saw the pre-divide layout). `[both]`; the aditi run is
  deferred with the batch — but note it likely needs a **full owned region**
  we do not have on aditi (the fixed SW-corner chop assumes we own the region
  origin), so the aditi leg may be infeasible without a suitable owned parcel
  and dynamic coordinates, unlike the other Phase 10 land cases.
- [x] `parcel-object-owners` — request object owners / return objects.
  `1av`. Runs as the estate-owner avatar (`--avatar estate-owner`), who owns the
  region-wide parcel on the local grid, since both the object-owners request and
  the object return need land rights. Exercises the two halves of the land
  panel's "Objects" tab as a self-contained rez-tally-return-tally cycle that
  leaves the region as found: (0) learn the region-centre parcel's local id and
  owner from a `ParcelPropertiesRequest` reply (confirm we own it) and build a
  [`ScopedParcelId`]; (1) request the per-owner tally with
  `ParcelObjectOwnersRequest` ([`Command::RequestParcelObjectOwners`] →
  [`Event::ParcelObjectOwners`], a `ParcelObjectOwnersReply` over UDP whose rows
  are [`ParcelObjectOwner`]s) as a **baseline**, asserting we own no objects on
  the parcel yet — the return returns objects *by owner*, so a clean baseline
  guarantees the cycle touches only this case's throwaway object; (2) rez a
  throwaway cube ([`Command::RezObject`], `ObjectAdd`) at the region centre,
  identified as the first [`Event::ObjectAdded`] with an id not seen while the
  initial scene settled; (3) re-request the tally and assert our owner now reads
  one prim higher; (4) return our parcel objects with `ParcelReturnObjects`
  ([`Command::ReturnParcelObjects`], `ParcelReturnType::LIST` scoped to our
  owner id — mirroring the viewer's "Return objects owned by \<owner\>" button,
  whose owner ids the reference `LandObject.ReturnLandObjects` matches against
  `primsOverMe`), confirmed by the [`Event::ObjectRemoved`] (`KillObject`) for
  the cube's id; (5) re-request the tally a final time and assert our owner is
  back to the baseline. New client code: only the `ParcelObjectOwner` re-export
  from both `sl-client-tokio` and `sl-client-bevy` (it appears in the public
  `Event::ParcelObjectOwners` variant but was missing from the re-exports — same
  gap the earlier Phase 10 cases closed); the `RequestParcelObjectOwners`/
  `ReturnParcelObjects` command surface and the `ParcelObjectOwnersReply` decode
  all already existed. **Green on OpenSim's
  Default Region:** the estate owner starts with no objects on the region-wide
  parcel (`local_id` 1), the cube tallies as one prim (owner count 0 → 1), and
  the return removes exactly that cube (owner count 1 → 0), leaving it in the
  estate owner's Lost and Found. A ~2 s settle after each edit lets the
  simulator update its per-parcel tally before the readback. `[both]`; the aditi
  run is deferred with the batch — but like `parcel-divide-join` it likely needs
  a **full owned region** (the fixed region-centre rez assumes we own the
  region), so the aditi leg may be infeasible without a suitable owned parcel
  and dynamic coordinates.

## Phase 11 — Region, estate & map `[both]`

- [x] `simulator-features` — request simulator features. `1av`.
  **Green on OpenSim.** The runtime already fetches the `SimulatorFeatures`
  capability automatically on region arrival (surfacing
  [`Event::SimulatorFeatures`]); the case additionally drives it on demand with
  `Command::RequestSimulatorFeatures` and asserts a decodable reply arrives
  carrying at least one advertised feature, plus (OpenSim only) the
  `OpenSimExtras` subtree that Second Life omits. Records the reply latency, the
  count of advertised top-level fields (12 on this grid), and the mesh-upload /
  physics-materials flags and the max-attachment/texture limits. **Surfaced and
  fixed a client parser bug:** OpenSim encodes `ExportSupported` inside
  `OpenSimExtras` as an LLSD **string** `"true"` (its
  `SimulatorFeaturesModule.GetGridExtraFeatures` stores every grid-wide extra as
  a string, and the `GridService` default for the key is the literal `"true"`),
  whereas the Second Life-style path sends a boolean — the strict `field_bool`
  decode rejected the whole reply (`field ExportSupported carried malformed
  value "string"`). `map_export_supported` now accepts either encoding
  (boolean/integer or a case-insensitive `"true"`/`"false"` string, matching
  OpenSim's own `bool.TryParse`), with unit tests for the string, `false`, and
  garbage cases. No other new client code — the whole command/event/CAPS surface
  already existed. `[both]`; the aditi run is deferred with the batch (SL sends
  a boolean, so the parser fix is not needed there, but the case exercises the
  same flow).
- [x] `environment` — request environment settings. `1av`. **Green on OpenSim.**
      Drives the Extended Environment (EEP) `ExtEnvironment` capability with
      `Command::RequestEnvironment` (`parcel_id: None`, the whole region) and
      asserts a decodable `Event::Environment` reply arrives describing a
      non-degenerate day cycle: a positive `day_length` and at least one named
      sky/water frame (an empty frame set would mean the capability answered but
      decoded to nothing). Both grids serve a region default when no custom
      environment is set, so the invariant holds with no world setup — OpenSim's
      `EnvironmentModule.GetExtEnvironmentSettings` returns its built-in
      `WLDaycycle` (recorded here: `day_length=14400`, `day_offset=57600`, 8 sky
      frames across 4 altitude tracks + 1 water frame). Records the reply
      latency, day length/offset, reported `env_version`, and the sky/water
      frame and sky-track counts. **No new client code** — the
      `Command`/`Event`/`ExtEnvironment` CAPS surface and the
      `environment_from_llsd` parser already existed; only the runtime crates
      gained a re-export of `EnvironmentSettings` (present in both
      `sl-client-tokio` and `sl-client-bevy` for parity). `[both]`; the aditi
      run is deferred with the batch (SL serves its regional default over the
      same path).
- [x] `open-region-info` — OpenRegionInfo limits bag. `[opensim] 1av`.
  **Partial on OpenSim (module not loaded).** `OpenRegionInfo` is an
  OpenSim-specific CAPS event-queue push (Firestorm
  `llpanelopenregionsettings.cpp`, `/message/OpenRegionInfo`): a bag of
  per-region overrides beyond the standard SL protocol — prim/link/scale
  limits, build bounds, the say/shout/whisper chat ranges, a UTC offset. It is
  **unsolicited**, so the case waits for [`Event::OpenRegionInfo`] after region
  arrival rather than issuing a command, and (when present) asserts the bag
  advertises at least one limit, recording the advertised-limit count plus the
  link/group/prim-scale and chat-range values. Every field is optional (the sim
  sends only the keys it overrides), so an empty push decodes to all-`None`.
  The push only appears when the optional `OpenRegionSettings` region module is
  loaded; the local standalone OpenSim does not ship it (absent from the source
  tree and the `bin/` module set), and Second Life never sends the event at all,
  so no live grid available here emits it. The case therefore marks the run
  **partial** with a note when the window elapses with no push (mirroring
  `library-tree-fetch` and the other optional-config cases); the decode path
  itself is covered by `sl-proto`'s `open_region_info_from_llsd` unit tests.
  **No new client code** — the CAPS event, the `OpenRegionInfo` type, and the
  parser already existed; only the runtime crates gained a re-export of
  `OpenRegionInfo` (added to both `sl-client-tokio` and `sl-client-bevy` for
  parity).
- [x] `estate-info` — request estate info / covenant. `1av` (estate owner).
  **Green on OpenSim.** Runs as the **estate-owner** avatar
  (`--avatar estate-owner`): OpenSim gates `EstateOwnerMessage`/`getinfo` behind
  `CanIssueEstateCommand`, so a non-manager gets *no* reply — a reply at all
  proves the rights. The case drives two round-trips over the estate channel:
  [`Command::RequestEstateInfo`] (`getinfo`) → an `estateupdateinfo`
  [`Event::EstateInfo`] (name/owner/id/flags/sun/parent/covenant-id+timestamp
  /abuse email) trailed by one `setaccess` [`Event::EstateAccessList`] per list
  (managers, allowed agents, allowed groups, bans — OpenSim emits one *even when
  empty*, via `SendEstateList`'s `do…while`); and
  [`Command::RequestEstateCovenant`] (`EstateCovenantRequest`) → an
  `EstateCovenantReply` [`Event::EstateCovenant`] (covenant notecard id
  +timestamp, estate name, owner). Asserts a non-empty estate name and that
  **both** replies agree the estate owner is the logged-in avatar; the trailing
  access lists are drained to a quiet gap and their count / total membership
  recorded (contents are the next case's job). Records both reply latencies plus
  the estate id (`101`), flags, parent estate, covenant presence, and the
  access-list count (`4`, all empty on the local grid). **No new client code** —
  the `Command`/`Event`/session surface (`request_estate_info`,
  `request_estate_covenant`, `estate_info_from_params`,
  `estate_access_from_params`) already existed; only the runtime crates gained a
  re-export of `EstateCovenant` (added to both `sl-client-tokio` and
  `sl-client-bevy` for parity). `[both]`; the aditi run is deferred with the
  batch (SL answers the same `getinfo`/covenant round-trips to an estate
  manager/owner).
- [x] `estate-access` — update estate access list. `1av` (estate owner).
  **Green on OpenSim.** Runs as the **estate-owner** avatar
  (`--avatar estate-owner`): editing an estate list needs estate-owner/god
  rights, which OpenSim rechecks (`IsEstateManager`/`CanIssueEstateCommand`) on
  every `estateaccessdelta`. A read-modify-verify-restore cycle over *two* lists
  that leaves the estate as it found it: read the current lists (`getinfo`,
  [`Command::RequestEstateInfo`]) and record the allowed-agents/banned-agents
  membership; then add a known **other** avatar to the allowed-agents list
  ([`Command::UpdateEstateAccess`] with [`EstateAccessDelta::AllowedAgentAdd`]),
  assert it lands in the `setaccess` [`Event::EstateAccessList`] reply, remove
  it and assert the list is back to its start size; repeat the add/remove
  round-trip against the banned-agents list. The target is never the estate
  owner (OpenSim short-circuits `_user == EstateOwner`, so the case asserts they
  differ up front); it need not be online (the lists are pure id sets) and the
  ban round-trip has no eject side effect because the target is not in the
  region. Two wire subtleties shaped the drain: OpenSim **defers** the
  `setaccess` replies, flushing only once its delta queue drains (~500 ms
  batch), and an allowed/banned change replies with *both* the allowed list and
  the ban list together — so after each delta the case drains every
  [`Event::EstateAccessList`] to a quiet gap and takes the **latest** membership
  per [`EstateAccessKind`], rather than matching the first event of a kind
  (which could be a stale reply from the previous step). Records the
  read/allowed/banned latencies, estate id+name, the target id, and the initial
  vs after-add counts (`0`→`1` for each list on the local grid). **No new client
  code** — the `Command`/`Event`/`Session` surface (`update_estate_access`,
  `EstateAccessDelta`, `EstateAccessKind`, `estate_access_from_params`) already
  existed; the case reuses `fixtures::opensim_secondary_avatar` (`Friend
  Tester`) as the target, mirroring `parcel-access-list`. `[both]`; the aditi
  run is deferred with the batch (SL enforces the same estate-owner gating and
  `estateaccessdelta` flow).
- [x] `map-blocks-items` — map blocks/items/layer. `1av`. **Green on OpenSim.**
      Drives all three world-map UDP round-trips against the current region:
      [`Command::RequestMapBlocks`] over a small grid-coordinate rectangle
      around the agent's own region (a one-cell margin on each side, so the
      multi-cell rectangle path is exercised too) → drains the
      [`Event::MapBlock`] entries to a quiet gap and asserts the agent's own
      region is among them; [`Command::RequestMapItems`] for
      [`MapItemType::AgentLocations`] targeting the current region
      (`RegionHandle(0)`) → asserts one [`Event::MapItems`] arrives echoing the
      requested type with at least one item (OpenSim always sends a placeholder
      green dot for a lightly-populated region); and
      [`Command::RequestMapLayer`] → asserts one [`Event::MapLayers`] with at
      least one image-tile layer (OpenSim's `RequestMapLayer` always answers
      with a single built-in whole-grid tile). Records the three round-trip
      latencies, the block/item/layer counts, the agent's grid coordinates, and
      the resolved region name. On the local grid the block rectangle returns
      **4** regions (the multi-region teleport-test set), the item reply the
      single green-dot placeholder, and the layer reply OpenSim's one built-in
      tile; region `Default Region` at grid `1000,1000`. **No new client code**
      — the whole command/event surface (`request_map_blocks`,
      `request_map_items`, `request_map_layer`, `MapRegionInfo`, `MapItem`,
      `MapItemType`) already existed and `sl-survey` uses the same
      `RequestMapBlocks` path to enumerate regions. `[both]`; the aditi run is
      deferred with the batch (SL answers the same UDP requests; it may
      additionally serve the layer tile over a CAPS path, but the UDP replies
      still arrive).

## Phase 12 — Teleport (state machine) `[both]`

- [x] `teleport-local-phases` — local teleport; assert the phase sequence
  Starting → Progress → Landing → Complete. `1av`. Teleports to the centre of
  the agent's current region and collects the teleport phases the session
  surfaces until arrival, asserting the sequence opens with *Starting*
  (`TeleportStart`) and ends at a terminal phase — the intra-region
  `TeleportLocal` for the expected local case (or a `RegionChanged` handover
  tolerated for an avatar that logged in adjacent to the target). OpenSim's
  local path emits only `TeleportStart` → `TeleportLocal` (no intermediate
  `TeleportProgress` / distinct Landing frame — `SendTeleportStart` then
  `SendLocalTeleport`), which is that grid's complete local sequence; recorded
  green with `phase_sequence = "started,local"` and `progress_updates = 0`.
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

Single-avatar SL-behaviour blocker (not multi-avatar, parked here):

- [ ] **`script-upload` on aditi — SL drops the task-inventory write.** The
      `script-upload` case is green on OpenSim but gated to OpenSim only: on SL
      the task-inventory *write* never lands (the object's contents serial stays
      `0` after both [`RezScript`](Command::RezScript) and an
      [`UpdateTaskInventory`](Command::UpdateTaskInventory) drop), while **rez,
      agent-inventory create, and reads (`RequestTaskInventory`) all succeed on
      the same authenticated session**, the avatar owns the object, and the wire
      encoding matches the viewer byte-for-byte. Ruled out live on aditi:
      login/MFA (auth confirmed — objects persisted and were auto-returned 15
      min later), land permission (you may edit your own objects wherever you
      can rez them — and the Firestorm viewer
      **successfully creates a script in an object at the same spot**), the item
      checksum (ported faithfully from `LLInventoryItem::getCRC32`, with the
      object as parent — `RestoreItem::for_task_drop`/`new_script`), and object
      selection (a fired `ObjectSelect` did not help; it also never returned
      `ObjectProperties` on SL). Since the viewer works on the same parcel, it
      is a client-message difference. **Next step: packet/message capture** of
      the Firestorm viewer doing object-Contents "New Script" (rez → New Script)
      — grab the outgoing `RezScript` (and any preceding `ObjectSelect`) and
      diff the field values against ours. Leading suspects: a required preceding
      selection message, or an item-block field value. When found, flip
      `script-upload` back to `[both]` and run the aditi SL-Mono error-format
      validation (the parser + all the upload code already exist and are
      unit-tested). **`script-running` rides the same blocker:** it plants its
      toggleable script with the same `RezScript` task-write, so it is gated
      OpenSim-only too — the same viewer capture that unblocks `script-upload`
      flips both back to `[both]` (its `GetScriptRunning`/`SetScriptRunning`/
      `ScriptReset` surface, including the CAPS `ScriptRunningReply` decode, is
      already grid-agnostic).

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
