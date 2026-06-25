//! The command registry: one build entry per [`Command`] variant.
//!
//! [`Registry::new`] builds the table; [`Registry::build`] looks an entry up by
//! name and runs its build function against the parsed [`Args`] and a
//! [`ReplContext`]. Each build function is a non-capturing closure, so the table
//! is a plain data structure with no per-command boilerplate types.
//!
//! Conventions for the grammar:
//!
//! - Scalars parse as you would write them (`42`, `1.5`, `true`); UUIDs are the
//!   usual hyphenated form; vectors/rotations use LSL `<x,y,z>` / `<x,y,z,s>`.
//! - Bytes are hex (`deadbeef`). Lists are comma-separated; records inside a
//!   list use `:` between fields (`role:member:add`).
//! - Enums accept their lowercase name (underscores optional) and/or their
//!   numeric wire code (e.g. `texture`, `lsl_text`, or `0` for an asset type).
//! - Optional struct fields are set by `key=value`; missing ones take a default.

use std::collections::BTreeMap;

use sl_proto::AgentKey;
use sl_proto::AnimationKey;
use sl_proto::AssetKey;
use sl_proto::ChatChannel;
use sl_proto::CircuitId;
use sl_proto::ClassifiedKey;
use sl_proto::Direction;
use sl_proto::Distance;
use sl_proto::EventId;
use sl_proto::ExperienceKey;
use sl_proto::FriendKey;
use sl_proto::GlobalCoordinates;
use sl_proto::GroupKey;
use sl_proto::GroupRequestId;
use sl_proto::GroupRoleKey;
use sl_proto::ImSessionId;
use sl_proto::InventoryFolderKey;
use sl_proto::InventoryItemOrFolderKey;
use sl_proto::InventoryKey;
use sl_proto::LureId;
use sl_proto::MeshKey;
use sl_proto::ObjectKey;
use sl_proto::OwnerKey;
use sl_proto::ParcelKey;
use sl_proto::QueryId;
use sl_proto::RegionCoordinates;
use sl_proto::TextureKey;
use sl_proto::TransactionId;
use sl_proto::{
    AbuseReport, AbuseReportType, AgentPreferences, AssetType, AttachmentMode, AttachmentPoint,
    Camera, ChatType, ClassifiedCategory, ClassifiedUpdate, Command, ControlFlags,
    CreateGroupParams, DeRezDestination, DetachOrder, DirFindFlags, EstateAccessDelta,
    ExperiencePermission, ExperienceUpdate, FriendRights, GestureActivation, GroupNoticeAttachment,
    GroupNoticeKey, GroupRoleChange, GroupRoleEdit, GroupRoleMemberChange, InterestsUpdate,
    InventoryItem, InventoryOffer, InventoryType, LandSearchType, LandStatReportType, LindenAmount,
    LookAtType, MapItemType, Material, MaterialOverrideUpdate, Maturity, MediaEntry,
    MoneyTransactionType, MovementMode, MuteFlags, MuteType, NewInventoryItem, NotecardRez,
    ObjectBuyItem, ObjectFlagSettings, ObjectPermMasks, ObjectTransform, ParcelAccessEntry,
    ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelFlags, ParcelReturnType,
    ParcelUpdate, PermissionField, Permissions, Permissions5, PickKey, PickUpdate, PointAtType,
    Postcard, PrimShape, ProfileUpdate, ProposalVoteId, RegionHandle, RegionInfoUpdate,
    RegionLocalObjectId, RegionLocalParcelId, RestoreItem, RezAttachment, Rotation, SaleType,
    ScopedObjectId, ScopedParcelId, ScriptPermissions, Throttle, Uuid, Vector, ViewerEffect,
    ViewerEffectData, ViewerEffectType, VoiceProvisionRequest, Wearable, WearableType,
};

use crate::args::{self, Args};
use crate::context::ReplContext;
use crate::error::ReplError;

/// The current root circuit, for scoping a freshly typed region-local id into
/// the [`ScopedObjectId`] / [`ScopedParcelId`] the object/parcel commands now
/// take. Errors with [`ReplError::Unresolved`] when no circuit is established
/// yet (so a command issued before login is rejected clearly).
fn current_circuit(ctx: &dyn ReplContext) -> Result<CircuitId, ReplError> {
    ctx.current_circuit_id()
        .ok_or_else(|| ReplError::Unresolved("circuitid".to_owned()))
}

/// Scopes a raw region-local object id to the current circuit.
fn scoped_object(ctx: &dyn ReplContext, id: u32) -> Result<ScopedObjectId, ReplError> {
    Ok(ScopedObjectId::new(
        current_circuit(ctx)?,
        RegionLocalObjectId(id),
    ))
}

/// Scopes a raw region-local parcel id to the current circuit.
fn scoped_parcel(ctx: &dyn ReplContext, id: i32) -> Result<ScopedParcelId, ReplError> {
    Ok(ScopedParcelId::new(
        current_circuit(ctx)?,
        RegionLocalParcelId(id),
    ))
}

/// Scopes a batch of raw region-local object ids to the current circuit.
fn scoped_objects(ctx: &dyn ReplContext, ids: Vec<u32>) -> Result<Vec<ScopedObjectId>, ReplError> {
    let circuit = current_circuit(ctx)?;
    Ok(ids
        .into_iter()
        .map(|id| ScopedObjectId::new(circuit, RegionLocalObjectId(id)))
        .collect())
}
use crate::parse::PendingCommand;

/// The signature of a registry build function: parsed arguments plus a
/// resolution context in, a [`Command`] (or an error) out.
pub type BuildFn = fn(&Args, &dyn ReplContext) -> Result<Command, ReplError>;

/// One registry entry: a command name, a one-line usage hint, and the build
/// function that constructs the [`Command`].
#[derive(Clone, Copy)]
pub struct CommandSpec {
    /// The command name used on a REPL line.
    pub name: &'static str,
    /// A short usage hint (argument names, in order).
    pub usage: &'static str,
    /// The build function turning [`Args`] into a [`Command`].
    pub build: BuildFn,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec")
            .field("name", &self.name)
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}

/// The command registry: a name-indexed table of [`CommandSpec`]s.
#[derive(Debug, Clone)]
pub struct Registry {
    /// The specs, in registration order.
    specs: Vec<CommandSpec>,
    /// A name → index lookup into `specs`.
    by_name: BTreeMap<&'static str, usize>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Build the registry with every command entry.
    #[must_use]
    pub fn new() -> Self {
        let specs = all_specs();
        let mut by_name = BTreeMap::new();
        for (index, spec) in specs.iter().enumerate() {
            let _: Option<usize> = by_name.insert(spec.name, index);
        }
        Self { specs, by_name }
    }

    /// All registered specs, in registration order.
    #[must_use]
    pub fn specs(&self) -> &[CommandSpec] {
        &self.specs
    }

    /// Look a spec up by command name.
    #[must_use]
    pub fn spec(&self, name: &str) -> Option<&CommandSpec> {
        self.by_name
            .get(name)
            .and_then(|index| self.specs.get(*index))
    }

    /// Build the [`Command`] named by a [`PendingCommand`], resolving its
    /// arguments against `ctx`.
    ///
    /// # Errors
    ///
    /// Returns [`ReplError::UnknownCommand`] if the name is not registered, or
    /// whatever the command's build function returns when an argument is
    /// missing, malformed, or an unresolvable placeholder.
    pub fn build(
        &self,
        pending: &PendingCommand,
        ctx: &dyn ReplContext,
    ) -> Result<Command, ReplError> {
        let spec = self
            .spec(&pending.name)
            .ok_or_else(|| ReplError::UnknownCommand(pending.name.clone()))?;
        let args = pending.args.clone().with_command(spec.name);
        (spec.build)(&args, ctx)
    }
}

/// Build a [`ReplError::InvalidArg`].
fn invalid(field: &str, value: &str, expected: &str) -> ReplError {
    ReplError::InvalidArg {
        field: field.to_owned(),
        value: value.to_owned(),
        expected: expected.to_owned(),
    }
}

/// Normalise an enum name for matching: lowercase, with `_` removed.
fn norm(value: &str) -> String {
    value.to_ascii_lowercase().replace('_', "")
}

/// Resolve a required argument and parse it with `f`.
fn enum_arg<T>(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
    f: fn(&str, &str) -> Result<T, ReplError>,
) -> Result<T, ReplError> {
    let value = args.req_str(ctx, field, pos)?;
    f(field, &value)
}

/// Parse an enum argument like [`enum_arg`], falling back to `default` when the
/// argument is absent (an optional positional or keyword field).
fn enum_arg_or<T>(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
    f: fn(&str, &str) -> Result<T, ReplError>,
    default: T,
) -> Result<T, ReplError> {
    match args.opt_str(ctx, field, pos)? {
        Some(value) => f(field, &value),
        None => Ok(default),
    }
}

/// The `idx`-th colon-field of a list record, or an error.
fn record_field<'a>(field: &str, record: &'a [String], idx: usize) -> Result<&'a str, ReplError> {
    record
        .get(idx)
        .map(String::as_str)
        .ok_or_else(|| invalid(field, &record.join(":"), "more colon-separated fields"))
}

/// Parse a `<x,y,z>` triple of `f64`s (a global position).
fn parse_global(field: &str, value: &str) -> Result<(f64, f64, f64), ReplError> {
    let inner = value
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .ok_or_else(|| invalid(field, value, "global <x,y,z>"))?;
    let mut parts = inner.split(',');
    let mut next = || -> Result<f64, ReplError> {
        parts
            .next()
            .and_then(|p| p.trim().parse::<f64>().ok())
            .ok_or_else(|| invalid(field, value, "global <x,y,z>"))
    };
    let x = next()?;
    let y = next()?;
    let z = next()?;
    if parts.next().is_some() {
        return Err(invalid(field, value, "global <x,y,z>"));
    }
    Ok((x, y, z))
}

/// An optional global-position triple from `field`, defaulting to the origin.
fn global_or_zero(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
) -> Result<GlobalCoordinates, ReplError> {
    match args.opt_str(ctx, field, pos)? {
        Some(value) => {
            let (x, y, z) = parse_global(field, &value)?;
            Ok(GlobalCoordinates::new(x, y, z))
        }
        None => Ok(GlobalCoordinates::new(0.0, 0.0, 0.0)),
    }
}

// ---- enum parsers -------------------------------------------------------

/// Parse a [`ChatType`] from its name or wire byte.
fn parse_chat_type(field: &str, value: &str) -> Result<ChatType, ReplError> {
    Ok(match norm(value).as_str() {
        "whisper" => ChatType::Whisper,
        "normal" | "say" => ChatType::Normal,
        "shout" => ChatType::Shout,
        "starttyping" => ChatType::StartTyping,
        "stoptyping" => ChatType::StopTyping,
        "debug" | "debugchannel" => ChatType::DebugChannel,
        "region" => ChatType::Region,
        "owner" => ChatType::Owner,
        "direct" => ChatType::Direct,
        _ => ChatType::from_u8(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "chat type"))?,
        ),
    })
}

/// Parse a [`MoneyTransactionType`] from its name or wire code.
fn parse_money_tx_type(field: &str, value: &str) -> Result<MoneyTransactionType, ReplError> {
    Ok(match norm(value).as_str() {
        "gift" => MoneyTransactionType::Gift,
        "payobject" | "pay" => MoneyTransactionType::PayObject,
        "objectsale" | "sale" => MoneyTransactionType::ObjectSale,
        _ => MoneyTransactionType::from_i32(
            value
                .parse::<i32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "transaction type"))?,
        ),
    })
}

/// Parse a [`MapItemType`] from its name or wire code.
fn parse_map_item_type(field: &str, value: &str) -> Result<MapItemType, ReplError> {
    Ok(match norm(value).as_str() {
        "telehub" => MapItemType::Telehub,
        "pgevent" => MapItemType::PgEvent,
        "matureevent" => MapItemType::MatureEvent,
        "agentlocations" | "agents" => MapItemType::AgentLocations,
        "landforsale" => MapItemType::LandForSale,
        "classified" => MapItemType::Classified,
        "adultevent" => MapItemType::AdultEvent,
        "adultlandforsale" => MapItemType::AdultLandForSale,
        _ => MapItemType::from_u32(
            value
                .parse::<u32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "map item type"))?,
        ),
    })
}

/// Parses an [`AbuseReportType`] from `bug`/`complaint` (or a raw byte).
fn parse_abuse_report_type(field: &str, value: &str) -> Result<AbuseReportType, ReplError> {
    Ok(match norm(value).as_str() {
        "bug" => AbuseReportType::Bug,
        "complaint" | "abuse" => AbuseReportType::Complaint,
        _ => AbuseReportType::from_u8(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "abuse report type"))?,
        ),
    })
}

/// Builds an [`AbuseReport`] from the shared `send_abuse_report` /
/// `send_abuse_report_caps` argument list, so both the UDP and capability
/// commands parse identically.
fn abuse_report_from_args(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<Box<AbuseReport>, ReplError> {
    Ok(Box::new(AbuseReport {
        report_type: enum_arg_or(
            args,
            ctx,
            "report_type",
            4,
            parse_abuse_report_type,
            AbuseReportType::Complaint,
        )?,
        category: args.parse_or(ctx, "category", 3, "u8", 0)?,
        position: args.opt_vector(ctx, "position", 6)?.unwrap_or(Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        check_flags: 0,
        screenshot_id: Uuid::nil(),
        object_id: args.object_or_nil(ctx, "object_id", 5)?,
        abuser_id: args.uuid_or_nil(ctx, "abuser_id", 1)?,
        abuse_region_name: {
            let raw = args.opt_str(ctx, "region_name", 2)?.unwrap_or_default();
            sl_proto::region_name_from_wire("region_name", &raw).map_err(|err| {
                ReplError::InvalidArg {
                    field: "region_name".to_owned(),
                    value: err.to_string(),
                    expected: "a valid region name (2-35 chars)".to_owned(),
                }
            })?
        },
        abuse_region_id: Uuid::nil(),
        summary: args.req_str(ctx, "summary", 0)?,
        details: String::new(),
        version_string: String::new(),
    }))
}

/// Parse a [`ParcelAccessScope`] from `access`/`ban`.
fn parse_parcel_access_scope(field: &str, value: &str) -> Result<ParcelAccessScope, ReplError> {
    match norm(value).as_str() {
        "access" | "allow" | "1" => Ok(ParcelAccessScope::Access),
        "ban" | "banned" | "2" => Ok(ParcelAccessScope::Ban),
        _ => Err(invalid(field, value, "access scope")),
    }
}

/// Parse an [`EstateAccessDelta`] from its name.
fn parse_estate_access_delta(field: &str, value: &str) -> Result<EstateAccessDelta, ReplError> {
    Ok(match norm(value).as_str() {
        "allowedagentadd" => EstateAccessDelta::AllowedAgentAdd,
        "allowedagentremove" => EstateAccessDelta::AllowedAgentRemove,
        "allowedgroupadd" => EstateAccessDelta::AllowedGroupAdd,
        "allowedgroupremove" => EstateAccessDelta::AllowedGroupRemove,
        "bannedagentadd" => EstateAccessDelta::BannedAgentAdd,
        "bannedagentremove" => EstateAccessDelta::BannedAgentRemove,
        "manageradd" => EstateAccessDelta::ManagerAdd,
        "managerremove" => EstateAccessDelta::ManagerRemove,
        _ => return Err(invalid(field, value, "estate access delta")),
    })
}

/// Parse a [`MuteType`] from its name or wire code.
fn parse_mute_type(field: &str, value: &str) -> Result<MuteType, ReplError> {
    Ok(match norm(value).as_str() {
        "byname" => MuteType::ByName,
        "agent" => MuteType::Agent,
        "object" => MuteType::Object,
        "group" => MuteType::Group,
        "external" => MuteType::External,
        _ => MuteType::from_i32(
            value
                .parse::<i32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "mute type"))?,
        ),
    })
}

/// Parse a [`DeRezDestination`] from its name and the destination id `id` (a
/// folder, item, or task id, depending on the destination; ignored by the
/// destinations that take no id).
fn parse_derez_destination(
    field: &str,
    value: &str,
    id: Uuid,
) -> Result<DeRezDestination, ReplError> {
    Ok(match norm(value).as_str() {
        "saveintoagentinventory" => {
            DeRezDestination::SaveIntoAgentInventory(InventoryKey::from(id))
        }
        "acquiretoagentinventory" => {
            DeRezDestination::AcquireToAgentInventory(InventoryFolderKey::from(id))
        }
        "saveintotaskinventory" => DeRezDestination::SaveIntoTaskInventory(ObjectKey::from(id)),
        "attachment" => DeRezDestination::Attachment,
        "takeintoagentinventory" | "take" => {
            DeRezDestination::TakeIntoAgentInventory(InventoryFolderKey::from(id))
        }
        "forcetogodinventory" => {
            DeRezDestination::ForceToGodInventory(InventoryFolderKey::from(id))
        }
        "trash" => DeRezDestination::Trash(InventoryFolderKey::from(id)),
        "attachmenttoinventory" => DeRezDestination::AttachmentToInventory,
        "attachmentexists" => DeRezDestination::AttachmentExists,
        "returntoowner" | "return" => DeRezDestination::ReturnToOwner,
        "returntolastowner" => DeRezDestination::ReturnToLastOwner,
        _ => return Err(invalid(field, value, "derez destination")),
    })
}

/// Parse a [`PermissionField`] from its name.
fn parse_permission_field(field: &str, value: &str) -> Result<PermissionField, ReplError> {
    Ok(match norm(value).as_str() {
        "base" => PermissionField::Base,
        "owner" => PermissionField::Owner,
        "group" => PermissionField::Group,
        "everyone" => PermissionField::Everyone,
        "nextowner" => PermissionField::NextOwner,
        _ => return Err(invalid(field, value, "permission field")),
    })
}

/// Parse a [`Material`] from its name or wire code.
fn parse_material(field: &str, value: &str) -> Result<Material, ReplError> {
    Ok(match norm(value).as_str() {
        "stone" => Material::Stone,
        "metal" => Material::Metal,
        "glass" => Material::Glass,
        "wood" => Material::Wood,
        "flesh" => Material::Flesh,
        "plastic" => Material::Plastic,
        "rubber" => Material::Rubber,
        "light" => Material::Light,
        _ => Material::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "material"))?,
        ),
    })
}

/// Parse a [`SaleType`] from its name or wire code.
fn parse_sale_type(field: &str, value: &str) -> Result<SaleType, ReplError> {
    Ok(match norm(value).as_str() {
        "notforsale" | "none" => SaleType::NotForSale,
        "original" => SaleType::Original,
        "copy" => SaleType::Copy,
        "contents" => SaleType::Contents,
        _ => SaleType::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "sale type"))?,
        ),
    })
}

/// Parse a [`LandStatReportType`] from its name or wire code (`0` = scripts,
/// `1` = colliders).
fn parse_land_stat_report_type(field: &str, value: &str) -> Result<LandStatReportType, ReplError> {
    Ok(match norm(value).as_str() {
        "scripts" | "topscripts" | "0" => LandStatReportType::TopScripts,
        "colliders" | "topcolliders" | "1" => LandStatReportType::TopColliders,
        _ => LandStatReportType::from_u32(
            value
                .parse::<u32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "land stat report type"))?,
        ),
    })
}

/// Parse an [`AssetType`] from its name or wire code.
fn parse_asset_type(field: &str, value: &str) -> Result<AssetType, ReplError> {
    Ok(match norm(value).as_str() {
        "texture" => AssetType::Texture,
        "sound" => AssetType::Sound,
        "callingcard" => AssetType::CallingCard,
        "landmark" => AssetType::Landmark,
        "clothing" => AssetType::Clothing,
        "object" => AssetType::Object,
        "notecard" => AssetType::Notecard,
        "lsltext" | "lsl" | "script" => AssetType::LslText,
        "lslbytecode" | "bytecode" => AssetType::LslBytecode,
        "texturetga" => AssetType::TextureTga,
        "bodypart" => AssetType::Bodypart,
        "soundwav" => AssetType::SoundWav,
        "imagetga" => AssetType::ImageTga,
        "imagejpeg" => AssetType::ImageJpeg,
        "animation" => AssetType::Animation,
        "gesture" => AssetType::Gesture,
        "mesh" => AssetType::Mesh,
        "settings" => AssetType::Settings,
        "material" => AssetType::Material,
        "gltf" => AssetType::Gltf,
        "gltfbin" => AssetType::GltfBin,
        "folder" | "category" => AssetType::Folder,
        _ => AssetType::from_code(
            value
                .parse::<i32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "asset type"))?,
        ),
    })
}

/// Parse an [`InventoryType`] from its name or wire code.
fn parse_inventory_type(field: &str, value: &str) -> Result<InventoryType, ReplError> {
    Ok(match norm(value).as_str() {
        "texture" => InventoryType::Texture,
        "sound" => InventoryType::Sound,
        "callingcard" => InventoryType::CallingCard,
        "landmark" => InventoryType::Landmark,
        "object" => InventoryType::Object,
        "notecard" => InventoryType::Notecard,
        "category" | "folder" => InventoryType::Category,
        "script" | "lsl" | "lsltext" => InventoryType::Script,
        "snapshot" => InventoryType::Snapshot,
        "attachment" => InventoryType::Attachment,
        "wearable" => InventoryType::Wearable,
        "animation" => InventoryType::Animation,
        "gesture" => InventoryType::Gesture,
        "mesh" => InventoryType::Mesh,
        "settings" => InventoryType::Settings,
        "material" => InventoryType::Material,
        _ => InventoryType::from_code(
            value
                .parse::<i32>()
                .ok()
                .ok_or_else(|| invalid(field, value, "inventory type"))?,
        ),
    })
}

