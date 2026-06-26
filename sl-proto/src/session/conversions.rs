//! Wire/LLSD <-> value-type converters shared by the session impls, plus the
//! server-side CAPS serializers and their round-trip tests.

use crate::GroupRoleKey;
use crate::appearance;
use crate::types::{
    ActiveGroup, AssetType, AvatarAppearance, AvatarAttachment, AvatarGroupMembership,
    AvatarInterests, AvatarName, AvatarProperties, ChatAudible, ChatMessage, ChatSource, ChatType,
    ClassifiedCategory, ClassifiedInfo, CloudPosDensity, Color, ColorAlpha, DayCycle,
    DayCycleFrame, DisplayNameUpdate, EconomyData, EnvironmentSettings, EstateAccessKind,
    EstateInfo, Event, Friend, FriendRights, Glow, GroupAccountDetails, GroupAccountDetailsEntry,
    GroupAccountSummary, GroupAccountTransaction, GroupAccountTransactions,
    GroupActiveProposalItem, GroupMember, GroupMembership, GroupName, GroupNotice, GroupNoticeKey,
    GroupProfile, GroupRole, GroupTitle, GroupVote, GroupVoteHistoryItem, ImDialog, InstantMessage,
    InventoryFolder, InventoryItem, LandingType, MapItem, MapItemType, MapLayer, MapRegionInfo,
    MapRequestFlags, Maturity, MoneyBalance, MoneyTransaction, MuteEntry, MuteFlags, MuteType,
    NavMeshBuildStatus, NavMeshStatus, NeighborInfo, Object, ObjectProperties, OpenRegionInfo,
    ParcelCategory, ParcelInfo, ParcelRequestResult, ParcelStatus, PickInfo, PickKey,
    PlayingAnimation, PrimShapeParams, ProductType, ProposalCandidateId, ProposalVoteId,
    RegionChatSettings, RegionCombatSettings, RegionIdentity, RegionLimits, RequiredVoiceVersion,
    Scale, ScriptDialog, ScriptPermissionRequest, ScriptPermissions, SetDisplayNameReply,
    SkySettings, WaterSettings, avatar_texture,
};
use sl_types::chat::ChatChannel;
use sl_types::key::AgentKey;
use sl_types::key::ClassifiedKey;
use sl_types::key::ExperienceKey;
use sl_types::key::FriendKey;
use sl_types::key::GroupKey;
use sl_types::key::InventoryFolderKey;
use sl_types::key::InventoryKey;
use sl_types::key::ObjectKey;
use sl_types::key::ParcelKey;
use sl_types::key::TextureKey;
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::GridCoordinates;
use sl_types::map::GridRectangle;
use sl_types::map::GridRectangleLike as _;
use sl_types::map::RegionCoordinates;
use sl_wire::DisplayName;
use sl_wire::RegionHandle;
use sl_wire::messages::{
    AgentDataUpdateAgentDataBlock, AgentGroupDataUpdateGroupDataBlock,
    AvatarGroupsReplyGroupDataBlock, AvatarInterestsReplyPropertiesDataBlock,
    AvatarPropertiesReplyPropertiesDataBlock, BulkUpdateInventoryFolderDataBlock,
    BulkUpdateInventoryItemDataBlock, ChatFromSimulatorChatDataBlock, ClassifiedInfoReplyDataBlock,
    EnableSimulatorSimulatorInfoBlock, EstateOwnerMessageParamListBlock, GroupAccountDetailsReply,
    GroupAccountSummaryReply, GroupAccountTransactionsReply,
    GroupActiveProposalItemReplyProposalDataBlock, GroupMembersReplyMemberDataBlock,
    GroupNoticesListReplyDataBlock, GroupProfileReplyGroupDataBlock,
    GroupRoleDataReplyRoleDataBlock, GroupTitlesReplyGroupDataBlock, GroupVoteHistoryItemReply,
    ImprovedInstantMessageAgentDataBlock, ImprovedInstantMessageMessageBlockBlock,
    InventoryDescendentsFolderDataBlock, InventoryDescendentsItemDataBlock, MapBlockReply,
    MapBlockReplyAgentDataBlock, MapBlockReplyDataBlock, MapBlockReplySizeBlock, MapItemReply,
    MapItemReplyAgentDataBlock, MapItemReplyDataBlock, MapItemReplyRequestDataBlock, MapLayerReply,
    MapLayerReplyAgentDataBlock, MapLayerReplyLayerDataBlock, ObjectPropertiesObjectDataBlock,
    ObjectUpdateObjectDataBlock, ParcelProperties, PickInfoReplyDataBlock, UUIDGroupNameReply,
    UUIDNameReply, UpdateCreateInventoryItemInventoryDataBlock,
};
use sl_wire::{Direction, GlobalCoordinates};
use sl_wire::{Llsd, SkeletonFolder};
use sl_wire::{Permissions, Permissions5};
use sl_wire::{RegionLocalObjectId, RegionLocalParcelId};
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use uuid::Uuid;

/// Decodes name/SKU bytes to a `String`, dropping any trailing NUL padding the
/// simulator appends to fixed-width string fields.
pub(crate) fn trimmed_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

/// Decodes a region handle's grid coordinates (region index pair) into the typed
/// [`GridCoordinates`]. A real grid index always fits the `u16` the type holds;
/// the only way [`GridCoordinates::try_from`] can fail is a malformed handle, in
/// which case we fall back to the `(0, 0)` "unknown" sentinel.
pub(crate) fn grid_coordinates_from_handle(handle: RegionHandle) -> GridCoordinates {
    GridCoordinates::from(handle)
}

/// Parses a UUID from a wire string field that carries a UUID in text form
/// (e.g. the `Creator` of an `EventInfoReply`), dropping any trailing NUL
/// padding. A present-but-unparsable value is a hard
/// [`WireError::InvalidUuid`] rather than the silently-coerced nil UUID the
/// viewer's `LLUUID(buffer)` would produce.
pub(crate) fn parse_uuid_string(
    field: &'static str,
    bytes: &[u8],
) -> Result<Uuid, sl_wire::WireError> {
    parse_uuid_field(field, trimmed_string(bytes).trim())
}

/// Converts a wire array index that uses a negative value to mean "absent"
/// (e.g. the `You`/`Prey` fields of `CoarseLocationUpdate`) into an
/// `Option<usize>`: negative values become `None`.
pub(crate) fn index_into(index: i16) -> Option<usize> {
    usize::try_from(index).ok()
}

/// Encodes a string as NUL-terminated UTF-8 bytes, as the viewer sends variable
/// string fields on the wire.
pub(crate) fn with_nul(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Builds an inventory-offer binary bucket: the asset-type byte followed by the
/// item's (or folder's) 16 raw id bytes, as the viewer's give-inventory path
/// packs it (#28). An asset type whose code does not fit the single wire byte is
/// a hard [`WireError::ValueOutOfRange`] rather than a silently-written `0`
/// (which would mistag the offer as a texture).
pub(crate) fn inventory_offer_bucket(
    asset_type: AssetType,
    id: Uuid,
) -> Result<Vec<u8>, sl_wire::WireError> {
    let code =
        u8::try_from(asset_type.to_code()).map_err(|_err| sl_wire::WireError::ValueOutOfRange {
            field: "inventory_offer_asset_type",
            value: i64::from(asset_type.to_code()),
        })?;
    let mut bucket = Vec::with_capacity(17);
    bucket.push(code);
    bucket.extend_from_slice(id.as_bytes());
    Ok(bucket)
}

/// Concatenates the raw 16-byte ids of `uuids` (the conference-start invitee
/// bucket form, #28).
pub(crate) fn pack_uuids(uuids: &[Uuid]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(uuids.len().saturating_mul(16));
    for id in uuids {
        packed.extend_from_slice(id.as_bytes());
    }
    packed
}

/// Extracts the region handle encoded in a teleport lure id (OpenSim's
/// `BuildFakeParcelID`: the handle is the first eight little-endian bytes).
/// Returns `0` for an id that is not a fake parcel id (e.g. a Second Life lure
/// id), in which case the destination is learned from `TeleportFinish` instead.
pub(crate) fn parse_lure_region_handle(lure_id: Uuid) -> RegionHandle {
    RegionHandle(sl_wire::Reader::new(lure_id.as_bytes()).u64().unwrap_or(0))
}

/// A fully-specified outgoing `ImprovedInstantMessage`, the argument of
/// [`send_im`](Circuit::send_im). Groups the dialog-dependent fields so the
/// offer-reply / give-inventory / conference flows (#28) share one builder.
pub(crate) struct OutgoingIm<'a> {
    /// The `ToAgentID` wire field. Its meaning is dialog-dependent: an agent id
    /// for a 1:1 IM, a group id for a group notice / group-session message, or
    /// an ad-hoc session id for a conference message — so it is a raw `Uuid`
    /// rather than any one typed key.
    pub(crate) to_agent_id: Uuid,
    /// Whether the message is from a group (sets the `FromGroup` flag).
    pub(crate) from_group: bool,
    /// The IM dialog (sub-type).
    pub(crate) dialog: ImDialog,
    /// The dialog-dependent id (session id, transaction id, or lure id).
    pub(crate) id: Uuid,
    /// The message text (encoded NUL-terminated).
    pub(crate) message: &'a str,
    /// The sender's display name (encoded NUL-terminated).
    pub(crate) from_name: &'a str,
    /// The dialog-dependent binary payload (e.g. a destination folder id, an
    /// offered asset's type+id, or a conference's invitee ids).
    pub(crate) binary_bucket: Vec<u8>,
}

/// Parses a downloaded mute-list file into [`MuteEntry`] values. Each non-empty
/// line is `<type> <uuid> <name>|<flags>` (the viewer's on-disk format).
pub(crate) fn parse_mute_list(bytes: &[u8]) -> Result<Vec<MuteEntry>, sl_wire::WireError> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| parse_mute_line(line).transpose())
        .collect()
}

/// Parses one mute-list line: `Ok(None)` for a blank line, `Ok(Some(_))` for a
/// well-formed entry, and a hard [`WireError`] for a present-but-malformed line
/// (a non-numeric type/flags or an unparsable UUID) — rather than silently
/// coercing the type/flags to `0` or the id to the nil UUID (which would mute
/// nobody, or the wrong avatar).
pub(crate) fn parse_mute_line(line: &str) -> Result<Option<MuteEntry>, sl_wire::WireError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    // The flags follow the last '|'; everything before is "<type> <uuid> <name>".
    // A line with no flags suffix carries no flags (the viewer omits it).
    let (head, flags) = match line.rsplit_once('|') {
        Some((head, tail)) => (head, parse_u32_field("mute_flags", tail)?),
        None => (line, 0),
    };
    let mut parts = head.splitn(3, ' ');
    let type_text = parts.next().unwrap_or("").trim();
    let mute_type =
        type_text
            .parse::<i32>()
            .map_err(|_err| sl_wire::WireError::MalformedField {
                field: "mute_type",
                value: type_text.to_owned(),
            })?;
    let id = parse_uuid_field("mute_id", parts.next().unwrap_or("").trim())?;
    let name = parts.next().unwrap_or("").trim().to_owned();
    Ok(Some(MuteEntry {
        id,
        name,
        mute_type: MuteType::from_i32(mute_type),
        flags: MuteFlags(flags),
    }))
}

/// Builds a [`RegionIdentity`] from a `RegionHandshake`'s region-info blocks. The
/// 64-bit flags / protocols come from the optional `RegionInfo4` block (absent on
/// OpenSim and older grids), falling back to the zero-extended 32-bit flags. The
/// `RegionHandshake` does not carry the region handle, so `region_handle` is
/// passed in by the caller (the handle the session has learned for the
/// simulator); its grid coordinates are derived from it.
///
/// # Errors
///
/// Returns a [`WireError`](sl_wire::WireError) when the region name is non-empty
/// but invalid.
pub(crate) fn region_identity(
    handshake: &sl_wire::messages::RegionHandshake,
    region_handle: RegionHandle,
) -> Result<RegionIdentity, sl_wire::WireError> {
    let info = &handshake.region_info;
    let info3 = &handshake.region_info3;
    let product_sku = trimmed_string(&info3.product_sku);
    let product_name = trimmed_string(&info3.product_name);
    let info4 = handshake.region_info4.first();
    let region_flags_extended = info4.map_or_else(
        || u64::from(info.region_flags),
        |i4| i4.region_flags_extended,
    );
    let region_protocols = info4.map_or(0, |i4| i4.region_protocols);
    Ok(RegionIdentity {
        sim_name: sl_wire::region_name_from_wire("SimName", &trimmed_string(&info.sim_name))?,
        region_id: handshake.region_info2.region_id,
        region_handle,
        grid_coordinates: grid_coordinates_from_handle(region_handle),
        region_flags: info.region_flags,
        region_flags_extended,
        region_protocols,
        maturity: Maturity::from_sim_access(info.sim_access),
        product: ProductType::classify(&product_sku, &product_name),
        product_sku,
        product_name,
        cpu_class_id: info3.cpu_class_id,
        cpu_ratio: info3.cpu_ratio,
        sim_owner: info.sim_owner,
        is_estate_manager: info.is_estate_manager,
        water_height: info.water_height,
        billable_factor: info.billable_factor,
    })
}

/// Builds a `RegionHandshake` message from a [`RegionIdentity`] — the server-side
/// inverse of [`region_identity`]. The grid coordinates / handle are *not* wire
/// fields of the handshake (the client derives them from the circuit), so they
/// are not encoded here; the terrain texture/height fields are left at their
/// defaults.
pub(crate) fn region_handshake_message(
    identity: &RegionIdentity,
) -> sl_wire::messages::RegionHandshake {
    use sl_wire::messages::{
        RegionHandshake, RegionHandshakeRegionInfo2Block, RegionHandshakeRegionInfo3Block,
        RegionHandshakeRegionInfo4Block, RegionHandshakeRegionInfoBlock,
    };
    let nil = Uuid::nil();
    RegionHandshake {
        region_info: RegionHandshakeRegionInfoBlock {
            region_flags: identity.region_flags,
            sim_access: identity.maturity.to_sim_access(),
            sim_name: with_nul(&sl_wire::region_name_to_wire(identity.sim_name.as_ref())),
            sim_owner: identity.sim_owner,
            is_estate_manager: identity.is_estate_manager,
            water_height: identity.water_height,
            billable_factor: identity.billable_factor,
            cache_id: nil,
            terrain_base0: nil,
            terrain_base1: nil,
            terrain_base2: nil,
            terrain_base3: nil,
            terrain_detail0: nil,
            terrain_detail1: nil,
            terrain_detail2: nil,
            terrain_detail3: nil,
            terrain_start_height00: 0.0,
            terrain_start_height01: 0.0,
            terrain_start_height10: 0.0,
            terrain_start_height11: 0.0,
            terrain_height_range00: 0.0,
            terrain_height_range01: 0.0,
            terrain_height_range10: 0.0,
            terrain_height_range11: 0.0,
        },
        region_info2: RegionHandshakeRegionInfo2Block {
            region_id: identity.region_id,
        },
        region_info3: RegionHandshakeRegionInfo3Block {
            cpu_class_id: identity.cpu_class_id,
            cpu_ratio: identity.cpu_ratio,
            colo_name: Vec::new(),
            product_sku: with_nul(&identity.product_sku),
            product_name: with_nul(&identity.product_name),
        },
        region_info4: vec![RegionHandshakeRegionInfo4Block {
            region_flags_extended: identity.region_flags_extended,
            region_protocols: identity.region_protocols,
        }],
    }
}

/// Builds [`AvatarName`]s from a `UUIDNameReply`'s variable name block.
pub(crate) fn avatar_names(reply: &UUIDNameReply) -> Vec<AvatarName> {
    reply
        .uuid_name_block
        .iter()
        .map(|block| AvatarName {
            id: AgentKey::from(block.id),
            first_name: trimmed_string(&block.first_name),
            last_name: trimmed_string(&block.last_name),
        })
        .collect()
}

/// Builds [`GroupName`]s from a `UUIDGroupNameReply`'s variable name block.
pub(crate) fn group_names(reply: &UUIDGroupNameReply) -> Vec<GroupName> {
    reply
        .uuid_name_block
        .iter()
        .map(|block| GroupName {
            id: GroupKey::from(block.id),
            name: trimmed_string(&block.group_name),
        })
        .collect()
}

