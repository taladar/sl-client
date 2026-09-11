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
//! results is structurally unable to test. Hence [`FILLER_COUNT`]: enough
//! records sharing one word to spill a query onto a second page, and one more
//! than a whole page rather than two whole pages, so the last page is short and
//! the boundary is visible from both sides.
//!
//! # What is deliberately *not* here
//!
//! The agent's own five relationships — allowed, blocked, owned, admin,
//! contributor — are left empty. An experience record states its **owner**, and
//! the scenario setup hook runs before the circuit is open, so
//! [`SimSession::agent_id`](sl_proto::SimSession::agent_id) is still `None`
//! there: a fixture claiming the agent owns an experience would have to name
//! some other agent as its owner, which is the one thing an "owned by you" list
//! must not do.

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

/// The `General` content rating — a `sim_access` code, which is how
/// [`ExperienceInfo::maturity`] states a rating.
const MATURITY_PG: i32 = 13;

/// The `Moderate` content rating, as a `sim_access` code.
const MATURITY_MATURE: i32 = 34;

/// The [`ExperienceKey`] for a catalogue offset.
fn experience_key(offset: u128) -> ExperienceKey {
    ExperienceKey::from(Uuid::from_u128(EXPERIENCE_ID_BASE.saturating_add(offset)))
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
/// record (grid-wide, privileged, group-owned, private) and [`FILLER_COUNT`]
/// identical-but-numbered trials that make a search page.
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
            public_id: experience_key(1),
            name: "Fake Grid Tour".to_owned(),
            owner: agent_owner,
            description: "A guided walk around the region, as an experience.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID),
            quota: 128,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(2),
            name: "Fake Grid Weather".to_owned(),
            owner: agent_owner,
            description: "Pushes a sky at anyone who agrees to let it.".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID | PROPERTY_PRIVILEGED),
            quota: 256,
            maturity: MATURITY_PG,
            ..ExperienceInfo::default()
        },
        ExperienceInfo {
            public_id: experience_key(3),
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
            public_id: experience_key(4),
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
            public_id: experience_key(0x100_u128.saturating_add(n)),
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

/// Seeds [`catalogue`] into a session's experience store, along with
/// the display name of the fixture resident that owns them — so the viewer's
/// Owner column resolves to a name rather than to a raw id.
pub fn seed_catalogue(sim: &mut SimSession) {
    for record in catalogue() {
        sim.experiences_mut().insert(record);
    }
    sim.set_display_name(owner_identity().display_name_record());
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_wire::SEARCH_PAGE_SIZE;

    use super::*;

    /// Every record has its own id: a catalogue with a collision would serve
    /// one record where the fixture states two, silently.
    #[test]
    fn every_record_has_its_own_id() {
        let records = catalogue();
        let mut ids: Vec<_> = records.iter().map(|info| info.public_id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two catalogue records share an id");
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
}
