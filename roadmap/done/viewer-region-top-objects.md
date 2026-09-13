---
id: viewer-region-top-objects
title: Top objects — top scripts / top colliders
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-region-options-debug, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

The estate "Top Objects" tool: ask the region which of its objects cost it the
most — script time, or collisions — and act on what comes back. Reached from the
Region / Estate floater's Debug tab, which grew the reference's two buttons
("Get Top Colliders…" / "Get Top Scripts…").

Reference (Firestorm, read-only): `llfloatertopobjects.cpp`,
`floater_top_objects.xml`, `llfloaterregioninfo.cpp`
(`LLPanelRegionDebugInfo::onClickTopScripts`).

## Done (2026-09-13)

`sl-viewer-places/src/top_objects.rs`, plus the protocol work the feature
turned out to need.

### The reply does not come back the way the request went out

`LandStatRequest` is UDP and every viewer sends it that way, but a simulator
with an **event queue** answers over the queue, not with the
(`UDPDeprecated`) `LandStatReply` packet — OpenSim's
`LLClientView::SendLandStatReply` falls back to the packet only when the region
has no queue, which no live region does. We decoded only the packet, so the
first live check sat on "Asking the region…" forever.

`sl-proto` now decodes both forms into the one `Event::LandStatReply`
(`land_stat_reply_from_caps_llsd`) and can encode the event-queue form
(`land_stat_reply_to_caps_llsd`, `SimSession::enqueue_land_stat_reply`).
`sl-fake-grid` answers over the queue for the same reason a real grid does: a
fake grid that answered by packet would pass a viewer that fails everywhere
else.

Only the event-queue form carries the reply's **`DataExtended`** half —
parcel name, rez date, script memory, public URL count, a Mono score and (on
OpenSim) the owner id — which is four of the window's eight columns, and why
the reference guards those columns with `msg->has("DataExtended")`.
`LandStatItem` gained `extended: Option<LandStatExtended>`; a row decoded from
the packet has none.

### The score is a unit, not a number

One wire `Score` field means milliseconds in one report and a collision count
in the other, decided by the reply's `ReportType`. That decision now happens at
the codec boundary: `LandStatScore::{ScriptTime(Duration), Collisions(f32),
Other(f32)}`, built by `from_wire` and written back by `raw()` (the symmetry
test proves a row round-trips through the event queue unchanged).

The cell renders a script time in mixed units
(`sl-viewer-ui-core::ui_format::format_duration_units`, `humantime`'s style:
every non-zero unit, most significant first — `1h 2m 21s 979ms`), and a
collision count as a count. The reference prints `%0.3f` of the raw number for
both; a live region reports script time in the millions of milliseconds, which
says nothing at a glance.

**That figure is the simulator's, and on this OpenSim it is inflated.** XEngine
sums a script's execution over a rolling 30-second window
(`ScriptInstance::MeasurementWindow`), so an hour of "script time" inside a
thirty-second window is arithmetic, not load: `MetricsCollectorTime::GetSumTime`
computes `ticks * 1000 / Stopwatch.Frequency`, and a Mono tick/frequency
mismatch scales it ~100×. A viewer shows what the region says.

Script **memory** is likewise always empty on OpenSim: XEngine's
`GetTopObjectStats` sets only the local id and the time, never `memory`, so the
estate module sends `0`. An empty cell means "not reported"; a `0` would claim
the region said zero. It should fill on Second Life.

### Two windows, one per region

The reference has one `top_objects` floater that `setMode` switches between the
reports. Ours is **two window kinds** (`top-scripts`, `top-colliders`), each
**instanced per region** on the same `FloaterKey` the About Region window that
opened it uses (`about_region::region_key`, now `pub(crate)`) — so two regions
are two reports, and a subject-keyed floater persists nothing and is not
restored at start-up, which is right for something that describes a region at a
moment. A window whose region the agent has left keeps its report, greys out
every action and says so: every write goes out on the *current* circuit.

A reply names no window; it is routed to the window of its report's kind about
the region the agent is in, which is the only window that could have asked.

### The rest of the window

The reference's list (score / name / owner / location / parcel / date, plus
memory and URLs on the scripts window — the colliders one has neither, where
the reference keeps the columns and blanks their headings), its three
region-side filters (`STAT_FILTER_BY_OBJECT` / `_OWNER` / `_PARCEL_NAME`, each
a fresh request, the filter consumed by it as `onRefresh` does), the selected
object's id, and Show Beacon / Return Selected / Return All / Disable Selected
/ Disable All / Refresh.

- **Show Beacon** sets the shared tracking beacon (`MapTracking`), the
  reference's `LLTracker::trackLocation`; a double-click on a row does the same.
- **Return / Disable** send `ParcelReturnObjects` / `ParcelDisableObjects` over
  the whole region (`LocalID = -1`, `RT_NONE`) naming the task ids — the shape
  OpenSim's `ReturnObjectsInParcel` reads as "these specific objects". The two
  "all" actions ask first (`ReturnAllTopObjects` / `DisableAllTopObjects`, both
  already in the notification catalogue). The reference dropped its Disable
  buttons but kept their confirmation; stopping a runaway script without taking
  somebody's build away is the gentler half of what this window is for.
- `Session::{return,disable}_parcel_objects` now **split a long id list into
  MTU-sized batches** (`TASK_IDS_PER_REQUEST`), as the reference's
  `isSendFullFast` loop does — a top-objects return names as many ids as the
  report has rows. An empty list still sends exactly one message: "every object
  of this type" is a different request, not an empty one.

### A widget change this found

A table **heading** is now always leading-aligned, whatever its column's values
do (`ui_table::spawn_table_header_cell`). The Memory heading in an end-aligned
column was clipped at its *start* — which reads as a different word, with the
ellipsis marker at the far edge where it warns nobody. Applies to every table in
the viewer; the values keep `TableColumn::align`.

Both windows also size themselves from their own table: the default width is
every fixed column, the gaps, the row padding, the scrollbar gutter and room for
the Name column, and the **minimum** is that sum without the Name room — so a
window cannot be dragged narrower than its own header, which the table widget
cannot scroll sideways.

### Verified

Live on the local OpenSim as the estate owner, against four busy-loop scripted
prims merge-loaded into Default Region: both windows open per region from the
Debug tab, the scripts report lists four rows with score / name / owner /
location / parcel / date, sorting and selection work, the id line fills, Show
Beacon and double-click track the object, and the colliders report honestly
reports nothing (no physics collisions in the scene). The wire itself was read
back independently with `sl-repl-tokio --script`, which is how the empty memory
field and the inflated score were pinned on the simulator rather than on the
decode.

Unit-tested: the two codec directions (`sim_session_symmetry`), the MTU split
and the empty-list case (`lifecycle`), the per-kind columns, the mixed-unit
duration rendering, the sort (including rows with no extended block), the
selection re-projection across a re-sort, the per-action preconditions and the
left-region freeze, plus four scheduled-app tests: opening asks for the named
report, two reports × two regions are four windows, a reply reaches only its own
report, and leaving a region freezes its window.

## Follow-ups

- `sl-fake-grid` answers with an **empty** report. A scenario that carries a
  few rows would let the whole window (list, sorting, selection, the
  `DataExtended` columns) be exercised offline, which is what the live OpenSim
  had to stand in for here.
- A table wider than its window still has no horizontal scroll; both windows
  avoid it by flooring their width. A widget that scrolled sideways would let a
  wide report live in a narrow window.
