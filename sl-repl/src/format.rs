//! Human-readable, symbolized renderers for the three things a REPL session
//! shows the user: the [`Event`]s it surfaces, the [`Command`]s it dispatches,
//! and the [`Diagnostic`]s the protocol layer reports.
//!
//! [`format_event`] and [`format_command`] render through a
//! [`ReplContext`]: every volatile literal a binding stands for (the agent id,
//! the region handle, a capability URL, a user variable, …) is rewritten back
//! to its `$placeholder` so two runs against the same grid produce a clean,
//! diffable transcript even though the underlying ids differ. [`format_diagnostic`]
//! renders **literally** — a diagnostic is about the raw wire, so its bytes and
//! ids are shown verbatim — and embeds a [`hexdump`] of the offending bytes with
//! the failing offset marked.
//!
//! The two enum renderers match **exhaustively** (no `_` arm): a newly added
//! [`Command`] or [`Event`] variant fails to compile here until it is named,
//! so the formatter can never silently fall back to an anonymous rendering.

use std::fmt::Write as _;

use sl_proto::{Command, Diagnostic, Event};

use crate::context::ReplContext;

/// Render an [`Event`] as `<event_name><symbolized fields>`.
///
/// The event name is the snake-case of the variant; the field rendering is the
/// variant's `Debug` form with every binding-backed literal symbolized through
/// `ctx` (see [`ReplContext::symbolize`]).
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`format_event` is the natural public name for this renderer"
)]
pub fn format_event(event: &Event, ctx: &dyn ReplContext) -> String {
    let body = symbolize_text(&strip_leading_ident(&format!("{event:?}")), ctx);
    let name = event_name(event);
    let mut out = String::with_capacity(name.len().saturating_add(body.len()));
    out.push_str(name);
    out.push_str(&body);
    out
}

/// Render a [`Command`] as `<repl_command_name><symbolized fields>`.
///
/// The name is the command's REPL spelling (the same token the
/// [registry](crate::registry) parses); the field rendering is the variant's
/// `Debug` form with every binding-backed literal symbolized through `ctx`.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`format_command` is the natural public name for this renderer"
)]
pub fn format_command(command: &Command, ctx: &dyn ReplContext) -> String {
    let body = symbolize_text(&strip_leading_ident(&format!("{command:?}")), ctx);
    let name = command_name(command);
    let mut out = String::with_capacity(name.len().saturating_add(body.len()));
    out.push_str(name);
    out.push_str(&body);
    out
}

/// Render a [`Diagnostic`] **literally** (no symbolization): a one-line header
/// for every variant, plus a marked [`hexdump`] of the captured bytes for a
/// [`Diagnostic::DecodeFailed`].
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`format_diagnostic` is the natural public name for this renderer"
)]
pub fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let mut out = String::new();
    let _rendered = write_diagnostic(&mut out, diagnostic);
    out
}

/// Render `bytes` as a classic offset / hex / ASCII dump, 16 bytes per row.
///
/// When `mark` is `Some(offset)` the byte at that offset is wrapped in square
/// brackets (`[ab]` rather than ` ab `, keeping every cell four columns wide so
/// the rows stay aligned). A `mark` at or past the end of `bytes` — the reader
/// position a decode stopped at — is noted on a trailing line instead.
#[must_use]
pub fn hexdump(bytes: &[u8], mark: Option<usize>) -> String {
    let mut out = String::new();
    let _rendered = write_hexdump(&mut out, bytes, mark);
    out
}

/// Drop the leading identifier of a `Debug` rendering (the Rust variant name),
/// leaving just the ` { … }` / `(…)` field tail (or the empty string for a unit
/// variant) so a caller can prefix its own name.
fn strip_leading_ident(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut stripping = true;
    for c in text.chars() {
        if stripping && (c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        stripping = false;
        out.push(c);
    }
    out
}

/// Rewrite the binding-backed literals in `text` back to their `$placeholder`
/// tokens.
///
/// Bare runs of `[0-9A-Za-z-]` (UUIDs, integers, names) and whole double-quoted
/// strings (capability URLs, user-variable values) are each offered to
/// [`ReplContext::symbolize`]; a match is replaced by the placeholder, anything
/// else is passed through unchanged. Quoted strings are unescaped before the
/// lookup but re-emitted verbatim when unmatched.
fn symbolize_text(text: &str, ctx: &dyn ReplContext) -> String {
    let mut out = String::with_capacity(text.len());
    let mut atom = String::new();
    let mut quoted = String::new();
    let mut in_quote = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_quote {
            if escaped {
                quoted.push(c);
                escaped = false;
            } else if c == '\\' {
                quoted.push(c);
                escaped = true;
            } else if c == '"' {
                in_quote = false;
                flush_quote(&mut quoted, &mut out, ctx);
            } else {
                quoted.push(c);
            }
            continue;
        }
        if c == '"' {
            flush_atom(&mut atom, &mut out, ctx);
            in_quote = true;
        } else if c.is_ascii_alphanumeric() || c == '-' {
            atom.push(c);
        } else {
            flush_atom(&mut atom, &mut out, ctx);
            out.push(c);
        }
    }
    flush_atom(&mut atom, &mut out, ctx);
    if in_quote {
        // An unterminated quote (it ran to the end of the text): emit verbatim.
        out.push('"');
        out.push_str(&quoted);
    }
    out
}

