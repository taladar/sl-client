---
id: test-fake-grid-imitates-simulator-features
title: A grid claiming to be Second Life still introduces itself as OpenSim
topic: test
status: done
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 3
refs: [test-fake-grid-object-asset-id-divergence]
---

Context: [context/testing.md](../context/testing.md).

How a region describes *itself* is the third divergence `ImitatedGrid` does not
decide, and the fake grid takes OpenSim's side of all of it:

- **`OpenSimExtras`** rides in `SimulatorFeatures` unconditionally
  (`runtime.rs`), carrying the map-tile server URL and the currency helper
  base. Second Life sends no such block — a viewer discovers those elsewhere —
  so a viewer that learned to read its map server *only* out of the extras
  works here and against OpenSim, and finds nothing on Second Life.
- **Voice** is always the WebRTC stub. That half is right for Second Life and
  wrong for OpenSim, which is Vivox or nothing — so this is the one place the
  grid is accidentally Second Life while being OpenSim about everything else in
  the same message.

The awkward part is that the map and currency URLs have to keep working on a
Second-Life-flavoured grid whether or not the extras block carries them: the
fake grid *is* its own tile server and its own currency helper, and a flavour
that hid the URLs without providing the other route would break the world map
offline. So the work is to find what Second Life's viewer path actually reads
for each and serve that, not merely to drop a field.

One thing deliberately stays put: `GridIdentity::platform` remains `OpenSim`
whichever grid is being imitated. It is not protocol behaviour — it is what
Firestorm's grid manager reads to decide whether it will add the grid at all,
and a fake grid Firestorm refuses to add tests nothing. That is stated in
`imitates.rs` and should stay stated.

Acceptance: `SimulatorFeatures` and the login response describe the grid the
flavour names — no `OpenSimExtras` on the Second Life side, no WebRTC voice on
the OpenSim one — with the map and currency surfaces still reachable on both,
and a conformance case that reads them declares the flavour it expects.

## What landed

Two knobs on [`ImitatedGrid`] — `advertises_open_sim_extras` and
`voice_backend` — each overridable on the builder and each resolved once at
`start`, like the two that were already there.

**The extras block.** Gone on the Second Life side, sent on the OpenSim one.
The worry the task raised — that hiding the block hides the URLs — did not
survive reading what the reference viewer does with them: every URL in the
block that anything reads has a second route both grids serve, and
`LFSimFeatureHandler` treats the extras copy as an **override** of that route
rather than as the source. The map-tile server is in the login response's
`map-server-url` (`LLStartUp::process_login_success_response`), the currency
symbol in its `currency`, and the currency helper base in `get_grid_info`'s
`economy` key (`LLGridManager::getHelperURI`). All three were already served
unconditionally, and `http_misc.rs` already asserted all three — so the block
was a duplicate, and the pin that keeps it that way is a comment on those
assertions saying what they are now load-bearing for. The chat ranges the
block also carries are the viewer's own defaults (20/100/10), so dropping
them changes nothing a viewer sees.

**Voice: silence, not Vivox.** The task said "Vivox or nothing" for OpenSim,
and the answer is nothing. Both of OpenSim's voice modules (`VivoxVoiceModule`,
`FreeSwitchVoiceModule`) are optional and **off by default**, so a stock region
speaks none — and both answer with the Vivox SIP account shape, which this
workspace implements nowhere: Second Life removed Vivox for WebRTC, and OpenSim
support for a leaf feature like voice is not a priority. A Vivox flavour would
have meant a fixture serving a path nothing here will ever speak, so there is
no third variant. Modelling the stock region is also the choice `stock_prices`
already makes for money.

That collapses what first looked like two knobs into one. The OpenSim column is
a single decision — no backend — and every advertisement falls away on its own:
no `SimulatorFeatures.VoiceServerType`, no login `voice-config`, no arrival
`RequiredVoiceVersion`, and a `ProvisionVoiceAccountRequest` that refuses itself
with `BackendUnavailable`. The first cut of this had a second
`names_voice_backend` knob, on the theory that OpenSim-with-a-voice-module still
names nothing (true — neither string appears anywhere in its sources); with no
Vivox side to run, that knob decided nothing observable and went.

Worth keeping in the docs anyway, because it is *why* OpenSim never needed the
field: a viewer told nothing falls back to Vivox by itself
(`LLVoiceClient::handleSimulatorFeaturesReceived`), then finds no capability on
a silent region and gives up — which is what a viewer meets on a stock OpenSim
grid today.

**Where the backend is chosen** moved: `default_setup` used to `enable_webrtc`
directly, which made a scenario decide a grid-wide question. Now the scenario
files only the parcel's voice *channel* — where voice happens is scene fixture
— and the runtime installs the backend afterwards, into a session whose voice
store is still empty. A scenario that enables one itself still wins.

**What it caught.** Nothing, and that is worth recording: the client reads
neither `voice_config` nor `currency`, so the "ask for everything read back"
rule had nothing to say here. The two fake-grid tests that broke
(`client_end_to_end`, `fake_grid_login_smoke`) both broke by asserting the
OpenSim shape on a Second-Life-flavoured grid — the tests were describing
the grid the fake one used to be, which is exactly the drift
[[test-fake-grid-imitates-audit]] set out to make visible.

The OpenSim end-to-end case runs its own client loop rather than going through
the `start_*` helpers, because its claim is about an event that must **not**
arrive and a wait that stepped over the arrival burst on the way to the
handshake would have eaten the push it is looking for. The login half is
asserted over raw HTTP in `http_glue.rs` instead: it is the *absence* of a
section, and on a grid that ignores the request's `options` an absent
`voice-config` is the grid's answer rather than the option filter's.

`simulator-features` became the second case to declare **both** flavours (the
first was `object-asset-format`), and holds each to the grid it says it is —
extras present and no `VoiceServerType` on the OpenSim side, the reverse on
the Second Life one. It records `voice_server_type` rather than pinning
which backend Second Life names, because that is a thing the grid has
changed once already. `Grid::behaves_like` is the helper the assertion asks
through: unlike `Grid::imitates` it answers for the live grids too, because
aditi is not a grid without a flavour, it *is* the flavour.

`GridIdentity::platform` still stays `OpenSim` either way, as the task asked.
