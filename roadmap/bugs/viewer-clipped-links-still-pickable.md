---
id: viewer-clipped-links-still-pickable
title: Scrolled-out chat links still hover and click, above the floater
topic: viewer
status: bugs
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-url-linkification, viewer-social-im-conversations,
  viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm bridge spammed nearby chat with URLs. Afterwards, the area of
screen **above** the Conversations floater responded to the mouse: hovering it
raised link tooltips and clicking it activated links, for transcript lines that
had scrolled out of the visible transcript. Nothing was drawn there — the lines
are clipped correctly, only their **hit boxes** are live.

It happens only while a conversation **transcript** pane is displayed (the user
saw it with the *Nearby Chat* tab active and not with *People*), which is
exactly the difference between a transcript that exists in the layout and one
that is `Display::None` — a hidden pane has no boxes to hit.

## Root cause: `clip_check_recursive` stops at the first non-clipping ancestor

`bevy_ui`'s picking backend does clip-check a hit
(`picking_backend.rs`, `clip_check_recursive`), but the walk gives up too early
(`crates/bevy_ui/src/focus.rs:345`, rev `807525f` — the revision this workspace
builds against, and unchanged from crates.io `bevy_ui 0.19.0`):

```text
if let Ok(child_of) = child_of_query.get(entity)
    && let Ok((computed_node, transform, node)) = clipping_query.get(child_of.0)
    && !node.overflow.is_visible()          // <-- the early exit
{
    if ...point outside this ancestor's clip rect... { return false; }
    return clip_check_recursive(point, child_of.0, ...);
}
true                                        // "unclipped by all ancestors"
```

The recursion only continues **through clipping ancestors**. As soon as one
ancestor has visible overflow the whole chain answers `true`, so no clipping
grandparent is ever consulted. A node is therefore only clip-tested when every
ancestor up to the clipper clips — in practice, when it is a *direct* child of
the clipper. That is why most of our scroll lists look fine and this one does
not.

The transcript's chain is three deep:

```text
transcript_scroll   (Overflow::scroll_y  — the clipper)
  transcript_column (visible)
    linkified row   (visible)
      link node     (Button + Pickable — the thing that gets picked)
```

The link's parent is the wrapping row, whose overflow is visible → `true`,
and `transcript_scroll` is never asked. Lines scrolled off the top sit at
negative offsets inside the content column, i.e. **above the floater** on
screen, which is precisely where the phantom hits are.

## Scope

Not a chat bug: any pickable content nested two or more levels inside a
clipping / scrolling container is affected. Chat is where it showed because a
wall of URLs is unusually rich in pick targets and the docked Conversations
floater sits at the bottom of the screen, so its overflow points at open sky.

## The fix

Upstream, in the `bevy` fork this workspace already carries (the same fork
`[patch.crates-io]` already points every `bevy_*` crate at; worktree
`~/devel/3rdparty/bevy-externally-posed-skin`): recurse to the parent
**regardless** of whether this ancestor clips, and only reject when a clipping
ancestor excludes the point. Roughly — split the two conditions:

```text
let Ok(child_of) = child_of_query.get(entity) else { return true };
let Ok((computed_node, transform, node)) = clipping_query.get(child_of.0)
    else { return true };
if !node.overflow.is_visible() && ...point outside the clip rect... {
    return false;
}
clip_check_recursive(point, child_of.0, clipping_query, child_of_query)
```

`OverrideClip` keeps working: it is a filter on `child_of_query`, so an entity
carrying it still ends the walk (that is what lets menu popovers escape a
floater's clip — see `menu.rs`, `ui_combo.rs`).

A viewer-side workaround exists but is worse: flattening the transcript so each
line is a direct child of the scroll node would fix chat alone and leave every
other nested list wrong.

## How to verify

Nearby chat with several screens of URL-bearing lines, scrolled so old lines
are off the top; the space above the floater must not raise a link tooltip
(`crate::linkified_text`'s `HoveredLink`) or activate anything. A unit test
belongs upstream, on the picking backend, not here.

Reference (Firestorm, read-only): `llui/lltextbase` hit-tests glyph rects
inside the laid-out block, so it has no equivalent of this — its scrollback
cannot leak hit boxes outside the widget.
