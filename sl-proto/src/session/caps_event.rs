//! The CAPS event tag space, as a type.
//!
//! Everything the client learns over HTTP rather than UDP arrives at
//! [`Session::handle_caps_event`](crate::Session::handle_caps_event) as a
//! `(&str, Llsd)` pair: either an event-queue message (`ParcelProperties`,
//! `TeleportFinish`, …) or the name of the capability whose POST the driver
//! performed on the session's behalf, standing in for "this is the answer to
//! that request" (`GetObjectCost`, `InventoryAPIv3`, …). A handful of
//! capabilities answer more than one kind of request, so those carry a stamped
//! sub-tag instead ([`AVATAR_PICKER_SEARCH_TAG`],
//! [`CHAT_SESSION_FETCH_HISTORY_TAG`], …).
//!
//! Matching that space as raw `&str` made every tag a spelling the compiler
//! never checked: a mistyped arm silently fell through to "unknown CAPS event"
//! at runtime. [`CapsEvent`] names it once, [`CapsEvent::from_tag`] is the only
//! place a wire string is compared, and the handler's match over it is
//! exhaustive — so a new tag cannot be added without the handler being told.

use super::{
    AVATAR_PICKER_SEARCH_TAG, CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES,
    CAP_ATTACHMENT_RESOURCES, CAP_CHAT_SESSION_REQUEST, CAP_CREATE_INVENTORY_CATEGORY,
    CAP_EXPERIENCE_PREFERENCES, CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_INVENTORY_ITEM,
    CAP_FETCH_LIBRARY, CAP_FETCH_LIBRARY_ITEM, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_OBJECT_COST, CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_GROUP_MEMBER_DATA, CAP_INCREMENT_COF_VERSION, CAP_INVENTORY_API_V3, CAP_LAND_RESOURCES,
    CAP_LIBRARY_API_V3, CAP_LSL_SYNTAX, CAP_MODIFY_MATERIAL_PARAMS, CAP_OBJECT_MEDIA,
    CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS,
    CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RESOURCE_COST_SELECTED,
    CAP_SIMULATOR_FEATURES, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, CAP_USER_INFO,
    CHAT_SESSION_FETCH_HISTORY_TAG, EXPERIENCE_QUERY_TAG, LAND_RESOURCE_DETAIL_TAG,
    LAND_RESOURCE_SUMMARY_TAG,
};