/// Flush an accumulated bare atom into `out`, symbolizing it if `ctx` binds it.
fn flush_atom(atom: &mut String, out: &mut String, ctx: &dyn ReplContext) {
    if atom.is_empty() {
        return;
    }
    match ctx.symbolize(atom) {
        Some(placeholder) => out.push_str(&placeholder),
        None => out.push_str(atom),
    }
    atom.clear();
}

/// Flush an accumulated quoted string into `out`, symbolizing its (unescaped)
/// value if `ctx` binds it, or re-quoting it verbatim otherwise.
fn flush_quote(quoted: &mut String, out: &mut String, ctx: &dyn ReplContext) {
    match ctx.symbolize(&unescape(quoted)) {
        Some(placeholder) => out.push_str(&placeholder),
        None => {
            out.push('"');
            out.push_str(quoted);
            out.push('"');
        }
    }
    quoted.clear();
}

/// Undo the common `Debug` string escapes (`\n`, `\t`, `\r`, `\0`, `\"`, `\\`),
/// passing any other escape through as its literal second character.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            match c {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                other => out.push(other),
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else {
            out.push(c);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Write a [`Diagnostic`]'s literal rendering into `out`.
fn write_diagnostic(out: &mut String, diagnostic: &Diagnostic) -> std::fmt::Result {
    match diagnostic {
        Diagnostic::DecodeFailed {
            id,
            name,
            error,
            raw,
            failed_offset,
        } => {
            let displayed_name = name.unwrap_or("?");
            write!(
                out,
                "DecodeFailed id={id:?} name={displayed_name} error={error} failed_offset={failed_offset}"
            )?;
            out.push('\n');
            write_hexdump(out, raw, Some(*failed_offset))?;
        }
        Diagnostic::UnhandledMessage { id, name, child } => {
            write!(out, "UnhandledMessage id={id:?} name={name} child={child}")?;
        }
        Diagnostic::UnknownCapsEvent { message } => {
            write!(out, "UnknownCapsEvent message={message}")?;
        }
        Diagnostic::CapsDecodeFailed { message } => {
            write!(out, "CapsDecodeFailed message={message}")?;
        }
        Diagnostic::ExpectedReplyMissing { request, sequence } => match sequence {
            Some(seq) => write!(out, "ExpectedReplyMissing request={request} sequence={seq}")?,
            None => write!(out, "ExpectedReplyMissing request={request} sequence=-")?,
        },
        // `Diagnostic` is `#[non_exhaustive]`: render any future kind generically.
        other => write!(out, "{other:?}")?,
    }
    Ok(())
}

/// Write a marked offset / hex / ASCII dump of `bytes` into `out`.
fn write_hexdump(out: &mut String, bytes: &[u8], mark: Option<usize>) -> std::fmt::Result {
    if bytes.is_empty() {
        out.push_str("(no bytes)");
        if let Some(at) = mark {
            write!(out, " — mark at offset {at}")?;
        }
        return Ok(());
    }
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let base = row.saturating_mul(16);
        write!(out, "{base:08x} ")?;
        for (col, byte) in chunk.iter().enumerate() {
            let offset = base.saturating_add(col);
            if Some(offset) == mark {
                write!(out, "[{byte:02x}]")?;
            } else {
                write!(out, " {byte:02x} ")?;
            }
        }
        out.push_str(" |");
        for byte in chunk {
            out.push(printable(*byte));
        }
        out.push('|');
        out.push('\n');
    }
    if let Some(at) = mark
        && at >= bytes.len()
    {
        write!(out, "(mark at offset {at} = end of {} bytes)", bytes.len())?;
    }
    Ok(())
}

/// The printable ASCII glyph for `byte`, or `.` for a non-printable byte.
fn printable(byte: u8) -> char {
    if (0x20..=0x7e).contains(&byte) {
        char::from(byte)
    } else {
        '.'
    }
}

