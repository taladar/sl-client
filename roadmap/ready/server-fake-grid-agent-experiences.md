---
id: server-fake-grid-agent-experiences
title: Fake grid — the agent's own five experience lists
topic: server
status: ready
origin: split out of [[protocol-experience-search-paging]] when the fake grid
  grew an experience catalogue (2026-09-11)
refs: [protocol-experience-search-paging, viewer-experiences-floater,
  protocol-27]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-fake-grid/src/experiences.rs` seeds a catalogue of experience **records**,
which is what `GetExperienceInfo`, `FindExperienceByName` and
`UpdateExperience` answer from — so the Experiences floater's Search tab, its
paging arrows and the Experience Profile window can all be driven offline.

What is still empty is the agent's own five relationships: allowed, blocked,
owned, admin, contributor (`SimExperiences::set_agent_permissions` /
`set_owned` / `set_admin` / `set_creator`, plus `set_group` for
`GroupExperiences`). The floater's other five tabs therefore come back empty
against the fake grid, and `ExperiencePreferences` has nothing to move between
lists.

**Why it was not simply seeded.** An experience record states its *owner*, and
a scenario's `setup` hook runs in `RuntimeInner::prepare_region_session` before
the circuit is open — `SimSession::agent_id()` is still `None` there. A fixture
that put the catalogue's experiences in the "owned by you" list would have to
name the fixture resident as their owner, so the Owned tab would list
experiences owned by somebody else. That is exactly the lie the tab exists to
expose.

What this needs, then, is somewhere later than `setup` to seed from — the
account's `agent_id` is known to `prepare_region_session` (it builds the
session under the login's identity), so the honest shapes are either a second
hook that runs once the identity is bound, or a `RegionConfig` /
`AccountConfig` field the runtime applies itself after `setup`. Pick one, then
re-stamp the catalogue's agent-owned records with the real agent id and fill
the five lists.

Worth having as well once that exists: a scripted `ExperiencePreferences`
exercise on the scenario timeline, so "allow it, then re-open the floater" is
a thing a test can do without a live grid.
