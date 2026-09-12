---
id: server-fake-grid-agent-experiences
title: Fake grid — the agent's own five experience lists
topic: server
status: done
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

## What landed (2026-09-12)

**The second hook, not the config field.** `Scenario` grew `setup_for_agent:
Option<AgentHook>`, an `Fn(&mut SimSession, &AvatarIdentity, Instant)` run by
`prepare_region_session` immediately after `setup`, with the identity the login
was matched to. A hook rather than a `RegionConfig` / `AccountConfig` field
because the other three seeding surfaces are hooks, and because what has to
happen here is *building records*, not naming ids a config could hold. It runs
for every session — login, teleport destination, child — so a fixture's
statement about the agent does not stop at the region line.

**Nothing is stamped twice.** The catalogue splits rather than being corrected:
`catalogue()` is what can be written down without an identity (owned by the
fixture resident), and `agent_catalogue(agent)` builds the agent's own two
records *with* the real owner. A record that spends any time naming the wrong
owner is a record a fetch can catch naming the wrong owner — and the grid
answers `GetExperienceInfo` from the same store the moment the circuit opens.

**The five lists are seeded distinct**, which is the point of seeding them at
all: the agent owns two experiences, administers those two **and** the
group-owned arena, and contributes to one it neither owns nor administers. A
fixture where owned == admin == contributor cannot show that a viewer wired
each tab to its own capability. Two further shapes are deliberate — one of the
agent's own records is private (the Owned tab lists what search will not), and
the blocked list holds the fixture resident's private record (a preference is
the agent's own keyed entry, not a search result). `set_group` for the arena's
group went to the catalogue half, where it belongs: a group's list is a
statement about the group, not about whoever logged in.

**The timeline exercise** is `Action::SetExperiencePreference { experience_id,
permission }`, over a now-public `SimExperiences::set_preference` — the
single-id counterpart to `set_agent_permissions`' wholesale replacement. It
sends nothing, and says so in its docs: the protocol has no message that tells
a viewer its own preferences moved, because the only thing that moves them is
that viewer. So the action stands for the agent having decided somewhere else,
and the round trip a floater's Allow button makes is driven from the *client*
side — which is what the new end-to-end test does: five lists back, the owned
records resolved through `GetExperienceInfo` and checked to name the agent,
then one `SetExperiencePermission` and the grid's own reply showing the id
moved across. The action itself has its own timeline test: it sends nothing,
so the script marks itself with a `Marker` and the claim is the one a re-opened
floater makes — the *next* `GetExperiences` answers differently, with the
scripted id joining the seeded blocked list rather than replacing it.

`FixtureSet::into_scenario` wires the same hook, for the same reason its
`setup` layers on `default_setup`: a fixture describes a region's objects, not
what the account logging in owns.
