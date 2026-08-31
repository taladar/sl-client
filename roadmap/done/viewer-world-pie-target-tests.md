---
id: viewer-world-pie-target-tests
title: Right-clicking each world target opens exactly its pie
topic: viewer
status: done
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-ui-radial-menu, viewer-ui-interaction-harness]
blocked_by: [viewer-world-test-harness]
---

Context: [context/testing.md](../context/testing.md).

All six targets landed (2026-08-31): prim → `OpenObjectMenu` (and none
over empty sky), another avatar and the own avatar → `OpenAvatarMenu`
naming the right agent, a worn attachment → `OpenAttachmentMenu` with
`hud: false`, bare land → `OpenLandMenu`, and — with the vendored
character assets satisfying `setup_hud_screen` — a HUD-Center
attachment → `OpenAttachmentMenu` with `hud: true` through the
orthographic HUD pick. Each is a real right click through the synthetic
pointer and the CPU resolver ([[viewer-cpu-pick-resolver]], done).
The rest landed 2026-08-31, and the task is done:

- **the two negatives** — a right-click on a floater parked over the prim
  opens nothing and stands no pie, while the same pixel with the panel
  closed opens exactly one (the control runs *second*, so the open pie's
  own blocking ring can never be what suppressed the first click); and a
  right *drag* — press, swing 60 px past the slop, come back, release on
  the prim it started on — opens nothing, where the plain click before it
  opened one. Both settle past a frame where any click is dropped for a
  reason of its own ([[viewer-prim-rebuild-drops-a-click]], found here);
- **the seat** decides which of the two fixed self-pie slices is live:
  a real right-click on the own body carries `self-standing` and resolves
  *Sit Down* live at north-west, and with the session's seat set — or the
  viewer's ground-sit flag, the second source the opener ORs in — carries
  `self-sitting` and resolves *Stand Up* live at west instead, neither
  slice having moved. The per-menu condition tests resolve a pie against
  conditions handed to them; this asks which conditions the *world* put
  in the request;
- **the pie itself**, opened by the world: exactly one request, carrying
  the object element, and the spawned menu's ring centres on the pixel
  that was right-clicked (off the viewport centre on purpose — a pie that
  ignored the request would pass at the centre and only there) and passes
  the whole `layout_violations` sweep the UI tier runs on every element.
  The compass click → action half landed with the harness
  (`a_pie_slice_clicked_in_world_sends_its_command`).

The menu bar's action table is pinned too, in `menu_bar.rs`: every
command in walk order under the `>`-joined path of menu labels that
reaches it, plus a check that no two entries in the whole bar declare the
same action string (the live dispatch matches on the name alone). Its
walker is `sl_viewer_ui_widgets::menu::action_paths`, promoted out of the
widget crate's test module so the fixture bar and the live one are pinned
by the same walk — the line-menu counterpart of `pie_menu::addresses`.

The four live pie address tables are pinned; what nobody tests is
*target classification under a real right click*. In the fixture world
with the CPU pick resolver: right-click on a prim, another avatar, the own
avatar, a worn attachment, bare land and a HUD attachment each drain
exactly one `OpenPieMenu` with the expected element at the cursor (seated
adds the stand-up condition); a right-click through a floater or after a
right-drag opens nothing. Then the spawned pie lays out clean
(`layout_violations` empty) and a compass click drains the declared action
— the end-to-end "right-click prim → Edit → `EditToolState.active`" check.
Pin the menu-bar action table the way the pies are, if it is not already.
