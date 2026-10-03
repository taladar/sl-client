---
id: gridspec-survey
title: Survey — tasks to measure aditi and OpenSim and make the fake grid be each
topic: gridspec
status: done
origin: user request (2026-10-03)
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-03)

Surveyed the workspace in six slices (session and world, objects and
building, social, land / estate / economy, inventory and appearance, LSL)
for, per feature: what `sl-fake-grid` does today, what is already known about
aditi and OpenSim (the `ImitatedGrid` table, conformance cases and done
files, the book), what is still unmeasured, how to measure it with the
automation, which tasks would implement it, and what our viewer must handle.
The method every task follows is in the context file; the book gained a
*Grid Behaviour* part (`book/src/gridspec/`) for the measurements.

Findings that shaped the tasks:

- **Most cases written for aditi never ran there** ("deferred with the
  batch"), and records are gitignored — re-running them is the cheapest
  discovery, so the book tables become the committed record.
- **`ImitatedGrid` covers assets, login, inventory announcements, bakes,
  economy and voice only**; nothing in land, estate, objects, social or LSL
  varies by flavour yet, and the fake grid's catch-all arms drop most social,
  land and inventory-mutation traffic.
- **Groups, profiles and the mute list have no server-side protocol** at all —
  new `protocol-sim-*` work precedes their fake-grid stores.
- **LSL probing on aditi** can likely become self-service through a carrier
  object and `UpdateScriptTask`, sidestepping the `RezScript` drop; no probe
  is committed today.
- Stale items noticed: [[server-fake-grid-object-undo]] (undo exists since
  2026-09-05 — the remaining question is per-flavour scope),
  [[server-lsl-lib-time-timers]]' "a state change cancels the timer" (aditi
  says it survives), [[server-fake-grid-script-compile-on-upload]]'s
  failed-compile assumption (unmeasured). Each is noted in its discovery task.

**Infrastructure:**

- [[gridspec-infra-fake-flavour-conformance]] — Run every conformance case
  against both fake-grid flavours, beside the live grids
- [[gridspec-aditi-test-land]] — Decide how we get land and estate rights on
  aditi for the land, estate and terraform measurements

**Feature discovery** (small fake-grid gaps implemented inside):

- [[gridspec-login]] — Login response fields, the options list and get_grid_info
  on each grid
- [[gridspec-login-refusals]] — Login refusal codes and texts, MFA, TOS/critical
  and presence on each grid
- [[gridspec-logout]] — Logout reply, timing and what logout does to seats and
  child circuits
- [[gridspec-circuit]] — Circuit behaviour: acks, resends, pings, inactivity,
  packet quirks and throttles
- [[gridspec-region-arrival]] — Region handshake identity, the arrival burst and
  region telemetry on each grid
- [[gridspec-simulator-features]] — The full SimulatorFeatures map on each grid
- [[gridspec-teleport]] — Teleport phases, flags, failures, cancel and access
  refusals on each grid
- [[gridspec-teleport-lures]] — Teleport offers, requests and their answers on
  each grid
- [[gridspec-neighbours-crossing]] — Child agents, EnableSimulator and region
  crossing on each grid
- [[gridspec-agent-movement]] — How each grid moves an agent: AgentUpdate
  handling, speeds, terse updates
- [[gridspec-sit-stand]] — Sitting and standing: placement, refusals and alerts
  on each grid
- [[gridspec-avatar-presence]] — Other avatars in the region: full updates,
  coarse locations and kills
- [[gridspec-world-map]] — World map blocks, items, layers and tiles on each
  grid
- [[gridspec-terrain]] — Terrain, wind and cloud layers as each grid sends them
- [[gridspec-environment]] — Region and parcel environments (EEP) on each grid
- [[gridspec-object-update-stream]] — Object update forms, the interest list and
  kills on each grid
- [[gridspec-object-properties]] — Object properties and selection replies on
  each grid
- [[gridspec-object-rez-derez]] — Rez, take, take-copy, delete, return and
  auto-return on each grid
- [[gridspec-object-edit]] — Object edits on each grid: transforms, shape,
  flags, admin fields, undo, duplicate
- [[gridspec-object-link-delink]] — Link order, limits and refusals on each grid
- [[gridspec-task-inventory]] — Task inventory reads and writes on each grid
- [[gridspec-touch-grab]] — Touch and grab on each grid
- [[gridspec-object-media]] — Media on a prim: ObjectMedia, navigation and
  propagation on each grid
- [[gridspec-materials]] — Legacy and PBR material edits and overrides on each
  grid
- [[gridspec-asset-fetch-http]] — HTTP asset fetch details on each grid
- [[gridspec-asset-upload]] — Asset uploads and their costs, refusals and
  announcements on each grid
- [[gridspec-viewer-effects-sounds]] — Viewer effects and sound relays on each
  grid
