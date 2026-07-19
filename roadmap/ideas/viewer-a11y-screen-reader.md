---
id: viewer-a11y-screen-reader
title: Screen-reader / assistive-technology support
topic: viewer
status: ideas
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

**The reference viewer has none.** Confirmed against the Firestorm source: no
`IAccessible` / UIAutomation (Windows), no `NSAccessibility` (macOS), no
AT-SPI / ATK (Linux), no `WM_GETOBJECT`. Its UI is the custom OpenGL-rendered
LLUI/XUI toolkit, which draws every control itself and never exposes an
accessibility tree, so NVDA / JAWS / VoiceOver / Orca cannot see any of it. The
only "speak"/"say" code in the viewer is **voice chat** (Vivox/WebRTC + the mic
button), which is unrelated. So this is a genuine feature gap, not something to
port.

Our viewer inherits the same starting problem: `bevy_ui` is also a self-rendered
toolkit with no OS accessibility tree by default. But the scaffold gives us a
head start the reference never had — a **content-driven, logically-structured**
widget tree ([[viewer-ui-widget-scaffold]]) with a focus model
([[viewer-input-focus-contexts]]) and real string keys
([[viewer-i18n-fluent-scaffold]]) — so an accessibility layer can be *derived*
rather than retrofitted onto pixel rects.

Direction (investigate, then scope):

- **Emit an accessibility tree.** Bevy has an AccessKit integration
  (`bevy_a11y` + `accesskit`) that bridges to each platform's AT API. Check how
  far the 0.19 `bevy_a11y` / `bevy_ui` `AccessibilityNode` support actually
  goes, and whether our custom logical-layout / focus widgets feed it correctly
  (role, name, value, focus, expanded/selected state for the tree rows, tabs,
  buttons, text fields, the pie menu).
- **Names come from the bundle, not the geometry.** Every widget's accessible
  name / description should resolve through the i18n `Translator`, so a screen
  reader speaks the localized label — the string scaffold is a prerequisite,
  not an afterthought.
- **Keyboard reachability.** The focus scaffold already makes controls
  Tab-reachable; audit that *every* actionable control is, and that focus order
  is meaningful (a screen-reader user navigates by focus).
- **In-world vs. chrome.** Scope decision: UI chrome (floaters, inventory, chat,
  menus) is the realistic target; the 3D world itself (avatars, objects,
  spatial audio cues) is a much larger, separate question — chat/IM/notice text
  going to the AT is the high-value near-term win.
- **Live-region announcements** for chat, IMs, notifications, and errors, so new
  text is spoken without the user hunting for it.

Interacts with [[viewer-i18n-colorblind-accessibility]] (both are accessibility;
neither should rely on a single sensory channel). A high-value, high-effort
area with essentially no prior art in this ecosystem.

Reference (Firestorm, read-only): none — the gap *is* the finding. AccessKit /
`bevy_a11y` docs are the real reference.
