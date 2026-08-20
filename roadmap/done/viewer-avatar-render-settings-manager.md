---
id: viewer-avatar-render-settings-manager
title: Per-avatar render-settings manager
topic: viewer
status: done
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-avatar-complexity-limit]
refs: [viewer-derender-blacklist]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Avatar Render Settings: a management floater over *persistent*
per-avatar render overrides — Render Fully (exempt from complexity
limits), Do Not Render (permanent jelly), or default — surviving
relog, with add/remove/edit from the floater or the avatar context
menu.

Scope:

- A persisted per-account map avatar → override
  (fully/never/default), applied by the avatar renderer above the
  automatic complexity rules ([[viewer-avatar-complexity-limit]]).
- The management floater listing overridden avatars with mode edit and
  removal; entries addable from the avatar context menu.
- Distinct from the transient session derender
  ([[viewer-derender-blacklist]]).

Reference (Firestorm, read-only): `Floater.Toggle
fs_avatar_render_settings` (`menu_viewer.xml` World section),
`fsavatarrenderpersistence` + `llfloateravatarrendersettings`.

Builds on: the complexity-limit render pipeline (its
jelly/never-render machinery is what the overrides drive).

## Built

The decision was already there — the complexity limit's per-avatar override,
which the avatar pie set for the session. What this adds is the thing that
makes it a *decision about a person* rather than a mood: it outlives the
session, and there is somewhere to see and change the whole set.

`avatar_render_settings.rs` is the store — the exception list, its per-account
file (`avatar_render_settings.json`, a sibling of the account `settings.toml`,
like the derender blacklist), and one guarded way in
(`RequestRenderException`, which the pie, the floater and the picker all
write; it refuses the null id and the agent itself, who is never jellied
anyway). Setting `Normally` **removes** the entry, the reference's own rule and
the only reading that makes sense: "let the automatic rules decide" is the
absence of a decision, not a third one. The render decision does not read the
store directly — the complexity model mirrors it by revision, exactly as it
mirrors the friends roster, so the per-avatar question stays one hash lookup at
a crowded event.

**The file is the reference's, on purpose.** An exception persists as the
`VisualMuteSettings` integer Firestorm writes (`0` normally, `1` never, `2`
always), so a list exported from `avatar_render_settings.xml` ports across by
transcription — the same reasoning as the complexity score itself.

`avatar_render_floater.rs` is World ▸ Avatar Render Settings: a filter box over
a sortable, virtualized Name / Setting / Date table, laid out like the Asset
Blacklist beside it, with Render Fully / Never Render / Remove acting on the
selected row and **Add Fully… / Add Never…** opening the shared avatar picker —
the reference's `+` menu, and the only way to decide about someone who is
nowhere near you, which is the usual case for a decision made after an event.

**Naming someone who is not here** is the part with a real trade-off. Each
entry stores the name the deciding surface knew, and the live name cache is
read over it when it has an answer (mirrored into the store by revision, so the
list rebuilds when a name lands rather than polling per row). A resolution
never *rewrites* the stored name: a grid answers an id it cannot resolve with a
placeholder — OpenSim literally says `Unknown UserUPUUI` — and adopting that
would destroy the record of who the decision was about. Blank stored name, no
live answer: the row shows the id, which is also filterable.

**The pie now reads the store too.** The reference offers its three as check
items; a pie slice cannot carry a tick, so the decision already in force is the
slice shown greyed out — Normally is dead until there is an exception to clear.

## Verification

Unit-tested: setting and clearing (and that `Normally` removes rather than
records); an identical re-decision moving neither the revision nor the dirty
flag, so a repeated pie pick does not re-stamp the date or rewrite the file; a
resolved name mirrored but never stored, and the mirror following the list; the
request guards; the file round-tripping with the reference's numbering, an
unknown stored value degrading to "no exception" instead of failing the load;
the mirror bumping the complexity model's revision only on a real change; the
floater's filter (name or id), its sort keys, and its live-name-then-stored-
then-id labelling; and the Render sub-pie greying exactly the slice in force.

Live-verified against the local OpenSim with a second avatar: the pie's
More ▸ Render ▸ Never jellies them and writes the entry (`reason=Override`),
Fully draws them in full again, Normally removes the row; the floater's three
selection actions do the same from the list; Add Fully… adds someone through
the picker; and the list is still there — and still applied — after a relog
(`loaded the avatar render exceptions count=1`).

Also learned: on the local grid a name can come back as OpenSim's
`Unknown UserUPUUI` sentinel (`OpenSim/Framework/Util.cs`'s
`ParseUniversalUserIdentifier` default), for an account that exists and is
online. It is grid-side state — the in-world hover text shows the same string
for that avatar — and it is what makes the "never overwrite the stored name"
rule earn its keep.
