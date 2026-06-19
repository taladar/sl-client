//! Experience capability fetches (admin/contributor/group).

use crate::http::get_llsd;
use reqwest::Client as ReqwestClient;
use sl_proto::{Event, Uuid, parse_experience_ids, parse_experience_status};
use tokio::sync::mpsc;

/// GETs the `GroupExperiences` capability and forwards an [`Event::GroupExperiences`]
/// over `events`, echoing the queried `group_id` (the cap reply does not carry it).
pub(crate) async fn fetch_group_experiences(
    url: String,
    group_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::GroupExperiences {
                group_id,
                experience_ids: parse_experience_ids(&llsd),
            })
            .await
            .ok();
    }
}

/// GETs the `IsExperienceAdmin` capability and forwards an
/// [`Event::ExperienceAdminStatus`] over `events`, echoing the queried experience.
pub(crate) async fn fetch_experience_admin(
    url: String,
    experience_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::ExperienceAdminStatus {
                experience_id,
                is_admin: parse_experience_status(&llsd),
            })
            .await
            .ok();
    }
}

/// GETs the `IsExperienceContributor` capability and forwards an
/// [`Event::ExperienceContributorStatus`] over `events`, echoing the queried
/// experience.
pub(crate) async fn fetch_experience_contributor(
    url: String,
    experience_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        events
            .send(Event::ExperienceContributorStatus {
                experience_id,
                is_contributor: parse_experience_status(&llsd),
            })
            .await
            .ok();
    }
}
