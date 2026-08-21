---
id: viewer-avatar-picker-search-finds-nothing
title: The avatar picker searches over the retired UDP path, so it finds nobody
topic: viewer
status: bugs
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-inventory-share-picker, viewer-avatar-picker-multi-pick,
  viewer-search-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Typing a name into the shared avatar picker's **Search** tab produces no rows
at all — while the Search floater's People tab finds people perfectly well.

## Not the same endpoint

That contrast is the whole diagnosis. The two searches use different
backends: the Search floater asks the **directory** (`DirFindQuery` with the
people flag → `DirPeopleReply`), and the picker asks the **UDP avatar picker**
(`AvatarPickerRequest` → `AvatarPickerReply`). The directory still answers;
the UDP avatar picker does not.

Isolated with `sl-repl-tokio` against aditi, away from any UI
(`avatar_picker_request <query-id> <name>`) — `Marina`, `Sandy`,
`MarinaVector`, `GisetteVector` (two of them logged-in test avatars' own
names) and `Linden` (which matches thousands on any grid):

- the request goes out and the grid **answers every time**, within ~200 ms;
- the `QueryID` round-trips correctly;
- every reply carries **exactly one result block: a nil `AvatarID` with empty
  names** — the "no matches" shape.

Our encoders match `message_template.msg` block for block, so nothing is
malformed. The sim simply has nothing to say over that message any more.

## The reference stopped using it

`LLFloaterAvatarPicker::find()` (`llfloateravatarpicker.cpp:751`) picks its
path by capability, and the UDP message is the **last** resort — Firestorm's
own comment on that branch reads *"Avatar picker doesn't work anymore when
using legacy simulator messages"* (FIRE-15194):

- **by name** — `GET <AvatarPickerSearch>/?page_size=100&names=<escaped>`
  ("Prefer use of capabilities to search on both SLID and display name").
  The reply is `{ "agents": [ { id, username, display_name,
  legacy_first_name, legacy_last_name, display_name_expires } ] }`; the
  picker shows *display name* and *username* as two columns.
- **by uuid** — when the typed text parses as a uuid, `GET
  <GetDisplayNames>/?ids=<uuid>` instead, deliberately going to the cap
  rather than the name cache (which has no failure callback for a bad uuid).
- **otherwise** — the legacy `AvatarPickerRequest`, which is what we do
  unconditionally.

We do not request `AvatarPickerSearch` at all: it is absent from
`REQUESTED_CAPABILITIES` and appears nowhere in the workspace.

## Fix

Add `AvatarPickerSearch` to the requested capabilities and make the picker
prefer it, keeping the UDP path as the fallback for a grid that publishes no
such cap (OpenSim) — the standing "modern CAPS where present, UDP too, chosen
at runtime by cap presence" shape this client uses everywhere else.

Worth doing in the same pass, since both are cheap once the plumbing is there:

- **Search by uuid** through `GetDisplayNames` (already a requested cap) when
  the text parses as one, as the reference does — pasting a key is a normal
  way to name someone you cannot spell.
- **Show display name *and* username**, the reference's two columns; our rows
  are a single legacy `First Last` string today.
- **Say "not found"** rather than leaving the list silently empty: the nil-uuid
  sentinel is a real answer and should read as one, since an empty list is
  indistinguishable from "the search never ran" — which is exactly how this
  was reported.

The picker's other two tabs (**Friends**, **Near Me**) read local sources and
work; it is only naming a resident you are not already near or befriended to
that is broken — the case that matters for starting a conversation with
someone new, and the reason
[[viewer-conference-start-ui]] could not be live-verified.

Reference (Firestorm, read-only): `llfloateravatarpicker.cpp` (`find`,
`findByNameCoro`, `findByIdCoro`, `processResponse`).
