---
id: viewer-contact-set-pseudonyms
title: Contact sets — pseudonyms and display-name removal
topic: viewer
status: done
origin: split from viewer-contact-sets (2026-08-20)
refs: [viewer-contact-sets, viewer-name-tags-decorations]
---

Context: [context/viewer.md](../context/viewer.md).

The half of Firestorm's contact sets that renames people rather than
colouring them: a per-avatar **pseudonym** (an alias the user gives
someone, shown instead of their name), and **display-name removal** (show
this person's legacy name only). Both are per resident, not per set, and
both live in the same per-account file as the sets
([[viewer-contact-sets]] already recognises and skips the reference's
`Pseudonyms` key rather than reading it as a set).

The point of the feature is that it applies **everywhere a name is
drawn** — name tags, chat, IM, the radar, the people lists — so the work
is not a panel but a hook: one override consulted where the viewer
resolves an avatar's display label (`crate::avatars`'s name cache), so
every consumer inherits it without knowing about contact sets. A
pseudonym must never be confused with the grid's answer, and must never
be persisted over the stored "what they were called when filed" memo.

Scope: the pseudonym / display-name-removed store beside the sets, the
name-resolution hook, and the UI to set and clear one — the reference's
`SetAvatarPseudonym` prompt (already in the notification catalogue) from
the Contact Sets panel and the avatar context menu, plus the
"Pseudonyms" pseudo-set in the panel's chooser listing everyone who has
one.

Reference (Firestorm, read-only): `lggcontactsets`
(`getPseudonym` / `hasDisplayNameRemoved` / `checkCustomName`),
`fspanelcontactsets` (the set / remove pseudonym buttons).

## Built

The feature is one hook and three buttons. `contact_sets.rs` grew the
alias store beside the sets — the reference's own `Pseudonyms` map in the
same `contact_sets.json`, read and written as Firestorm writes it (it was
skipped as an internal key before), with display-name removal stored as
the reference's marker alias `--- ---` rather than a flag of our own, so
an alias list survives a round trip through either viewer.

The hook is `apply_name_aliases`, which mirrors the store into the
**name cache** as a `NameAlias` on each `NameRecord`. That is what makes
the feature reach: `NameRecord::preferred_name` already answers every
drawn name, so tags, the radar, tooltips, the inspectors and linkified
names inherit the alias without knowing contact sets exist. Two details
make the mirror honest rather than clever:

- the grid's own answer is never overwritten — it stays in the record's
  fields, `grid_name` returns it, and `name_of` (the wire-facing legacy
  name a mute entry carries) is untouched;
- a record *created later* — an avatar first seen after the alias was
  given — folds the alias in as it is made, so the alias is not a race
  against when someone walks into view.

An alias is shown in the reference's **quoted** form (`'Nickname'`).
That is the feature's honesty: a name in quotes is visibly the user's
own, and can never be read as something the grid answered.

Three surfaces keep their own name stores and so are mirrored too rather
than inheriting: the friends list (people who are nowhere near the
viewer), the chat/IM transcript (whose speaker name is resolved at render
from the line's agent link, so an alias renames the **backlog** as well),
and the contact-sets panel's own rows. The transcript rebuild gates on a
separate `alias_revision` — the ordinary revision moves every time a
member's name resolves, and that is far too often to redraw every
transcript for.

The panel gained the reference's three buttons (*Set Alias… / Rem Alias…
/ Rem DN…*) and its third pseudo-set, *Pseudonyms*, listing everyone with
an alias — who need not be in any set, which is why the name memo now
follows an alias as well as a filing. The `SetAvatarPseudonym` prompt was
already in the notification catalogue; it is raised through one message
(`OpenSetPseudonym`) so the panel and the avatar pie answer it in the
same place.

One **deliberate addition**: the avatar pie's `Add ▸` grew a *Set Alias*
slice (north, beside Add to Set). The reference reaches pseudonyms only
from the Contact Sets panel, but naming someone is something you do while
looking at them.

Deliberately not built: the reference's multi-select
`SetAvatarPseudonymMultiple` path (our contact-set panel selects one
member), and greying the three buttons when they do not apply — the panel
has nine older buttons that are inert in the same way, and greying three
of twelve would read as a bug.
