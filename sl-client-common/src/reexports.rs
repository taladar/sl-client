//! The one list of library items both runtimes re-export.
//!
//! `sl-client-tokio` and `sl-client-bevy` each promise the core types a
//! consumer needs, so a consumer can depend on either runtime alone. They used
//! to carry a list each, and the two drifted: every directory-search result was
//! exported by one and not the other, the avatar and geometry crates by one
//! only, and so on. [`runtime_reexports!`](crate::runtime_reexports) is that list,
//! written once, and each runtime expands it at its crate root.
//!
//! It is a macro rather than a module each runtime glob-imports because the
//! Bevy runtime also glob-imports `bevy::prelude`: two globs that both name
//! `Camera`, `Color` or `Texture` make the name ambiguous, while the explicit
//! `use` items a macro expands to shadow the prelude as a hand-written list
//! does.
//!
//! A runtime's own items (its fetchers, its plugin, its mesh conversions) stay
//! in that runtime's `lib.rs`; anything that comes from a library crate
//! belongs here.

/// The library crates [`runtime_reexports!`](crate::runtime_reexports) names,
/// reachable through this crate so the runtime expanding it needs no
/// dependency of its own on each of them.
#[doc(hidden)]
pub mod __reexport {
    pub use sl_asset;
    pub use sl_avatar;
    pub use sl_bake;
    pub use sl_material;
    pub use sl_mesh;
    pub use sl_prim;
    pub use sl_proto;
    pub use sl_sculpt;
    pub use sl_texture;
    pub use sl_tree;
}

