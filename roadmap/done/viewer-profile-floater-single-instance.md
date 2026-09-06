---
id: viewer-profile-floater-single-instance
title: A second resident profile replaces the first instead of opening its own
topic: viewer
status: done
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-social-profiles, viewer-ui-floater-basic,
  viewer-social-group-profile, viewer-keyed-floater-audit,
  viewer-about-landmark-floater]
---

Context: [context/viewer.md](../context/viewer.md).

With one resident's profile open, opening a second resident's profile **reused
the same window**: the subject was swapped and the first profile was gone. The
reference viewer opens one profile floater **per resident** — they stack, and
you can compare two people side by side.

## Why it happened

`crate::avatar_profile` was built as a singleton: one `PROFILE_FLOATER_ID =
"avatar-profile"` window, one `ProfileState { target }`, one `ProfileUi`.
`open_profile` read only the **last** `OpenAvatarProfile` of the frame and, for
a different agent, reset the state, re-requested everything and showed the one
floater. There was nowhere for a second subject to live.

The floater scaffold was singleton-shaped too: a floater's identity was its
`&'static str` id alone, so "another instance of this kind, keyed by its
subject" was not expressible.

## What landed

The scaffold, in `sl-viewer-ui-widgets::floater`, modelled on the reference's
`LLFloaterReg::showInstance(name, key)`:

- **`FloaterKey`** — a floater's second half of identity, in two variants that
  carry the reference's `LLFloater::getControlName` split: `Subject` (an agent,
  a group, an item — instances are distinct but persist **nothing**, because a
  saved rect per agent id would grow the settings file by one entry per
  resident ever opened) and `Named` (a small closed set — each keeps its own
  geometry under `{id}_{name}`). `Floater::persist_id` is the one place that
  decides, and every stage of `floater_persist` asks it.
- **`KeyedFloaters::open`** — find-this-subject's-window-or-spawn-one, as a
  single `SystemParam`. A new instance cascades 16 px off the kind's newest
  window (`UIFloaterOffset` / `stackWith`, wrapping after 8 so the twentieth
  window is still reachable) and is raised **directly** rather than through a
  `FloaterCommand`: the command pass would not find an entity spawned the same
  frame, and the new window would open behind the old one.
- **Closing a keyed instance despawns it**, as the reference destroys a
  non-single-instance floater on close. That is what makes per-instance state a
  component on the window: it goes with the window, and the next open starts
  from fresh replies rather than a stale shell.
- **`host_floater`** — the window a clicked node belongs to, so an observer
  acts on *its* instance instead of a resource that could hold only one.

The profile is converted onto it: `ProfileState` (now with a non-optional
`target`), `ProfileDirty` and `ProfileUi` are components on the window;
`open_profile` honours **every** open of the frame, not just the last; the
per-frame systems and both observers resolve their window; the Web tab's load
clock is per browser view. Nothing spawns at `Startup` any more, so the profile
no longer costs the per-frame UI walk of a never-opened window.

## One thing this shipped wrong, fixed with the next conversion

The raise above happened *before* the manager's command pass, and one press can
ask for both: a click inside a floater raises that floater (its root observer's
`BringToFront`) and may open a keyed window (a group row, a name link). The
open won the first z and the click's raise then landed on top of it, so a
window opened from inside another one appeared **behind** it. Found live while
checking the group profile ([[viewer-keyed-floater-audit]]); fixed by
`FloaterSystems::Commands`, which every keyed open system now orders after.

## Still to do

The audit of the *other* per-subject singletons is
[[viewer-keyed-floater-audit]] — group profile, item properties, the notecard /
script editors, the texture picker, About Land, About Landmark.

## How to verify

Headless: `sl-viewer-ui-widgets` `floater::tests` (a second subject opens its
own cascaded window in front, the same subject is reused and raised, a close
ends one window and leaves the other, a keyed kind has no singleton for the
by-id openers, and only a named instance gets its own settings key), and
`sl-viewer-people` `avatar_profile::tests::instances` (two residents are two
windows with their own subjects and handles; closing one leaves the other).

Live (aditi, 2026-09-06, release build): two residents opened two windows, the
second cascaded off the first and in front of it, each showing its own
resident; closing one left the other untouched; re-opening a resident already
on screen raised that window rather than duplicating it.

Reference (Firestorm, read-only): `llfloaterreg` (name + key instance
registry), `llpanelprofile` / `llfloaterprofile` (opened per agent id),
`llfloater.cpp` (`getControlName`, `applyRectControl`, `stackWith`).
