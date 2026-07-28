---
id: viewer-clickable-name-widgets
title: Reusable clickable avatar-name / group-name widgets
topic: viewer
status: ready
origin: user request (2026-07-28) — noticed while adding owner links to the
  About Region floater that name resolution + click-to-profile is reimplemented
  per floater
---

Context: [context/viewer.md](../context/viewer.md).

Clickable avatar and group **names** appear all over the viewer UI, and every
site currently re-implements the same two concerns:

1. **Name resolution** — look the id up in `AvatarState::name_of` /
   `GroupsModel::group_name`, fall back to `(id)`, and re-resolve in place when
   the name cache changes.
2. **Click → profile** — open `avatar_profile::OpenAvatarProfile` /
   `group_profile::OpenGroupProfile` on press.

Build two small reusable widgets (an `avatar_name` and a `group_name` widget, or
one parameterised by owner kind) that own both concerns: spawn a node bound to
an `Option<AgentKey>` / `Option<GroupKey>`, keep the label in step with the name
cache, tint it as a link only when set, and open the right profile on click.
Include the "unset → plain, non-clickable" behaviour.

**Owner (avatar-or-group) variant.** Many places carry an owner that may be
*either* an avatar or a group — an `OwnerKey` (`OwnerKey::Agent` /
`OwnerKey::Group`), e.g. parcel/object owners. The widget set must cover this:
bind to an `Option<OwnerKey>`, resolve the name from the matching cache
(`AvatarState` for an agent, `GroupsModel` for a group), and open the right
profile on click (`OpenAvatarProfile` vs `OpenGroupProfile`). Ideally the
avatar-only and group-only widgets are just the two concrete cases of this
owner-kind-aware widget so there is a single resolution + click path.

**Optionality is first-class for all three bindings.** Every binding is an
`Option` — `Option<AgentKey>`, `Option<GroupKey>`, `Option<OwnerKey>` — and the
`None` / unset case is a supported, in-place state, not a caller precondition:
it renders the configured "unset" label (e.g. `(none)`) in the plain, non-link
colour and is non-clickable, and flips to a live link the moment a value is
bound. This is exactly the About Region owner case (a nil id maps to `None`),
so the migration there is a drop-in.

**Three display states, not two.** "We don't know the owner yet" (the reply is
still in flight) must be distinguishable from "there is genuinely no owner":

- **Loading** — data not yet received: show `(loading)`, non-clickable.
- **Unset** — known to have no owner: show `(none)`, non-clickable.
- **Set(key)** — a real owner: show the resolved name (or `(id)` until the name
  cache resolves), tinted as a clickable link.

Model this as a small tri-state (e.g. `Loading` / `None` / `Some(key)`) rather
than a bare `Option`, so callers that have not yet fetched the id don't have to
misuse `None` to mean "loading". Both non-clickable labels are configurable
per call site.

Then **migrate the existing bespoke implementations** onto them:

- `about_region.rs` — `OwnerLink` + `on_owner_link` + `set_owner_link`
  (Region / Estate / Covenant owner names). Delete these once migrated.
- `about_land.rs` — `spawn_link_button` with `AboutLandAction::OpenOwner` /
  `OpenGroup` (parcel owner + group names).
- Other name sites worth auditing: chat transcript sender names, the People /
  friends list, group member/role lists, IM participant names, the minimap /
  nearby-avatar labels.

Reference (Firestorm, read-only): the `LLNameEditor` / `LLAvatarName` name
widgets and the common "click a resident/group name → profile" behaviour.
