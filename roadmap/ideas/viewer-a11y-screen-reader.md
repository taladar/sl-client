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
accessibility tree, so NVDA / JAWS / VoiceOver / Orca cannot see any of it.
The only "speak"/"say" code in the viewer is **voice chat** (Vivox/WebRTC +
the mic button), which is unrelated. So this is a genuine feature gap, not
something to port.

Researched 2026-07-22; the sections below are the plan-of-record when this
is picked up. Every prerequisite the original note listed is now done: the
widget scaffold ([[viewer-ui-widget-scaffold]], `ui.rs`), the focus model
([[viewer-input-focus-contexts]], `input_context.rs` — single source of
truth in `bevy_input_focus::InputFocus`), and the Fluent `Translator`
([[viewer-i18n-fluent-scaffold]], `i18n.rs`). Better yet, the stack is
already running: the viewer's `DefaultPlugins` include `AccessibilityPlugin`
(`bevy_a11y` 0.19.0 / `accesskit` 0.24.1 / `accesskit_winit` 0.32.2 sit in
the lockfile today), but nothing emits `AccessibilityNode`s yet — greenfield
wiring, not a retrofit. No shipped Bevy app with screen-reader support is
known anywhere; this would be first-of-its-kind in the ecosystem.

## Platform reality (mid-2026)

One AccessKit tree serves all three desktop platforms; the adapters differ
in maturity and in what they need from us:

- **Windows (NVDA / JAWS / Narrator).** All three consume UI Automation;
  AccessKit's Windows adapter is a UIA provider and `accesskit_winit` wires
  `WM_GETOBJECT` automatically — no MSAA / IAccessible2 work. Oldest, most
  mature adapter. UIA marshals AT calls onto the thread that owns the
  window, so a blocked main thread stalls the screen reader: keep per-frame
  tree updates cheap.
- **macOS (VoiceOver).** NSAccessibility; the adapter subclasses winit's
  NSView and is at rough parity with Windows. Gotchas: the VoiceOver cursor
  is separate from keyboard focus, and enabling VO switches on Full
  Keyboard Access (Tab behaviour changes) — our focus handling must stay
  coherent with what the tree reports. Real-world VO-on-AccessKit
  testimonies are thin; plan an explicit VO test pass.
- **Linux (Orca).** The Unix adapter speaks AT-SPI directly over D-Bus
  (pure-Rust zbus, no ATK/C deps) and works on X11 *and* Wayland (the
  toolkit must supply window bounds under Wayland). The old "needs
  experimental Orca forks" claim is outdated: niri 25.08 ships Orca
  support with its own UI exposed via AccessKit, and Orca's Wayland
  keyboard-monitoring gap is solved by the
  `org.freedesktop.a11y.KeyboardMonitor` interface (GNOME 48 / AT-SPI
  2.56, Plasma 6.4, niri; sway/wlroots apparently not yet). GNOME's
  push-based "Newton" successor is stalled — target AT-SPI2.

