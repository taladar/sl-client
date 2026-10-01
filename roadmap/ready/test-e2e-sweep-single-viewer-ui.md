---
id: test-e2e-sweep-single-viewer-ui
title: End-to-end tests for the remaining single-viewer UI checks
topic: test
status: ready
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
