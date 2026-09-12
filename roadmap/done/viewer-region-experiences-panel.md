---
id: viewer-region-experiences-panel
title: Region / Estate floater — Experiences tab
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-estate, viewer-experiences-floater,
  viewer-parcel-config-missing-writes, protocol-27,
  viewer-region-experiences-default-experience,
  viewer-audit-picker-requester-identity]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Region/Estate Experiences panel
(LLPanelRegionExperiences) manages the estate's three experience lists —
Trusted, Allowed, Blocked — with add via an experience picker and
per-row remove, plus read-only captions explaining where each list
applies.

Our About Region Experiences tab is a permanently-disabled placeholder
(`sl-client-bevy-viewer/src/about_region.rs`; its module doc claims the
write path "is its own roadmap item", but until now no task covered the
region half). The protocol is fully paired in sl-proto: the
`RegionExperiences` cap with `RequestRegionExperiences` /
`SetRegionExperiences` (`sl-proto/src/session.rs`) and
`Event::RegionExperiences` (`sl-proto/src/event.rs`), delivered by
[[protocol-27]]. Implementing this means building the three-list tab
over those commands, estate-manager gated, reusing whatever experience
picker [[viewer-experiences-floater]] grows. The parcel-side experience
lists stay with [[viewer-parcel-config-missing-writes]] (no per-parcel
message in our scope enum yet).

**The picker now exists** (2026-09-11): [[viewer-experiences-floater]]'s Search
tab is `FindExperiences` over a `ui_table`, and its rows open
`experience_profile`'s keyed window. Reuse that shape rather than building a
second one — the three estate lists are the same rows with an Add that takes
the search tab's selection, and `experience_profile::maturity_key` / the
`experiences-col-*` Fluent keys are already there to share.

Reference (Firestorm, read-only):
`indra/newview/llfloaterregioninfo.cpp` (LLPanelRegionExperiences),
`indra/newview/skins/default/xui/en/panel_region_experiences.xml`.

## What landed (2026-09-12)

**A picker window, not a second search.** The reference does not reuse the
Experiences floater's Search *tab*: it embeds the same panel class
(`LLPanelExperiencePicker`) in a second window, `LLFloaterExperiencePicker`,
because an Add has to answer a caller, and a tab in another floater cannot.
So the search machinery moved into `sl-viewer-notices/src/experience_search.rs`
— the paging state, the rating/name/owner column set, the row rendering, the
sort and the picker filters — and both surfaces are now windows over it:
`experiences_floater`'s Search tab as before, and the new
`experience_picker.rs`, "Choose Experience", opened by
`OpenExperiencePicker { requester, filter }` and answering
`ExperiencePicked { requester, experience, name }`. Both messages live in
`sl-viewer-world-api` beside `OpenAvatarPicker` / `AvatarPicked`, so a list
wanting an experience is written the same way as a list wanting a resident.
Registered in the `FLOATERS` sweep as `experience-picker`.