/// Parse a [`WearableType`] from its name or wire code.
fn parse_wearable_type(field: &str, value: &str) -> Result<WearableType, ReplError> {
    Ok(match norm(value).as_str() {
        "shape" => WearableType::Shape,
        "skin" => WearableType::Skin,
        "hair" => WearableType::Hair,
        "eyes" => WearableType::Eyes,
        "shirt" => WearableType::Shirt,
        "pants" => WearableType::Pants,
        "shoes" => WearableType::Shoes,
        "socks" => WearableType::Socks,
        "jacket" => WearableType::Jacket,
        "gloves" => WearableType::Gloves,
        "undershirt" => WearableType::Undershirt,
        "underpants" => WearableType::Underpants,
        "skirt" => WearableType::Skirt,
        "alpha" => WearableType::Alpha,
        "tattoo" => WearableType::Tattoo,
        "physics" => WearableType::Physics,
        "universal" => WearableType::Universal,
        _ => WearableType::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "wearable type"))?,
        ),
    })
}

/// Parse an [`AttachmentPoint`] from its name (e.g. `chest`, `lefthand`,
/// `hudtopright`, `default`) or its wire code.
fn parse_attachment_point(field: &str, value: &str) -> Result<AttachmentPoint, ReplError> {
    Ok(match norm(value).as_str() {
        "default" => AttachmentPoint::Default,
        "chest" => AttachmentPoint::Chest,
        "skull" | "head" => AttachmentPoint::Skull,
        "leftshoulder" => AttachmentPoint::LeftShoulder,
        "rightshoulder" => AttachmentPoint::RightShoulder,
        "lefthand" => AttachmentPoint::LeftHand,
        "righthand" => AttachmentPoint::RightHand,
        "leftfoot" => AttachmentPoint::LeftFoot,
        "rightfoot" => AttachmentPoint::RightFoot,
        "spine" | "back" => AttachmentPoint::Spine,
        "pelvis" => AttachmentPoint::Pelvis,
        "mouth" => AttachmentPoint::Mouth,
        "chin" => AttachmentPoint::Chin,
        "leftear" => AttachmentPoint::LeftEar,
        "rightear" => AttachmentPoint::RightEar,
        "lefteyeball" => AttachmentPoint::LeftEyeball,
        "righteyeball" => AttachmentPoint::RightEyeball,
        "nose" => AttachmentPoint::Nose,
        "rupperarm" => AttachmentPoint::RUpperArm,
        "rforearm" => AttachmentPoint::RForearm,
        "lupperarm" => AttachmentPoint::LUpperArm,
        "lforearm" => AttachmentPoint::LForearm,
        "righthip" => AttachmentPoint::RightHip,
        "rupperleg" => AttachmentPoint::RUpperLeg,
        "rlowerleg" => AttachmentPoint::RLowerLeg,
        "lefthip" => AttachmentPoint::LeftHip,
        "lupperleg" => AttachmentPoint::LUpperLeg,
        "llowerleg" => AttachmentPoint::LLowerLeg,
        "stomach" | "belly" => AttachmentPoint::Stomach,
        "leftpec" => AttachmentPoint::LeftPec,
        "rightpec" => AttachmentPoint::RightPec,
        "hudcenter2" => AttachmentPoint::HudCenter2,
        "hudtopright" => AttachmentPoint::HudTopRight,
        "hudtop" => AttachmentPoint::HudTop,
        "hudtopleft" => AttachmentPoint::HudTopLeft,
        "hudcenter" => AttachmentPoint::HudCenter,
        "hudbottomleft" => AttachmentPoint::HudBottomLeft,
        "hudbottom" => AttachmentPoint::HudBottom,
        "hudbottomright" => AttachmentPoint::HudBottomRight,
        "neck" => AttachmentPoint::Neck,
        "avatarcenter" | "root" => AttachmentPoint::AvatarCenter,
        "leftringfinger" => AttachmentPoint::LeftRingFinger,
        "rightringfinger" => AttachmentPoint::RightRingFinger,
        "tailbase" => AttachmentPoint::TailBase,
        "tailtip" => AttachmentPoint::TailTip,
        "leftwing" => AttachmentPoint::LeftWing,
        "rightwing" => AttachmentPoint::RightWing,
        "jaw" => AttachmentPoint::Jaw,
        "altleftear" => AttachmentPoint::AltLeftEar,
        "altrightear" => AttachmentPoint::AltRightEar,
        "altlefteye" => AttachmentPoint::AltLeftEye,
        "altrighteye" => AttachmentPoint::AltRightEye,
        "tongue" => AttachmentPoint::Tongue,
        "groin" => AttachmentPoint::Groin,
        "lefthindfoot" => AttachmentPoint::LeftHindFoot,
        "righthindfoot" => AttachmentPoint::RightHindFoot,
        _ => AttachmentPoint::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "attachment point"))?,
        ),
    })
}

/// Parse an [`AttachmentMode`] from `add`/`replace` (or the legacy boolean
/// spelling: `true`/`yes`/`1` = [`AttachmentMode::Add`],
/// `false`/`no`/`0` = [`AttachmentMode::Replace`]).
fn parse_attachment_mode(field: &str, value: &str) -> Result<AttachmentMode, ReplError> {
    Ok(match norm(value).as_str() {
        "add" | "true" | "yes" | "1" => AttachmentMode::Add,
        "replace" | "false" | "no" | "0" => AttachmentMode::Replace,
        _ => return Err(invalid(field, value, "add|replace")),
    })
}

/// Parse a [`MovementMode`] from `run`/`walk` (or the legacy boolean spelling:
/// `true`/`yes`/`1` = [`MovementMode::AlwaysRun`],
/// `false`/`no`/`0` = [`MovementMode::Walk`]).
fn parse_movement_mode(field: &str, value: &str) -> Result<MovementMode, ReplError> {
    Ok(match norm(value).as_str() {
        "run" | "alwaysrun" | "always_run" | "true" | "yes" | "1" => MovementMode::AlwaysRun,
        "walk" | "false" | "no" | "0" => MovementMode::Walk,
        _ => return Err(invalid(field, value, "run|walk")),
    })
}

/// Parse a [`DetachOrder`] from `detach`/`keep` (or the legacy boolean spelling:
/// `true`/`yes`/`1` = [`DetachOrder::DetachAllFirst`],
/// `false`/`no`/`0` = [`DetachOrder::Keep`]).
fn parse_detach_order(field: &str, value: &str) -> Result<DetachOrder, ReplError> {
    Ok(match norm(value).as_str() {
        "detach" | "detachall" | "detach_all" | "true" | "yes" | "1" => DetachOrder::DetachAllFirst,
        "keep" | "false" | "no" | "0" => DetachOrder::Keep,
        _ => return Err(invalid(field, value, "detach|keep")),
    })
}

/// Parse a [`ViewerEffectType`] from its name or wire byte.
fn parse_viewer_effect_type(field: &str, value: &str) -> Result<ViewerEffectType, ReplError> {
    Ok(match norm(value).as_str() {
        "text" => ViewerEffectType::Text,
        "icon" => ViewerEffectType::Icon,
        "connector" => ViewerEffectType::Connector,
        "flexibleobject" => ViewerEffectType::FlexibleObject,
        "animalcontrols" => ViewerEffectType::AnimalControls,
        "localanimationobject" => ViewerEffectType::LocalAnimationObject,
        "cloth" => ViewerEffectType::Cloth,
        "beam" => ViewerEffectType::Beam,
        "glow" => ViewerEffectType::Glow,
        "point" => ViewerEffectType::Point,
        "trail" => ViewerEffectType::Trail,
        "sphere" => ViewerEffectType::Sphere,
        "spiral" => ViewerEffectType::Spiral,
        "edit" => ViewerEffectType::Edit,
        "lookat" => ViewerEffectType::LookAt,
        "pointat" => ViewerEffectType::PointAt,
        "voicevisualizer" => ViewerEffectType::VoiceVisualizer,
        "nametag" => ViewerEffectType::NameTag,
        "blob" => ViewerEffectType::Blob,
        "resetskeleton" => ViewerEffectType::ResetSkeleton,
        _ => ViewerEffectType::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "viewer-effect type"))?,
        ),
    })
}

/// Parse a [`LookAtType`] from its name or wire byte.
fn parse_lookat_type(field: &str, value: &str) -> Result<LookAtType, ReplError> {
    Ok(match norm(value).as_str() {
        "none" => LookAtType::None,
        "idle" => LookAtType::Idle,
        "autolisten" => LookAtType::AutoListen,
        "freelook" => LookAtType::FreeLook,
        "respond" => LookAtType::Respond,
        "hover" => LookAtType::Hover,
        "conversation" => LookAtType::Conversation,
        "select" => LookAtType::Select,
        "focus" => LookAtType::Focus,
        "mouselook" => LookAtType::MouseLook,
        "clear" => LookAtType::Clear,
        _ => LookAtType::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "look-at type"))?,
        ),
    })
}

/// Parse a [`PointAtType`] from its name or wire byte.
fn parse_pointat_type(field: &str, value: &str) -> Result<PointAtType, ReplError> {
    Ok(match norm(value).as_str() {
        "none" => PointAtType::None,
        "select" => PointAtType::Select,
        "grab" => PointAtType::Grab,
        "clear" => PointAtType::Clear,
        _ => PointAtType::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "point-at type"))?,
        ),
    })
}

/// Parse a `[u8; 4]` `RGBA` colour from an 8-hex-digit string, defaulting to
/// opaque white when the field is absent.
fn color_or_white(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
) -> Result<[u8; 4], ReplError> {
    let Some(value) = args.opt_str(ctx, field, pos)? else {
        return Ok([255, 255, 255, 255]);
    };
    let bytes = args::parse_hex(field, &value)?;
    <[u8; 4]>::try_from(bytes.as_slice())
        .ok()
        .ok_or_else(|| invalid(field, &value, "RGBA colour (8 hex digits)"))
}

/// The default [`ViewerEffectData`] kind for an effect type: structured for the
/// look-at / point-at / spiral-family types, `raw` otherwise.
const fn default_effect_data_kind(effect_type: ViewerEffectType) -> &'static str {
    match effect_type {
        ViewerEffectType::LookAt => "lookat",
        ViewerEffectType::PointAt => "pointat",
        ViewerEffectType::Beam
        | ViewerEffectType::Glow
        | ViewerEffectType::Point
        | ViewerEffectType::Sphere
        | ViewerEffectType::Spiral
        | ViewerEffectType::Edit => "spiral",
        _ => "raw",
    }
}

/// Build the [`ViewerEffectData`] for a `viewer_effect` command: an explicit
/// `data=` selector picks the layout, otherwise it is inferred from the effect
/// type. The structured layouts read `source`/`target`/`position` (and a
/// `look_at`/`point_at` kind); `raw` reads a `raw=<hex>` blob.
fn parse_effect_data(
    args: &Args,
    ctx: &dyn ReplContext,
    effect_type: ViewerEffectType,
) -> Result<ViewerEffectData, ReplError> {
    let selector = args.str_or(ctx, "data", 200, default_effect_data_kind(effect_type))?;
    match norm(&selector).as_str() {
        "lookat" => Ok(ViewerEffectData::LookAt {
            source: args.opt_agent(ctx, "source", 201)?,
            target: args.opt_object(ctx, "target", 202)?,
            target_position: global_or_zero(args, ctx, "position", 203)?,
            look_at_type: parse_lookat_type("look_at", &args.str_or(ctx, "look_at", 204, "none")?)?,
        }),
        "pointat" => Ok(ViewerEffectData::PointAt {
            source: args.opt_agent(ctx, "source", 201)?,
            target: args.opt_object(ctx, "target", 202)?,
            target_position: global_or_zero(args, ctx, "position", 203)?,
            point_at_type: parse_pointat_type(
                "point_at",
                &args.str_or(ctx, "point_at", 205, "none")?,
            )?,
        }),
        "spiral" => Ok(ViewerEffectData::Spiral {
            source: args.opt_object(ctx, "source", 201)?,
            target: args.opt_object(ctx, "target", 202)?,
            position: global_or_zero(args, ctx, "position", 203)?,
        }),
        "raw" => Ok(ViewerEffectData::Raw(args.bytes_or_empty(ctx, "raw", 206)?)),
        _ => Err(invalid("data", &selector, "lookat|pointat|spiral|raw")),
    }
}

/// Parse an [`ExperiencePermission`] from `allow`/`block`/`forget`.
fn parse_experience_permission(
    field: &str,
    value: &str,
) -> Result<ExperiencePermission, ReplError> {
    Ok(match norm(value).as_str() {
        "allow" => ExperiencePermission::Allow,
        "block" => ExperiencePermission::Block,
        "forget" => ExperiencePermission::Forget,
        _ => return Err(invalid(field, value, "experience permission")),
    })
}

/// Parse a [`GroupRoleChange`] from `add`/`remove`.
fn parse_group_role_change(field: &str, value: &str) -> Result<GroupRoleChange, ReplError> {
    match norm(value).as_str() {
        "add" | "0" => Ok(GroupRoleChange::Add),
        "remove" | "1" => Ok(GroupRoleChange::Remove),
        _ => Err(invalid(field, value, "role change")),
    }
}

/// Parse a [`Maturity`] from `pg`/`mature`/`adult`.
fn parse_maturity(field: &str, value: &str) -> Result<Maturity, ReplError> {
    Ok(match norm(value).as_str() {
        "pg" | "general" => Maturity::Pg,
        "mature" | "moderate" => Maturity::Mature,
        "adult" => Maturity::Adult,
        _ => return Err(invalid(field, value, "maturity")),
    })
}

// ---- struct builders ----------------------------------------------------

/// Build a [`Throttle`] from seven optional kbps fields (default `0.0`).
fn build_throttle(args: &Args, ctx: &dyn ReplContext) -> Result<Throttle, ReplError> {
    let resend = args.parse_or(ctx, "resend", 0, "kbps", 0.0)?;
    let land = args.parse_or(ctx, "land", 1, "kbps", 0.0)?;
    let wind = args.parse_or(ctx, "wind", 2, "kbps", 0.0)?;
    let cloud = args.parse_or(ctx, "cloud", 3, "kbps", 0.0)?;
    let task = args.parse_or(ctx, "task", 4, "kbps", 0.0)?;
    let texture = args.parse_or(ctx, "texture", 5, "kbps", 0.0)?;
    let asset = args.parse_or(ctx, "asset", 6, "kbps", 0.0)?;
    Throttle::new(resend, land, wind, cloud, task, texture, asset).map_err(|err| {
        ReplError::InvalidArg {
            field: "resend,land,wind,cloud,task,texture,asset".to_owned(),
            value: err.to_string(),
            expected: "finite, non-negative kbps rates".to_owned(),
        }
    })
}

/// Build a [`Camera`]: `eye`+`target` keywords use [`Camera::looking_at`];
/// otherwise the four basis vectors are required.
fn build_camera(args: &Args, ctx: &dyn ReplContext) -> Result<Camera, ReplError> {
    if let (Some(eye), Some(target)) = (
        args.opt_vector(ctx, "eye", 0)?,
        args.opt_vector(ctx, "target", 1)?,
    ) {
        return Ok(Camera::looking_at(eye, target));
    }
    let center = args.req_vector(ctx, "center", 0)?;
    let at_axis = args.req_vector(ctx, "at_axis", 1)?;
    let left_axis = args.req_vector(ctx, "left_axis", 2)?;
    let up_axis = args.req_vector(ctx, "up_axis", 3)?;
    Camera::new(center, at_axis, left_axis, up_axis).map_err(|err| ReplError::InvalidArg {
        field: "at_axis,left_axis,up_axis".to_owned(),
        value: err.to_string(),
        expected: "right-handed orthonormal camera basis (at × left = up)".to_owned(),
    })
}

/// Build a [`ProfileUpdate`] from keyword fields (all optional).
fn build_profile_update(args: &Args, ctx: &dyn ReplContext) -> Result<ProfileUpdate, ReplError> {
    Ok(ProfileUpdate {
        image_id: TextureKey::from(args.uuid_or_nil(ctx, "image_id", 0)?),
        fl_image_id: TextureKey::from(args.uuid_or_nil(ctx, "fl_image_id", 1)?),
        about_text: args.str_or(ctx, "about_text", 2, "")?,
        fl_about_text: args.str_or(ctx, "fl_about_text", 3, "")?,
        allow_publish: args.bool_or(ctx, "allow_publish", 4, false)?,
        mature_publish: args.bool_or(ctx, "mature_publish", 5, false)?,
        profile_url: args.str_or(ctx, "profile_url", 6, "")?,
    })
}

