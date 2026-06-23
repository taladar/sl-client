//! Experience capability fetches.

use crate::http::blocking_get_llsd;
use crossbeam_channel::Sender;
use sl_proto::Event as SessionEvent;
use sl_proto::{ExperienceKey, GroupKey, parse_experience_ids, parse_experience_status};

/// GETs the `GroupExperiences` capability and forwards an
/// [`SlSessionEvent::GroupExperiences`] over `asset_tx`, echoing the queried
/// `group_id` (the cap reply does not carry it).
pub(crate) fn run_group_experiences(
    url: &str,
    group_id: GroupKey,
    asset_tx: &Sender<SessionEvent>,
) {
    if let Some(llsd) = blocking_get_llsd(url) {
        asset_tx
            .send(SessionEvent::GroupExperiences {
                group_id,
                experience_ids: parse_experience_ids(&llsd),
            })
            .ok();
    }
}

/// GETs an `IsExperienceAdmin` (`admin` true) or `IsExperienceContributor`
/// (`admin` false) capability and forwards the corresponding status event over
/// `asset_tx`, echoing the queried `experience_id`.
pub(crate) fn run_experience_status(
    url: &str,
    experience_id: ExperienceKey,
    admin: bool,
    asset_tx: &Sender<SessionEvent>,
) {
    let Some(llsd) = blocking_get_llsd(url) else {
        return;
    };
    let status = parse_experience_status(&llsd);
    let event = if admin {
        SessionEvent::ExperienceAdminStatus {
            experience_id,
            is_admin: status,
        }
    } else {
        SessionEvent::ExperienceContributorStatus {
            experience_id,
            is_contributor: status,
        }
    };
    asset_tx.send(event).ok();
}
