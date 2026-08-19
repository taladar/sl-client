---
id: viewer-preferences-search-synonyms
title: Preferences search — synonyms / alternative search terms
topic: viewer
status: ideas
origin: user request (2026-08-19), after searching "complexity" failed to find
  the render-cost name-tag rows
refs: [viewer-preferences-floater, viewer-name-tags-complexity-distance]
---

Context: [context/viewer.md](../context/viewer.md).

The preferences filter matches a row **only against its rendered label text**
(`preferences.rs`: `text.0.to_lowercase().contains(&state.filter)`). So a
setting is findable only by the exact words someone happened to put in its
label — and a viewer full of Second Life jargon has several names for almost
everything.

The case that prompted this: the name-tag render-cost rows were labelled
"render cost", so searching **complexity** or **ARC** — the words residents
actually use, and the words the *graphics* tab's own section uses for the same
feature — found nothing at all. Relabelling fixed that one row, but relabelling
cannot fix the general problem: "complexity" and "ARC" are both right, and a
label can only be one of them. Nor can it help a user searching "lag",
"jelly(doll)", "draw distance" vs "view distance", "IM" vs "instant message",
"mute" vs "block", "avatar" vs "resident".

Sketch:

- Give the searchable-row spawners an optional list of extra **keywords**, and
  match the filter against the label **or** any keyword. Keywords are Fluent
  keys like the labels, so they translate — a German user should be able to
  search German synonyms, and the English jargon (`ARC`) should stay findable
  in every locale.
- One keyword catalogue per row is a lot of ceremony for the common case, so
  most rows should carry none; the ones worth it are those whose subject has a
  well-known second name.
- The same mechanism would serve the **menu** search ([`crate::menu_search`]),
  which has the same label-only limitation and the same jargon problem.
- Worth considering, and cheaper than per-row keywords: matching a row against
  its **section heading** too, and against its **setting key** (a user who
  knows `RenderAvatarMaxComplexity` from a wiki page or another viewer should
  find it). The setting key is already on the row's binding, so that one is
  nearly free and would have made the original case work — the key contains
  "Complexity".

Reference (Firestorm, read-only): `fssearchablecontrol.h` — the reference's
filter has the same label-only behaviour, so this is an improvement on it
rather than a parity gap.
