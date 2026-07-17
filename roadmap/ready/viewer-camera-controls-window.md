---
id: viewer-camera-controls-window
title: Camera controls window (llfloatercamera)
topic: viewer
status: ready
origin: split from the camera-system pass (the on-screen camera UI)
blocked_by: [viewer-camera-third-person-orbit, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's **Camera Controls** floater (`llfloatercamera`): the
on-screen panel that drives the camera the mouse gestures already drive — the
orbit / pan / zoom pad, the preset views (front / side / rear / mouselook), the
object-view and "presets" rows, and the flycam toggle. It sits on top of the
[[viewer-camera-third-person-orbit]] mode machine and the mouse controls,
calling the same orbit / zoom / focus operations rather than reimplementing
them.

Related, separate pieces already carved out: the camera **presets**
([[viewer-camera-presets]]), the **flycam floater**
([[viewer-camera-flycam-floater]]), and the preferences camera/move tab
([[viewer-preferences-camera-move-tab]]). This task is the primary in-world
camera control panel those extend or sit beside.

Built on the UI scaffold ([[viewer-ui-widget-scaffold]]) and the pie-menu-style
radial control the reference's orbit pad resembles.

Reference (Firestorm, read-only): `indra/newview/llfloatercamera.cpp/h`,
`indra/newview/skins/default/xui/en/floater_camera.xml`.
