---
id: viewer-object-mark-copy-key
title: Object marking + copy object keys
topic: viewer
status: ideas
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [viewer-area-search, viewer-object-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

Builder/moderator helpers from Firestorm: visually *mark* objects
(`FSMarkObjects`) so a set of objects stays highlighted while you work
through them (pairs with area search results), and copy selected
objects' UUIDs to the clipboard with a configurable separator
(`FSCopyObjKeySeparator`) from the context menu. Needs scoping: decide
whether marking lives in the selection layer or as a render overlay, and
where the copy-keys entry sits in our context/pie menus.

Reference (Firestorm, read-only): `FSMarkObjects`,
`FSCopyObjKeySeparator` settings; the mark/copy actions in
`fsareasearch.cpp` and the object menu.