/// Parses an `ExtEnvironment` GET reply (the
/// [`Command::RequestEnvironment`](crate::Command::RequestEnvironment) result)
/// into [`EnvironmentSettings`]. Returns `None` if the `environment` envelope is
/// absent (e.g. a failure reply).
pub(crate) fn environment_from_llsd(body: &Llsd) -> Option<EnvironmentSettings> {
    let env = body.get("environment")?;
    let alt = env.get("track_altitudes");
    let altitude = |index: usize| {
        alt.and_then(|a| a.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    Some(EnvironmentSettings {
        parcel_id: i32_member(env, "parcel_id"),
        region_id: uuid_member(env, "region_id"),
        day_length: i32_member(env, "day_length"),
        day_offset: i32_member(env, "day_offset"),
        flags: u32_member(env, "flags"),
        env_version: i32_member(env, "env_version"),
        track_altitudes: [altitude(0), altitude(1), altitude(2)],
        day_cycle: day_cycle_from_llsd(env.get("day_cycle")),
    })
}

/// Parses a day-cycle `OSDMap` into a [`DayCycle`]: its tracks (track 0 water, the
/// rest sky) and its named sky/water frames. An absent cycle yields an empty one.
fn day_cycle_from_llsd(value: Option<&Llsd>) -> DayCycle {
    let name = value
        .map(|cycle| string_member(cycle, "name"))
        .unwrap_or_default();
    let mut sky_frames = BTreeMap::new();
    let mut water_frames = BTreeMap::new();
    if let Some(frames) = value
        .and_then(|cycle| cycle.get("frames"))
        .and_then(Llsd::as_map)
    {
        for (frame_name, frame) in frames {
            if frame.get("type").and_then(Llsd::as_str) == Some("water") {
                drop(water_frames.insert(
                    frame_name.clone(),
                    water_settings_from_llsd(frame_name, frame),
                ));
            } else {
                drop(sky_frames.insert(
                    frame_name.clone(),
                    sky_settings_from_llsd(frame_name, frame),
                ));
            }
        }
    }
    let tracks = value
        .and_then(|cycle| cycle.get("tracks"))
        .and_then(Llsd::as_array)
        .map(|array| array.iter().map(track_from_llsd).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut iter = tracks.into_iter();
    let water_track = iter.next().unwrap_or_default();
    let sky_tracks = iter.collect();
    DayCycle {
        name,
        water_track,
        sky_tracks,
        sky_frames,
        water_frames,
    }
}

/// Parses one track (an array of `{key_keyframe, key_name}` maps) into its
/// [`DayCycleFrame`] keyframes.
fn track_from_llsd(track: &Llsd) -> Vec<DayCycleFrame> {
    track
        .as_array()
        .map(|frames| {
            frames
                .iter()
                .map(|frame| DayCycleFrame {
                    keyframe: f32_member(frame, "key_keyframe"),
                    name: string_member(frame, "key_name"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a sky frame `OSDMap` into [`SkySettings`]. The legacy haze colours and
/// scalars come from the frame's `legacy_haze` sub-map.
fn sky_settings_from_llsd(name: &str, sky: &Llsd) -> SkySettings {
    let haze = sky.get("legacy_haze");
    let haze_f32 = |key: &str| haze.map_or(0.0, |block| f32_member(block, key));
    let haze_color = |key: &str| color_from_llsd(haze.and_then(|block| block.get(key)));
    SkySettings {
        name: name.to_owned(),
        sun_rotation: rotation_from_llsd(sky.get("sun_rotation")),
        moon_rotation: rotation_from_llsd(sky.get("moon_rotation")),
        sunlight_color: color_alpha_from_llsd(sky.get("sunlight_color")),
        ambient: haze_color("ambient"),
        blue_horizon: haze_color("blue_horizon"),
        blue_density: haze_color("blue_density"),
        haze_horizon: haze_f32("haze_horizon"),
        haze_density: haze_f32("haze_density"),
        density_multiplier: haze_f32("density_multiplier"),
        distance_multiplier: haze_f32("distance_multiplier"),
        max_y: f32_member(sky, "max_y"),
        gamma: f32_member(sky, "gamma"),
        cloud_color: color_from_llsd(sky.get("cloud_color")),
        cloud_pos_density1: cloud_pos_density_from_llsd(sky.get("cloud_pos_density1")),
        cloud_pos_density2: cloud_pos_density_from_llsd(sky.get("cloud_pos_density2")),
        cloud_scale: f32_member(sky, "cloud_scale"),
        cloud_scroll_rate: vec2_from_llsd(sky.get("cloud_scroll_rate")),
        cloud_shadow: f32_member(sky, "cloud_shadow"),
        cloud_variance: f32_member(sky, "cloud_variance"),
        glow: glow_from_llsd(sky.get("glow")),
        star_brightness: f32_member(sky, "star_brightness"),
        sun_scale: f32_member(sky, "sun_scale"),
        moon_scale: f32_member(sky, "moon_scale"),
        moon_brightness: f32_member(sky, "moon_brightness"),
        sun_arc_radians: f32_member(sky, "sun_arc_radians"),
        droplet_radius: f32_member(sky, "droplet_radius"),
        ice_level: f32_member(sky, "ice_level"),
        moisture_level: f32_member(sky, "moisture_level"),
        sky_top_radius: f32_member(sky, "sky_top_radius"),
        sky_bottom_radius: f32_member(sky, "sky_bottom_radius"),
        planet_radius: f32_member(sky, "planet_radius"),
        sun_texture: optional_texture_member(sky, "sun_id"),
        moon_texture: optional_texture_member(sky, "moon_id"),
        cloud_texture: optional_texture_member(sky, "cloud_id"),
        bloom_texture: optional_texture_member(sky, "bloom_id"),
        halo_texture: optional_texture_member(sky, "halo_id"),
        rainbow_texture: optional_texture_member(sky, "rainbow_id"),
    }
}

/// Parses a water frame `OSDMap` into [`WaterSettings`].
fn water_settings_from_llsd(name: &str, water: &Llsd) -> WaterSettings {
    WaterSettings {
        name: name.to_owned(),
        blur_multiplier: f32_member(water, "blur_multiplier"),
        fresnel_offset: f32_member(water, "fresnel_offset"),
        fresnel_scale: f32_member(water, "fresnel_scale"),
        normal_scale: scale_from_llsd(water.get("normal_scale")),
        normal_map: optional_texture_member(water, "normal_map"),
        scale_above: f32_member(water, "scale_above"),
        scale_below: f32_member(water, "scale_below"),
        transparent_texture: optional_texture_member(water, "transparent_texture"),
        underwater_fog_mod: f32_member(water, "underwater_fog_mod"),
        water_fog_color: color_from_llsd(water.get("water_fog_color")),
        water_fog_density: f32_member(water, "water_fog_density"),
        wave1_direction: vec2_from_llsd(water.get("wave1_direction")),
        wave2_direction: vec2_from_llsd(water.get("wave2_direction")),
    }
}

/// Builds [`RegionLimits`] from a `RegionInfo` message's region-info blocks. The
/// 64-bit flags come from the optional `RegionInfo3` block, and the chat / combat
/// settings from the optional `RegionInfo5` / `CombatSettings` blocks (all absent
/// on OpenSim and older grids).
pub(crate) fn region_limits(
    message: &sl_wire::messages::RegionInfo,
) -> Result<RegionLimits, sl_wire::WireError> {
    let info = &message.region_info;
    let info2 = &message.region_info2;
    // Prefer the 32-bit agent cap; fall back to the legacy 8-bit field when the
    // grid leaves the wider one at zero.
    let max_agents = if info2.max_agents32 == 0 {
        u32::from(info.max_agents)
    } else {
        info2.max_agents32
    };
    let region_flags_extended = message.region_info3.first().map_or_else(
        || u64::from(info.region_flags),
        |i3| i3.region_flags_extended,
    );
    let chat_settings = message
        .region_info5
        .first()
        .map(|info5| RegionChatSettings {
            whisper_range: info5.chat_whisper_range,
            normal_range: info5.chat_normal_range,
            shout_range: info5.chat_shout_range,
            whisper_offset: info5.chat_whisper_offset,
            normal_offset: info5.chat_normal_offset,
            shout_offset: info5.chat_shout_offset,
            flags: info5.chat_flags,
        });
    let combat_settings = message
        .combat_settings
        .first()
        .map(|combat| RegionCombatSettings {
            flags: combat.combat_flags,
            on_death: combat.on_death,
            damage_throttle: combat.damage_throttle,
            regeneration_rate: combat.regeneration_rate,
            invulnerability_time: combat.invulnerabily_time,
            damage_limit: combat.damage_limit,
        });
    Ok(RegionLimits {
        sim_name: sl_wire::region_name_from_wire("SimName", &trimmed_string(&info.sim_name))?,
        max_agents,
        hard_max_agents: info2.hard_max_agents,
        hard_max_objects: info2.hard_max_objects,
        region_flags: info.region_flags,
        region_flags_extended,
        maturity: Maturity::from_sim_access(info.sim_access),
        estate_id: info.estate_id,
        parent_estate_id: info.parent_estate_id,
        water_height: info.water_height,
        billable_factor: info.billable_factor,
        object_bonus_factor: info.object_bonus_factor,
        terrain_raise_limit: info.terrain_raise_limit,
        terrain_lower_limit: info.terrain_lower_limit,
        price_per_meter: crate::types::linden_from_wire("PricePerMeter", info.price_per_meter)?,
        redirect_grid_x: info.redirect_grid_x,
        redirect_grid_y: info.redirect_grid_y,
        use_estate_sun: info.use_estate_sun,
        sun_hour: info.sun_hour,
        chat_settings,
        combat_settings,
    })
}

/// Builds a [`MoneyBalance`] from a `MoneyBalanceReply`. The optional
/// `TransactionInfo` block is all-zero for a plain balance poll; it is surfaced
/// only when it describes a real transaction (non-zero type).
pub(crate) fn money_balance(
    reply: &sl_wire::messages::MoneyBalanceReply,
) -> Result<MoneyBalance, sl_wire::WireError> {
    use crate::types::{land_area_from_wire, linden_from_wire};
    let data = &reply.money_data;
    let info = &reply.transaction_info;
    let transaction = if info.transaction_type == 0 {
        None
    } else {
        Some(MoneyTransaction {
            transaction_type: info.transaction_type,
            source: crate::types::owner_key_from_wire(info.source_id, info.is_source_group),
            dest: crate::types::owner_key_from_wire(info.dest_id, info.is_dest_group),
            amount: linden_from_wire("Amount", info.amount)?,
            item_description: trimmed_string(&info.item_description),
        })
    };
    Ok(MoneyBalance {
        agent_id: AgentKey::from(data.agent_id),
        transaction_id: data.transaction_id,
        success: data.transaction_success,
        balance: linden_from_wire("MoneyBalance", data.money_balance)?,
        square_meters_credit: land_area_from_wire("SquareMetersCredit", data.square_meters_credit)?,
        square_meters_committed: land_area_from_wire(
            "SquareMetersCommitted",
            data.square_meters_committed,
        )?,
        description: trimmed_string(&data.description),
        transaction,
    })
}

/// Builds an [`AvatarAppearance`] from an `AvatarAppearance` message: decodes the
/// per-avatar `TextureEntry` (the baked-texture ids) and collects the visual
/// params and optional appearance/hover/attachment blocks.
pub(crate) fn avatar_appearance(message: &sl_wire::messages::AvatarAppearance) -> AvatarAppearance {
    let texture_entry =
        appearance::decode_texture_entry(&message.object_data.texture_entry, avatar_texture::COUNT);
    let visual_params = message
        .visual_param
        .iter()
        .map(|block| block.param_value)
        .collect();
    let appearance_block = message.appearance_data.first();
    let attachments = message
        .attachment_block
        .iter()
        .map(|block| AvatarAttachment {
            id: ObjectKey::from(block.id),
            attachment_point: block.attachment_point,
        })
        .collect();
    AvatarAppearance {
        avatar_id: AgentKey::from(message.sender.id),
        is_trial: message.sender.is_trial,
        texture_entry,
        visual_params,
        appearance_version: appearance_block.map(|block| block.appearance_version),
        cof_version: appearance_block.map(|block| block.cof_version),
        appearance_flags: appearance_block.map(|block| block.flags),
        hover_height: message
            .appearance_hover
            .first()
            .map(|block| block.hover_height.clone()),
        attachments,
    }
}

/// Builds the [`PlayingAnimation`] list from an `AvatarAnimation` message. The
/// `AnimationSourceList` is positionally correlated with the `AnimationList`
/// (entry `i`'s source is `AnimationSourceList[i]`, when present), matching the
/// reference viewer's `process_avatar_animation`.
pub(crate) fn avatar_animations(
    message: &sl_wire::messages::AvatarAnimation,
) -> Vec<PlayingAnimation> {
    message
        .animation_list
        .iter()
        .enumerate()
        .map(|(index, block)| PlayingAnimation {
            anim_id: block.anim_id,
            sequence_id: block.anim_sequence_id,
            source_id: message
                .animation_source_list
                .get(index)
                .map(|source| ObjectKey::from(source.object_id)),
        })
        .collect()
}

/// Builds an [`Event::ServerAppearanceUpdate`] from the LLSD reply to an
/// `UpdateAvatarAppearance` POST (`{ success, error?, expected? }`).
pub(crate) fn server_appearance_update_from_llsd(body: &Llsd) -> Event {
    Event::ServerAppearanceUpdate {
        success: body.get("success").and_then(Llsd::as_bool).unwrap_or(false),
        error: body
            .get("error")
            .and_then(Llsd::as_str)
            .map(ToOwned::to_owned),
        expected_cof_version: body.get("expected").and_then(Llsd::as_i32),
    }
}

/// Builds [`EconomyData`] from an `EconomyData` message's info block.
///
/// The L$ price fields are decoded at the codec boundary
/// ([`linden_from_wire`](crate::types::linden_from_wire)); a negative price
/// (which a conforming simulator never sends) rejects the whole message with
/// [`WireError::ValueOutOfRange`].
pub(crate) fn economy_data(
    data: &sl_wire::messages::EconomyData,
) -> Result<EconomyData, sl_wire::WireError> {
    use crate::types::linden_from_wire;
    let info = &data.info;
    Ok(EconomyData {
        object_capacity: info.object_capacity,
        object_count: info.object_count,
        price_energy_unit: linden_from_wire("PriceEnergyUnit", info.price_energy_unit)?,
        price_object_claim: linden_from_wire("PriceObjectClaim", info.price_object_claim)?,
        price_public_object_decay: linden_from_wire(
            "PricePublicObjectDecay",
            info.price_public_object_decay,
        )?,
        price_public_object_delete: linden_from_wire(
            "PricePublicObjectDelete",
            info.price_public_object_delete,
        )?,
        price_parcel_claim: linden_from_wire("PriceParcelClaim", info.price_parcel_claim)?,
        price_parcel_claim_factor: info.price_parcel_claim_factor,
        price_upload: linden_from_wire("PriceUpload", info.price_upload)?,
        price_rent_light: linden_from_wire("PriceRentLight", info.price_rent_light)?,
        teleport_min_price: linden_from_wire("TeleportMinPrice", info.teleport_min_price)?,
        teleport_price_exponent: info.teleport_price_exponent,
        energy_efficiency: info.energy_efficiency,
        price_object_rent: info.price_object_rent,
        price_object_scale_factor: info.price_object_scale_factor,
        price_parcel_rent: linden_from_wire("PriceParcelRent", info.price_parcel_rent)?,
        price_group_create: linden_from_wire("PriceGroupCreate", info.price_group_create)?,
    })
}

/// Builds a [`ParcelInfo`] from a `ParcelProperties` message. The `ParcelData`
/// block carries the bulk of the fields; the three trailing single blocks add
/// the age-verification, access-override and environment-override settings. The
/// `SeeAVs`/`AnyAVSounds`/`GroupAVSounds` booleans are only sent over the CAPS
/// LLSD form, so they are `None` here (see `parcel_info_from_llsd`).
pub(crate) fn parcel_info(msg: &ParcelProperties) -> Result<ParcelInfo, sl_wire::WireError> {
    let data = &msg.parcel_data;
    Ok(ParcelInfo {
        sequence_id: data.sequence_id,
        request_result: ParcelRequestResult::from_i32(data.request_result),
        snap_selection: data.snap_selection,
        self_count: data.self_count,
        other_count: data.other_count,
        public_count: data.public_count,
        local_id: RegionLocalParcelId(data.local_id),
        owner: crate::types::owner_key_from_wire(data.owner_id, data.is_group_owned),
        group: crate::types::group_from_wire(data.group_id),
        auction_id: data.auction_id,
        claim_date: data.claim_date,
        claim_price: crate::types::linden_from_wire("ClaimPrice", data.claim_price)?,
        rent_price: crate::types::linden_from_wire("RentPrice", data.rent_price)?,
        aabb_min: RegionCoordinates::new(data.aabb_min.x, data.aabb_min.y, data.aabb_min.z),
        aabb_max: RegionCoordinates::new(data.aabb_max.x, data.aabb_max.y, data.aabb_max.z),
        area: crate::types::land_area_from_wire("Area", data.area)?,
        bitmap: data.bitmap.clone(),
        status: ParcelStatus::from_i32(i32::from(data.status)),
        category: ParcelCategory::from_u8(data.category),
        max_prims: data.max_prims,
        sim_wide_max_prims: data.sim_wide_max_prims,
        sim_wide_total_prims: data.sim_wide_total_prims,
        total_prims: data.total_prims,
        owner_prims: data.owner_prims,
        group_prims: data.group_prims,
        other_prims: data.other_prims,
        selected_prims: data.selected_prims,
        parcel_prim_bonus: data.parcel_prim_bonus,
        other_clean_time: data.other_clean_time,
        raw_parcel_flags: data.parcel_flags,
        sale_price: crate::types::linden_price_from_wire(
            sl_wire::ParcelFlags::from_bits(data.parcel_flags)
                .contains(sl_wire::ParcelFlags::FOR_SALE),
            "SalePrice",
            data.sale_price,
        )?,
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.desc),
        music_url: sl_wire::optional_url_from_wire("MusicURL", &trimmed_string(&data.music_url))?,
        media_url: sl_wire::optional_url_from_wire("MediaURL", &trimmed_string(&data.media_url))?,
        media_id: crate::types::optional_key_from_wire(data.media_id),
        media_auto_scale: data.media_auto_scale != 0,
        auth_buyer_id: crate::types::optional_key_from_wire(data.auth_buyer_id),
        snapshot_id: crate::types::optional_key_from_wire(data.snapshot_id),
        pass_price: crate::types::linden_from_wire("PassPrice", data.pass_price)?,
        pass_hours: data.pass_hours,
        user_location: RegionCoordinates::new(
            data.user_location.x,
            data.user_location.y,
            data.user_location.z,
        ),
        user_look_at: Direction::new(
            data.user_look_at.x,
            data.user_look_at.y,
            data.user_look_at.z,
        ),
        landing_type: LandingType::from_u8(data.landing_type),
        region_push_override: data.region_push_override,
        region_deny_anonymous: data.region_deny_anonymous,
        region_deny_identified: data.region_deny_identified,
        region_deny_transacted: data.region_deny_transacted,
        region_deny_age_unverified: msg.age_verification_block.region_deny_age_unverified,
        region_allow_access_override: msg.region_allow_access_block.region_allow_access_override,
        parcel_environment_version: msg.parcel_environment_block.parcel_environment_version,
        region_allow_environment_override: msg
            .parcel_environment_block
            .region_allow_environment_override,
        see_avs: None,
        any_av_sounds: None,
        group_av_sounds: None,
    })
}

/// Builds a [`ChatMessage`] from a `ChatFromSimulator` chat-data block. The
/// `FromName` and `Message` strings carry trailing NUL padding, which is removed.
pub(crate) fn chat_message(data: &ChatFromSimulatorChatDataBlock) -> ChatMessage {
    ChatMessage {
        from_name: trimmed_string(&data.from_name),
        source: ChatSource::from_wire(data.source_type, data.source_id),
        owner_id: crate::types::optional_uuid_from_wire(data.owner_id),
        chat_type: ChatType::from_u8(data.chat_type),
        audible: ChatAudible::from_u8(data.audible),
        position: RegionCoordinates::new(data.position.x, data.position.y, data.position.z),
        message: trimmed_string(&data.message),
    }
}

/// Computes the canonical 1:1 IM session id the viewer uses: the byte-wise XOR
/// of the two agent ids, except an IM to oneself (where the XOR would be nil)
/// uses the agent id directly.
pub(crate) fn compute_im_session_id(agent_id: AgentKey, other: AgentKey) -> Uuid {
    if agent_id == other {
        return agent_id.uuid();
    }
    let mut out = [0u8; 16];
    for (slot, (a, b)) in out.iter_mut().zip(
        agent_id
            .uuid()
            .as_bytes()
            .iter()
            .zip(other.uuid().as_bytes()),
    ) {
        *slot = a ^ b;
    }
    Uuid::from_bytes(out)
}

/// Builds an [`InstantMessage`] from an `ImprovedInstantMessage`'s agent-data and
/// message blocks. The `FromAgentName` and `Message` strings carry trailing NUL
/// padding, which is removed. Shared with the simulator-side [`SimSession`](crate::SimSession),
/// which decodes the same client message.
pub(crate) fn instant_message(
    agent_data: &ImprovedInstantMessageAgentDataBlock,
    block: &ImprovedInstantMessageMessageBlockBlock,
) -> InstantMessage {
    InstantMessage {
        from_agent_id: AgentKey::from(agent_data.agent_id),
        from_agent_name: trimmed_string(&block.from_agent_name),
        to_agent_id: AgentKey::from(block.to_agent_id),
        dialog: ImDialog::from_u8(block.dialog),
        from_group: block.from_group,
        region_id: crate::types::optional_uuid_from_wire(block.region_id),
        position: RegionCoordinates::new(block.position.x, block.position.y, block.position.z),
        offline: block.offline != 0,
        timestamp: crate::types::optional_u32_from_wire(block.timestamp),
        id: block.id,
        parent_estate_id: block.parent_estate_id,
        message: trimmed_string(&block.message),
        binary_bucket: block.binary_bucket.clone(),
    }
}

/// Builds [`AvatarProperties`] from an `AvatarPropertiesReply` properties block.
pub(crate) fn avatar_properties(
    avatar_id: Uuid,
    data: &AvatarPropertiesReplyPropertiesDataBlock,
) -> AvatarProperties {
    AvatarProperties {
        avatar_id: AgentKey::from(avatar_id),
        image_id: TextureKey::from(data.image_id),
        fl_image_id: TextureKey::from(data.fl_image_id),
        // A nil partner id means the avatar has no partner.
        partner_id: (!data.partner_id.is_nil()).then(|| AgentKey::from(data.partner_id)),
        about_text: trimmed_string(&data.about_text),
        fl_about_text: trimmed_string(&data.fl_about_text),
        born_on: trimmed_string(&data.born_on),
        profile_url: trimmed_string(&data.profile_url),
        charter_member: trimmed_string(&data.charter_member),
        flags: data.flags,
    }
}

/// Builds [`AvatarInterests`] from an `AvatarInterestsReply` properties block.
pub(crate) fn avatar_interests(
    avatar_id: Uuid,
    data: &AvatarInterestsReplyPropertiesDataBlock,
) -> AvatarInterests {
    AvatarInterests {
        avatar_id: AgentKey::from(avatar_id),
        want_to_mask: data.want_to_mask,
        want_to_text: trimmed_string(&data.want_to_text),
        skills_mask: data.skills_mask,
        skills_text: trimmed_string(&data.skills_text),
        languages_text: trimmed_string(&data.languages_text),
    }
}

/// Builds an [`AvatarGroupMembership`] from an `AvatarGroupsReply` group entry.
pub(crate) fn avatar_group(data: &AvatarGroupsReplyGroupDataBlock) -> AvatarGroupMembership {
    AvatarGroupMembership {
        group_id: GroupKey::from(data.group_id),
        group_name: trimmed_string(&data.group_name),
        group_title: trimmed_string(&data.group_title),
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: TextureKey::from(data.group_insignia_id),
    }
}

/// Builds [`PickInfo`] from a `PickInfoReply` data block (#29).
///
/// # Errors
///
/// Returns a [`WireError`](sl_wire::WireError) when the region name is non-empty
/// but invalid.
pub(crate) fn pick_info(data: &PickInfoReplyDataBlock) -> Result<PickInfo, sl_wire::WireError> {
    let [x, y, z] = data.pos_global;
    Ok(PickInfo {
        pick_id: PickKey::from(data.pick_id),
        creator_id: AgentKey::from(data.creator_id),
        top_pick: data.top_pick,
        parcel_id: ParcelKey::from(data.parcel_id),
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.desc),
        snapshot_id: crate::types::optional_key_from_wire(data.snapshot_id),
        user: trimmed_string(&data.user),
        original_name: trimmed_string(&data.original_name),
        sim_name: sl_wire::region_name_from_wire("SimName", &trimmed_string(&data.sim_name))?,
        pos_global: GlobalCoordinates::new(x, y, z),
        sort_order: data.sort_order,
        enabled: data.enabled,
    })
}

/// Builds [`ClassifiedInfo`] from a `ClassifiedInfoReply` data block (#29).
pub(crate) fn classified_info(
    data: &ClassifiedInfoReplyDataBlock,
) -> Result<ClassifiedInfo, sl_wire::WireError> {
    let [x, y, z] = data.pos_global;
    Ok(ClassifiedInfo {
        classified_id: ClassifiedKey::from(data.classified_id),
        creator_id: AgentKey::from(data.creator_id),
        creation_date: data.creation_date,
        expiration_date: data.expiration_date,
        category: ClassifiedCategory::from_u32(data.category),
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.desc),
        parcel_id: ParcelKey::from(data.parcel_id),
        parent_estate: data.parent_estate,
        snapshot_id: crate::types::optional_key_from_wire(data.snapshot_id),
        sim_name: sl_wire::region_name_from_wire("SimName", &trimmed_string(&data.sim_name))?,
        pos_global: GlobalCoordinates::new(x, y, z),
        parcel_name: trimmed_string(&data.parcel_name),
        classified_flags: data.classified_flags,
        price_for_listing: crate::types::linden_from_wire(
            "PriceForListing",
            data.price_for_listing,
        )?,
    })
}

/// Converts a login [`SkeletonFolder`] into an [`InventoryFolder`].
pub(crate) fn skeleton_folder(folder: &SkeletonFolder) -> InventoryFolder {
    InventoryFolder {
        folder_id: folder.folder_id,
        parent_id: (!folder.parent_id.uuid().is_nil()).then_some(folder.parent_id),
        name: folder.name.clone(),
        folder_type: folder.type_default,
        version: folder.version,
    }
}

/// Builds a [`Friend`] from a login `buddy-list` entry.
pub(crate) fn friend(entry: &sl_wire::BuddyListEntry) -> Friend {
    Friend {
        id: FriendKey::from(entry.buddy_id),
        rights_granted: FriendRights(entry.rights_granted),
        rights_received: FriendRights(entry.rights_has),
    }
}

/// Builds [`ActiveGroup`] from an `AgentDataUpdate` block.
pub(crate) fn active_group(data: &AgentDataUpdateAgentDataBlock) -> ActiveGroup {
    ActiveGroup {
        agent_id: AgentKey::from(data.agent_id),
        first_name: trimmed_string(&data.first_name),
        last_name: trimmed_string(&data.last_name),
        group_title: trimmed_string(&data.group_title),
        active_group_id: crate::types::optional_key_from_wire(data.active_group_id),
        group_powers: data.group_powers,
        group_name: trimmed_string(&data.group_name),
    }
}

/// Builds [`GroupMembership`] from an `AgentGroupDataUpdate` entry.
pub(crate) fn group_membership(
    data: &AgentGroupDataUpdateGroupDataBlock,
) -> Result<GroupMembership, sl_wire::WireError> {
    Ok(GroupMembership {
        group_id: GroupKey::from(data.group_id),
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: TextureKey::from(data.group_insignia_id),
        contribution: crate::types::land_area_from_wire("Contribution", data.contribution)?,
        group_name: trimmed_string(&data.group_name),
    })
}

/// Builds [`GroupMember`] from a `GroupMembersReply` entry.
pub(crate) fn group_member(
    data: &GroupMembersReplyMemberDataBlock,
) -> Result<GroupMember, sl_wire::WireError> {
    Ok(GroupMember {
        agent_id: AgentKey::from(data.agent_id),
        contribution: crate::types::land_area_from_wire("Contribution", data.contribution)?,
        online_status: trimmed_string(&data.online_status),
        agent_powers: data.agent_powers,
        title: trimmed_string(&data.title),
        is_owner: data.is_owner,
    })
}

/// Builds [`GroupRole`] from a `GroupRoleDataReply` entry.
pub(crate) fn group_role(data: &GroupRoleDataReplyRoleDataBlock) -> GroupRole {
    GroupRole {
        role_id: crate::types::optional_key_from_wire(data.role_id),
        name: trimmed_string(&data.name),
        title: trimmed_string(&data.title),
        description: trimmed_string(&data.description),
        powers: data.powers,
        members: data.members,
    }
}

/// Builds [`GroupTitle`] from a `GroupTitlesReply` entry.
pub(crate) fn group_title(data: &GroupTitlesReplyGroupDataBlock) -> GroupTitle {
    GroupTitle {
        title: trimmed_string(&data.title),
        role_id: crate::types::optional_key_from_wire(data.role_id),
        selected: data.selected,
    }
}

/// Builds [`GroupProfile`] from a `GroupProfileReply` block.
pub(crate) fn group_profile(
    data: &GroupProfileReplyGroupDataBlock,
) -> Result<GroupProfile, sl_wire::WireError> {
    Ok(GroupProfile {
        group_id: GroupKey::from(data.group_id),
        name: trimmed_string(&data.name),
        charter: trimmed_string(&data.charter),
        show_in_list: data.show_in_list,
        member_title: trimmed_string(&data.member_title),
        powers: data.powers_mask,
        insignia_id: crate::types::optional_key_from_wire(data.insignia_id),
        founder_id: AgentKey::from(data.founder_id),
        membership_fee: crate::types::linden_from_wire("MembershipFee", data.membership_fee)?,
        open_enrollment: data.open_enrollment,
        money: data.money,
        member_count: data.group_membership_count,
        role_count: data.group_roles_count,
        allow_publish: data.allow_publish,
        mature_publish: data.mature_publish,
        owner_role: GroupRoleKey::from(data.owner_role),
    })
}

/// Builds [`GroupNotice`] from a `GroupNoticesListReply` entry.
pub(crate) fn group_notice(data: &GroupNoticesListReplyDataBlock) -> GroupNotice {
    GroupNotice {
        notice_id: GroupNoticeKey::from(data.notice_id),
        timestamp: data.timestamp,
        from_name: trimmed_string(&data.from_name),
        subject: trimmed_string(&data.subject),
        has_attachment: data.has_attachment,
        asset_type: data.asset_type,
    }
}

/// Builds [`GroupAccountSummary`] from a `GroupAccountSummaryReply`.
pub(crate) fn group_account_summary(
    reply: &GroupAccountSummaryReply,
) -> Result<GroupAccountSummary, sl_wire::WireError> {
    use crate::types::{LindenBalance, linden_from_wire};
    let money = &reply.money_data;
    Ok(GroupAccountSummary {
        group_id: GroupKey::from(reply.agent_data.group_id),
        request_id: money.request_id,
        interval_days: money.interval_days,
        current_interval: money.current_interval,
        start_date: trimmed_string(&money.start_date),
        balance: LindenBalance::from_i32(money.balance),
        total_credits: linden_from_wire("TotalCredits", money.total_credits)?,
        total_debits: linden_from_wire("TotalDebits", money.total_debits)?,
        object_tax_current: linden_from_wire("ObjectTaxCurrent", money.object_tax_current)?,
        light_tax_current: linden_from_wire("LightTaxCurrent", money.light_tax_current)?,
        land_tax_current: linden_from_wire("LandTaxCurrent", money.land_tax_current)?,
        group_tax_current: linden_from_wire("GroupTaxCurrent", money.group_tax_current)?,
        parcel_dir_fee_current: linden_from_wire(
            "ParcelDirFeeCurrent",
            money.parcel_dir_fee_current,
        )?,
        object_tax_estimate: linden_from_wire("ObjectTaxEstimate", money.object_tax_estimate)?,
        light_tax_estimate: linden_from_wire("LightTaxEstimate", money.light_tax_estimate)?,
        land_tax_estimate: linden_from_wire("LandTaxEstimate", money.land_tax_estimate)?,
        group_tax_estimate: linden_from_wire("GroupTaxEstimate", money.group_tax_estimate)?,
        parcel_dir_fee_estimate: linden_from_wire(
            "ParcelDirFeeEstimate",
            money.parcel_dir_fee_estimate,
        )?,
        non_exempt_members: money.non_exempt_members,
        last_tax_date: trimmed_string(&money.last_tax_date),
        tax_date: trimmed_string(&money.tax_date),
    })
}

/// Builds [`GroupAccountDetails`] from a `GroupAccountDetailsReply`.
pub(crate) fn group_account_details(reply: &GroupAccountDetailsReply) -> GroupAccountDetails {
    let money = &reply.money_data;
    GroupAccountDetails {
        group_id: GroupKey::from(reply.agent_data.group_id),
        request_id: money.request_id,
        interval_days: money.interval_days,
        current_interval: money.current_interval,
        start_date: trimmed_string(&money.start_date),
        entries: reply
            .history_data
            .iter()
            .map(|entry| GroupAccountDetailsEntry {
                description: trimmed_string(&entry.description),
                amount: crate::types::LindenBalance::from_i32(entry.amount),
            })
            .collect(),
    }
}

/// Builds [`GroupAccountTransactions`] from a `GroupAccountTransactionsReply`.
pub(crate) fn group_account_transactions(
    reply: &GroupAccountTransactionsReply,
) -> GroupAccountTransactions {
    let money = &reply.money_data;
    GroupAccountTransactions {
        group_id: GroupKey::from(reply.agent_data.group_id),
        request_id: money.request_id,
        interval_days: money.interval_days,
        current_interval: money.current_interval,
        start_date: trimmed_string(&money.start_date),
        entries: reply
            .history_data
            .iter()
            .map(|entry| GroupAccountTransaction {
                time: trimmed_string(&entry.time),
                user: trimmed_string(&entry.user),
                transaction_type: entry.r#type,
                item: trimmed_string(&entry.item),
                amount: crate::types::LindenBalance::from_i32(entry.amount),
            })
            .collect(),
    }
}

/// Builds [`GroupActiveProposalItem`] from a `GroupActiveProposalItemReply`
/// proposal entry.
pub(crate) fn group_active_proposal_item(
    data: &GroupActiveProposalItemReplyProposalDataBlock,
) -> GroupActiveProposalItem {
    GroupActiveProposalItem {
        vote_id: ProposalVoteId::from(data.vote_id),
        vote_initiator: AgentKey::from(data.vote_initiator),
        terse_date_id: trimmed_string(&data.terse_date_id),
        start_date_time: trimmed_string(&data.start_date_time),
        end_date_time: trimmed_string(&data.end_date_time),
        already_voted: data.already_voted,
        vote_cast: trimmed_string(&data.vote_cast),
        majority: data.majority,
        quorum: data.quorum,
        proposal_text: trimmed_string(&data.proposal_text),
    }
}

/// Builds [`GroupVoteHistoryItem`] from a `GroupVoteHistoryItemReply` (its single
/// history item plus the per-candidate tallies).
pub(crate) fn group_vote_history_item(reply: &GroupVoteHistoryItemReply) -> GroupVoteHistoryItem {
    let item = &reply.history_item_data;
    GroupVoteHistoryItem {
        vote_id: ProposalVoteId::from(item.vote_id),
        terse_date_id: trimmed_string(&item.terse_date_id),
        start_date_time: trimmed_string(&item.start_date_time),
        end_date_time: trimmed_string(&item.end_date_time),
        vote_initiator: AgentKey::from(item.vote_initiator),
        vote_type: trimmed_string(&item.vote_type),
        vote_result: trimmed_string(&item.vote_result),
        majority: item.majority,
        quorum: item.quorum,
        proposal_text: trimmed_string(&item.proposal_text),
        votes: reply
            .vote_item
            .iter()
            .map(|vote| GroupVote {
                candidate_id: ProposalCandidateId::from(vote.candidate_id),
                vote_cast: trimmed_string(&vote.vote_cast),
                num_votes: vote.num_votes,
            })
            .collect(),
    }
}

/// Builds a [`ScriptDialog`] value from a `ScriptDialog` message.
pub(crate) fn script_dialog(message: &sl_wire::messages::ScriptDialog) -> ScriptDialog {
    let data = &message.data;
    ScriptDialog {
        object_id: ObjectKey::from(data.object_id),
        object_name: trimmed_string(&data.object_name),
        owner_first_name: trimmed_string(&data.first_name),
        owner_last_name: trimmed_string(&data.last_name),
        owner_id: crate::types::optional_uuid_from_wire(
            message
                .owner_data
                .first()
                .map_or_else(Uuid::nil, |owner| owner.owner_id),
        ),
        message: trimmed_string(&data.message),
        chat_channel: ChatChannel(data.chat_channel),
        image_id: TextureKey::from(data.image_id),
        buttons: message
            .buttons
            .iter()
            .map(|button| trimmed_string(&button.button_label))
            .collect(),
    }
}

/// Builds a [`ScriptPermissionRequest`] value from a `ScriptQuestion` message.
pub(crate) fn script_permission_request(
    message: &sl_wire::messages::ScriptQuestion,
) -> ScriptPermissionRequest {
    let data = &message.data;
    ScriptPermissionRequest {
        task_id: ObjectKey::from(data.task_id),
        item_id: InventoryKey::from(data.item_id),
        object_name: trimmed_string(&data.object_name),
        object_owner: trimmed_string(&data.object_owner),
        experience_id: {
            // A nil `experience_id` means the request is not under an experience.
            let raw = message.experience.experience_id;
            (!raw.is_nil()).then(|| ExperienceKey::from(raw))
        },
        permissions: ScriptPermissions(data.questions),
    }
}

/// Builds an [`InventoryFolder`] from an `InventoryDescendents` folder entry.
/// Such entries carry no per-folder version, so it is reported as `0`.
pub(crate) fn inventory_folder(data: &InventoryDescendentsFolderDataBlock) -> InventoryFolder {
    InventoryFolder {
        folder_id: InventoryFolderKey::from(data.folder_id),
        parent_id: crate::types::optional_key_from_wire(data.parent_id),
        name: trimmed_string(&data.name),
        folder_type: data.r#type,
        version: 0,
    }
}

/// The LL "CRC" of a UUID: its 16 bytes read as four little-endian `u32`s,
/// summed (wrapping). A faithful port of `LLUUID::getCRC32`.
pub(crate) fn uuid_crc(id: Uuid) -> u32 {
    id.as_bytes().chunks_exact(4).fold(0_u32, |acc, chunk| {
        let b0 = chunk.first().copied().unwrap_or(0);
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let b3 = chunk.get(3).copied().unwrap_or(0);
        let word =
            u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16) | (u32::from(b3) << 24);
        acc.wrapping_add(word)
    })
}

/// The LL inventory-item "CRC" (a checksum, not a true CRC) carried in
/// `UpdateInventoryItem`/`BulkUpdateInventory`, a faithful port of
/// `LLInventoryItem::getCRC32` (with `LLPermissions::getCRC32` and
/// `LLSaleInfo::getCRC32`). The simulator uses it to detect a no-op update; an
/// `i8` asset/inventory type is sign-extended to `u32` as in the C++ promotion.
pub(crate) fn inventory_item_crc(item: &InventoryItem) -> Result<u32, sl_wire::WireError> {
    let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
    let permissions_crc = uuid_crc(item.creator_id.uuid())
        .wrapping_add(uuid_crc(owner_id))
        .wrapping_add(uuid_crc(item.last_owner_id))
        .wrapping_add(uuid_crc(group_id))
        .wrapping_add(
            item.permissions
                .base
                .bits()
                .wrapping_add(item.permissions.owner.bits())
                .wrapping_add(item.permissions.everyone.bits())
                .wrapping_add(item.permissions.group.bits()),
        );
    let sale_info_crc = crate::types::linden_price_to_wire("SalePrice", item.sale_price.as_ref())?
        .cast_unsigned()
        .wrapping_add(u32::from(item.sale_type).wrapping_mul(0x0707_3096));
    Ok(uuid_crc(item.item_id.uuid())
        .wrapping_add(uuid_crc(item.folder_id.uuid()))
        .wrapping_add(permissions_crc)
        .wrapping_add(uuid_crc(item.asset_id))
        .wrapping_add(i32::from(item.item_type).cast_unsigned())
        .wrapping_add(i32::from(item.inv_type).cast_unsigned())
        .wrapping_add(item.flags)
        .wrapping_add(sale_info_crc)
        .wrapping_add(item.creation_date.cast_unsigned()))
    // The thumbnail UUID (nil here) contributes 0 and is omitted.
}

/// Builds an [`InventoryItem`] from an `InventoryDescendents` item entry.
pub(crate) fn inventory_item(
    data: &InventoryDescendentsItemDataBlock,
) -> Result<InventoryItem, sl_wire::WireError> {
    Ok(InventoryItem {
        item_id: InventoryKey::from(data.item_id),
        folder_id: InventoryFolderKey::from(data.folder_id),
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.description),
        asset_id: data.asset_id,
        item_type: data.r#type,
        inv_type: data.inv_type,
        flags: data.flags,
        sale_type: data.sale_type,
        sale_price: crate::types::linden_price_from_wire(
            data.sale_type != 0,
            "SalePrice",
            data.sale_price,
        )?,
        creation_date: data.creation_date,
        owner: crate::types::inventory_owner_from_wire(
            data.owner_id,
            data.group_id,
            data.group_owned,
        ),
        // The legacy UDP descendents reply carries no previous-owner id.
        last_owner_id: Uuid::nil(),
        creator_id: AgentKey::from(data.creator_id),
        group: crate::types::group_from_wire(data.group_id),
        permissions: Permissions5 {
            base: Permissions::from_bits(data.base_mask),
            owner: Permissions::from_bits(data.owner_mask),
            group: Permissions::from_bits(data.group_mask),
            everyone: Permissions::from_bits(data.everyone_mask),
            next_owner: Permissions::from_bits(data.next_owner_mask),
        },
    })
}

/// Builds an [`InventoryItem`] from an `UpdateCreateInventoryItem` entry (the
/// reply to a `CreateInventoryItem`, carrying the new asset id).
pub(crate) fn inventory_item_from_create(
    data: &UpdateCreateInventoryItemInventoryDataBlock,
) -> Result<InventoryItem, sl_wire::WireError> {
    Ok(InventoryItem {
        item_id: InventoryKey::from(data.item_id),
        folder_id: InventoryFolderKey::from(data.folder_id),
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.description),
        asset_id: data.asset_id,
        item_type: data.r#type,
        inv_type: data.inv_type,
        flags: data.flags,
        sale_type: data.sale_type,
        sale_price: crate::types::linden_price_from_wire(
            data.sale_type != 0,
            "SalePrice",
            data.sale_price,
        )?,
        creation_date: data.creation_date,
        owner: crate::types::inventory_owner_from_wire(
            data.owner_id,
            data.group_id,
            data.group_owned,
        ),
        last_owner_id: Uuid::nil(),
        creator_id: AgentKey::from(data.creator_id),
        group: crate::types::group_from_wire(data.group_id),
        permissions: Permissions5 {
            base: Permissions::from_bits(data.base_mask),
            owner: Permissions::from_bits(data.owner_mask),
            group: Permissions::from_bits(data.group_mask),
            everyone: Permissions::from_bits(data.everyone_mask),
            next_owner: Permissions::from_bits(data.next_owner_mask),
        },
    })
}

/// Builds an [`InventoryFolder`] from a `BulkUpdateInventory` folder entry.
pub(crate) fn bulk_update_folder(data: &BulkUpdateInventoryFolderDataBlock) -> InventoryFolder {
    InventoryFolder {
        folder_id: InventoryFolderKey::from(data.folder_id),
        parent_id: crate::types::optional_key_from_wire(data.parent_id),
        name: trimmed_string(&data.name),
        folder_type: data.r#type,
        version: 0,
    }
}

/// Builds an [`InventoryItem`] from a `BulkUpdateInventory` item entry.
pub(crate) fn bulk_update_item(
    data: &BulkUpdateInventoryItemDataBlock,
) -> Result<InventoryItem, sl_wire::WireError> {
    Ok(InventoryItem {
        item_id: InventoryKey::from(data.item_id),
        folder_id: InventoryFolderKey::from(data.folder_id),
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.description),
        asset_id: data.asset_id,
        item_type: data.r#type,
        inv_type: data.inv_type,
        flags: data.flags,
        sale_type: data.sale_type,
        sale_price: crate::types::linden_price_from_wire(
            data.sale_type != 0,
            "SalePrice",
            data.sale_price,
        )?,
        creation_date: data.creation_date,
        owner: crate::types::inventory_owner_from_wire(
            data.owner_id,
            data.group_id,
            data.group_owned,
        ),
        last_owner_id: Uuid::nil(),
        creator_id: AgentKey::from(data.creator_id),
        group: crate::types::group_from_wire(data.group_id),
        permissions: Permissions5 {
            base: Permissions::from_bits(data.base_mask),
            owner: Permissions::from_bits(data.owner_mask),
            group: Permissions::from_bits(data.group_mask),
            everyone: Permissions::from_bits(data.everyone_mask),
            next_owner: Permissions::from_bits(data.next_owner_mask),
        },
    })
}

/// Builds a [`NeighborInfo`] from an `EnableSimulator` simulator-info block.
pub(crate) fn neighbor_info(info: &EnableSimulatorSimulatorInfoBlock) -> NeighborInfo {
    // IPPORT is big-endian (network order) on the wire, but the generated field
    // decoder reads it as a little-endian U16, so swap the bytes back to host
    // order here. (IPADDR is raw octets in order and needs no swap.)
    let port = info.port.swap_bytes();
    let sim = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(info.ip)), port);
    let region_handle = RegionHandle(info.handle);
    NeighborInfo {
        region_handle,
        sim,
        grid_coordinates: grid_coordinates_from_handle(region_handle),
    }
}

