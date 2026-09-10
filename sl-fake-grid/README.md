# sl-fake-grid

An in-process loopback fake Second Life / OpenSim grid built on the
workspace's sans-I/O server machinery: `sl-wire`'s login server,
`sl-proto`'s `SimSession` (the simulator-side protocol machine) and
`SimCaps` (the capability dispatch). This crate adds only the I/O glue —
an HTTP endpoint serving login and CAPS (including the `EventQueueGet`
long-poll), one loopback UDP socket per logged-in session, and scriptable
content fixtures — including the legacy UDP asset paths (named `Xfer`
files, task inventories, `TransferRequest` sources, the estate terrain RAW
heightmap) and the world burst a simulator pushes on region entry (the
agent's own avatar, the parcel overlay, the agent's parcel, the region's
ground as `LayerData` terrain/wind/cloud patches, the region's objects),
replayed on request. No world authority, no persistence: content is
whatever the scenario scripts.

Two consumers by design:

- **Integration tests**: `FakeGridBuilder` starts a grid on ephemeral
  ports inside the test process, so tests run in parallel; the returned
  handles (`FakeGrid`, `FakeAgent`) let the test drive the grid side of
  the conversation (send chat, push object updates, enqueue CAPS events)
  and assert on the `ServerEvent` stream. Both the tokio client and the
  Bevy plugin (`sl-client-bevy/tests/fake_grid_login_smoke.rs`) log into
  it in their end-to-end tests.
- **Manual viewer testing**: the `sl-fake-grid` binary serves a grid an
  unmodified viewer (this workspace's, or Firestorm's grid manager) can
  log into at `http://127.0.0.1:<port>/` — the highest-fidelity offline
  test target this workspace has short of a real grid.

Next to login and CAPS the port also serves the non-CAPS surfaces a grid
manager and the world map expect: `GET /get_grid_info` (and the XML-RPC
`get_grid_info` method on `/`), world-map tiles at
`/map-<zoom>-<x>-<y>-objects.jpg` (the login response's `map-server-url`
points back at the grid), and the economy helper scripts
`/currency.php` + `/landtool.php` for the buy-L$ / buy-land flows.

A stock grid also speaks WebRTC **voice signalling** (offer → answer, ICE
trickle, parcel channel, logout — no media plane) and advertises it the way
a Second Life region does (`voice-config`,
`SimulatorFeatures.VoiceServerType`, `RequiredVoiceVersion`). Whether a
region speaks voice at all follows the grid this one is imitating — see
below.

## Policy, not content

A few of the answers a grid gives are not content at all but *policy* —
what it charges, what it lets an agent do, what it refuses. `EconomyConfig`
carries both halves of the money policy: the L$ rate its web helper quotes
and the price list its simulator answers an `EconomyDataRequest` with, so
the two cannot disagree. `AgentPolicy` (from `AccountConfig::estate_manager`
and `FakeGridBuilder::legacy_udp_inventory`) decides whether an agent's
estate commands are answered or silently dropped the way OpenSim drops them,
and how the deprecated UDP inventory fetch is answered — served, refused
with a `FeatureDisabled`, or ignored the way Second Life ignores it.

## Neighbours, teleports and crossings

A grid with more than one region behaves like a real one about its
**neighbours**: the moment an agent is rooted, every region touching its
own (`RegionConfig::neighbours`, `NeighbourPolicy::Adjacent` by default)
is announced over the event queue, the client opens a child circuit, and
that circuit is handed the neighbour's objects, avatars and ground. This
is why the region across a border is drawn before you reach it.

`FakeGrid::teleport_agent` and `FakeGrid::cross_agent` are the two ways
an agent moves between regions, and they are deliberately different:
a teleport puts up the teleport screen, hands the client a
`TeleportFinish` and retires the source circuit; a crossing sends one
`CrossedRegion`, promotes the child circuit the client already holds
without any screen at all, and leaves the source open as a child. The
grid claims no movement authority, so a crossing is asked for rather than
noticed.

Either way a destination the agent already borders is reused rather than
opened a second time, and arriving retires the children that have dropped
out of view. `FakeGridBuilder::handover_timeout` shortens how long the
grid waits for a client to complete its movement, so a test of the
failure path need not wait out the real budget.

## Content fixtures

Assets are **grid-wide**. A `RegionFixture` states the ids its own content
references, and the builder folds every region's into one store when the
grid starts — because an asset id names a blob the whole grid knows, and a
viewer fetches every one of them over its *root* region's capability,
including the textures of the neighbour it can see across a border.

A parcel is stated once, with both its halves: the region-local record a
`ParcelProperties` reply carries, and the `ParcelListing` naming the
grid-wide id a `RemoteParcelRequest` resolves a location to and the dwell a
`ParcelDwellRequest` asks for. The search listing a `ParcelInfoRequest`
answers with is *derived* from the two, so the record and the listing cannot
disagree about the parcel they both describe.

`fixtures::PrimFixture` builds the `Object` records a region pushes:
every builder method sets a typed value and packs it into the raw wire
blob beside it (`texture_entry`, `extra_params`, `particle_system`,
`texture_anim`), which is the only form an `ObjectUpdate` carries. A
`fixtures::RegionFixture` is one region's whole content — objects,
assets, legacy materials, per-face media, environment, ground — and
`into_region` wires each piece to the surface that serves it.

`fixtures::catalogue()` is the **named catalogue**: one prim per
rendering feature (textured, sphere-shaped, per-face styled, mesh,
sculpt, PBR, legacy material, projecting light, flexi, particles,
animated texture, hover text, media, reflection probe, linkset) in a
west-to-east row north of the arrival point, with every asset it
references served. The automated tiers and the binary's `--scenario
catalogue` load the same fixture, so "the mesh prim" is the same object
with the same id in a unit test, a full-stack capture and a Firestorm
session.

`fixtures::border()` is the **border** scene: one checkered marker pillar
floating just inside the region's west edge. It exists for the questions
that need two regions — is the region across the border drawn at all, and
does it stay put when the avatar walks into it — because both are only
decidable in pixels if the subject's position is stated relative to a
border rather than to the middle of a region. `border_with_vehicle`
adds a rideable platform against the border for a *ridden* crossing, with
`BorderSide` deciding which side of the line a region is on so its ids and
its position cannot be mispaired.

`fixtures::scenarios` names the scenes — `stock`, `catalogue` and
`border` today — so a harness selects one by name and the next one is a
registry entry rather than a change to the harness. Each scene names its
landmarks (a name and a region position per thing worth aiming a camera
at).
`scripts/fake-grid.sh` starts the binary on a fixed port with a named
scenario and prints, once the grid answers `get_grid_info`, the login URI
as an IPv4 literal plus the `--grid` argument Firestorm wants.

## Scripted timelines

Everything above answers something the client asked for. A `Timeline` is
the other half: what happens to a session because **time passed**.

```rust,ignore
let timeline = Timeline::new()
    .then(At::AfterArrival(Duration::from_secs(2)),
          Action::MoveObject { local_id, to })
    .after(Duration::ZERO, Action::Marker("moved".to_owned()))
    .then(At::OnMarkerAck, Action::KillObject(local_id));
```

A step's `At` is a duration from the arrival or from the previous step, a
`ServerEvent` the session drains (`OnEvent`), or the client's own
acknowledgement of the last `Marker` (`OnMarkerAck`) — the one wait that
is a happens-before rather than a guess, because a client acknowledges a
packet it has already decoded and handled. Its `Action` is anything a
simulator does unprompted: rez, move, edit or kill an object, attach or
detach one, animate the avatar, push an appearance, chat, IM, change the
environment, save the region's own settings, change a parcel, teleport or
walk over a border, report stats or the simulator's clock, send a marker,
or a `Custom` hook.

`SetEnvironment` needs `ConfigureRegion` beside it to reach a viewer that
is already in the region: nothing carries new environment settings to one,
so the viewer re-reads `ExtEnvironment` when a `RegionInfo` arrives — which
is what `ConfigureRegion` sends, and what the reference viewer re-reads on
unconditionally.

The waits are `tokio` sleeps and the stamps come from the grid's injected
clock, so a paused-time test runs a scripted minute in no wall-clock time
at all.

A script belongs to the **avatar**, not to the region it started in: when
the client arrives in a teleport destination or across a border, the steps
that have not run yet are handed to that session and the one left behind
keeps only the prefix it ran. That happens for a client-initiated hop as
much as for a scripted one. A script that has already finished hands over
nothing, which is what leaves a destination region's own timeline alone.

## Which grid this one is

The fake grid exists to fail a viewer the way a real grid would, and there are
two real grids that do not agree. Where they differ one of them has to be
picked — and picking per behaviour produces a grid that is nobody: a stock fake
grid used to announce `platform: OpenSim`, keep every login field like OpenSim,
and withhold a taken object's asset like Second Life, all at once.

So the grid names the live one it is being, once —
`FakeGridBuilder::imitates(ImitatedGrid::OpenSim)`, defaulting to Second Life,
the grid this workspace targets — and every divergent behaviour takes its
default from that. A per-behaviour setter still wins where it is called: the
flavour is what an unset knob falls back to, not a lock.

Nine behaviours follow it today: a taken object's asset (below), whether the
login response is trimmed to the request's `options` list (Second Life honours
it, OpenSim sends every field regardless), the two that make up how a region
introduces itself (below), the two that make up how it does inventory (below),
who composites an avatar, what the grid charges, and what it says the account
is entitled to.

There is no longer a list of divergences the flavour does *not* decide: every
one this crate has measured is derived from it. `imitates.rs` keeps the audit
so a divergence taken one-sidedly in future has somewhere to be written down
rather than rediscovered.

**What the grid charges** is the price list an `EconomyDataRequest` is answered
with, measured on both grids — plus the currency symbol, where the divergence is
one of *presence*: Second Life says `L$` and a stock OpenSim grid says nothing
at all, leaving a viewer on its own default (`OS$` in Firestorm).

**What the account is entitled to** is the login response's benefits package —
`account_type`, `account_level_benefits`, `premium_packages` — and the maturity
preference beside it. Second Life sends all four; a stock OpenSim grid sends
none, which is why a modern viewer prices uploads from the package on one grid
and from the legacy `EconomyData` on the other. It matters because the package
is where **tiered** texture pricing lives: L$ 50 above 1024×1024 against L$ 10
below it on the free tier, a distinction the older reply cannot express.

## How inventory is fetched, and how a new item is announced

The loudest divergence a viewer meets, and the one it is most likely to depend
on without noticing, because both halves are silent when they are wrong.

**The fetch.** OpenSim still serves the deprecated UDP
`FetchInventoryDescendents`; Second Life dropped it when inventory moved behind
AIS3. So an OpenSim-flavoured grid answers it out of the session's own
inventory tree — the same tree `FetchInventoryDescendents2` reads, packed the
way `LLClientView` packs it: at most six folders or five items per message,
never both in one, and a nil-id placeholder block padding whatever half is
empty. A Second-Life-flavoured grid does not have the path, and of the two
answers a grid without it can give, this one takes the loud road: a
`FeatureDisabled` naming the refused feature. Aditi was measured (2026-08-12)
*ignoring* the fetch instead, and `LegacyUdpInventory::Ignored` reproduces
that — but silence is indistinguishable from a lost packet, so it is not the
default a test would have to wait out.

**The take's announcement.** A take is answered with the legacy UDP
`UpdateCreateInventoryItem` on OpenSim and with a `BulkUpdateInventory` over
the event queue on Second Life. `InventoryAnnouncement` picks between them
(`FakeGridBuilder::inventory_announcement` overrides). A client listening for
only one of the two hears nothing at all from the other grid, which is why the
conformance cases that take something use one shared helper that accepts
either and records which arrived.

**An upload's announcement is a different question, the grids swap sides on it,
and it is two answers rather than one.** After an asset saved in place over an
`Update*AgentInventory` capability, Second Life pushes the legacy
`UpdateCreateInventoryItem` and OpenSim sends *nothing at all*. After a
`NewFileAgentInventory` completion — the path that *creates* an item —
**neither grid sends anything**: that response body carries the whole new item,
so a push would repeat what the client is already holding, while a save's
response names only the new asset and the push is what stops the client's copy
of the item naming the one it replaced. So `UploadAnnouncements` is its own
knob and carries a value per path
(`FakeGridBuilder::upload_announcements`, `UploadAnnouncements::uniform` for a
grid that should treat the two alike); reusing the take's answer for it would
be wrong about a live grid in both directions at once, and reusing one upload
answer for both paths was wrong about Second Life until it was measured.

Measured 2026-09-08 by the conformance cases `notecard-create-update`
(`save_announcement`: `update-create-inventory-item` on aditi, `none` on
OpenSim) and `asset-upload` (`upload_announcement`: `none` on both).

The take and the save diverge for **opposite reasons**, which is worth knowing
before reading the save as "Second Life does something extra". The push is the
older behaviour and OpenSim is the grid that omits it: at the in-place save
`CapsUpdateInventoryItemAsset` ends on a commented-out
`SendInventoryItemCreateUpdate` — commented since 2007-08, when that capability
path was written — and answers with an `AlertMessage`, while the
`NewFileAgentInventory` completion reaches inventory through the *client-less*
`AddInventoryItem` overload. So the take is Second Life having moved on to
AIS3, and the save is OpenSim having never sent what a Linden simulator sends.
The reference viewer wants neither push: it builds the item from the response
body.

Reaching Second Life's `NewFileAgentInventory` completion at all costs money —
that grid serves the capability only for the chargeable upload classes — so
that half of the table was extrapolated from the save until 2026-09-08, and
extrapolated the wrong way. The run that settled it uploaded a 64×64 texture at
the account's own benefits price (L$ 10, charged) and recorded `none` twice.

The legacy UDP transaction save (`UpdateInventoryItem`, how a wearable is
saved) follows neither knob: its `UpdateCreateInventoryItem` is the **reply**
to a UDP request, echoing the transaction and callback ids the client sent, and
OpenSim sends it there exactly where it stays quiet after a capability upload.

## How a region introduces itself

`SimulatorFeatures` is where the two grids describe themselves, and the
flavour decides two things about it.

**`OpenSimExtras`** is sent by OpenSim unconditionally and by Second Life
never — the one structural difference that reliably tells the two replies
apart — so a Second-Life-flavoured grid omits the block
(`FakeGridBuilder::open_sim_extras` overrides). Nothing goes missing with it:
what a viewer actually reads out of the block reaches it by a second route
both grids serve, and the reference viewer reads that route first when no
extras block overrode it — the login response's `map-server-url` and
`currency`, and `get_grid_info`'s `economy` key. Dropping the block removes a
duplicate, not a surface.

**Voice** (`FakeGridBuilder::voice_backend`) is WebRTC on the Second Life
side, named three ways — `SimulatorFeatures.VoiceServerType`, the login
`voice-config`, the `RequiredVoiceVersion` push on arrival — and
`VoiceBackend::Silent` on the OpenSim one: nothing advertised, and a
`ProvisionVoiceAccountRequest` refused for want of a backend.

Silence is what a *stock* OpenSim region is. Both its voice modules
(`VivoxVoiceModule`, `FreeSwitchVoiceModule`) are optional and off by
default, and both answer with the Vivox SIP account shape — which this
workspace implements nowhere, Second Life having moved to WebRTC. So there
is no Vivox flavour to pick: a grid defaulting to one would be serving a
path nothing here speaks. Modelling the stock region is the same choice
`open_sim_prices` makes for money.

## A taken object's asset

The two live grids disagree about `AssetType::Object`, so the fake grid says
which of them it is being. Measured on aditi 2026-09-06 by the
`object-asset-format` conformance case: **Second Life gives a viewer no asset
id for an object inventory item.** Eleven of eleven object items answered with
a nil `asset_id`, in the AIS3 folder listing and again in the per-item
`GET /item/<id>`, and all eleven were full-perm to their owner — so it is not
the "no asset id unless you fully own it" rule, it is the class. OpenSim is the
opposite: every object item names an asset and `ViewerAsset` serves it as
`SceneObjectSerializer` XML.

`assets::ObjectAssetPolicy` picks a side, and it follows the grid this one is
imitating (see below), so the **default is Second Life** (`Withheld`): an object
a resident takes is filed under a nil asset id and its body goes into a store no
capability reads. That is deliberately the strict configuration — a viewer that
has come to rely on opening a taken object's asset fails against it, which is
what the fake grid is for. A grid imitating OpenSim serves it instead, where the
item names the body and the grid hands it over.

Rezzing the item back into the world works under **both**: on Second Life too
the simulator resolves the body itself, and the viewer never needs to see it.
The divergence is about what a viewer may *fetch*, not what a resident may
*do*.

One thing the switch does not govern: the seeded `Fixture Object`
(`sl_test_assets::inventory`) keeps its asset id and stays fetchable either
way. It is the fake grid's own fixture, seeded so the `asset-round-trip` case
has an authored object body to read back, and no live grid has an item like it
at all.

See the book chapter "The fake grid" for architecture and usage.
