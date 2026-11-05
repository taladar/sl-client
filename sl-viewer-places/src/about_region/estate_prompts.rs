//! The About Region actions that ask first: **setting the estate covenant** by
//! dropping a notecard on the Covenant tab, the Covenant tab's **Reset**, and
//! the Region tab's **Message region…**.
//!
//! Each puts up a prompt (the reference's `EstateChangeCovenant` and
//! `MessageRegion`) and acts on the answer — in the region the window was
//! about, and only while the agent is still there, as the terrain actions do
//! ([`crate::region_terrain_actions`]): every message here goes out on the
//! current circuit, whatever region the window shows.
//!
//! # A dropped covenant is read before it is set
//!
//! The reference reads the notecard through its item before it points the
//! estate at the asset (`LLPanelEstateCovenant::loadInvItem`), so a notecard the
//! agent may not read, or one whose body does not decode, is refused with the
//! reference's notice rather than set; the body read then fills the tab at
//! once, before the grid's covenant summary comes back.

use bevy::prelude::*;
use sl_client_bevy::{
    AssetKey, AssetType, Command, InventoryKey, ItemInfo, SlCommand, SlCurrentRegion, SlEvent,
    SlRegionIdentity, SlSessionEvent, TransferStatus, Uuid,
};

use super::{AboutRegionDirty, AboutRegionState, AboutRegionUi};
use crate::intents::{ItemDropTarget, ItemDropped};
use crate::notifications::{NotificationResponse, OkCancel, ShowNotification, TemplateRef};

/// The confirmation a covenant change — a dropped notecard, or Reset — asks
/// first.
const CONFIRM_CHANGE_COVENANT: TemplateRef<OkCancel> = TemplateRef::new("EstateChangeCovenant");

/// The prompt that asks for the region message's text.
const PROMPT_MESSAGE_REGION: TemplateRef<OkCancel> = TemplateRef::new("MessageRegion");

/// The notice for a covenant notecard whose body does not decode.
const PROBLEM_IMPORTING: &str = "ProblemImportingEstateCovenant";

/// What an About Region button asks to prompt for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegionPrompt {
    /// Clear the estate's covenant, once the user confirms.
    ResetCovenant,
    /// Ask for a message, and send it to everyone in the region.
    MessageRegion,
}

/// A prompting About Region button's request, about its window's region.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RegionPromptRequest {
    /// The window the button is in.
    pub window: Entity,
    /// The region the window was about when the button was pressed.
    pub region: Uuid,
    /// What it asks for.
    pub prompt: RegionPrompt,
}

/// The covenant change a confirmation would make.
#[derive(Debug, Clone, PartialEq)]
enum CovenantChange {
    /// Point the estate at this notecard, once its body reads.
    Notecard(Box<ItemInfo>),
    /// Clear the covenant.
    Reset,
}

/// A prompt on screen, and what its answer acts on.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    /// The covenant-change confirmation.
    Covenant(CovenantChange),
    /// The region-message prompt.
    MessageRegion,
}

/// A dropped covenant notecard whose body is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CovenantLoad {
    /// The window it was dropped on.
    window: Entity,
    /// The region the window was about.
    region: Uuid,
    /// The notecard's item, which the read names.
    item: InventoryKey,
    /// The notecard's asset, which the covenant is set to.
    asset: Uuid,
}

/// The prompts and the notecard read in flight.
#[derive(Resource, Debug, Default)]
pub(super) struct EstatePrompts {
    /// The prompt on screen: its window, region, and what it would do.
    pending: Option<(Entity, Uuid, Pending)>,
    /// The dropped notecard being read.
    loading: Option<CovenantLoad>,
}

/// The region the agent is in, if any.
fn current_region(regions: &Query<&SlRegionIdentity, With<SlCurrentRegion>>) -> Option<Uuid> {
    regions.iter().next().map(|region| region.0.region_id)
}