/// Expands to the `pub use` items of every library type and function a runtime
/// re-exports. Invoke it once at a runtime's crate root:
///
/// ```ignore
/// sl_client_common::runtime_reexports!();
/// ```
#[macro_export]
macro_rules! runtime_reexports {
    () => {
        // The shared halves of the runtimes themselves.
        pub use $crate::avatar_dirs::{AccountDirsConfig, resolve_account_directories};
        pub use $crate::agent_parcel::AgentParcel;
        pub use $crate::parcel_overlays::ParcelOverlays;
        // The protocol core: keys, commands, events, the wire-level data types, the
        // capability names and the request builders a consumer drives a session with.
        // `Event` is not here: the Bevy runtime names it `SlSessionEvent` so it does
        // not shadow Bevy's own `Event`, and the tokio runtime names it `Event`.
        pub use $crate::reexports::__reexport::sl_proto::{
            ASSET_CODE_LINK, ASSET_CODE_LINK_FOLDER, ActiveGroup, AdultListingBits, AgentKey,
            AgentOrObjectKey, AgentPreferences, AlertInfo, AnimatedObjects, AnimationKey,
            AnyMessage, Arrival, Asset, AssetKey, AssetType, AssetUpdateLocation,
            AssociateInventory, AttachmentMode, AttachmentPoint, AvatarAppearance,
            AvatarAttachment, AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarName,
            AvatarPick, AvatarPickerResult, AvatarProperties, AvatarRenderInfo, AvatarRenderReport,
            BUILTIN_UI_SOUNDS, CAP_DISPATCH_REGION_INFO, CAP_FIND_EXPERIENCE_BY_NAME,
            CAP_GET_EXPERIENCES, CAP_GET_EXPERIENCE_INFO, CAP_GET_MESH, CAP_GET_MESH2,
            CAP_GET_OBJECT_COST, CAP_GET_TEXTURE, CAP_INCREMENT_COF_VERSION,
            CAP_MODIFY_MATERIAL_PARAMS, CAP_MODIFY_REGION, CAP_OBJECT_MEDIA,
            CAP_REGION_EXPERIENCES, CAP_RENDER_MATERIALS, CAP_UPDATE_AVATAR_APPEARANCE,
            CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPDATE_SETTINGS_TASK_INVENTORY,
            CAP_UPLOAD_BAKED_TEXTURE, CAP_VIEWER_ASSET, Camera, CameraError, ChatAudible,
            ChatChannel, ChatLifecycleView, ChatLogConfig, ChatMessage, ChatSessionAgentUpdate,
            ChatSessionInfo, ChatSessionKind, ChatSessionReply, ChatSessionRequestPurpose,
            ChatSource, ChatSourceType, ChatType, ChatTypeNotAVolume, Child, CircuitCode,
            CircuitId, CircuitProbe, ClassifiedCategory, ClassifiedClickStats, ClassifiedInfo,
            ClassifiedKey, ClassifiedQueryFlags, ClassifiedSearchClicks, ClassifiedUpdate,
            ClickAction, ClientDirectories, ClockStyle, CloudPosDensity, CoarseLocation, Color,
            ColorAlpha, ColumnUnit, Command, ControlFlags, ConversationKind, CreateGroupParams,
            CreateListing, DEFAULT_BLOOM_TEXTURE, DEFAULT_CLOUD_TEXTURE, DEFAULT_HALO_TEXTURE,
            DEFAULT_MOON_TEXTURE, DEFAULT_PRIM_TEXTURE, DEFAULT_RAINBOW_TEXTURE,
            DEFAULT_SUN_TEXTURE, DEFAULT_WATER_NORMAL_TEXTURE, DayCycle, DayCycleFrame, DayNames,
            DayTrack, DeRezDestination, DensityLayer, DetachOrder, Diagnostic, DirClassifiedResult,
            DirEventResult, DirFindFlags, DirGroupResult, DirLandResult, DirPeopleResult,
            DirPlaceResult, Direction, DirectoryVisibility, DiscardLevel, DisconnectReason,
            DisplayName, DisplayNameUpdate, Distance, DynamicMenus, EconomyData, EjectAction,
            EnvironmentAsset, EnvironmentPushAction, EnvironmentSettings, EnvironmentUpdate,
            Error as SessionError, EstateAccessDelta, EstateAccessKind, EstateAccessLists,
            EstateBan, EstateCovenant, EstateInfo, EstateInfoUpdate, EventId, EventInfo,
            ExperienceEnvironmentPush, ExperienceEvent, ExperienceEventPermission, ExperienceInfo,
            ExperienceKey, ExperienceLocation, ExperiencePermission, ExperienceProperties,
            ExperienceSearchPage, ExperienceUpdate, ExtendedMesh, FaceMaterialPut, FlexibleData,
            FolderInfo, FolderState, FolderType, FreezeAction, Friend, FriendKey, FriendPresence,
            FriendRights, GestureActivation, GlobalCoordinates, Glow, GltfMaterialOverride,
            GodKickAction, GodKickTarget, GodRegionUpdate, GridCoordinates, GroupBan,
            GroupBanAction, GroupInvitationReceived, GroupKey, GroupMember, GroupMembership,
            GroupNotice, GroupNoticeAttachment, GroupNoticeItem, GroupNoticeKey,
            GroupNoticeReceived, GroupProfile, GroupRequestId, GroupRole, GroupRoleChange,
            GroupRoleEdit, GroupRoleKey, GroupRoleMember, GroupRoleMemberChange,
            GroupRoleUpdateType, GroupTitle, HomeLocation, HomeLocationAnswer, IceCandidate,
            ImDialog, ImSessionId, ImageCodec, InstantMessage, InterestListMode, InterestListReply,
            InterestsUpdate, InventoryCacheConfig, InventoryCallbackId, InventoryCursor,
            InventoryFolder, InventoryFolderKey, InventoryItem, InventoryItemOrFolderKey,
            InventoryKey, InventoryOffer, InventoryOwner, InventoryType, InviteChannel, ItemInfo,
            KEYFRAME_SLOP, Key, Kilobits, LOGOUT_TIMEOUT, LandArea, LandBrushAction,
            LandBrushRadius, LandBrushSize, LandEdit, LandImpact, LandProduct, LandSearchType,
            LandStatExtended, LandStatItem, LandStatReportType, LandStatScore, LandingType,
            LegacyMaterial, LegacyPresetError, LightData, LightImage, LindenAmount, LindenBalance,
            Listing, ListingId, Llsd, LoadUrlRequest, LoggedChatType, LoginAccount, LoginFailure,
            LoginParams, LoginRejectKind, LoginRequest, LoginResponse, LoginServer, LoginSuccess,
            LookAtType, LureDestination, LureId, MAX_FACES, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE,
            MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MIDDAY, MIDNIGHT, MUTE_LIST_LIMIT,
            MapBlockBatch, MapBlockKind, MapBlockRecord, MapItem, MapItemType, MapLayer,
            MapRegionInfo, MapRequestFlags, MarketplaceApiError, MarketplaceApiErrorKind,
            MarketplaceAssociateInventoryInfo, MarketplaceInventoryInfo, MarketplaceOperation,
            Material, MaterialOverrideUpdate, Maturity, MediaEntry, MerchantStatus, MeshKey,
            MeshUploadStatus, MessageCursor, MessageId, MessageStatus, MfaChallenge,
            ModifyRegionAnswer, MoneyBalance, MoneyTransaction, MoneyTransactionType, MovementMode,
            MuteEntry, MuteFlags, MuteList, MuteType, NearbyHistoryLine, NegativeBalanceError,
            NeighborInfo, NeighborRetirement, NewInventoryItem, NewInventoryLink, Object,
            ObjectBuyItem, ObjectExtraParams, ObjectFlagSettings, ObjectKey, ObjectMediaResponse,
            ObjectMotion, ObjectPermMasks, ObjectPhysicsData, ObjectPlayingAnimation,
            ObjectProperties, ObjectPropertiesFamily, ObjectStreamBatch, ObjectStreamEntry,
            ObjectTransform, ObjectUpdateForm, OpenRegionInfo, OpenSimExtras, OwnerKey,
            PRESET_DAY_CYCLE_NAME, PRESET_DAY_KEYFRAMES, PacketFlags, ParcelAccessEntry,
            ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelCollision, ParcelDetails,
            ParcelFlags, ParcelInfo, ParcelKey, ParcelListingFlags, ParcelMediaCommand,
            ParcelMediaData, ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelObjectOwnersPart,
            ParcelOverlayCell, ParcelOverlayGrid, ParcelOverlayInfo, ParcelOwnership, ParcelRect,
            ParcelRequestResult, ParcelReturnType, ParcelStatus, ParcelUpdate, ParcelVoiceInfo,
            ParticleSystem, PermissionField, Permissions, Permissions5, PhysicsShapeType,
            PhysicsShapeTypes, PickInfo, PickKey, PickUpdate, PingId, PlacesResult,
            PlayingAnimation, PointAtType, PrimShape, PrimShapeParams, ProductType, ProfileImage,
            ProfileUpdate, ProposalCandidateId, ProposalVoteId, QueryId, REQUESTED_CAPABILITIES,
            ReflectionProbe, ReflectionProbeFlags, RegionChatSettings, RegionCombatSettings,
            RegionCoordinates, RegionDebugUpdate, RegionFlags, RegionHandle,
            RegionHandshakeReplyFlags, RegionIdentity, RegionInfoDispatch, RegionInfoUpdate,
            RegionLimits, RegionLocalObjectId, RegionLocalParcelId, RegionName,
            RegionRestartNotice, RegionSchedule, RegionStats, RegionTerrainComposition,
            RegionTerrainUpdate, Reliability, RemoteParcelRequest, RenderLimitCounts,
            RenderMaterialEntry, RenderMaterialRef, RestartDays, RestartWhen, RestoreItem,
            RezAttachment, RezObjectParams, RezScriptParams, Rotation, SKY_TRACK_COUNT, SUNRISE,
            SUNSET, SaleInfo, SaleType, Scale, ScopedObjectId, ScopedParcelId, ScriptCompileError,
            ScriptControl, ScriptControlAction, ScriptDialog, ScriptLanguage, ScriptMetadata,
            ScriptPermissionRequest, ScriptPermissions, ScriptTarget, ScriptTeleportRequest,
            ScriptUploadLocation, SculptData, SculptOrMeshKey, SequenceNumber,
            ServerHistoryMessage, SessionMessage, SessionTransition, SetDisplayNameReply,
            SettingsKind, SimStatId, SimWideDeleteFlags, SimulatorFeatures, SimulatorTime,
            SkyPreset, SkySettings, SoundFlags, SoundPreload, StartLocation,
            StartLocationParseError, StartLocationSlot, SurfaceInfo, TRACK_MAX,
            TaskInventoryCapReply, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply,
            TelehubInfo, TeleportFlags, TerraformArea, TerrainLayerBatch, TerrainLayerType,
            TerrainMaterialOverrides, TerrainPatch, TerrainPatchHeader, Texture, TextureAnimation,
            TextureEntry, TextureFace, TextureKey, Throttle, ThrottleBuilder, ThrottleError,
            ThumbnailTarget, TimestampFormat, TransactionFlags, TransactionId, TransferId,
            TransferStatus, Transmit, TriggeredSound, UI_SOUND_ALERT, UI_SOUND_CLICK,
            UI_SOUND_IM_OR_OFFER, UI_SOUND_INVALID_OP, UI_SOUND_MONEY_DOWN, UI_SOUND_MONEY_UP,
            UI_SOUND_NEARBY_CHAT, UI_SOUND_SNAPSHOT, UI_SOUND_TELEPORT_OUT, UI_SOUND_TYPING,
            UI_SOUND_WINDOW_CLOSE, UI_SOUND_WINDOW_OPEN, UpdatableAssetType, UpdateGroupInfoParams,
            UpdateListing, UploadClass, UserInfo, Uuid, Vector, ViewerEffect, ViewerEffectData,
            ViewerEffectType, VoiceAccountInfo, VoiceProvisionRequest, WaterSettings, Wearable,
            WearableType, WireError, WireLandmarkAsset, XferId, XferListing, ais_item_update_body,
            avatar_texture, azimuth_altitude_to_rotation, build_get_metadata_request,
            chat_text_muted, decode_particle_system, decode_texture_anim, decode_texture_entry,
            encode_texture_entry, environment_asset_from_bytes, environment_asset_to_bytes,
            expected_upload_cost, grid_to_handle, group_powers, handle_to_global, handle_to_grid,
            install_preset_day_cycle, j2c, landmark_to_wire, legacy_day_cycle_from_bytes,
            legacy_preset_from_bytes, legacy_preset_name, parse_get_metadata_reply, parse_landmark,
            parse_task_inventory, parse_task_inventory_cap_reply, particle_pattern, pcode,
            preset_day_cycle, preset_sky_schedule, prim_flags, rotation_to_azimuth_altitude,
            second_life_unserved_syntax, sim_access, sky_settings_from, sky_with_blended_values,
            sky_with_pushed_values, stamp_get_metadata_reply, stamp_task_inventory_cap_reply,
            task_inventory_cap_url, texture_anim_mode, update_inventory_item_message,
            water_with_blended_values, water_with_pushed_values
        };
        // The decoding, LOD-aware texture store. Its `TextureEntry` and
        // `TextureReadLease` stay under `sl_texture::` because `TextureEntry` would
        // collide with the prim-face type of the same name. `StoreStats` and
        // `GateStats` are the shared `sl-asset-sched` snapshots of all three stores,
        // so they are exported once, from here.
        pub use $crate::reexports::__reexport::sl_texture::{
            AssetFetcher, CacheLimits, DecodedImage as DecodedTexture, FULL_RESOLUTION_PIXEL_AREA,
            FetchChunk, GateStats, NotRemotelyFetchable, Priority, RemoteTextureSource,
            ScreenMetrics, StoreStats, TextureError, TextureFetchType, TextureFetcher,
            TextureProgress, TextureRequest, TextureStore
        };
        // The decoding, LOD-aware mesh store; its `CacheLimits` is aliased so it
        // does not collide with the texture store's.
        pub use $crate::reexports::__reexport::sl_mesh::{
            AssetBytes, CacheLimits as MeshCacheLimits, DEFAULT_LOD_FACTOR, DecodedMesh,
            MeshDiskCache, MeshEntry, MeshError, MeshFetcher, MeshHeader, MeshLod, MeshPhysics,
            MeshProgress, MeshReadLease, MeshRequest, MeshSkin, MeshStore, PhysicsConvex, Submesh,
            VertexWeights, est_triangles_by_lod
        };
        // The generic-asset store, fetched whole over the `ViewerAsset` capability;
        // its `CacheLimits` and `FetchError` are aliased for the same reason.
        pub use $crate::reexports::__reexport::sl_asset::{
            AssetDiskCache, AssetEntry, AssetError, AssetProgress, AssetRef, AssetStore,
            BlobFetcher, CacheLimits as AssetCacheLimits, FetchError as AssetFetchError,
            StaticAssetLibrary
        };
        // The GLTF (PBR) render-material asset codec and the per-face override
        // delta layered on a base material.
        pub use $crate::reexports::__reexport::sl_material::{
            GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform,
            MaterialError as GltfMaterialError, MaterialOverride, TextureOverride,
            TextureTransformOverride, encode_material_asset, encode_override_gltf_json,
            parse_gltf_material_document, parse_material_asset, parse_material_override
        };
        // Prim tessellation. The dequantized float shape is aliased
        // `PrimShapeFloat` so it does not collide with the quantized rez-params
        // `PrimShape`.
        pub use $crate::reexports::__reexport::sl_prim::{
            FlexiAttributes, FlexiChain, HoleType, LodScaleBias, PRIM_LOD_COUNT, PathByteForm,
            PathCurve, PrimFace, PrimFaceId, PrimLod, PrimMesh, PrimShape as PrimShapeFloat,
            ProfileCurve, lod_triangle_counts, tessellate, tessellate_with_path
        };
        // Sculpt-map tessellation. Its function is taken by its module path and
        // aliased `tessellate_sculpt`: `sl_sculpt::tessellate` is both a module and a
        // function, and renaming both would make doc links to the name ambiguous.
        pub use $crate::reexports::__reexport::sl_sculpt::tessellate::tessellate as tessellate_sculpt;
        pub use $crate::reexports::__reexport::sl_sculpt::{
            MAX_SUBDIVISIONS, SculptParams, SculptStitch, mesh_resolution
        };
        // Linden tree and grass geometry, with the tree placement constants
        // prefixed so they say what they belong to.
        pub use $crate::reexports::__reexport::sl_tree::{
            GRASS_MAX_BLADES, GrassMesh, GrassSpecies,
            RADIUS_SCALE_FACTOR as TREE_RADIUS_SCALE_FACTOR, TreeLod, TreeMesh, TreeSpecies,
            YAW_DEGREES as TREE_YAW_DEGREES, billboard_geometry as tree_billboard_geometry,
            grass_geometry, grass_species, tree_geometry, tree_species
        };
        // The system-avatar decoders: skeleton, base body, visual params,
        // wearables and avatar physics. The wearable sale type is aliased
        // `WearableSaleType` so it does not collide with the object `SaleType`.
        pub use $crate::reexports::__reexport::sl_avatar::{
            AppearanceValues, AttachmentPointDef, AttachmentPoints, BaseMesh, BaseMeshError,
            BodyPhysics, BodyPhysicsState, BodySizeMetrics, BoneDeform, CollisionVolume, ColorOp,
            ColorRamp, HAND_POSE_MORPH_PARAMS, Joint, JointSample, MaskTexture, MorphMask,
            MorphMasks, MorphWeights, MorphedMesh, PHYSICS_MORPH_PARAMS, ParamEffect, ParamError,
            ParamGroup, ParamSex, PartMorphMask, PhysicsDrivenParam, PhysicsMotion,
            PhysicsMotionConfig, PhysicsSettings, RUNTIME_MORPH_PARAMS, ResolvedParams,
            SaleType as WearableSaleType, SkeletalDeformations, Skeleton, SkeletonError,
            VisualParam, VisualParams, VolumeDeform, VolumeDeformations, VolumeMorph,
            WearableAsset, WearableError, WearablePermissions, combine_layer_color, global_color,
            global_color_params, hand_pose_morph_param, is_runtime_morph_param
        };
        // The client-side avatar baker (the OpenSim / legacy path) and its
        // per-region layer plan.
        pub use $crate::reexports::__reexport::sl_bake::{
            BAKE_SIZE, BakeRegion, BakedImage, Layer, LayerKind, LayerSource, LayerTint,
            PlannedLayer, ShapeMask, ShapeMaskSpec, StaticLayers, TexGen, assemble_layers,
            bake_region, composite_region, load_static_layers, region_layers, region_plan,
            region_texture_ids, shape_mask_files, static_layer_files
        };
    };
}
