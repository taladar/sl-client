//! The experience catalogue the experience capabilities answer from.
//!
//! Experiences are a Second Life feature — stock OpenSim ships no experience
//! module — so a viewer's Experiences floater has, until now, had exactly one
//! grid it could be pointed at, and that grid costs an account, a login and a
//! network. This module is the offline other one: a small set of records
//! [`SimExperiences`](sl_proto::SimExperiences) serves to `GetExperienceInfo`,
//! `FindExperienceByName` and `UpdateExperience`.
//!
//! # Why it is bigger than four records
//!
//! `FindExperienceByName` is **paged**, and the reply says whether there is a
//! page on either side of the one it carries (`next_page_url` /
//! `previous_page_url`). A catalogue small enough to fit one page can never
//! make the grid say *yes* to that question, so it can never exercise the
//! viewer's paging arrows — the one part of a search UI that a single page of
//! results is structurally unable to test. Hence the filler records: enough
//! sharing one word to spill a query onto a second page, and one more than a
//! whole page rather than two whole pages, so the last page is short and the
//! boundary is visible from both sides.
//!
//! # Why it is seeded in two halves
//!
//! An experience record states its **owner**, and the agent's five
//! relationships — allowed, blocked, owned, admin, contributor — are statements
//! about *who is logged in*. The scenario's `setup` hook runs before the
//! circuit is open, so [`SimSession::agent_id`](sl_proto::SimSession::agent_id)
//! is still `None` there: a fixture claiming the agent owns an experience would
//! have to name some other agent as its owner, which is the one thing an "owned
//! by you" list must not do.
//!
//! So [`catalogue`] is the half that can be written down without an identity —
//! the grid's own records, owned by a fixture resident — and
//! [`seed_for_agent`] is the half that names the agent, run from the
//! scenario's [`setup_for_agent`](crate::scenario::Scenario::setup_for_agent)
//! hook once the login has been matched to an account. Nothing is ever stored
//! under a placeholder owner and corrected afterwards: the records the agent
//! owns are built at the moment the owner is known.
//!
//! The five lists are seeded *distinct*, because three of the floater's tabs
//! are otherwise indistinguishable: the agent owns two experiences, administers
//! those two **and** a group's, and contributes to one it neither owns nor
//! administers. A fixture where owned == admin == contributor cannot show that
//! a viewer has wired each tab to its own capability.

use sl_proto::{ExperienceInfo, ExperienceProperties, SimSession};
use sl_types::key::{AgentKey, ExperienceKey, GroupKey, OwnerKey};
use sl_wire::{PROPERTY_GRID, PROPERTY_PRIVATE, PROPERTY_PRIVILEGED, PROPERTY_SUSPENDED};
use uuid::Uuid;

use crate::world::AvatarIdentity;

/// The id base of every catalogue record, picked far from the other fixture
/// families' bases so a stray id collision is visible rather than plausible.
const EXPERIENCE_ID_BASE: u128 = 0x00E4_0000;

/// The id of the agent that owns the catalogue's agent-owned experiences — a
/// fixture resident, not the logged-in agent (see the module docs).
const EXPERIENCE_OWNER_ID: u128 = 0x00E4_00A0;

/// The id of the group that owns the catalogue's group-owned experience.
const EXPERIENCE_GROUP_ID: u128 = 0x00E4_00B0;

/// The number of filler records, one more than a full search page so a query
/// that matches them all spills onto a short second page (see the module docs).
const FILLER_COUNT: u128 = 31;

/// The catalogue offset of the grid-wide walking tour — the experience the
/// agent has admitted and contributes to without owning or administering it.
const OFFSET_TOUR: u128 = 1;

/// The catalogue offset of the privileged sky-pushing experience — the one an
/// `llSetEnvironment` push comes from, which the agent has therefore admitted.
const OFFSET_WEATHER: u128 = 2;

/// The catalogue offset of the group-owned arena — the experience the agent
/// administers but does not own, which is what makes Admin ≠ Owned.
const OFFSET_ARENA: u128 = 3;

/// The catalogue offset of the private, suspended workshop — owned by the
/// fixture resident and blocked by the agent.
const OFFSET_WORKSHOP: u128 = 4;

/// The catalogue offset the numbered trials start one past.
const OFFSET_FILLER_BASE: u128 = 0x100;

/// The catalogue offset of the agent's own public experience — the one an
/// administrator can edit from the profile window.
const OFFSET_AGENT_STUDIO: u128 = 0x20;

/// The catalogue offset of the agent's own private experience — owned, and
/// therefore listed, but hidden from search.
const OFFSET_AGENT_DRAFTS: u128 = 0x21;