- [[gridspec-local-chat]] — Local chat and typing on each grid
- [[gridspec-instant-messages]] — Instant messages, typing, busy replies and
  offline storage on each grid
- [[gridspec-group-chat-and-conference]] — Group chat sessions and ad-hoc
  conferences on each grid
- [[gridspec-friends-presence]] — Friendship, rights, presence notifications and
  calling cards on each grid
- [[gridspec-groups]] — Groups: membership, roles, notices, invitations and
  accounting on each grid
- [[gridspec-profiles]] — Profiles, picks, classifieds, notes and display names
  on each grid
- [[gridspec-search-directory]] — Directory search and the avatar picker on each
  grid
- [[gridspec-mute-list]] — Mute (block) list storage and enforcement on each
  grid
- [[gridspec-voice]] — Voice provisioning and signalling on each grid
- [[gridspec-experiences]] — Experience records, permissions and admin on each
  grid
- [[gridspec-parcel-properties]] — ParcelProperties transport, fields and pushes
  on each grid
- [[gridspec-parcel-access-and-ban-lines]] — Parcel access and ban lists,
  enforcement and ban lines on each grid
- [[gridspec-parcel-management]] — Parcel edits, divide/join, object owners and
  return on each grid
- [[gridspec-land-transactions]] — Buying, selling, deeding, abandoning land and
  passes on each grid
- [[gridspec-parcel-info-dwell]] — Parcel info, dwell and the remote parcel id
  on each grid
- [[gridspec-estate]] — Estate info, access, covenant and estate actions on each
  grid
- [[gridspec-region-info]] — RegionInfo, region flags and limits on each grid
- [[gridspec-terrain-editing]] — Terraforming, raw terrain transfer and terrain
  textures on each grid
- [[gridspec-god-tools]] — God tools as OpenSim answers them
- [[gridspec-money]] — Balance, L$ transfers, pay dialogs and buying objects on
  each grid
- [[gridspec-marketplace]] — Marketplace direct delivery on each grid
- [[gridspec-landmarks-home]] — Landmarks and the home location on each grid
- [[gridspec-inventory-fetch]] — Inventory skeleton, fetch caps, AIS3, the
  library and cache versions on each grid
- [[gridspec-inventory-mutations]] — Inventory item and folder operations and
  their pushes on each grid
- [[gridspec-inventory-offers]] — Giving inventory, accepting and declining, and
  object gives on each grid
- [[gridspec-notecards-gestures]] — Notecards, embedded items and gestures on
  each grid
- [[gridspec-animations]] — Animations as each grid broadcasts them
- [[gridspec-outfits-wearables]] — The Current Outfit Folder and wearables on
  each grid
- [[gridspec-appearance-baking]] — Appearance and baking on each grid
- [[gridspec-attachments]] — Attachments on each grid: attach, detach, limits,
  HUDs, temporary
- [[gridspec-avatar-render-info]] — AvatarRenderInfo and AttachmentResources on
  each grid

**LSL** — infrastructure:

- [[gridspec-lsl-aditi-script-carrier]] — Put scripts into prims on aditi
  through a carrier object and UpdateScriptTask
- [[gridspec-lsl-probe-corpus]] — A committed LSL probe corpus and per-grid
  result files
- [[gridspec-lsl-live-differential-runner]] — Run one probe on OpenSim YEngine,
  XEngine and aditi and collect the transcripts

**LSL** — per area (each blocks the matching `server-lsl-*` / `server-world-*`
implementation):

- [[gridspec-lsl-compile]] — Script compilation and compile errors on each grid
- [[gridspec-lsl-events]] — The LSL event model on OpenSim, beside what aditi
  already showed
- [[gridspec-lsl-time-timers]] — LSL time, timers and sleep on each grid
- [[gridspec-lsl-memory-limits]] — LSL memory and limits on OpenSim and the
  remaining SL unknowns
- [[gridspec-lsl-runtime-errors]] — LSL run-time errors on OpenSim and the debug
  channel in our viewer
- [[gridspec-lsl-comms]] — LSL library behaviour on each grid — chat, listens,
  dialogs and link messages
- [[gridspec-lsl-prim-state]] — LSL library behaviour on each grid — reading and
  writing the prim
- [[gridspec-lsl-detection-sensors]] — LSL library behaviour on each grid —
  sensors and raycasts
- [[gridspec-lsl-avatar-control]] — LSL library behaviour on each grid —
  animations, sitting, controls and camera
- [[gridspec-lsl-money-permissions]] — LSL library behaviour on each grid —
  script permissions, money and payment
- [[gridspec-lsl-task-inventory]] — LSL library behaviour on each grid — task
  inventory, giving, rezzing and notecards
- [[gridspec-lsl-land-region-env]] — LSL library behaviour on each grid —
  parcel, region, estate and environment queries