/// Keep each window's covenant drop target taking a notecard only while the
/// agent may manage the estate the window is about.
pub(super) fn sync_covenant_drop_target(
    windows: Query<(&AboutRegionState, &AboutRegionUi)>,
    mut targets: Query<&mut ItemDropTarget>,
) {
    for (state, ui) in &windows {
        let Some(mut target) = ui
            .covenant
            .drop_area
            .and_then(|area| targets.get_mut(area).ok())
        else {
            continue;
        };
        let enabled = state.can_manage;
        if target.enabled != enabled {
            target.enabled = enabled;
        }
    }
}

/// A notecard dropped on a Covenant tab that takes it: ask before changing the
/// covenant.
pub(super) fn take_covenant_drops(
    mut drops: MessageReader<ItemDropped>,
    windows: Query<(Entity, &AboutRegionState, &AboutRegionUi)>,
    mut prompts: ResMut<EstatePrompts>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for drop in drops.read() {
        let Some((window, state, _ui)) = windows
            .iter()
            .find(|(_window, _state, ui)| ui.covenant.drop_area == Some(drop.target))
        else {
            continue;
        };
        if !state.can_manage {
            continue;
        }
        prompts.pending = Some((
            window,
            state.region,
            Pending::Covenant(CovenantChange::Notecard(Box::new(drop.item.clone()))),
        ));
        notifications.write(ShowNotification::new(CONFIRM_CHANGE_COVENANT.name()));
    }
}

/// Put up the prompt each request asks for.
pub(super) fn start_region_prompts(
    mut requests: MessageReader<RegionPromptRequest>,
    mut prompts: ResMut<EstatePrompts>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for request in requests.read() {
        let (pending, template) = match request.prompt {
            RegionPrompt::ResetCovenant => (
                Pending::Covenant(CovenantChange::Reset),
                CONFIRM_CHANGE_COVENANT.name(),
            ),
            RegionPrompt::MessageRegion => (Pending::MessageRegion, PROMPT_MESSAGE_REGION.name()),
        };
        prompts.pending = Some((request.window, request.region, pending));
        notifications.write(ShowNotification::new(template));
    }
}

/// Act on an answered prompt — in the region it was asked about, and only while
/// the agent is still there.
pub(super) fn answer_region_prompts(
    mut responses: MessageReader<NotificationResponse>,
    mut prompts: ResMut<EstatePrompts>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        if !response.is_for(CONFIRM_CHANGE_COVENANT) && !response.is_for(PROMPT_MESSAGE_REGION) {
            continue;
        }
        let Some((window, region, pending)) = prompts.pending.take() else {
            continue;
        };
        let template = match pending {
            Pending::Covenant(_) => CONFIRM_CHANGE_COVENANT,
            Pending::MessageRegion => PROMPT_MESSAGE_REGION,
        };
        if response.answer(template) != Some(OkCancel::Ok) {
            continue;
        }
        if current_region(&regions) != Some(region) {
            warn!(
                region = %region,
                "the agent left the region an About Region prompt was about; nothing sent"
            );
            continue;
        }
        match pending {
            Pending::MessageRegion => {
                if let Some(message) = response
                    .input
                    .as_deref()
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
                {
                    sl_commands.write(SlCommand(Command::SendRegionMessage {
                        message: message.to_owned(),
                    }));
                }
            }
            Pending::Covenant(CovenantChange::Notecard(item)) => {
                prompts.loading = Some(CovenantLoad {
                    window,
                    region,
                    item: item.item_id,
                    asset: item.asset_id,
                });
                sl_commands.write(SlCommand(Command::FetchInventoryItemAsset {
                    item_id: item.item_id,
                    asset_id: AssetKey::from(item.asset_id),
                    asset_type: AssetType::Notecard,
                }));
            }
            Pending::Covenant(CovenantChange::Reset) => {
                if let Ok((mut state, mut dirty)) = windows.get_mut(window) {
                    set_covenant(&mut state, &mut dirty, None, None, &mut sl_commands);
                }
            }
        }
    }
}

