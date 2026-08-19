---
id: viewer-ui-color-customization-extras
title: Colour customization beyond the shipped colors tab
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-preferences-colors-skins-tab, viewer-name-tags-decorations,
       viewer-minimap-avatar-dots, viewer-lookat-faithful]
---

Context: [context/viewer.md](../context/viewer.md).

The user-tunable colour surface the reference exposes beyond our
shipped colors tab (done [[viewer-preferences-colors-skins-tab]] —
chat user/self/objects/system + IM, name-tag palette, distance bands,
keyword colour, minimap opacity). All of these follow the pattern
`preferences_colors_skins.rs` established: account-scope swatch rows
layered over skin-supplied defaults.

Chat text: extra source colours (friends, Lindens, muted,
script errors, object-IM, owner-say, direct, chat header, object-name
header), link and URI-query-part colours, mention highlight colours
(self / other resident), username-distinct colouring
(`FSColorUsername`), distinct IM colour in the console
(`FSColorIMsDistinctly` + console_im) and the beyond-hearing-range
diminish factor (`FSBeyondNearbyChatColorDiminishFactor`). Minimap:
pick-radius colour + alpha, self/agent/object/Linden/muted dot
colours, and per-ring whisper/chat/shout colours & toggles — the rings
exist (done [[viewer-minimap-avatar-dots]]) with fixed colours.
World effects: the look-at/selection beam colour swatch (pairs
[[viewer-lookat-faithful]]).

Panel/floater surfaces: script-dialog and group-notice colours plus
their opaque-background toggles (`ScriptDialog*` colors,
`FSScriptDialogNoTransparency`, `FSGroupNotifyNoTransparency`),
floater transparency (active/inactive/camera/conversation opacity),
console background colour+opacity, floating-text and menu background
opacity (`FSHudTextBackgroundOpacity`, `FSMenuBackgroundAlpha`),
preferences-search highlight colours, area-search beacon colour,
notecard-editor colours, and pie-menu overrides (`OverridePieColors`,
bg/selected colours, `PieMenuOpacity`, `PieMenuFade`) — our pie menu
is skinned via CSS today. Name-tag opacity/Z-offset knobs stay with
[[viewer-name-tags-decorations]].

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_colors.xml`,
`indra/newview/llfloaterpreference.cpp` (Pref.applyUIColor).