**The picker is keyed, one window per requester.** It started as a singleton,
like the avatar picker, and the live check found both ways that is wrong: it
came back **open at the next login** (a singleton's visibility is persisted
floater geometry), and pressing Add on a second list *replaced* the first
list's picker instead of opening its own. So it is a subject-keyed
(`FloaterKey::subject`) instance keyed by the requester tag — the reference
goes further still and mints a fresh key per Add
(`LLPanelExperienceListEditor::onAdd`'s `mKey.generateNewID()`), marking the
previous picker dead. Keying buys three things at once: a subject-keyed
floater persists nothing, so no transient dialog reopens itself over a world
nobody asked it about; every per-window datum (the query, the page, the
filter, the resolved metadata) is a component that dies with the window, so a
re-open cannot confirm a row a previous open's filter admitted; and two lists
can have their pickers up at once.

That last one needed a matching fix on the panel: the Add claim was the single
`pending_pick` slot the avatar picks use, and with two pickers open at once it
named whichever list was opened last — the *other* list's pick would arrive and
be dropped. The experience claims are a per-list set instead.

**The filters are the reference's three predicates, named.**
`ExperiencePickerFilter::{Any, LandScoped, GridScopedUnprivileged}` — Key takes
anything, Allowed refuses a grid-scoped experience (one that already runs
everywhere), Blocked takes only grid-scoped and never a privileged one (which
cannot be refused at all). The reference passes a `boost::bind` predicate
vector; naming the three keeps the *reason* at the call site. An id the
metadata cache has not resolved yet **passes every filter**, because the
reference reads properties off a cached record and a miss leaves the row in —
hiding unresolved rows would make the visible result set depend on reply order.

Allowed and Blocked are therefore kept apart by **scope**, not by a duplicate
check: an experience is either grid-scoped or it is not, so nothing can be
offered to both. Key overlaps both on purpose — any experience may be Key, and
the reference filters that list not at all. A test checks the disjointness over
every combination of the property bits rather than trusting two `match` arms to
stay opposite.

That property had one hole, found by asking the question rather than by running
anything: a **`missing`** record (the `DoesNotExist` placeholder a cap returns
for an id it cannot resolve) was listed in the results but never folded into the
metadata cache — and an unresolved record is admitted by every filter by design,
so that placeholder was the one thing that could reach both pickers, under a
short-id label since it has no name either. Search results now drop the missing
records (`page_results`), which is also what the reference's picker lists.

**The tab.** Three bounded tables (name / rating / row actions) under the
estate-wide caption and each list's own caption, an Add per list, a Profile and
a Remove per row. Estate-manager gated the way the Access tab is: Add hides
with the other write buttons, Remove hides per row, Profile stays — reading an
experience's page is not an estate write. The `RegionExperiences` GET goes out
once per window per stay in its region (re-armed when the agent walks back in),
and only while the window is current *and* managing: the cap is estate-gated
region-side, and a refusal is not data. Names and ratings come from
`RequestExperienceInfo` for ids the window has no record of, asked once each.

**The write posts all three lists.** The cap replaces the whole set rather than
taking a delta, so every add and every remove sends
`SetRegionExperiences { allowed, blocked, trusted }` — which is what the
reference's `sendUpdate` does too — and the reply replaces the lists, so an
edit the region refused visibly reverts. An Add onto a list already holding
`ESTATE_MAX_EXPERIENCE_IDS` (8) refuses before opening a picker whose pick it
would have to drop.

**Fixture.** `sl-fake-grid`'s experience catalogue now seeds the region's three
lists one deep and distinct (arena / trial 02 / weather), each eligible for the
list it is in under that list's own picker filter — three identical tables
seeded equal could not show that each is wired to its own slot of the reply.

### Opening a window now raises it — for every floater

Live-checking this tab turned up the recurring bug one more time: the picker
opened **behind** the About Region window it was opened from. The cause is
general, not this feature's — a press on the Add button raises the window it
landed in (the floater root observer's `BringToFront`), and a feature that only
flips the other window's `UiPanelShown` therefore puts it underneath. Several
features had learned to write their own raise; several had not.

So the raise moved into the manager: `raise_floaters_on_open` in
`sl-viewer-ui-widgets/src/floater.rs` raises and activates any floater that goes
hidden → shown, whoever made it visible. It runs in `PostUpdate` before the UI
stack pass, so it sees every flip `Update` made and no feature can be scheduled
"too late" to be raised. It watches a **transition**, not Bevy's change flag: a
`FloaterWasShown` component holds the last value, because several features
mirror a toggle's state into `UiPanelShown` unconditionally every frame and
raising on change would pin such a window in front for as long as it stayed
open. Two tests in that file pin both halves.

The existing per-feature raises stay: opening a window that is *already* shown
is not an edge this pass can see, and that is an ordinary thing to do (press Add
on a second list without closing the picker). Same reason `KeyedFloaters` keeps
raising directly.

Two more layout fixes from the same live check: the three experience tables were
squeezed to a header and half a row (a fixed height still shrinks — the wrappers
now refuse to, and the tab panel scrolls instead), and the paging buttons broke
their labels across two lines with the arrow alone on the first (the action rows
now wrap whole buttons rather than squeezing them — fixed in the Experiences
floater's search tab too, which had the same row).

### Divergences from the reference, deliberate

- **No `estateexperiencedelta`.** The reference writes the incremental estate
  message as well as the cap, behind a "this estate / all estates / cancel"
  confirmation. Neither that message nor an all-estates scope exists in our
  protocol surface; the cap POST alone is what this task specified, and it is
  what the reference's own `sendUpdate` path uses.
- **No default experience.** The reference reads the reply's `default` key and
  pins that experience into the Key list as a non-removable row. Our decoded
  `Event::RegionExperiences` carries only the three arrays, so the row is
  simply absent — the round-trip still preserves whatever the grid sent.
  Filed as [[viewer-region-experiences-default-experience]].
- **No acquire / purchase.** As on the Owned tab, there is no purchase command
  in our protocol surface.

### Known edge

The picker is keyed by requester *tag*, so it is one window per list rather than
per press. Two About Region windows — one per region, which is the point of
keying that floater — pressing Add on the same list share one picker, and since
each window's claim is its own per-list flag, both take the pick. That is a
property of the picker contract rather than of this panel (the avatar pickers
here and in About Land have the same shape, and worse, a single claim slot), and
it is filed as [[viewer-audit-picker-requester-identity]].