/// Finish a dropped notecard's read: set the covenant to it, or say why not.
pub(super) fn finish_covenant_loads(
    mut events: MessageReader<SlEvent>,
    mut prompts: ResMut<EstatePrompts>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        let Some(load) = prompts.loading else {
            return;
        };
        let outcome = match &event.0 {
            SlSessionEvent::InventoryItemAssetReceived { item, data, .. } if *item == load.item => {
                read_covenant(data)
            }
            SlSessionEvent::TransferFailed {
                item: Some(item),
                status,
                ..
            } if *item == load.item => Err(refusal_notice(*status)),
            _ => continue,
        };
        prompts.loading = None;
        match outcome {
            Err(notice) => {
                notifications.write(ShowNotification::new(notice));
            }
            Ok(text) => {
                if current_region(&regions) != Some(load.region) {
                    warn!(
                        region = %load.region,
                        "the agent left the region a covenant was dropped for; nothing sent"
                    );
                    continue;
                }
                if let Ok((mut state, mut dirty)) = windows.get_mut(load.window) {
                    set_covenant(
                        &mut state,
                        &mut dirty,
                        Some(load.asset),
                        Some(text),
                        &mut sl_commands,
                    );
                }
            }
        }
    }
}

/// The body of a covenant notecard, or the notice that refuses it: a body
/// marked as a Linden notecard has to decode; one without the mark is the
/// plain text of a version-0 notecard and is taken as it is, as the reference
/// takes it.
pub(super) fn read_covenant(data: &[u8]) -> Result<String, &'static str> {
    if data.starts_with(b"Linden text version") {
        sl_notecard::Notecard::decode(data)
            .map(|notecard| notecard.text)
            .map_err(|_error| PROBLEM_IMPORTING)
    } else {
        Ok(String::from_utf8_lossy(data).into_owned())
    }
}

/// The reference's notice for a notecard read the grid refused.
const fn refusal_notice(status: TransferStatus) -> &'static str {
    match status {
        TransferStatus::UnknownSource => "MissingNotecardAssetID",
        TransferStatus::InsufficientPermissions => "NotAllowedToViewNotecard",
        _ => "UnableToLoadNotecardAsset",
    }
}

/// Point the estate at `asset` (or at no covenant), unless it already is — the
/// reference's `sendChangeCovenantID` — show `text` at once, and ask for the
/// summary again for its new time.
fn set_covenant(
    state: &mut AboutRegionState,
    dirty: &mut AboutRegionDirty,
    asset: Option<Uuid>,
    text: Option<String>,
    sl_commands: &mut MessageWriter<SlCommand>,
) {
    let current = state
        .covenant
        .as_ref()
        .and_then(|covenant| covenant.covenant_id);
    state.covenant_text = text;
    state.covenant_pending = None;
    if let Some(covenant) = state.covenant.as_mut() {
        covenant.covenant_id = asset;
    }
    dirty.covenant_values = true;
    if current == asset {
        return;
    }
    sl_commands.write(SlCommand(Command::SetEstateCovenant {
        covenant: asset.map(AssetKey::from),
    }));
    sl_commands.write(SlCommand(Command::RequestEstateCovenant));
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::TransferStatus;

    use super::{PROBLEM_IMPORTING, read_covenant, refusal_notice};

    /// A Linden notecard decodes to its text; plain text is taken as it is; a
    /// marked body that does not decode is refused with the reference's notice.
    #[test]
    fn a_covenant_body_reads_as_the_reference_reads_it() {
        let notecard = sl_notecard::Notecard {
            source_version: sl_notecard::NotecardVersion::V2,
            items: Vec::new(),
            text: "Be excellent.".to_owned(),
        };
        assert_eq!(
            read_covenant(&notecard.encode()),
            Ok("Be excellent.".to_owned())
        );
        assert_eq!(read_covenant(b"plain words"), Ok("plain words".to_owned()));
        assert_eq!(
            read_covenant(b"Linden text version 2\n{garbage"),
            Err(PROBLEM_IMPORTING)
        );
    }

    /// Each refusal maps to the reference's notice.
    #[test]
    fn a_refused_read_names_the_reference_notice() {
        assert_eq!(
            refusal_notice(TransferStatus::UnknownSource),
            "MissingNotecardAssetID"
        );
        assert_eq!(
            refusal_notice(TransferStatus::InsufficientPermissions),
            "NotAllowedToViewNotecard"
        );
        assert_eq!(
            refusal_notice(TransferStatus::Abort),
            "UnableToLoadNotecardAsset"
        );
    }
}
