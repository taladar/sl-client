---
id: viewer-region-estate-group-picker
title: Group picker for the Region/Estate Access → Allowed Groups list
topic: viewer
status: done
origin: user request (2026-07-28) — follow-up found while building the About
  Region floater (viewer-region-options-estate)
points: 3
refs: [viewer-region-options-estate, viewer-audit-picker-requester-identity,
  viewer-keyed-floater-audit, viewer-build-widget-parity-upgrades]
---

Done (2026-09-12). The picker exists as
`sl-viewer-pickers/src/group_picker.rs` and three surfaces open it: the
estate's allowed-groups **Add**, About Land General's group **Set…**, and the
build tool's General-tab group. See [Done](#done) for what landed and what
diverges from the reference.

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater's **Access** tab has four estate access lists
(managers, allowed residents, allowed groups, banned residents). Three are
fully wired — **Add** opens the avatar picker
(`crate::avatar_picker::OpenAvatarPicker`) and per-row **Remove** commits an
`estateaccessdelta` — but **Allowed Groups** currently only supports display
and per-row Remove: there is **no group picker**, so its **Add** button is
omitted.

This task adds a **group picker** widget (the group analogue of
`avatar_picker.rs` — search groups by name / list the agent's groups) and
wires it into the About Region Access tab so a manager can add a group to the
allowed-groups list. The write path already exists
(`Command::UpdateEstateAccess` with `EstateAccessDelta::AllowedGroupAdd`, target
`OwnerKey::Group`); this is the missing picker plus the `AddAllowedGroup`
action in `about_region.rs` (mirror `AddManager` / `AddAllowed` / `AddBanned`).

A group picker is also reusable elsewhere (e.g. parcel group assignment).

Reference (Firestorm, read-only): `llfloatergrouppicker`,
`panel_region_access.xml` (the allowed-groups sub-tab Add button).

## Parity-audit addendum (2026-08-19)

The group picker this task builds (`floater_choose_group.xml` in the
reference) is a **generic reusable picker**, not an estate-only widget:
the same surface serves group notices, avatar-profile group selection,
and About Land General's group assignment. In particular, wire About
Land's "Set…" (change parcel group) through it — the write path already
exists as `ParcelUpdate.group_id` (`sl-proto/src/types/parcel.rs`), and
`AboutLandAction` in `sl-client-bevy-viewer/src/about_land.rs` has no
group-set action today. Build it group-picker-shaped like
`avatar_picker.rs`, then reuse everywhere.

## Done

`sl-viewer-pickers/src/group_picker.rs` — a keyed floater built exactly like
its two siblings in that crate: `OpenGroupPicker { requester, field,
allow_none, scope }` in, `GroupPicked { requester, group, name }` out, the
window keyed by `picker_identity` (the opening floater *and* the field), and
every piece of state a component on the window root, so it persists nothing
and dies with the window that opened it
([[viewer-audit-picker-requester-identity]]).

### Two sources, and which of them a caller may have

The reference's picker is the agent's own memberships and nothing else. That
is right for a *set-group* control and wrong for the estate list, and the
difference is a protocol fact rather than a preference: the simulator refuses
a parcel or object group the agent is not in, while the allowed-groups list
only ever records an id. So `GroupPickerScope` says which a caller can use —
`MemberGroups` (the default; the tab strip and search row are hidden, and the
window is the reference's single list) or `AnyGroup`, which adds a **Search**
tab issuing the directory query the Search floater's Groups category already
makes (`Command::DirFindQuery` + `DirFindFlags::GROUPS` → `DirGroupsReply`).
The estate's Add is the one caller that takes it, so an estate manager can
allow a group they have not joined — which the reference cannot do at all.

The search asks for every maturity band, unlike the Search floater, which
filters by the person's content preferences: that floater is browsing, this
one is resolving a group somebody already named, and a band filter would hide
the one group the search exists to find. Only the first page is taken.

`allow_none` is the reference's `removeNoneOption` inverted, so the row is
never *built* for a list that cannot hold a null group rather than built and
deleted. The worn group is drawn bold as the reference draws it — whichever
source found it — but nothing is preselected and OK with nothing selected does
nothing, so a stray press cannot commit the null group nobody picked.

### The three callers

- **About Region → Access → Allowed groups**: an `Add Group…` button replaces
  the note that said a picker was needed. `AboutRegionAction::AddAllowedGroup`
  opens the picker `without_none().searching_the_directory()`;
  `apply_group_picks` routes the answer by the pressed button's action and its
  host floater, as the avatar and experience picks already do, and commits
  through the same `add_access_entry`, which now takes the bare `Uuid` — three
  of the four lists hold agents and the fourth holds groups, and
  `AccessList::target` is what re-types it for the wire.
- **About Land → General**: a **Set…** button beside the group name.
  `AboutLandAction::SetGroup` opens a member-only picker *with* the none row,
  and the pick commits immediately rather than waiting for Apply — as the
  reference does (`LLPanelLandGeneral::setGroup` calls
  `sendParcelPropertiesUpdate`) and as this floater must, since the group it
  shows is the parcel's own and only the grid's echo moves it. `commit_draft`
  is split out of `apply_draft` for that, and deliberately does not read the
  edit fields: a group set must not smuggle out a half-typed parcel name.
- **Build tool → General**: the group **cycle** button is gone. It walked the
  agent's memberships one press at a time — usable with three groups, not with
  thirty — and its own comment called itself a stand-in. The row is now the
  reference's `llpanelpermissions` shape: the group's name as an `InfoText`
  line, a **Set…** button, and Deed. `apply_group_picks` there runs *outside*
  the build-tool-active gate, since a pick can land the frame the picker closes
  and the tool settling out from under it must not swallow the answer.

`GroupsModel` gained `active()` — `ordered()` already marks the worn group on a
membership row, but a search result comes from somewhere else and has to be
marked the same way.

### And the names those estate rows show

`sync_allowed_groups_view` used to render `(uuid)` for any listed group the
membership cache could not name, because nothing ever asked: `group_name` was
read but `request_name` never called. Harmless while the only addable groups
were ones you had joined; reachable in normal use the moment the picker can
name a group by search. It now asks (`RequestGroupNames`) for the unnamed ids
when the **list** moves — not whenever a name lands, which would make a group
the grid will not name into a re-request loop — and the estate's own pick path
seeds the cache from the name the picker already had
(`GroupsModel::note_resolved_name`), so the new row reads properly without a
round trip at all.

### What the first live check found, and what it cost

Three defects, all of them invisible to the tests that existed, all now pinned
by tests that fail without the fix:

- **The build tool's Set… opened the picker and then did nothing.**
  `apply_group_picks` read the selection's `ObjectProperties` to echo the new
  group locally — and *bailed* when there were none. Those arrive when the
  simulator feels like sending them, so an object selected a moment ago has
  none, and the send sat behind a local echo it had no business depending on.
  The echo is now best-effort and the command goes out either way.
  `the_group_set_button_opens_a_picker_whose_answer_commits` drives the button
  through the real pointer stack and fails (0 commands) against the old guard.
- **About Land's Set… and About Region's Add Group… were not on screen.** Both
  are write buttons, and a write button used to **hide** where the agent could
  not edit the parcel / manage the estate. The button existed and was correct;
  it was invisible, which reads as a viewer that does not have the feature at
  all — exactly the wrong answer to "where do I set the group". They now grey
  (`InteractionDisabled` plus a dimmed label) and stay put, which is also what
  the reference does: `LLPanelLandGeneral::refresh` walks its buttons with
  `setEnabled`, and `LLFloaterRegionInfo` greys the whole Access panel with
  `setCtrlsEnabled(false)`. This applies to **every** write button in both
  floaters, not just the two new ones — one button greying while its neighbours
  vanish would be worse than either rule alone.

The second live check found a fourth, and it was not this task's at all:
confirming a group **deselected the object**. A keyed picker despawns its
window during the press that closed it, and the build tool's world-pick gesture
then reads that press as a click on empty world. Every self-closing surface in
the viewer had the same hole; it is fixed generically and written up as
[[viewer-self-closing-widget-leaks-its-press]].

The first version of the build-tool test failed for a fifth reason worth
recording: it drained `Messages<OpenGroupPicker>` directly after `settle`, and
a message lives two frames while `settle` runs two updates. `sl_viewer_testkit`
says so in `drain_actions`' own docs and offers `record` / `drain` for it; the
test now uses those.

### Divergences from the reference, deliberate

- **No powers mask.** `LLFloaterGroupPicker::setPowersMask` has exactly one
  caller in the whole reference — invite-to-group, which this viewer does not
  have — so a mask here would be a parameter no call site passes and no test
  could exercise. `GroupsModel` does not retain `group_powers` today either;
  both land together with the invite feature.
- **Title.** The reference titles the window "Groups" and captions the list
  "Choose a group:"; this one folds the caption into the title, "Choose
  Group", matching the sibling "Choose Resident". A window that is only ever a
  chooser should not be titled as if it were the group list, which this viewer
  has as the People pane's Groups tab.
- **No double-click-to-OK.** The reference's list takes one
  (`setDoubleClickCallback`); this codebase has no double-click plumbing, and
  neither does the resident picker. Worth adding for both at once, not for one.

Two scrambled doc comments are fixed in passing: `GroupsModel::own_title` and
`revision` had swapped halves of each other's docs, as did `PickedAvatar` and
`AvatarPicked`; `about_region::apply_texture_edits` carried a superseded copy
of its own first paragraph.

## Verified

`cargo check --workspace --all-targets` and `cargo clippy --workspace
--all-targets` clean.

Unit-verified: seventeen tests in `group_picker.rs` — twelve over the state
(the name-ordered membership list with its worn mark, the none row's position
and its absence where the open refused it, nothing-picked-until-clicked, a
click past the last row, a selection carried across a membership change **by
group** and dropped when its group leaves, the empty list, a search row's
member count and worn mark), and five driving a real window through the pointer
stack (press → row → OK answers and closes; OK before a row answers nothing;
Cancel answers nothing; two fields open two windows; a member-only picker hides
its tab strip and search row).

Each of the three callers has a test for its own half: the build tool's Set…
opens a picker whose answer commits a `SetObjectGroup`; About Land's pick
commits a `ParcelUpdate` carrying the group; About Region's pick posts an
`AllowedGroupAdd`, reaches that window's list, and seeds the name cache. Both
floaters also pin **greyed, not gone** for their write buttons.

Live-checked on the local OpenSim grid: the first pass found the three defects
above, the second confirmed About Land and About Region and found the
deselect ([[viewer-self-closing-widget-leaks-its-press]]).

## Follow-ups

- A **powers mask** and the invite-to-group action that needs it (see above).
- Double-click a row to confirm, for this picker and the resident picker
  together.
- The search takes only its first page. If a picker ever wants paging it should
  take the Search floater's `Page<DirGroupResult>` rather than grow its own.
