---
id: gridspec-experiences
title: Experience records, permissions and admin on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-experience,
  viewer-refused-capability-silent-drops, server-fake-grid-estate-actions,
  viewer-experience-profile-extended-metadata]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Stock OpenSim ships no experience module; none of the experience cases ran on
aditi. The fake grid serves experiences on its Second Life flavour only: the
OpenSim flavour's seed withholds every experience capability
(`OPENSIM_REFUSED_CAPABILITIES` in `sl-fake-grid/src/imitates.rs`, from
[[gridspec-seed-capabilities]]).

## Discover

The three experience cases on aditi (an experience needs creating by the user in
Firestorm or the web UI).

## Document

`book/src/gridspec/experiences.md`.

## Fake grid

The OpenSim flavour's gating is done (above). What remains is holding the
Second Life flavour's answers to what the aditi cases record.

## Viewer

Experience floater hidden without caps.

## Done (2026-10-16)

**Discover**: three `sl-repl` wire probes on aditi read every experience
capability's raw reply (records, search paging, preferences, relationship,
group, region and parcel lists, an unknown id, and the refusals of an update
and an estate POST by an avatar without rights); the three cases then ran on
aditi (fixture: a stranger's public experience, set in the gitignored
`fixtures.aditi.toml`), on OpenSim and on both fake flavours. The cases now
record the capability's presence against the flavour
(`experience_capabilities`, held: Second Life grants, OpenSim does not)
instead of marking OpenSim `partial`, and `experience-info` records the
record's properties, group and kind of home location. The fake grid supplies
its own `experience` fixture (`sl_fake_grid::experiences::stranger_experience`),
and the three cases joined the offline list on both flavours.

Findings beyond the plan:

- **A search page holding one record whose `slurl` is the word `last`** —
  about one aditi record in five — **failed to decode as a whole**, as did a
  `GetExperienceInfo` naming one: on Second Life experience search showed
  nothing for most queries. `slurl` is now an `ExperienceLocation`
  (`Last` / `Home` / `Slurl(url)`), shown as the word in the profile.
- A record names its **owner and its group as two fields**; ours folded them
  into one and dropped the group of every group experience. `ExperienceInfo`
  now has `owner: Option<AgentKey>` and `group: Option<GroupKey>`.
- `RegionExperiences` sends `default` on every reply, the **nil id** when the
  estate has none; we read that as a default named nil.
- Second Life search **lists private** (and disabled, suspended) experiences
  and clamps a page outside the results into them; the fake grid hid private
  ones and answered such a page empty.
- Refusals are plain text: `UpdateExperience` by a non-administrator is a 403
  with a sentence, a non-manager's `RegionExperiences` POST a 405. Our POST
  helper threw status and body away and did not check the status; both
  runtimes now log a refused or unreadable POST, and the three experience GET
  helpers log a reply they cannot decode instead of dropping it silently.

**Document**: `book/src/gridspec/experiences.md`, linked from
`content/experiences.md`.

**Fake grid**: the Second Life flavour now writes the replies in aditi's shape
(`<undef/>` for an empty preference or region list, the nil `default`, a
`disabled` key, string `error_ids`), lists private experiences, clamps search
pages and numbers the paging markers from the requested page, refuses an
update by a non-administrator with Second Life's 403, and its catalogue
carries a `last` location, a SLURL and a group beside an owner. The OpenSim
flavour's gating was already done. Not imitated: the non-manager's 405 (no
estate rights are modelled — noted on [[server-fake-grid-estate-actions]]),
`total_pages` (the reference never reads it), and the grid's own result order.

**Viewer**: the decode fixes above; the profile shows a **Group** row when the
record names one (hidden otherwise, as the reference does) and the location
words; the search and list tables' owner column names the owning resident, as
the reference's picker does. Unit tests cover the aditi shapes; the e2e stage
`an_experience_profile_shows_its_location_word_and_its_group` holds the
profile to the fake grid's `last` record and group experience. The floater is
not hidden on a grid without the capabilities — the reference does not hide it
either — and its silent wait there stays with
[[viewer-refused-capability-silent-drops]]. Saving a profile now sends the
record's `group_id` back (the nil id for none), as the reference's save does;
it sent none before, which Second Life might have read as "no group" — not
measurable without an administrator.

### What could not be measured

- The owner's and administrator's side of every capability — no aditi avatar
  owns or administers an experience, and creating one needs Premium.
- `RegionExperiences` as an estate manager, and an estate's real default.
- Whether a parcel with no experience list of its own admits only the
  estate's (the fake grid admits everything there).