/// The `General` content rating — a `sim_access` code, which is how
/// [`ExperienceInfo::maturity`] states a rating.
const MATURITY_PG: i32 = 13;

/// The `Moderate` content rating, as a `sim_access` code.
const MATURITY_MATURE: i32 = 34;

/// The [`ExperienceKey`] for a catalogue offset.
fn experience_key(offset: u128) -> ExperienceKey {
    ExperienceKey::from(Uuid::from_u128(EXPERIENCE_ID_BASE.saturating_add(offset)))
}

/// The [`ExperienceKey`] of the `n`-th numbered trial, counting from 1.
fn filler_key(n: u128) -> ExperienceKey {
    experience_key(OFFSET_FILLER_BASE.saturating_add(n))
}

/// The fixture resident every catalogue record that is not group-owned belongs
/// to.
fn owner_identity() -> AvatarIdentity {
    AvatarIdentity::new(
        AgentKey::from(Uuid::from_u128(EXPERIENCE_OWNER_ID)),
        "Experience",
        "Fixture",
    )
}

/// The catalogue's records: four hand-written ones covering the corners of the
/// record (grid-wide, privileged, group-owned, private) and a run of
/// identical-but-numbered trials, one longer than a page, that make a search
/// page (see the module docs).
///
/// The private one is here to be *not* found: `SimExperiences::find` hides
/// private records from the search surface as the grid does, and a fixture with
/// nothing hidden cannot show that.
#[must_use]
pub fn catalogue() -> Vec<ExperienceInfo> {
    let agent_owner = Some(OwnerKey::Agent(AgentKey::from(Uuid::from_u128(
        EXPERIENCE_OWNER_ID,
    ))));
    let mut records = vec![
        ExperienceInfo {
            public_id: experience_key(OFFSET_TOUR),
            name: "Fake Grid Tour".to_owned(),
            owner: agent_owner,
            description: "A guided walk around the region, as an experience.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID),
            quota: 128,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(OFFSET_WEATHER),
            name: "Fake Grid Weather".to_owned(),
            owner: agent_owner,
            description: "Pushes a sky at anyone who agrees to let it.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID | PROPERTY_PRIVILEGED),
            quota: 256,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(OFFSET_ARENA),
            name: "Fake Grid Arena".to_owned(),
            owner: Some(OwnerKey::Group(GroupKey::from(Uuid::from_u128(
                EXPERIENCE_GROUP_ID,
            )))),
            description: "A group-owned experience, rated Moderate.".to_owned(),
            properties: ExperienceProperties(0),
            quota: 64,
            maturity: MATURITY_MATURE,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(OFFSET_WORKSHOP),
            name: "Fake Grid Workshop".to_owned(),
            owner: agent_owner,
            description: "Private and suspended: search must not list this one.".to_owned(),
            properties: ExperienceProperties(PROPERTY_PRIVATE | PROPERTY_SUSPENDED),
            quota: 0,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
    ];
    for n in 1..=FILLER_COUNT {
        records.push(ExperienceInfo {
            public_id: filler_key(n),
            name: format!("Fake Grid Trial {n:02}"),
            owner: agent_owner,
            description: "One of the numbered trials a paged search runs over.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID),
            quota: 16,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        });
    }
    records
}

/// The records the **logged-in agent** owns: one public and one private.
///
/// Built here, from the identity, rather than listed in [`catalogue`] with a
/// placeholder owner to be corrected later — a record that spends any time
/// naming the wrong owner is a record a fetch can catch naming the wrong owner.
///
/// The private one is the pair to the fixture resident's: an owner sees their
/// own unpublished experience in the Owned tab, and search still does not list
/// it. A tab that only ever shows what search shows cannot demonstrate that it
/// is reading `AgentExperiences` rather than filtering a search.
#[must_use]
pub fn agent_catalogue(agent: &AvatarIdentity) -> Vec<ExperienceInfo> {
    let owner = Some(OwnerKey::Agent(agent.agent_id));
    vec![
        ExperienceInfo {
            public_id: experience_key(OFFSET_AGENT_STUDIO),
            name: "Fake Grid Studio".to_owned(),
            owner,
            description: "Yours: the one the profile window lets you rename.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID),
            quota: 128,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(OFFSET_AGENT_DRAFTS),
            name: "Fake Grid Drafts".to_owned(),
            owner,
            description: "Yours and private: listed as owned, never by search.".to_owned(),
            properties: ExperienceProperties(PROPERTY_PRIVATE),
            quota: 8,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
    ]
}

/// Seeds [`catalogue`] into a session's experience store, along with
/// the display name of the fixture resident that owns them — so the viewer's
/// Owner column resolves to a name rather than to a raw id — and the one group
/// list the catalogue can state (`GroupExperiences` for the arena's group).
///
/// The half of the fixture that names the agent is
/// [`seed_for_agent`].
pub fn seed_catalogue(sim: &mut SimSession) {
    for record in catalogue() {
        sim.experiences_mut().insert(record);
    }
    // A group's experience list is a statement about the group, not about
    // whoever is logged in, so it belongs on this side of the split.
    sim.experiences_mut().set_group(
        Uuid::from_u128(EXPERIENCE_GROUP_ID),
        vec![experience_key(OFFSET_ARENA)],
    );
    sim.set_display_name(owner_identity().display_name_record());
}

/// Seeds the agent's own five experience relationships — allowed, blocked,
/// owned, admin, contributor — and the records [`agent_catalogue`] says it
/// owns, under the identity the login was matched to.
///
/// Every list is non-empty and no two are equal, so each of the floater's tabs
/// shows something different; the shape is documented in the module docs. The
/// blocked list carries the fixture resident's *private* experience, because a
/// preference is the agent's own keyed entry rather than a search result —
/// blocking something search will not show you is a thing a viewer can do, and
/// a list that only ever holds searchable ids cannot prove it round-trips.
pub fn seed_for_agent(sim: &mut SimSession, agent: &AvatarIdentity) {
    let mut owned = Vec::new();
    for record in agent_catalogue(agent) {
        owned.push(record.public_id);
        sim.experiences_mut().insert(record);
    }
    // The owner of an experience administers it; the arena is the one the agent
    // administers *without* owning, which is the whole difference between the
    // two tabs.
    let mut admin = owned.clone();
    admin.push(experience_key(OFFSET_ARENA));
    let store = sim.experiences_mut();
    store.set_agent_permissions(
        vec![experience_key(OFFSET_TOUR), experience_key(OFFSET_WEATHER)],
        vec![experience_key(OFFSET_WORKSHOP), filler_key(1)],
    );
    store.set_owned(owned);
    store.set_admin(admin);
    store.set_creator(vec![experience_key(OFFSET_TOUR)]);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_wire::SEARCH_PAGE_SIZE;

    use super::*;

    /// The avatar the agent-owned half of the fixture is seeded under.
    fn test_agent() -> AvatarIdentity {
        AvatarIdentity::new(
            AgentKey::from(Uuid::from_u128(0x00E4_0F01)),
            "Fixture",
            "Resident",
        )
    }

    /// A session seeded the way a login's is: the catalogue, then the agent's
    /// own half — the pair [`crate::scenario`] runs in that order.
    fn seeded_session(agent: &AvatarIdentity) -> SimSession {
        let mut sim = SimSession::new(
            sl_proto::RegionHandle::from_grid(1000, 1000),
            std::time::Instant::now(),
        );
        seed_catalogue(&mut sim);
        seed_for_agent(&mut sim, agent);
        sim
    }

    /// Every record has its own id — across **both** halves, since they share
    /// one store: a catalogue with a collision would serve one record where the
    /// fixture states two, silently, and the agent-owned half is exactly where
    /// an id picked twice would land.
    #[test]
    fn every_record_has_its_own_id() {
        let mut ids: Vec<_> = catalogue()
            .iter()
            .chain(agent_catalogue(&test_agent()).iter())
            .map(|info| info.public_id)
            .collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two catalogue records share an id");
    }

    /// Every record the agent is said to own names the agent as its owner —
    /// the one lie an "owned by you" list must not tell.
    #[test]
    fn the_agents_records_name_the_agent() {
        let agent = test_agent();
        for record in agent_catalogue(&agent) {
            assert_eq!(
                record.owner,
                Some(OwnerKey::Agent(agent.agent_id)),
                "{} is listed as the agent's and owned by somebody else",
                record.name
            );
        }
    }

    /// The filler spills a query onto a second, short page — the property the
    /// whole catalogue size exists for. Checked through the store that serves
    /// it, so the assertion is about the grid's answer and not about the list.
    #[test]
    fn a_search_for_the_trials_pages() {
        let mut store = sl_proto::SimExperiences::default();
        for record in catalogue() {
            store.insert(record);
        }
        let page_size = usize::try_from(SEARCH_PAGE_SIZE).unwrap_or_default();
        let first = store.find("trial", 1);
        assert_eq!(first.infos.len(), page_size);
        assert!(first.has_next_page);
        assert!(!first.has_previous_page);
        let second = store.find("trial", 2);
        assert_eq!(
            second.infos.len(),
            usize::try_from(FILLER_COUNT)
                .unwrap_or_default()
                .saturating_sub(page_size)
        );
        assert!(!second.has_next_page);
        assert!(second.has_previous_page);
    }

    /// The private record is in the catalogue and out of the search results —
    /// the grid hides private experiences from its search surface.
    #[test]
    fn the_private_record_is_not_searchable() {
        let mut store = sl_proto::SimExperiences::default();
        for record in catalogue() {
            store.insert(record);
        }
        let workshop = experience_key(4);
        assert_eq!(
            store.infos(&[workshop]).first().map(|info| info.missing),
            Some(false),
            "the private record should still resolve by id"
        );
        assert!(
            !store
                .find("workshop", 1)
                .infos
                .iter()
                .any(|info| info.public_id == workshop),
            "the private record should not be listed by search"
        );
    }

    /// All five of the agent's lists are seeded, and no two of them are the
    /// same list: five tabs over five capabilities cannot be told apart by a
    /// fixture that answers them all alike.
    #[test]
    fn the_agents_five_lists_are_seeded_and_distinct() {
        let agent = test_agent();
        let sim = seeded_session(&agent);
        let store = sim.experiences();
        let (allowed, blocked) = store.agent_permissions();
        let lists = [
            ("allowed", allowed),
            ("blocked", blocked),
            ("owned", store.owned()),
            ("admin", store.admin()),
            ("contributor", store.creator()),
        ];
        for (name, ids) in &lists {
            assert!(!ids.is_empty(), "the {name} list is empty");
        }
        for (index, (name, ids)) in lists.iter().enumerate() {
            for (other_name, other) in lists.iter().skip(index.saturating_add(1)) {
                assert_ne!(ids, other, "the {name} and {other_name} lists are equal");
            }
        }
    }

    /// The agent owns what it is said to own, administers those **and** the
    /// group's arena, and contributes to one it does neither to — the three
    /// relationships the reference files under three separate tabs.
    #[test]
    fn owned_admin_and_contributor_are_three_different_relationships() {
        let agent = test_agent();
        let sim = seeded_session(&agent);
        let store = sim.experiences();
        let studio = experience_key(OFFSET_AGENT_STUDIO);
        let drafts = experience_key(OFFSET_AGENT_DRAFTS);
        let arena = experience_key(OFFSET_ARENA);
        let tour = experience_key(OFFSET_TOUR);
        assert_eq!(store.owned(), vec![studio, drafts]);
        assert!(
            store.is_admin(arena),
            "the group's arena is not administered"
        );
        assert!(
            !store.owned().contains(&arena),
            "the arena the agent administers must not be one it owns"
        );
        assert_eq!(store.creator(), vec![tour]);
        assert!(
            store.is_contributor(tour) && !store.is_admin(tour),
            "the contributed experience must not also be an administered one"
        );
    }

    /// The agent's own records are in the store the search reads, so the public
    /// one is findable and the private one is not — the Owned tab's two rows
    /// answer differently to the one surface that filters.
    #[test]
    fn the_agents_private_record_is_owned_but_not_searchable() {
        let agent = test_agent();
        let sim = seeded_session(&agent);
        let store = sim.experiences();
        let found: Vec<_> = store
            .find("fake grid", 1)
            .infos
            .iter()
            .map(|info| info.public_id)
            .collect();
        assert!(
            found.contains(&experience_key(OFFSET_AGENT_STUDIO)),
            "the agent's public experience should be findable"
        );
        assert!(
            !found.contains(&experience_key(OFFSET_AGENT_DRAFTS)),
            "the agent's private experience should not be listed by search"
        );
        assert!(
            store.owned().contains(&experience_key(OFFSET_AGENT_DRAFTS)),
            "the agent's private experience should still be owned"
        );
    }

    /// The blocked list holds an id search never offers — a preference is the
    /// agent's own keyed entry, not a search result.
    #[test]
    fn a_blocked_experience_need_not_be_a_findable_one() {
        let agent = test_agent();
        let sim = seeded_session(&agent);
        let (_allowed, blocked) = sim.experiences().agent_permissions();
        let workshop = experience_key(OFFSET_WORKSHOP);
        assert!(blocked.contains(&workshop));
        assert!(
            !sim.experiences()
                .find("workshop", 1)
                .infos
                .iter()
                .any(|info| info.public_id == workshop)
        );
    }

    /// The arena's group answers the arena, and a group nobody declared
    /// answers empty — the `GroupExperiences` pair.
    #[test]
    fn the_group_list_answers_the_arena() {
        let agent = test_agent();
        let sim = seeded_session(&agent);
        assert_eq!(
            sim.experiences()
                .group(Uuid::from_u128(EXPERIENCE_GROUP_ID)),
            vec![experience_key(OFFSET_ARENA)]
        );
        assert_eq!(sim.experiences().group(Uuid::nil()), Vec::new());
    }
}