/// Builds a [`MapRegionInfo`] from a `MapBlockReply` data block (with its
/// optional size block), or `Ok(None)` for a sentinel/empty entry.
///
/// # Errors
///
/// Returns a [`WireError`](sl_wire::WireError) when the entry carries a
/// non-empty but invalid region name (so the datagram is rejected as a hard
/// error rather than silently dropping the entry).
pub(crate) fn map_region_info(
    data: &MapBlockReplyDataBlock,
    size: Option<&MapBlockReplySizeBlock>,
) -> Result<Option<MapRegionInfo>, sl_wire::WireError> {
    // The map sends a sentinel block (0,0 / empty name) for "not found".
    if data.x == 0 && data.y == 0 {
        return Ok(None);
    }
    // An empty name is also a sentinel (→ `Ok(None)`, skipped); a non-empty but
    // invalid name rejects the datagram (a hard error, not a silent drop).
    let Some(name) = sl_wire::region_name_from_wire("Name", &trimmed_string(&data.name))? else {
        return Ok(None);
    };
    // The wire carries the grid index pair as `u16`; widen into the `u32`
    // `GridCoordinates` holds.
    let grid_coordinates = GridCoordinates::new(u32::from(data.x), u32::from(data.y));
    Ok(Some(MapRegionInfo {
        name: Some(name),
        grid_coordinates,
        region_handle: RegionHandle::from(grid_coordinates),
        maturity: Maturity::from_sim_access(data.access),
        region_flags: data.region_flags,
        size_x: size
            .map(|block| u32::from(block.size_x))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        size_y: size
            .map(|block| u32::from(block.size_y))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        agents: data.agents,
        water_height: data.water_height,
        map_image_id: TextureKey::from(data.map_image_id),
    }))
}

/// Builds a [`MapItem`] from a `MapItemReply` data block. Coordinates are global
/// metres; `extra`/`extra2` are type-specific (see [`MapItem`]).
pub(crate) fn map_item(data: &sl_wire::messages::MapItemReplyDataBlock) -> MapItem {
    MapItem {
        position: GlobalCoordinates::new(f64::from(data.x), f64::from(data.y), 0.0),
        id: crate::types::optional_uuid_from_wire(data.id),
        extra: data.extra,
        extra2: data.extra2,
        name: trimmed_string(&data.name),
    }
}

/// Narrows a `MapItem`'s global-metre `f64` coordinate back to the `u32` the
/// `MapItemReply` carries. A map-item coordinate is an integer metre value that
/// originated as the wire `u32`, so the round trip is exact.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a map-item global coordinate is an integer u32 metre value widened to f64"
)]
const fn map_global_to_u32(meters: f64) -> u32 {
    meters as u32
}

/// Encodes a [`MapRegionInfo`] into a `MapBlockReply` `Data` block — the
/// simulator-side inverse of [`map_region_info`]. The grid coordinates are the
/// `u16` the wire carries, and the name is NUL-terminated as a map server sends
/// it. The region size is *not* carried here; it travels in the parallel `Size`
/// block (see [`build_map_block_reply`]).
pub(crate) fn map_region_info_to_data_block(info: &MapRegionInfo) -> MapBlockReplyDataBlock {
    MapBlockReplyDataBlock {
        // the wire field is `u16`; a real region index always fits (saturate
        // defensively rather than panic on a malformed value)
        x: u16::try_from(info.grid_coordinates.x()).unwrap_or(u16::MAX),
        y: u16::try_from(info.grid_coordinates.y()).unwrap_or(u16::MAX),
        name: with_nul(&sl_wire::region_name_to_wire(info.name.as_ref())),
        access: info.maturity.to_sim_access(),
        region_flags: info.region_flags,
        water_height: info.water_height,
        agents: info.agents,
        map_image_id: info.map_image_id.uuid(),
    }
}

/// Builds a `MapBlockReply` reporting `regions`, the simulator-side inverse of
/// the client's `MapBlockRequest`/`MapNameRequest` (decoded into
/// [`Event::MapBlock`] entries). `agent_id` and `flags` fill the agent block (the
/// client ignores them); `flags` is the request's map-layer flag echoed back.
///
/// Variable-sized regions are reported by a parallel `Size` block: it is emitted
/// for every entry — mirroring OpenSim's `SendMapBlock` — whenever any region is
/// not the standard 256 m, and omitted entirely when every region is 256 m (the
/// size the client assumes for a missing block). The `data` array is capped at
/// the 255 entries the wire count byte allows; longer runs must be split across
/// several replies by the caller.
#[must_use]
pub fn build_map_block_reply(
    agent_id: AgentKey,
    flags: MapRequestFlags,
    regions: &[MapRegionInfo],
) -> MapBlockReply {
    let needs_size = regions
        .iter()
        .any(|region| region.size_x != 256 || region.size_y != 256);
    let size = if needs_size {
        regions
            .iter()
            .map(|region| MapBlockReplySizeBlock {
                size_x: u16::try_from(region.size_x).unwrap_or(u16::MAX),
                size_y: u16::try_from(region.size_y).unwrap_or(u16::MAX),
            })
            .collect()
    } else {
        Vec::new()
    };
    MapBlockReply {
        agent_data: MapBlockReplyAgentDataBlock {
            agent_id: agent_id.uuid(),
            flags: flags.0,
        },
        data: regions.iter().map(map_region_info_to_data_block).collect(),
        size,
    }
}

/// Encodes a [`MapItem`] into a `MapItemReply` `Data` block — the simulator-side
/// inverse of [`map_item`]. Coordinates stay global metres; the name is
/// NUL-terminated as a map server sends it.
pub(crate) fn map_item_to_data_block(item: &MapItem) -> MapItemReplyDataBlock {
    MapItemReplyDataBlock {
        x: map_global_to_u32(item.position.x()),
        y: map_global_to_u32(item.position.y()),
        id: crate::types::optional_uuid_to_wire(item.id),
        extra: item.extra,
        extra2: item.extra2,
        name: with_nul(&item.name),
    }
}

/// Builds a `MapItemReply` of the given [`MapItemType`] reporting `items`, the
/// simulator-side inverse of the client's `MapItemRequest` (decoded into an
/// [`Event::MapItems`]). `agent_id` and `flags` fill the agent block (the client
/// ignores them). The `data` array is capped at the 255 entries the wire count
/// byte allows; longer runs must be split across several replies by the caller.
#[must_use]
pub fn build_map_item_reply(
    agent_id: AgentKey,
    flags: MapRequestFlags,
    item_type: MapItemType,
    items: &[MapItem],
) -> MapItemReply {
    MapItemReply {
        agent_data: MapItemReplyAgentDataBlock {
            agent_id: agent_id.uuid(),
            flags: flags.0,
        },
        request_data: MapItemReplyRequestDataBlock {
            item_type: item_type.to_u32(),
        },
        data: items.iter().map(map_item_to_data_block).collect(),
    }
}

/// Builds a [`MapLayer`] from a `MapLayerReply` `LayerData` block. The bounds
/// are inclusive grid coordinates.
pub(crate) fn map_layer(data: &MapLayerReplyLayerDataBlock) -> MapLayer {
    MapLayer {
        // the wire bounds are inclusive corners: (left, bottom) is the
        // lower-left and (right, top) the upper-right; `GridRectangle::new`
        // normalises them
        rect: GridRectangle::new(
            GridCoordinates::new(data.left, data.bottom),
            GridCoordinates::new(data.right, data.top),
        ),
        image_id: TextureKey::from(data.image_id),
    }
}

/// Encodes a [`MapLayer`] into a `MapLayerReply` `LayerData` block — the
/// simulator-side inverse of [`map_layer`].
pub(crate) fn map_layer_to_data_block(layer: &MapLayer) -> MapLayerReplyLayerDataBlock {
    let lower_left = layer.rect.lower_left_corner();
    let upper_right = layer.rect.upper_right_corner();
    MapLayerReplyLayerDataBlock {
        left: lower_left.x(),
        right: upper_right.x(),
        top: upper_right.y(),
        bottom: lower_left.y(),
        image_id: layer.image_id.uuid(),
    }
}

/// Builds a `MapLayerReply` reporting `layers`, the simulator-side inverse of
/// the client's `MapLayerRequest` (decoded into [`Event::MapLayers`]). `agent_id`
/// and `flags` fill the agent block (the client ignores them). The `layer_data`
/// array is capped at the 255 entries the wire count byte allows.
#[must_use]
pub fn build_map_layer_reply(
    agent_id: AgentKey,
    flags: MapRequestFlags,
    layers: &[MapLayer],
) -> MapLayerReply {
    MapLayerReply {
        agent_data: MapLayerReplyAgentDataBlock {
            agent_id: agent_id.uuid(),
            flags: flags.0,
        },
        layer_data: layers.iter().map(map_layer_to_data_block).collect(),
    }
}

/// Builds [`EstateInfo`] from an `estateupdateinfo` `EstateOwnerMessage`'s param
/// list (10 string parameters: name, owner, id, flags, sun, parent, covenant id,
/// covenant timestamp, "1", abuse email).
pub(crate) fn estate_info_from_params(
    params: &[EstateOwnerMessageParamListBlock],
) -> Result<Option<EstateInfo>, sl_wire::WireError> {
    if params.len() < 8 {
        return Ok(None);
    }
    let text = |index: usize| {
        params
            .get(index)
            .map(|block| trimmed_string(&block.parameter))
            .unwrap_or_default()
    };
    // A present-but-malformed parameter is a hard error (rejecting the whole
    // `EstateInfo`) rather than a silently-coerced nil/`0`. The covenant id is
    // an optional notecard (`None` for "no covenant"); every other field is
    // required, so an unparsable value is `MalformedField`/`InvalidUuid`.
    Ok(Some(EstateInfo {
        estate_name: text(0),
        estate_owner: parse_uuid_field("estate_owner", &text(1))?,
        estate_id: parse_u32_field("estate_id", &text(2))?,
        estate_flags: parse_u32_field("estate_flags", &text(3))?,
        sun_position: parse_u32_field("sun_position", &text(4))?,
        parent_estate: parse_u32_field("parent_estate", &text(5))?,
        covenant_id: parse_optional_uuid_field("covenant_id", &text(6))?,
        covenant_timestamp: parse_u32_field("covenant_timestamp", &text(7))?,
        abuse_email: text(9),
    }))
}