/// One recognised CAPS event tag.
///
/// The variants are named after the wire tag, not after what the handler does
/// with them, so the mapping in [`CapsEvent::from_tag`] reads as a table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CapsEvent {
    /// A parcel's full properties, pushed over the event queue.
    ParcelProperties,
    /// The region's (or the parcel's) environment settings.
    ExtEnvironment,
    /// A task script's run state, in place of the UDP `ScriptRunningReply`.
    ScriptRunningReply,
    /// The region's top-scripts / top-colliders report.
    LandStatReply,
    /// A parcel's object-owner tally.
    ParcelObjectOwnersReply,
    /// The destination region of a teleport that is going through.
    TeleportFinish,
    /// A neighbour region the simulator wants a child circuit to.
    EnableSimulator,
    /// The child-agent endpoint for a neighbour, with its seed capability.
    EstablishAgentCommunication,
    /// A physical region border crossing handing the agent to a neighbour.
    CrossedRegion,
    /// A `FetchInventoryDescendents2` folder-contents reply.
    FetchInventoryDescendents,
    /// A `FetchLibDescendents2` folder-contents reply (the Library tree).
    FetchLibraryDescendents,
    /// A `FetchInventory2` per-item reply.
    FetchInventoryItem,
    /// A `FetchLib2` per-item reply (the Library tree).
    FetchLibraryItem,
    /// A bulk inventory update delivered over the event queue.
    BulkUpdateInventory,
    /// The reply to an AIS3 operation on the agent's tree.
    InventoryApiV3,
    /// The reply to an AIS3 operation on the Library tree.
    LibraryApiV3,
    /// The folder a `CreateInventoryCategory` POST created.
    CreateInventoryCategory,
    /// The agent's group memberships.
    AgentGroupDataUpdate,
    /// A group's member roster.
    GroupMemberData,
    /// The server's answer to an appearance (baked-texture) update.
    UpdateAvatarAppearance,
    /// The current-outfit folder's new version.
    IncrementCofVersion,
    /// An object's media (`MOAP`) faces.
    ObjectMedia,
    /// The reply to a GLTF material-parameter edit.
    ModifyMaterialParams,
    /// The voice account credentials for this session.
    ProvisionVoiceAccount,
    /// The voice channel of the parcel the agent stands on.
    ParcelVoiceInfo,
    /// A display-name lookup reply.
    GetDisplayNames,
    /// An avatar-picker search reply, stamped with its query id.
    AvatarPickerSearchReply,
    /// A parcel id looked up from a region handle and a position.
    RemoteParcelRequest,
    /// The region's feature/limit advertisement.
    SimulatorFeatures,
    /// The region's LSL syntax definition (for the script editor).
    LslSyntax,
    /// The account's server-side viewer preferences.
    AgentPreferences,
    /// The account's email address and its verification state.
    UserInfo,
    /// The land-impact cost of a set of objects.
    GetObjectCost,
    /// The land-impact cost of the current selection.
    ResourceCostSelected,
    /// The physics shape parameters of a set of objects.
    GetObjectPhysicsData,
    /// An object's physics shape type / material, pushed over the event queue.
    ObjectPhysicsProperties,
    /// The script resources an attachment uses.
    AttachmentResources,
    /// A parcel's script-resource report (the request's own reply).
    LandResources,
    /// A parcel's script-resource summary, fetched from the report's URL.
    LandResourceSummary,
    /// A parcel's per-object script-resource detail, likewise.
    LandResourceDetail,
    /// Experience metadata for a set of experience keys.
    GetExperienceInfo,
    /// An experience search page.
    FindExperienceByName,
    /// The experiences the agent has admin/contributor rights over.
    GetExperiences,
    /// The agent's per-experience allow/block preferences.
    ExperiencePreferences,
    /// The experiences the agent has accepted.
    AgentExperiences,
    /// The experiences the agent administers.
    GetAdminExperiences,
    /// The experiences the agent created.
    GetCreatorExperiences,
    /// The experience an update POST wrote back.
    UpdateExperience,
    /// The experiences allowed, blocked or trusted in this region.
    RegionExperiences,
    /// The experience a parcel runs, stamped with the queried parcel.
    ExperienceQueryReply,
    /// The offline IMs queued since the last login.
    ReadOfflineMsgs,
    /// An invitation into a group or conference IM session.
    ChatterBoxInvitation,
    /// The reply to a chat-session request (join, leave, moderate…).
    ChatSessionRequest,
    /// A page of a chat session's server-side history.
    ChatSessionFetchHistory,
    /// The confirmation that a chat session has started.
    ChatterBoxSessionStartReply,
    /// A chat session's participant roster, as a delta.
    ChatterBoxSessionAgentListUpdates,
    /// The agent's god level, flying-allowed and similar region-scoped state.
    AgentStateUpdate,
    /// The region's navmesh (pathfinding) bake state.
    NavMeshStatusUpdate,
    /// A group the agent was dropped from.
    AgentDropGroup,
    /// Another agent's display name changed.
    DisplayNameUpdate,
    /// The reply to this agent's own display-name change.
    SetDisplayNameReply,
    /// The region asking the viewer to re-read its environment.
    WindLightRefresh,
    /// The output of a `SimConsole` command.
    SimConsoleResponse,
    /// The voice server version the region requires.
    RequiredVoiceVersion,
    /// OpenSim's region-wide viewer limits (`OpenRegionInfo`).
    OpenRegionInfo,
}

