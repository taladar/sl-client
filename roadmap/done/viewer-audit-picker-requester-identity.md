---
id: viewer-audit-picker-requester-identity
title: Two instances of one window share a picker, and both claim its answer
topic: viewer
status: done
origin: live check of [[viewer-region-experiences-panel]] (2026-09-12)
points: 5
refs: [viewer-region-experiences-panel, viewer-keyed-floater-audit,
  viewer-experiences-floater, viewer-settings-save-as-create-then-put,
  viewer-audit-asset-editor-scaffold]
---

Done (2026-09-12). All five pickers are opener-keyed and route by `Entity`, the
claim bookkeeping is gone, and the floater manager owns both halves of a
picker's lifetime. See [Done](#done) at the end for what landed.

Context: [context/viewer.md](../context/viewer.md).

A picker is opened by a *window*, and several of the windows that open one are
**instanced** — About Region is one per region, About Land one per parcel, and
the profiles one per resident / group / experience
([[viewer-keyed-floater-audit]]). Two instances of such a window that open the
same picker share one picker window, and where the reply is routed by a
`&'static str` tag they **both claim the answer**.

Concretely, with two About Region windows open (walk across a border and open
it again — the whole point of keying it):

- press Add on the Allowed experience list in window A, then in window B: both
  resolve to `FloaterKey::subject("about-region-experience-allowed")`, so B
  raises and *restarts* A's picker rather than opening its own;
- the pick then lands on whichever window still holds the claim. For the
  experience lists that is a per-list `bool` on each window
  (`AboutRegionState::pending_experience_picks`), so **both** windows take the
  pick and both post their own three lists to the region;
- the avatar pickers are worse, because their claim is a single slot
  (`AboutRegionState::pending_pick`): the second open overwrites the first
  window's claim, and a pick confirmed for A is dropped on the floor. About
  Land has the same pair of tags (`about-land-allow` / `about-land-ban`) and the
  same single slot.

## The five pickers, and which half each one already gets right

There are two separable questions — *which picker window* an open addresses, and
*who receives the pick* — and the answer is inconsistent across the five:

| Picker | Reply routed by | Window identity |
| --- | --- | --- |
| avatar (`OpenAvatarPicker`) | `&'static str` tag | singleton |
| experience (`OpenExperiencePicker`) | `&'static str` tag | `FloaterKey::subject(tag)` |
| settings (`OpenSettingsPicker`) | `Entity` | singleton |
| colour (`OpenColorPicker`) | `Entity` | singleton (a `Resource` holding one `requester`) |
| texture (`OpenTexturePicker`) | `Entity` | `FloaterKey::named(field)` |

The **texture picker is the model for routing**: `requester: Entity` names the
widget, so `about_region::apply_texture_edits` resolves that entity's host
floater and the pick reaches the right window even with two open. Nothing is
claimed and nothing can be stolen. The settings and colour pickers route the
same way; they are simply one-at-a-time.

The **window identity** is where all five are still wrong for an instanced
opener: a tag and a field element id are both properties of the *control*, and
two instances of a window have the same controls.

## Scope

- Route every pick by `Entity`, as the texture / settings / colour pickers do.
  That deletes the claim bookkeeping entirely — `pending_pick`,
  `pending_experience_picks`, and the `requester` matching in
  `apply_avatar_picks` / `apply_experience_picks` — and with it the class of bug
  where a pick is taken twice or dropped. A consumer resolves the requester's
  host floater (`host_floater`) exactly as the texture path already does.
- Make the picker's window identity include the **opening window**, so two
  instances get two pickers. The natural key is (opener, field): the opener's
  own `FloaterKey` where it has one, and the singleton's id where it does not.
- **A picker persists no geometry** (decided 2026-09-12). An opener-keyed picker
  cannot use `FloaterKey::named` anyway: an entity id is not stable across
  sessions, and keying on the opener's *subject* would file one settings entry
  per region or parcel ever visited — which is the reason `FloaterKey::Subject`
  persists nothing in the first place (see `Floater::persist_id`). So the
  texture picker gives up its remembered per-field geometry, and every picker
  becomes subject-keyed: a transient dialog is the wrong thing to restore, as
  the experience picker reopening itself at login already showed
  ([[viewer-region-experiences-panel]]).
- **A picker closes with the window that opened it** (decided 2026-09-12).
  Otherwise it outlives the window it was answering and confirms a pick into
  nothing — which is harmless *today* only because the claim check drops the
  orphan, and the claim check is the first thing this task removes.
- The avatar, settings and colour pickers are singletons and need keying, not
  just re-routing: two instanced windows wanting a resident at once is the same
  situation.

## Closing with the opener belongs to the manager

The same argument as `raise_floaters_on_open` (see
[[viewer-region-experiences-panel]]): "this window belongs to that one" is a
relationship the floater manager can act on once, rather than five features
each remembering to tear their picker down — and the one that forgets is found
by a person, months later, when a stale picker answers for a window that is
gone.

So: a component in `sl-viewer-ui-widgets::floater` recording a floater's
**owner**, set by the picker as it opens, and a pass beside the raise that
closes any floater whose owner has despawned or been hidden. A keyed opener's
Close despawns it, so "owner is gone" covers the About Region / About Land
case; hiding covers a singleton opener. Closing the picker then goes through
the ordinary `FloaterOp::Close`, which despawns it because it is keyed — so its
per-window state and its outstanding search go with it.