- [[gridspec-lsl-physics-vehicles]] — LSL library behaviour on each grid —
  status flags, forces, targets and vehicles
- [[gridspec-lsl-http-url]] — LSL library behaviour on each grid — outbound HTTP
  and in-world URLs
- [[gridspec-lsl-json-linkset-data]] — LSL library behaviour on each grid —
  llJson* and the linkset data store
- [[gridspec-lsl-strings-math]] — LSL library behaviour on each grid — strings,
  lists, maths and rotations
- [[gridspec-lsl-experience]] — LSL library behaviour on each grid — experiences
  and the key-value store
- [[gridspec-lsl-persistence]] — Script state across take, rez and region
  restart on each grid
- [[gridspec-lsl-throttles]] — Forced delays and throttles on each grid
- [[gridspec-lsl-ossl]] — OSSL availability and denial on OpenSim

**New implementation tasks** for the large gaps, each blocked by its
discovery:

- [[server-fake-grid-object-update-forms]] — Fake grid — send compressed, terse
  and cached object updates as each grid does
- [[server-fake-grid-edit-permission-enforcement]] — Fake grid — refuse edits
  the editor may not make, as each grid refuses them
- [[server-fake-grid-object-media-region]] — Fake grid — region-wide object
  media with version bumps and propagation
- [[server-fake-grid-material-edits]] — Fake grid — apply legacy and PBR
  material edits and push overrides
- [[server-fake-grid-mesh-upload-cost]] — Fake grid — mesh upload costing and
  refusals
- [[protocol-sim-group-messages]] — SimSession — decode group requests and send
  group replies
- [[server-fake-grid-groups]] — Fake grid — a group store: membership, roles,
  notices, invitations and group sessions
- [[protocol-sim-profile-messages]] — SimSession — decode profile requests and
  send profile replies
- [[server-fake-grid-profiles]] — Fake grid — profiles, picks, classifieds and
  notes with each grid's quirks
- [[server-fake-grid-friends-presence]] — Fake grid — a mutable friendship store
  with rights, presence fan-out and calling cards
- [[server-fake-grid-directory-search]] — Fake grid — answer directory searches
  from an index of accounts, parcels and fixtures
- [[server-fake-grid-mute-list]] — Fake grid — mute list storage, the Xfer file
  and each grid's empty-list reply
- [[server-fake-grid-parcel-access-enforcement]] — Fake grid — enforce parcel
  access and push ban lines
- [[server-fake-grid-parcel-divide-join]] — Fake grid — divide and join parcels
- [[server-fake-grid-estate-actions]] — Fake grid — kick, eject, freeze,
  teleport home, estate messages and restarts
- [[server-fake-grid-terraform]] — Fake grid — terraforming, undo and bake
- [[server-fake-grid-god-tools]] — Fake grid — god tools as OpenSim answers them
- [[server-fake-grid-money-ledger]] — Fake grid — a money ledger: balances,
  transfers, buying and paying
- [[server-fake-grid-inventory-udp-mutations]] — Fake grid — the UDP inventory
  mutations, Trash gating and pushes
- [[server-fake-grid-inventory-offers]] — Fake grid — inventory offers, filing
  and object gives
- [[protocol-avatar-render-info]] — AvatarRenderInfo — client request, SimCaps
  service and fake-grid answers

Existing implementation tasks that are now blocked by a discovery task:
[[fake-grid-own-attachments-and-region-moves]], [[server-agent-transfer]],
[[server-bake-service]], [[server-fake-grid-agent-avatars-shared]],
[[server-fake-grid-im-relay]], [[server-fake-grid-object-undo]],
[[server-fake-grid-parcel-on-movement]],
[[server-fake-grid-script-compile-on-upload]],
[[server-lsl-lib-avatar-control]], [[server-lsl-lib-comms]],
[[server-lsl-lib-detection-sensors]], [[server-lsl-lib-experience]],
[[server-lsl-lib-http-url]], [[server-lsl-lib-json-linkset-data]],
[[server-lsl-lib-land-region-env]], [[server-lsl-lib-math-rotations]],
[[server-lsl-lib-money-permissions]], [[server-lsl-lib-ossl]],
[[server-lsl-lib-physics-vehicles]], [[server-lsl-lib-prim-state]],
[[server-lsl-lib-strings-lists]], [[server-lsl-lib-task-inventory]],
[[server-lsl-lib-time-timers]], [[server-lsl-memory-and-limits]],
[[server-lsl-script-persistence]], [[server-world-agent-movement]],
[[server-world-changed-raisers]], [[server-world-link-sets]],
[[server-world-sit-and-attach]], [[server-world-touch-and-grab]],
[[server-world-update-scheduling]], [[test-lsl-differential-opensim]].