Adapter gating is *not* symmetric (verified in the registry sources):
`accesskit_winit` pulls `accesskit_windows` and `accesskit_macos` as
unconditional target-cfg dependencies — always compiled in on those OSes,
nothing to enable — while the Unix adapter is an optional feature (it drags
in zbus/async-io) that **Bevy disables by default** (the chain is
`bevy/accesskit_unix` → `bevy_internal` → `bevy_winit` →
`accesskit_winit/accesskit_unix`). Enable it cfg-guarded via a
target-specific dependency stanza in the viewer's `Cargo.toml`, which Cargo
merges with the base dependency:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
bevy = { version = "0.19.0", features = ["accesskit_unix"] }
```

(`accesskit_winit`'s own gate also covers the BSDs — dragonfly / freebsd /
openbsd / netbsd — widen the cfg to match if we ever build there. Android
would additionally need winit's `android-game-activity` backend, since
AccessKit does not work with NativeActivity; the iOS adapter is 0.1-new; a
web adapter does not exist yet. All out of scope.)

## Schema coverage and version churn

AccessKit's Chromium-derived schema covers what the chrome needs: roles for
buttons, labels, text inputs, trees, tab lists, list boxes, sliders and
windows; properties for label, value, numeric ranges, toggled / expanded /
selected and live regions; plus an `ActionRequest` channel back from the
AT. Plain single-/multi-line text editing is supported on all adapters;
rich text and hypertext are explicitly not. Two costs to budget: AccessKit
ships a breaking release roughly every 2–3 months, and since Bevy 0.15
`accesskit` is no longer re-exported — we depend on it directly and must
pin exactly the version Bevy uses (0.24 for Bevy 0.19, even though
upstream is newer). Expect a small re-pin on every Bevy upgrade.

## What Bevy gives free, and what we own

Free from `bevy_ui` / `bevy_ui_widgets` 0.19: `Button` / `ImageNode` /
`Label` map to roles automatically, names concatenate from child `Text`,
and the new `AccessibleLabel` component overrides the name; the core
widgets we already build on (slider, checkbox, radio, listbox, menu,
scroll area) set roles/values via hooks. `bevy_winit` rebuilds the tree
each frame from `AccessibilityNode` entities and reports focus from
`InputFocus` — which `input_context.rs` already makes the single source of
truth, so our focus model feeds the AT for free once nodes exist.

We own:

- enabling the Linux adapter (stanza above);
- accessible names resolved through the Fluent `Translator`: an
  `AccessibleLabel`-from-key convention parallel to `Translated`, so the
  screen reader speaks the localized label — never geometry-derived text;
- nodes, roles and states for our custom widgets: floaters (window/dialog
  role), tab rows, `virtual_list` / inventory tree rows
  (expanded/selected), menus, and the pie menu;
- live-region announcements (next section);
- **AccessKit action handling** — Bevy widgets are one-way today: they
  publish state but ignore incoming Increment / SetValue / Click
  `ActionRequest`s, so AT-initiated control needs our own handler systems;
- accessible text editing, the biggest gap: Bevy's `EditableText` never
  populates AccessKit `TextRun` / selection data, so accessible text input
  is effectively absent in Bevy today — likely an upstream contribution;
- the keyboard-reachability audit: every actionable control Tab-reachable
  in a meaningful order (`UiPanelShown` already parks hidden panels'
  `TabIndex`, so closed floaters stay out of the order).

## Live regions and announcements

AccessKit models announcements as the schema-level `Live` property
(Off / Polite / Assertive) on a node whose text we update — mapped to UIA
`LiveRegionChanged` on Windows, `NSAccessibilityAnnouncementRequested` on
macOS, and the AT-SPI announcement signal (Orca 45.2+) on Linux. NVDA and
Narrator honour live regions; JAWS is the laggard (flattens
assertive→polite, ignores some sources) — keep announcements short and
keep the same text reachable as ordinary focusable UI. Bevy offers no
convenience API; we maintain a visually-hidden `Live::Polite` node
ourselves. Hook points: `conversations.rs` (the `ConversationModel`
already centralises local chat, IMs, group tabs and unread counts) and
`chat.rs` (fed by `ChatReceived`). There is no general toast/notification
subsystem yet — when one lands it must route through the same announcer.

## Self-voicing and virtual-world prior art

Games ship self-voicing (their own TTS) because an AT tree cannot
represent a 3D scene. The Rust `tts` crate covers WinRT/SAPI,
AVSpeechSynthesizer and speech-dispatcher; its last release was July 2024
— low-maintenance but functional, and small enough to fork if needed
(Tolk-style screen-reader bridge DLLs are dead; skip them). The strong
position for us: a real AT tree for the 2D chrome plus an announcement
channel for world events, with a user toggle between "announce via screen
reader (live region)" and "own TTS" to avoid double-speaking — exposed as
a CLI option per the user-features convention.

**Radegast** is the canonical accessible SL client and is alive (July 2026
release; legacy WinForms plus the Avalonia-based cross-platform
"RadegastVeles"). Its model is exactly "standard widgets + the user's own
screen reader", and it shows what blind SL users actually rely on: chat/IM
reading first and foremost, nearby-avatar/object lists *by name*, and
keyboard-driven movement/teleport. IBM's old SL accessibility work ("Max
the virtual guide dog") points the complementary direction: expose "what
is around me" / "lead me to X" as queryable, announceable client features
rather than in-world scripts. Sonification patterns from accessible AAA
games (The Last of Us Part II, Forza's Blind Driving Assists): per-cue
channels with individual toggles and volume, proximity pings, a "sonar"
query key, and a learn/preview mode for every cue. Interacts with
[[viewer-i18n-colorblind-accessibility]] — never a single sensory channel.

## Likely scope when promoted to ready

1. Enable the Linux AT-SPI adapter (cfg-guarded stanza above; Windows and
   macOS adapters are always compiled in), emit the first nodes, and
   smoke-test with Orca under niri ≥ 25.08 — the local desktop is a
   first-class test bed.
2. `AccessibleLabel`-from-Fluent-key convention plus a role/name audit of
   the existing chrome widgets.
3. Custom-widget nodes: floaters, tab rows, virtual list / inventory tree
   (expanded/selected), menus, pie menu.
4. Live-region announcer for chat/IM/notifications with verbosity settings
   and the screen-reader-vs-own-TTS toggle (CLI-exposed).
5. AccessKit action handling for our widgets (the Bevy gap).
6. Accessible text input: populate `TextRun`/selection — likely upstream
   Bevy / `bevy_ui_widgets` work.
7. Keyboard-reachability audit across all floaters and bars.
8. Later, in-world: nearby-avatar/object announce lists, "what's around
   me" queries, sonification cue channels.

Testing matrix: NVDA (free) on Windows and Orca locally are the
workhorses; VoiceOver needs Mac hardware; JAWS is commercial — expect its
live-region quirks and rely on the visible-text fallback.

Reference (Firestorm, read-only): none — the gap *is* the finding.
AccessKit / `bevy_a11y` docs are the real reference.
