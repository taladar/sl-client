---
id: viewer-ui-tab-widget-dynamic-tabs
title: The tab widget cannot grow a tab, so the one strip that needs to is hand-rolled
topic: viewer
status: done
origin: viewer-skin-panel-state-classes sweep (2026-09-22)
points: 5
refs: [viewer-ui-tab-widget, viewer-social-im-conversations,
  viewer-skin-panel-state-classes]
---

Context: [context/viewer.md](../context/viewer.md).

The Conversations floater's strip is **half** the shared widget. It carries a
real [`TabStrip`](../../sl-viewer-ui-widgets/src/ui_tab.rs) — the widget's own
component, and the single source of truth for which tab is active — but every
tab **button** under it is spawned by hand, by `conversations.rs`, with that
file's own colours. The People tab beside them (`people.rs`) is a second
hand-rolled button in the same strip.

## Why it is hand-rolled

`TabSpec::labels` is a `&[String]` consumed at spawn, and both
`spawn_tab_strip` and `spawn_tab_container` / `fill_tab_container` build the
whole set of tabs in one pass. There is no add-a-tab and no remove-a-tab. A
conversation strip grows a tab when an IM opens and drops one when it closes,
so the widget as it stands cannot express it — that, and not a styling
preference, is why the strip was written twice.

So this is two pieces of work, and the first is the widget's:

## 1. Dynamic tabs on `ui_tab`

Add the operations a strip that changes at runtime needs — push, remove by
index or key, and reorder — keeping `TabStrip::active` the single source of
truth it already is (the `Checked` flags, the highlight and the panel
visibilities are all derived from it, and a removal has to re-derive them
rather than leave the active index dangling past the end).

Worth deciding early: whether a dynamic tab is addressed by **index** or by a
caller-supplied **key**. Conversations are keyed by `ConversationKey` and the
index of a given conversation's tab changes as earlier tabs close, so an
index-only API pushes that bookkeeping back onto the caller — which is half of
what makes the hand-rolled version hand-rolled.

## 2. What the conversation tab needs beyond a label

Not a reason to stay hand-rolled, but the adoption has to carry them:

- **A close affordance.** Note the reference puts it in the *pane's*
  top-trailing corner rather than on the strip tab, and `conversations.rs`
  already follows that — so this may be nothing the widget needs at all.
- **Unread attention.** A tab with unread lines currently alternates
  `TAB_ATTENTION_BACKGROUND` with its resting background at `BLINK_HZ`, from
  Rust, every frame. [`ATTENTION_CLASS`](../../sl-viewer-ui-core/src/skin.rs)
  already exists for exactly this and carries a CSS animation, so the blink
  should become a class the widget (or the caller) adds and the skin decides
  the look of — a skin that would rather not blink then overrides the rule.
- **A per-tab press payload.** Each tab's observer captures its
  `ConversationKey`. The widget's strip reports a `UiAction` with an index; a
  keyed API (above) would close that gap too.

## What it settles

Four colour constants in `conversations.rs` and three in `people.rs`
(`TAB_ACTIVE_BACKGROUND`, `TAB_INACTIVE_BACKGROUND`, `TAB_ACTIVE_BORDER`,
`TAB_BORDER`, `TAB_ATTENTION_BACKGROUND`, `TAB_LABEL_COLOR`) are the last
state-painting constants [[viewer-skin-panel-state-classes]] left behind, and
they go without a line of skin work here: the shared widget already takes its
selected look from `:checked` and its caption from `.sk-tab-label`. That sweep
deliberately did not convert them in place, because dressing a hand-rolled
strip to look like the widget is the wrong half of the fix.

## Done when

Conversations and People spawn their tabs through `ui_tab`, a conversation
opening or closing adds or removes one tab rather than rebuilding a strip, no
tab colour is named outside the widget, and an unread tab's attention is the
skin's animation rather than a per-frame colour flip.

## Done (2026-09-24)

**Addressed by button entity, not index.** `spawn_dynamic_tab_strip` returns a
`DynamicTabStrip` whose `add_tab(caption, position)` hands back a `TabHandle`
(the button and its caption node) and whose `remove_tab(button)` drops one. The
button *is* the key: a caller that needs to know what a tab stands for puts its
own component on it (`ConversationTab(ConversationKey)`) and reads it off
whichever tab the strip makes active — which also closes the per-tab payload
gap without the widget growing a generic key type. After every add or remove a
queued world command re-derives each `TabButton::index` from the viewport's
child order and moves `TabStrip::active` to wherever the `Checked` tab now
sits; removing the active tab hands `Checked` to the tab that took its place
and marks the strip changed even when the number did not move. Reorder was
left out: nothing needs it, and an unused operation would be forward-looking
surface.

A dynamic strip is a **bare** strip. The Conversations floater keeps one pane
per conversation, each with its own input, so the widget's panel switching was
never what it wanted. Captions are per tab (`TabCaption::Literal` /
`::Key`) because the one strip holds both a translated word (People) and data
(a resident's name).

**Adoption.** Conversations and People spawn their tabs through it; the
hand-rolled buttons, their press observers, `SelectConversation`,
`SelectPeople`, the divider copy (`spawn_tab_divider` is now `pub`) and all
seven colour constants are gone. `follow_strip_selection` carries a click into
the model (runs before the refresh, which carries the model's own selections
back onto the strip; both write only on a real difference). The People tab
wears `ExternalStripTab`, which is how the conversations module tells it apart
without knowing what it is, and the friends list now seeds on the People pane's
first showing rather than on a press message. Keyboard selection came free:
the strip is a `RadioGroup`, so the arrow keys walk it.

**Attention** is `ATTENTION_CLASS` on the tab, with a `.sk-tab.sk-attention`
pulse of its own in `common.css` (from the tab's `--surface-bg`, where the
toolbar button's pulse starts from `--control-bg`). `BLINK_HZ` and the
per-frame colour flip are gone.

The panel area and the two corner buttons (close, add participants) that
occlude a transcript line now wear `.sk-tab-panel`, so the pane the selected
tab merges into follows the same skin shade as that tab.

Tests: four in `ui_tab` (first tab active, an insert before the active tab
keeps the selection, removal renumbers and hands the selection on — heard by a
`Changed<TabStrip>` reader even when the index is unchanged — and a translated
caption), and `strip_sync` in `conversations.rs`, which drives the real
dynamic strip with People put in front of Nearby.
