---
id: viewer-contact-set-pseudonyms
title: Contact sets — pseudonyms and display-name removal
topic: viewer
status: ready
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
