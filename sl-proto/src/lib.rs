#![doc = include_str!("../README.md")]

mod error;
mod session;
mod types;

pub use error::Error;
pub use session::{CAP_FETCH_INVENTORY, CAP_GROUP_MEMBER_DATA, REQUESTED_CAPABILITIES, Session};
pub use types::{
    ActiveGroup, AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties, ChatAudible,
    ChatMessage, ChatSourceType, ChatType, CreateGroupParams, DisconnectReason, EconomyData,
    EstateAccessDelta, EstateAccessKind, EstateInfo, Event, Friend, FriendRights, GroupMember,
    GroupMembership, GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle, ImDialog,
    InstantMessage, InventoryFolder, InventoryItem, LoadUrlRequest, LoginHttpRequest, LoginParams,
    MapItem, MapItemType, MapRegionInfo, Maturity, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NeighborInfo, ParcelAccessEntry,
    ParcelAccessScope, ParcelCategory, ParcelInfo, ParcelOverlayInfo, ParcelReturnType,
    ParcelUpdate, ProductType, RegionIdentity, RegionInfoUpdate, RegionLimits, Reliability,
    ScriptDialog, ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest, Transmit,
    grid_to_handle, handle_to_global, handle_to_grid,
};

// Re-export `Uuid`: it appears in public types (chat/IM ids) and command APIs,
// so consumers can name it without depending on `uuid` directly.
pub use uuid::Uuid;

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AnyMessage, ControlFlags, EventQueueEvent, EventQueueResponse, Llsd, LoginRequest,
    LoginResponse, MfaChallenge, ParcelFlags, RegionFlags, WireError, build_event_queue_request,
    build_fetch_inventory_request, build_group_member_data_request, build_login_request,
    build_seed_request, parse_event_queue_response, parse_llsd_xml, parse_login_response,
    parse_seed_response, sim_access,
};
// Re-export the vector and rotation types used by the teleport and movement APIs.
pub use sl_types::lsl::{Rotation, Vector};
// Re-export the L$ amount type used by the money balance/transfer APIs.
pub use sl_types::money::LindenAmount;