/// The snake-case event name for an [`Event`] variant.
const fn event_name(event: &Event) -> &'static str {
    match event {
        Event::CircuitEstablished { .. } => "circuit_established",
        Event::RegionHandshakeComplete => "region_handshake_complete",
        Event::RegionInfoHandshake(..) => "region_info_handshake",
        Event::RegionLimits(..) => "region_limits",
        Event::AvatarNames(..) => "avatar_names",
        Event::GroupNames(..) => "group_names",
        Event::DisplayNames(..) => "display_names",
        Event::Environment(..) => "environment",
        Event::MoneyBalance(..) => "money_balance",
        Event::EconomyData(..) => "economy_data",
        Event::ParcelProperties(..) => "parcel_properties",
        Event::ParcelOverlay(..) => "parcel_overlay",
        Event::ParcelMediaCommand { .. } => "parcel_media_command",
        Event::ParcelMediaUpdate(..) => "parcel_media_update",
        Event::ParcelDwell { .. } => "parcel_dwell",
        Event::ParcelAccessList { .. } => "parcel_access_list",
        Event::ParcelObjectOwners { .. } => "parcel_object_owners",
        Event::ParcelDetails(..) => "parcel_details",
        Event::RemoteParcelId(..) => "remote_parcel_id",
        Event::SimulatorFeatures(..) => "simulator_features",
        Event::AgentPreferences(..) => "agent_preferences",
        Event::ObjectCosts(..) => "object_costs",
        Event::SelectedResourceCost(..) => "selected_resource_cost",
        Event::ObjectPhysicsData(..) => "object_physics_data",
        Event::ObjectPhysicsProperties(..) => "object_physics_properties",
        Event::AttachmentResources(..) => "attachment_resources",
        Event::LandResourcesUrls(..) => "land_resources_urls",
        Event::LandResourceSummary(..) => "land_resource_summary",
        Event::LandResourceDetail(..) => "land_resource_detail",
        Event::LandStatReply { .. } => "land_stat_reply",
        Event::EstateInfo(..) => "estate_info",
        Event::EstateAccessList { .. } => "estate_access_list",
        Event::EstateCovenant(..) => "estate_covenant",
        Event::TelehubInfo(..) => "telehub_info",
        Event::ScriptRunning { .. } => "script_running",
        Event::NeighborDiscovered(..) => "neighbor_discovered",
        Event::NeighborSeed { .. } => "neighbor_seed",
        Event::MapBlock(..) => "map_block",
        Event::MapItems { .. } => "map_items",
        Event::MapLayers { .. } => "map_layers",
        Event::TeleportStarted => "teleport_started",
        Event::TeleportProgress { .. } => "teleport_progress",
        Event::TeleportLocal => "teleport_local",
        Event::TeleportFailed { .. } => "teleport_failed",
        Event::TeleportFinished { .. } => "teleport_finished",
        Event::RegionChanged { .. } => "region_changed",
        Event::ChatReceived(..) => "chat_received",
        Event::ChatTyping { .. } => "chat_typing",
        Event::InstantMessageReceived(..) => "instant_message_received",
        Event::ImTyping { .. } => "im_typing",
        Event::AvatarProperties(..) => "avatar_properties",
        Event::AvatarInterests(..) => "avatar_interests",
        Event::AvatarGroups { .. } => "avatar_groups",
        Event::AvatarPicks { .. } => "avatar_picks",
        Event::AvatarNotes { .. } => "avatar_notes",
        Event::AvatarClassifieds { .. } => "avatar_classifieds",
        Event::PickInfo(..) => "pick_info",
        Event::ClassifiedInfo(..) => "classified_info",
        Event::Account(..) => "account",
        Event::InventorySkeleton(..) => "inventory_skeleton",
        Event::LibraryInventory(..) => "library_inventory",
        Event::InventoryDescendents { .. } => "inventory_descendents",
        Event::InventoryItemCreated { .. } => "inventory_item_created",
        Event::InventoryBulkUpdate { .. } => "inventory_bulk_update",
        Event::FriendList(..) => "friend_list",
        Event::FriendsOnline(..) => "friends_online",
        Event::FriendsOffline(..) => "friends_offline",
        Event::FriendRightsChanged { .. } => "friend_rights_changed",
        Event::ActiveGroupChanged(..) => "active_group_changed",
        Event::GroupMemberships(..) => "group_memberships",
        Event::GroupMembers { .. } => "group_members",
        Event::GroupRoleData { .. } => "group_role_data",
        Event::GroupRoleMembers { .. } => "group_role_members",
        Event::GroupTitles { .. } => "group_titles",
        Event::GroupProfileReceived(..) => "group_profile_received",
        Event::GroupNotices { .. } => "group_notices",
        Event::GroupAccountSummary(..) => "group_account_summary",
        Event::GroupAccountDetails(..) => "group_account_details",
        Event::GroupAccountTransactions(..) => "group_account_transactions",
        Event::GroupActiveProposals { .. } => "group_active_proposals",
        Event::GroupVoteHistory { .. } => "group_vote_history",
        Event::GroupSessionMessage { .. } => "group_session_message",
        Event::GroupSessionParticipant { .. } => "group_session_participant",
        Event::ConferenceSessionMessage { .. } => "conference_session_message",
        Event::ConferenceSessionParticipant { .. } => "conference_session_participant",
        Event::ConferenceInvited { .. } => "conference_invited",
        Event::CreateGroupResult { .. } => "create_group_result",
        Event::JoinGroupResult { .. } => "join_group_result",
        Event::LeaveGroupResult { .. } => "leave_group_result",
        Event::DroppedFromGroup { .. } => "dropped_from_group",
        Event::EjectGroupMemberResult { .. } => "eject_group_member_result",
        Event::ScriptDialog(..) => "script_dialog",
        Event::ScriptPermissionRequest(..) => "script_permission_request",
        Event::LoadUrl(..) => "load_url",
        Event::ScriptTeleport(..) => "script_teleport",
        Event::ScriptControlChange(..) => "script_control_change",
        Event::SetFollowCamProperties { .. } => "set_follow_cam_properties",
        Event::ClearFollowCamProperties { .. } => "clear_follow_cam_properties",
        Event::MuteList(..) => "mute_list",
        Event::MuteListUnchanged => "mute_list_unchanged",
        Event::SitResult { .. } => "sit_result",
        Event::ObjectAdded(..) => "object_added",
        Event::ObjectUpdated(..) => "object_updated",
        Event::TimeDilation { .. } => "time_dilation",
        Event::ObjectRemoved { .. } => "object_removed",
        Event::ObjectProperties(..) => "object_properties",
        Event::ObjectPropertiesFamily { .. } => "object_properties_family",
        Event::PayPriceReply { .. } => "pay_price_reply",
        Event::ObjectMedia { .. } => "object_media",
        Event::GltfMaterialOverride { .. } => "gltf_material_override",
        Event::RenderMaterials(..) => "render_materials",
        Event::MaterialParamsResult { .. } => "material_params_result",
        Event::VoiceAccountProvisioned(..) => "voice_account_provisioned",
        Event::ParcelVoiceInfo(..) => "parcel_voice_info",
        Event::ExperienceInfo(..) => "experience_info",
        Event::ExperienceSearchResults(..) => "experience_search_results",
        Event::ExperiencePermissions { .. } => "experience_permissions",
        Event::OwnedExperiences(..) => "owned_experiences",
        Event::AdminExperiences(..) => "admin_experiences",
        Event::CreatorExperiences(..) => "creator_experiences",
        Event::GroupExperiences { .. } => "group_experiences",
        Event::ExperienceAdminStatus { .. } => "experience_admin_status",
        Event::ExperienceContributorStatus { .. } => "experience_contributor_status",
        Event::ExperienceUpdated(..) => "experience_updated",
        Event::RegionExperiences { .. } => "region_experiences",
        Event::TerrainPatch(..) => "terrain_patch",
        Event::TextureReceived(..) => "texture_received",
        Event::TextureNotFound(..) => "texture_not_found",
        Event::AssetReceived(..) => "asset_received",
        Event::AssetTransferStarted { .. } => "asset_transfer_started",
        Event::AssetTransferFailed { .. } => "asset_transfer_failed",
        Event::AssetUploadComplete { .. } => "asset_upload_complete",
        Event::AssetUploaded { .. } => "asset_uploaded",
        Event::AssetUploadFailed { .. } => "asset_upload_failed",
        Event::AvatarAppearance(..) => "avatar_appearance",
        Event::AgentWearables { .. } => "agent_wearables",
        Event::ServerAppearanceUpdate { .. } => "server_appearance_update",
        Event::CachedTextureResponse { .. } => "cached_texture_response",
        Event::AvatarAnimation { .. } => "avatar_animation",
        Event::CoarseLocationUpdate { .. } => "coarse_location_update",
        Event::ViewerEffect(..) => "viewer_effect",
        Event::FindAgentReply { .. } => "find_agent_reply",
        Event::DirPeopleReply { .. } => "dir_people_reply",
        Event::DirGroupsReply { .. } => "dir_groups_reply",
        Event::DirEventsReply { .. } => "dir_events_reply",
        Event::DirClassifiedReply { .. } => "dir_classified_reply",
        Event::DirPlacesReply { .. } => "dir_places_reply",
        Event::DirLandReply { .. } => "dir_land_reply",
        Event::AvatarPickerReply { .. } => "avatar_picker_reply",
        Event::PlacesReply { .. } => "places_reply",
        Event::EventInfoReply { .. } => "event_info_reply",
        Event::SoundTrigger { .. } => "sound_trigger",
        Event::AttachedSound { .. } => "attached_sound",
        Event::AttachedSoundGainChange { .. } => "attached_sound_gain_change",
        Event::PreloadSound { .. } => "preload_sound",
        Event::AlertMessage { .. } => "alert_message",
        Event::AgentAlertMessage { .. } => "agent_alert_message",
        Event::MeanCollisionAlert(..) => "mean_collision_alert",
        Event::HealthMessage { .. } => "health_message",
        Event::CameraConstraint { .. } => "camera_constraint",
        Event::ViewerFrozen { .. } => "viewer_frozen",
        Event::LoggedOut => "logged_out",
        Event::Disconnected(..) => "disconnected",
    }
}