/// Build an [`InterestsUpdate`] from keyword fields.
fn build_interests_update(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<InterestsUpdate, ReplError> {
    Ok(InterestsUpdate {
        want_to_mask: args.parse_or(ctx, "want_to_mask", 0, "u32", 0)?,
        want_to_text: args.str_or(ctx, "want_to_text", 1, "")?,
        skills_mask: args.parse_or(ctx, "skills_mask", 2, "u32", 0)?,
        skills_text: args.str_or(ctx, "skills_text", 3, "")?,
        languages_text: args.str_or(ctx, "languages_text", 4, "")?,
    })
}

/// Build a [`PickUpdate`] from keyword fields.
fn build_pick_update(args: &Args, ctx: &dyn ReplContext) -> Result<PickUpdate, ReplError> {
    Ok(PickUpdate {
        pick_id: PickKey::from(args.uuid_or_nil(ctx, "pick_id", 0)?),
        parcel_id: {
            // A nil/absent parcel id means "use the agent's current parcel".
            let parcel = args.uuid_or_nil(ctx, "parcel_id", 1)?;
            (!parcel.is_nil()).then(|| ParcelKey::from(parcel))
        },
        name: args.str_or(ctx, "name", 2, "")?,
        description: args.str_or(ctx, "description", 3, "")?,
        snapshot_id: args.opt_texture(ctx, "snapshot_id", 4)?,
        pos_global: global_or_zero(args, ctx, "pos_global", 5)?,
        sort_order: args.parse_or(ctx, "sort_order", 6, "i32", 0)?,
        enabled: args.bool_or(ctx, "enabled", 7, true)?,
    })
}

/// Build a [`ClassifiedUpdate`] from keyword fields.
fn build_classified_update(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<ClassifiedUpdate, ReplError> {
    Ok(ClassifiedUpdate {
        classified_id: ClassifiedKey::from(args.uuid_or_nil(ctx, "classified_id", 0)?),
        category: ClassifiedCategory::from_u32(args.parse_or(ctx, "category", 1, "u32", 0)?),
        name: args.str_or(ctx, "name", 2, "")?,
        description: args.str_or(ctx, "description", 3, "")?,
        parcel_id: {
            // A nil/absent parcel id means "use the agent's current parcel".
            let parcel = args.uuid_or_nil(ctx, "parcel_id", 4)?;
            (!parcel.is_nil()).then(|| ParcelKey::from(parcel))
        },
        snapshot_id: args.opt_texture(ctx, "snapshot_id", 5)?,
        pos_global: global_or_zero(args, ctx, "pos_global", 6)?,
        classified_flags: args.parse_or(ctx, "classified_flags", 7, "u8", 0)?,
        price_for_listing: LindenAmount(args.parse_or(ctx, "price_for_listing", 8, "u64", 0)?),
    })
}

/// Build [`CreateGroupParams`] from keyword fields (`name` required).
fn build_create_group_params(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<CreateGroupParams, ReplError> {
    Ok(CreateGroupParams {
        name: args.req_str(ctx, "name", 0)?,
        charter: args.str_or(ctx, "charter", 1, "")?,
        show_in_list: args.bool_or(ctx, "show_in_list", 2, true)?,
        insignia_id: args.opt_texture(ctx, "insignia_id", 3)?,
        membership_fee: LindenAmount(args.parse_or(ctx, "membership_fee", 4, "u64", 0)?),
        open_enrollment: args.bool_or(ctx, "open_enrollment", 5, false)?,
        allow_publish: args.bool_or(ctx, "allow_publish", 6, false)?,
        mature_publish: args.bool_or(ctx, "mature_publish", 7, false)?,
    })
}

/// Build a [`NewInventoryItem`] from keyword fields.
fn build_new_inventory_item(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<NewInventoryItem, ReplError> {
    Ok(NewInventoryItem {
        folder_id: InventoryFolderKey::from(args.uuid_or_nil(ctx, "folder_id", 0)?),
        transaction_id: args.uuid_or_nil(ctx, "transaction_id", 1)?,
        next_owner_mask: args.parse_or(ctx, "next_owner_mask", 2, "u32", 0)?,
        asset_type: args.parse_or(ctx, "asset_type", 3, "i8", 0)?,
        inv_type: args.parse_or(ctx, "inv_type", 4, "i8", 0)?,
        wearable_type: args.parse_or(ctx, "wearable_type", 5, "u8", 0)?,
        name: args.str_or(ctx, "name", 6, "")?,
        description: args.str_or(ctx, "description", 7, "")?,
    })
}

/// Build an [`InventoryItem`] from keyword fields (`item_id` required).
fn build_inventory_item(args: &Args, ctx: &dyn ReplContext) -> Result<InventoryItem, ReplError> {
    let owner_id = args.uuid_or_nil(ctx, "owner_id", 11)?;
    let group_id = args.uuid_or_nil(ctx, "group_id", 14)?;
    let group_owned = args.bool_or(ctx, "group_owned", 15, false)?;
    let owner = if group_owned {
        OwnerKey::Group(GroupKey::from(group_id))
    } else {
        OwnerKey::Agent(AgentKey::from(owner_id))
    };
    let group = if group_id.is_nil() {
        None
    } else {
        Some(GroupKey::from(group_id))
    };
    Ok(InventoryItem {
        item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
        folder_id: InventoryFolderKey::from(args.uuid_or_nil(ctx, "folder_id", 1)?),
        name: args.str_or(ctx, "name", 2, "")?,
        description: args.str_or(ctx, "description", 3, "")?,
        asset_id: args.uuid_or_nil(ctx, "asset_id", 4)?,
        item_type: args.parse_or(ctx, "item_type", 5, "i8", 0)?,
        inv_type: args.parse_or(ctx, "inv_type", 6, "i8", 0)?,
        flags: args.parse_or(ctx, "flags", 7, "u32", 0)?,
        sale_type: args.parse_or(ctx, "sale_type", 8, "u8", 0)?,
        sale_price: Some(LindenAmount(args.parse_or(
            ctx,
            "sale_price",
            9,
            "u64",
            0,
        )?)),
        creation_date: args.parse_or(ctx, "creation_date", 10, "i32", 0)?,
        owner,
        last_owner_id: args.uuid_or_nil(ctx, "last_owner_id", 12)?,
        creator_id: AgentKey::from(args.uuid_or_nil(ctx, "creator_id", 13)?),
        group,
        permissions: Permissions5 {
            base: Permissions::from_bits(args.parse_or(ctx, "base_mask", 16, "u32", 0)?),
            owner: Permissions::from_bits(args.parse_or(ctx, "owner_mask", 17, "u32", 0)?),
            group: Permissions::from_bits(args.parse_or(ctx, "group_mask", 18, "u32", 0)?),
            everyone: Permissions::from_bits(args.parse_or(ctx, "everyone_mask", 19, "u32", 0)?),
            next_owner: Permissions::from_bits(args.parse_or(
                ctx,
                "next_owner_mask",
                20,
                "u32",
                0,
            )?),
        },
    })
}

/// Build a [`ParcelUpdate`] from keyword fields (`local_id` required).
fn build_parcel_update(args: &Args, ctx: &dyn ReplContext) -> Result<ParcelUpdate, ReplError> {
    Ok(ParcelUpdate {
        local_id: RegionLocalParcelId(args.req_parse(ctx, "local_id", 0, "i32")?),
        parcel_flags: ParcelFlags::from_bits(args.parse_or(ctx, "parcel_flags", 1, "u32", 0)?),
        sale_price: Some(LindenAmount(args.parse_or(
            ctx,
            "sale_price",
            2,
            "u64",
            0,
        )?)),
        name: args.str_or(ctx, "name", 3, "")?,
        description: args.str_or(ctx, "description", 4, "")?,
        music_url: args.opt_url(ctx, "music_url", 5)?,
        media_url: args.opt_url(ctx, "media_url", 6)?,
        media_id: args.opt_texture(ctx, "media_id", 7)?,
        media_auto_scale: args.bool_or(ctx, "media_auto_scale", 8, false)?,
        group_id: args.opt_group(ctx, "group_id", 9)?,
        pass_price: LindenAmount(args.parse_or(ctx, "pass_price", 10, "u64", 0)?),
        pass_hours: args.parse_or(ctx, "pass_hours", 11, "f32", 0.0)?,
        category: ParcelCategory::from_u8(args.parse_or(ctx, "category", 12, "u8", 0)?),
        auth_buyer_id: args.opt_agent(ctx, "auth_buyer_id", 13)?,
        snapshot_id: args.opt_texture(ctx, "snapshot_id", 14)?,
        user_location: args.opt_vector(ctx, "user_location", 15)?.map_or(
            RegionCoordinates::new(0.0, 0.0, 0.0),
            RegionCoordinates::from,
        ),
        user_look_at: args
            .opt_vector(ctx, "user_look_at", 16)?
            .map_or(Direction::ZERO, |v| Direction::new(v.x, v.y, v.z)),
        landing_type: args.parse_or(ctx, "landing_type", 17, "u8", 0)?,
    })
}

/// Build an [`AgentPreferences`] from optional keyword fields. Only the fields
/// present on the line are set ([`Some`]); the rest stay [`None`] so the update
/// changes only what was named. The three permission masks
/// (`perm_group`/`perm_everyone`/`perm_next_owner`) collapse into one
/// [`ObjectPermMasks`] when any is present (absent masks default to `0`).
fn build_agent_preferences(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<AgentPreferences, ReplError> {
    let perm_group = args.opt_parse::<i32>(ctx, "perm_group", 1, "i32")?;
    let perm_everyone = args.opt_parse::<i32>(ctx, "perm_everyone", 2, "i32")?;
    let perm_next_owner = args.opt_parse::<i32>(ctx, "perm_next_owner", 3, "i32")?;
    let default_object_perm_masks =
        if perm_group.is_some() || perm_everyone.is_some() || perm_next_owner.is_some() {
            Some(ObjectPermMasks {
                group: perm_group.unwrap_or(0),
                everyone: perm_everyone.unwrap_or(0),
                next_owner: perm_next_owner.unwrap_or(0),
            })
        } else {
            None
        };
    Ok(AgentPreferences {
        hover_height: args.opt_parse::<f64>(ctx, "hover_height", 0, "f64")?,
        default_object_perm_masks,
        max_access_pref: args.opt_str(ctx, "max_access", 4)?,
        language: args.opt_str(ctx, "language", 5)?,
        language_is_public: args.opt_parse::<bool>(ctx, "language_is_public", 6, "bool")?,
        god_level: None,
    })
}

/// Build a [`RegionInfoUpdate`] from keyword fields.
fn build_region_info_update(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<RegionInfoUpdate, ReplError> {
    let maturity = match args.opt_str(ctx, "maturity", 6)? {
        Some(value) => parse_maturity("maturity", &value)?,
        None => Maturity::Pg,
    };
    Ok(RegionInfoUpdate {
        block_terraform: args.bool_or(ctx, "block_terraform", 0, false)?,
        block_fly: args.bool_or(ctx, "block_fly", 1, false)?,
        allow_damage: args.bool_or(ctx, "allow_damage", 2, false)?,
        allow_land_resell: args.bool_or(ctx, "allow_land_resell", 3, true)?,
        agent_limit: args.parse_or(ctx, "agent_limit", 4, "i32", 40)?,
        object_bonus: args.parse_or(ctx, "object_bonus", 5, "f32", 1.0)?,
        maturity,
        restrict_pushobject: args.bool_or(ctx, "restrict_pushobject", 7, false)?,
        allow_parcel_changes: args.bool_or(ctx, "allow_parcel_changes", 8, true)?,
    })
}

/// Build an [`ObjectTransform`] from optional position/rotation/scale fields.
fn build_object_transform(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<ObjectTransform, ReplError> {
    Ok(ObjectTransform {
        position: args.opt_vector(ctx, "position", 1)?,
        rotation: args.opt_rotation(ctx, "rotation", 2)?,
        scale: args.opt_vector(ctx, "scale", 3)?,
        group: args.bool_or(ctx, "group", 4, false)?,
        uniform: args.bool_or(ctx, "uniform", 5, false)?,
    })
}

/// Build [`ObjectFlagSettings`] from four optional bool fields.
fn build_object_flag_settings(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<ObjectFlagSettings, ReplError> {
    Ok(ObjectFlagSettings {
        use_physics: args.bool_or(ctx, "use_physics", 1, false)?,
        is_temporary: args.bool_or(ctx, "is_temporary", 2, false)?,
        is_phantom: args.bool_or(ctx, "is_phantom", 3, false)?,
        casts_shadows: args.bool_or(ctx, "casts_shadows", 4, true)?,
    })
}

/// Build a [`MediaEntry`] from keyword fields (defaults match an empty entry).
fn build_media_entry(args: &Args, ctx: &dyn ReplContext) -> Result<MediaEntry, ReplError> {
    Ok(MediaEntry {
        alt_image_enable: args.bool_or(ctx, "alt_image_enable", 100, false)?,
        controls: args.parse_or(ctx, "controls", 101, "i32", 0)?,
        current_url: args.opt_url(ctx, "current_url", 102)?,
        home_url: args.opt_url(ctx, "home_url", 103)?,
        auto_loop: args.bool_or(ctx, "auto_loop", 104, false)?,
        auto_play: args.bool_or(ctx, "auto_play", 105, false)?,
        auto_scale: args.bool_or(ctx, "auto_scale", 106, false)?,
        auto_zoom: args.bool_or(ctx, "auto_zoom", 107, false)?,
        first_click_interact: args.bool_or(ctx, "first_click_interact", 108, false)?,
        width_pixels: args.parse_or(ctx, "width_pixels", 109, "i32", 0)?,
        height_pixels: args.parse_or(ctx, "height_pixels", 110, "i32", 0)?,
        whitelist_enable: args.bool_or(ctx, "whitelist_enable", 111, false)?,
        whitelist: args.vec_parse(ctx, "whitelist", 112, "url")?,
        perms_interact: args.parse_or(ctx, "perms_interact", 113, "u8", 0)?,
        perms_control: args.parse_or(ctx, "perms_control", 114, "u8", 0)?,
    })
}

/// Build a [`VoiceProvisionRequest`] from keyword fields (default: Vivox).
fn build_voice_request(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<VoiceProvisionRequest, ReplError> {
    Ok(VoiceProvisionRequest {
        voice_server_type: args.opt_str(ctx, "voice_server_type", 0)?,
        channel_type: args.opt_str(ctx, "channel_type", 1)?,
        parcel_local_id: match args.opt_str(ctx, "parcel_local_id", 2)? {
            Some(value) => Some(RegionLocalParcelId(args::literal::<i32>(
                "parcel_local_id",
                &value,
                "i32",
            )?)),
            None => None,
        },
        jsep_offer_sdp: args.opt_str(ctx, "jsep_offer_sdp", 3)?,
        logout: args.bool_or(ctx, "logout", 4, false)?,
        viewer_session: args.opt_str(ctx, "viewer_session", 5)?,
    })
}

/// Build an [`ExperienceUpdate`] from keyword fields (`public_id` required).
fn build_experience_update(
    args: &Args,
    ctx: &dyn ReplContext,
) -> Result<ExperienceUpdate, ReplError> {
    Ok(ExperienceUpdate {
        public_id: ExperienceKey::from(args.req_uuid(ctx, "public_id", 0)?),
        name: args.str_or(ctx, "name", 1, "")?,
        description: args.str_or(ctx, "description", 2, "")?,
        maturity: args.parse_or(ctx, "maturity", 3, "i32", 0)?,
        properties: args.parse_or(ctx, "properties", 4, "i32", 0)?,
        slurl: args.opt_url(ctx, "slurl", 5)?,
        extended_metadata: args.str_or(ctx, "extended_metadata", 6, "")?,
    })
}

/// Build an [`InventoryOffer`] from keyword fields.
fn build_inventory_offer(args: &Args, ctx: &dyn ReplContext) -> Result<InventoryOffer, ReplError> {
    let asset_type = enum_arg(args, ctx, "asset_type", 0, parse_asset_type)?;
    let id = args.req_uuid(ctx, "item_id", 1)?;
    // A folder offer types the id as a folder; any other asset class as an item.
    let item_id = if matches!(asset_type, AssetType::Folder) {
        InventoryItemOrFolderKey::Folder(InventoryFolderKey::from(id))
    } else {
        InventoryItemOrFolderKey::Item(InventoryKey::from(id))
    };
    Ok(InventoryOffer {
        asset_type,
        item_id,
        transaction_id: args.uuid_or_nil(ctx, "transaction_id", 2)?,
        from_agent_id: AgentKey::from(args.uuid_or_nil(ctx, "from_agent_id", 3)?),
        from_task: args.bool_or(ctx, "from_task", 4, false)?,
    })
}

/// A `(Uuid, bool)` pair list (`uuid:true,uuid:false`).
fn uuid_bool_pairs(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
) -> Result<Vec<(Uuid, bool)>, ReplError> {
    let mut out = Vec::new();
    for record in args.vec_records(ctx, field, pos)? {
        let id = args::literal_uuid(field, record_field(field, &record, 0)?)?;
        let flag = args::literal_bool(field, record_field(field, &record, 1)?)?;
        out.push((id, flag));
    }
    Ok(out)
}

/// A `(Uuid, u8)` pair list (`uuid:index,…`).
fn uuid_u8_pairs(
    args: &Args,
    ctx: &dyn ReplContext,
    field: &str,
    pos: usize,
) -> Result<Vec<(Uuid, u8)>, ReplError> {
    let mut out = Vec::new();
    for record in args.vec_records(ctx, field, pos)? {
        let id = args::literal_uuid(field, record_field(field, &record, 0)?)?;
        let index = args::literal::<u8>(field, record_field(field, &record, 1)?, "u8")?;
        out.push((id, index));
    }
    Ok(out)
}

/// Build the full set of command specs, one per [`Command`] variant.
fn all_specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            name: "send",
            usage: "(not supported: arbitrary messages cannot be built from text)",
            build: |_args, _ctx| {
                Err(ReplError::NotSupported(
                    "send",
                    "construct a specific command instead of a raw message",
                ))
            },
        },
        CommandSpec {
            name: "chat",
            usage: "<message> [chat_type=normal] [channel=0]",
            build: |args, ctx| {
                let chat_type = match args.opt_str(ctx, "chat_type", 1)? {
                    Some(value) => parse_chat_type("chat_type", &value)?,
                    None => ChatType::Normal,
                };
                Ok(Command::Chat {
                    message: args.req_str(ctx, "message", 0)?,
                    chat_type,
                    channel: ChatChannel(args.parse_or(ctx, "channel", 2, "i32", 0)?),
                })
            },
        },
        CommandSpec {
            name: "typing",
            usage: "<true|false>",
            build: |args, ctx| Ok(Command::Typing(args.req_bool(ctx, "typing", 0)?)),
        },
        CommandSpec {
            name: "im",
            usage: "<to_agent_id> <message>",
            build: |args, ctx| {
                Ok(Command::InstantMessage {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    message: args.req_str(ctx, "message", 1)?,
                })
            },
        },
        CommandSpec {
            name: "im_typing",
            usage: "<to_agent_id> <true|false>",
            build: |args, ctx| {
                Ok(Command::ImTyping {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    typing: args.req_bool(ctx, "typing", 1)?,
                })
            },
        },
        CommandSpec {
            name: "set_controls",
            usage: "<bits-u32>",
            build: |args, ctx| {
                Ok(Command::SetControls(ControlFlags::from_bits(
                    args.req_parse(ctx, "bits", 0, "u32")?,
                )))
            },
        },
        CommandSpec {
            name: "set_throttle",
            usage: "[resend=] [land=] [wind=] [cloud=] [task=] [texture=] [asset=]",
            build: |args, ctx| Ok(Command::SetThrottle(build_throttle(args, ctx)?)),
        },
        CommandSpec {
            name: "set_rotation",
            usage: "<body-rot> <head-rot>",
            build: |args, ctx| {
                Ok(Command::SetRotation {
                    body: args.req_rotation(ctx, "body", 0)?,
                    head: args.req_rotation(ctx, "head", 1)?,
                })
            },
        },
        CommandSpec {
            name: "set_camera",
            usage: "eye=<v> target=<v> | center=<v> at_axis=<v> left_axis=<v> up_axis=<v>",
            build: |args, ctx| Ok(Command::SetCamera(build_camera(args, ctx)?)),
        },
        CommandSpec {
            name: "stand",
            usage: "",
            build: |_args, _ctx| Ok(Command::Stand),
        },
        CommandSpec {
            name: "sit_on_ground",
            usage: "",
            build: |_args, _ctx| Ok(Command::SitOnGround),
        },
        CommandSpec {
            name: "sit",
            usage: "<target> <offset-vec>",
            build: |args, ctx| {
                Ok(Command::Sit {
                    target: args.req_object(ctx, "target", 0)?,
                    offset: args.req_vector(ctx, "offset", 1)?,
                })
            },
        },
        CommandSpec {
            name: "autopilot",
            usage: "<global_x> <global_y> <z>",
            build: |args, ctx| {
                Ok(Command::Autopilot {
                    global_x: args.req_parse(ctx, "global_x", 0, "f64")?,
                    global_y: args.req_parse(ctx, "global_y", 1, "f64")?,
                    z: args.req_parse(ctx, "z", 2, "f64")?,
                })
            },
        },
        CommandSpec {
            name: "request_avatar_properties",
            usage: "<avatar_id>",
            build: |args, ctx| {
                Ok(Command::RequestAvatarProperties(args.req_agent(
                    ctx,
                    "avatar_id",
                    0,
                )?))
            },
        },
        CommandSpec {
            name: "request_avatar_picks",
            usage: "<avatar_id>",
            build: |args, ctx| {
                Ok(Command::RequestAvatarPicks(args.req_agent(
                    ctx,
                    "avatar_id",
                    0,
                )?))
            },
        },
        CommandSpec {
            name: "request_avatar_notes",
            usage: "<avatar_id>",
            build: |args, ctx| {
                Ok(Command::RequestAvatarNotes(args.req_agent(
                    ctx,
                    "avatar_id",
                    0,
                )?))
            },
        },
        CommandSpec {
            name: "request_avatar_classifieds",
            usage: "<avatar_id>",
            build: |args, ctx| {
                Ok(Command::RequestAvatarClassifieds(args.req_agent(
                    ctx,
                    "avatar_id",
                    0,
                )?))
            },
        },
        CommandSpec {
            name: "request_pick_info",
            usage: "<creator_id> <pick_id>",
            build: |args, ctx| {
                Ok(Command::RequestPickInfo {
                    creator_id: AgentKey::from(args.req_uuid(ctx, "creator_id", 0)?),
                    pick_id: PickKey::from(args.req_uuid(ctx, "pick_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "request_classified_info",
            usage: "<classified_id>",
            build: |args, ctx| {
                Ok(Command::RequestClassifiedInfo(ClassifiedKey::from(
                    args.req_uuid(ctx, "classified_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "update_profile",
            usage: "[image_id=] [about_text=] [profile_url=] …",
            build: |args, ctx| Ok(Command::UpdateProfile(build_profile_update(args, ctx)?)),
        },
        CommandSpec {
            name: "update_interests",
            usage: "[want_to_mask=] [want_to_text=] …",
            build: |args, ctx| Ok(Command::UpdateInterests(build_interests_update(args, ctx)?)),
        },
        CommandSpec {
            name: "update_avatar_notes",
            usage: "<target_id> <notes>",
            build: |args, ctx| {
                Ok(Command::UpdateAvatarNotes {
                    target_id: args.req_agent(ctx, "target_id", 0)?,
                    notes: args.req_str(ctx, "notes", 1)?,
                })
            },
        },
        CommandSpec {
            name: "update_pick",
            usage: "[pick_id=] [name=] [pos_global=<x,y,z>] …",
            build: |args, ctx| Ok(Command::UpdatePick(build_pick_update(args, ctx)?)),
        },
        CommandSpec {
            name: "delete_pick",
            usage: "<pick_id>",
            build: |args, ctx| {
                Ok(Command::DeletePick(PickKey::from(
                    args.req_uuid(ctx, "pick_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "god_delete_pick",
            usage: "<pick_id> <query_id>",
            build: |args, ctx| {
                Ok(Command::GodDeletePick {
                    pick_id: PickKey::from(args.req_uuid(ctx, "pick_id", 0)?),
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "update_classified",
            usage: "[classified_id=] [name=] …",
            build: |args, ctx| {
                Ok(Command::UpdateClassified(build_classified_update(
                    args, ctx,
                )?))
            },
        },
        CommandSpec {
            name: "delete_classified",
            usage: "<classified_id>",
            build: |args, ctx| {
                Ok(Command::DeleteClassified(ClassifiedKey::from(
                    args.req_uuid(ctx, "classified_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "god_delete_classified",
            usage: "<classified_id> <query_id>",
            build: |args, ctx| {
                Ok(Command::GodDeleteClassified {
                    classified_id: ClassifiedKey::from(args.req_uuid(ctx, "classified_id", 0)?),
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "request_folder_contents",
            usage: "<folder_id>",
            build: |args, ctx| {
                Ok(Command::RequestFolderContents(InventoryFolderKey::from(
                    args.req_uuid(ctx, "folder_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "fetch_inventory_folders",
            usage: "<folder_id,folder_id,…>",
            build: |args, ctx| {
                Ok(Command::FetchInventoryFolders(
                    args.vec_uuid(ctx, "folder_ids", 0)?
                        .into_iter()
                        .map(InventoryFolderKey::from)
                        .collect(),
                ))
            },
        },
        CommandSpec {
            name: "create_inventory_folder",
            usage: "<folder_id> <parent_id> <folder_type> <name>",
            build: |args, ctx| {
                Ok(Command::CreateInventoryFolder {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 1)?),
                    folder_type: args.parse_or(ctx, "folder_type", 2, "i8", -1)?,
                    name: args.req_str(ctx, "name", 3)?,
                })
            },
        },
        CommandSpec {
            name: "update_inventory_folder",
            usage: "<folder_id> <parent_id> <folder_type> <name>",
            build: |args, ctx| {
                Ok(Command::UpdateInventoryFolder {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 1)?),
                    folder_type: args.parse_or(ctx, "folder_type", 2, "i8", -1)?,
                    name: args.req_str(ctx, "name", 3)?,
                })
            },
        },
        CommandSpec {
            name: "move_inventory_folder",
            usage: "<folder_id> <parent_id>",
            build: |args, ctx| {
                Ok(Command::MoveInventoryFolder {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "remove_inventory_folders",
            usage: "<folder_id,folder_id,…>",
            build: |args, ctx| {
                Ok(Command::RemoveInventoryFolders(
                    args.vec_uuid(ctx, "folder_ids", 0)?
                        .into_iter()
                        .map(InventoryFolderKey::from)
                        .collect(),
                ))
            },
        },
        CommandSpec {
            name: "create_inventory_item",
            usage: "[folder_id=] [name=] [asset_type=] …",
            build: |args, ctx| {
                Ok(Command::CreateInventoryItem(build_new_inventory_item(
                    args, ctx,
                )?))
            },
        },
        CommandSpec {
            name: "update_inventory_item",
            usage: "item_id=<id> [transaction_id=] [name=] …",
            build: |args, ctx| {
                Ok(Command::UpdateInventoryItem {
                    item: Box::new(build_inventory_item(args, ctx)?),
                    transaction_id: TransactionId::from(args.uuid_or_nil(
                        ctx,
                        "transaction_id",
                        100,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "move_inventory_item",
            usage: "<item_id> <folder_id> [new_name]",
            build: |args, ctx| {
                Ok(Command::MoveInventoryItem {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 1)?),
                    new_name: args.str_or(ctx, "new_name", 2, "")?,
                })
            },
        },
        CommandSpec {
            name: "copy_inventory_item",
            usage: "<old_agent_id> <old_item_id> <new_folder_id> <new_name>",
            build: |args, ctx| {
                Ok(Command::CopyInventoryItem {
                    old_agent_id: AgentKey::from(args.req_uuid(ctx, "old_agent_id", 0)?),
                    old_item_id: InventoryKey::from(args.req_uuid(ctx, "old_item_id", 1)?),
                    new_folder_id: InventoryFolderKey::from(args.req_uuid(
                        ctx,
                        "new_folder_id",
                        2,
                    )?),
                    new_name: args.req_str(ctx, "new_name", 3)?,
                })
            },
        },
        CommandSpec {
            name: "remove_inventory_items",
            usage: "<item_id,item_id,…>",
            build: |args, ctx| {
                Ok(Command::RemoveInventoryItems(
                    args.vec_uuid(ctx, "item_ids", 0)?
                        .into_iter()
                        .map(InventoryKey::from)
                        .collect(),
                ))
            },
        },
        CommandSpec {
            name: "change_inventory_item_flags",
            usage: "<item_id> <flags-u32>",
            build: |args, ctx| {
                Ok(Command::ChangeInventoryItemFlags {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    flags: args.req_parse(ctx, "flags", 1, "u32")?,
                })
            },
        },
        CommandSpec {
            name: "purge_inventory_descendents",
            usage: "<folder_id>",
            build: |args, ctx| {
                Ok(Command::PurgeInventoryDescendents(
                    InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                ))
            },
        },
        CommandSpec {
            name: "remove_inventory_objects",
            usage: "folder_ids=<…> item_ids=<…>",
            build: |args, ctx| {
                Ok(Command::RemoveInventoryObjects {
                    folder_ids: args
                        .vec_uuid(ctx, "folder_ids", 0)?
                        .into_iter()
                        .map(InventoryFolderKey::from)
                        .collect(),
                    item_ids: args
                        .vec_uuid(ctx, "item_ids", 1)?
                        .into_iter()
                        .map(InventoryKey::from)
                        .collect(),
                })
            },
        },
        CommandSpec {
            name: "create_inventory_category",
            usage: "<parent_id> <folder_type> <name>",
            build: |args, ctx| {
                Ok(Command::CreateInventoryCategory {
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 0)?),
                    folder_type: args.parse_or(ctx, "folder_type", 1, "i32", -1)?,
                    name: args.req_str(ctx, "name", 2)?,
                })
            },
        },
        CommandSpec {
            name: "ais3_create_folder",
            usage: "<parent_id> <folder_type> <name>",
            build: |args, ctx| {
                Ok(Command::Ais3CreateFolder {
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 0)?),
                    folder_type: args.parse_or(ctx, "folder_type", 1, "i32", -1)?,
                    name: args.req_str(ctx, "name", 2)?,
                })
            },
        },
        CommandSpec {
            name: "ais3_rename_folder",
            usage: "<folder_id> <name>",
            build: |args, ctx| {
                Ok(Command::Ais3RenameFolder {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    name: args.req_str(ctx, "name", 1)?,
                })
            },
        },
        CommandSpec {
            name: "ais3_move_folder",
            usage: "<folder_id> <parent_id>",
            build: |args, ctx| {
                Ok(Command::Ais3MoveFolder {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "ais3_remove_folder",
            usage: "<folder_id>",
            build: |args, ctx| {
                Ok(Command::Ais3RemoveFolder(InventoryFolderKey::from(
                    args.req_uuid(ctx, "folder_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "ais3_purge_folder",
            usage: "<folder_id>",
            build: |args, ctx| {
                Ok(Command::Ais3PurgeFolder(InventoryFolderKey::from(
                    args.req_uuid(ctx, "folder_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "ais3_fetch_folder_children",
            usage: "<folder_id> <depth>",
            build: |args, ctx| {
                Ok(Command::Ais3FetchFolderChildren {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 0)?),
                    depth: args.parse_or(ctx, "depth", 1, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "ais3_update_item",
            usage: "<item_id> <name> <description>",
            build: |args, ctx| {
                Ok(Command::Ais3UpdateItem {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    name: args.req_str(ctx, "name", 1)?,
                    description: args.str_or(ctx, "description", 2, "")?,
                })
            },
        },
        CommandSpec {
            name: "ais3_move_item",
            usage: "<item_id> <parent_id>",
            build: |args, ctx| {
                Ok(Command::Ais3MoveItem {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    parent_id: InventoryFolderKey::from(args.req_uuid(ctx, "parent_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "ais3_remove_item",
            usage: "<item_id>",
            build: |args, ctx| {
                Ok(Command::Ais3RemoveItem(InventoryKey::from(
                    args.req_uuid(ctx, "item_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "ais3_fetch_item",
            usage: "<item_id>",
            build: |args, ctx| {
                Ok(Command::Ais3FetchItem(InventoryKey::from(
                    args.req_uuid(ctx, "item_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "grant_user_rights",
            usage: "<target> <rights-i32>",
            build: |args, ctx| {
                Ok(Command::GrantUserRights {
                    target: FriendKey::from(args.req_uuid(ctx, "target", 0)?),
                    rights: FriendRights(args.req_parse(ctx, "rights", 1, "i32")?),
                })
            },
        },
        CommandSpec {
            name: "offer_friendship",
            usage: "<to_agent_id> [message]",
            build: |args, ctx| {
                Ok(Command::OfferFriendship {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    message: args.str_or(ctx, "message", 1, "")?,
                })
            },
        },
        CommandSpec {
            name: "terminate_friendship",
            usage: "<agent_id>",
            build: |args, ctx| {
                Ok(Command::TerminateFriendship(FriendKey::from(
                    args.req_uuid(ctx, "agent_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "accept_friendship",
            usage: "<transaction_id> <calling_card_folder>",
            build: |args, ctx| {
                Ok(Command::AcceptFriendship {
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 0)?),
                    calling_card_folder: InventoryFolderKey::from(args.req_uuid(
                        ctx,
                        "calling_card_folder",
                        1,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "decline_friendship",
            usage: "<transaction_id>",
            build: |args, ctx| {
                Ok(Command::DeclineFriendship(TransactionId::from(
                    args.req_uuid(ctx, "transaction_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "offer_calling_card",
            usage: "<to_agent_id> <transaction_id>",
            build: |args, ctx| {
                Ok(Command::OfferCallingCard {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "accept_calling_card",
            usage: "<transaction_id> <calling_card_folder>",
            build: |args, ctx| {
                Ok(Command::AcceptCallingCard {
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 0)?),
                    calling_card_folder: InventoryFolderKey::from(args.req_uuid(
                        ctx,
                        "calling_card_folder",
                        1,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "decline_calling_card",
            usage: "<transaction_id>",
            build: |args, ctx| {
                Ok(Command::DeclineCallingCard(TransactionId::from(
                    args.req_uuid(ctx, "transaction_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "activate_group",
            usage: "[group_id]",
            build: |args, ctx| {
                Ok(Command::ActivateGroup(GroupKey::from(
                    args.uuid_or_nil(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_members",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupMembers(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "fetch_group_members",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::FetchGroupMembers(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_roles",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupRoles(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_role_members",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupRoleMembers(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_titles",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupTitles(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_profile",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupProfile(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_notices",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupNotices(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "request_group_notice",
            usage: "<notice_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupNotice(GroupNoticeKey::from(
                    args.req_uuid(ctx, "notice_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "create_group",
            usage: "name=<name> [charter=] [open_enrollment=] …",
            build: |args, ctx| Ok(Command::CreateGroup(build_create_group_params(args, ctx)?)),
        },
        CommandSpec {
            name: "join_group",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::JoinGroup(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "leave_group",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::LeaveGroup(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "invite_to_group",
            usage: "<group_id> <invitee:role,invitee:role,…>",
            build: |args, ctx| {
                let mut invitees = Vec::new();
                for record in args.vec_records(ctx, "invitees", 1)? {
                    let invitee =
                        args::literal_uuid("invitees", record_field("invitees", &record, 0)?)?;
                    let role = match record.get(1) {
                        Some(value) => args::literal_uuid("invitees", value)?,
                        None => Uuid::nil(),
                    };
                    invitees.push((AgentKey::from(invitee), GroupRoleKey::from(role)));
                }
                Ok(Command::InviteToGroup {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    invitees,
                })
            },
        },
        CommandSpec {
            name: "set_group_accept_notices",
            usage: "<group_id> <accept_notices> <list_in_profile>",
            build: |args, ctx| {
                Ok(Command::SetGroupAcceptNotices {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    accept_notices: args.req_bool(ctx, "accept_notices", 1)?,
                    list_in_profile: args.bool_or(ctx, "list_in_profile", 2, true)?,
                })
            },
        },
        CommandSpec {
            name: "set_group_contribution",
            usage: "<group_id> <contribution>",
            build: |args, ctx| {
                Ok(Command::SetGroupContribution {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    contribution: args.req_parse(ctx, "contribution", 1, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "start_group_session",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::StartGroupSession(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "send_group_message",
            usage: "<group_id> <message>",
            build: |args, ctx| {
                Ok(Command::SendGroupMessage {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    message: args.req_str(ctx, "message", 1)?,
                })
            },
        },
        CommandSpec {
            name: "leave_group_session",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::LeaveGroupSession(GroupKey::from(
                    args.req_uuid(ctx, "group_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "update_group_roles",
            usage: "<group_id> [role_id=] [name=] [powers=] [update_type=]",
            build: |args, ctx| {
                let update_type = match args.opt_str(ctx, "update_type", 100)? {
                    Some(value) => parse_group_role_update_type("update_type", &value)?,
                    None => sl_proto::GroupRoleUpdateType::UpdateAll,
                };
                let role = GroupRoleEdit {
                    role_id: {
                        let raw = args.uuid_or_nil(ctx, "role_id", 1)?;
                        (!raw.is_nil()).then(|| GroupRoleKey::from(raw))
                    },
                    name: args.str_or(ctx, "name", 2, "")?,
                    description: args.str_or(ctx, "description", 3, "")?,
                    title: args.str_or(ctx, "title", 4, "")?,
                    powers: args.parse_or(ctx, "powers", 5, "u64", 0)?,
                    update_type,
                };
                Ok(Command::UpdateGroupRoles {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    roles: vec![role],
                })
            },
        },
        CommandSpec {
            name: "change_group_role_members",
            usage: "<group_id> <role:member:add|remove,…>",
            build: |args, ctx| {
                let mut changes = Vec::new();
                for record in args.vec_records(ctx, "changes", 1)? {
                    changes.push(GroupRoleMemberChange {
                        role_id: {
                            let raw = args::literal_uuid(
                                "changes",
                                record_field("changes", &record, 0)?,
                            )?;
                            (!raw.is_nil()).then(|| GroupRoleKey::from(raw))
                        },
                        member_id: AgentKey::from(args::literal_uuid(
                            "changes",
                            record_field("changes", &record, 1)?,
                        )?),
                        change: parse_group_role_change(
                            "changes",
                            record_field("changes", &record, 2)?,
                        )?,
                    });
                }
                Ok(Command::ChangeGroupRoleMembers {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    changes,
                })
            },
        },
        CommandSpec {
            name: "eject_group_members",
            usage: "<group_id> <member_id,member_id,…>",
            build: |args, ctx| {
                Ok(Command::EjectGroupMembers {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    member_ids: args
                        .vec_uuid(ctx, "member_ids", 1)?
                        .into_iter()
                        .map(AgentKey::from)
                        .collect(),
                })
            },
        },
        CommandSpec {
            name: "send_group_notice",
            usage: "<group_id> <subject> <message> [attachment_item=] [attachment_owner=]",
            build: |args, ctx| {
                let attachment = match args.opt_str(ctx, "attachment_item", 100)? {
                    Some(item) => Some(GroupNoticeAttachment {
                        item_id: InventoryKey::from(args::literal_uuid("attachment_item", &item)?),
                        owner_id: args.uuid_or_nil(ctx, "attachment_owner", 101)?,
                    }),
                    None => None,
                };
                Ok(Command::SendGroupNotice {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    subject: args.req_str(ctx, "subject", 1)?,
                    message: args.req_str(ctx, "message", 2)?,
                    attachment,
                })
            },
        },
        CommandSpec {
            name: "request_group_account_summary",
            usage: "<group_id> <request_id> <interval_days> <current_interval>",
            build: |args, ctx| {
                Ok(Command::RequestGroupAccountSummary {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    request_id: GroupRequestId::from(args.req_uuid(ctx, "request_id", 1)?),
                    interval_days: args.req_parse(ctx, "interval_days", 2, "i32")?,
                    current_interval: args.req_parse(ctx, "current_interval", 3, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "request_group_account_details",
            usage: "<group_id> <request_id> <interval_days> <current_interval>",
            build: |args, ctx| {
                Ok(Command::RequestGroupAccountDetails {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    request_id: GroupRequestId::from(args.req_uuid(ctx, "request_id", 1)?),
                    interval_days: args.req_parse(ctx, "interval_days", 2, "i32")?,
                    current_interval: args.req_parse(ctx, "current_interval", 3, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "request_group_account_transactions",
            usage: "<group_id> <request_id> <interval_days> <current_interval>",
            build: |args, ctx| {
                Ok(Command::RequestGroupAccountTransactions {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    request_id: GroupRequestId::from(args.req_uuid(ctx, "request_id", 1)?),
                    interval_days: args.req_parse(ctx, "interval_days", 2, "i32")?,
                    current_interval: args.req_parse(ctx, "current_interval", 3, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "request_group_active_proposals",
            usage: "<group_id> <transaction_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupActiveProposals {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "request_group_vote_history",
            usage: "<group_id> <transaction_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupVoteHistory {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "start_group_proposal",
            usage: "<group_id> <quorum> <majority> <duration_secs> <proposal_text>",
            build: |args, ctx| {
                Ok(Command::StartGroupProposal {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                    quorum: args.req_parse(ctx, "quorum", 1, "i32")?,
                    majority: args.req_parse(ctx, "majority", 2, "f32")?,
                    duration: args.req_parse(ctx, "duration", 3, "i32")?,
                    proposal_text: args.req_str(ctx, "proposal_text", 4)?,
                })
            },
        },
        CommandSpec {
            name: "group_proposal_ballot",
            usage: "<proposal_id> <group_id> <vote_cast>",
            build: |args, ctx| {
                Ok(Command::GroupProposalBallot {
                    proposal_id: ProposalVoteId::from(args.req_uuid(ctx, "proposal_id", 0)?),
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 1)?),
                    vote_cast: args.req_str(ctx, "vote_cast", 2)?,
                })
            },
        },
        CommandSpec {
            name: "reply_script_dialog",
            usage: "<object_id> <chat_channel> <button_index> <button_label>",
            build: |args, ctx| {
                Ok(Command::ReplyScriptDialog {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    chat_channel: ChatChannel(args.req_parse(ctx, "chat_channel", 1, "i32")?),
                    button_index: args.req_parse(ctx, "button_index", 2, "i32")?,
                    button_label: args.req_str(ctx, "button_label", 3)?,
                })
            },
        },
        CommandSpec {
            name: "answer_script_permissions",
            usage: "<task_id> <item_id> <permissions-i32>",
            build: |args, ctx| {
                Ok(Command::AnswerScriptPermissions {
                    task_id: args.req_object(ctx, "task_id", 0)?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                    permissions: ScriptPermissions(args.parse_or(
                        ctx,
                        "permissions",
                        2,
                        "i32",
                        0,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "request_mute_list",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestMuteList),
        },
        CommandSpec {
            name: "mute",
            usage: "<id> <name> [mute_type=agent] [flags=0]",
            build: |args, ctx| {
                let mute_type = match args.opt_str(ctx, "mute_type", 2)? {
                    Some(value) => parse_mute_type("mute_type", &value)?,
                    None => MuteType::Agent,
                };
                Ok(Command::Mute {
                    id: args.uuid_or_nil(ctx, "id", 0)?,
                    name: args.str_or(ctx, "name", 1, "")?,
                    mute_type,
                    flags: MuteFlags(args.parse_or(ctx, "flags", 3, "u32", 0)?),
                })
            },
        },
        CommandSpec {
            name: "unmute",
            usage: "<id> <name>",
            build: |args, ctx| {
                Ok(Command::Unmute {
                    id: args.uuid_or_nil(ctx, "id", 0)?,
                    name: args.str_or(ctx, "name", 1, "")?,
                })
            },
        },
        CommandSpec {
            name: "teleport",
            usage: "<region_handle> <position-vec> <look_at-vec>",
            build: |args, ctx| {
                Ok(Command::Teleport {
                    region_handle: RegionHandle(args.req_parse(ctx, "region_handle", 0, "u64")?),
                    position: RegionCoordinates::from(args.req_vector(ctx, "position", 1)?),
                    look_at: args.opt_vector(ctx, "look_at", 2)?.unwrap_or(Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                })
            },
        },
        CommandSpec {
            name: "request_region_info",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestRegionInfo),
        },
        CommandSpec {
            name: "request_avatar_names",
            usage: "<agent_id> [agent_id...]",
            build: |args, ctx| {
                Ok(Command::RequestAvatarNames(
                    args.req_agent_list(ctx, "agent_id", 0)?,
                ))
            },
        },
        CommandSpec {
            name: "request_group_names",
            usage: "<group_id> [group_id...]",
            build: |args, ctx| {
                Ok(Command::RequestGroupNames(
                    args.req_group_list(ctx, "group_id", 0)?,
                ))
            },
        },
        CommandSpec {
            name: "request_display_names",
            usage: "<agent_id> [agent_id...]",
            build: |args, ctx| {
                Ok(Command::RequestDisplayNames(
                    args.req_agent_list(ctx, "agent_id", 0)?,
                ))
            },
        },
        CommandSpec {
            name: "request_environment",
            usage: "[parcel_id]",
            build: |args, ctx| {
                Ok(Command::RequestEnvironment {
                    parcel_id: args.opt_parse(ctx, "parcel_id", 0, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "request_parcel_properties",
            usage: "<west> <south> <east> <north> [sequence_id=0]",
            build: |args, ctx| {
                Ok(Command::RequestParcelProperties {
                    west: args.req_parse(ctx, "west", 0, "f32")?,
                    south: args.req_parse(ctx, "south", 1, "f32")?,
                    east: args.req_parse(ctx, "east", 2, "f32")?,
                    north: args.req_parse(ctx, "north", 3, "f32")?,
                    sequence_id: args.parse_or(ctx, "sequence_id", 4, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "update_parcel",
            usage: "local_id=<id> [name=] [sale_price=] …",
            build: |args, ctx| Ok(Command::UpdateParcel(build_parcel_update(args, ctx)?)),
        },
        CommandSpec {
            name: "request_parcel_access_list",
            usage: "<local_id> <access|ban>",
            build: |args, ctx| {
                Ok(Command::RequestParcelAccessList {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    scope: enum_arg(args, ctx, "scope", 1, parse_parcel_access_scope)?,
                })
            },
        },
        CommandSpec {
            name: "update_parcel_access_list",
            usage: "<local_id> <access|ban> [entries=<id:time:flags,…>]",
            build: |args, ctx| {
                let mut entries = Vec::new();
                for record in args.vec_records(ctx, "entries", 2)? {
                    entries.push(ParcelAccessEntry {
                        id: args::literal_uuid("entries", record_field("entries", &record, 0)?)?,
                        time: match record.get(1) {
                            Some(value) => args::literal::<i32>("entries", value, "i32")?,
                            None => 0,
                        },
                        flags: ParcelAccessFlags(match record.get(2) {
                            Some(value) => args::literal::<u32>("entries", value, "u32")?,
                            None => 0,
                        }),
                    });
                }
                Ok(Command::UpdateParcelAccessList {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    scope: enum_arg(args, ctx, "scope", 1, parse_parcel_access_scope)?,
                    entries,
                })
            },
        },
        CommandSpec {
            name: "request_parcel_dwell",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::RequestParcelDwell {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                })
            },
        },
        CommandSpec {
            name: "buy_parcel",
            usage: "<local_id> <price> <area> [group_id] [is_group_owned]",
            build: |args, ctx| {
                Ok(Command::BuyParcel {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    price: args.req_parse(ctx, "price", 1, "i32")?,
                    area: args.req_parse(ctx, "area", 2, "i32")?,
                    group_id: args.opt_group(ctx, "group_id", 3)?,
                    is_group_owned: args.bool_or(ctx, "is_group_owned", 4, false)?,
                })
            },
        },
        CommandSpec {
            name: "return_parcel_objects",
            usage: "<local_id> <return_type-u32> [owner_ids=] [task_ids=]",
            build: |args, ctx| {
                Ok(Command::ReturnParcelObjects {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    return_type: ParcelReturnType(args.req_parse(ctx, "return_type", 1, "u32")?),
                    owner_ids: args.vec_owner(ctx, "owner_ids", 2)?,
                    task_ids: args.vec_object(ctx, "task_ids", 3)?,
                })
            },
        },
        CommandSpec {
            name: "select_parcel_objects",
            usage: "<local_id> <return_type-u32> [object_ids=]",
            build: |args, ctx| {
                Ok(Command::SelectParcelObjects {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    return_type: ParcelReturnType(args.req_parse(ctx, "return_type", 1, "u32")?),
                    object_ids: args.vec_object(ctx, "object_ids", 2)?,
                })
            },
        },
        CommandSpec {
            name: "deed_parcel_to_group",
            usage: "<local_id> <group_id>",
            build: |args, ctx| {
                Ok(Command::DeedParcelToGroup {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "reclaim_parcel",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::ReclaimParcel {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                })
            },
        },
        CommandSpec {
            name: "release_parcel",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::ReleaseParcel {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                })
            },
        },
        CommandSpec {
            name: "join_parcels",
            usage: "<west> <south> <east> <north>",
            build: |args, ctx| {
                Ok(Command::JoinParcels {
                    west: args.req_parse(ctx, "west", 0, "f32")?,
                    south: args.req_parse(ctx, "south", 1, "f32")?,
                    east: args.req_parse(ctx, "east", 2, "f32")?,
                    north: args.req_parse(ctx, "north", 3, "f32")?,
                })
            },
        },
        CommandSpec {
            name: "divide_parcel",
            usage: "<west> <south> <east> <north>",
            build: |args, ctx| {
                Ok(Command::DivideParcel {
                    west: args.req_parse(ctx, "west", 0, "f32")?,
                    south: args.req_parse(ctx, "south", 1, "f32")?,
                    east: args.req_parse(ctx, "east", 2, "f32")?,
                    north: args.req_parse(ctx, "north", 3, "f32")?,
                })
            },
        },
        CommandSpec {
            name: "request_parcel_object_owners",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::RequestParcelObjectOwners {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                })
            },
        },
        CommandSpec {
            name: "buy_parcel_pass",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::BuyParcelPass {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                })
            },
        },
        CommandSpec {
            name: "disable_parcel_objects",
            usage: "<local_id> <return_type-u32> [owner_ids=] [task_ids=]",
            build: |args, ctx| {
                Ok(Command::DisableParcelObjects {
                    local_id: scoped_parcel(ctx, args.req_parse(ctx, "local_id", 0, "i32")?)?,
                    return_type: ParcelReturnType(args.req_parse(ctx, "return_type", 1, "u32")?),
                    owner_ids: args.vec_owner(ctx, "owner_ids", 2)?,
                    task_ids: args.vec_object(ctx, "task_ids", 3)?,
                })
            },
        },
        CommandSpec {
            name: "request_parcel_info",
            usage: "<parcel_id>",
            build: |args, ctx| {
                Ok(Command::RequestParcelInfo {
                    parcel_id: ParcelKey::from(args.req_uuid(ctx, "parcel_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "request_remote_parcel_id",
            usage: "<location-x,y,z> [region_id] [region_handle-u64]",
            build: |args, ctx| {
                Ok(Command::RequestRemoteParcelId {
                    location: args.req_vector(ctx, "location", 0)?.into(),
                    region_id: args.uuid_or_nil(ctx, "region_id", 1)?,
                    region_handle: RegionHandle(args.parse_or(
                        ctx,
                        "region_handle",
                        2,
                        "u64",
                        0_u64,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "request_simulator_features",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestSimulatorFeatures),
        },
        CommandSpec {
            name: "request_agent_preferences",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestAgentPreferences),
        },
        CommandSpec {
            name: "set_agent_preferences",
            usage: "[hover_height=] [perm_group=] [perm_everyone=] [perm_next_owner=] \
                    [max_access=PG|M|A] [language=] [language_is_public=]",
            build: |args, ctx| {
                Ok(Command::SetAgentPreferences(Box::new(
                    build_agent_preferences(args, ctx)?,
                )))
            },
        },
        CommandSpec {
            name: "request_object_cost",
            usage: "<object_ids-comma-separated>",
            build: |args, ctx| {
                Ok(Command::RequestObjectCost {
                    object_ids: args.vec_object(ctx, "object_ids", 0)?,
                })
            },
        },
        CommandSpec {
            name: "request_selected_cost",
            usage: "<object_ids-comma-separated> [roots=true]",
            build: |args, ctx| {
                Ok(Command::RequestSelectedCost {
                    object_ids: args.vec_object(ctx, "object_ids", 0)?,
                    roots: args.bool_or(ctx, "roots", 1, true)?,
                })
            },
        },
        CommandSpec {
            name: "request_object_physics_data",
            usage: "<object_ids-comma-separated>",
            build: |args, ctx| {
                Ok(Command::RequestObjectPhysicsData {
                    object_ids: args.vec_object(ctx, "object_ids", 0)?,
                })
            },
        },
        CommandSpec {
            name: "request_attachment_resources",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestAttachmentResources),
        },
        CommandSpec {
            name: "request_land_resources",
            usage: "<parcel_id>",
            build: |args, ctx| {
                Ok(Command::RequestLandResources {
                    parcel_id: ParcelKey::from(args.req_uuid(ctx, "parcel_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "request_land_stat",
            usage: "[report_type=scripts|colliders] [request_flags-u32] [filter] \
                    [parcel_local_id-i32]",
            build: |args, ctx| {
                Ok(Command::RequestLandStat {
                    report_type: enum_arg_or(
                        args,
                        ctx,
                        "report_type",
                        0,
                        parse_land_stat_report_type,
                        LandStatReportType::TopScripts,
                    )?,
                    request_flags: args.parse_or(ctx, "request_flags", 1, "u32", 0_u32)?,
                    filter: args.opt_str(ctx, "filter", 2)?.unwrap_or_default(),
                    parcel_local_id: scoped_parcel(
                        ctx,
                        args.parse_or(ctx, "parcel_local_id", 3, "i32", 0_i32)?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "request_estate_info",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestEstateInfo),
        },
        CommandSpec {
            name: "update_estate_access",
            usage: "<delta> <target>",
            build: |args, ctx| {
                Ok(Command::UpdateEstateAccess {
                    delta: enum_arg(args, ctx, "delta", 0, parse_estate_access_delta)?,
                    target: args.req_owner(ctx, "target", 1)?,
                })
            },
        },
        CommandSpec {
            name: "kick_estate_user",
            usage: "<target>",
            build: |args, ctx| {
                Ok(Command::KickEstateUser {
                    target: args.req_agent(ctx, "target", 0)?,
                })
            },
        },
        CommandSpec {
            name: "teleport_home_user",
            usage: "<target>",
            build: |args, ctx| {
                Ok(Command::TeleportHomeUser {
                    target: args.req_agent(ctx, "target", 0)?,
                })
            },
        },
        CommandSpec {
            name: "teleport_home_all_users",
            usage: "",
            build: |_args, _ctx| Ok(Command::TeleportHomeAllUsers),
        },
        CommandSpec {
            name: "restart_region",
            usage: "<seconds>",
            build: |args, ctx| {
                Ok(Command::RestartRegion {
                    seconds: args.req_parse(ctx, "seconds", 0, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "send_estate_message",
            usage: "<message>",
            build: |args, ctx| {
                Ok(Command::SendEstateMessage {
                    message: args.req_str(ctx, "message", 0)?,
                })
            },
        },
        CommandSpec {
            name: "set_region_info",
            usage: "[block_fly=] [agent_limit=] [maturity=] …",
            build: |args, ctx| Ok(Command::SetRegionInfo(build_region_info_update(args, ctx)?)),
        },
        CommandSpec {
            name: "request_estate_covenant",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestEstateCovenant),
        },
        CommandSpec {
            name: "request_telehub_info",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestTelehubInfo),
        },
        CommandSpec {
            name: "connect_telehub",
            usage: "<object_local_id>",
            build: |args, ctx| {
                Ok(Command::ConnectTelehub {
                    object_local_id: scoped_object(
                        ctx,
                        args.req_parse(ctx, "object_local_id", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "disconnect_telehub",
            usage: "",
            build: |_args, _ctx| Ok(Command::DisconnectTelehub),
        },
        CommandSpec {
            name: "add_telehub_spawn_point",
            usage: "<object_local_id>",
            build: |args, ctx| {
                Ok(Command::AddTelehubSpawnPoint {
                    object_local_id: scoped_object(
                        ctx,
                        args.req_parse(ctx, "object_local_id", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "remove_telehub_spawn_point",
            usage: "<spawn_index>",
            build: |args, ctx| {
                Ok(Command::RemoveTelehubSpawnPoint {
                    spawn_index: args.req_parse(ctx, "spawn_index", 0, "u32")?,
                })
            },
        },
        CommandSpec {
            name: "god_kick_user",
            usage: "<target> [reason]",
            build: |args, ctx| {
                Ok(Command::GodKickUser {
                    target: args.req_agent(ctx, "target", 0)?,
                    reason: args.str_or(ctx, "reason", 1, "")?,
                })
            },
        },
        CommandSpec {
            name: "send_godlike_message",
            usage: "<method> [params=a,b,c]",
            build: |args, ctx| {
                Ok(Command::SendGodlikeMessage {
                    method: args.req_str(ctx, "method", 0)?,
                    params: args.vec_parse(ctx, "params", 1, "string")?,
                })
            },
        },
        CommandSpec {
            name: "request_money_balance",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestMoneyBalance),
        },
        CommandSpec {
            name: "request_economy_data",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestEconomyData),
        },
        CommandSpec {
            name: "send_money_transfer",
            usage: "<dest> <amount> [kind=gift] [description]",
            build: |args, ctx| {
                let kind = match args.opt_str(ctx, "kind", 2)? {
                    Some(value) => parse_money_tx_type("kind", &value)?,
                    None => MoneyTransactionType::Gift,
                };
                Ok(Command::SendMoneyTransfer {
                    dest: args.req_uuid(ctx, "dest", 0)?,
                    amount: LindenAmount(args.req_parse(ctx, "amount", 1, "u64")?),
                    kind,
                    description: args.str_or(ctx, "description", 3, "")?,
                })
            },
        },
        CommandSpec {
            name: "set_draw_distance",
            usage: "<metres>",
            build: |args, ctx| {
                Ok(Command::SetDrawDistance(Distance::new(f64::from(
                    args.req_parse::<f32>(ctx, "metres", 0, "f32")?,
                ))))
            },
        },
        CommandSpec {
            name: "request_map_blocks",
            usage: "<min_x> <max_x> <min_y> <max_y>",
            build: |args, ctx| {
                Ok(Command::RequestMapBlocks {
                    min_x: args.req_parse(ctx, "min_x", 0, "u32")?,
                    max_x: args.req_parse(ctx, "max_x", 1, "u32")?,
                    min_y: args.req_parse(ctx, "min_y", 2, "u32")?,
                    max_y: args.req_parse(ctx, "max_y", 3, "u32")?,
                })
            },
        },
        CommandSpec {
            name: "request_map_by_name",
            usage: "<name>",
            build: |args, ctx| {
                Ok(Command::RequestMapByName {
                    name: args.req_str(ctx, "name", 0)?,
                })
            },
        },
        CommandSpec {
            name: "request_map_items",
            usage: "<item_type> [region_handle=0]",
            build: |args, ctx| {
                Ok(Command::RequestMapItems {
                    item_type: enum_arg(args, ctx, "item_type", 0, parse_map_item_type)?,
                    region_handle: RegionHandle(args.parse_or(
                        ctx,
                        "region_handle",
                        1,
                        "u64",
                        0_u64,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "request_map_layer",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestMapLayer),
        },
        CommandSpec {
            name: "send_abuse_report",
            usage: "<summary> [abuser_id=nil] [region_name] [category=0] \
                    [report_type=complaint|bug] [object_id=nil] [position=<x,y,z>]",
            build: |args, ctx| Ok(Command::SendAbuseReport(abuse_report_from_args(args, ctx)?)),
        },
        CommandSpec {
            name: "send_abuse_report_caps",
            usage: "<summary> [abuser_id=nil] [region_name] [category=0] \
                    [report_type=complaint|bug] [object_id=nil] [position=<x,y,z>] \
                    [screenshot=<hex>]",
            build: |args, ctx| {
                Ok(Command::SendAbuseReportViaCaps {
                    report: abuse_report_from_args(args, ctx)?,
                    screenshot: args.opt_bytes(ctx, "screenshot", 7)?,
                })
            },
        },
        CommandSpec {
            name: "send_postcard",
            usage: "<asset_id> <to> [subject] [message] [name] [from] \
                    [position=<x,y,z>] [allow_publish=false] [mature_publish=false]",
            build: |args, ctx| {
                let (pos_x, pos_y, pos_z) = args
                    .opt_str(ctx, "position", 6)?
                    .map(|value| parse_global("position", &value))
                    .transpose()?
                    .unwrap_or((0.0, 0.0, 0.0));
                Ok(Command::SendPostcard(Box::new(Postcard {
                    asset_id: args.req_uuid(ctx, "asset_id", 0)?,
                    pos_global: GlobalCoordinates::new(pos_x, pos_y, pos_z),
                    to: args.req_str(ctx, "to", 1)?,
                    from: args.opt_str(ctx, "from", 5)?.unwrap_or_default(),
                    name: args.opt_str(ctx, "name", 4)?.unwrap_or_default(),
                    subject: args.opt_str(ctx, "subject", 2)?.unwrap_or_default(),
                    message: args.opt_str(ctx, "message", 3)?.unwrap_or_default(),
                    allow_publish: args.bool_or(ctx, "allow_publish", 7, false)?,
                    mature_publish: args.bool_or(ctx, "mature_publish", 8, false)?,
                })))
            },
        },
        CommandSpec {
            name: "request_objects",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::RequestObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "request_object_properties",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::RequestObjectProperties {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "deselect_objects",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::DeselectObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "touch_object",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::TouchObject {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                })
            },
        },
        CommandSpec {
            name: "grab_object",
            usage: "<local_id> <grab_offset-vec>",
            build: |args, ctx| {
                Ok(Command::GrabObject {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    grab_offset: args.req_vector(ctx, "grab_offset", 1)?,
                })
            },
        },
        CommandSpec {
            name: "grab_object_update",
            usage: "<object_id> <grab_offset_initial> <grab_position> <time_since_last>",
            build: |args, ctx| {
                Ok(Command::GrabObjectUpdate {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    grab_offset_initial: args.req_vector(ctx, "grab_offset_initial", 1)?,
                    grab_position: args.req_vector(ctx, "grab_position", 2)?,
                    time_since_last: args.parse_or(ctx, "time_since_last", 3, "u32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "degrab_object",
            usage: "<local_id>",
            build: |args, ctx| {
                Ok(Command::DegrabObject {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                })
            },
        },
        CommandSpec {
            name: "rez_object",
            usage: "<position-vec> [group_id]",
            build: |args, ctx| {
                Ok(Command::RezObject {
                    shape: PrimShape::cube(args.req_vector(ctx, "position", 0)?),
                    group_id: args.opt_group(ctx, "group_id", 1)?,
                })
            },
        },
        CommandSpec {
            name: "duplicate_objects",
            usage: "<local_id,…> <offset-vec> [group_id]",
            build: |args, ctx| {
                Ok(Command::DuplicateObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                    offset: args.req_vector(ctx, "offset", 1)?,
                    group_id: args.opt_group(ctx, "group_id", 2)?,
                })
            },
        },
        CommandSpec {
            name: "delete_objects",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::DeleteObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "derez_objects",
            usage: "<local_id,…> <destination> <destination_id> <transaction_id> [group_id]",
            build: |args, ctx| {
                Ok(Command::DerezObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                    destination: {
                        let id = args.uuid_or_nil(ctx, "destination_id", 2)?;
                        let value = args.req_str(ctx, "destination", 1)?;
                        parse_derez_destination("destination", &value, id)?
                    },
                    transaction_id: TransactionId::from(args.uuid_or_nil(
                        ctx,
                        "transaction_id",
                        3,
                    )?),
                    group_id: args.opt_group(ctx, "group_id", 4)?,
                })
            },
        },
        CommandSpec {
            name: "update_object",
            usage: "<local_id> [position=<v>] [rotation=<r>] [scale=<v>] [uniform=]",
            build: |args, ctx| {
                Ok(Command::UpdateObject {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    transform: build_object_transform(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_name",
            usage: "<local_id> <name>",
            build: |args, ctx| {
                Ok(Command::SetObjectName {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    name: args.req_str(ctx, "name", 1)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_description",
            usage: "<local_id> <description>",
            build: |args, ctx| {
                Ok(Command::SetObjectDescription {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    description: args.req_str(ctx, "description", 1)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_click_action",
            usage: "<local_id> <action>",
            build: |args, ctx| {
                Ok(Command::SetObjectClickAction {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    action: enum_arg(args, ctx, "action", 1, parse_click_action)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_material",
            usage: "<local_id> <material>",
            build: |args, ctx| {
                Ok(Command::SetObjectMaterial {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    material: enum_arg(args, ctx, "material", 1, parse_material)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_flags",
            usage: "<local_id> [use_physics=] [is_temporary=] [is_phantom=] [casts_shadows=]",
            build: |args, ctx| {
                Ok(Command::SetObjectFlags {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    flags: build_object_flag_settings(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_group",
            usage: "<local_id,…> <group_id>",
            build: |args, ctx| {
                Ok(Command::SetObjectGroup {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "set_object_permissions",
            usage: "<local_id,…> <field> <set-bool> <mask-u32>",
            build: |args, ctx| {
                Ok(Command::SetObjectPermissions {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                    field: enum_arg(args, ctx, "field", 1, parse_permission_field)?,
                    set: args.req_bool(ctx, "set", 2)?,
                    mask: args.req_parse(ctx, "mask", 3, "u32")?,
                })
            },
        },
        CommandSpec {
            name: "set_object_for_sale",
            usage: "<local_id> <sale_type> <sale_price>",
            build: |args, ctx| {
                Ok(Command::SetObjectForSale {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    sale_type: enum_arg(args, ctx, "sale_type", 1, parse_sale_type)?,
                    sale_price: Some(LindenAmount(args.req_parse(ctx, "sale_price", 2, "u64")?)),
                })
            },
        },
        CommandSpec {
            name: "set_object_category",
            usage: "<local_id> <category-u32>",
            build: |args, ctx| {
                Ok(Command::SetObjectCategory {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    category: args.req_parse(ctx, "category", 1, "u32")?,
                })
            },
        },
        CommandSpec {
            name: "set_object_include_in_search",
            usage: "<local_id> <include-bool>",
            build: |args, ctx| {
                Ok(Command::SetObjectIncludeInSearch {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    include: args.req_bool(ctx, "include", 1)?,
                })
            },
        },
        CommandSpec {
            name: "link_objects",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::LinkObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "delink_objects",
            usage: "<local_id,local_id,…>",
            build: |args, ctx| {
                Ok(Command::DelinkObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "buy_object",
            usage: "<group_id> <category_id> <local_id:sale_type:sale_price,…>",
            build: |args, ctx| {
                let group_id = GroupKey::from(args.uuid_or_nil(ctx, "group_id", 0)?);
                let category_id = args.uuid_or_nil(ctx, "category_id", 1)?;
                let mut objects = Vec::new();
                for record in args.vec_records(ctx, "objects", 2)? {
                    objects.push(ObjectBuyItem {
                        local_id: RegionLocalObjectId(args::literal(
                            "objects",
                            record_field("objects", &record, 0)?,
                            "u32",
                        )?),
                        sale_type: parse_sale_type(
                            "objects",
                            record_field("objects", &record, 1)?,
                        )?,
                        sale_price: LindenAmount(args::literal(
                            "objects",
                            record_field("objects", &record, 2)?,
                            "u64",
                        )?),
                    });
                }
                Ok(Command::BuyObject {
                    group_id,
                    category_id,
                    objects,
                })
            },
        },
        CommandSpec {
            name: "buy_object_inventory",
            usage: "<object_id> <item_id> <folder_id>",
            build: |args, ctx| {
                Ok(Command::BuyObjectInventory {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                    folder_id: InventoryFolderKey::from(args.uuid_or_nil(ctx, "folder_id", 2)?),
                })
            },
        },
        CommandSpec {
            name: "request_pay_price",
            usage: "<object_id>",
            build: |args, ctx| {
                Ok(Command::RequestPayPrice {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                })
            },
        },
        CommandSpec {
            name: "request_object_properties_family",
            usage: "<object_id> [request_flags=]",
            build: |args, ctx| {
                Ok(Command::RequestObjectPropertiesFamily {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    request_flags: args.parse_or(ctx, "request_flags", 1, "u32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "spin_object_start",
            usage: "<object_id>",
            build: |args, ctx| {
                Ok(Command::SpinObjectStart {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                })
            },
        },
        CommandSpec {
            name: "spin_object_update",
            usage: "<object_id> <rotation>",
            build: |args, ctx| {
                Ok(Command::SpinObjectUpdate {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    rotation: args.req_rotation(ctx, "rotation", 1)?,
                })
            },
        },
        CommandSpec {
            name: "spin_object_stop",
            usage: "<object_id>",
            build: |args, ctx| {
                Ok(Command::SpinObjectStop {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                })
            },
        },
        CommandSpec {
            name: "duplicate_objects_on_ray",
            usage: "<local_id,…> <ray_start-vec> <ray_end-vec> [group_id=] [ray_target_id=] \
                    [bypass_raycast=] [ray_end_is_intersection=] [copy_centers=] [copy_rotates=] \
                    [duplicate_flags=]",
            build: |args, ctx| {
                Ok(Command::DuplicateObjectsOnRay {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                    ray_start: args.req_vector(ctx, "ray_start", 1)?,
                    ray_end: args.req_vector(ctx, "ray_end", 2)?,
                    group_id: args.opt_group(ctx, "group_id", 100)?,
                    ray_target_id: args.opt_object(ctx, "ray_target_id", 101)?,
                    bypass_raycast: args.bool_or(ctx, "bypass_raycast", 102, false)?,
                    ray_end_is_intersection: args.bool_or(
                        ctx,
                        "ray_end_is_intersection",
                        103,
                        false,
                    )?,
                    copy_centers: args.bool_or(ctx, "copy_centers", 104, true)?,
                    copy_rotates: args.bool_or(ctx, "copy_rotates", 105, true)?,
                    duplicate_flags: args.parse_or(ctx, "duplicate_flags", 106, "u32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "rez_restore_to_world",
            usage: "<item_id> [folder_id=] [creator_id=] [owner_id=] [group_id=] [base_mask=] \
                    [owner_mask=] [group_mask=] [everyone_mask=] [next_owner_mask=] \
                    [group_owned=] [transaction_id=] [asset_type=] [inv_type=] [flags=] \
                    [sale_type=] [sale_price=] [name=] [description=] [creation_date=] [crc=]",
            build: |args, ctx| {
                let owner_id = args.uuid_or_nil(ctx, "owner_id", 102)?;
                let group_id = args.uuid_or_nil(ctx, "group_id", 103)?;
                let group_owned = args.bool_or(ctx, "group_owned", 109, false)?;
                let owner = if group_owned {
                    OwnerKey::Group(GroupKey::from(group_id))
                } else {
                    OwnerKey::Agent(AgentKey::from(owner_id))
                };
                let group = if group_id.is_nil() {
                    None
                } else {
                    Some(GroupKey::from(group_id))
                };
                Ok(Command::RezRestoreToWorld {
                    item: RestoreItem {
                        item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                        folder_id: InventoryFolderKey::from(args.uuid_or_nil(
                            ctx,
                            "folder_id",
                            100,
                        )?),
                        creator_id: AgentKey::from(args.uuid_or_nil(ctx, "creator_id", 101)?),
                        owner,
                        group,
                        permissions: Permissions5 {
                            base: Permissions::from_bits(args.parse_or(
                                ctx,
                                "base_mask",
                                104,
                                "u32",
                                0,
                            )?),
                            owner: Permissions::from_bits(args.parse_or(
                                ctx,
                                "owner_mask",
                                105,
                                "u32",
                                0,
                            )?),
                            group: Permissions::from_bits(args.parse_or(
                                ctx,
                                "group_mask",
                                106,
                                "u32",
                                0,
                            )?),
                            everyone: Permissions::from_bits(args.parse_or(
                                ctx,
                                "everyone_mask",
                                107,
                                "u32",
                                0,
                            )?),
                            next_owner: Permissions::from_bits(args.parse_or(
                                ctx,
                                "next_owner_mask",
                                108,
                                "u32",
                                0,
                            )?),
                        },
                        transaction_id: args.uuid_or_nil(ctx, "transaction_id", 110)?,
                        asset_type: args.parse_or(ctx, "asset_type", 111, "i8", -1)?,
                        inv_type: args.parse_or(ctx, "inv_type", 112, "i8", -1)?,
                        flags: args.parse_or(ctx, "flags", 113, "u32", 0)?,
                        sale_type: enum_arg_or(
                            args,
                            ctx,
                            "sale_type",
                            114,
                            parse_sale_type,
                            SaleType::NotForSale,
                        )?,
                        sale_price: Some(LindenAmount(args.parse_or(
                            ctx,
                            "sale_price",
                            115,
                            "u64",
                            0,
                        )?)),
                        name: args.str_or(ctx, "name", 116, "")?,
                        description: args.str_or(ctx, "description", 117, "")?,
                        creation_date: args.parse_or(ctx, "creation_date", 118, "i32", 0)?,
                        crc: args.parse_or(ctx, "crc", 119, "u32", 0)?,
                    },
                })
            },
        },
        CommandSpec {
            name: "rez_object_from_notecard",
            usage: "<notecard_item_id> <ray_start-vec> <ray_end-vec> <item_id,…> [group_id=] \
                    [from_task_id=] [object_id=] [ray_target_id=] [bypass_raycast=] \
                    [ray_end_is_intersection=] [rez_selected=] [remove_item=] [item_flags=] \
                    [group_mask=] [everyone_mask=] [next_owner_mask=]",
            build: |args, ctx| {
                Ok(Command::RezObjectFromNotecard {
                    rez: NotecardRez {
                        notecard_item_id: InventoryKey::from(args.req_uuid(
                            ctx,
                            "notecard_item_id",
                            0,
                        )?),
                        ray_start: args.req_vector(ctx, "ray_start", 1)?,
                        ray_end: args.req_vector(ctx, "ray_end", 2)?,
                        item_ids: args
                            .vec_uuid(ctx, "item_ids", 3)?
                            .into_iter()
                            .map(InventoryKey::from)
                            .collect(),
                        group_id: args.opt_group(ctx, "group_id", 100)?,
                        from_task_id: args.opt_object(ctx, "from_task_id", 101)?,
                        object_id: args.object_or_nil(ctx, "object_id", 102)?,
                        ray_target_id: args.opt_object(ctx, "ray_target_id", 103)?,
                        bypass_raycast: args.bool_or(ctx, "bypass_raycast", 104, false)?,
                        ray_end_is_intersection: args.bool_or(
                            ctx,
                            "ray_end_is_intersection",
                            105,
                            false,
                        )?,
                        rez_selected: args.bool_or(ctx, "rez_selected", 106, false)?,
                        remove_item: args.bool_or(ctx, "remove_item", 107, false)?,
                        item_flags: args.parse_or(ctx, "item_flags", 108, "u32", 0)?,
                        group_mask: args.parse_or(ctx, "group_mask", 109, "u32", 0)?,
                        everyone_mask: args.parse_or(ctx, "everyone_mask", 110, "u32", 0)?,
                        next_owner_mask: args.parse_or(ctx, "next_owner_mask", 111, "u32", 0)?,
                    },
                })
            },
        },
        CommandSpec {
            name: "request_script_running",
            usage: "<object_id> <item_id>",
            build: |args, ctx| {
                Ok(Command::RequestScriptRunning {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "set_script_running",
            usage: "<object_id> <item_id> <running>",
            build: |args, ctx| {
                Ok(Command::SetScriptRunning {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                    running: args.req_bool(ctx, "running", 2)?,
                })
            },
        },
        CommandSpec {
            name: "reset_script",
            usage: "<object_id> <item_id>",
            build: |args, ctx| {
                Ok(Command::ResetScript {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "request_texture",
            usage: "<texture_id> [discard_level=0] [priority=1.0]",
            build: |args, ctx| {
                Ok(Command::RequestTexture {
                    texture_id: TextureKey::from(args.req_uuid(ctx, "texture_id", 0)?),
                    discard_level: args.parse_or(ctx, "discard_level", 1, "i8", 0)?,
                    priority: args.parse_or(ctx, "priority", 2, "f32", 1.0)?,
                })
            },
        },
        CommandSpec {
            name: "request_asset",
            usage: "<asset_id> <asset_type-code> [priority=1.0]",
            build: |args, ctx| {
                Ok(Command::RequestAsset {
                    asset_id: AssetKey::from(args.req_uuid(ctx, "asset_id", 0)?),
                    asset_type: enum_arg(args, ctx, "asset_type", 1, parse_asset_type)?,
                    priority: args.parse_or(ctx, "priority", 2, "f32", 1.0)?,
                })
            },
        },
        CommandSpec {
            name: "fetch_texture",
            usage: "<texture_id> [discard_level=0]",
            build: |args, ctx| {
                Ok(Command::FetchTexture {
                    texture_id: TextureKey::from(args.req_uuid(ctx, "texture_id", 0)?),
                    discard_level: args.parse_or(ctx, "discard_level", 1, "u8", 0)?,
                })
            },
        },
        CommandSpec {
            name: "fetch_mesh",
            usage: "<mesh_id> [start= end=]",
            build: |args, ctx| {
                Ok(Command::FetchMesh {
                    mesh_id: MeshKey::from(args.req_uuid(ctx, "mesh_id", 0)?),
                    byte_range: byte_range(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "fetch_asset",
            usage: "<asset_id> <asset_type-code> [start= end=]",
            build: |args, ctx| {
                Ok(Command::FetchAsset {
                    asset_id: AssetKey::from(args.req_uuid(ctx, "asset_id", 0)?),
                    asset_type: enum_arg(args, ctx, "asset_type", 1, parse_asset_type)?,
                    byte_range: byte_range(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "request_wearables",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestWearables),
        },
        CommandSpec {
            name: "set_wearing",
            usage: "<item_id:asset_id:wearable_type,…>",
            build: |args, ctx| {
                let mut wearables = Vec::new();
                for record in args.vec_records(ctx, "wearables", 0)? {
                    wearables.push(Wearable {
                        item_id: InventoryKey::from(args::literal_uuid(
                            "wearables",
                            record_field("wearables", &record, 0)?,
                        )?),
                        asset_id: {
                            let raw = args::literal_uuid(
                                "wearables",
                                record_field("wearables", &record, 1)?,
                            )?;
                            (!raw.is_nil()).then_some(raw)
                        },
                        wearable_type: parse_wearable_type(
                            "wearables",
                            record_field("wearables", &record, 2)?,
                        )?,
                    });
                }
                Ok(Command::SetWearing(wearables))
            },
        },
        CommandSpec {
            name: "attach_object",
            usage: "<local_id> <attachment_point> [mode=add|replace] [rotation=<r>]",
            build: |args, ctx| {
                Ok(Command::AttachObject {
                    local_id: scoped_object(ctx, args.req_parse(ctx, "local_id", 0, "u32")?)?,
                    attachment_point: enum_arg(
                        args,
                        ctx,
                        "attachment_point",
                        1,
                        parse_attachment_point,
                    )?,
                    mode: enum_arg_or(
                        args,
                        ctx,
                        "mode",
                        2,
                        parse_attachment_mode,
                        AttachmentMode::Replace,
                    )?,
                    rotation: args.opt_rotation(ctx, "rotation", 3)?.unwrap_or(Rotation {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        s: 1.0,
                    }),
                })
            },
        },
        CommandSpec {
            name: "detach_objects",
            usage: "<local_id,…>",
            build: |args, ctx| {
                Ok(Command::DetachObjects {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "drop_attachments",
            usage: "<local_id,…>",
            build: |args, ctx| {
                Ok(Command::DropAttachments {
                    local_ids: scoped_objects(
                        ctx,
                        args.vec_parse::<u32>(ctx, "local_ids", 0, "u32")?,
                    )?,
                })
            },
        },
        CommandSpec {
            name: "remove_attachment",
            usage: "<attachment_point> <item_id>",
            build: |args, ctx| {
                Ok(Command::RemoveAttachment {
                    attachment_point: enum_arg(
                        args,
                        ctx,
                        "attachment_point",
                        0,
                        parse_attachment_point,
                    )?,
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "rez_attachment",
            usage: "<item_id> <attachment_point> [owner_id=] [mode=add|replace] [name=] \
                    [description=]",
            build: |args, ctx| {
                Ok(Command::RezAttachment(RezAttachment {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    attachment_point: enum_arg(
                        args,
                        ctx,
                        "attachment_point",
                        1,
                        parse_attachment_point,
                    )?,
                    owner_id: args.uuid_or_nil(ctx, "owner_id", 2)?,
                    mode: enum_arg_or(
                        args,
                        ctx,
                        "mode",
                        3,
                        parse_attachment_mode,
                        AttachmentMode::Replace,
                    )?,
                    name: args.opt_str(ctx, "name", 4)?.unwrap_or_default(),
                    description: args.opt_str(ctx, "description", 5)?.unwrap_or_default(),
                }))
            },
        },
        CommandSpec {
            name: "rez_attachments",
            usage: "<compound_id> <item_id:owner_id:attachment_point[:add|replace],…> \
                    [detach=detach|keep]",
            build: |args, ctx| {
                let compound_id = TransactionId::from(args.uuid_or_nil(ctx, "compound_id", 0)?);
                let detach = enum_arg_or(
                    args,
                    ctx,
                    "detach",
                    100,
                    parse_detach_order,
                    DetachOrder::Keep,
                )?;
                let mut attachments = Vec::new();
                for record in args.vec_records(ctx, "attachments", 1)? {
                    let mode = match record.get(3) {
                        Some(value) => parse_attachment_mode("attachments", value)?,
                        None => AttachmentMode::Replace,
                    };
                    attachments.push(RezAttachment {
                        item_id: InventoryKey::from(args::literal_uuid(
                            "attachments",
                            record_field("attachments", &record, 0)?,
                        )?),
                        owner_id: args::literal_uuid(
                            "attachments",
                            record_field("attachments", &record, 1)?,
                        )?,
                        attachment_point: parse_attachment_point(
                            "attachments",
                            record_field("attachments", &record, 2)?,
                        )?,
                        mode,
                        name: String::new(),
                        description: String::new(),
                    });
                }
                Ok(Command::RezAttachments {
                    compound_id,
                    detach,
                    attachments,
                })
            },
        },
        CommandSpec {
            name: "viewer_effect",
            usage: "<effect_type> [id=] [agent=] [duration=] [color=<hex8>] \
                    [data=lookat|pointat|spiral|raw] [source=] [target=] [position=<x,y,z>] \
                    [look_at=] [point_at=] [raw=<hex>]",
            build: |args, ctx| {
                let effect_type = enum_arg(args, ctx, "effect_type", 0, parse_viewer_effect_type)?;
                Ok(Command::ViewerEffect(vec![ViewerEffect {
                    id: args.uuid_or_nil(ctx, "id", 100)?,
                    agent_id: AgentKey::from(args.uuid_or_nil(ctx, "agent", 101)?),
                    effect_type,
                    duration: args.parse_or(ctx, "duration", 102, "f32", 1.0_f32)?,
                    color: color_or_white(args, ctx, "color", 103)?,
                    data: parse_effect_data(args, ctx, effect_type)?,
                }]))
            },
        },
        CommandSpec {
            name: "track_agent",
            usage: "<prey_id>",
            build: |args, ctx| {
                Ok(Command::TrackAgent {
                    prey_id: AgentKey::from(args.req_uuid(ctx, "prey_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "find_agent",
            usage: "<hunter> <prey>",
            build: |args, ctx| {
                Ok(Command::FindAgent {
                    hunter: args.req_agent(ctx, "hunter", 0)?,
                    prey: args.req_agent(ctx, "prey", 1)?,
                })
            },
        },
        CommandSpec {
            name: "dir_find_query",
            usage: "<query_id> <query_text> <flags-u32> [query_start=0]",
            build: |args, ctx| {
                Ok(Command::DirFindQuery {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    query_text: args.req_str(ctx, "query_text", 1)?,
                    flags: DirFindFlags::from_bits(args.req_parse(ctx, "flags", 2, "u32")?),
                    query_start: args.parse_or(ctx, "query_start", 3, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "dir_places_query",
            usage: "<query_id> <query_text> <flags-u32> [category=0] [sim_name=] [query_start=0]",
            build: |args, ctx| {
                Ok(Command::DirPlacesQuery {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    query_text: args.req_str(ctx, "query_text", 1)?,
                    flags: DirFindFlags::from_bits(args.req_parse(ctx, "flags", 2, "u32")?),
                    category: ParcelCategory::from_u8(args.parse_or(ctx, "category", 3, "u8", 0)?),
                    sim_name: args.str_or(ctx, "sim_name", 4, "")?,
                    query_start: args.parse_or(ctx, "query_start", 5, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "dir_land_query",
            usage: "<query_id> <flags-u32> [search_type=4294967295] [price=0] [area=0] \
                    [query_start=0]",
            build: |args, ctx| {
                Ok(Command::DirLandQuery {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    flags: DirFindFlags::from_bits(args.req_parse(ctx, "flags", 1, "u32")?),
                    search_type: LandSearchType::from_bits(args.parse_or(
                        ctx,
                        "search_type",
                        2,
                        "u32",
                        LandSearchType::ALL.bits(),
                    )?),
                    price: args.parse_or(ctx, "price", 3, "i32", 0)?,
                    area: args.parse_or(ctx, "area", 4, "i32", 0)?,
                    query_start: args.parse_or(ctx, "query_start", 5, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "dir_classified_query",
            usage: "<query_id> <query_text> <flags-u32> [category=0] [query_start=0]",
            build: |args, ctx| {
                Ok(Command::DirClassifiedQuery {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    query_text: args.req_str(ctx, "query_text", 1)?,
                    flags: DirFindFlags::from_bits(args.req_parse(ctx, "flags", 2, "u32")?),
                    category: ClassifiedCategory::from_u32(
                        args.parse_or(ctx, "category", 3, "u32", 0)?,
                    ),
                    query_start: args.parse_or(ctx, "query_start", 4, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "avatar_picker_request",
            usage: "<query_id> <name>",
            build: |args, ctx| {
                Ok(Command::AvatarPickerRequest {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    name: args.req_str(ctx, "name", 1)?,
                })
            },
        },
        CommandSpec {
            name: "places_query",
            usage: "<query_id> <transaction_id> [query_text=] [flags=0] [category=0] [sim_name=]",
            build: |args, ctx| {
                Ok(Command::PlacesQuery {
                    query_id: QueryId::from(args.req_uuid(ctx, "query_id", 0)?),
                    transaction_id: TransactionId::from(args.req_uuid(ctx, "transaction_id", 1)?),
                    query_text: args.str_or(ctx, "query_text", 2, "")?,
                    flags: DirFindFlags::from_bits(args.parse_or(ctx, "flags", 3, "u32", 0)?),
                    category: ParcelCategory::from_u8(args.parse_or(ctx, "category", 4, "u8", 0)?),
                    sim_name: args.str_or(ctx, "sim_name", 5, "")?,
                })
            },
        },
        CommandSpec {
            name: "event_info_request",
            usage: "<event_id-u32>",
            build: |args, ctx| {
                Ok(Command::EventInfoRequest {
                    event_id: EventId::new(args.req_parse(ctx, "event_id", 0, "u32")?),
                })
            },
        },
        CommandSpec {
            name: "event_notification_add_request",
            usage: "<event_id-u32>",
            build: |args, ctx| {
                Ok(Command::EventNotificationAddRequest {
                    event_id: EventId::new(args.req_parse(ctx, "event_id", 0, "u32")?),
                })
            },
        },
        CommandSpec {
            name: "event_notification_remove_request",
            usage: "<event_id-u32>",
            build: |args, ctx| {
                Ok(Command::EventNotificationRemoveRequest {
                    event_id: EventId::new(args.req_parse(ctx, "event_id", 0, "u32")?),
                })
            },
        },
        CommandSpec {
            name: "set_appearance",
            usage: "<serial> <size-vec> texture_entry=<hex> visual_params=<hex> [wearable_cache=id:idx,…]",
            build: |args, ctx| {
                Ok(Command::SetAppearance {
                    serial: args.req_parse(ctx, "serial", 0, "u32")?,
                    size: args.req_vector(ctx, "size", 1)?,
                    texture_entry: args.bytes_or_empty(ctx, "texture_entry", 100)?,
                    visual_params: args.bytes_or_empty(ctx, "visual_params", 101)?,
                    wearable_cache: uuid_u8_pairs(args, ctx, "wearable_cache", 102)?,
                })
            },
        },
        CommandSpec {
            name: "request_cached_textures",
            usage: "<serial> [slots=id:idx,…]",
            build: |args, ctx| {
                Ok(Command::RequestCachedTextures {
                    serial: args.req_parse(ctx, "serial", 0, "i32")?,
                    slots: uuid_u8_pairs(args, ctx, "slots", 1)?,
                })
            },
        },
        CommandSpec {
            name: "request_server_appearance_update",
            usage: "<cof_version>",
            build: |args, ctx| {
                Ok(Command::RequestServerAppearanceUpdate {
                    cof_version: args.req_parse(ctx, "cof_version", 0, "i32")?,
                })
            },
        },
        CommandSpec {
            name: "set_animations",
            usage: "<anim_id:start,anim_id:start,…>",
            build: |args, ctx| {
                Ok(Command::SetAnimations(
                    uuid_bool_pairs(args, ctx, "animations", 0)?
                        .into_iter()
                        .map(|(id, start)| (AnimationKey::from(id), start))
                        .collect(),
                ))
            },
        },
        CommandSpec {
            name: "play_animation",
            usage: "<anim_id>",
            build: |args, ctx| {
                Ok(Command::PlayAnimation(AnimationKey::from(
                    args.req_uuid(ctx, "anim_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "stop_animation",
            usage: "<anim_id>",
            build: |args, ctx| {
                Ok(Command::StopAnimation(AnimationKey::from(
                    args.req_uuid(ctx, "anim_id", 0)?,
                )))
            },
        },
        CommandSpec {
            name: "activate_gestures",
            usage: "<item_id:asset_id,item_id:asset_id,…>",
            build: |args, ctx| {
                let mut gestures = Vec::new();
                for record in args.vec_records(ctx, "gestures", 0)? {
                    gestures.push(GestureActivation {
                        item_id: InventoryKey::from(args::literal_uuid(
                            "gestures",
                            record_field("gestures", &record, 0)?,
                        )?),
                        asset_id: args::literal_uuid(
                            "gestures",
                            record_field("gestures", &record, 1)?,
                        )?,
                    });
                }
                Ok(Command::ActivateGestures { gestures })
            },
        },
        CommandSpec {
            name: "deactivate_gestures",
            usage: "<item_id,item_id,…>",
            build: |args, ctx| {
                Ok(Command::DeactivateGestures {
                    item_ids: args
                        .vec_uuid(ctx, "item_ids", 0)?
                        .into_iter()
                        .map(InventoryKey::from)
                        .collect(),
                })
            },
        },
        CommandSpec {
            name: "set_always_run",
            usage: "<mode:run|walk>",
            build: |args, ctx| {
                Ok(Command::SetAlwaysRun {
                    mode: enum_arg(args, ctx, "mode", 0, parse_movement_mode)?,
                })
            },
        },
        CommandSpec {
            name: "pause_agent",
            usage: "",
            build: |_args, _ctx| Ok(Command::PauseAgent),
        },
        CommandSpec {
            name: "resume_agent",
            usage: "",
            build: |_args, _ctx| Ok(Command::ResumeAgent),
        },
        CommandSpec {
            name: "set_agent_fov",
            usage: "<vertical_angle:radians>",
            build: |args, ctx| {
                Ok(Command::SetAgentFov {
                    vertical_angle: args.req_parse(ctx, "vertical_angle", 0, "f32")?,
                })
            },
        },
        CommandSpec {
            name: "set_agent_size",
            usage: "<height:px> <width:px>",
            build: |args, ctx| {
                Ok(Command::SetAgentSize {
                    height: args.req_parse(ctx, "height", 0, "u16")?,
                    width: args.req_parse(ctx, "width", 1, "u16")?,
                })
            },
        },
        CommandSpec {
            name: "release_script_controls",
            usage: "",
            build: |_args, _ctx| Ok(Command::ReleaseScriptControls),
        },
        CommandSpec {
            name: "upload_asset_udp",
            usage: "<asset_type-code> data=<hex> [temp_file=] [store_local=]",
            build: |args, ctx| {
                Ok(Command::UploadAssetUdp {
                    asset_type: enum_arg(args, ctx, "asset_type", 0, parse_asset_type)?,
                    data: args.bytes_or_empty(ctx, "data", 100)?,
                    temp_file: args.bool_or(ctx, "temp_file", 101, false)?,
                    store_local: args.bool_or(ctx, "store_local", 102, false)?,
                })
            },
        },
        CommandSpec {
            name: "upload_asset",
            usage: "folder_id=<id> asset_type=<code> inventory_type=<code> name=<n> data=<hex> …",
            build: |args, ctx| {
                Ok(Command::UploadAsset {
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 100)?),
                    asset_type: enum_arg(args, ctx, "asset_type", 101, parse_asset_type)?,
                    inventory_type: enum_arg(
                        args,
                        ctx,
                        "inventory_type",
                        102,
                        parse_inventory_type,
                    )?,
                    name: args.str_or(ctx, "name", 103, "")?,
                    description: args.str_or(ctx, "description", 104, "")?,
                    next_owner_mask: args.parse_or(ctx, "next_owner_mask", 105, "u32", 0)?,
                    group_mask: args.parse_or(ctx, "group_mask", 106, "u32", 0)?,
                    everyone_mask: args.parse_or(ctx, "everyone_mask", 107, "u32", 0)?,
                    expected_upload_cost: args.parse_or(
                        ctx,
                        "expected_upload_cost",
                        108,
                        "i32",
                        0,
                    )?,
                    data: args.bytes_or_empty(ctx, "data", 109)?,
                })
            },
        },
        CommandSpec {
            name: "upload_baked_texture",
            usage: "data=<hex>",
            build: |args, ctx| {
                Ok(Command::UploadBakedTexture {
                    data: args.bytes_or_empty(ctx, "data", 0)?,
                })
            },
        },
        CommandSpec {
            name: "update_inventory_asset",
            usage: "<item_id> <asset_type-code> data=<hex>",
            build: |args, ctx| {
                Ok(Command::UpdateInventoryAsset {
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 0)?),
                    asset_type: enum_arg(args, ctx, "asset_type", 1, parse_asset_type)?,
                    data: args.bytes_or_empty(ctx, "data", 100)?,
                })
            },
        },
        CommandSpec {
            name: "request_object_media",
            usage: "<object_id>",
            build: |args, ctx| {
                Ok(Command::RequestObjectMedia {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                })
            },
        },
        CommandSpec {
            name: "set_object_media",
            usage: "<object_id> [clear=N | home_url=<url> …]",
            build: |args, ctx| {
                let faces = if args
                    .keyword()
                    .keys()
                    .any(|key| key != "object_id" && key != "clear")
                {
                    vec![Some(build_media_entry(args, ctx)?)]
                } else {
                    let count = args.parse_or::<u32>(ctx, "clear", 100, "u32", 0)?;
                    let count = usize::try_from(count)
                        .ok()
                        .ok_or_else(|| invalid("clear", "count", "usize"))?;
                    let mut faces = Vec::new();
                    faces.resize(count, None);
                    faces
                };
                Ok(Command::SetObjectMedia {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    faces,
                })
            },
        },
        CommandSpec {
            name: "navigate_object_media",
            usage: "<object_id> <face> <url>",
            build: |args, ctx| {
                Ok(Command::NavigateObjectMedia {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    face: args.req_parse(ctx, "face", 1, "u8")?,
                    url: args.req_str(ctx, "url", 2)?,
                })
            },
        },
        CommandSpec {
            name: "request_render_materials",
            usage: "<material_id,material_id,…>",
            build: |args, ctx| {
                Ok(Command::RequestRenderMaterials {
                    material_ids: args.vec_uuid(ctx, "material_ids", 0)?,
                })
            },
        },
        CommandSpec {
            name: "modify_material_params",
            usage: "object_id=<id> [side=-1] [gltf_json=] [asset_id=]",
            build: |args, ctx| {
                let update = MaterialOverrideUpdate {
                    object_id: args.req_object(ctx, "object_id", 0)?,
                    side: args.parse_or(ctx, "side", 1, "i32", -1)?,
                    gltf_json: args.opt_str(ctx, "gltf_json", 2)?,
                    asset_id: match args.opt_str(ctx, "asset_id", 3)? {
                        Some(value) => Some(args::literal_uuid("asset_id", &value)?),
                        None => None,
                    },
                };
                Ok(Command::ModifyMaterialParams {
                    updates: vec![update],
                })
            },
        },
        CommandSpec {
            name: "request_voice_account",
            usage: "[logout=] [jsep_offer_sdp=] [channel_type=] …",
            build: |args, ctx| {
                Ok(Command::RequestVoiceAccount {
                    request: build_voice_request(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "request_parcel_voice_info",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestParcelVoiceInfo),
        },
        CommandSpec {
            name: "send_voice_signaling",
            usage: "<viewer_session> [completed=true] [sdp_mid= sdp_mline_index= candidate=]",
            build: |args, ctx| {
                let mut candidates = Vec::new();
                if let Some(candidate) = args.opt_str(ctx, "candidate", 100)? {
                    candidates.push(sl_proto::IceCandidate {
                        sdp_mid: args.str_or(ctx, "sdp_mid", 101, "")?,
                        sdp_mline_index: args.parse_or(ctx, "sdp_mline_index", 102, "i32", 0)?,
                        candidate,
                    });
                }
                Ok(Command::SendVoiceSignaling {
                    viewer_session: args.req_str(ctx, "viewer_session", 0)?,
                    candidates,
                    completed: args.bool_or(ctx, "completed", 1, true)?,
                })
            },
        },
        CommandSpec {
            name: "request_experience_info",
            usage: "<experience_id,experience_id,…>",
            build: |args, ctx| {
                Ok(Command::RequestExperienceInfo {
                    experience_ids: args
                        .vec_uuid(ctx, "experience_ids", 0)?
                        .into_iter()
                        .map(ExperienceKey::from)
                        .collect(),
                })
            },
        },
        CommandSpec {
            name: "find_experiences",
            usage: "<query> [page=0]",
            build: |args, ctx| {
                Ok(Command::FindExperiences {
                    query: args.req_str(ctx, "query", 0)?,
                    page: args.parse_or(ctx, "page", 1, "i32", 0)?,
                })
            },
        },
        CommandSpec {
            name: "request_experience_permissions",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestExperiencePermissions),
        },
        CommandSpec {
            name: "set_experience_permission",
            usage: "<experience_id> <allow|block|forget>",
            build: |args, ctx| {
                Ok(Command::SetExperiencePermission {
                    experience_id: ExperienceKey::from(args.req_uuid(ctx, "experience_id", 0)?),
                    permission: enum_arg(args, ctx, "permission", 1, parse_experience_permission)?,
                })
            },
        },
        CommandSpec {
            name: "request_owned_experiences",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestOwnedExperiences),
        },
        CommandSpec {
            name: "request_admin_experiences",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestAdminExperiences),
        },
        CommandSpec {
            name: "request_creator_experiences",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestCreatorExperiences),
        },
        CommandSpec {
            name: "request_group_experiences",
            usage: "<group_id>",
            build: |args, ctx| {
                Ok(Command::RequestGroupExperiences {
                    group_id: GroupKey::from(args.req_uuid(ctx, "group_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "request_experience_admin",
            usage: "<experience_id>",
            build: |args, ctx| {
                Ok(Command::RequestExperienceAdmin {
                    experience_id: ExperienceKey::from(args.req_uuid(ctx, "experience_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "request_experience_contributor",
            usage: "<experience_id>",
            build: |args, ctx| {
                Ok(Command::RequestExperienceContributor {
                    experience_id: ExperienceKey::from(args.req_uuid(ctx, "experience_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "update_experience",
            usage: "public_id=<id> [name=] [description=] …",
            build: |args, ctx| {
                Ok(Command::UpdateExperience {
                    update: build_experience_update(args, ctx)?,
                })
            },
        },
        CommandSpec {
            name: "request_region_experiences",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestRegionExperiences),
        },
        CommandSpec {
            name: "set_region_experiences",
            usage: "[allowed=…] [blocked=…] [trusted=…]",
            build: |args, ctx| {
                Ok(Command::SetRegionExperiences {
                    allowed: args
                        .vec_uuid(ctx, "allowed", 0)?
                        .into_iter()
                        .map(ExperienceKey::from)
                        .collect(),
                    blocked: args
                        .vec_uuid(ctx, "blocked", 1)?
                        .into_iter()
                        .map(ExperienceKey::from)
                        .collect(),
                    trusted: args
                        .vec_uuid(ctx, "trusted", 2)?
                        .into_iter()
                        .map(ExperienceKey::from)
                        .collect(),
                })
            },
        },
        CommandSpec {
            name: "offer_teleport",
            usage: "<target,target,…> [message]",
            build: |args, ctx| {
                Ok(Command::OfferTeleport {
                    targets: args.vec_agent(ctx, "targets", 0)?,
                    message: args.str_or(ctx, "message", 1, "")?,
                })
            },
        },
        CommandSpec {
            name: "accept_teleport_lure",
            usage: "<lure_id>",
            build: |args, ctx| {
                Ok(Command::AcceptTeleportLure {
                    lure_id: LureId::from(args.req_uuid(ctx, "lure_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "decline_teleport_lure",
            usage: "<from_agent_id> <lure_id>",
            build: |args, ctx| {
                Ok(Command::DeclineTeleportLure {
                    from_agent_id: AgentKey::from(args.req_uuid(ctx, "from_agent_id", 0)?),
                    lure_id: LureId::from(args.req_uuid(ctx, "lure_id", 1)?),
                })
            },
        },
        CommandSpec {
            name: "request_teleport",
            usage: "<to_agent_id> [message]",
            build: |args, ctx| {
                Ok(Command::RequestTeleport {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    message: args.str_or(ctx, "message", 1, "")?,
                })
            },
        },
        CommandSpec {
            name: "give_inventory",
            usage: "<to_agent_id> <item_id> <asset_type-code> <item_name> [transaction_id]",
            build: |args, ctx| {
                Ok(Command::GiveInventory {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    item_id: InventoryKey::from(args.req_uuid(ctx, "item_id", 1)?),
                    asset_type: enum_arg(args, ctx, "asset_type", 2, parse_asset_type)?,
                    item_name: args.str_or(ctx, "item_name", 3, "")?,
                    transaction_id: TransactionId::from(args.uuid_or_nil(
                        ctx,
                        "transaction_id",
                        4,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "give_inventory_folder",
            usage: "<to_agent_id> <folder_id> <folder_name> [transaction_id]",
            build: |args, ctx| {
                Ok(Command::GiveInventoryFolder {
                    to_agent_id: AgentKey::from(args.req_uuid(ctx, "to_agent_id", 0)?),
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 1)?),
                    folder_name: args.str_or(ctx, "folder_name", 2, "")?,
                    transaction_id: TransactionId::from(args.uuid_or_nil(
                        ctx,
                        "transaction_id",
                        3,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "accept_inventory_offer",
            usage: "asset_type=<code> item_id=<id> folder_id=<id> …",
            build: |args, ctx| {
                Ok(Command::AcceptInventoryOffer {
                    offer: build_inventory_offer(args, ctx)?,
                    folder_id: InventoryFolderKey::from(args.req_uuid(ctx, "folder_id", 100)?),
                })
            },
        },
        CommandSpec {
            name: "decline_inventory_offer",
            usage: "asset_type=<code> item_id=<id> trash_folder_id=<id> …",
            build: |args, ctx| {
                Ok(Command::DeclineInventoryOffer {
                    offer: build_inventory_offer(args, ctx)?,
                    trash_folder_id: InventoryFolderKey::from(args.req_uuid(
                        ctx,
                        "trash_folder_id",
                        100,
                    )?),
                })
            },
        },
        CommandSpec {
            name: "start_conference",
            usage: "<session_id> <invitee,invitee,…> [message]",
            build: |args, ctx| {
                Ok(Command::StartConference {
                    session_id: ImSessionId::from(args.req_uuid(ctx, "session_id", 0)?),
                    invitees: args
                        .vec_uuid(ctx, "invitees", 1)?
                        .into_iter()
                        .map(AgentKey::from)
                        .collect(),
                    message: args.str_or(ctx, "message", 2, "")?,
                })
            },
        },
        CommandSpec {
            name: "send_conference_message",
            usage: "<session_id> <message>",
            build: |args, ctx| {
                Ok(Command::SendConferenceMessage {
                    session_id: ImSessionId::from(args.req_uuid(ctx, "session_id", 0)?),
                    message: args.req_str(ctx, "message", 1)?,
                })
            },
        },
        CommandSpec {
            name: "leave_conference",
            usage: "<session_id>",
            build: |args, ctx| {
                Ok(Command::LeaveConference {
                    session_id: ImSessionId::from(args.req_uuid(ctx, "session_id", 0)?),
                })
            },
        },
        CommandSpec {
            name: "retrieve_instant_messages",
            usage: "",
            build: |_args, _ctx| Ok(Command::RetrieveInstantMessages),
        },
        CommandSpec {
            name: "request_offline_messages",
            usage: "",
            build: |_args, _ctx| Ok(Command::RequestOfflineMessages),
        },
        CommandSpec {
            name: "logout",
            usage: "",
            build: |_args, _ctx| Ok(Command::Logout),
        },
    ]
}

/// Parse a [`GroupRoleUpdateType`](sl_proto::GroupRoleUpdateType) from its name
/// or wire code.
fn parse_group_role_update_type(
    field: &str,
    value: &str,
) -> Result<sl_proto::GroupRoleUpdateType, ReplError> {
    use sl_proto::GroupRoleUpdateType;
    Ok(match norm(value).as_str() {
        "noupdate" | "0" => GroupRoleUpdateType::NoUpdate,
        "updatedata" | "1" => GroupRoleUpdateType::UpdateData,
        "updatepowers" | "2" => GroupRoleUpdateType::UpdatePowers,
        "updateall" | "3" => GroupRoleUpdateType::UpdateAll,
        "create" | "4" => GroupRoleUpdateType::Create,
        "delete" | "5" => GroupRoleUpdateType::Delete,
        _ => return Err(invalid(field, value, "role update type")),
    })
}

/// Parse a [`ClickAction`](sl_proto::ClickAction) from its name or wire code.
fn parse_click_action(field: &str, value: &str) -> Result<sl_proto::ClickAction, ReplError> {
    use sl_proto::ClickAction;
    Ok(match norm(value).as_str() {
        "touch" => ClickAction::Touch,
        "sit" => ClickAction::Sit,
        "buy" => ClickAction::Buy,
        "pay" => ClickAction::Pay,
        "open" => ClickAction::Open,
        "play" => ClickAction::Play,
        "openmedia" => ClickAction::OpenMedia,
        "zoom" => ClickAction::Zoom,
        "disabled" => ClickAction::Disabled,
        "ignore" => ClickAction::Ignore,
        _ => ClickAction::from_code(
            value
                .parse::<u8>()
                .ok()
                .ok_or_else(|| invalid(field, value, "click action"))?,
        ),
    })
}

/// Parse an optional `(start, end)` byte range from the `start`/`end` keywords.
fn byte_range(args: &Args, ctx: &dyn ReplContext) -> Result<Option<(u32, u32)>, ReplError> {
    match (
        args.opt_str(ctx, "start", 200)?,
        args.opt_str(ctx, "end", 201)?,
    ) {
        (Some(start), Some(end)) => Ok(Some((
            args::literal::<u32>("start", &start, "u32")?,
            args::literal::<u32>("end", &end, "u32")?,
        ))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sl_proto::{
        AbuseReportType, AgentKey, AgentPreferences, AssetType, ChatChannel, ChatType, CircuitId,
        Command, ControlFlags, FriendRights, GroupKey, InventoryKey, LandStatReportType,
        LindenAmount, MapItemType, MovementMode, ObjectBuyItem, ObjectKey, OwnerKey, RegionHandle,
        RegionLocalObjectId, RegionLocalParcelId, SaleType, ScopedObjectId, ScopedParcelId,
        TransactionId, Uuid,
    };

    use super::Registry;
    use crate::context::{NoContext, ReplContext};
    use crate::error::ReplError;
    use crate::parse::{ReplAction, parse_line};

    /// A [`ReplContext`] backed by a fixed placeholder map.
    struct MapContext(BTreeMap<String, String>);

    impl ReplContext for MapContext {
        fn resolve_placeholder(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    /// A test UUID (nil on a malformed literal, which the tests never pass).
    fn uuid(text: &str) -> Uuid {
        Uuid::parse_str(text).unwrap_or_else(|_unused| Uuid::nil())
    }

    /// Parse and build a command line against `ctx`.
    fn build_ctx(line: &str, ctx: &dyn ReplContext) -> Result<Command, ReplError> {
        match parse_line(line) {
            Ok(Some(ReplAction::Command(pending))) => Registry::new().build(&pending, ctx),
            Ok(_) => Err(ReplError::UnknownCommand("not-a-command".to_owned())),
            Err(error) => Err(error),
        }
    }

    /// Parse and build a command line with no placeholder context.
    fn build(line: &str) -> Result<Command, ReplError> {
        build_ctx(line, &NoContext)
    }

    /// The fixed circuit the scoped-id tests build their commands against.
    const TEST_CIRCUIT: CircuitId = CircuitId::new(7);

    /// A [`ReplContext`] that resolves no placeholders but reports a fixed
    /// [`CircuitId`], so object/parcel commands scope their region-local ids.
    struct CircuitContext;

    impl ReplContext for CircuitContext {
        fn resolve_placeholder(&self, _name: &str) -> Option<String> {
            None
        }

        fn current_circuit_id(&self) -> Option<CircuitId> {
            Some(TEST_CIRCUIT)
        }
    }

    /// Parse and build a command line against a fixed-circuit context.
    fn build_scoped(line: &str) -> Result<Command, ReplError> {
        build_ctx(line, &CircuitContext)
    }

    const ONE: &str = "11111111-1111-1111-1111-111111111111";
    const TWO: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn every_spec_name_is_unique_and_resolvable() {
        let registry = Registry::new();
        let mut seen = std::collections::BTreeSet::new();
        for spec in registry.specs() {
            assert!(
                seen.insert(spec.name),
                "duplicate command name {}",
                spec.name
            );
            assert!(registry.spec(spec.name).is_some());
        }
    }

    #[test]
    fn string_and_enum_by_name_and_i32() {
        assert!(matches!(
            build(r#"chat "hi there" shout 5"#),
            Ok(Command::Chat { message, chat_type: ChatType::Shout, channel: ChatChannel(5) }) if message == "hi there"
        ));
    }

    #[test]
    fn bool_argument() {
        assert!(matches!(build("typing true"), Ok(Command::Typing(true))));
    }

    #[test]
    fn signed_integer_argument() {
        assert!(matches!(
            build("restart_region -1"),
            Ok(Command::RestartRegion { seconds: -1 })
        ));
    }

    #[test]
    fn unsigned_integer_argument() {
        assert!(matches!(
            build_scoped("touch_object 42"),
            Ok(Command::TouchObject {
                local_id: ScopedObjectId {
                    circuit: TEST_CIRCUIT,
                    id: RegionLocalObjectId(42)
                }
            })
        ));
    }

    #[test]
    fn u64_argument() {
        assert!(matches!(
            build("teleport 123456789 <1,2,3> <0,0,0>"),
            Ok(Command::Teleport {
                region_handle: RegionHandle(123_456_789),
                ..
            })
        ));
    }

    #[test]
    fn f32_argument() {
        assert!(matches!(
            build("set_draw_distance 128.5"),
            Ok(Command::SetDrawDistance(d)) if d.meters().to_bits() == f64::from(128.5_f32).to_bits()
        ));
    }

    #[test]
    fn f64_argument() {
        assert!(matches!(
            build("autopilot 1.5 2.0 3.0"),
            Ok(Command::Autopilot { global_x, .. }) if global_x.to_bits() == 1.5_f64.to_bits()
        ));
    }

    #[test]
    fn i8_argument() {
        assert!(matches!(
            build(&format!("create_inventory_folder {ONE} {TWO} -1 Stuff")),
            Ok(Command::CreateInventoryFolder {
                folder_type: -1,
                ..
            })
        ));
    }

    #[test]
    fn u8_argument() {
        assert!(matches!(
            build(&format!(
                "navigate_object_media {ONE} 3 http://example.test"
            )),
            Ok(Command::NavigateObjectMedia { face: 3, .. })
        ));
    }

    #[test]
    fn uuid_argument() {
        assert!(matches!(
            build(&format!("delete_pick {ONE}")),
            Ok(Command::DeletePick(id)) if id.uuid() == uuid(ONE)
        ));
    }

    #[test]
    fn vector_argument() {
        assert!(matches!(
            build(&format!("sit {ONE} <1,2,3>")),
            Ok(Command::Sit { offset, .. })
                if offset.x.to_bits() == 1.0_f32.to_bits() && offset.z.to_bits() == 3.0_f32.to_bits()
        ));
    }

    #[test]
    fn rotation_argument() {
        assert!(matches!(
            build("set_rotation <0,0,0,1> <0,0,0,1>"),
            Ok(Command::SetRotation { body, .. }) if body.s.to_bits() == 1.0_f32.to_bits()
        ));
    }

    #[test]
    fn vec_uuid_argument() {
        assert!(matches!(
            build(&format!("fetch_inventory_folders {ONE},{TWO}")),
            Ok(Command::FetchInventoryFolders(ids)) if ids.len() == 2
        ));
    }

    #[test]
    fn vec_u32_argument() {
        assert!(matches!(
            build_scoped("request_objects 1,2,3"),
            Ok(Command::RequestObjects { local_ids }) if local_ids.len() == 3
        ));
    }

    #[test]
    fn enum_by_numeric_code() {
        assert!(matches!(
            build("request_map_items 7"),
            Ok(Command::RequestMapItems {
                item_type: MapItemType::LandForSale,
                ..
            })
        ));
    }

    #[test]
    fn request_map_layer_takes_no_args() {
        assert!(matches!(
            build("request_map_layer"),
            Ok(Command::RequestMapLayer)
        ));
    }

    #[test]
    fn abuse_report_parses_summary_and_abuser() {
        assert!(matches!(
            build(&format!("send_abuse_report Griefing {ONE} TestRegion 66 bug")),
            Ok(Command::SendAbuseReport(report))
                if report.summary == "Griefing"
                    && report.abuser_id == uuid(ONE)
                    && report.abuse_region_name
                        == sl_proto::region_name_from_wire("region_name", "TestRegion").ok().flatten()
                    && report.category == 66
                    && report.report_type == AbuseReportType::Bug
        ));
    }

    #[test]
    fn abuse_report_caps_uses_caps_command() {
        assert!(matches!(
            build("send_abuse_report_caps Griefing"),
            Ok(Command::SendAbuseReportViaCaps { report, screenshot: None })
                if report.summary == "Griefing"
        ));
    }

    #[test]
    fn abuse_report_caps_parses_screenshot() {
        assert!(matches!(
            build("send_abuse_report_caps Griefing screenshot=deadbeef"),
            Ok(Command::SendAbuseReportViaCaps { report, screenshot: Some(bytes) })
                if report.summary == "Griefing" && bytes == [0xde, 0xad, 0xbe, 0xef]
        ));
    }

    #[test]
    fn postcard_parses_asset_and_recipient() {
        assert!(matches!(
            build(&format!("send_postcard {ONE} friend@example.com Subject")),
            Ok(Command::SendPostcard(postcard))
                if postcard.asset_id == uuid(ONE)
                    && postcard.to == "friend@example.com"
                    && postcard.subject == "Subject"
        ));
    }

    #[test]
    fn code_enum_accepts_name() {
        // `asset_type` was a code-only enum; it now also accepts its name
        // (underscores optional), with the numeric code still working.
        assert!(matches!(
            build(&format!("request_asset {ONE} texture")),
            Ok(Command::RequestAsset {
                asset_type: AssetType::Texture,
                ..
            })
        ));
        assert!(matches!(
            build(&format!("request_asset {ONE} lsl_text")),
            Ok(Command::RequestAsset {
                asset_type: AssetType::LslText,
                ..
            })
        ));
        assert!(matches!(
            build(&format!("request_asset {ONE} 0")),
            Ok(Command::RequestAsset {
                asset_type: AssetType::Texture,
                ..
            })
        ));
    }

    #[test]
    fn flag_newtype_argument() {
        assert!(matches!(
            build(&format!("grant_user_rights {ONE} 3")),
            Ok(Command::GrantUserRights {
                rights: FriendRights(3),
                ..
            })
        ));
    }

    #[test]
    fn control_flags_from_bits() {
        assert!(matches!(
            build("set_controls 8192"),
            Ok(Command::SetControls(flags)) if flags.bits() == ControlFlags::FLY.bits()
        ));
    }

    #[test]
    fn hex_bytes_argument() {
        assert!(matches!(
            build("upload_baked_texture data=deadbeef"),
            Ok(Command::UploadBakedTexture { data }) if data == vec![0xde, 0xad, 0xbe, 0xef]
        ));
    }

    #[test]
    fn record_list_argument() {
        assert!(matches!(
            build(&format!("set_animations {ONE}:true,{TWO}:false")),
            Ok(Command::SetAnimations(pairs)) if pairs.len() == 2
        ));
    }

    #[test]
    fn activate_gestures_pairs() {
        let one = Uuid::parse_str(ONE).ok();
        let two = Uuid::parse_str(TWO).ok();
        assert!(matches!(
            build(&format!("activate_gestures {ONE}:{TWO}")),
            Ok(Command::ActivateGestures { gestures })
                if gestures.first().map(|g| g.item_id) == one.map(InventoryKey::from)
                    && gestures.first().map(|g| g.asset_id) == two
        ));
    }

    #[test]
    fn deactivate_gestures_uuid_list() {
        assert!(matches!(
            build(&format!("deactivate_gestures {ONE},{TWO}")),
            Ok(Command::DeactivateGestures { item_ids }) if item_ids.len() == 2
        ));
    }

    #[test]
    fn set_always_run_mode() {
        assert!(matches!(
            build("set_always_run run"),
            Ok(Command::SetAlwaysRun {
                mode: MovementMode::AlwaysRun
            })
        ));
        assert!(matches!(
            build("set_always_run walk"),
            Ok(Command::SetAlwaysRun {
                mode: MovementMode::Walk
            })
        ));
        // The legacy boolean spelling still parses.
        assert!(matches!(
            build("set_always_run true"),
            Ok(Command::SetAlwaysRun {
                mode: MovementMode::AlwaysRun
            })
        ));
    }

    #[test]
    fn pause_resume_agent_no_args() {
        assert!(matches!(build("pause_agent"), Ok(Command::PauseAgent)));
        assert!(matches!(build("resume_agent"), Ok(Command::ResumeAgent)));
    }

    #[test]
    fn release_script_controls_no_args() {
        assert!(matches!(
            build("release_script_controls"),
            Ok(Command::ReleaseScriptControls)
        ));
    }

    #[test]
    fn set_agent_fov_radians() {
        assert!(matches!(
            build("set_agent_fov 1.25"),
            Ok(Command::SetAgentFov { vertical_angle }) if vertical_angle.to_bits() == 1.25_f32.to_bits()
        ));
    }

    #[test]
    fn set_agent_size_dimensions() {
        assert!(matches!(
            build("set_agent_size 768 1024"),
            Ok(Command::SetAgentSize {
                height: 768,
                width: 1024
            })
        ));
    }

    #[test]
    fn keyword_struct_fields() {
        assert!(matches!(
            build(r#"create_group name="My Group" open_enrollment=true"#),
            Ok(Command::CreateGroup(params))
                if params.name == "My Group" && params.open_enrollment
        ));
    }

    #[test]
    fn placeholder_resolution_at_build_time() {
        let mut map = BTreeMap::new();
        drop(map.insert("self".to_owned(), ONE.to_owned()));
        let ctx = MapContext(map);
        assert!(matches!(
            build_ctx("delete_pick $self", &ctx),
            Ok(Command::DeletePick(id)) if id.uuid() == uuid(ONE)
        ));
    }

    #[test]
    fn unknown_command_errors() {
        assert!(matches!(
            build("frobnicate"),
            Err(ReplError::UnknownCommand(_))
        ));
    }

    #[test]
    fn send_is_not_supported() {
        assert!(matches!(
            build("send"),
            Err(ReplError::NotSupported("send", _))
        ));
    }

    #[test]
    fn missing_required_argument_errors() {
        assert!(matches!(build("sit"), Err(ReplError::MissingArg { .. })));
    }

    #[test]
    fn buy_object_parses_records() {
        assert!(matches!(
            build(&format!("buy_object {ONE} {TWO} 99:copy:250")),
            Ok(Command::BuyObject { objects, .. })
                if matches!(
                    objects.first(),
                    Some(ObjectBuyItem { local_id: RegionLocalObjectId(99), sale_type: SaleType::Copy, sale_price: LindenAmount(250) })
                )
        ));
    }

    #[test]
    fn request_object_properties_family_keyword_flags() {
        assert!(matches!(
            build(&format!("request_object_properties_family {ONE} request_flags=4")),
            Ok(Command::RequestObjectPropertiesFamily { request_flags: 4, object_id })
                if object_id == ObjectKey::from(uuid(ONE))
        ));
    }

    #[test]
    fn spin_object_update_parses_rotation() {
        assert!(matches!(
            build(&format!("spin_object_update {ONE} <0,0,0,1>")),
            Ok(Command::SpinObjectUpdate { object_id, .. }) if object_id == ObjectKey::from(uuid(ONE))
        ));
    }

    #[test]
    fn rez_object_from_notecard_parses() {
        assert!(matches!(
            build(&format!("rez_object_from_notecard {ONE} <1,2,3> <4,5,6> {TWO}")),
            Ok(Command::RezObjectFromNotecard { rez })
                if rez.notecard_item_id == InventoryKey::from(uuid(ONE))
                    && rez.item_ids == vec![InventoryKey::from(uuid(TWO))]
        ));
    }

    #[test]
    fn join_parcels_parses_rectangle() {
        assert!(matches!(
            build("join_parcels 16 32 48 64"),
            Ok(Command::JoinParcels { west, south, east, north })
                if west.to_bits() == 16.0_f32.to_bits()
                    && south.to_bits() == 32.0_f32.to_bits()
                    && east.to_bits() == 48.0_f32.to_bits()
                    && north.to_bits() == 64.0_f32.to_bits()
        ));
    }

    #[test]
    fn disable_parcel_objects_parses_scope() {
        assert!(matches!(
            build_scoped(&format!("disable_parcel_objects 7 8 owner_ids={ONE} task_ids={TWO}")),
            Ok(Command::DisableParcelObjects { local_id: ScopedParcelId { circuit: TEST_CIRCUIT, id: RegionLocalParcelId(7) }, return_type, owner_ids, task_ids })
                if return_type.0 == 8 && owner_ids == vec![OwnerKey::Agent(AgentKey::from(uuid(ONE)))] && task_ids == vec![ObjectKey::from(uuid(TWO))]
        ));
    }

    #[test]
    fn request_parcel_info_parses_id() {
        assert!(matches!(
            build(&format!("request_parcel_info {ONE}")),
            Ok(Command::RequestParcelInfo { parcel_id }) if parcel_id.uuid() == uuid(ONE)
        ));
    }

    #[test]
    fn request_remote_parcel_id_parses_location_and_handle() {
        assert!(matches!(
            build("request_remote_parcel_id <128,64,22> 00000000-0000-0000-0000-000000000000 281483566841976"),
            Ok(Command::RequestRemoteParcelId { location, region_id, region_handle })
                if location.x().to_bits() == 128.0_f32.to_bits()
                    && region_id == Uuid::nil()
                    && region_handle == RegionHandle(281_483_566_841_976)
        ));
    }

    #[test]
    fn request_simulator_features_parses() {
        assert!(matches!(
            build("request_simulator_features"),
            Ok(Command::RequestSimulatorFeatures)
        ));
    }

    #[test]
    fn request_agent_preferences_parses() {
        assert!(matches!(
            build("request_agent_preferences"),
            Ok(Command::RequestAgentPreferences)
        ));
    }

    #[test]
    fn set_agent_preferences_parses_keyword_fields() {
        assert!(matches!(
            build(
                "set_agent_preferences hover_height=0.5 perm_next_owner=532480 \
                 max_access=M language=en-us language_is_public=true",
            ),
            Ok(Command::SetAgentPreferences(prefs))
                if prefs.hover_height.map(f64::to_bits) == Some(0.5_f64.to_bits())
                    && prefs.default_object_perm_masks.map(|masks| masks.next_owner)
                        == Some(532_480)
                    && prefs.max_access_pref.as_deref() == Some("M")
                    && prefs.language.as_deref() == Some("en-us")
                    && prefs.language_is_public == Some(true)
        ));
    }

    #[test]
    fn set_agent_preferences_empty_is_a_get() {
        assert!(matches!(
            build("set_agent_preferences"),
            Ok(Command::SetAgentPreferences(prefs)) if *prefs == AgentPreferences::default()
        ));
    }

    #[test]
    fn request_object_cost_parses_ids() {
        assert!(matches!(
            build(&format!("request_object_cost {ONE}")),
            Ok(Command::RequestObjectCost { object_ids }) if object_ids == vec![ObjectKey::from(uuid(ONE))]
        ));
    }

    #[test]
    fn request_selected_cost_parses_ids_and_roots() {
        assert!(matches!(
            build(&format!("request_selected_cost {ONE} false")),
            Ok(Command::RequestSelectedCost { object_ids, roots })
                if object_ids == vec![ObjectKey::from(uuid(ONE))] && !roots
        ));
    }

    #[test]
    fn request_selected_cost_defaults_to_roots() {
        assert!(matches!(
            build(&format!("request_selected_cost {ONE}")),
            Ok(Command::RequestSelectedCost { roots, .. }) if roots
        ));
    }

    #[test]
    fn request_object_physics_data_parses_ids() {
        assert!(matches!(
            build(&format!("request_object_physics_data {ONE}")),
            Ok(Command::RequestObjectPhysicsData { object_ids }) if object_ids == vec![ObjectKey::from(uuid(ONE))]
        ));
    }

    #[test]
    fn request_attachment_resources_parses() {
        assert!(matches!(
            build("request_attachment_resources"),
            Ok(Command::RequestAttachmentResources)
        ));
    }

    #[test]
    fn request_land_resources_parses_parcel_id() {
        assert!(matches!(
            build(&format!("request_land_resources {ONE}")),
            Ok(Command::RequestLandResources { parcel_id }) if parcel_id.uuid() == uuid(ONE)
        ));
    }

    #[test]
    fn request_land_stat_parses_report_type_and_scope() {
        assert!(matches!(
            build_scoped("request_land_stat colliders 0 spammer 3"),
            Ok(Command::RequestLandStat { report_type, filter, parcel_local_id, .. })
                if report_type == LandStatReportType::TopColliders
                    && filter == "spammer"
                    && parcel_local_id == ScopedParcelId::new(TEST_CIRCUIT, RegionLocalParcelId(3))
        ));
    }

    #[test]
    fn request_land_stat_defaults_to_top_scripts() {
        assert!(matches!(
            build_scoped("request_land_stat"),
            Ok(Command::RequestLandStat { report_type, parcel_local_id, .. })
                if report_type == LandStatReportType::TopScripts
                    && parcel_local_id == ScopedParcelId::new(TEST_CIRCUIT, RegionLocalParcelId(0))
        ));
    }

    #[test]
    fn request_estate_covenant_parses() {
        assert!(matches!(
            build("request_estate_covenant"),
            Ok(Command::RequestEstateCovenant)
        ));
    }

    #[test]
    fn telehub_info_request_parses() {
        assert!(matches!(
            build("request_telehub_info"),
            Ok(Command::RequestTelehubInfo)
        ));
    }

    #[test]
    fn connect_telehub_parses_local_id() {
        assert!(matches!(
            build_scoped("connect_telehub 42"),
            Ok(Command::ConnectTelehub {
                object_local_id: ScopedObjectId {
                    circuit: TEST_CIRCUIT,
                    id: RegionLocalObjectId(42)
                }
            })
        ));
    }

    #[test]
    fn remove_telehub_spawn_point_parses_index() {
        assert!(matches!(
            build("remove_telehub_spawn_point 2"),
            Ok(Command::RemoveTelehubSpawnPoint { spawn_index: 2 })
        ));
    }

    #[test]
    fn request_script_running_parses() {
        assert!(matches!(
            build(
                "request_script_running \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222"
            ),
            Ok(Command::RequestScriptRunning { object_id, item_id })
                if object_id == ObjectKey::from(Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111))
                    && item_id == InventoryKey::from(Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222))
        ));
    }

    #[test]
    fn set_script_running_parses_running_flag() {
        assert!(matches!(
            build(
                "set_script_running \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222 true"
            ),
            Ok(Command::SetScriptRunning { running: true, .. })
        ));
    }

    #[test]
    fn reset_script_parses() {
        assert!(matches!(
            build(
                "reset_script \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222"
            ),
            Ok(Command::ResetScript { object_id, item_id })
                if object_id == ObjectKey::from(Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111))
                    && item_id == InventoryKey::from(Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222))
        ));
    }

    #[test]
    fn request_group_account_summary_parses() {
        assert!(matches!(
            build(
                "request_group_account_summary \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222 60 1"
            ),
            Ok(Command::RequestGroupAccountSummary {
                group_id,
                interval_days: 60,
                current_interval: 1,
                ..
            }) if group_id == GroupKey::from(Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111))
        ));
    }

    #[test]
    fn request_group_active_proposals_parses() {
        assert!(matches!(
            build(
                "request_group_active_proposals \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222"
            ),
            Ok(Command::RequestGroupActiveProposals { group_id, transaction_id })
                if group_id == GroupKey::from(Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111))
                    && transaction_id == TransactionId::from(Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222))
        ));
    }

    #[test]
    fn start_group_proposal_parses() {
        assert!(matches!(
            build(
                "start_group_proposal \
                 11111111-1111-1111-1111-111111111111 \
                 3 0.5 86400 \"Adopt the bylaws?\""
            ),
            Ok(Command::StartGroupProposal {
                quorum: 3,
                duration: 86_400,
                proposal_text,
                ..
            }) if proposal_text == "Adopt the bylaws?"
        ));
    }

    #[test]
    fn group_proposal_ballot_parses() {
        assert!(matches!(
            build(
                "group_proposal_ballot \
                 11111111-1111-1111-1111-111111111111 \
                 22222222-2222-2222-2222-222222222222 yes"
            ),
            Ok(Command::GroupProposalBallot { proposal_id, group_id, vote_cast })
                if proposal_id.uuid() == Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111)
                    && group_id == GroupKey::from(Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222))
                    && vote_cast == "yes"
        ));
    }
}