/// Builds an [`Event::EstateAccessList`] from a `setaccess` `EstateOwnerMessage`.
/// `param[0]` is the estate id, `param[1]` the single-category code bit,
/// `param[2..=5]` per-category counts, and `param[6..]` the member ids — each a
/// raw 16-byte UUID (not a string).
pub(crate) fn estate_access_from_params(
    params: &[EstateOwnerMessageParamListBlock],
) -> Option<Event> {
    if params.len() < 6 {
        return None;
    }
    let text = |index: usize| {
        params
            .get(index)
            .map(|block| trimmed_string(&block.parameter))
            .unwrap_or_default()
    };
    let estate_id = text(0).parse().unwrap_or(0);
    let code: u32 = text(1).parse().unwrap_or(0);
    let kind = if code & 1 != 0 {
        EstateAccessKind::AllowedAgents
    } else if code & 2 != 0 {
        EstateAccessKind::AllowedGroups
    } else if code & 4 != 0 {
        EstateAccessKind::BannedAgents
    } else if code & 8 != 0 {
        EstateAccessKind::Managers
    } else {
        return None;
    };
    let members = params
        .iter()
        .skip(6)
        .filter_map(|block| {
            let bytes = block.parameter.get(..16)?;
            Uuid::from_slice(bytes).ok()
        })
        .collect();
    Some(Event::EstateAccessList {
        estate_id,
        kind,
        members,
    })
}

/// A decoded CAPS `TeleportFinish` event: the destination simulator address and
/// seed capability plus the destination region's maturity (`SimAccess`) and the
/// teleport flags (how/why the teleport happened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapsTeleportFinish {
    /// The destination simulator's UDP address.
    pub(crate) dest: SocketAddr,
    /// The destination region's seed capability URL.
    pub(crate) seed: url::Url,
    /// The destination region's maturity byte (`SimAccess`).
    pub(crate) sim_access: u8,
    /// The `TeleportFlags` bitfield.
    pub(crate) teleport_flags: u32,
}

/// Extracts the destination UDP address, seed capability, maturity and teleport
/// flags from a CAPS `TeleportFinish` event body: `{ "Info": [ { "SimIP":
/// <binary 4 bytes>, "SimPort": <integer>, "SeedCapability": <string>,
/// "SimAccess": <integer>, "TeleportFlags": <integer>, … } ] }`. The CAPS
/// `SimPort` is a plain host-order integer port (unlike the byte-swapped
/// generated-UDP field).
pub(crate) fn teleport_finish_from_llsd(body: &Llsd) -> Option<CapsTeleportFinish> {
    let info = body.get("Info").and_then(|info| info.index(0))?;
    let octets: [u8; 4] = info
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(info.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    // The seed capability is required for the teleport; an empty or unparsable
    // value drops the whole `TeleportFinish` (surfaced as a CAPS-decode failure).
    let seed = sl_wire::url_from_wire(
        "SeedCapability",
        info.get("SeedCapability")
            .and_then(Llsd::as_str)
            .unwrap_or(""),
    )
    .ok()?;
    // `SimAccess` is encoded as an LLSD integer; clamp into the maturity byte.
    let sim_access = info
        .get("SimAccess")
        .and_then(Llsd::as_i32)
        .and_then(|access| u8::try_from(access).ok())
        .unwrap_or(0);
    // `TeleportFlags` is a U32 bitfield carried as an LLSD integer (and some
    // grids encode the high U32 fields as binary), so read it tolerantly.
    let teleport_flags = info.get("TeleportFlags").map_or(0, llsd_u32);
    Some(CapsTeleportFinish {
        dest: SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
        sim_access,
        teleport_flags,
    })
}

/// Extracts a neighbour's region handle and simulator address from a CAPS
/// `EnableSimulator` event body: `{ "SimulatorInfo": [{ "Handle": <u64 binary>,
/// "IP": <4 bytes>, "Port": <integer> }] }`. Unlike the UDP message the port is
/// a plain integer (no byte swap).
pub(crate) fn enable_simulator_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr)> {
    let info = body.get("SimulatorInfo").and_then(|s| s.index(0))?;
    let handle = info.get("Handle").map(llsd_u64)?;
    let octets: [u8; 4] = info.get("IP").and_then(Llsd::as_binary)?.try_into().ok()?;
    let port = u16::try_from(info.get("Port").and_then(Llsd::as_i32)?).ok()?;
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
    ))
}

/// Extracts the destination region handle, simulator address and seed capability
/// from a CAPS `CrossedRegion` event body: the `RegionData` array carries
/// `RegionHandle` (u64), `SimIP` (4 bytes), `SimPort` (plain integer, no swap)
/// and `SeedCapability` (url).
pub(crate) fn crossed_region_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr, url::Url)> {
    let region = body.get("RegionData").and_then(|r| r.index(0))?;
    let handle = region.get("RegionHandle").map(llsd_u64)?;
    let octets: [u8; 4] = region
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(region.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    // The seed capability is required for the handover; an empty or unparsable
    // value drops the whole `CrossedRegion` (surfaced as a CAPS-decode failure).
    let seed = sl_wire::url_from_wire(
        "SeedCapability",
        region
            .get("SeedCapability")
            .and_then(Llsd::as_str)
            .unwrap_or(""),
    )
    .ok()?;
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
    ))
}

/// Extracts the child region's simulator address and seed capability from a CAPS
/// `EstablishAgentCommunication` event body: `{ "sim-ip-and-port": "ip:port",
/// "seed-capability": url }`.
pub(crate) fn establish_agent_communication_from_llsd(
    body: &Llsd,
) -> Option<(SocketAddr, url::Url)> {
    let sim = body.get("sim-ip-and-port").and_then(Llsd::as_str)?;
    let sim: SocketAddr = sim.parse().ok()?;
    // The seed capability is required; an empty or unparsable URL drops the
    // whole establish event (the `.ok()?` matches the `sim` parse above).
    let seed = sl_wire::url_from_wire(
        "seed-capability",
        body.get("seed-capability")
            .and_then(Llsd::as_str)
            .unwrap_or(""),
    )
    .ok()?;
    Some((sim, seed))
}

/// Decodes a CAPS `AgentStateUpdate` event body into the pathfinding
/// `can_modify_navmesh` flag. The body is `{ "can_modify_navmesh": bool }`;
/// the flag is required (a missing or non-boolean value drops the event).
pub(crate) fn agent_state_update_from_llsd(body: &Llsd) -> Option<bool> {
    body.get("can_modify_navmesh").and_then(Llsd::as_bool)
}

/// Decodes a CAPS `NavMeshStatusUpdate` event body into a [`NavMeshStatus`].
/// The body is `{ region_id: uuid, version: int, status: string }`; the
/// `region_id` is required (the version and status default as the reference
/// viewer does — version `0`, status [`NavMeshBuildStatus::Complete`]).
pub(crate) fn nav_mesh_status_from_llsd(body: &Llsd) -> Option<NavMeshStatus> {
    let region_id = body.get("region_id").and_then(Llsd::as_uuid)?;
    Some(NavMeshStatus {
        region_id,
        version: u32_member(body, "version"),
        status: NavMeshBuildStatus::from_wire(&string_member(body, "status")),
    })
}

/// Decodes a CAPS `AgentDropGroup` event body into the dropped group's key.
/// The body is `{ AgentData: [ { AgentID, GroupID } ] }` (the viewer's
/// `LLAgentDropGroupViewerNode`); the echoed `AgentID` is this agent itself and
/// is dropped, leaving the `GroupID` the simulator removed the agent from. A
/// missing/empty `AgentData` or `GroupID` drops the event.
pub(crate) fn agent_drop_group_from_llsd(body: &Llsd) -> Option<GroupKey> {
    // The sim sometimes double-wraps the payload in a nested `body`.
    let body = body.get("body").unwrap_or(body);
    let first = body.get("AgentData").and_then(Llsd::as_array)?.first()?;
    let group_id = first.get("GroupID").and_then(Llsd::as_uuid)?;
    Some(GroupKey::from(group_id))
}

/// Decodes a CAPS `DisplayNameUpdate` event body into a [`DisplayNameUpdate`].
/// The body is `{ agent_id: uuid, old_display_name: string, agent: <av record>
/// }`; the new-name record (`agent`) is required and carries the same People
/// API fields as a `GetDisplayNames` reply entry — but, unlike that entry, no
/// embedded `id` (it is `LLAvatarName::asLLSD`, which omits it), so the id is
/// taken from the body's top-level `agent_id`. `old_display_name` defaults to
/// empty when absent.
pub(crate) fn display_name_update_from_llsd(body: &Llsd) -> Option<DisplayNameUpdate> {
    let agent_id = body.get("agent_id").and_then(Llsd::as_uuid)?;
    let record = body.get("agent")?;
    let name = DisplayName {
        // The record may carry its own `id`; fall back to the body's `agent_id`.
        id: AgentKey::from(record.get("id").and_then(Llsd::as_uuid).unwrap_or(agent_id)),
        username: string_member(record, "username"),
        display_name: string_member(record, "display_name"),
        legacy_first_name: string_member(record, "legacy_first_name"),
        legacy_last_name: string_member(record, "legacy_last_name"),
        is_display_name_default: record
            .get("is_display_name_default")
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        display_name_expires: string_member(record, "display_name_expires"),
        display_name_next_update: string_member(record, "display_name_next_update"),
        missing: false,
    };
    Some(DisplayNameUpdate {
        old_display_name: string_member(body, "old_display_name"),
        name,
    })
}

/// Decodes a CAPS `SetDisplayNameReply` event body into a [`SetDisplayNameReply`].
/// The body is `{ status: int, reason: string, content: { display_name } | {
/// error_tag, error_description } }`: on success `content.display_name` holds the
/// new name, on failure `content.error_tag` identifies the error (mirroring the
/// viewer's `LLSetDisplayNameReply`). The fields all degrade gracefully, so this
/// is infallible.
pub(crate) fn set_display_name_reply_from_llsd(body: &Llsd) -> SetDisplayNameReply {
    let content = body.get("content");
    let content_string = |key: &str| -> Option<String> {
        content
            .and_then(|map| map.get(key))
            .and_then(Llsd::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    SetDisplayNameReply {
        status: i32_member(body, "status"),
        reason: string_member(body, "reason"),
        new_display_name: content_string("display_name"),
        error_tag: content_string("error_tag"),
    }
}

/// Decodes a CAPS `WindLightRefresh` event body into the interpolate flag. The
/// body is `{ Interpolate: int(0/1) }` (OpenSim `WindlightRefreshEvent`); the
/// sim asks the client to re-fetch the region's environment, interpolating the
/// transition when the flag is non-zero. A missing flag defaults to `false`
/// (apply immediately).
pub(crate) fn windlight_refresh_from_llsd(body: &Llsd) -> bool {
    let body = body.get("body").unwrap_or(body);
    i32_member(body, "Interpolate") != 0
}

/// Decodes a CAPS `SimConsoleResponse` event body into the console output text.
/// Unusually, the body is a bare LLSD string — the command's raw output — not a
/// map (OpenSim `RegionConsoleModule`, Firestorm
/// `llfloaterregiondebugconsole.cpp`).
pub(crate) fn sim_console_response_from_llsd(body: &Llsd) -> String {
    body.as_str().unwrap_or("").to_owned()
}

/// Decodes a CAPS `RequiredVoiceVersion` event body into a
/// [`RequiredVoiceVersion`]. The body is `{ major_version: int, region_name:
/// string, voice_server_type?: string }` (Firestorm
/// `LLViewerRequiredVoiceVersion`); `voice_server_type` is absent on older
/// grids. The fields all degrade gracefully, so this is infallible.
pub(crate) fn required_voice_version_from_llsd(body: &Llsd) -> RequiredVoiceVersion {
    let body = body.get("body").unwrap_or(body);
    RequiredVoiceVersion {
        major_version: i32_member(body, "major_version"),
        region_name: string_member(body, "region_name"),
        voice_server_type: body
            .get("voice_server_type")
            .and_then(Llsd::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    }
}

/// Decodes a CAPS `OpenRegionInfo` event body into an [`OpenRegionInfo`]. The
/// body is a map of optional OpenSim per-region settings; only the keys the sim
/// chooses to override are present (field names mirror Firestorm
/// `llpanelopenregionsettings.cpp`). Each key is read independently, so this is
/// infallible — an empty body simply yields all-`None`.
pub(crate) fn open_region_info_from_llsd(body: &Llsd) -> OpenRegionInfo {
    let body = body.get("body").unwrap_or(body);
    let flag = |key: &str| body.get(key).and_then(Llsd::as_i32).map(|value| value != 0);
    let real = |key: &str| body.get(key).and_then(Llsd::as_f32);
    let int = |key: &str| body.get(key).and_then(Llsd::as_i32);
    // A position bound is present only when all three components are; the
    // reference viewer reads `*PosX`/`*PosY`/`*PosZ` together.
    let position = |prefix: &str| -> Option<RegionCoordinates> {
        let x = body.get(&format!("{prefix}X")).and_then(Llsd::as_f32)?;
        let y = body.get(&format!("{prefix}Y")).and_then(Llsd::as_f32)?;
        let z = body.get(&format!("{prefix}Z")).and_then(Llsd::as_f32)?;
        Some(RegionCoordinates::new(x, y, z))
    };
    OpenRegionInfo {
        allow_minimap: flag("AllowMinimap"),
        allow_physical_prims: flag("AllowPhysicalPrims"),
        draw_distance: real("DrawDistance"),
        force_draw_distance: flag("ForceDrawDistance"),
        terrain_detail_scale: real("TerrainDetailScale"),
        max_drag_distance: real("MaxDragDistance"),
        min_hole_size: real("MinHoleSize"),
        max_hollow_size: real("MaxHollowSize"),
        max_inventory_items_transfer: int("MaxInventoryItemsTransfer"),
        max_link_count: int("MaxLinkCount"),
        max_link_count_phys: int("MaxLinkCountPhys"),
        max_position: position("MaxPos"),
        min_position: position("MinPos"),
        max_prim_scale: real("MaxPrimScale"),
        max_phys_prim_scale: real("MaxPhysPrimScale"),
        min_prim_scale: real("MinPrimScale"),
        offset_of_utc: int("OffsetOfUTC"),
        offset_of_utc_dst: flag("OffsetOfUTCDST"),
        render_water: flag("RenderWater"),
        say_distance: real("SayDistance"),
        shout_distance: real("ShoutDistance"),
        whisper_distance: real("WhisperDistance"),
        teen_mode: flag("ToggleTeenMode"),
        show_tags: int("ShowTags"),
        enforce_max_build: flag("EnforceMaxBuild"),
        max_groups: int("MaxGroups"),
        allow_parcel_windlight: flag("AllowParcelWindLight"),
    }
}

/// Builds a [`ParcelInfo`] from a CAPS `ParcelProperties` event body.
pub(crate) fn parcel_info_from_llsd(body: &Llsd) -> Option<ParcelInfo> {
    let data = body
        .get("ParcelData")
        .and_then(|parcel_data| parcel_data.index(0))?;
    // The three trailing single-blocks are each encoded as a one-element array
    // holding a map, mirroring the UDP message's `ParcelData` block (read above).
    let block = |key: &str| body.get(key).and_then(|array| array.index(0));
    let age_verification = block("AgeVerificationBlock");
    let region_allow_access = block("RegionAllowAccessBlock");
    let parcel_environment = block("ParcelEnvironmentBlock");
    let i32_field = |key: &str| data.get(key).and_then(Llsd::as_i32).unwrap_or(0);
    let bool_field = |key: &str| data.get(key).and_then(Llsd::as_bool).unwrap_or(false);
    let str_field = |key: &str| {
        data.get(key)
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let uuid_field = |key: &str| {
        data.get(key)
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil)
    };
    Some(ParcelInfo {
        sequence_id: i32_field("SequenceID"),
        request_result: ParcelRequestResult::from_i32(i32_field("RequestResult")),
        snap_selection: bool_field("SnapSelection"),
        self_count: i32_field("SelfCount"),
        other_count: i32_field("OtherCount"),
        public_count: i32_field("PublicCount"),
        local_id: RegionLocalParcelId(i32_field("LocalID")),
        owner: crate::types::owner_key_from_wire(uuid_field("OwnerID"), bool_field("IsGroupOwned")),
        group: crate::types::group_from_wire(uuid_field("GroupID")),
        // OpenSim encodes the `uint` AuctionID as a 4-byte binary LLSD element,
        // so read it tolerantly (binary / integer / string).
        auction_id: data.get("AuctionID").map_or(0, llsd_u32),
        // OpenSim sends ClaimDate as an LLSD `date`; the SL/UDP form is an integer
        // `time_t`. Accept either.
        claim_date: llsd_unix_time(data.get("ClaimDate")),
        claim_price: crate::types::linden_from_wire("ClaimPrice", i32_field("ClaimPrice")).ok()?,
        rent_price: crate::types::linden_from_wire("RentPrice", i32_field("RentPrice")).ok()?,
        aabb_min: region_coords_from_llsd(data.get("AABBMin")),
        aabb_max: region_coords_from_llsd(data.get("AABBMax")),
        area: crate::types::land_area_from_wire("Area", i32_field("Area")).ok()?,
        bitmap: data
            .get("Bitmap")
            .and_then(Llsd::as_binary)
            .map(<[u8]>::to_vec)
            .unwrap_or_default(),
        status: ParcelStatus::from_i32(i32_field("Status")),
        category: ParcelCategory::from_u8(u8::try_from(i32_field("Category")).unwrap_or(0)),
        max_prims: i32_field("MaxPrims"),
        sim_wide_max_prims: i32_field("SimWideMaxPrims"),
        sim_wide_total_prims: i32_field("SimWideTotalPrims"),
        total_prims: i32_field("TotalPrims"),
        owner_prims: i32_field("OwnerPrims"),
        group_prims: i32_field("GroupPrims"),
        other_prims: i32_field("OtherPrims"),
        selected_prims: i32_field("SelectedPrims"),
        parcel_prim_bonus: data
            .get("ParcelPrimBonus")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        other_clean_time: i32_field("OtherCleanTime"),
        // OpenSim encodes the `uint` ParcelFlags as a 4-byte binary LLSD element,
        // so read it tolerantly (binary / integer / string).
        raw_parcel_flags: data.get("ParcelFlags").map_or(0, llsd_u32),
        // `None` unless the parcel is for sale. A negative L$ price on a
        // for-sale parcel (which a conforming peer never sends) collapses the
        // whole CAPS decode to `None` — surfaced as a `CapsDecodeFailed`
        // diagnostic — rather than masquerading as `0`.
        sale_price: crate::types::linden_price_from_wire(
            sl_wire::ParcelFlags::from_bits(data.get("ParcelFlags").map_or(0, llsd_u32))
                .contains(sl_wire::ParcelFlags::FOR_SALE),
            "SalePrice",
            i32_field("SalePrice"),
        )
        .ok()?,
        name: str_field("Name"),
        description: str_field("Desc"),
        // A non-empty but unparsable URL drops the whole parcel record (the same
        // non-masking stance the `.ok()?` above applies to the sale price).
        music_url: sl_wire::optional_url_from_wire("MusicURL", &str_field("MusicURL")).ok()?,
        media_url: sl_wire::optional_url_from_wire("MediaURL", &str_field("MediaURL")).ok()?,
        media_id: crate::types::optional_key_from_wire(uuid_field("MediaID")),
        // OpenSim encodes MediaAutoScale as an LLSD boolean; `as_bool` also
        // tolerates the integer form.
        media_auto_scale: bool_field("MediaAutoScale"),
        auth_buyer_id: crate::types::optional_key_from_wire(uuid_field("AuthBuyerID")),
        snapshot_id: crate::types::optional_key_from_wire(uuid_field("SnapshotID")),
        pass_price: crate::types::linden_from_wire("PassPrice", i32_field("PassPrice")).ok()?,
        pass_hours: data.get("PassHours").and_then(Llsd::as_f32).unwrap_or(0.0),
        user_location: region_coords_from_llsd(data.get("UserLocation")),
        user_look_at: direction_from_llsd(data.get("UserLookAt")),
        landing_type: LandingType::from_u8(u8::try_from(i32_field("LandingType")).unwrap_or(0)),
        region_push_override: bool_field("RegionPushOverride"),
        region_deny_anonymous: bool_field("RegionDenyAnonymous"),
        region_deny_identified: bool_field("RegionDenyIdentified"),
        region_deny_transacted: bool_field("RegionDenyTransacted"),
        region_deny_age_unverified: age_verification
            .and_then(|map| map.get("RegionDenyAgeUnverified"))
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        region_allow_access_override: region_allow_access
            .and_then(|map| map.get("RegionAllowAccessOverride"))
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        parcel_environment_version: parcel_environment
            .and_then(|map| map.get("ParcelEnvironmentVersion"))
            .and_then(Llsd::as_i32)
            .unwrap_or(0),
        region_allow_environment_override: parcel_environment
            .and_then(|map| map.get("RegionAllowEnvironmentOverride"))
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        // Sent only over the CAPS LLSD form (the UDP message omits them).
        see_avs: data.get("SeeAVs").and_then(Llsd::as_bool),
        any_av_sounds: data.get("AnyAVSounds").and_then(Llsd::as_bool),
        group_av_sounds: data.get("GroupAVSounds").and_then(Llsd::as_bool),
    })
}

/// Reads a Unix `time_t` (seconds) from an LLSD value that is either an integer
/// (a `time_t` directly, as Second Life sends) or an ISO-8601 `date` element
/// (`YYYY-MM-DDThh:mm:ssZ`, as OpenSim's parcel encoder emits `ClaimDate`).
/// Returns `0` when absent or unparsable.
pub(crate) fn llsd_unix_time(value: Option<&Llsd>) -> i32 {
    let Some(value) = value else { return 0 };
    if let Some(seconds) = value.as_i32() {
        return seconds;
    }
    value
        .as_str()
        .and_then(parse_iso8601_to_unix)
        .and_then(|seconds| i32::try_from(seconds).ok())
        .unwrap_or(0)
}

/// Parses an ISO-8601 UTC timestamp (`YYYY-MM-DDThh:mm:ss[.fff][Z]`, or a bare
/// `YYYY-MM-DD`) into a Unix timestamp in seconds. Fractional seconds and the
/// trailing `Z` are ignored and UTC is assumed (the only form the LLSD wire
/// uses). Returns `None` on a malformed string.
pub(crate) fn parse_iso8601_to_unix(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date_part, time_part) = text.split_once('T').unwrap_or((text, ""));

    let mut date_fields = date_part.split('-');
    let year: i64 = date_fields.next()?.parse().ok()?;
    let month: i64 = date_fields.next()?.parse().ok()?;
    let day: i64 = date_fields.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Drop the trailing `Z` and any fractional-seconds suffix.
    let time_part = time_part.trim_end_matches('Z');
    let time_part = time_part.split('.').next().unwrap_or("");
    let mut time_fields = time_part.split(':');
    let parse_unit = |field: Option<&str>| -> Option<i64> {
        match field {
            None | Some("") => Some(0),
            Some(value) => value.parse().ok(),
        }
    };
    let hour = parse_unit(time_fields.next())?;
    let minute = parse_unit(time_fields.next())?;
    let second = parse_unit(time_fields.next())?;

    let days = days_from_civil(year, month, day)?;
    days.checked_mul(86_400)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)
}

/// Days since the Unix epoch (1970-01-01) for a proleptic-Gregorian date, via
/// Howard Hinnant's `days_from_civil` algorithm. Returns `None` only on
/// arithmetic overflow (impossible for any realistic year).
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let year = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let shifted = if year >= 0 {
        year
    } else {
        year.checked_sub(399)?
    };
    let era = shifted.checked_div(400)?;
    let year_of_era = year.checked_sub(era.checked_mul(400)?)?;
    let month_index = if month > 2 {
        month.checked_sub(3)?
    } else {
        month.checked_add(9)?
    };
    let day_of_year = month_index
        .checked_mul(153)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(day)?
        .checked_sub(1)?;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era.checked_div(4)?)?
        .checked_sub(year_of_era.checked_div(100)?)?
        .checked_add(day_of_year)?;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

