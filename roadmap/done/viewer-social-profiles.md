---
id: viewer-social-profiles
title: Avatar profiles — picks / classifieds
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Avatar **profiles**: the second-life / web tabs plus **picks** and
**classifieds**, shown for any avatar and editable for one's own. Hosted in a
floater ([[viewer-ui-widget-scaffold]]).

The profile / picks / classifieds protocol already exists; this task is the
panel that renders and edits it.

Reference (Firestorm, read-only): `llfloaterprofile`, `llpanelpicks`.

Builds on: the profile / picks / classifieds model.

## Shipped (2026-07-22)

`sl-client-bevy-viewer/src/avatar_profile.rs`: the profile floater with the
reference's tab set — 2nd Life / Web / Picks / Classifieds / 1st Life /
Notes — opened from the avatar pie's **Profile** slice (self and other, both
now live) and a new **Profile** action button in the People list.

- **2nd Life**: name, key, profile picture, online status, birthdate,
  account caption (charter-member decode) + payment info, partner, groups,
  about text; for one's own profile the about text edits with Save / Discard
  and the "Show in search" (`allow_publish`) toggle; for another avatar the
  reference's action row — Instant Message, Offer Teleport, Add / Remove
  Friend (by friendship state), Block, and Pay (amount field →
  `SendMoneyTransfer`, Gift).
- **Web**: shows / edits the profile URL. The reference renders the feed in
  an embedded browser; [[viewer-profile-web-tab-browser]] upgrades this tab
  once CEF lands.
- **Picks**: left tab-strip list, detail with snapshot / name / description /
  location, Teleport; own-profile New (create at current location, ≤ 10) /
  Delete / edit + Save Pick / Set Location.
- **Classifieds**: same list shape; detail with snapshot, category, content
  type, auto-renew, price, creation date; own-profile New (draft editor with
  price, Publish / Cancel), edit (category / content-type cycles, auto-renew
  toggle) / Save / Delete / Set to Current Location.
- **1st Life**: picture + about text (editable own).
- **Notes**: private notes with Save (`UpdateAvatarNotes`).

Dropped by decision: the **Interests** tab (the reference itself no longer
has one — `AvatarInterestsReply` is a null handler there). Greyed
placeholders, matching the pie-menu convention: Find on Map / Show on Map
(needs the world map floater), Invite to Group (needs a group/role picker).
Profile / pick / classified images display but are not editable
([[viewer-profile-image-editing]], blocked on
[[viewer-ui-texture-picker]]); saves keep the existing image ids.

Infrastructure that landed with it (live-test findings): the floater is
resizable, which needed a tab-widget **fill mode**
(`ui_tab::fill_tab_container` — container/strip/panels track a
definite-size parent, per-panel wheel scroll + trailing scrollbar shown
only on overflow of the visible panel); and subject-bound floaters are now
**exempt from floater persistence** (`floater_persist::FloaterPersistExempt`
on the profile, item-properties and Open-preview roots — a restored "open"
without its subject is an empty shell).