**The owner is the opening floater, not the control.** Two different entities
are in play and they answer two different questions: the pick is *routed* to
the widget that asked (`requester: Entity`, the swatch or the Add button), while
the picker's *lifetime* hangs off the floater that widget lives in — reached
with `host_floater`, the way the texture path already resolves a pick back to
its window. Recording the control as owner would work by accident in most
cases and fail exactly where it matters: a floater rebuilding a panel in place
would despawn the control and take the picker with it while the window is still
open.

Worth a test per half: an owner that despawns takes its picker with it, and an
owner that merely hides does too.

Reference (Firestorm, read-only): `llpanelexperiencelisteditor.cpp`
(`onAdd` mints a fresh `mKey` per press and marks the previous picker dead — the
reference's answer to exactly this, one picker per *press* rather than per
control), `lltexturectrl.cpp` (`LLTextureCtrl::showPicker`, a picker owned by
the control instance).

## Done

### The manager grew two relationships it did not have

`FloaterOwner(Entity)` records **which window opened this one**, and
`close_owned_floaters` — a pass beside `raise_floaters_on_open` in `PostUpdate`
— closes any floater whose owner has despawned (a keyed opener's Close) or been
hidden (a singleton's). It writes the close for the **next** frame's command
pass on purpose: several windows read a `FloaterOp::Close` *before* that pass to
answer whoever was waiting (the texture picker reverts an uncommitted preview
that way), and a command written inside `Update` would reach the despawn before
some of those readers reached the command. A frame of latency on a close nobody
asked for is invisible; a lost revert is a wrongly-textured object.

`picker_identity(requester, field, …)` answers both questions from one walk up
the tree: the owning floater, and the `FloaterKey` for a picker that window
opened for that field — `{opener id}/{opener key}/{field}`. Keying on the
opener's *identity* rather than its entity is what survives a panel rebuilt in
place, and is what a person can read in the window's `Name`.

### Routing

`OpenAvatarPicker` / `AvatarPicked` and `OpenExperiencePicker` /
`ExperiencePicked` now carry `requester: Entity` (the button pressed) plus a
`field` naming which of that window's pickers it is. That deleted every claim:
`AboutRegionState::pending_pick`, `pending_experience_picks`,
`AboutLandState::pending_pick` and `conversations::PendingParticipantPick` are
gone. Each consumer reads *what* to do off the pressed button's own component
(`AboutRegionAction`, `AboutLandAction`, `AddParticipantsButton`,
`RenderSettingsButton`, `BlockedButton`, `ContactSetsButton`) and *which window*
off `host_floater` — so the two About Region windows the bug needed now each
take only their own pick.

Two openers had no button entity to name. The inventory **Share** is a menu
action, so it names the inventory window itself — which is also the window its
picker belongs to and closes with. The Conversations pane's add-participants
button appends the conversation key to its field, because one window holds a
pane per conversation and two panes sharing a field name would share a picker.

### Three singletons became keyed windows

The avatar, settings and colour pickers moved their `Resource` state onto the
window root as components and build their content per instance, as the
experience picker already did. Two swatches answered at once are now two
windows: the colour picker used to warn about the losers of a frame and drop
them, leaving a live preview nobody would ever commit or revert.

### `FloaterKey::Named` went with the texture picker's remembered rectangle

A picker keyed on the window that opened it cannot be one of a closed set of
names, and an entry per region or parcel ever visited is what `FloaterKey`
exists to avoid — so the texture picker gave up its per-field geometry, which
left `Named` with no users at all. `FloaterKey` is now the one subject form and
`Floater::persist_id` is one line.

## Also done: a window may refuse a close

Raised while reworking the close path, and the missing half of
[[viewer-settings-save-as-create-then-put]]'s *Not done*: refusing a close
"needs the floater chrome to support vetoing one, which it does not".

- `FloaterCloseGuard { armed }` on a window root. While armed, the manager turns
  a `FloaterOp::Close` into a `FloaterCloseRequested` naming the window and
  leaves it standing; the feature asks, and answers with the unrefusable
  `FloaterOp::CloseNow`. The manager never disarms the guard — a feature that
  cleared it instead of answering would leave the *next* close unguarded, which
  is the silent discard this exists to stop.
- `close_owned_floaters` uses `CloseNow`: the window the work was for is the
  thing that just went away, so there is nothing left to answer — and a
  held-back close from a pass that runs every frame would re-ask forever.
- Both settings editors arm the guard from `session.modified` and raise the
  same `SettingsConfirmLoss` their open-replaces and Import paths already raise,
  through the same held slot. The notecard and script editors
  ([[viewer-audit-asset-editor-scaffold]]) can now do the same; what they still
  lack is dirty tracking, which no chrome can give them.

## Verified

`cargo clippy --workspace --all-targets` clean.
`cargo test --release --lib` green on the seven touched crates:
`sl-viewer-ui-widgets` 231, `sl-viewer-people` 131, `sl-viewer-environment` 92,
`sl-viewer-inventory` 85, `sl-viewer-notices` 79, `sl-viewer-places` 37,
`sl-viewer-pickers` 12 — including the new opener-keyed identity test, the
picker-closes-with-a-despawned-and-with-a-hidden-owner pair, the guard's three
states, and a guard that does not outlive its owner.

Not live-verified: two About Region windows on two regions each adding to the
same experience list, and a dirty settings editor's ✕, both want a grid.