/// Reads a three-component vector (`[x, y, z]` reals) from an LLSD array.
pub(crate) fn vec3_from_llsd(value: Option<&Llsd>) -> (f32, f32, f32) {
    let component = |index: usize| {
        value
            .and_then(|vector| vector.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    (component(0), component(1), component(2))
}

/// Reads region-local coordinates (`[x, y, z]` reals) from an LLSD array.
pub(crate) fn region_coords_from_llsd(value: Option<&Llsd>) -> RegionCoordinates {
    let (x, y, z) = vec3_from_llsd(value);
    RegionCoordinates::new(x, y, z)
}

/// Reads a facing direction (`[x, y, z]` reals) from an LLSD array.
pub(crate) fn direction_from_llsd(value: Option<&Llsd>) -> Direction {
    let (x, y, z) = vec3_from_llsd(value);
    Direction::new(x, y, z)
}

/// Reads a two-component vector (`[x, y]` reals) from an LLSD array as `[f32; 2]`.
fn vec2_from_llsd(value: Option<&Llsd>) -> [f32; 2] {
    let component = |index: usize| {
        value
            .and_then(|vector| vector.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    [component(0), component(1)]
}

/// Reads an RGBA [`ColorAlpha`] from an LLSD `[r, g, b, a]` real array.
fn color_alpha_from_llsd(value: Option<&Llsd>) -> ColorAlpha {
    let [red, green, blue, alpha] = vec4_from_llsd(value);
    ColorAlpha::new(red, green, blue, alpha)
}

/// Reads an RGB [`Color`] from an LLSD `[r, g, b]` real array.
fn color_from_llsd(value: Option<&Llsd>) -> Color {
    let (red, green, blue) = vec3_from_llsd(value);
    Color::new(red, green, blue)
}

/// Reads a [`Scale`] from an LLSD `[x, y, z]` real array.
fn scale_from_llsd(value: Option<&Llsd>) -> Scale {
    let (x, y, z) = vec3_from_llsd(value);
    Scale::new(x, y, z)
}

/// Reads a [`Glow`] from an LLSD `[size, reserved, focus]` real array.
fn glow_from_llsd(value: Option<&Llsd>) -> Glow {
    let (size, reserved, focus) = vec3_from_llsd(value);
    Glow::new(size, reserved, focus)
}

/// Reads a [`CloudPosDensity`] from an LLSD `[x, y, density]` real array.
fn cloud_pos_density_from_llsd(value: Option<&Llsd>) -> CloudPosDensity {
    let (position_x, position_y, density) = vec3_from_llsd(value);
    CloudPosDensity::new(position_x, position_y, density)
}

/// Reads a four-component vector (`[x, y, z, w]` reals) from an LLSD array as
/// `[f32; 4]`.
fn vec4_from_llsd(value: Option<&Llsd>) -> [f32; 4] {
    let component = |index: usize| {
        value
            .and_then(|vector| vector.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    [component(0), component(1), component(2), component(3)]
}

/// Reads a quaternion (`[x, y, z, w]` reals) from an LLSD array. The trailing
/// component is the quaternion's scalar part ([`Rotation::s`]).
fn rotation_from_llsd(value: Option<&Llsd>) -> Rotation {
    let [x, y, z, s] = vec4_from_llsd(value);
    Rotation { x, y, z, s }
}

/// Reads an `f32` from an LLSD map member, defaulting to `0.0`.
fn f32_member(map: &Llsd, key: &str) -> f32 {
    map.get(key).and_then(Llsd::as_f32).unwrap_or(0.0)
}

/// Reads a UUID from an LLSD map member, defaulting to nil.
pub(crate) fn uuid_member(map: &Llsd, key: &str) -> Uuid {
    map.get(key)
        .and_then(Llsd::as_uuid)
        .unwrap_or_else(Uuid::nil)
}

/// Reads an optional texture id from an LLSD map member: a nil or absent id
/// becomes `None` — the "use the viewer default" sentinel for the EEP
/// sky/water texture slots.
pub(crate) fn optional_texture_member(map: &Llsd, key: &str) -> Option<TextureKey> {
    let raw = uuid_member(map, key);
    (!raw.is_nil()).then(|| TextureKey::from(raw))
}

/// Encodes an optional texture id as an LLSD UUID: `None` emits the nil
/// "use the viewer default" sentinel (the inverse of
/// [`optional_texture_member`]).
pub(crate) fn optional_texture_to_llsd(texture: Option<TextureKey>) -> Llsd {
    Llsd::Uuid(texture.map_or_else(Uuid::nil, |key| key.uuid()))
}

/// Parses a text-form UUID strictly: a value that does not parse is a hard
/// [`WireError::InvalidUuid`] rather than the silently-coerced nil UUID. Used
/// where the wire carries an id as a string (e.g. an `EstateOwnerMessage`
/// parameter or an `EventInfoReply` creator).
pub(crate) fn parse_uuid_field(
    field: &'static str,
    value: &str,
) -> Result<Uuid, sl_wire::WireError> {
    Uuid::parse_str(value.trim()).map_err(|_err| sl_wire::WireError::InvalidUuid {
        field,
        value: value.to_owned(),
    })
}

/// Parses an optional text-form UUID: an empty/whitespace value is the "absent"
/// sentinel and yields `None`; a present value parses strictly (a malformed id
/// is a hard [`WireError::InvalidUuid`], never silently the nil UUID).
pub(crate) fn parse_optional_uuid_field(
    field: &'static str,
    value: &str,
) -> Result<Option<Uuid>, sl_wire::WireError> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    parse_uuid_field(field, value).map(Some)
}

/// Parses a text-form integer strictly: a value that does not parse is a hard
/// [`WireError::MalformedField`] rather than a silently-coerced `0`.
pub(crate) fn parse_u32_field(field: &'static str, value: &str) -> Result<u32, sl_wire::WireError> {
    value
        .trim()
        .parse()
        .map_err(|_err| sl_wire::WireError::MalformedField {
            field,
            value: value.to_owned(),
        })
}

/// Reads an `i32` from an LLSD map member, defaulting to `0`.
pub(crate) fn i32_member(map: &Llsd, key: &str) -> i32 {
    map.get(key).and_then(Llsd::as_i32).unwrap_or(0)
}

/// Reads a UUID from an LLSD value tolerantly: a `uuid` element, or a string
/// holding the canonical UUID text (Second Life's offline-IM and conference
/// records encode ids either way). Defaults to nil.
pub(crate) fn llsd_uuid(value: &Llsd) -> Uuid {
    value
        .as_uuid()
        .or_else(|| value.as_str().and_then(|s| Uuid::parse_str(s.trim()).ok()))
        .unwrap_or_else(Uuid::nil)
}

/// Reads a UUID from an LLSD map member tolerantly (see [`llsd_uuid`]).
pub(crate) fn uuid_member_lenient(map: &Llsd, key: &str) -> Uuid {
    map.get(key).map_or_else(Uuid::nil, llsd_uuid)
}

/// Reads a `u32` from an LLSD map member tolerantly (see [`llsd_u32`]).
pub(crate) fn u32_member(map: &Llsd, key: &str) -> u32 {
    map.get(key).map_or(0, llsd_u32)
}

/// Reads a string from an LLSD map member, defaulting to empty.
pub(crate) fn string_member(map: &Llsd, key: &str) -> String {
    map.get(key).and_then(Llsd::as_str).unwrap_or("").to_owned()
}

/// Decodes a `u64` from an LLSD value as the viewer's `ll_U64_from_sd` does:
/// from an 8-byte big-endian binary, a hex/decimal string, or an integer.
/// Reads a `u32` from an LLSD value that may be a 4-byte big-endian binary
/// element (how OpenSim encodes `uint` fields such as `ParcelFlags`), an
/// integer, or a decimal/hex string.
pub(crate) fn llsd_u32(value: &Llsd) -> u32 {
    match value {
        Llsd::Binary(bytes) if bytes.len() >= 4 => bytes
            .iter()
            .take(4)
            .fold(0u32, |acc, &byte| (acc << 8) | u32::from(byte)),
        Llsd::String(s) => {
            let trimmed = s.trim().trim_start_matches("0x");
            u32::from_str_radix(trimmed, 16)
                .ok()
                .or_else(|| s.trim().parse().ok())
                .unwrap_or(0)
        }
        Llsd::Integer(i) => u32::try_from(*i).unwrap_or(0),
        _ => 0,
    }
}

/// Decodes the `ReadOfflineMsgs` capability reply (#28) into [`InstantMessage`]s.
/// The body is either an array of per-message maps or a map with a `messages`
/// array (both forms the viewer accepts); each record mirrors an
/// `ImprovedInstantMessage` and is marked [`InstantMessage::offline`].
pub(crate) fn offline_messages_from_llsd(body: &Llsd) -> Vec<InstantMessage> {
    let records = body
        .as_array()
        .or_else(|| body.get("messages").and_then(Llsd::as_array))
        .unwrap_or_default();
    records
        .iter()
        .filter_map(offline_message_from_record)
        .collect()
}

/// Decodes one offline-IM record map into an [`InstantMessage`]; returns `None`
/// for a non-map element.
pub(crate) fn offline_message_from_record(record: &Llsd) -> Option<InstantMessage> {
    if !matches!(record, Llsd::Map(_)) {
        return None;
    }
    let dialog = ImDialog::from_u8(u8::try_from(i32_member(record, "dialog")).unwrap_or(0));
    // The session/transaction id falls back to the asset id for task messages,
    // matching the viewer's offline-message processing.
    let id = record
        .get("transaction-id")
        .or_else(|| record.get("transaction_id"))
        .or_else(|| record.get("asset_id"))
        .map_or_else(Uuid::nil, llsd_uuid);
    let binary_bucket = record
        .get("binary_bucket")
        .and_then(Llsd::as_binary)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    Some(InstantMessage {
        from_agent_id: AgentKey::from(uuid_member_lenient(record, "from_agent_id")),
        from_agent_name: string_member(record, "from_agent_name"),
        to_agent_id: AgentKey::from(uuid_member_lenient(record, "to_agent_id")),
        dialog,
        from_group: record
            .get("from_group")
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        region_id: crate::types::optional_uuid_from_wire(uuid_member_lenient(record, "region_id")),
        position: offline_message_position(record),
        offline: true,
        timestamp: crate::types::optional_u32_from_wire(u32_member(record, "timestamp")),
        id,
        parent_estate_id: u32_member(record, "parent_estate_id"),
        message: string_member(record, "message"),
        binary_bucket,
    })
}

/// Reads an offline-IM record's region-local position, from either a `position`
/// `[x, y, z]` array or the `local_x`/`local_y`/`local_z` members.
pub(crate) fn offline_message_position(record: &Llsd) -> RegionCoordinates {
    if let Some(array) = record.get("position").and_then(Llsd::as_array) {
        return RegionCoordinates::new(
            array.first().and_then(Llsd::as_f32).unwrap_or(0.0),
            array.get(1).and_then(Llsd::as_f32).unwrap_or(0.0),
            array.get(2).and_then(Llsd::as_f32).unwrap_or(0.0),
        );
    }
    RegionCoordinates::new(
        record.get("local_x").and_then(Llsd::as_f32).unwrap_or(0.0),
        record.get("local_y").and_then(Llsd::as_f32).unwrap_or(0.0),
        record.get("local_z").and_then(Llsd::as_f32).unwrap_or(0.0),
    )
}

/// Decodes a `ChatterBoxInvitation` CAPS event body (#28, #45) into an
/// [`Event::ConferenceInvited`], reading the nested
/// `instantmessage.message_params`. Returns `None` if the structure is absent.
///
/// Beyond the session id, inviter and message, this surfaces the session
/// classification and labelling fields the simulator carries but the viewer's
/// `LLViewerChatterBoxInvitation` reads: the `type` dialog byte (group vs.
/// ad-hoc conference vs. P2P), `from_group`, the region/position/estate/
/// timestamp source fields, and the `binary_bucket` (nested under
/// `message_params.data`, as both OpenSim and the reference viewer encode it).
pub(crate) fn chatterbox_invitation_from_llsd(body: &Llsd) -> Option<Event> {
    let params = body.get("instantmessage")?.get("message_params")?;
    // OpenSim/SL nest the bucket under `data`; tolerate a flat `binary_bucket`.
    let binary_bucket = params
        .get("data")
        .and_then(|data| data.get("binary_bucket"))
        .or_else(|| params.get("binary_bucket"))
        .and_then(Llsd::as_binary)
        .map_or_else(Vec::new, <[u8]>::to_vec);
    Some(Event::ConferenceInvited {
        session_id: uuid_member_lenient(params, "id"),
        from_agent_id: AgentKey::from(uuid_member_lenient(params, "from_id")),
        from_name: string_member(params, "from_name"),
        dialog: ImDialog::from_u8(u8::try_from(i32_member(params, "type")).unwrap_or(0)),
        from_group: params
            .get("from_group")
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        session_name: string_member(body, "session_name"),
        message: string_member(params, "message"),
        region_id: uuid_member_lenient(params, "region_id"),
        position: llsd_position(params),
        parent_estate_id: u32_member(params, "parent_estate_id"),
        timestamp: crate::types::optional_u32_from_wire(u32_member(params, "timestamp")),
        binary_bucket,
    })
}

/// Reads a region-local position from an LLSD map's `position` member, encoded
/// as a `[x, y, z]` real array (how the simulator encodes an LLSD `Vector3`).
/// Defaults each missing component to `0.0`.
pub(crate) fn llsd_position(map: &Llsd) -> RegionCoordinates {
    map.get("position").and_then(Llsd::as_array).map_or_else(
        || RegionCoordinates::new(0.0, 0.0, 0.0),
        |array| {
            RegionCoordinates::new(
                array.first().and_then(Llsd::as_f32).unwrap_or(0.0),
                array.get(1).and_then(Llsd::as_f32).unwrap_or(0.0),
                array.get(2).and_then(Llsd::as_f32).unwrap_or(0.0),
            )
        },
    )
}

/// Reads a `u64` from an LLSD value that may be an 8-byte big-endian binary
/// element (how OpenSim encodes `U64` region handles), an integer, or a
/// decimal/hex string.
pub(crate) fn llsd_u64(value: &Llsd) -> u64 {
    match value {
        Llsd::Binary(bytes) if bytes.len() >= 8 => bytes
            .iter()
            .take(8)
            .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte)),
        Llsd::String(s) => {
            let trimmed = s.trim().trim_start_matches("0x");
            u64::from_str_radix(trimmed, 16)
                .ok()
                .or_else(|| s.trim().parse().ok())
                .unwrap_or(0)
        }
        Llsd::Integer(i) => u64::try_from(*i).unwrap_or(0),
        _ => 0,
    }
}

/// Decodes the CAPS event-queue `AgentGroupDataUpdate` event (the modern
/// delivery of the agent's group memberships) into [`Event::GroupMemberships`].
/// The LLSD mirrors the UDP message: a `GroupData` array of per-group maps.
pub(crate) fn group_memberships_from_caps_llsd(body: &Llsd) -> Option<Event> {
    // The sim sometimes double-wraps the payload in a nested `body`.
    let body = body.get("body").unwrap_or(body);
    let groups = body.get("GroupData").and_then(Llsd::as_array)?;
    let memberships = groups
        .iter()
        .filter_map(|group| {
            let group_id = group.get("GroupID").and_then(Llsd::as_uuid)?;
            Some(GroupMembership {
                group_id: GroupKey::from(group_id),
                group_powers: group.get("GroupPowers").map_or(0, llsd_u64),
                accept_notices: group
                    .get("AcceptNotices")
                    .and_then(Llsd::as_bool)
                    .unwrap_or(false),
                group_insignia_id: TextureKey::from(
                    group
                        .get("GroupInsigniaID")
                        .and_then(Llsd::as_uuid)
                        .unwrap_or_else(Uuid::nil),
                ),
                contribution: crate::types::land_area_from_wire(
                    "Contribution",
                    group
                        .get("Contribution")
                        .and_then(Llsd::as_i32)
                        .unwrap_or(0),
                )
                .ok()?,
                group_name: group
                    .get("GroupName")
                    .and_then(Llsd::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect();
    Some(Event::GroupMemberships(memberships))
}

/// Decodes a `GroupMemberData` capability response into [`Event::GroupMembers`].
/// The LLSD is `{ group_id, members: { <id>: {...} }, titles: [...],
/// defaults: { default_powers } }`; per-member fields fall back to the defaults.
pub(crate) fn group_members_from_caps_llsd(body: &Llsd) -> Option<Event> {
    let group_id = body.get("group_id").and_then(Llsd::as_uuid)?;
    let Llsd::Map(members) = body.get("members")? else {
        return None;
    };
    let titles = body.get("titles").and_then(Llsd::as_array);
    let default_title = titles
        .and_then(|t| t.first())
        .and_then(Llsd::as_str)
        .unwrap_or_default();
    let default_powers = body
        .get("defaults")
        .and_then(|d| d.get("default_powers"))
        .map_or(0, llsd_u64);

    let mut roster: Vec<GroupMember> = members
        .iter()
        .filter_map(|(member_id, info)| {
            let agent_id = AgentKey::from(Uuid::parse_str(member_id).ok()?);
            let title = info
                .get("title")
                .and_then(Llsd::as_i32)
                .and_then(|index| titles?.get(usize::try_from(index).ok()?))
                .and_then(Llsd::as_str)
                .unwrap_or(default_title)
                .to_owned();
            Some(GroupMember {
                agent_id,
                contribution: crate::types::land_area_from_wire(
                    "donated_square_meters",
                    info.get("donated_square_meters")
                        .and_then(Llsd::as_i32)
                        .unwrap_or(0),
                )
                .ok()?,
                online_status: info
                    .get("last_login")
                    .and_then(Llsd::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                agent_powers: info.get("powers").map_or(default_powers, llsd_u64),
                title,
                is_owner: info.get("owner").is_some(),
            })
        })
        .collect();
    // The members map is unordered; sort by id for deterministic output.
    roster.sort_by_key(|member| member.agent_id.uuid());
    let member_count = i32::try_from(roster.len()).unwrap_or(i32::MAX);
    Some(Event::GroupMembers {
        group_id: GroupKey::from(group_id),
        request_id: Uuid::nil(),
        member_count,
        members: roster,
    })
}

/// Parses a `FetchInventoryDescendents2` CAPS response body into one
/// [`Event::InventoryDescendents`] per returned folder. The HTTP response shape
/// differs from the UDP `InventoryDescendents`, but yields the same value types.
pub(crate) fn inventory_descendents_from_llsd(body: &Llsd) -> Vec<Event> {
    let Some(folders) = body.get("folders").and_then(Llsd::as_array) else {
        return Vec::new();
    };
    folders
        .iter()
        .map(|folder| {
            let categories = folder
                .get("categories")
                .and_then(Llsd::as_array)
                .unwrap_or(&[]);
            let items = folder.get("items").and_then(Llsd::as_array).unwrap_or(&[]);
            Event::InventoryDescendents {
                folder_id: InventoryFolderKey::from(uuid_member(folder, "folder_id")),
                version: i32_member(folder, "version"),
                descendents: i32_member(folder, "descendents"),
                folders: categories.iter().map(inventory_folder_from_llsd).collect(),
                items: items.iter().filter_map(inventory_item_from_llsd).collect(),
            }
        })
        .collect()
}

/// Parses a `BulkUpdateInventory` CAPS event-queue body (`AgentData` /
/// `FolderData` / `ItemData` arrays of `CamelCase`-keyed maps, mirroring the UDP
/// message blocks) into its transaction id, folders, and items. Returns `None`
/// if the body is not a `BulkUpdateInventory` map. Nil-id placeholder folders
/// (which OpenSim emits) are skipped.
pub(crate) fn bulk_update_inventory_from_llsd(
    body: &Llsd,
) -> Option<(Uuid, Vec<InventoryFolder>, Vec<InventoryItem>)> {
    let transaction_id = body
        .get("AgentData")
        .and_then(Llsd::as_array)
        .and_then(<[Llsd]>::first)
        .map_or_else(Uuid::nil, |agent| uuid_member(agent, "TransactionID"));
    let folders = body
        .get("FolderData")
        .and_then(Llsd::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|folder| InventoryFolder {
            folder_id: InventoryFolderKey::from(uuid_member(folder, "FolderID")),
            parent_id: crate::types::optional_key_from_wire(uuid_member(folder, "ParentID")),
            name: string_member(folder, "Name"),
            folder_type: i8::try_from(i32_member(folder, "Type")).unwrap_or(-1),
            version: 0,
        })
        .filter(|folder| !folder.folder_id.uuid().is_nil())
        .collect();
    let items = body
        .get("ItemData")
        .and_then(Llsd::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(bulk_update_item_from_llsd)
        .filter(|item| !item.item_id.uuid().is_nil())
        .collect();
    Some((transaction_id, folders, items))
}

/// Builds an [`InventoryItem`] from a `BulkUpdateInventory` CAPS `ItemData`
/// entry (`CamelCase` keys, flat — permissions are not nested as in AIS).
pub(crate) fn bulk_update_item_from_llsd(item: &Llsd) -> Option<InventoryItem> {
    Some(InventoryItem {
        item_id: InventoryKey::from(uuid_member(item, "ItemID")),
        folder_id: InventoryFolderKey::from(uuid_member(item, "FolderID")),
        name: string_member(item, "Name"),
        description: string_member(item, "Description"),
        asset_id: uuid_member(item, "AssetID"),
        item_type: i8::try_from(i32_member(item, "Type")).unwrap_or(-1),
        inv_type: i8::try_from(i32_member(item, "InvType")).unwrap_or(-1),
        flags: i32_member(item, "Flags").cast_unsigned(),
        sale_type: u8::try_from(i32_member(item, "SaleType")).unwrap_or(0),
        sale_price: crate::types::linden_price_from_wire(
            i32_member(item, "SaleType") != 0,
            "SalePrice",
            i32_member(item, "SalePrice"),
        )
        .ok()?,
        creation_date: i32_member(item, "CreationDate"),
        owner: crate::types::inventory_owner_from_wire(
            uuid_member(item, "OwnerID"),
            uuid_member(item, "GroupID"),
            item.get("GroupOwned")
                .and_then(Llsd::as_bool)
                .unwrap_or(false),
        ),
        last_owner_id: Uuid::nil(),
        creator_id: AgentKey::from(uuid_member(item, "CreatorID")),
        group: crate::types::group_from_wire(uuid_member(item, "GroupID")),
        permissions: Permissions5 {
            base: Permissions::from_bits(i32_member(item, "BaseMask").cast_unsigned()),
            owner: Permissions::from_bits(i32_member(item, "OwnerMask").cast_unsigned()),
            group: Permissions::from_bits(i32_member(item, "GroupMask").cast_unsigned()),
            everyone: Permissions::from_bits(i32_member(item, "EveryoneMask").cast_unsigned()),
            next_owner: Permissions::from_bits(i32_member(item, "NextOwnerMask").cast_unsigned()),
        },
    })
}

/// Parses an AIS3 (`InventoryAPIv3`) response into the folders and items it
/// carries. AIS embeds the affected objects under `_embedded` as uuid-keyed maps
/// (`categories`, `items`, `links`), and a single-object fetch returns the object
/// at the top level. Both are gathered, reusing the AIS-shaped folder/item
/// decoders ([`inventory_folder_from_llsd`] / [`inventory_item_from_llsd`]).
pub(crate) fn ais_inventory_update_from_llsd(
    body: &Llsd,
) -> (Vec<InventoryFolder>, Vec<InventoryItem>) {
    let mut folders = Vec::new();
    let mut items = Vec::new();
    // Top-level single object (e.g. a GET /item/<id> or GET /category/<id>).
    if body.get("item_id").is_some() {
        items.extend(inventory_item_from_llsd(body));
    }
    if body.get("category_id").is_some() {
        folders.push(inventory_folder_from_llsd(body));
    }
    // Embedded objects (the affected set of a create/update/move).
    if let Some(embedded) = body.get("_embedded") {
        if let Some(categories) = embedded.get("categories").and_then(Llsd::as_map) {
            folders.extend(categories.values().map(inventory_folder_from_llsd));
        }
        if let Some(embedded_items) = embedded.get("items").and_then(Llsd::as_map) {
            items.extend(embedded_items.values().filter_map(inventory_item_from_llsd));
        }
        if let Some(links) = embedded.get("links").and_then(Llsd::as_map) {
            items.extend(links.values().filter_map(inventory_item_from_llsd));
        }
    }
    folders.retain(|folder| !folder.folder_id.uuid().is_nil());
    items.retain(|item| !item.item_id.uuid().is_nil());
    (folders, items)
}

/// Parses the synchronous `CreateInventoryCategory` reply
/// (`{ folder_id, name, parent_id, type }`) into the created folder.
pub(crate) fn created_category_from_llsd(body: &Llsd) -> Option<InventoryFolder> {
    let folder_id = uuid_member(body, "folder_id");
    if folder_id.is_nil() {
        return None;
    }
    Some(InventoryFolder {
        folder_id: InventoryFolderKey::from(folder_id),
        parent_id: crate::types::optional_key_from_wire(uuid_member(body, "parent_id")),
        name: string_member(body, "name"),
        folder_type: i8::try_from(i32_member(body, "type")).unwrap_or(-1),
        version: 1,
    })
}

/// Builds an [`InventoryFolder`] from a CAPS `categories` entry.
pub(crate) fn inventory_folder_from_llsd(category: &Llsd) -> InventoryFolder {
    InventoryFolder {
        folder_id: InventoryFolderKey::from(uuid_member(category, "category_id")),
        parent_id: crate::types::optional_key_from_wire(uuid_member(category, "parent_id")),
        name: string_member(category, "name"),
        folder_type: i8::try_from(i32_member(category, "type_default")).unwrap_or(-1),
        version: i32_member(category, "version"),
    }
}

/// Builds an [`InventoryItem`] from a CAPS `items` entry (with nested
/// `permissions` and `sale_info` maps).
pub(crate) fn inventory_item_from_llsd(item: &Llsd) -> Option<InventoryItem> {
    let permissions = item.get("permissions");
    let sale_info = item.get("sale_info");
    let perm = |key: &str| {
        permissions
            .map_or(0, |p| i32_member(p, key))
            .cast_unsigned()
    };
    let perm_uuid = |key: &str| permissions.map_or_else(Uuid::nil, |p| uuid_member(p, key));
    Some(InventoryItem {
        item_id: InventoryKey::from(uuid_member(item, "item_id")),
        folder_id: InventoryFolderKey::from(uuid_member(item, "parent_id")),
        name: string_member(item, "name"),
        description: string_member(item, "desc"),
        asset_id: uuid_member(item, "asset_id"),
        item_type: i8::try_from(i32_member(item, "type")).unwrap_or(-1),
        inv_type: i8::try_from(i32_member(item, "inv_type")).unwrap_or(-1),
        flags: i32_member(item, "flags").cast_unsigned(),
        sale_type: sale_info.map_or(0, |s| u8::try_from(i32_member(s, "sale_type")).unwrap_or(0)),
        sale_price: crate::types::linden_price_from_wire(
            sale_info.map_or(0, |s| i32_member(s, "sale_type")) != 0,
            "sale_price",
            sale_info.map_or(0, |s| i32_member(s, "sale_price")),
        )
        .ok()?,
        creation_date: i32_member(item, "created_at"),
        owner: crate::types::inventory_owner_from_wire(
            perm_uuid("owner_id"),
            perm_uuid("group_id"),
            permissions
                .and_then(|p| p.get("is_owner_group"))
                .and_then(Llsd::as_bool)
                .unwrap_or(false),
        ),
        last_owner_id: perm_uuid("last_owner_id"),
        creator_id: AgentKey::from(perm_uuid("creator_id")),
        group: crate::types::group_from_wire(perm_uuid("group_id")),
        permissions: Permissions5 {
            base: Permissions::from_bits(perm("base_mask")),
            owner: Permissions::from_bits(perm("owner_mask")),
            group: Permissions::from_bits(perm("group_mask")),
            everyone: Permissions::from_bits(perm("everyone_mask")),
            next_owner: Permissions::from_bits(perm("next_owner_mask")),
        },
    })
}

// ---------------------------------------------------------------------------
// CAPS event serializers (#59 / Tier F): the server direction of the CAPS
// event-queue and HTTP-capability bodies. Each `*_to_llsd` below is the
// element-by-element inverse of the matching `*_from_llsd` parser above — a
// simulator / grid-service uses them to *produce* the LLSD bodies the client
// decodes, so an `Llsd` round-trips back to an equal decoded value. The
// top-level encoders are exported `pub` (terrain-style: encoders without a
// runtime consumer yet, reused by the `SimSession` skeleton, #60); the leaf
// folder/item/record helpers stay `pub(crate)`. The batch wrapper
// `build_event_queue_response` lives in `sl-wire` beside its parser.
// ---------------------------------------------------------------------------

/// Builds an LLSD map from `(key, value)` pairs, owning the keys. Keeps the
/// serializers readable versus hand-building a [`HashMap`].
pub(crate) fn llsd_map(entries: Vec<(&str, Llsd)>) -> Llsd {
    Llsd::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// Masks a value to its low byte (the `& 0xff` always fits a `u8`).
pub(crate) fn low_byte(value: u32) -> u8 {
    u8::try_from(value & 0xff).unwrap_or(0)
}

/// Masks a 64-bit value to its low byte.
pub(crate) fn low_byte64(value: u64) -> u8 {
    u8::try_from(value & 0xff).unwrap_or(0)
}

/// The four big-endian bytes of a `u32` (the `big_endian_bytes` lint forbids
/// `to_be_bytes`, so the bytes are extracted by hand, mirroring `llsd_u32`).
pub(crate) fn u32_be_bytes(value: u32) -> [u8; 4] {
    [
        low_byte(value >> 24),
        low_byte(value >> 16),
        low_byte(value >> 8),
        low_byte(value),
    ]
}

/// The eight big-endian bytes of a `u64` (mirrors `llsd_u64`).
pub(crate) fn u64_be_bytes(value: u64) -> [u8; 8] {
    [
        low_byte64(value >> 56),
        low_byte64(value >> 48),
        low_byte64(value >> 40),
        low_byte64(value >> 32),
        low_byte64(value >> 24),
        low_byte64(value >> 16),
        low_byte64(value >> 8),
        low_byte64(value),
    ]
}

/// Encodes a `u32` the way `llsd_u32` reads one: a plain integer when it fits in
/// an `i32`, else the 4-byte big-endian binary OpenSim uses for `uint` fields.
pub(crate) fn u32_to_llsd(value: u32) -> Llsd {
    i32::try_from(value).map_or_else(
        |_ignored| Llsd::Binary(u32_be_bytes(value).to_vec()),
        Llsd::Integer,
    )
}

/// Encodes a `u64` the way `llsd_u64` reads one: a plain integer when it fits in
/// an `i32`, else the 8-byte big-endian binary OpenSim uses for `U64` fields.
pub(crate) fn u64_to_llsd(value: u64) -> Llsd {
    i32::try_from(value).map_or_else(
        |_ignored| Llsd::Binary(u64_be_bytes(value).to_vec()),
        Llsd::Integer,
    )
}

/// Encodes a `(x, y, z)` vector as an LLSD `[x, y, z]` real array (the inverse
/// of [`vec3_from_llsd`] / [`llsd_position`]).
pub(crate) fn vec3_to_llsd(vector: (f32, f32, f32)) -> Llsd {
    let (x, y, z) = vector;
    Llsd::Array(vec![
        Llsd::Real(f64::from(x)),
        Llsd::Real(f64::from(y)),
        Llsd::Real(f64::from(z)),
    ])
}

/// Encodes region-local coordinates as an LLSD `[x, y, z]` real array (the
/// inverse of [`region_coords_from_llsd`]).
pub(crate) fn region_coords_to_llsd(coords: RegionCoordinates) -> Llsd {
    vec3_to_llsd((coords.x(), coords.y(), coords.z()))
}

/// Encodes a facing direction as an LLSD `[x, y, z]` real array (the inverse of
/// [`direction_from_llsd`]).
pub(crate) fn direction_to_llsd(direction: Direction) -> Llsd {
    vec3_to_llsd((direction.x(), direction.y(), direction.z()))
}

/// The four IPv4 octets of a socket address (the only address family the wire
/// uses); an IPv6 address degrades to zeroes.
pub(crate) const fn ipv4_octets(addr: SocketAddr) -> [u8; 4] {
    match addr.ip() {
        IpAddr::V4(v4) => v4.octets(),
        IpAddr::V6(_) => [0, 0, 0, 0],
    }
}

/// Serializes a CAPS `TeleportFinish` event body from the destination address,
/// seed capability, maturity (`SimAccess`) and teleport flags (the
/// element-by-element inverse of the `teleport_finish_from_llsd` parser, whose
/// decoded `CapsTeleportFinish` is a private type).
#[must_use]
pub fn teleport_finish_to_llsd(
    dest: SocketAddr,
    seed: &str,
    sim_access: u8,
    teleport_flags: u32,
) -> Llsd {
    let info = llsd_map(vec![
        ("SimIP", Llsd::Binary(ipv4_octets(dest).to_vec())),
        ("SimPort", Llsd::Integer(i32::from(dest.port()))),
        ("SeedCapability", Llsd::String(seed.to_owned())),
        ("SimAccess", Llsd::Integer(i32::from(sim_access))),
        ("TeleportFlags", u32_to_llsd(teleport_flags)),
    ]);
    llsd_map(vec![("Info", Llsd::Array(vec![info]))])
}

/// Serializes a neighbour's region handle and address as a CAPS
/// `EnableSimulator` event body (inverse of `enable_simulator_from_caps_llsd`).
#[must_use]
pub fn enable_simulator_to_caps_llsd(handle: u64, sim: SocketAddr) -> Llsd {
    let info = llsd_map(vec![
        ("Handle", u64_to_llsd(handle)),
        ("IP", Llsd::Binary(ipv4_octets(sim).to_vec())),
        ("Port", Llsd::Integer(i32::from(sim.port()))),
    ]);
    llsd_map(vec![("SimulatorInfo", Llsd::Array(vec![info]))])
}

/// Serializes the destination region handle, address and seed capability as a
/// CAPS `CrossedRegion` event body (inverse of `crossed_region_from_caps_llsd`).
#[must_use]
pub fn crossed_region_to_caps_llsd(handle: u64, dest: SocketAddr, seed: &str) -> Llsd {
    let region = llsd_map(vec![
        ("RegionHandle", u64_to_llsd(handle)),
        ("SimIP", Llsd::Binary(ipv4_octets(dest).to_vec())),
        ("SimPort", Llsd::Integer(i32::from(dest.port()))),
        ("SeedCapability", Llsd::String(seed.to_owned())),
    ]);
    llsd_map(vec![("RegionData", Llsd::Array(vec![region]))])
}

/// Serializes a child region's address and seed capability as a CAPS
/// `EstablishAgentCommunication` event body (inverse of
/// `establish_agent_communication_from_llsd`).
#[must_use]
pub fn establish_agent_communication_to_llsd(sim: SocketAddr, seed: &str) -> Llsd {
    llsd_map(vec![
        ("sim-ip-and-port", Llsd::String(sim.to_string())),
        ("seed-capability", Llsd::String(seed.to_owned())),
    ])
}

/// Serializes the pathfinding `can_modify_navmesh` flag as a CAPS
/// `AgentStateUpdate` event body (inverse of `agent_state_update_from_llsd`).
#[must_use]
pub fn agent_state_update_to_llsd(can_modify_navmesh: bool) -> Llsd {
    llsd_map(vec![(
        "can_modify_navmesh",
        Llsd::Boolean(can_modify_navmesh),
    )])
}

/// Serializes a [`NavMeshStatus`] as a CAPS `NavMeshStatusUpdate` event body
/// (inverse of `nav_mesh_status_from_llsd`).
#[must_use]
pub fn nav_mesh_status_to_llsd(status: &NavMeshStatus) -> Llsd {
    llsd_map(vec![
        ("region_id", Llsd::Uuid(status.region_id)),
        ("version", u32_to_llsd(status.version)),
        ("status", Llsd::String(status.status.to_wire().to_owned())),
    ])
}

/// Serializes the agent's own id and the dropped group's key as a CAPS
/// `AgentDropGroup` event body (inverse of `agent_drop_group_from_llsd`). The
/// echoed `AgentID` is the agent the simulator removed from `group`.
#[must_use]
pub fn agent_drop_group_to_llsd(agent_id: AgentKey, group: GroupKey) -> Llsd {
    let entry = llsd_map(vec![
        ("AgentID", Llsd::Uuid(agent_id.uuid())),
        ("GroupID", Llsd::Uuid(group.uuid())),
    ]);
    llsd_map(vec![("AgentData", Llsd::Array(vec![entry]))])
}

/// Serializes a [`DisplayNameUpdate`] as a CAPS `DisplayNameUpdate` event body
/// (inverse of `display_name_update_from_llsd`). The top-level `agent_id`
/// echoes the record's own id, matching the reference viewer's
/// `LLAvatarNameCache` push.
#[must_use]
pub fn display_name_update_to_llsd(update: &DisplayNameUpdate) -> Llsd {
    llsd_map(vec![
        ("agent_id", Llsd::Uuid(update.name.id.uuid())),
        (
            "old_display_name",
            Llsd::String(update.old_display_name.clone()),
        ),
        ("agent", update.name.to_llsd()),
    ])
}

/// Serializes a [`SetDisplayNameReply`] as a CAPS `SetDisplayNameReply` event
/// body (inverse of `set_display_name_reply_from_llsd`). The `content` map
/// carries the new name on success or the error tag on failure.
#[must_use]
pub fn set_display_name_reply_to_llsd(reply: &SetDisplayNameReply) -> Llsd {
    let mut content = Vec::new();
    if let Some(name) = &reply.new_display_name {
        content.push(("display_name", Llsd::String(name.clone())));
    }
    if let Some(tag) = &reply.error_tag {
        content.push(("error_tag", Llsd::String(tag.clone())));
    }
    llsd_map(vec![
        ("status", Llsd::Integer(reply.status)),
        ("reason", Llsd::String(reply.reason.clone())),
        ("content", llsd_map(content)),
    ])
}

/// Serializes the windlight-refresh interpolate flag as a CAPS `WindLightRefresh`
/// event body (inverse of `windlight_refresh_from_llsd`).
#[must_use]
pub fn windlight_refresh_to_llsd(interpolate: bool) -> Llsd {
    llsd_map(vec![("Interpolate", Llsd::Integer(i32::from(interpolate)))])
}

/// Serializes a region debug-console command's output as a CAPS
/// `SimConsoleResponse` event body — a bare LLSD string, not a map (inverse of
/// `sim_console_response_from_llsd`).
#[must_use]
pub fn sim_console_response_to_llsd(output: &str) -> Llsd {
    Llsd::String(output.to_owned())
}

/// Serializes a [`RequiredVoiceVersion`] as a CAPS `RequiredVoiceVersion` event
/// body (inverse of `required_voice_version_from_llsd`). `voice_server_type` is
/// omitted when `None`, matching older grids.
#[must_use]
pub fn required_voice_version_to_llsd(version: &RequiredVoiceVersion) -> Llsd {
    let mut entries = vec![
        ("major_version", Llsd::Integer(version.major_version)),
        ("region_name", Llsd::String(version.region_name.clone())),
    ];
    if let Some(server_type) = &version.voice_server_type {
        entries.push(("voice_server_type", Llsd::String(server_type.clone())));
    }
    llsd_map(entries)
}

/// Serializes an [`OpenRegionInfo`] as a CAPS `OpenRegionInfo` event body
/// (inverse of `open_region_info_from_llsd`). Only the `Some` fields are
/// emitted, so an all-`None` value yields an empty map — the simulator sends
/// just the keys it wants to override.
#[must_use]
pub fn open_region_info_to_llsd(info: &OpenRegionInfo) -> Llsd {
    let mut entries: HashMap<String, Llsd> = HashMap::new();
    let mut flag = |key: &str, value: Option<bool>| {
        if let Some(value) = value {
            let _previous = entries.insert(key.to_owned(), Llsd::Integer(i32::from(value)));
        }
    };
    flag("AllowMinimap", info.allow_minimap);
    flag("AllowPhysicalPrims", info.allow_physical_prims);
    flag("ForceDrawDistance", info.force_draw_distance);
    flag("OffsetOfUTCDST", info.offset_of_utc_dst);
    flag("RenderWater", info.render_water);
    flag("ToggleTeenMode", info.teen_mode);
    flag("EnforceMaxBuild", info.enforce_max_build);
    flag("AllowParcelWindLight", info.allow_parcel_windlight);
    let mut real = |key: &str, value: Option<f32>| {
        if let Some(value) = value {
            let _previous = entries.insert(key.to_owned(), Llsd::Real(f64::from(value)));
        }
    };
    real("DrawDistance", info.draw_distance);
    real("TerrainDetailScale", info.terrain_detail_scale);
    real("MaxDragDistance", info.max_drag_distance);
    real("MinHoleSize", info.min_hole_size);
    real("MaxHollowSize", info.max_hollow_size);
    real("MaxPrimScale", info.max_prim_scale);
    real("MaxPhysPrimScale", info.max_phys_prim_scale);
    real("MinPrimScale", info.min_prim_scale);
    real("SayDistance", info.say_distance);
    real("ShoutDistance", info.shout_distance);
    real("WhisperDistance", info.whisper_distance);
    let mut int = |key: &str, value: Option<i32>| {
        if let Some(value) = value {
            let _previous = entries.insert(key.to_owned(), Llsd::Integer(value));
        }
    };
    int(
        "MaxInventoryItemsTransfer",
        info.max_inventory_items_transfer,
    );
    int("MaxLinkCount", info.max_link_count);
    int("MaxLinkCountPhys", info.max_link_count_phys);
    int("OffsetOfUTC", info.offset_of_utc);
    int("ShowTags", info.show_tags);
    int("MaxGroups", info.max_groups);
    let mut position = |prefix: &str, value: Option<RegionCoordinates>| {
        if let Some(value) = value {
            let _x = entries.insert(format!("{prefix}X"), Llsd::Real(f64::from(value.x())));
            let _y = entries.insert(format!("{prefix}Y"), Llsd::Real(f64::from(value.y())));
            let _z = entries.insert(format!("{prefix}Z"), Llsd::Real(f64::from(value.z())));
        }
    };
    position("MaxPos", info.max_position);
    position("MinPos", info.min_position);
    Llsd::Map(entries)
}

/// Serializes an [`Event::ServerAppearanceUpdate`] as an `UpdateAvatarAppearance`
/// capability reply body (inverse of `server_appearance_update_from_llsd`).
#[must_use]
pub fn server_appearance_update_to_llsd(event: &Event) -> Llsd {
    let Event::ServerAppearanceUpdate {
        success,
        error,
        expected_cof_version,
    } = event
    else {
        return Llsd::Undef;
    };
    let mut entries = vec![("success", Llsd::Boolean(*success))];
    if let Some(error) = error {
        entries.push(("error", Llsd::String(error.clone())));
    }
    if let Some(expected) = *expected_cof_version {
        entries.push(("expected", Llsd::Integer(expected)));
    }
    llsd_map(entries)
}

/// Encodes an `f32` as an LLSD real.
fn real(value: f32) -> Llsd {
    Llsd::Real(f64::from(value))
}

/// Encodes an RGB [`Color`] as an LLSD `[r, g, b]` real array.
fn color_to_llsd(color: Color) -> Llsd {
    reals_to_llsd(&[color.red(), color.green(), color.blue()])
}

/// Encodes an RGBA [`ColorAlpha`] as an LLSD `[r, g, b, a]` real array.
fn color_alpha_to_llsd(color: ColorAlpha) -> Llsd {
    reals_to_llsd(&[color.red(), color.green(), color.blue(), color.alpha()])
}

/// Encodes a [`Scale`] as an LLSD `[x, y, z]` real array.
fn scale_to_llsd(scale: Scale) -> Llsd {
    reals_to_llsd(&[scale.x(), scale.y(), scale.z()])
}

/// Encodes a [`Glow`] as an LLSD `[size, reserved, focus]` real array.
fn glow_to_llsd(glow: Glow) -> Llsd {
    reals_to_llsd(&[glow.size(), glow.reserved(), glow.focus()])
}

/// Encodes a [`CloudPosDensity`] as an LLSD `[x, y, density]` real array.
fn cloud_pos_density_to_llsd(value: CloudPosDensity) -> Llsd {
    reals_to_llsd(&[value.position_x(), value.position_y(), value.density()])
}

/// Encodes a slice of `f32` components as an LLSD array of reals (used for the
/// colour / vector / rotation tuples in environment frames).
fn reals_to_llsd(values: &[f32]) -> Llsd {
    Llsd::Array(values.iter().copied().map(real).collect())
}

/// Encodes [`SkySettings`] into a sky-frame `OSDMap` (the inverse of
/// `sky_settings_from_llsd`). The legacy haze colours/scalars go into a
/// `legacy_haze` sub-map, as the viewer expects.
fn sky_settings_to_llsd(sky: &SkySettings) -> Llsd {
    let legacy_haze = llsd_map(vec![
        ("ambient", color_to_llsd(sky.ambient)),
        ("blue_horizon", color_to_llsd(sky.blue_horizon)),
        ("blue_density", color_to_llsd(sky.blue_density)),
        ("haze_horizon", real(sky.haze_horizon)),
        ("haze_density", real(sky.haze_density)),
        ("density_multiplier", real(sky.density_multiplier)),
        ("distance_multiplier", real(sky.distance_multiplier)),
    ]);
    llsd_map(vec![
        ("type", Llsd::String("sky".to_owned())),
        ("name", Llsd::String(sky.name.clone())),
        (
            "sun_rotation",
            reals_to_llsd(&[
                sky.sun_rotation.x,
                sky.sun_rotation.y,
                sky.sun_rotation.z,
                sky.sun_rotation.s,
            ]),
        ),
        (
            "moon_rotation",
            reals_to_llsd(&[
                sky.moon_rotation.x,
                sky.moon_rotation.y,
                sky.moon_rotation.z,
                sky.moon_rotation.s,
            ]),
        ),
        ("sunlight_color", color_alpha_to_llsd(sky.sunlight_color)),
        ("legacy_haze", legacy_haze),
        ("max_y", real(sky.max_y)),
        ("gamma", real(sky.gamma)),
        ("cloud_color", color_to_llsd(sky.cloud_color)),
        (
            "cloud_pos_density1",
            cloud_pos_density_to_llsd(sky.cloud_pos_density1),
        ),
        (
            "cloud_pos_density2",
            cloud_pos_density_to_llsd(sky.cloud_pos_density2),
        ),
        ("cloud_scale", real(sky.cloud_scale)),
        ("cloud_scroll_rate", reals_to_llsd(&sky.cloud_scroll_rate)),
        ("cloud_shadow", real(sky.cloud_shadow)),
        ("cloud_variance", real(sky.cloud_variance)),
        ("glow", glow_to_llsd(sky.glow)),
        ("star_brightness", real(sky.star_brightness)),
        ("sun_scale", real(sky.sun_scale)),
        ("moon_scale", real(sky.moon_scale)),
        ("moon_brightness", real(sky.moon_brightness)),
        ("sun_arc_radians", real(sky.sun_arc_radians)),
        ("droplet_radius", real(sky.droplet_radius)),
        ("ice_level", real(sky.ice_level)),
        ("moisture_level", real(sky.moisture_level)),
        ("sky_top_radius", real(sky.sky_top_radius)),
        ("sky_bottom_radius", real(sky.sky_bottom_radius)),
        ("planet_radius", real(sky.planet_radius)),
        ("sun_id", optional_texture_to_llsd(sky.sun_texture)),
        ("moon_id", optional_texture_to_llsd(sky.moon_texture)),
        ("cloud_id", optional_texture_to_llsd(sky.cloud_texture)),
        ("bloom_id", optional_texture_to_llsd(sky.bloom_texture)),
        ("halo_id", optional_texture_to_llsd(sky.halo_texture)),
        ("rainbow_id", optional_texture_to_llsd(sky.rainbow_texture)),
    ])
}

/// Encodes [`WaterSettings`] into a water-frame `OSDMap` (the inverse of
/// `water_settings_from_llsd`).
fn water_settings_to_llsd(water: &WaterSettings) -> Llsd {
    llsd_map(vec![
        ("type", Llsd::String("water".to_owned())),
        ("name", Llsd::String(water.name.clone())),
        ("blur_multiplier", real(water.blur_multiplier)),
        ("fresnel_offset", real(water.fresnel_offset)),
        ("fresnel_scale", real(water.fresnel_scale)),
        ("normal_scale", scale_to_llsd(water.normal_scale)),
        ("normal_map", optional_texture_to_llsd(water.normal_map)),
        ("scale_above", real(water.scale_above)),
        ("scale_below", real(water.scale_below)),
        (
            "transparent_texture",
            optional_texture_to_llsd(water.transparent_texture),
        ),
        ("underwater_fog_mod", real(water.underwater_fog_mod)),
        ("water_fog_color", color_to_llsd(water.water_fog_color)),
        ("water_fog_density", real(water.water_fog_density)),
        ("wave1_direction", reals_to_llsd(&water.wave1_direction)),
        ("wave2_direction", reals_to_llsd(&water.wave2_direction)),
    ])
}

/// Encodes one day-cycle track (its keyframes) as an LLSD array of
/// `{key_keyframe, key_name}` maps.
fn track_to_llsd(track: &[DayCycleFrame]) -> Llsd {
    Llsd::Array(
        track
            .iter()
            .map(|frame| {
                llsd_map(vec![
                    ("key_keyframe", real(frame.keyframe)),
                    ("key_name", Llsd::String(frame.name.clone())),
                ])
            })
            .collect(),
    )
}

/// Encodes a [`DayCycle`] into a day-cycle `OSDMap`: its named frames and its
/// tracks (the water track first, then the sky tracks).
fn day_cycle_to_llsd(cycle: &DayCycle) -> Llsd {
    let mut frames: Vec<(&str, Llsd)> = Vec::new();
    for (name, sky) in &cycle.sky_frames {
        frames.push((name.as_str(), sky_settings_to_llsd(sky)));
    }
    for (name, water) in &cycle.water_frames {
        frames.push((name.as_str(), water_settings_to_llsd(water)));
    }
    let mut tracks = vec![track_to_llsd(&cycle.water_track)];
    tracks.extend(cycle.sky_tracks.iter().map(|track| track_to_llsd(track)));
    llsd_map(vec![
        ("type", Llsd::String("daycycle".to_owned())),
        ("name", Llsd::String(cycle.name.clone())),
        ("frames", llsd_map(frames)),
        ("tracks", Llsd::Array(tracks)),
    ])
}

/// Encodes [`EnvironmentSettings`] into the `ExtEnvironment` GET response
/// envelope (the inverse of `environment_from_llsd`): an `environment` map
/// wrapped with `parcel_id` and `success`.
#[must_use]
pub fn environment_to_llsd(env: &EnvironmentSettings) -> Llsd {
    let environment = llsd_map(vec![
        ("parcel_id", Llsd::Integer(env.parcel_id)),
        ("region_id", Llsd::Uuid(env.region_id)),
        ("day_length", Llsd::Integer(env.day_length)),
        ("day_offset", Llsd::Integer(env.day_offset)),
        ("flags", u32_to_llsd(env.flags)),
        ("env_version", Llsd::Integer(env.env_version)),
        ("track_altitudes", reals_to_llsd(&env.track_altitudes)),
        ("day_cycle", day_cycle_to_llsd(&env.day_cycle)),
    ]);
    llsd_map(vec![
        ("environment", environment),
        ("parcel_id", Llsd::Integer(env.parcel_id)),
        ("success", Llsd::Boolean(true)),
    ])
}

/// Serializes a [`ParcelInfo`] as a CAPS `ParcelProperties` event body (inverse
/// of `parcel_info_from_llsd`). The three trailing single-blocks the parser
/// reads are emitted as one-element arrays, and the CAPS-only `SeeAVs` /
/// `AnyAVSounds` / `GroupAVSounds` booleans only when present.
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// the L$ sale price exceeds the signed 32-bit range the wire field can hold.
#[expect(
    clippy::too_many_lines,
    reason = "one entry per ParcelProperties field — a flat inverse of the parser"
)]
pub fn parcel_info_to_llsd(info: &ParcelInfo) -> Result<Llsd, sl_wire::WireError> {
    let mut data = vec![
        ("SequenceID", Llsd::Integer(info.sequence_id)),
        ("RequestResult", Llsd::Integer(info.request_result.to_i32())),
        ("SnapSelection", Llsd::Boolean(info.snap_selection)),
        ("SelfCount", Llsd::Integer(info.self_count)),
        ("OtherCount", Llsd::Integer(info.other_count)),
        ("PublicCount", Llsd::Integer(info.public_count)),
        ("LocalID", Llsd::Integer(info.local_id.0)),
        ("OwnerID", Llsd::Uuid(info.owner.uuid())),
        ("IsGroupOwned", Llsd::Boolean(info.owner.is_group())),
        (
            "GroupID",
            Llsd::Uuid(crate::types::group_to_wire(info.group)),
        ),
        ("AuctionID", u32_to_llsd(info.auction_id)),
        ("ClaimDate", Llsd::Integer(info.claim_date)),
        (
            "ClaimPrice",
            Llsd::Integer(crate::types::linden_to_wire(
                "ClaimPrice",
                &info.claim_price,
            )?),
        ),
        (
            "RentPrice",
            Llsd::Integer(crate::types::linden_to_wire("RentPrice", &info.rent_price)?),
        ),
        ("AABBMin", region_coords_to_llsd(info.aabb_min)),
        ("AABBMax", region_coords_to_llsd(info.aabb_max)),
        (
            "Area",
            Llsd::Integer(crate::types::land_area_to_wire("Area", &info.area)?),
        ),
        ("Bitmap", Llsd::Binary(info.bitmap.clone())),
        ("Status", Llsd::Integer(info.status.to_i32())),
        ("Category", Llsd::Integer(i32::from(info.category.to_u8()))),
        ("MaxPrims", Llsd::Integer(info.max_prims)),
        ("SimWideMaxPrims", Llsd::Integer(info.sim_wide_max_prims)),
        (
            "SimWideTotalPrims",
            Llsd::Integer(info.sim_wide_total_prims),
        ),
        ("TotalPrims", Llsd::Integer(info.total_prims)),
        ("OwnerPrims", Llsd::Integer(info.owner_prims)),
        ("GroupPrims", Llsd::Integer(info.group_prims)),
        ("OtherPrims", Llsd::Integer(info.other_prims)),
        ("SelectedPrims", Llsd::Integer(info.selected_prims)),
        (
            "ParcelPrimBonus",
            Llsd::Real(f64::from(info.parcel_prim_bonus)),
        ),
        ("OtherCleanTime", Llsd::Integer(info.other_clean_time)),
        ("ParcelFlags", u32_to_llsd(info.raw_parcel_flags)),
        (
            "SalePrice",
            Llsd::Integer(crate::types::linden_price_to_wire(
                "SalePrice",
                info.sale_price.as_ref(),
            )?),
        ),
        ("Name", Llsd::String(info.name.clone())),
        ("Desc", Llsd::String(info.description.clone())),
        (
            "MusicURL",
            Llsd::String(sl_wire::optional_url_to_wire(info.music_url.as_ref())),
        ),
        (
            "MediaURL",
            Llsd::String(sl_wire::optional_url_to_wire(info.media_url.as_ref())),
        ),
        (
            "MediaID",
            Llsd::Uuid(crate::types::optional_key_to_wire(info.media_id, |k| {
                k.uuid()
            })),
        ),
        ("MediaAutoScale", Llsd::Boolean(info.media_auto_scale)),
        (
            "AuthBuyerID",
            Llsd::Uuid(crate::types::optional_key_to_wire(
                info.auth_buyer_id,
                |k| k.uuid(),
            )),
        ),
        (
            "SnapshotID",
            Llsd::Uuid(crate::types::optional_key_to_wire(info.snapshot_id, |k| {
                k.uuid()
            })),
        ),
        (
            "PassPrice",
            Llsd::Integer(crate::types::linden_to_wire("PassPrice", &info.pass_price)?),
        ),
        ("PassHours", Llsd::Real(f64::from(info.pass_hours))),
        ("UserLocation", region_coords_to_llsd(info.user_location)),
        ("UserLookAt", direction_to_llsd(info.user_look_at)),
        (
            "LandingType",
            Llsd::Integer(i32::from(info.landing_type.to_u8())),
        ),
        (
            "RegionPushOverride",
            Llsd::Boolean(info.region_push_override),
        ),
        (
            "RegionDenyAnonymous",
            Llsd::Boolean(info.region_deny_anonymous),
        ),
        (
            "RegionDenyIdentified",
            Llsd::Boolean(info.region_deny_identified),
        ),
        (
            "RegionDenyTransacted",
            Llsd::Boolean(info.region_deny_transacted),
        ),
    ];
    if let Some(see_avs) = info.see_avs {
        data.push(("SeeAVs", Llsd::Boolean(see_avs)));
    }
    if let Some(any_av_sounds) = info.any_av_sounds {
        data.push(("AnyAVSounds", Llsd::Boolean(any_av_sounds)));
    }
    if let Some(group_av_sounds) = info.group_av_sounds {
        data.push(("GroupAVSounds", Llsd::Boolean(group_av_sounds)));
    }
    let age_verification = llsd_map(vec![(
        "RegionDenyAgeUnverified",
        Llsd::Boolean(info.region_deny_age_unverified),
    )]);
    let region_allow_access = llsd_map(vec![(
        "RegionAllowAccessOverride",
        Llsd::Boolean(info.region_allow_access_override),
    )]);
    let parcel_environment = llsd_map(vec![
        (
            "ParcelEnvironmentVersion",
            Llsd::Integer(info.parcel_environment_version),
        ),
        (
            "RegionAllowEnvironmentOverride",
            Llsd::Boolean(info.region_allow_environment_override),
        ),
    ]);
    Ok(llsd_map(vec![
        ("ParcelData", Llsd::Array(vec![llsd_map(data)])),
        ("AgeVerificationBlock", Llsd::Array(vec![age_verification])),
        (
            "RegionAllowAccessBlock",
            Llsd::Array(vec![region_allow_access]),
        ),
        (
            "ParcelEnvironmentBlock",
            Llsd::Array(vec![parcel_environment]),
        ),
    ]))
}

/// Serializes offline IMs as a `ReadOfflineMsgs` capability reply body — an
/// array of per-message records (inverse of `offline_messages_from_llsd`).
#[must_use]
pub fn offline_messages_to_llsd(messages: &[InstantMessage]) -> Llsd {
    Llsd::Array(messages.iter().map(offline_message_to_record).collect())
}

/// Serializes one [`InstantMessage`] as an offline-IM record (inverse of
/// [`offline_message_from_record`]). The `offline` flag is implicit (the parser
/// always marks these messages offline), so it is not emitted.
pub(crate) fn offline_message_to_record(im: &InstantMessage) -> Llsd {
    llsd_map(vec![
        ("from_agent_id", Llsd::Uuid(im.from_agent_id.uuid())),
        ("from_agent_name", Llsd::String(im.from_agent_name.clone())),
        ("to_agent_id", Llsd::Uuid(im.to_agent_id.uuid())),
        ("dialog", Llsd::Integer(i32::from(im.dialog.to_u8()))),
        ("from_group", Llsd::Boolean(im.from_group)),
        (
            "region_id",
            Llsd::Uuid(crate::types::optional_uuid_to_wire(im.region_id)),
        ),
        ("position", region_coords_to_llsd(im.position)),
        (
            "timestamp",
            u32_to_llsd(crate::types::optional_u32_to_wire(im.timestamp)),
        ),
        ("transaction-id", Llsd::Uuid(im.id)),
        ("parent_estate_id", u32_to_llsd(im.parent_estate_id)),
        ("message", Llsd::String(im.message.clone())),
        ("binary_bucket", Llsd::Binary(im.binary_bucket.clone())),
    ])
}

/// Serializes an [`Event::ConferenceInvited`] as a `ChatterBoxInvitation` event
/// body (inverse of `chatterbox_invitation_from_llsd`). `session_name` sits at
/// the top level and the bucket nests under `message_params.data`, matching the
/// shape the parser reads.
#[must_use]
pub fn chatterbox_invitation_to_llsd(event: &Event) -> Llsd {
    let Event::ConferenceInvited {
        session_id,
        from_agent_id,
        from_name,
        dialog,
        from_group,
        session_name,
        message,
        region_id,
        position,
        parent_estate_id,
        timestamp,
        binary_bucket,
    } = event
    else {
        return Llsd::Undef;
    };
    let params = llsd_map(vec![
        ("id", Llsd::Uuid(*session_id)),
        ("from_id", Llsd::Uuid(from_agent_id.uuid())),
        ("from_name", Llsd::String(from_name.clone())),
        ("type", Llsd::Integer(i32::from(dialog.to_u8()))),
        ("from_group", Llsd::Boolean(*from_group)),
        ("message", Llsd::String(message.clone())),
        ("region_id", Llsd::Uuid(*region_id)),
        ("position", region_coords_to_llsd(*position)),
        ("parent_estate_id", u32_to_llsd(*parent_estate_id)),
        (
            "timestamp",
            u32_to_llsd(crate::types::optional_u32_to_wire(*timestamp)),
        ),
        (
            "data",
            llsd_map(vec![("binary_bucket", Llsd::Binary(binary_bucket.clone()))]),
        ),
    ]);
    llsd_map(vec![
        ("session_name", Llsd::String(session_name.clone())),
        ("instantmessage", llsd_map(vec![("message_params", params)])),
    ])
}

/// Serializes an [`Event::GroupMemberships`] as the CAPS event-queue
/// `AgentGroupDataUpdate` body (inverse of `group_memberships_from_caps_llsd`).
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// a membership's land contribution exceeds the signed 32-bit wire range.
pub fn group_memberships_to_caps_llsd(event: &Event) -> Result<Llsd, sl_wire::WireError> {
    let Event::GroupMemberships(memberships) = event else {
        return Ok(Llsd::Undef);
    };
    let groups = memberships
        .iter()
        .map(|membership| {
            Ok(llsd_map(vec![
                ("GroupID", Llsd::Uuid(membership.group_id.uuid())),
                ("GroupPowers", u64_to_llsd(membership.group_powers)),
                ("AcceptNotices", Llsd::Boolean(membership.accept_notices)),
                (
                    "GroupInsigniaID",
                    Llsd::Uuid(membership.group_insignia_id.uuid()),
                ),
                (
                    "Contribution",
                    Llsd::Integer(crate::types::land_area_to_wire(
                        "Contribution",
                        &membership.contribution,
                    )?),
                ),
                ("GroupName", Llsd::String(membership.group_name.clone())),
            ]))
        })
        .collect::<Result<_, sl_wire::WireError>>()?;
    Ok(llsd_map(vec![("GroupData", Llsd::Array(groups))]))
}

/// Serializes an [`Event::GroupMembers`] as a `GroupMemberData` capability
/// response body (inverse of `group_members_from_caps_llsd`). Each member's
/// title is emitted inline by index into the `titles` array, and `request_id` /
/// `member_count` are dropped (the parser sets them itself: nil and the roster
/// length).
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// a member's land contribution exceeds the signed 32-bit wire range.
pub fn group_members_to_caps_llsd(event: &Event) -> Result<Llsd, sl_wire::WireError> {
    let Event::GroupMembers {
        group_id, members, ..
    } = event
    else {
        return Ok(Llsd::Undef);
    };
    let mut titles = Vec::with_capacity(members.len());
    let mut roster = HashMap::with_capacity(members.len());
    for (index, member) in members.iter().enumerate() {
        let title_index = i32::try_from(index).unwrap_or(0);
        titles.push(Llsd::String(member.title.clone()));
        let mut entries = vec![
            (
                "donated_square_meters",
                Llsd::Integer(crate::types::land_area_to_wire(
                    "donated_square_meters",
                    &member.contribution,
                )?),
            ),
            ("last_login", Llsd::String(member.online_status.clone())),
            ("powers", u64_to_llsd(member.agent_powers)),
            ("title", Llsd::Integer(title_index)),
        ];
        if member.is_owner {
            entries.push(("owner", Llsd::Boolean(true)));
        }
        roster.insert(member.agent_id.to_string(), llsd_map(entries));
    }
    Ok(llsd_map(vec![
        ("group_id", Llsd::Uuid(group_id.uuid())),
        ("members", Llsd::Map(roster)),
        ("titles", Llsd::Array(titles)),
        (
            "defaults",
            llsd_map(vec![("default_powers", Llsd::Integer(0))]),
        ),
    ]))
}

/// Serializes `InventoryDescendents` events as a `FetchInventoryDescendents2`
/// capability response body (inverse of `inventory_descendents_from_llsd`):
/// one `folders` entry per event, each carrying its `categories` and `items`.
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// an item's L$ sale price exceeds the signed 32-bit range the wire field can
/// hold.
pub fn inventory_descendents_to_llsd(events: &[Event]) -> Result<Llsd, sl_wire::WireError> {
    let mut folders = Vec::new();
    for event in events {
        let Event::InventoryDescendents {
            folder_id,
            version,
            descendents,
            folders: event_folders,
            items,
        } = event
        else {
            continue;
        };
        let items_llsd = items
            .iter()
            .map(inventory_item_to_llsd)
            .collect::<Result<Vec<_>, _>>()?;
        folders.push(llsd_map(vec![
            ("folder_id", Llsd::Uuid(folder_id.uuid())),
            ("version", Llsd::Integer(*version)),
            ("descendents", Llsd::Integer(*descendents)),
            (
                "categories",
                Llsd::Array(event_folders.iter().map(inventory_folder_to_llsd).collect()),
            ),
            ("items", Llsd::Array(items_llsd)),
        ]));
    }
    Ok(llsd_map(vec![("folders", Llsd::Array(folders))]))
}

/// Serializes a `BulkUpdateInventory` CAPS event-queue body (inverse of
/// `bulk_update_inventory_from_llsd`).
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// an item's L$ sale price exceeds the signed 32-bit range the wire field can
/// hold.
pub fn bulk_update_inventory_to_llsd(
    transaction_id: Uuid,
    folders: &[InventoryFolder],
    items: &[InventoryItem],
) -> Result<Llsd, sl_wire::WireError> {
    let agent = llsd_map(vec![("TransactionID", Llsd::Uuid(transaction_id))]);
    let folder_data = folders
        .iter()
        .map(|folder| {
            llsd_map(vec![
                ("FolderID", Llsd::Uuid(folder.folder_id.uuid())),
                (
                    "ParentID",
                    Llsd::Uuid(crate::types::optional_key_to_wire(folder.parent_id, |p| {
                        p.uuid()
                    })),
                ),
                ("Name", Llsd::String(folder.name.clone())),
                ("Type", Llsd::Integer(i32::from(folder.folder_type))),
            ])
        })
        .collect();
    let item_data = items
        .iter()
        .map(bulk_update_item_to_llsd)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(llsd_map(vec![
        ("AgentData", Llsd::Array(vec![agent])),
        ("FolderData", Llsd::Array(folder_data)),
        ("ItemData", Llsd::Array(item_data)),
    ]))
}

/// Serializes an [`InventoryItem`] as a flat `BulkUpdateInventory` `ItemData`
/// entry (inverse of [`bulk_update_item_from_llsd`]). `last_owner_id` has no
/// place in this wire form (the parser leaves it nil), so it is not emitted.
pub(crate) fn bulk_update_item_to_llsd(item: &InventoryItem) -> Result<Llsd, sl_wire::WireError> {
    let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
    Ok(llsd_map(vec![
        ("ItemID", Llsd::Uuid(item.item_id.uuid())),
        ("FolderID", Llsd::Uuid(item.folder_id.uuid())),
        ("Name", Llsd::String(item.name.clone())),
        ("Description", Llsd::String(item.description.clone())),
        ("AssetID", Llsd::Uuid(item.asset_id)),
        ("Type", Llsd::Integer(i32::from(item.item_type))),
        ("InvType", Llsd::Integer(i32::from(item.inv_type))),
        ("Flags", Llsd::Integer(item.flags.cast_signed())),
        ("SaleType", Llsd::Integer(i32::from(item.sale_type))),
        (
            "SalePrice",
            Llsd::Integer(crate::types::linden_price_to_wire(
                "SalePrice",
                item.sale_price.as_ref(),
            )?),
        ),
        ("CreationDate", Llsd::Integer(item.creation_date)),
        ("OwnerID", Llsd::Uuid(owner_id)),
        ("CreatorID", Llsd::Uuid(item.creator_id.uuid())),
        ("GroupID", Llsd::Uuid(group_id)),
        ("GroupOwned", Llsd::Boolean(item.owner.is_group())),
        (
            "BaseMask",
            Llsd::Integer(item.permissions.base.bits().cast_signed()),
        ),
        (
            "OwnerMask",
            Llsd::Integer(item.permissions.owner.bits().cast_signed()),
        ),
        (
            "GroupMask",
            Llsd::Integer(item.permissions.group.bits().cast_signed()),
        ),
        (
            "EveryoneMask",
            Llsd::Integer(item.permissions.everyone.bits().cast_signed()),
        ),
        (
            "NextOwnerMask",
            Llsd::Integer(item.permissions.next_owner.bits().cast_signed()),
        ),
    ]))
}

/// Serializes folders and items as an AIS3 (`InventoryAPIv3`) response body
/// (inverse of `ais_inventory_update_from_llsd`): the affected objects nest
/// under `_embedded` as uuid-keyed maps.
///
/// # Errors
///
/// Returns [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) if
/// an item's L$ sale price exceeds the signed 32-bit range the wire field can
/// hold.
pub fn ais_inventory_update_to_llsd(
    folders: &[InventoryFolder],
    items: &[InventoryItem],
) -> Result<Llsd, sl_wire::WireError> {
    let categories = folders
        .iter()
        .map(|folder| {
            (
                folder.folder_id.to_string(),
                inventory_folder_to_llsd(folder),
            )
        })
        .collect();
    let item_map = items
        .iter()
        .map(|item| Ok((item.item_id.to_string(), inventory_item_to_llsd(item)?)))
        .collect::<Result<_, sl_wire::WireError>>()?;
    Ok(llsd_map(vec![(
        "_embedded",
        llsd_map(vec![
            ("categories", Llsd::Map(categories)),
            ("items", Llsd::Map(item_map)),
        ]),
    )]))
}

/// Serializes an [`InventoryFolder`] as a `CreateInventoryCategory` reply body
/// (inverse of `created_category_from_llsd`; `version` is fixed at 1 by the
/// parser, so it is not emitted).
#[must_use]
pub fn created_category_to_llsd(folder: &InventoryFolder) -> Llsd {
    llsd_map(vec![
        ("folder_id", Llsd::Uuid(folder.folder_id.uuid())),
        (
            "parent_id",
            Llsd::Uuid(crate::types::optional_key_to_wire(folder.parent_id, |p| {
                p.uuid()
            })),
        ),
        ("name", Llsd::String(folder.name.clone())),
        ("type", Llsd::Integer(i32::from(folder.folder_type))),
    ])
}

/// Serializes an [`InventoryFolder`] as an AIS-shaped `categories` entry (inverse
/// of [`inventory_folder_from_llsd`]).
pub(crate) fn inventory_folder_to_llsd(folder: &InventoryFolder) -> Llsd {
    llsd_map(vec![
        ("category_id", Llsd::Uuid(folder.folder_id.uuid())),
        (
            "parent_id",
            Llsd::Uuid(crate::types::optional_key_to_wire(folder.parent_id, |p| {
                p.uuid()
            })),
        ),
        ("name", Llsd::String(folder.name.clone())),
        ("type_default", Llsd::Integer(i32::from(folder.folder_type))),
        ("version", Llsd::Integer(folder.version)),
    ])
}

/// Serializes an [`InventoryItem`] as an AIS-shaped `items` entry with the nested
/// `permissions` and `sale_info` maps (inverse of [`inventory_item_from_llsd`]).
pub(crate) fn inventory_item_to_llsd(item: &InventoryItem) -> Result<Llsd, sl_wire::WireError> {
    let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
    let permissions = llsd_map(vec![
        (
            "base_mask",
            Llsd::Integer(item.permissions.base.bits().cast_signed()),
        ),
        (
            "owner_mask",
            Llsd::Integer(item.permissions.owner.bits().cast_signed()),
        ),
        (
            "group_mask",
            Llsd::Integer(item.permissions.group.bits().cast_signed()),
        ),
        (
            "everyone_mask",
            Llsd::Integer(item.permissions.everyone.bits().cast_signed()),
        ),
        (
            "next_owner_mask",
            Llsd::Integer(item.permissions.next_owner.bits().cast_signed()),
        ),
        ("owner_id", Llsd::Uuid(owner_id)),
        ("last_owner_id", Llsd::Uuid(item.last_owner_id)),
        ("creator_id", Llsd::Uuid(item.creator_id.uuid())),
        ("group_id", Llsd::Uuid(group_id)),
        ("is_owner_group", Llsd::Boolean(item.owner.is_group())),
    ]);
    let sale_info = llsd_map(vec![
        ("sale_type", Llsd::Integer(i32::from(item.sale_type))),
        (
            "sale_price",
            Llsd::Integer(crate::types::linden_price_to_wire(
                "sale_price",
                item.sale_price.as_ref(),
            )?),
        ),
    ]);
    Ok(llsd_map(vec![
        ("item_id", Llsd::Uuid(item.item_id.uuid())),
        ("parent_id", Llsd::Uuid(item.folder_id.uuid())),
        ("name", Llsd::String(item.name.clone())),
        ("desc", Llsd::String(item.description.clone())),
        ("asset_id", Llsd::Uuid(item.asset_id)),
        ("type", Llsd::Integer(i32::from(item.item_type))),
        ("inv_type", Llsd::Integer(i32::from(item.inv_type))),
        ("flags", Llsd::Integer(item.flags.cast_signed())),
        ("created_at", Llsd::Integer(item.creation_date)),
        ("permissions", permissions),
        ("sale_info", sale_info),
    ]))
}

// ---------------------------------------------------------------------------
// Object / scene graph (#16): assembling decoded objects from full-update
// blocks. The packed `ObjectData`/`Data` blob (de)coders live in
// [`crate::object_update`].
// ---------------------------------------------------------------------------

/// A zero [`Vector`], used as the fall-back for absent/short motion fields.
pub(crate) const ZERO_VECTOR: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// Packs a unit quaternion into the three-float form a `MultipleObjectUpdate`
/// `Data` blob carries (LL's `LLQuaternion::packToVector3`): normalize, then if
/// the real component is negative negate the vector part so the receiver can
/// reconstruct `w = sqrt(1 - x² - y² - z²) >= 0`.
pub(crate) fn pack_quaternion_to_vec3(rotation: &Rotation) -> [f32; 3] {
    let Rotation { x, y, z, s } = *rotation;
    let magnitude = s.mul_add(s, z.mul_add(z, x.mul_add(x, y * y))).sqrt();
    let (mut x, mut y, mut z) = if magnitude > f32::EPSILON {
        (x / magnitude, y / magnitude, z / magnitude)
    } else {
        (x, y, z)
    };
    if s < 0.0 {
        x = -x;
        y = -y;
        z = -z;
    }
    [x, y, z]
}

/// Builds an [`Object`] from a full `ObjectUpdate` object-data block.
///
/// # Errors
///
/// Returns [`WireError::InvalidUrl`](sl_wire::WireError::InvalidUrl) when the
/// block carries a non-empty but unparsable media URL.
pub(crate) fn object_from_full_update(
    block: &ObjectUpdateObjectDataBlock,
    region_handle: RegionHandle,
) -> Result<Object, sl_wire::WireError> {
    Ok(Object {
        region_handle,
        local_id: RegionLocalObjectId(block.id),
        // Stamped by the session when the object is cached (`upsert_object`).
        circuit: crate::scoped_id::CircuitId::default(),
        full_id: ObjectKey::from(block.full_id),
        parent_id: RegionLocalObjectId(block.parent_id),
        pcode: block.p_code,
        state: block.state,
        crc: block.crc,
        material: block.material,
        click_action: block.click_action,
        update_flags: block.update_flags,
        scale: block.scale.clone(),
        motion: crate::object_update::full_object_motion(&block.object_data),
        owner_id: block.owner_id,
        sound: block.sound,
        gain: block.gain,
        sound_flags: block.flags,
        sound_radius: block.radius,
        text: trimmed_string(&block.text),
        text_color: block.text_color,
        name_value: trimmed_string(&block.name_value),
        media_url: sl_wire::optional_url_from_wire("MediaURL", &trimmed_string(&block.media_url))?,
        texture_entry: block.texture_entry.clone(),
        texture_anim: block.texture_anim.clone(),
        texture_animation: crate::particles::decode_texture_anim(&block.texture_anim),
        shape: shape_from_full_block(block),
        particle_system: block.ps_block.clone(),
        particles: crate::particles::decode_particle_system(&block.ps_block),
        data: block.data.clone(),
        extra: crate::extra_params::decode_extra_params(&block.extra_params),
        extra_params: block.extra_params.clone(),
        properties: None,
        joint_type: block.joint_type,
        joint_pivot: block.joint_pivot.clone(),
        joint_axis_or_anchor: block.joint_axis_or_anchor.clone(),
    })
}

/// Reads the path/profile [`PrimShapeParams`] from a full `ObjectUpdate` block's
/// individual shape fields. (The compressed update packs the same values as a
/// single 23-byte blob — see `read_compressed_shape` in
/// [`crate::object_update`].)
pub(crate) const fn shape_from_full_block(block: &ObjectUpdateObjectDataBlock) -> PrimShapeParams {
    PrimShapeParams {
        path_curve: block.path_curve,
        profile_curve: block.profile_curve,
        path_begin: block.path_begin,
        path_end: block.path_end,
        path_scale_x: block.path_scale_x,
        path_scale_y: block.path_scale_y,
        path_shear_x: block.path_shear_x,
        path_shear_y: block.path_shear_y,
        path_twist: block.path_twist,
        path_twist_begin: block.path_twist_begin,
        path_radius_offset: block.path_radius_offset,
        path_taper_x: block.path_taper_x,
        path_taper_y: block.path_taper_y,
        path_revolutions: block.path_revolutions,
        path_skew: block.path_skew,
        profile_begin: block.profile_begin,
        profile_end: block.profile_end,
        profile_hollow: block.profile_hollow,
    }
}

/// Builds an [`ObjectProperties`] from an `ObjectProperties` object-data block.
pub(crate) fn object_properties(
    block: &ObjectPropertiesObjectDataBlock,
) -> Result<ObjectProperties, sl_wire::WireError> {
    Ok(ObjectProperties {
        object_id: ObjectKey::from(block.object_id),
        creator_id: AgentKey::from(block.creator_id),
        owner: crate::types::object_owner_from_wire(block.owner_id, block.group_id),
        group: crate::types::group_from_wire(block.group_id),
        last_owner_id: block.last_owner_id,
        creation_date: block.creation_date,
        permissions: Permissions5 {
            base: Permissions::from_bits(block.base_mask),
            owner: Permissions::from_bits(block.owner_mask),
            group: Permissions::from_bits(block.group_mask),
            everyone: Permissions::from_bits(block.everyone_mask),
            next_owner: Permissions::from_bits(block.next_owner_mask),
        },
        ownership_cost: crate::types::linden_from_wire("OwnershipCost", block.ownership_cost)?,
        sale_type: block.sale_type,
        sale_price: crate::types::linden_price_from_wire(
            block.sale_type != 0,
            "SalePrice",
            block.sale_price,
        )?,
        category: block.category,
        inventory_serial: block.inventory_serial,
        item_id: InventoryKey::from(block.item_id),
        folder_id: crate::types::optional_key_from_wire(block.folder_id),
        from_task_id: crate::types::optional_key_from_wire(block.from_task_id),
        aggregate_perms: block.aggregate_perms,
        aggregate_perm_textures: block.aggregate_perm_textures,
        aggregate_perm_textures_owner: block.aggregate_perm_textures_owner,
        name: trimmed_string(&block.name),
        description: trimmed_string(&block.description),
        touch_name: trimmed_string(&block.touch_name),
        sit_name: trimmed_string(&block.sit_name),
        texture_ids: concatenated_uuids(&block.texture_id)
            .into_iter()
            .map(TextureKey::from)
            .collect(),
    })
}

/// Splits a wire blob of back-to-back 16-byte UUIDs into a vector of ids,
/// ignoring any trailing bytes that do not form a complete UUID.
pub(crate) fn concatenated_uuids(bytes: &[u8]) -> Vec<Uuid> {
    bytes
        .chunks_exact(16)
        .filter_map(|chunk| Uuid::from_slice(chunk).ok())
        .collect()
}

#[cfg(test)]
mod caps_serializer_tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use sl_types::key::GroupKey;
    use sl_types::key::InventoryFolderKey;
    use sl_types::key::InventoryKey;
    use sl_types::key::TextureKey;
    use sl_types::map::RegionCoordinates;
    use sl_types::money::LindenAmount;
    use uuid::Uuid;

    use crate::types::LandArea;
    use sl_wire::Direction;

    use super::{
        CapsTeleportFinish, ais_inventory_update_from_llsd, ais_inventory_update_to_llsd,
        bulk_update_inventory_from_llsd, bulk_update_inventory_to_llsd,
        chatterbox_invitation_from_llsd, chatterbox_invitation_to_llsd, created_category_from_llsd,
        created_category_to_llsd, crossed_region_from_caps_llsd, crossed_region_to_caps_llsd,
        enable_simulator_from_caps_llsd, enable_simulator_to_caps_llsd,
        establish_agent_communication_from_llsd, establish_agent_communication_to_llsd,
        group_members_from_caps_llsd, group_members_to_caps_llsd, group_memberships_from_caps_llsd,
        group_memberships_to_caps_llsd, inventory_descendents_from_llsd,
        inventory_descendents_to_llsd, offline_messages_from_llsd, offline_messages_to_llsd,
        parcel_info_from_llsd, parcel_info_to_llsd, server_appearance_update_from_llsd,
        server_appearance_update_to_llsd, teleport_finish_from_llsd, teleport_finish_to_llsd,
    };
    use crate::types::{
        Event, GroupMember, GroupMembership, ImDialog, InstantMessage, InventoryFolder,
        InventoryItem, LandingType, ParcelCategory, ParcelInfo, ParcelRequestResult, ParcelStatus,
    };
    use sl_wire::{Permissions, Permissions5};

    /// A V4 socket address for the given octets and port.
    fn addr(a: u8, b: u8, c: u8, d: u8, port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a, b, c, d)), port)
    }

    /// The strict text-field parsers reject a present-but-malformed value with a
    /// hard `WireError` instead of silently coercing it to nil/`0`, while an
    /// empty optional value is the documented "absent" sentinel (`None`).
    #[test]
    fn strict_field_parsers_reject_malformed_values() {
        let good = "11111111-1111-1111-1111-111111111111";
        // A valid id parses; rendering it back matches the input verbatim.
        assert_eq!(
            super::parse_uuid_field("f", good).map(|id| id.to_string()),
            Ok(good.to_owned())
        );
        assert!(matches!(
            super::parse_uuid_field("f", "not-a-uuid"),
            Err(sl_wire::WireError::InvalidUuid { .. })
        ));
        // Empty optional id is the "absent" sentinel; a present bad one rejects.
        assert_eq!(super::parse_optional_uuid_field("f", "   "), Ok(None));
        assert!(matches!(
            super::parse_optional_uuid_field("f", "bad"),
            Err(sl_wire::WireError::InvalidUuid { .. })
        ));
        assert_eq!(super::parse_u32_field("f", "42"), Ok(42));
        assert!(matches!(
            super::parse_u32_field("f", "-1"),
            Err(sl_wire::WireError::MalformedField { .. })
        ));
    }

    /// A mute-list line with an unparsable UUID or flags is a hard error rather
    /// than a silent nil-mute / zero-flags; a blank line is skipped (`Ok(None)`).
    #[test]
    fn parse_mute_line_rejects_malformed_lines() {
        // A well-formed line yields an entry with the parsed name and flags.
        assert!(matches!(
            super::parse_mute_line("1 11111111-1111-1111-1111-111111111111 Bob|5"),
            Ok(Some(ref entry)) if entry.name == "Bob" && entry.flags.0 == 5
        ));
        assert_eq!(super::parse_mute_line("   "), Ok(None));
        assert!(matches!(
            super::parse_mute_line("1 not-a-uuid Bob|5"),
            Err(sl_wire::WireError::InvalidUuid { .. })
        ));
        assert!(matches!(
            super::parse_mute_line("1 11111111-1111-1111-1111-111111111111 Bob|notflags"),
            Err(sl_wire::WireError::MalformedField { .. })
        ));
    }

    #[test]
    fn teleport_finish_round_trips() -> Result<(), url::ParseError> {
        let dest = addr(192, 168, 7, 9, 13_001);
        let llsd = teleport_finish_to_llsd(dest, "https://seed/tp", 21, 0x8000_00ff);
        assert_eq!(
            teleport_finish_from_llsd(&llsd),
            Some(CapsTeleportFinish {
                dest,
                seed: "https://seed/tp".parse()?,
                sim_access: 21,
                teleport_flags: 0x8000_00ff,
            })
        );
        Ok(())
    }

    #[test]
    fn enable_simulator_round_trips() {
        let sim = addr(10, 0, 0, 5, 9000);
        let handle = 0x0003_e800_0003_e800;
        let llsd = enable_simulator_to_caps_llsd(handle, sim);
        assert_eq!(enable_simulator_from_caps_llsd(&llsd), Some((handle, sim)));
    }

    #[test]
    fn crossed_region_round_trips() -> Result<(), url::ParseError> {
        let dest = addr(10, 0, 0, 6, 9001);
        let handle = 0x0003_ec00_0003_e800;
        let llsd = crossed_region_to_caps_llsd(handle, dest, "https://seed/x");
        assert_eq!(
            crossed_region_from_caps_llsd(&llsd),
            Some((handle, dest, "https://seed/x".parse()?))
        );
        Ok(())
    }

    #[test]
    fn establish_agent_communication_round_trips() -> Result<(), url::ParseError> {
        let sim = addr(10, 0, 0, 7, 9002);
        let llsd = establish_agent_communication_to_llsd(sim, "https://seed/eac");
        assert_eq!(
            establish_agent_communication_from_llsd(&llsd),
            Some((sim, "https://seed/eac".parse()?))
        );
        Ok(())
    }

    #[test]
    fn server_appearance_update_round_trips() {
        let with_error = Event::ServerAppearanceUpdate {
            success: false,
            error: Some("stale COF".to_owned()),
            expected_cof_version: Some(7),
        };
        assert_eq!(
            server_appearance_update_from_llsd(&server_appearance_update_to_llsd(&with_error)),
            with_error
        );
        let ok = Event::ServerAppearanceUpdate {
            success: true,
            error: None,
            expected_cof_version: None,
        };
        assert_eq!(
            server_appearance_update_from_llsd(&server_appearance_update_to_llsd(&ok)),
            ok
        );
    }

    #[test]
    fn parcel_info_round_trips() -> Result<(), sl_wire::WireError> {
        let info = ParcelInfo {
            sequence_id: 7,
            request_result: ParcelRequestResult::Multiple,
            snap_selection: true,
            self_count: 1,
            other_count: 2,
            public_count: 3,
            local_id: sl_wire::RegionLocalParcelId(42),
            owner: sl_types::key::OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0x11))),
            group: Some(GroupKey::from(Uuid::from_u128(0x22))),
            auction_id: 0xdead_beef,
            claim_date: 1_700_000_000,
            claim_price: LindenAmount(100),
            rent_price: LindenAmount(5),
            aabb_min: RegionCoordinates::new(1.0, 2.0, 3.0),
            aabb_max: RegionCoordinates::new(4.0, 5.0, 6.0),
            area: LandArea(1024),
            bitmap: vec![1, 2, 3, 4],
            status: ParcelStatus::Abandoned,
            category: ParcelCategory::Commercial,
            max_prims: 500,
            sim_wide_max_prims: 1000,
            sim_wide_total_prims: 800,
            total_prims: 50,
            owner_prims: 30,
            group_prims: 10,
            other_prims: 10,
            selected_prims: 2,
            parcel_prim_bonus: 1.5,
            other_clean_time: 60,
            // 0x04 is FOR_SALE, so the sale price survives the round trip.
            raw_parcel_flags: 0x8000_0005,
            sale_price: Some(LindenAmount(999)),
            name: "Test Parcel".to_owned(),
            description: "A description".to_owned(),
            music_url: Some(sl_wire::url_from_wire("MusicURL", "http://music")?),
            media_url: Some(sl_wire::url_from_wire("MediaURL", "http://media")?),
            media_id: Some(TextureKey::from(Uuid::from_u128(0x33))),
            media_auto_scale: true,
            auth_buyer_id: Some(AgentKey::from(Uuid::from_u128(0x44))),
            snapshot_id: Some(TextureKey::from(Uuid::from_u128(0x55))),
            pass_price: LindenAmount(25),
            pass_hours: 2.0,
            user_location: RegionCoordinates::new(10.0, 20.0, 30.0),
            user_look_at: Direction::new(0.0, 1.0, 0.0),
            landing_type: LandingType::LandingPoint,
            region_push_override: true,
            region_deny_anonymous: false,
            region_deny_identified: true,
            region_deny_transacted: false,
            region_deny_age_unverified: true,
            region_allow_access_override: true,
            parcel_environment_version: 3,
            region_allow_environment_override: false,
            see_avs: Some(true),
            any_av_sounds: Some(false),
            group_av_sounds: Some(true),
        };
        assert_eq!(
            parcel_info_from_llsd(&parcel_info_to_llsd(&info)?),
            Some(info)
        );
        Ok(())
    }

    #[test]
    fn offline_messages_round_trip() {
        let messages = vec![InstantMessage {
            from_agent_id: AgentKey::from(Uuid::from_u128(0xa1)),
            from_agent_name: "Sender Resident".to_owned(),
            to_agent_id: AgentKey::from(Uuid::from_u128(0xa2)),
            dialog: ImDialog::FromTask,
            from_group: false,
            region_id: Some(Uuid::from_u128(0xa3)),
            position: RegionCoordinates::new(128.0, 64.0, 32.0),
            offline: true,
            timestamp: Some(1_700_000_500),
            id: Uuid::from_u128(0xa4),
            parent_estate_id: 1,
            message: "stored while offline".to_owned(),
            binary_bucket: vec![9, 8, 7],
        }];
        assert_eq!(
            offline_messages_from_llsd(&offline_messages_to_llsd(&messages)),
            messages
        );
    }

    #[test]
    fn chatterbox_invitation_round_trips() {
        let event = Event::ConferenceInvited {
            session_id: Uuid::from_u128(0xb1),
            from_agent_id: AgentKey::from(Uuid::from_u128(0xb2)),
            from_name: "Inviter Resident".to_owned(),
            dialog: ImDialog::SessionGroupStart,
            from_group: true,
            session_name: "The Group".to_owned(),
            message: "join us".to_owned(),
            region_id: Uuid::from_u128(0xb3),
            position: RegionCoordinates::new(12.0, 34.0, 56.0),
            parent_estate_id: 2,
            timestamp: Some(1_700_001_000),
            binary_bucket: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(
            chatterbox_invitation_from_llsd(&chatterbox_invitation_to_llsd(&event)),
            Some(event)
        );
    }

    #[test]
    fn group_memberships_round_trip() -> Result<(), sl_wire::WireError> {
        let event = Event::GroupMemberships(vec![GroupMembership {
            group_id: GroupKey::from(Uuid::from_u128(0xc1)),
            group_powers: 0x0000_0001_0000_00ff,
            accept_notices: true,
            group_insignia_id: TextureKey::from(Uuid::from_u128(0xc2)),
            contribution: LandArea(128),
            group_name: "Test Group".to_owned(),
        }]);
        assert_eq!(
            group_memberships_from_caps_llsd(&group_memberships_to_caps_llsd(&event)?),
            Some(event)
        );
        Ok(())
    }

    #[test]
    fn group_members_round_trip() -> Result<(), sl_wire::WireError> {
        // Members already sorted by agent id, request id nil, count == roster
        // length — the shape the parser reconstructs.
        let event = Event::GroupMembers {
            group_id: GroupKey::from(Uuid::from_u128(0xd0)),
            request_id: Uuid::nil(),
            member_count: 2,
            members: vec![
                GroupMember {
                    agent_id: AgentKey::from(Uuid::from_u128(0xd1)),
                    contribution: LandArea(10),
                    online_status: "Online".to_owned(),
                    agent_powers: 0x0000_0002_0000_0000,
                    title: "Owner".to_owned(),
                    is_owner: true,
                },
                GroupMember {
                    agent_id: AgentKey::from(Uuid::from_u128(0xd2)),
                    contribution: LandArea(0),
                    online_status: "Offline".to_owned(),
                    agent_powers: 7,
                    title: "Member".to_owned(),
                    is_owner: false,
                },
            ],
        };
        assert_eq!(
            group_members_from_caps_llsd(&group_members_to_caps_llsd(&event)?),
            Some(event)
        );
        Ok(())
    }

    /// A fully-populated inventory item in the AIS/CAPS shape (nested
    /// permissions + sale info), used by the descendents and AIS round-trips.
    fn sample_item(seed: u128) -> InventoryItem {
        InventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(seed)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(seed.wrapping_add(0x100))),
            name: "An Item".to_owned(),
            description: "desc".to_owned(),
            asset_id: Uuid::from_u128(seed.wrapping_add(0x200)),
            item_type: 6,
            inv_type: 6,
            flags: 0x8000_0001,
            sale_type: 2,
            sale_price: Some(LindenAmount(50)),
            creation_date: 1_700_002_000,
            owner: sl_types::key::OwnerKey::Agent(AgentKey::from(Uuid::from_u128(
                seed.wrapping_add(0x300),
            ))),
            last_owner_id: Uuid::from_u128(seed.wrapping_add(0x400)),
            creator_id: AgentKey::from(Uuid::from_u128(seed.wrapping_add(0x500))),
            group: Some(GroupKey::from(Uuid::from_u128(seed.wrapping_add(0x600)))),
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7fff_ffff),
                owner: Permissions::from_bits(0x0008_0000),
                group: Permissions::NONE,
                everyone: Permissions::from_bits(0x0002_0000),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
        }
    }

    #[test]
    fn inventory_descendents_round_trip() -> Result<(), sl_wire::WireError> {
        let events = vec![Event::InventoryDescendents {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0xe0)),
            version: 4,
            descendents: 2,
            folders: vec![InventoryFolder {
                folder_id: InventoryFolderKey::from(Uuid::from_u128(0xe1)),
                parent_id: crate::types::optional_key_from_wire(Uuid::from_u128(0xe0)),
                name: "Sub".to_owned(),
                folder_type: -1,
                version: 3,
            }],
            items: vec![sample_item(0xe2)],
        }];
        assert_eq!(
            inventory_descendents_from_llsd(&inventory_descendents_to_llsd(&events)?),
            events
        );
        Ok(())
    }

    #[test]
    fn bulk_update_inventory_round_trip() -> Result<(), sl_wire::WireError> {
        let transaction_id = Uuid::from_u128(0xf0);
        // The bulk wire form carries no folder version (parser defaults 0) and no
        // last-owner id (parser defaults nil).
        let folders = vec![InventoryFolder {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0xf1)),
            parent_id: crate::types::optional_key_from_wire(Uuid::from_u128(0xf2)),
            name: "Folder".to_owned(),
            folder_type: -1,
            version: 0,
        }];
        let mut item = sample_item(0xf3);
        item.last_owner_id = Uuid::nil();
        let items = vec![item];
        assert_eq!(
            bulk_update_inventory_from_llsd(&bulk_update_inventory_to_llsd(
                transaction_id,
                &folders,
                &items
            )?),
            Some((transaction_id, folders, items))
        );
        Ok(())
    }

    #[test]
    fn ais_inventory_update_round_trip() -> Result<(), sl_wire::WireError> {
        let folders = vec![InventoryFolder {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x1a1)),
            parent_id: crate::types::optional_key_from_wire(Uuid::from_u128(0x1a2)),
            name: "AIS Folder".to_owned(),
            folder_type: 8,
            version: 5,
        }];
        let items = vec![sample_item(0x1b1)];
        let (mut got_folders, mut got_items) =
            ais_inventory_update_from_llsd(&ais_inventory_update_to_llsd(&folders, &items)?);
        // The `_embedded` maps are uuid-keyed and unordered; sort for comparison.
        got_folders.sort_by_key(|folder| folder.folder_id.uuid());
        got_items.sort_by_key(|item| item.item_id.uuid());
        assert_eq!(got_folders, folders);
        assert_eq!(got_items, items);
        Ok(())
    }

    #[test]
    fn created_category_round_trips() {
        // The synchronous reply fixes version at 1.
        let folder = InventoryFolder {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x2a1)),
            parent_id: crate::types::optional_key_from_wire(Uuid::from_u128(0x2a2)),
            name: "New Category".to_owned(),
            folder_type: -1,
            version: 1,
        };
        assert_eq!(
            created_category_from_llsd(&created_category_to_llsd(&folder)),
            Some(folder)
        );
    }
}
