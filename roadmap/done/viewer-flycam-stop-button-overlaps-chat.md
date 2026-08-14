---
id: viewer-flycam-stop-button-overlaps-chat
title: "Stop flycam" button overlaps the "Chat" button in the bottom bar
topic: viewer
status: bugs
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

In the viewer bottom bar, the **Stand / Stop-flycam button** (added with the
seated-placement / sit-camera work) overlaps the **Chat** button rather than
laying out beside it. The two controls draw on top of each other.

Likely a layout issue in the bottom-bar row: the conditionally-shown
Stand/Stop-flycam button is not participating in the flex layout the way the
fixed buttons are (absolute placement, or inserted without reserving width), so
it lands over the neighbouring Chat button. Fix the bottom-bar row so the
button takes its own slot and pushes/leaves room for the others.
