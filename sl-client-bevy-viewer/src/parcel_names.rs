//! The **parcel name cache** (`viewer-url-linkification`): a small companion to
//! the avatar ([`crate::avatars::AvatarState`]) and group
//! ([`crate::groups::GroupsModel`]) name caches that resolves a grid-wide parcel
//! id ([`ParcelKey`]) to its name.
//!
//! A `secondlife:///app/parcel/<uuid>/about` link ([`crate::linkified_text`])
//! addresses a parcel by its **grid-wide** id, which is looked up with the
//! `ParcelInfoRequest` / `ParcelInfoReply` "places" listing — distinct from the
//! region-local `ParcelProperties` the About-Land floater reads. The reply is
//! already decoded to [`SlSessionEvent::ParcelDetails`]; this cache folds each
//! one's `parcel_id` → `name` in, and the link widget requests a lookup on demand
//! ([`ParcelNames::request`]) and resolves the label from it, exactly as the
//! group cache does for a non-member group name.
//!
//! Reference (Firestorm, read-only): `llui/llurlentry` `LLUrlEntryParcel`, which
//! fires a parcel-info request and rewrites the link label when the name arrives.

use std::collections::BTreeMap;

use bevy::prelude::*;

use sl_client_bevy::{Command, ParcelKey, SlCommand, SlEvent, SlSessionEvent};

/// The parcel-name cache: grid-wide parcel id → resolved name, fed solely from the
/// `ParcelInfoReply` listing ([`SlSessionEvent::ParcelDetails`]).
#[derive(Resource, Debug, Default)]
pub(crate) struct ParcelNames {
    /// Resolved parcel names, by grid-wide parcel id.
    names: BTreeMap<ParcelKey, String>,
}

impl ParcelNames {
    /// The resolved name of `parcel`, if a `ParcelInfoReply` has arrived for it.
    pub(crate) fn name_of(&self, parcel: ParcelKey) -> Option<&str> {
        self.names.get(&parcel).map(String::as_str)
    }

    /// Request `parcel`'s listing (`ParcelInfoRequest`) if it is not already
    /// cached — the shared resolve path a parcel-name display site uses so the
    /// name fills the cache instead of showing a UUID forever. Call at a discrete
    /// event (a link spawning), not per frame; the reply folds into the cache via
    /// [`ingest_parcel_names`].
    pub(crate) fn request(&self, parcel: ParcelKey, commands: &mut MessageWriter<SlCommand>) {
        if !self.names.contains_key(&parcel) {
            commands.write(SlCommand(Command::RequestParcelInfo { parcel_id: parcel }));
        }
    }
}

/// Fold every arriving `ParcelInfoReply` listing into the cache, writing only on a
/// genuinely new / changed name so a static cache does not trip change detection.
fn ingest_parcel_names(mut events: MessageReader<SlEvent>, mut model: ResMut<ParcelNames>) {
    for event in events.read() {
        let SlSessionEvent::ParcelDetails(details) = &event.0 else {
            continue;
        };
        if model.names.get(&details.parcel_id) != Some(&details.name) {
            model.names.insert(details.parcel_id, details.name.clone());
        }
    }
}

/// Wires the parcel-name cache: the resource and its ingest system.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ParcelNamesPlugin;

impl Plugin for ParcelNamesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParcelNames>()
            .add_systems(Update, ingest_parcel_names);
    }
}

#[cfg(test)]
mod tests {
    use super::ParcelNames;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ParcelKey, Uuid};

    /// A freshly-built cache resolves nothing; once a name is inserted it resolves.
    #[test]
    fn resolves_only_after_a_reply() {
        let mut cache = ParcelNames::default();
        let parcel = ParcelKey::from(Uuid::from_u128(0x9a3c));
        assert_eq!(cache.name_of(parcel), None);
        cache.names.insert(parcel, "The Grove".to_owned());
        assert_eq!(cache.name_of(parcel), Some("The Grove"));
    }
}
