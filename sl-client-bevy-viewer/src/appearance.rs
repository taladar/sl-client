//! Trigger the modern Second Life **server-side** appearance bake ("Sunshine")
//! for our own avatar, so the P14 body-texturing pipeline has bakes to fetch.
//!
//! The viewer is otherwise a passive renderer, but on a central-baking grid our
//! own avatar is only textured once *we* ask the grid to bake our Current Outfit
//! Folder (COF): the grid then composites the worn body parts / clothing layers
//! and broadcasts the resulting baked-texture ids over `AvatarAppearance`, which
//! [`ingest_avatar_bakes`](crate::avatars::ingest_avatar_bakes) fetches and
//! [`assign_avatar_bake_materials`](crate::avatars::assign_avatar_bake_materials)
//! drapes over the system body. Without this our own avatar stays an untextured
//! "cloud".
//!
//! The trigger is the `UpdateAvatarAppearance` capability — a POST of
//! `{ "cof_version": <int> }`. The grid bakes a specific COF version and rejects
//! a stale one, answering with the version it `expected`. Rather than blindly
//! start from `0` and rely on that recovery, the viewer reads the current COF
//! version from the login-seeded inventory skeleton (the local
//! [`Command::QueryInventoryFolders`](sl_client_bevy::Command) snapshot — the
//! same model the [`inventory_cache`](sl_client_bevy) is built on) and requests
//! that version up front, falling back to the grid's `expected` version on a
//! mismatch. A one-shot handshake per session; central baking is Second
//! Life-only, so on OpenSim (which never offers the capability) this is inert.

use bevy::prelude::*;
use sl_client_bevy::{
    CAP_UPDATE_AVATAR_APPEARANCE, Command, FolderInfo, FolderType, SlCapabilities, SlCommand,
    SlEvent, SlSessionEvent,
};

/// A bound on the COF-version mismatch recovery loop, so a grid that never
/// accepts a bake cannot make the viewer spin forever.
const MAX_BAKE_ATTEMPTS: u32 = 4;

/// The stage of the one-shot server-bake handshake.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum BakeStage {
    /// Not started — no central-baking capability seen yet.
    #[default]
    Idle,
    /// The inventory-folder snapshot has been requested to read the COF version.
    FoldersRequested,
    /// A bake has been requested; awaiting the grid's accept / version mismatch.
    BakeRequested,
    /// The bake was accepted (or given up on) — nothing more to do this session.
    Done,
}

/// One-shot bookkeeping for triggering our own avatar's server-side bake.
#[derive(Resource, Default)]
pub(crate) struct ServerBakeState {
    /// The handshake stage.
    stage: BakeStage,
    /// The COF version last requested, compared against the grid's expected one.
    cof_version: i32,
    /// The number of bake requests sent, bounding the mismatch-recovery loop.
    attempts: u32,
    /// Whether the central-baking capability has been offered this session, so a
    /// later `RebakeAvatarTextures` request can re-run the handshake (P14.4). On a
    /// grid that never offers the capability (OpenSim) this stays `false` and a
    /// rebake request is inert.
    cap_available: bool,
}

/// The version of the agent's Current Outfit Folder in the inventory-folder
/// snapshot, or `0` if it is somehow absent (the grid then answers with the
/// version it expects, which the handshake retries with).
fn current_outfit_version(folders: &[FolderInfo]) -> i32 {
    folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::CurrentOutfit)
        .map_or(0, |folder| folder.version)
}

