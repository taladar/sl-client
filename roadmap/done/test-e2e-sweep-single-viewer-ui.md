---
id: test-e2e-sweep-single-viewer-ui
title: End-to-end tests for the remaining single-viewer UI checks
topic: test
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 8
refs: [test-e2e-live-verify-sweep]
---

Context: [context/automation.md](../context/automation.md).

What one viewer on the fake grid can show, left after the first batch:

- [[viewer-multiline-field-fills-its-floater]]: resize the notecard and
  script windows and the text body's bounds follow.
- [[viewer-build-material-tab-permission-gate]]: the material tab greys on
  a prim the agent may not modify.
- [[viewer-region-experiences-default-experience]]: the default experience
  row in About Region.
- [[viewer-audit-picker-requester-identity]]: two About Region windows on
  two regions adding to the same experience list; the ✕ on an editor with
  unsaved changes.
- [[viewer-camera-reset-on-distant-teleport]]: flycam away, teleport to a
  distant region, the camera is back behind the avatar; a double-click
  teleport within the region keeps the framing.
- [[viewer-arrival-orientation-snap]]: after a teleport the avatar faces
  the way it was sent on its first frame, and a walked crossing turns
  nothing.
- [[viewer-skin-scrollbar-shape]]: a right-to-left row that overflows to
  the left still scrolls.
- [[viewer-audit-cef-browser-settings-hardening]]: the search tab and the
  web window still load a page (needs `web_media` on the stage viewer).
- [[viewer-inventory-clothing-layers-shirt-icon]]: a mixed outfit in the
  fake grid's inventory shows each type's icon.
- [[viewer-preferences-colors-skins-tab]]: a skin flip re-dresses the UI and
  un-overridden swatches follow; a chat-self pick recolours the overlay and
  the Nearby transcript (Cancel / OK); the account's `settings.toml` gains
  only the overridden colours and a row's Reset returns to the skin value;
  `--skin` overrides without rewriting the stored skin; `--watch-skins`
  picks up an edited sheet.
- [[viewer-preferences-debug-settings-editor]]: a row click fills the detail
  pane, an edit takes effect in-world (ShowPropertyLines), a per-scope reset,
  copy-name reaches a paste, and an edit made in Preferences refreshes the
  open editor.
- [[viewer-about-landmark-floater]]: About Landmark from an inventory
  landmark fills every row, shows a parcel's snapshot, copies its SLURL, and
  commits a title / notes edit.
- The About window's simulator-version line across a teleport
  ([[viewer-about-floater]]): the fake grid gives every region one
  channel version, so a region config field for it comes first.

## Done (2026-10-01)

Fifteen stage tests over three files, each on both backends:

- `tests/e2e_windows.rs` — the notecard and script bodies grow with their
  resized window; the material controls grey on the stock box (somebody
  else's) and are live on an own prim, in both material modes; About
  Region's Key list carries the estate default with no Remove, its name cell
  has room, and the Allowed picker leaves it out while offering the land
  experience beside it; two About Region windows on two regions get a
  Blocked picker each, a pick in the second is posted once and lands only
  there, and each picker closes with its window; a sky editor with a moved
  knob asks on ✕ (No keeps it, Yes closes it); every clothing layer shows its
  own glyph; About Landmark from an inventory landmark fills its rows, loads
  the parcel's snapshot, copies the SLURL and commits a title and a notes
  edit; the About window's simulator line names each region's simulator
  across a teleport.
- `tests/e2e_arrival.rs` — a flycam flown off is back in third person behind
  the avatar after a distant teleport, and a teleport within the region
  leaves it exactly where it was; a grid teleport facing north faces north on
  the first frame the agent is there and stays so, and a crossing whose
  motion points east turns nothing.
- `tests/e2e_preferences.rs` — a skin flip re-dresses the window and moves the
  un-overridden swatches; a chat-self pick recolours the overlay and the
  Nearby transcript, Cancel takes it back, OK keeps it, `settings.toml`
  gains `ChatColorSelf` alone and the row's Reset clears it; `--skin vintage`
  is worn while Preferences shows the stored skin and nothing stores it; the
  debug-settings editor fills its detail pane, an edit unticks World ▸
  Property Lines and records a Global override, Reset clears that scope,
  Copy Name reaches a paste and a Preferences edit shows in the open editor;
  in Arabic the search window's overflowing right-to-left tab strip scrolls
  to its last tab and back; with web media on, the web browser and the
  search window's web tab load a page from a loopback server.

What it took: a held key (`RequestBody::Press::hold_frames`, the driver's
`hold`, `sl-viewer-ctl press --hold`); the camera's eye in the agent readout;
colours in the model (a colour well's `NodeValue::Color`, a text node's
`UiNode::color`, both fed to AccessKit); a `Document` role, with the browser
view named by its page's title and valued by its address (`SemanticValue`);
stage options `estate_manager`, `web_media` and `skin`, and an in-process
viewer finding `sl-cef-helper` above cargo's `deps/`; the fake grid's
per-region simulator version and its `class_folder`. `sl-cef-helper` moved
from `sl-cef` into the viewer's package (user decision): the commit hook's
test run builds only the crates a change touches, so it had no helper and
web media was off there, as in any viewer build that skipped `-p sl-cef`.

Fixed on the way:

- a viewer with web media on exited on a `SIGTERM` without logging out:
  Chromium's start-up installs its own shutdown handler over the viewer's
  (`disable_signal_handlers` does not stop it), so `sl-cef` now saves the
  `SIGTERM` / `SIGINT` actions before CEF starts and restores them after;
- a teleport never re-sent the simulator version, so the About window kept
  the region left behind (`commit_handover` now emits it, as the login's
  root circuit does);
- the inventory search found nothing in a folder nobody had opened — the
  window pages a folder only on expand; a query now asks for every unpaged
  folder, as the reference's filter starts its background fetch;
- About Region's experience lists squeezed the name column to an ellipsis
  (an Experiences cell now has room for its two fixed columns and a name, so
  two lists share a row and the window opens 70 px taller to show the
  third);
- the semantic model named a list row after the first button inside it
  ("Profile"), not its cells;
- the grid's echo of one's own nearby chat was coloured as another
  avatar's in the transcript;
- a world aim that had revealed its target failed at the first look while
  the camera was still easing into the framing (seen at ten frames a second
  in a viewer process); it now looks again until the deadline;
- the fake grid left a map teleport's avatar at height 0 under the ground,
  and called a Moderate experience by a code (34) that reads as Adult;
- the stage's teardown called a session that was on its way out stranded
  (a closed session stays in the grid's table a moment; only an open one is
  stranded, and it is looked for once — a wait would let the grid's
  inactivity timer close a killed viewer's session and hide it).

Not done here, and why:

- **A double-click teleport within the region** — the camera half is
  checked with a grid-side teleport within the region (the same
  `TeleportLocal` arrival); the driver has no way to aim at bare ground, so
  the double-click itself is [[viewer-automation-ground-aim]].
- **`--watch-skins` picking up an edited sheet** — the stage cannot point a
  viewer at a copy of the asset tree to edit;
  [[test-e2e-skin-sheet-watch]].
- **The debug-settings edit "in-world"** is read from World ▸ Property
  Lines' tick, which follows the same setting: the parcel line bands have no
  node in either model.