/// The REPL command name for a [`Command`] variant (the registry spelling).
const fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Send { .. } => "send",
        Command::Chat { .. } => "chat",
        Command::Typing(..) => "typing",
        Command::InstantMessage { .. } => "im",
        Command::ImTyping { .. } => "im_typing",
        Command::SetControls(..) => "set_controls",
        Command::SetThrottle(..) => "set_throttle",
        Command::SetRotation { .. } => "set_rotation",
        Command::SetCamera(..) => "set_camera",
        Command::Stand => "stand",
        Command::SitOnGround => "sit_on_ground",
        Command::Sit { .. } => "sit",
        Command::Autopilot { .. } => "autopilot",
        Command::RequestAvatarProperties(..) => "request_avatar_properties",
        Command::RequestAvatarPicks(..) => "request_avatar_picks",
        Command::RequestAvatarNotes(..) => "request_avatar_notes",
        Command::RequestAvatarClassifieds(..) => "request_avatar_classifieds",
        Command::RequestPickInfo { .. } => "request_pick_info",
        Command::RequestClassifiedInfo(..) => "request_classified_info",
        Command::UpdateProfile(..) => "update_profile",
        Command::UpdateInterests(..) => "update_interests",
        Command::UpdateAvatarNotes { .. } => "update_avatar_notes",
        Command::UpdatePick(..) => "update_pick",
        Command::DeletePick(..) => "delete_pick",
        Command::GodDeletePick { .. } => "god_delete_pick",
        Command::UpdateClassified(..) => "update_classified",
        Command::DeleteClassified(..) => "delete_classified",
        Command::GodDeleteClassified { .. } => "god_delete_classified",
        Command::RequestFolderContents(..) => "request_folder_contents",
        Command::FetchInventoryFolders(..) => "fetch_inventory_folders",
        Command::CreateInventoryFolder { .. } => "create_inventory_folder",
        Command::UpdateInventoryFolder { .. } => "update_inventory_folder",
        Command::MoveInventoryFolder { .. } => "move_inventory_folder",
        Command::RemoveInventoryFolders(..) => "remove_inventory_folders",
        Command::CreateInventoryItem(..) => "create_inventory_item",
        Command::UpdateInventoryItem { .. } => "update_inventory_item",
        Command::MoveInventoryItem { .. } => "move_inventory_item",
        Command::CopyInventoryItem { .. } => "copy_inventory_item",
        Command::RemoveInventoryItems(..) => "remove_inventory_items",
        Command::ChangeInventoryItemFlags { .. } => "change_inventory_item_flags",
        Command::PurgeInventoryDescendents(..) => "purge_inventory_descendents",
        Command::RemoveInventoryObjects { .. } => "remove_inventory_objects",
        Command::CreateInventoryCategory { .. } => "create_inventory_category",
        Command::Ais3CreateFolder { .. } => "ais3_create_folder",
        Command::Ais3RenameFolder { .. } => "ais3_rename_folder",
        Command::Ais3MoveFolder { .. } => "ais3_move_folder",
        Command::Ais3RemoveFolder(..) => "ais3_remove_folder",
        Command::Ais3PurgeFolder(..) => "ais3_purge_folder",
        Command::Ais3FetchFolderChildren { .. } => "ais3_fetch_folder_children",
        Command::Ais3UpdateItem { .. } => "ais3_update_item",
        Command::Ais3MoveItem { .. } => "ais3_move_item",
        Command::Ais3RemoveItem(..) => "ais3_remove_item",
        Command::Ais3FetchItem(..) => "ais3_fetch_item",
        Command::GrantUserRights { .. } => "grant_user_rights",
        Command::OfferFriendship { .. } => "offer_friendship",
        Command::TerminateFriendship(..) => "terminate_friendship",
        Command::AcceptFriendship { .. } => "accept_friendship",
        Command::DeclineFriendship(..) => "decline_friendship",
        Command::ActivateGroup(..) => "activate_group",
        Command::RequestGroupMembers(..) => "request_group_members",
        Command::FetchGroupMembers(..) => "fetch_group_members",
        Command::RequestGroupRoles(..) => "request_group_roles",
        Command::RequestGroupRoleMembers(..) => "request_group_role_members",
        Command::RequestGroupTitles(..) => "request_group_titles",
        Command::RequestGroupProfile(..) => "request_group_profile",
        Command::RequestGroupNotices(..) => "request_group_notices",
        Command::RequestGroupNotice(..) => "request_group_notice",
        Command::CreateGroup(..) => "create_group",
        Command::JoinGroup(..) => "join_group",
        Command::LeaveGroup(..) => "leave_group",
        Command::InviteToGroup { .. } => "invite_to_group",
        Command::SetGroupAcceptNotices { .. } => "set_group_accept_notices",
        Command::SetGroupContribution { .. } => "set_group_contribution",
        Command::StartGroupSession(..) => "start_group_session",
        Command::SendGroupMessage { .. } => "send_group_message",
        Command::LeaveGroupSession(..) => "leave_group_session",
        Command::UpdateGroupRoles { .. } => "update_group_roles",
        Command::ChangeGroupRoleMembers { .. } => "change_group_role_members",
        Command::EjectGroupMembers { .. } => "eject_group_members",
        Command::RequestGroupAccountSummary { .. } => "request_group_account_summary",
        Command::RequestGroupAccountDetails { .. } => "request_group_account_details",
        Command::RequestGroupAccountTransactions { .. } => "request_group_account_transactions",
        Command::RequestGroupActiveProposals { .. } => "request_group_active_proposals",
        Command::RequestGroupVoteHistory { .. } => "request_group_vote_history",
        Command::StartGroupProposal { .. } => "start_group_proposal",
        Command::GroupProposalBallot { .. } => "group_proposal_ballot",
        Command::SendGroupNotice { .. } => "send_group_notice",
        Command::ReplyScriptDialog { .. } => "reply_script_dialog",
        Command::AnswerScriptPermissions { .. } => "answer_script_permissions",
        Command::RequestMuteList => "request_mute_list",
        Command::Mute { .. } => "mute",
        Command::Unmute { .. } => "unmute",
        Command::Teleport { .. } => "teleport",
        Command::RequestRegionInfo => "request_region_info",
        Command::RequestAvatarNames(..) => "request_avatar_names",
        Command::RequestGroupNames(..) => "request_group_names",
        Command::RequestDisplayNames(..) => "request_display_names",
        Command::RequestEnvironment { .. } => "request_environment",
        Command::RequestParcelProperties { .. } => "request_parcel_properties",
        Command::UpdateParcel(..) => "update_parcel",
        Command::RequestParcelAccessList { .. } => "request_parcel_access_list",
        Command::UpdateParcelAccessList { .. } => "update_parcel_access_list",
        Command::RequestParcelDwell { .. } => "request_parcel_dwell",
        Command::BuyParcel { .. } => "buy_parcel",
        Command::ReturnParcelObjects { .. } => "return_parcel_objects",
        Command::SelectParcelObjects { .. } => "select_parcel_objects",
        Command::DeedParcelToGroup { .. } => "deed_parcel_to_group",
        Command::ReclaimParcel { .. } => "reclaim_parcel",
        Command::ReleaseParcel { .. } => "release_parcel",
        Command::JoinParcels { .. } => "join_parcels",
        Command::DivideParcel { .. } => "divide_parcel",
        Command::RequestParcelObjectOwners { .. } => "request_parcel_object_owners",
        Command::BuyParcelPass { .. } => "buy_parcel_pass",
        Command::DisableParcelObjects { .. } => "disable_parcel_objects",
        Command::RequestParcelInfo { .. } => "request_parcel_info",
        Command::RequestRemoteParcelId { .. } => "request_remote_parcel_id",
        Command::RequestSimulatorFeatures => "request_simulator_features",
        Command::RequestAgentPreferences => "request_agent_preferences",
        Command::SetAgentPreferences(..) => "set_agent_preferences",
        Command::RequestObjectCost { .. } => "request_object_cost",
        Command::RequestSelectedCost { .. } => "request_selected_cost",
        Command::RequestObjectPhysicsData { .. } => "request_object_physics_data",
        Command::RequestAttachmentResources => "request_attachment_resources",
        Command::RequestLandResources { .. } => "request_land_resources",
        Command::RequestLandStat { .. } => "request_land_stat",
        Command::RequestEstateInfo => "request_estate_info",
        Command::UpdateEstateAccess { .. } => "update_estate_access",
        Command::KickEstateUser { .. } => "kick_estate_user",
        Command::TeleportHomeUser { .. } => "teleport_home_user",
        Command::TeleportHomeAllUsers => "teleport_home_all_users",
        Command::RestartRegion { .. } => "restart_region",
        Command::SendEstateMessage { .. } => "send_estate_message",
        Command::SetRegionInfo(..) => "set_region_info",
        Command::RequestEstateCovenant => "request_estate_covenant",
        Command::RequestTelehubInfo => "request_telehub_info",
        Command::ConnectTelehub { .. } => "connect_telehub",
        Command::DisconnectTelehub => "disconnect_telehub",
        Command::AddTelehubSpawnPoint { .. } => "add_telehub_spawn_point",
        Command::RemoveTelehubSpawnPoint { .. } => "remove_telehub_spawn_point",
        Command::GodKickUser { .. } => "god_kick_user",
        Command::SendGodlikeMessage { .. } => "send_godlike_message",
        Command::RequestMoneyBalance => "request_money_balance",
        Command::RequestEconomyData => "request_economy_data",
        Command::SendMoneyTransfer { .. } => "send_money_transfer",
        Command::SetDrawDistance(..) => "set_draw_distance",
        Command::RequestMapBlocks { .. } => "request_map_blocks",
        Command::RequestMapByName { .. } => "request_map_by_name",
        Command::RequestMapItems { .. } => "request_map_items",
        Command::RequestMapLayer => "request_map_layer",
        Command::SendAbuseReport(..) => "send_abuse_report",
        Command::SendAbuseReportViaCaps { .. } => "send_abuse_report_caps",
        Command::SendPostcard(..) => "send_postcard",
        Command::RequestObjects { .. } => "request_objects",
        Command::RequestObjectProperties { .. } => "request_object_properties",
        Command::DeselectObjects { .. } => "deselect_objects",
        Command::TouchObject { .. } => "touch_object",
        Command::GrabObject { .. } => "grab_object",
        Command::GrabObjectUpdate { .. } => "grab_object_update",
        Command::DegrabObject { .. } => "degrab_object",
        Command::RezObject { .. } => "rez_object",
        Command::DuplicateObjects { .. } => "duplicate_objects",
        Command::DeleteObjects { .. } => "delete_objects",
        Command::DerezObjects { .. } => "derez_objects",
        Command::UpdateObject { .. } => "update_object",
        Command::SetObjectName { .. } => "set_object_name",
        Command::SetObjectDescription { .. } => "set_object_description",
        Command::SetObjectClickAction { .. } => "set_object_click_action",
        Command::SetObjectMaterial { .. } => "set_object_material",
        Command::SetObjectFlags { .. } => "set_object_flags",
        Command::SetObjectGroup { .. } => "set_object_group",
        Command::SetObjectPermissions { .. } => "set_object_permissions",
        Command::SetObjectForSale { .. } => "set_object_for_sale",
        Command::SetObjectCategory { .. } => "set_object_category",
        Command::SetObjectIncludeInSearch { .. } => "set_object_include_in_search",
        Command::LinkObjects { .. } => "link_objects",
        Command::DelinkObjects { .. } => "delink_objects",
        Command::BuyObject { .. } => "buy_object",
        Command::BuyObjectInventory { .. } => "buy_object_inventory",
        Command::RequestPayPrice { .. } => "request_pay_price",
        Command::RequestObjectPropertiesFamily { .. } => "request_object_properties_family",
        Command::SpinObjectStart { .. } => "spin_object_start",
        Command::SpinObjectUpdate { .. } => "spin_object_update",
        Command::SpinObjectStop { .. } => "spin_object_stop",
        Command::DuplicateObjectsOnRay { .. } => "duplicate_objects_on_ray",
        Command::RezRestoreToWorld { .. } => "rez_restore_to_world",
        Command::RezObjectFromNotecard { .. } => "rez_object_from_notecard",
        Command::RequestScriptRunning { .. } => "request_script_running",
        Command::SetScriptRunning { .. } => "set_script_running",
        Command::ResetScript { .. } => "reset_script",
        Command::RequestTexture { .. } => "request_texture",
        Command::RequestAsset { .. } => "request_asset",
        Command::FetchTexture { .. } => "fetch_texture",
        Command::FetchMesh { .. } => "fetch_mesh",
        Command::FetchAsset { .. } => "fetch_asset",
        Command::RequestWearables => "request_wearables",
        Command::SetWearing(..) => "set_wearing",
        Command::SetAppearance { .. } => "set_appearance",
        Command::RequestCachedTextures { .. } => "request_cached_textures",
        Command::RequestServerAppearanceUpdate { .. } => "request_server_appearance_update",
        Command::SetAnimations(..) => "set_animations",
        Command::PlayAnimation(..) => "play_animation",
        Command::StopAnimation(..) => "stop_animation",
        Command::ActivateGestures { .. } => "activate_gestures",
        Command::DeactivateGestures { .. } => "deactivate_gestures",
        Command::SetAlwaysRun { .. } => "set_always_run",
        Command::PauseAgent => "pause_agent",
        Command::ResumeAgent => "resume_agent",
        Command::SetAgentFov { .. } => "set_agent_fov",
        Command::SetAgentSize { .. } => "set_agent_size",
        Command::ReleaseScriptControls => "release_script_controls",
        Command::AttachObject { .. } => "attach_object",
        Command::DetachObjects { .. } => "detach_objects",
        Command::DropAttachments { .. } => "drop_attachments",
        Command::RemoveAttachment { .. } => "remove_attachment",
        Command::RezAttachment(..) => "rez_attachment",
        Command::RezAttachments { .. } => "rez_attachments",
        Command::ViewerEffect(..) => "viewer_effect",
        Command::TrackAgent { .. } => "track_agent",
        Command::FindAgent { .. } => "find_agent",
        Command::DirFindQuery { .. } => "dir_find_query",
        Command::DirPlacesQuery { .. } => "dir_places_query",
        Command::DirLandQuery { .. } => "dir_land_query",
        Command::DirClassifiedQuery { .. } => "dir_classified_query",
        Command::AvatarPickerRequest { .. } => "avatar_picker_request",
        Command::PlacesQuery { .. } => "places_query",
        Command::EventInfoRequest { .. } => "event_info_request",
        Command::EventNotificationAddRequest { .. } => "event_notification_add_request",
        Command::EventNotificationRemoveRequest { .. } => "event_notification_remove_request",
        Command::UploadAssetUdp { .. } => "upload_asset_udp",
        Command::UploadAsset { .. } => "upload_asset",
        Command::UploadBakedTexture { .. } => "upload_baked_texture",
        Command::UpdateInventoryAsset { .. } => "update_inventory_asset",
        Command::RequestObjectMedia { .. } => "request_object_media",
        Command::SetObjectMedia { .. } => "set_object_media",
        Command::NavigateObjectMedia { .. } => "navigate_object_media",
        Command::RequestRenderMaterials { .. } => "request_render_materials",
        Command::ModifyMaterialParams { .. } => "modify_material_params",
        Command::RequestVoiceAccount { .. } => "request_voice_account",
        Command::RequestParcelVoiceInfo => "request_parcel_voice_info",
        Command::SendVoiceSignaling { .. } => "send_voice_signaling",
        Command::RequestExperienceInfo { .. } => "request_experience_info",
        Command::FindExperiences { .. } => "find_experiences",
        Command::RequestExperiencePermissions => "request_experience_permissions",
        Command::SetExperiencePermission { .. } => "set_experience_permission",
        Command::RequestOwnedExperiences => "request_owned_experiences",
        Command::RequestAdminExperiences => "request_admin_experiences",
        Command::RequestCreatorExperiences => "request_creator_experiences",
        Command::RequestGroupExperiences { .. } => "request_group_experiences",
        Command::RequestExperienceAdmin { .. } => "request_experience_admin",
        Command::RequestExperienceContributor { .. } => "request_experience_contributor",
        Command::UpdateExperience { .. } => "update_experience",
        Command::RequestRegionExperiences => "request_region_experiences",
        Command::SetRegionExperiences { .. } => "set_region_experiences",
        Command::OfferTeleport { .. } => "offer_teleport",
        Command::AcceptTeleportLure { .. } => "accept_teleport_lure",
        Command::DeclineTeleportLure { .. } => "decline_teleport_lure",
        Command::RequestTeleport { .. } => "request_teleport",
        Command::GiveInventory { .. } => "give_inventory",
        Command::GiveInventoryFolder { .. } => "give_inventory_folder",
        Command::AcceptInventoryOffer { .. } => "accept_inventory_offer",
        Command::DeclineInventoryOffer { .. } => "decline_inventory_offer",
        Command::StartConference { .. } => "start_conference",
        Command::SendConferenceMessage { .. } => "send_conference_message",
        Command::LeaveConference { .. } => "leave_conference",
        Command::RetrieveInstantMessages => "retrieve_instant_messages",
        Command::RequestOfflineMessages => "request_offline_messages",
        Command::Logout => "logout",
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use pretty_assertions::assert_eq;
    use sl_proto::{
        AgentKey, Command, Diagnostic, Event, MessageId, RegionHandle, Uuid, WireError,
    };

    use super::{format_command, format_diagnostic, format_event, hexdump};
    use crate::context::{NoContext, SessionContext};

    /// A UUID built from a single repeated hex nibble, for stable test ids.
    fn uuid(n: char) -> Uuid {
        let group = |len: usize| n.to_string().repeat(len);
        let text = format!(
            "{}-{}-{}-{}-{}",
            group(8),
            group(4),
            group(4),
            group(4),
            group(12)
        );
        Uuid::parse_str(&text).unwrap_or_else(|_| Uuid::nil())
    }

    #[test]
    fn command_renders_name_and_literal_fields() {
        let formatted = format_command(
            &Command::Chat {
                message: "hi there".to_owned(),
                chat_type: sl_proto::ChatType::Normal,
                channel: 0,
            },
            &NoContext,
        );
        assert!(
            formatted.starts_with("chat "),
            "the registry name leads the line: {formatted}"
        );
        assert!(
            formatted.contains("\"hi there\""),
            "the message is rendered literally: {formatted}"
        );
    }

    #[test]
    fn command_symbolizes_session_bindings() {
        let mut ctx = SessionContext::new();
        ctx.set_identity(uuid('1'), uuid('2'), sl_proto::CircuitCode(7));
        let formatted = format_command(
            &Command::InstantMessage {
                to_agent_id: AgentKey::from(uuid('1')),
                message: "hello".to_owned(),
            },
            &ctx,
        );
        assert!(
            formatted.starts_with("im "),
            "InstantMessage is named `im`: {formatted}"
        );
        assert!(
            formatted.contains("$self"),
            "the agent id is symbolized to $self: {formatted}"
        );
        assert!(
            !formatted.contains(&uuid('1').to_string()),
            "the literal agent id no longer appears: {formatted}"
        );
    }

    #[test]
    fn event_symbolizes_region_handle() {
        let sim: SocketAddr = "127.0.0.1:9000"
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 9000)));
        let mut ctx = SessionContext::new();
        ctx.set_region(RegionHandle(1_099_511_628_032), "Da Boom");
        let formatted = format_event(
            &Event::RegionChanged {
                region_handle: RegionHandle(1_099_511_628_032),
                sim,
                circuit: sl_proto::CircuitId(1),
            },
            &ctx,
        );
        assert!(
            formatted.starts_with("region_changed "),
            "the snake-case event name leads the line: {formatted}"
        );
        assert!(
            formatted.contains("$region"),
            "the region handle is symbolized: {formatted}"
        );
    }

    #[test]
    fn diagnostic_renders_literally() {
        assert_eq!(
            format_diagnostic(&Diagnostic::UnknownCapsEvent {
                message: "WeirdEvent".to_owned(),
            }),
            "UnknownCapsEvent message=WeirdEvent"
        );
        assert_eq!(
            format_diagnostic(&Diagnostic::ExpectedReplyMissing {
                request: "Logout".to_owned(),
                sequence: Some(sl_proto::SequenceNumber(42)),
            }),
            "ExpectedReplyMissing request=Logout sequence=42"
        );
        assert_eq!(
            format_diagnostic(&Diagnostic::ExpectedReplyMissing {
                request: "Sit".to_owned(),
                sequence: None,
            }),
            "ExpectedReplyMissing request=Sit sequence=-"
        );
    }

    #[test]
    fn decode_failed_renders_header_and_marked_hexdump() {
        // These are exactly the fields a live `Session` captures for an unknown
        // High(0) id with a two-byte body (see `sl-proto`'s
        // `unknown_message_id_surfaces_decode_failed_with_offset` lifecycle
        // test): decoding stops after the one-byte id prefix, so the failing
        // offset is 1 and the captured bytes are the id byte plus the body.
        let rendered = format_diagnostic(&Diagnostic::DecodeFailed {
            id: MessageId::High(0),
            name: None,
            error: WireError::UnknownMessage {
                id: MessageId::High(0),
            },
            raw: vec![0x00, 0xAA, 0xBB],
            failed_offset: 1,
        });
        assert!(
            rendered.contains("DecodeFailed id=High(0)"),
            "the header names the undecodable id: {rendered}"
        );
        assert!(
            rendered.contains("name=?"),
            "an id owned by no template renders as `?`: {rendered}"
        );
        assert!(
            rendered.contains("failed_offset=1"),
            "the header carries the failing offset: {rendered}"
        );
        assert!(
            rendered.contains("[aa]"),
            "the byte at the failing offset is bracketed in the hexdump: {rendered}"
        );
        assert!(
            rendered.contains(" bb "),
            "the trailing byte stays unmarked: {rendered}"
        );
    }

    #[test]
    fn hexdump_marks_the_failing_byte() {
        let bytes = [0x00, 0x41, 0x42, 0xff];
        let dump = hexdump(&bytes, Some(2));
        assert!(
            dump.contains("[42]"),
            "the marked byte is bracketed: {dump}"
        );
        assert!(
            dump.contains(" 41 "),
            "an unmarked byte is not bracketed: {dump}"
        );
        assert!(
            dump.contains("|.AB.|"),
            "the ASCII gutter renders printable bytes: {dump}"
        );
        assert!(
            dump.starts_with("00000000 "),
            "the row carries its offset: {dump}"
        );
    }

    #[test]
    fn hexdump_notes_an_end_of_data_mark() {
        let bytes = [0x01, 0x02];
        let dump = hexdump(&bytes, Some(2));
        assert!(
            dump.contains("end of 2 bytes"),
            "a mark at the end is noted: {dump}"
        );
    }
}
