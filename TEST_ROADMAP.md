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
- [ ] `chat-whisper-shout-range` — verify whisper/shout reach vs normal. `2av`.
- [ ] `typing-indicator` — `set_typing` start/stop observed by the other. `2av`.

## Phase 3 — Instant messaging & chat sessions `[both]`

- [ ] `im-1to1` — send IM, peer receives; reply back. `2av`.
- [ ] `im-typing` — IM typing start/stop. `2av`.
- [ ] `group-session-message` — start a group session, send, leave. `2av`
  (needs Groups; OpenSim requires Groups V2).
- [ ] `chat-invite-accept-decline` — exercise `AcceptChatInvite` /
  `DeclineChatInvite` and the CAPS `ChatSessionRequest` path on SL. `2av`.
- [ ] `session-mark-read` — unread → mark-read transition. `1av` or `2av`.
- [ ] `offline-msg-fetch` — fetch offline IMs (CAPS `ReadOfflineMsgs` vs UDP
  `RetrieveInstantMessages`). `2av`.
- [ ] `conference-roster` — start an ad-hoc conference; verify it is distinct
  from a 1:1 (multi-party roster, `SessionAdd`/`SessionLeave`). **`3av`** —
  Aditi variant deferred (Phase Z).

## Phase 4 — Friends & presence `[both] 2av`

- [ ] `friendship-offer-accept` — offer, accept, confirm both friend lists.
- [ ] `friendship-terminate` — terminate, confirm removal.
- [ ] `presence-online-offline` — observe `OnlineNotification` /
  `OfflineNotification` as the peer logs in/out.
- [ ] `grant-user-rights` — grant see-online / map / modify rights; confirm.
- [ ] `calling-card` — offer/accept calling card. OpenSim may not surface
  offers — `mark_partial` there; full path is effectively `[aditi]`.
- All Aditi variants deferred to Phase Z.

## Phase 5 — Inventory (deep) `[both]`

- [ ] `inventory-tree-crawl` — background/full-tree fetch beyond the root;
  record folder/item totals. `1av`.
- [ ] `ais3-folder-lifecycle` — create / rename / move / remove / purge a
  folder (CAPS AIS3 on SL; gate vs UDP on OpenSim). `1av`.
- [ ] `inventory-item-ops` — create / copy / move / link an item. `1av`.
- [ ] `library-tree-fetch` — fetch the read-only Library tree. `1av`.
- [ ] `inventory-cache-skip` — refetch with matching version is skipped. `1av`.
- [ ] `give-inventory` — give an item to another avatar; peer accepts. `2av`
  (Aditi deferred).

## Phase 6 — Groups `[both]`

OpenSim requires Groups V2 enabled (see appendix).

- [ ] `group-create-activate` — create a group, activate it. `1av`.
- [ ] `group-join-leave` — join and leave. `2av`.
- [ ] `group-roster` — fetch members / roles / titles / profile. `1av`.
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