/// Drive the one-shot server-side appearance bake for our own avatar: once the
/// `UpdateAvatarAppearance` capability appears, read the current COF version from
/// the login-seeded inventory skeleton and POST a bake request, retrying with the
/// grid's expected version on a mismatch until it is accepted (or the attempt
/// bound is reached). Inert on grids without central baking (e.g. OpenSim).
pub(crate) fn drive_server_bake(
    mut capabilities: MessageReader<SlCapabilities>,
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ServerBakeState>,
    mut writer: MessageWriter<SlCommand>,
) {
    // Kick off the handshake once the central-baking capability is offered, by
    // snapshotting the inventory folders to learn the current COF version.
    for SlCapabilities(map) in capabilities.read() {
        if map.contains_key(CAP_UPDATE_AVATAR_APPEARANCE) {
            state.cap_available = true;
            if state.stage == BakeStage::Idle {
                writer.write(SlCommand(Command::QueryInventoryFolders));
                state.stage = BakeStage::FoldersRequested;
                debug!("central baking offered; reading the Current Outfit Folder version");
            }
        }
    }
    for event in events.read() {
        match &event.0 {
            // The inventory snapshot arrived: request a bake at the COF version.
            SlSessionEvent::InventoryFolders(folders)
                if state.stage == BakeStage::FoldersRequested =>
            {
                let cof_version = current_outfit_version(folders);
                state.cof_version = cof_version;
                state.attempts = 1;
                state.stage = BakeStage::BakeRequested;
                writer.write(SlCommand(Command::RequestServerAppearanceUpdate {
                    cof_version,
                }));
                debug!("requesting server appearance bake at COF version {cof_version}");
            }
            // The grid's reply: accepted, a recoverable version mismatch, or a
            // terminal rejection.
            SlSessionEvent::ServerAppearanceUpdate {
                success,
                error,
                expected_cof_version,
            } if state.stage == BakeStage::BakeRequested => {
                if *success {
                    state.stage = BakeStage::Done;
                    info!(
                        "server appearance bake accepted (COF version {})",
                        state.cof_version
                    );
                } else if let Some(expected) = *expected_cof_version
                    && expected != state.cof_version
                    && state.attempts < MAX_BAKE_ATTEMPTS
                {
                    // The grid wants a different COF version — retry with it.
                    state.cof_version = expected;
                    state.attempts = state.attempts.saturating_add(1);
                    writer.write(SlCommand(Command::RequestServerAppearanceUpdate {
                        cof_version: expected,
                    }));
                    debug!("COF version mismatch; retrying the bake at version {expected}");
                } else {
                    state.stage = BakeStage::Done;
                    let reason = error
                        .clone()
                        .unwrap_or_else(|| "no reason given".to_owned());
                    warn!("server appearance bake not accepted: {reason}");
                }
            }
            // The simulator lost one of our baked textures and is asking for a
            // rebake (P14.4). On a central-baking grid the response is to re-run
            // the server-side bake handshake so the grid re-composites and
            // re-publishes our appearance; a rebake mid-handshake is ignored since
            // the in-flight bake will satisfy it. Inert without the capability.
            SlSessionEvent::RebakeAvatarTextures { .. }
                if state.cap_available && state.stage == BakeStage::Done =>
            {
                writer.write(SlCommand(Command::QueryInventoryFolders));
                state.stage = BakeStage::FoldersRequested;
                state.attempts = 0;
                debug!("simulator requested a rebake; re-running the server appearance bake");
            }
            _other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::current_outfit_version;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{FolderInfo, FolderState, FolderType, InventoryFolderKey, Uuid};

    /// A minimal folder snapshot entry of the given type and version.
    fn folder(folder_type: FolderType, version: i32) -> FolderInfo {
        FolderInfo {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0)),
            parent_id: None,
            name: String::new(),
            folder_type,
            version,
            state: FolderState::Unknown,
        }
    }

    /// The COF version is read from the Current Outfit Folder entry; other
    /// folders are ignored.
    #[test]
    fn current_outfit_version_reads_the_cof_folder() {
        let folders = [
            folder(FolderType::Clothing, 3),
            folder(FolderType::CurrentOutfit, 15),
            folder(FolderType::Bodypart, 7),
        ];
        assert_eq!(current_outfit_version(&folders), 15);
    }

    /// With no Current Outfit Folder present, the version defaults to `0` (the
    /// grid then supplies the expected version to retry with).
    #[test]
    fn current_outfit_version_defaults_without_a_cof() {
        let folders = [folder(FolderType::Clothing, 3)];
        assert_eq!(current_outfit_version(&folders), 0);
    }
}