impl CapsEvent {
    /// The event this wire tag names, or `None` for one the session does not
    /// handle (surfaced as a
    /// [`Diagnostic::UnknownCapsEvent`](crate::Diagnostic::UnknownCapsEvent)).
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "ParcelProperties" => Self::ParcelProperties,
            CAP_EXT_ENVIRONMENT => Self::ExtEnvironment,
            "ScriptRunningReply" => Self::ScriptRunningReply,
            "LandStatReply" => Self::LandStatReply,
            "ParcelObjectOwnersReply" => Self::ParcelObjectOwnersReply,
            "TeleportFinish" => Self::TeleportFinish,
            "EnableSimulator" => Self::EnableSimulator,
            "EstablishAgentCommunication" => Self::EstablishAgentCommunication,
            "CrossedRegion" => Self::CrossedRegion,
            CAP_FETCH_INVENTORY => Self::FetchInventoryDescendents,
            CAP_FETCH_LIBRARY => Self::FetchLibraryDescendents,
            CAP_FETCH_INVENTORY_ITEM => Self::FetchInventoryItem,
            CAP_FETCH_LIBRARY_ITEM => Self::FetchLibraryItem,
            "BulkUpdateInventory" => Self::BulkUpdateInventory,
            CAP_INVENTORY_API_V3 => Self::InventoryApiV3,
            CAP_LIBRARY_API_V3 => Self::LibraryApiV3,
            CAP_CREATE_INVENTORY_CATEGORY => Self::CreateInventoryCategory,
            "AgentGroupDataUpdate" => Self::AgentGroupDataUpdate,
            CAP_GROUP_MEMBER_DATA => Self::GroupMemberData,
            CAP_UPDATE_AVATAR_APPEARANCE => Self::UpdateAvatarAppearance,
            CAP_INCREMENT_COF_VERSION => Self::IncrementCofVersion,
            CAP_OBJECT_MEDIA => Self::ObjectMedia,
            CAP_MODIFY_MATERIAL_PARAMS => Self::ModifyMaterialParams,
            CAP_PROVISION_VOICE_ACCOUNT => Self::ProvisionVoiceAccount,
            CAP_PARCEL_VOICE_INFO => Self::ParcelVoiceInfo,
            CAP_GET_DISPLAY_NAMES => Self::GetDisplayNames,
            AVATAR_PICKER_SEARCH_TAG => Self::AvatarPickerSearchReply,
            CAP_REMOTE_PARCEL_REQUEST => Self::RemoteParcelRequest,
            CAP_SIMULATOR_FEATURES => Self::SimulatorFeatures,
            CAP_LSL_SYNTAX => Self::LslSyntax,
            CAP_AGENT_PREFERENCES => Self::AgentPreferences,
            CAP_USER_INFO => Self::UserInfo,
            CAP_GET_OBJECT_COST => Self::GetObjectCost,
            CAP_RESOURCE_COST_SELECTED => Self::ResourceCostSelected,
            CAP_GET_OBJECT_PHYSICS_DATA => Self::GetObjectPhysicsData,
            "ObjectPhysicsProperties" => Self::ObjectPhysicsProperties,
            CAP_ATTACHMENT_RESOURCES => Self::AttachmentResources,
            CAP_LAND_RESOURCES => Self::LandResources,
            LAND_RESOURCE_SUMMARY_TAG => Self::LandResourceSummary,
            LAND_RESOURCE_DETAIL_TAG => Self::LandResourceDetail,
            CAP_GET_EXPERIENCE_INFO => Self::GetExperienceInfo,
            CAP_FIND_EXPERIENCE_BY_NAME => Self::FindExperienceByName,
            CAP_GET_EXPERIENCES => Self::GetExperiences,
            CAP_EXPERIENCE_PREFERENCES => Self::ExperiencePreferences,
            CAP_AGENT_EXPERIENCES => Self::AgentExperiences,
            CAP_GET_ADMIN_EXPERIENCES => Self::GetAdminExperiences,
            CAP_GET_CREATOR_EXPERIENCES => Self::GetCreatorExperiences,
            CAP_UPDATE_EXPERIENCE => Self::UpdateExperience,
            CAP_REGION_EXPERIENCES => Self::RegionExperiences,
            EXPERIENCE_QUERY_TAG => Self::ExperienceQueryReply,
            CAP_READ_OFFLINE_MSGS => Self::ReadOfflineMsgs,
            "ChatterBoxInvitation" => Self::ChatterBoxInvitation,
            CAP_CHAT_SESSION_REQUEST => Self::ChatSessionRequest,
            CHAT_SESSION_FETCH_HISTORY_TAG => Self::ChatSessionFetchHistory,
            "ChatterBoxSessionStartReply" => Self::ChatterBoxSessionStartReply,
            "ChatterBoxSessionAgentListUpdates" => Self::ChatterBoxSessionAgentListUpdates,
            "AgentStateUpdate" => Self::AgentStateUpdate,
            "NavMeshStatusUpdate" => Self::NavMeshStatusUpdate,
            "AgentDropGroup" => Self::AgentDropGroup,
            "DisplayNameUpdate" => Self::DisplayNameUpdate,
            "SetDisplayNameReply" => Self::SetDisplayNameReply,
            "WindLightRefresh" => Self::WindLightRefresh,
            "SimConsoleResponse" => Self::SimConsoleResponse,
            "RequiredVoiceVersion" => Self::RequiredVoiceVersion,
            "OpenRegionInfo" => Self::OpenRegionInfo,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod test {
    use super::CapsEvent;
    use pretty_assertions::{assert_eq, assert_ne};

    use crate::session::{
        AVATAR_PICKER_SEARCH_TAG, CAP_FETCH_LIBRARY, CAP_LIBRARY_API_V3,
        CHAT_SESSION_FETCH_HISTORY_TAG,
    };

    /// The three tag shapes the space is made of all resolve: a bare
    /// event-queue message name, a capability name standing in for the answer
    /// to its POST, and a stamped per-request sub-tag.
    #[test]
    fn each_tag_shape_resolves() {
        assert_eq!(
            CapsEvent::from_tag("TeleportFinish"),
            Some(CapsEvent::TeleportFinish)
        );
        assert_eq!(
            CapsEvent::from_tag(CAP_LIBRARY_API_V3),
            Some(CapsEvent::LibraryApiV3)
        );
        assert_eq!(
            CapsEvent::from_tag(CHAT_SESSION_FETCH_HISTORY_TAG),
            Some(CapsEvent::ChatSessionFetchHistory)
        );
        assert_eq!(
            CapsEvent::from_tag(AVATAR_PICKER_SEARCH_TAG),
            Some(CapsEvent::AvatarPickerSearchReply)
        );
    }

    /// The agent tree and the Library tree share every fetch shape but are
    /// separate tags, and the handler routes the reply by which one arrived —
    /// so they must not collapse onto one variant.
    #[test]
    fn the_library_tags_are_their_own_events() {
        assert_eq!(
            CapsEvent::from_tag(CAP_FETCH_LIBRARY),
            Some(CapsEvent::FetchLibraryDescendents)
        );
        assert_ne!(
            CapsEvent::from_tag(CAP_FETCH_LIBRARY),
            CapsEvent::from_tag("FetchInventoryDescendents2")
        );
    }

    /// A tag the session does not handle stays unhandled rather than being
    /// mistaken for a neighbour: the handler surfaces it as a diagnostic.
    #[test]
    fn an_unknown_tag_has_no_event() {
        assert_eq!(CapsEvent::from_tag("NoSuchCapsEvent"), None);
        assert_eq!(CapsEvent::from_tag(""), None);
        // A capability the session POSTs to but whose answer it never feeds
        // back through the event handler.
        assert_eq!(CapsEvent::from_tag("UploadBakedTexture"), None);
    }
}
