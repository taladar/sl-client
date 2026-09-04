//! Message-level `Session` ↔ [`SimSession`] symmetry: a representative set of
//! messages driven **both ways** through the in-memory loopback, asserting
//! that what one peer encodes the other decodes to the same value — and, for
//! the client messages the simulator forwards verbatim, that re-encoding the
//! surfaced message reproduces the wire bytes exactly.
//!
//! This complements `tests/sim_session.rs`, which covers the *flow-level*
//! state machines (circuit lifecycle, teleport, Xfer, chat sessions, …) and
//! pins the [`sl_proto::SESSION_FLOW_COVERAGE`] ledger. The ledger pinned
//! here, [`RAW_FORWARDED`], is the message-level sibling: every client
//! message that reaches the consumer as
//! [`ServerEvent::ClientMessage`] (no typed server event yet). Adding a typed
//! arm for one of these makes its family test fail — edit the ledger
//! deliberately.
//!
//! Auditing the server side is a grep, not a test: compare
//! `grep -oE 'pub fn (send|enqueue)_[a-z_0-9]+' sl-proto/src/sim_session.rs`
//! against `.<name>(` call sites in `tests/sim_session.rs` and this file.
//! At the time of writing every `send_*`/`enqueue_*` is exercised by one of
//! the two.

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::{
        AgentKey, AnimationKey, AssetType, AttachmentMode, AttachmentPoint, Camera, ChatChannel,
        ChatSessionKind, ClassifiedCategory, ClassifiedKey, ClassifiedUpdate, ClickAction,
        ControlFlags, CreateGroupParams, Event, FolderType, FriendKey, GridCoordinates, GroupKey,
        GroupNoticeKey, GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMemberChange,
        GroupRoleUpdateType, InterestsUpdate, InventoryCallbackId, InventoryFolderKey,
        InventoryItem, InventoryKey, InventoryType, LandStatReportType, LindenAmount, LoginParams,
        LureId, Material, Maturity, MoneyTransactionType, MuteFlags, MuteType, NewInventoryItem,
        ObjectExtraParams, ObjectFlagSettings, ObjectKey, ObjectTransform, OwnerKey,
        ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelFlags,
        ParcelReturnType, ParcelUpdate, PermissionField, Permissions, Permissions5, PickKey,
        PickUpdate, PrimShapeParams, ProductType, ProfileUpdate, QueryId, RegionHandle,
        RegionIdentity, RegionLocalObjectId, RegionLocalParcelId, RegionTerrainComposition,
        RezAttachment, SaleType, ScopedObjectId, ScopedParcelId, ServerEvent, Session, SimSession,
        TextureKey, TransactionId, Wearable, WearableType, group_powers,
        parse_event_queue_response,
    };
    use sl_types::lsl::{Rotation, Vector};
    use sl_wire::{
        AnyMessage, CircuitCode, LoginRequest, LoginResponse, LoginSuccess, MessageId, PacketFlags,
        Reader, StartLocation, Writer, parse_datagram, zero_decode,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// The region handle the simulator serves throughout these tests.
    const REGION_HANDLE: u64 = 0x0000_03e8_0000_03e8;

    /// The simulator's UDP address (matches the [`success`] login fixture).
    fn sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000)
    }

    /// The client's UDP address, as the simulator sees it.
    fn client_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 40000)
    }

    /// `now + millis`, for advancing the simulated clock.
    fn after(now: Instant, millis: u64) -> Result<Instant, TestError> {
        now.checked_add(Duration::from_millis(millis))
            .ok_or_else(|| "clock overflow".into())
    }

    /// A fresh client session pointing at the test simulator.
    fn new_client() -> Result<Session, TestError> {
        Ok(Session::new(LoginParams {
            login_uri: "http://127.0.0.1:9000/".parse()?,
            request: LoginRequest::new(
                "Test",
                "User",
                "secret",
                StartLocation::Last,
                "MyViewer",
                "1.2.3",
            ),
        }))
    }

    /// A successful login response pointing at the test simulator.
    fn success() -> Result<LoginResponse, TestError> {
        Ok(LoginResponse::Success(Box::new(LoginSuccess::minimal(
            AgentKey::from(uuid::Uuid::from_u128(1)),
            uuid::Uuid::from_u128(2),
            uuid::Uuid::from_u128(3),
            CircuitCode(0x0011_2233),
            Ipv4Addr::new(127, 0, 0, 1),
            9000,
            "http://127.0.0.1:9000/seed".parse()?,
        ))))
    }

    /// Delivers all queued datagrams between the client and simulator (in both
    /// directions) until neither has anything more to send.
    fn pump(client: &mut Session, sim: &mut SimSession, now: Instant) -> Result<(), TestError> {
        loop {
            let mut moved = false;
            while let Some(transmit) = client.poll_transmit() {
                sim.handle_datagram(client_addr(), &transmit.payload, now)?;
                moved = true;
            }
            while let Some(transmit) = sim.poll_transmit() {
                client.handle_datagram(sim_addr(), &transmit.payload, now)?;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        Ok(())
    }

    /// Drains all queued server events.
    fn drain_server(sim: &mut SimSession) -> Vec<ServerEvent> {
        let mut out = Vec::new();
        while let Some(event) = sim.poll_event() {
            out.push(event);
        }
        out
    }

    /// Drains all queued client events.
    fn drain_client(client: &mut Session) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = client.poll_event() {
            out.push(event);
        }
        out
    }

    /// Delivers the simulator's queued CAPS events to the client over the real
    /// `EventQueueGet` long-poll path, returning the resulting client events.
    fn deliver_caps(
        client: &mut Session,
        sim: &mut SimSession,
        now: Instant,
    ) -> Result<Vec<Event>, TestError> {
        let xml = sim
            .take_event_queue_response()
            .ok_or("the simulator queued at least one CAPS event")?;
        for event in parse_event_queue_response(&xml)?.events {
            client.handle_caps_event(&event.message, &event.body, now)?;
        }
        Ok(drain_client(client))
    }

    /// Logs a client in and drives both peers through circuit setup and arrival,
    /// returning the active pair with both event queues drained.
    fn setup(now: Instant) -> Result<(Session, SimSession), TestError> {
        let mut client = new_client()?;
        client.handle_login_response(success()?, now)?;
        client.notify_capabilities_ready(now)?;
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        drain_client(&mut client);
        Ok((client, sim))
    }

    /// One client datagram as the simulator received it: the message decoded
    /// standalone from the wire and the (zero-code expanded) body bytes it was
    /// decoded from, message id included.
    struct Relayed {
        /// The message decoded independently of the simulator.
        message: AnyMessage,
        /// The wire body the simulator saw, after zero-code expansion.
        body: Vec<u8>,
    }

    /// Delivers every queued client datagram to the simulator, decoding each
    /// one independently on the way so the test can compare the simulator's
    /// view against it. Transport chatter (acks, pings) is delivered but not
    /// returned.
    fn relay_client(
        client: &mut Session,
        sim: &mut SimSession,
        now: Instant,
    ) -> Result<Vec<Relayed>, TestError> {
        let mut out = Vec::new();
        while let Some(transmit) = client.poll_transmit() {
            let parsed = parse_datagram(&transmit.payload)?;
            let body = if parsed.flags.contains(PacketFlags::ZEROCODED) {
                zero_decode(parsed.body)?
            } else {
                parsed.body.to_vec()
            };
            let mut reader = Reader::new(&body);
            let id = MessageId::decode(&mut reader)?;
            let message = AnyMessage::decode(id, &mut reader)?;
            sim.handle_datagram(client_addr(), &transmit.payload, now)?;
            if !matches!(
                message,
                AnyMessage::PacketAck(_)
                    | AnyMessage::StartPingCheck(_)
                    | AnyMessage::CompletePingCheck(_)
            ) {
                out.push(Relayed { message, body });
            }
        }
        Ok(out)
    }

    /// Re-encodes a message exactly as a circuit would frame it: message id
    /// followed by the body.
    fn encode(message: &AnyMessage) -> Result<Vec<u8>, TestError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        Ok(writer.into_bytes())
    }

    /// The message's template name, for the ledger comparison.
    fn name_of(message: &AnyMessage) -> String {
        message.name().to_owned()
    }

    /// Asserts the simulator forwarded each relayed client message verbatim —
    /// `ServerEvent::ClientMessage(m)` with `m` equal to the independently
    /// decoded message, in order — and that re-encoding each surfaced message
    /// reproduces the wire body byte-for-byte. Returns the message names in
    /// arrival order, for the family's ledger assertion.
    fn assert_forwarded_verbatim(
        relayed: &[Relayed],
        events: &[ServerEvent],
    ) -> Result<Vec<String>, TestError> {
        let surfaced: Vec<&AnyMessage> = events
            .iter()
            .filter_map(|event| match event {
                ServerEvent::ClientMessage(message) => Some(message.as_ref()),
                _ => None,
            })
            .collect();
        let typed: Vec<&ServerEvent> = events
            .iter()
            .filter(|event| !matches!(event, ServerEvent::ClientMessage(_)))
            .collect();
        assert!(
            typed.is_empty(),
            "every message in this family is raw-forwarded; got typed events {typed:?}"
        );
        assert_eq!(
            surfaced.len(),
            relayed.len(),
            "one ClientMessage per relayed datagram"
        );
        let mut names = Vec::new();
        for (sent, seen) in relayed.iter().zip(surfaced) {
            assert_eq!(
                seen,
                &sent.message,
                "the simulator's decode of {} differs from a standalone decode",
                name_of(&sent.message)
            );
            assert_eq!(
                encode(seen)?,
                sent.body,
                "re-encoding the surfaced {} does not reproduce the wire body",
                name_of(seen)
            );
            names.push(name_of(seen));
        }
        Ok(names)
    }

    /// Drives the client's queued messages into the simulator and asserts the
    /// family arrived verbatim in the expected order.
    fn assert_family(
        client: &mut Session,
        sim: &mut SimSession,
        now: Instant,
        expected: &[&str],
    ) -> Result<Vec<Relayed>, TestError> {
        let relayed = relay_client(client, sim, now)?;
        let events = drain_server(sim);
        let names = assert_forwarded_verbatim(&relayed, &events)?;
        assert_eq!(names, expected);
        for name in expected {
            assert!(
                RAW_FORWARDED.contains(name),
                "{name} is raw-forwarded but missing from the RAW_FORWARDED ledger"
            );
        }
        Ok(relayed)
    }

    /// Finds the relayed message of the given template name.
    fn find<'a>(relayed: &'a [Relayed], name: &str) -> Result<&'a AnyMessage, TestError> {
        relayed
            .iter()
            .map(|entry| &entry.message)
            .find(|message| name_of(message) == name)
            .ok_or_else(|| format!("expected a relayed {name}").into())
    }

    /// Strips the NUL terminator of a wire string.
    fn trimmed(bytes: &[u8]) -> &[u8] {
        bytes.strip_suffix(b"\0").unwrap_or(bytes)
    }

    /// A region identity for the simulator's `RegionHandshake`.
    fn region_identity() -> RegionIdentity {
        RegionIdentity {
            sim_name: sl_proto::region_name_from_wire("test", "Server Region")
                .ok()
                .flatten(),
            region_id: uuid::Uuid::from_u128(0xBEEF),
            region_handle: RegionHandle(REGION_HANDLE),
            grid_coordinates: GridCoordinates::new(1000, 1000),
            region_flags: 0x40,
            region_flags_extended: 0x1_0000_0040,
            region_protocols: 0x5,
            maturity: Maturity::Mature,
            product: ProductType::Homestead,
            product_sku: String::new(),
            product_name: "Homestead".to_owned(),
            cpu_class_id: 4,
            cpu_ratio: 8,
            sim_owner: uuid::Uuid::from_u128(0x0411),
            is_estate_manager: true,
            water_height: 20.0,
            billable_factor: 1.0,
            terrain: RegionTerrainComposition {
                detail_textures: [
                    uuid::Uuid::from_u128(0xD0),
                    uuid::Uuid::from_u128(0xD1),
                    uuid::Uuid::from_u128(0xD2),
                    uuid::Uuid::from_u128(0xD3),
                ],
                start_heights: [1.0, 2.0, 3.0, 4.0],
                height_ranges: [10.0, 20.0, 30.0, 40.0],
            },
        }
    }

    /// A rezzed box the simulator can push so the client's object cache knows
    /// `local_id` (the edit methods that consult the cache need it).
    fn box_prim(local_id: u32, full_id: u128) -> sl_proto::Object {
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        sl_proto::Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(local_id),
            circuit: sl_proto::CircuitId::default(),
            full_id: ObjectKey::from(uuid::Uuid::from_u128(full_id)),
            parent_id: RegionLocalObjectId(0),
            pcode: sl_proto::pcode::PRIMITIVE,
            state: 0,
            crc: 7,
            material: 3,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: sl_proto::ObjectMotion {
                position: Vector {
                    x: 128.0,
                    y: 128.0,
                    z: 25.0,
                },
                velocity: zero.clone(),
                acceleration: zero.clone(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero.clone(),
                collision_plane: None,
            },
            owner_id: own_agent(),
            sound: uuid::Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0, 0, 0, 0],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: PrimShapeParams {
                path_curve: 16,
                profile_curve: 1,
                path_scale_x: 100,
                path_scale_y: 100,
                ..PrimShapeParams::default()
            },
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero.clone(),
            joint_axis_or_anchor: zero,
        }
    }

    /// The agent id the [`success`] login fixture grants.
    fn own_agent() -> uuid::Uuid {
        uuid::Uuid::from_u128(1)
    }

    /// **The raw-forward ledger.** Every client message that [`SimSession`]
    /// surfaces verbatim as [`ServerEvent::ClientMessage`] rather than as a
    /// typed server event, grouped by family exactly as the family tests below
    /// send them. The message-level sibling of
    /// [`sl_proto::SESSION_FLOW_COVERAGE`]: when a `protocol-sim-*` follow-up
    /// adds a typed arm for one of these, its family test fails (the message
    /// no longer arrives as `ClientMessage`) — remove the row here and assert
    /// the typed event in `tests/sim_session.rs` instead.
    const RAW_FORWARDED: &[&str] = &[
        // inventory (legacy UDP mutation; AISv3 is the modern path)
        "CreateInventoryFolder",
        "UpdateInventoryFolder",
        "MoveInventoryFolder",
        "RemoveInventoryFolder",
        "CreateInventoryItem",
        "UpdateInventoryItem",
        "MoveInventoryItem",
        "CopyInventoryItem",
        "ChangeInventoryItemFlags",
        "RemoveInventoryItem",
        "RemoveInventoryObjects",
        "PurgeInventoryDescendents",
        "FetchInventoryDescendents",
        // groups
        "CreateGroupRequest",
        "JoinGroupRequest",
        "LeaveGroupRequest",
        "InviteGroupRequest",
        "EjectGroupMemberRequest",
        "ActivateGroup",
        "SetGroupAcceptNotices",
        "SetGroupContribution",
        "GroupRoleUpdate",
        "GroupRoleChanges",
        "GroupProfileRequest",
        "GroupMembersRequest",
        "GroupRoleDataRequest",
        "GroupRoleMembersRequest",
        "GroupTitlesRequest",
        "GroupNoticesListRequest",
        "GroupNoticeRequest",
        // object edits
        "MultipleObjectUpdate",
        "ObjectName",
        "ObjectDescription",
        "ObjectCategory",
        "ObjectClickAction",
        "ObjectMaterial",
        "ObjectSaleInfo",
        "ObjectFlagUpdate",
        "ObjectIncludeInSearch",
        "ObjectPermissions",
        "ObjectGroup",
        "ObjectOwner",
        "ObjectLink",
        "ObjectDelink",
        "ObjectDuplicate",
        "ObjectSelect",
        "ObjectDeselect",
        "ObjectGrab",
        "ObjectDeGrab",
        "ObjectGrabUpdate",
        "Undo",
        "Redo",
        "ObjectDelete",
        // parcels / land / region
        "ParcelPropertiesUpdate",
        "ParcelBuy",
        "ParcelDeedToGroup",
        "ParcelRelease",
        "ParcelReclaim",
        "ParcelReturnObjects",
        "ParcelSelectObjects",
        "ParcelAccessListRequest",
        "ParcelAccessListUpdate",
        "LandStatRequest",
        "RequestRegionInfo",
        // profile / picks / classifieds
        "AvatarPropertiesRequest",
        "AvatarPropertiesUpdate",
        "AvatarInterestsUpdate",
        "AvatarNotesUpdate",
        "PickInfoUpdate",
        "PickDelete",
        "PickGodDelete",
        "ClassifiedInfoRequest",
        "ClassifiedInfoUpdate",
        "ClassifiedDelete",
        "ClassifiedGodDelete",
        // money
        "MoneyBalanceRequest",
        "MoneyTransferRequest",
        // mutes
        "MuteListRequest",
        "UpdateMuteListEntry",
        "RemoveMuteListEntry",
        // appearance / misc agent
        "AgentAnimation",
        "AgentIsNowWearing",
        "AgentSetAppearance",
        "AgentCachedTexture",
        "RequestImage",
        "RetrieveInstantMessages",
        "ScriptDialogReply",
        "StartLure",
        "GodKickUser",
        "GodlikeMessage",
        "GenericMessage",
    ];

    /// The raw-forwarded messages the matching family test sends, in order.
    const INVENTORY_FAMILY: &[&str] = &[
        "CreateInventoryFolder",
        "CreateInventoryFolder",
        "UpdateInventoryFolder",
        "MoveInventoryFolder",
        "RemoveInventoryFolder",
        "CreateInventoryItem",
        "UpdateInventoryItem",
        "MoveInventoryItem",
        "CopyInventoryItem",
        "ChangeInventoryItemFlags",
        "RemoveInventoryItem",
        "RemoveInventoryObjects",
        "PurgeInventoryDescendents",
        "FetchInventoryDescendents",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const GROUP_FAMILY: &[&str] = &[
        "CreateGroupRequest",
        "JoinGroupRequest",
        "LeaveGroupRequest",
        "InviteGroupRequest",
        "EjectGroupMemberRequest",
        "ActivateGroup",
        "SetGroupAcceptNotices",
        "SetGroupContribution",
        "GroupRoleUpdate",
        "GroupRoleChanges",
        "GroupProfileRequest",
        "GroupMembersRequest",
        "GroupRoleDataRequest",
        "GroupRoleMembersRequest",
        "GroupTitlesRequest",
        "GroupNoticesListRequest",
        "GroupNoticeRequest",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const OBJECT_FAMILY: &[&str] = &[
        "MultipleObjectUpdate",
        "ObjectName",
        "ObjectDescription",
        "ObjectCategory",
        "ObjectClickAction",
        "ObjectMaterial",
        "ObjectSaleInfo",
        "ObjectFlagUpdate",
        "ObjectIncludeInSearch",
        "ObjectPermissions",
        "ObjectGroup",
        "ObjectOwner",
        "ObjectLink",
        "ObjectDelink",
        "ObjectDuplicate",
        "ObjectSelect",
        "ObjectDeselect",
        "ObjectGrab",
        "ObjectDeGrab",
        "ObjectGrabUpdate",
        "Undo",
        "Redo",
        "ObjectDelete",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const PARCEL_FAMILY: &[&str] = &[
        "ParcelPropertiesUpdate",
        "ParcelBuy",
        "ParcelDeedToGroup",
        "ParcelRelease",
        "ParcelReclaim",
        "ParcelReturnObjects",
        "ParcelSelectObjects",
        "ParcelAccessListRequest",
        "ParcelAccessListUpdate",
        "LandStatRequest",
        "RequestRegionInfo",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const PROFILE_FAMILY: &[&str] = &[
        "AvatarPropertiesRequest",
        "AvatarPropertiesUpdate",
        "AvatarInterestsUpdate",
        "AvatarNotesUpdate",
        "PickInfoUpdate",
        "PickDelete",
        "PickGodDelete",
        "ClassifiedInfoRequest",
        "ClassifiedInfoUpdate",
        "ClassifiedDelete",
        "ClassifiedGodDelete",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const MONEY_AND_MUTE_FAMILY: &[&str] = &[
        "MoneyBalanceRequest",
        "MoneyTransferRequest",
        "MuteListRequest",
        "UpdateMuteListEntry",
        "RemoveMuteListEntry",
    ];
    /// The raw-forwarded messages the matching family test sends, in order.
    const AGENT_FAMILY: &[&str] = &[
        "AgentAnimation",
        "AgentIsNowWearing",
        "AgentSetAppearance",
        "AgentCachedTexture",
        "RequestImage",
        "RetrieveInstantMessages",
        "ScriptDialogReply",
        "StartLure",
        "GodKickUser",
        "GodlikeMessage",
        "GenericMessage",
    ];

    /// The ledger has no duplicates, and every entry is a real message
    /// template name.
    #[test]
    fn raw_forwarded_ledger_is_pinned() {
        let unique: BTreeSet<&str> = RAW_FORWARDED.iter().copied().collect();
        assert_eq!(unique.len(), RAW_FORWARDED.len(), "duplicate ledger rows");
        // A family may legitimately send one message twice (two folders);
        // the ledger lists each message once.
        let mut families_unique = Vec::new();
        let families: Vec<&str> = [
            INVENTORY_FAMILY,
            GROUP_FAMILY,
            OBJECT_FAMILY,
            PARCEL_FAMILY,
            PROFILE_FAMILY,
            MONEY_AND_MUTE_FAMILY,
            AGENT_FAMILY,
        ]
        .concat();
        for name in families {
            if !families_unique.contains(&name) {
                families_unique.push(name);
            }
        }
        assert_eq!(
            families_unique, RAW_FORWARDED,
            "the family tests together cover exactly the ledger"
        );
    }

    // ----- client → simulator, raw-forwarded families -----------------------

    #[test]
    fn inventory_mutations_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;

        let root = InventoryFolderKey::from(uuid::Uuid::from_u128(0x10));
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x11));
        let other = InventoryFolderKey::from(uuid::Uuid::from_u128(0x12));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x21));

        client.create_inventory_folder(folder, root, FolderType::None, "Stuff", now)?;
        client.create_inventory_folder(other, root, FolderType::None, "Other", now)?;
        client.update_inventory_folder(folder, root, FolderType::None, "Things", now)?;
        client.move_inventory_folders(&[(folder, other)], true, now)?;
        client.remove_inventory_folders(&[other], now)?;
        let callback = client.create_inventory_item(
            &NewInventoryItem {
                folder_id: folder,
                transaction_id: uuid::Uuid::nil(),
                next_owner_mask: 0x0008_e000,
                asset_type: AssetType::Notecard,
                inv_type: InventoryType::Notecard,
                wearable_type: WearableType::Shape,
                name: "Notes".to_owned(),
                description: "a note".to_owned(),
            },
            now,
        )?;
        assert_eq!(callback, InventoryCallbackId(1));
        client.update_inventory_item(
            &InventoryItem {
                item_id: item,
                folder_id: folder,
                name: "Renamed".to_owned(),
                description: String::new(),
                asset_id: uuid::Uuid::nil(),
                item_type: 0,
                inv_type: 0,
                flags: 0,
                sale_type: 0,
                sale_price: Some(LindenAmount(0)),
                creation_date: 0,
                owner: OwnerKey::Agent(AgentKey::from(own_agent())),
                last_owner_id: uuid::Uuid::nil(),
                creator_id: AgentKey::from(own_agent()),
                group: None,
                permissions: Permissions5::empty(),
            },
            TransactionId::from(uuid::Uuid::nil()),
            now,
        )?;
        client.move_inventory_items(&[(item, other, "Moved".to_owned())], false, now)?;
        client.copy_inventory_item(AgentKey::from(own_agent()), item, other, "Copy", now)?;
        client.change_inventory_item_flags(item, 0x100, now)?;
        client.remove_inventory_items(&[item], now)?;
        client.remove_inventory_objects(&[other], &[item], now)?;
        client.purge_inventory_descendents(folder, now)?;
        client.request_folder_contents(folder, now)?;

        let relayed = assert_family(&mut client, &mut sim, now, INVENTORY_FAMILY)?;
        let AnyMessage::CreateInventoryItem(create) = find(&relayed, "CreateInventoryItem")? else {
            return Err("expected a CreateInventoryItem".into());
        };
        assert_eq!(create.inventory_block.callback_id, 1);
        assert_eq!(trimmed(&create.inventory_block.name), b"Notes");
        Ok(())
    }

    #[test]
    fn group_edits_and_queries_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;

        let group = GroupKey::from(uuid::Uuid::from_u128(0x670C));
        let role = GroupRoleKey::from(uuid::Uuid::from_u128(0x670D));
        let member = AgentKey::from(uuid::Uuid::from_u128(0x670E));

        client.create_group(
            &CreateGroupParams {
                name: "My Group".to_owned(),
                charter: "hi".to_owned(),
                show_in_list: true,
                insignia_id: None,
                membership_fee: LindenAmount(0),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            },
            now,
        )?;
        client.join_group(group, now)?;
        client.leave_group(group, now)?;
        client.invite_to_group(group, &[(member, role)], now)?;
        client.eject_group_members(group, &[member], now)?;
        client.activate_group(Some(group), now)?;
        client.set_group_accept_notices(group, true, false, now)?;
        client.set_group_contribution(group, 512, now)?;
        client.update_group_roles(
            group,
            &[GroupRoleEdit {
                role_id: Some(role),
                name: "Officers".to_owned(),
                description: "the officers".to_owned(),
                title: "Officer".to_owned(),
                powers: group_powers::MEMBER_INVITE | group_powers::NOTICES_SEND,
                update_type: GroupRoleUpdateType::Create,
            }],
            now,
        )?;
        client.change_group_role_members(
            group,
            &[GroupRoleMemberChange {
                role_id: Some(role),
                member_id: member,
                change: GroupRoleChange::Add,
            }],
            now,
        )?;
        client.request_group_profile(group, now)?;
        client.request_group_members(group, now)?;
        client.request_group_roles(group, now)?;
        client.request_group_role_members(group, now)?;
        client.request_group_titles(group, now)?;
        client.request_group_notices(group, now)?;
        client.request_group_notice(GroupNoticeKey::from(uuid::Uuid::from_u128(0x670F)), now)?;

        let relayed = assert_family(&mut client, &mut sim, now, GROUP_FAMILY)?;
        let AnyMessage::SetGroupContribution(contribution) =
            find(&relayed, "SetGroupContribution")?
        else {
            return Err("expected a SetGroupContribution".into());
        };
        assert_eq!(contribution.data.group_id, group.uuid());
        assert_eq!(contribution.data.contribution, 512);
        Ok(())
    }

    #[test]
    fn object_edits_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let circuit = client.root_circuit_id().ok_or("no circuit")?;
        let one = ScopedObjectId::new(circuit, RegionLocalObjectId(1));
        let two = ScopedObjectId::new(circuit, RegionLocalObjectId(2));
        let group = GroupKey::from(uuid::Uuid::from_u128(0x6711));
        let object = ObjectKey::from(uuid::Uuid::from_u128(0x0B1));

        // Undo/Redo only name objects the client has seen; rez two first.
        sim.send_object_update(&[box_prim(1, 0x0B1), box_prim(2, 0x0B2)], 0xFFFF, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);
        drain_server(&mut sim);

        let position = Vector {
            x: 128.0,
            y: 64.0,
            z: 25.5,
        };
        client.update_object(
            one,
            &ObjectTransform {
                position: Some(position.clone()),
                ..ObjectTransform::default()
            },
            now,
        )?;
        client.set_object_name(one, "Cube", now)?;
        client.set_object_description(one, "a cube", now)?;
        client.set_object_category(one, 3, now)?;
        client.set_object_click_action(one, ClickAction::Sit, now)?;
        client.set_object_material(one, Material::Metal, now)?;
        client.set_object_for_sale(one, SaleType::Copy, Some(LindenAmount(250)), now)?;
        client.set_object_flags(
            one,
            &ObjectFlagSettings {
                use_physics: true,
                is_phantom: true,
                ..ObjectFlagSettings::default()
            },
            now,
        )?;
        client.set_object_include_in_search(one, true, now)?;
        client.set_object_permissions(
            &[one, two],
            PermissionField::NextOwner,
            true,
            Permissions::COPY,
            now,
        )?;
        client.set_object_group(&[one], group, now)?;
        client.deed_objects_to_group(&[one], group, now)?;
        client.link_objects(&[one, two], now)?;
        client.delink_objects(&[two], now)?;
        client.duplicate_objects(
            &[one],
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            None,
            now,
        )?;
        client.request_object_properties(&[one], now)?;
        client.deselect_objects(&[one], now)?;
        client.touch_object(one, None, now)?;
        client.grab_object_update(
            object,
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            position.clone(),
            10,
            None,
            now,
        )?;
        client.undo_objects(&[one], now)?;
        client.redo_objects(&[one], now)?;
        client.delete_objects(&[two], now)?;

        let relayed = assert_family(&mut client, &mut sim, now, OBJECT_FAMILY)?;
        let AnyMessage::ObjectSaleInfo(sale) = find(&relayed, "ObjectSaleInfo")? else {
            return Err("expected an ObjectSaleInfo".into());
        };
        let block = sale.object_data.first().ok_or("one sale block")?;
        assert_eq!(block.local_id, 1);
        assert_eq!(block.sale_price, 250);
        Ok(())
    }

    #[test]
    fn parcel_and_region_requests_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let circuit = client.root_circuit_id().ok_or("no circuit")?;
        let parcel = ScopedParcelId::new(circuit, RegionLocalParcelId(7));
        let group = GroupKey::from(uuid::Uuid::from_u128(0x6712));

        client.update_parcel(
            &ParcelUpdate {
                local_id: RegionLocalParcelId(7),
                parcel_flags: ParcelFlags::CREATE_OBJECTS.union(ParcelFlags::USE_BAN_LIST),
                name: "My Parcel".to_owned(),
                description: "A test parcel".to_owned(),
                category: ParcelCategory::Residential,
                sale_price: Some(LindenAmount(100)),
                ..ParcelUpdate::default()
            },
            now,
        )?;
        client.buy_parcel(parcel, 512, 1024, None, false, now)?;
        client.deed_parcel_to_group(parcel, group, now)?;
        client.release_parcel(parcel, now)?;
        client.reclaim_parcel(parcel, now)?;
        client.return_parcel_objects(
            parcel,
            ParcelReturnType::OTHER,
            &[OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x99)))],
            &[],
            now,
        )?;
        client.select_parcel_objects(parcel, ParcelReturnType::OTHER, &[], now)?;
        client.request_parcel_access_list(parcel, ParcelAccessScope::Ban, now)?;
        client.update_parcel_access_list(
            parcel,
            ParcelAccessScope::Access,
            &[ParcelAccessEntry {
                id: uuid::Uuid::from_u128(0x55),
                time: 0,
                flags: ParcelAccessFlags::ALLOW_EXPERIENCE,
            }],
            uuid::Uuid::from_u128(0x7A),
            now,
        )?;
        client.request_land_stat(LandStatReportType::TopScripts, 0, "", parcel, now)?;
        client.request_region_info(now)?;

        let relayed = assert_family(&mut client, &mut sim, now, PARCEL_FAMILY)?;
        let AnyMessage::ParcelBuy(buy) = find(&relayed, "ParcelBuy")? else {
            return Err("expected a ParcelBuy".into());
        };
        assert_eq!(buy.data.local_id, 7);
        assert_eq!(buy.parcel_data.price, 512);
        assert_eq!(buy.parcel_data.area, 1024);
        Ok(())
    }

    #[test]
    fn profile_pick_and_classified_edits_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let target = AgentKey::from(uuid::Uuid::from_u128(0xA1));
        let pick = PickKey::from(uuid::Uuid::from_u128(0xC1));
        let classified = ClassifiedKey::from(uuid::Uuid::from_u128(0xD1));
        let query = QueryId::from(uuid::Uuid::from_u128(0xE1));

        client.request_avatar_properties(target, now)?;
        client.update_profile(
            &ProfileUpdate {
                image_id: TextureKey::from(uuid::Uuid::from_u128(0x5E)),
                about_text: "Hello world".to_owned(),
                allow_publish: true,
                profile_url: "https://example.com".to_owned(),
                ..ProfileUpdate::default()
            },
            now,
        )?;
        client.update_interests(
            &InterestsUpdate {
                want_to_mask: 0x7,
                want_to_text: "build, explore".to_owned(),
                skills_mask: 0x2,
                skills_text: "scripting".to_owned(),
                languages_text: "English".to_owned(),
            },
            now,
        )?;
        client.update_avatar_notes(target, "a good friend", now)?;
        client.update_pick(
            &PickUpdate {
                pick_id: pick,
                name: "New pick".to_owned(),
                description: "a place".to_owned(),
                ..PickUpdate::default()
            },
            now,
        )?;
        client.delete_pick(pick, now)?;
        client.god_delete_pick(pick, query, now)?;
        client.request_classified_info(classified, now)?;
        client.update_classified(
            &ClassifiedUpdate {
                classified_id: classified,
                category: ClassifiedCategory::PropertyRental,
                name: "New classified".to_owned(),
                description: "for sale".to_owned(),
                price_for_listing: LindenAmount(100),
                classified_flags: 0x4,
                ..ClassifiedUpdate::default()
            },
            now,
        )?;
        client.delete_classified(classified, now)?;
        client.god_delete_classified(classified, query, now)?;

        let relayed = assert_family(&mut client, &mut sim, now, PROFILE_FAMILY)?;
        let AnyMessage::AvatarNotesUpdate(notes) = find(&relayed, "AvatarNotesUpdate")? else {
            return Err("expected an AvatarNotesUpdate".into());
        };
        assert_eq!(notes.data.target_id, target.uuid());
        assert_eq!(trimmed(&notes.data.notes), b"a good friend");
        Ok(())
    }

    #[test]
    fn money_and_mute_requests_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let target = uuid::Uuid::from_u128(0x9001);

        client.request_money_balance(now)?;
        client.send_money_transfer(
            uuid::Uuid::from_u128(0xABCD),
            LindenAmount(250),
            MoneyTransactionType::PayObject,
            "tip",
            now,
        )?;
        client.request_mute_list(now)?;
        client.mute(
            target,
            "Bad Actor",
            MuteType::Agent,
            MuteFlags::default(),
            now,
        )?;
        client.unmute(target, "Bad Actor", now)?;

        let relayed = assert_family(&mut client, &mut sim, now, MONEY_AND_MUTE_FAMILY)?;
        let AnyMessage::MoneyTransferRequest(transfer) = find(&relayed, "MoneyTransferRequest")?
        else {
            return Err("expected a MoneyTransferRequest".into());
        };
        assert_eq!(transfer.money_data.amount, 250);
        assert_eq!(trimmed(&transfer.money_data.description), b"tip");
        Ok(())
    }

    #[test]
    fn appearance_and_misc_agent_messages_forward_verbatim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let texture = TextureKey::from(uuid::Uuid::from_u128(0x7E));

        client.set_animations(
            &[
                (AnimationKey::from(uuid::Uuid::from_u128(0x100)), true),
                (AnimationKey::from(uuid::Uuid::from_u128(0x200)), false),
            ],
            now,
        )?;
        client.set_wearing(
            &[Wearable {
                item_id: InventoryKey::from(uuid::Uuid::from_u128(0x31)),
                asset_id: None,
                wearable_type: WearableType::Shape,
            }],
            now,
        )?;
        client.set_appearance(
            3,
            Vector {
                x: 0.45,
                y: 0.6,
                z: 1.9,
            },
            &[0u8; 4],
            &[128u8; 8],
            &[(uuid::Uuid::from_u128(0x41), 0)],
            now,
        )?;
        client.request_cached_textures(3, &[(uuid::Uuid::from_u128(0x51), 0)], now)?;
        client.request_texture(texture, 0, 1.0e6, now)?;
        client.retrieve_instant_messages(now)?;
        client.reply_script_dialog(
            ObjectKey::from(uuid::Uuid::from_u128(0x0B2)),
            ChatChannel(-1234),
            1,
            "No",
            now,
        )?;
        client.offer_teleport(
            &[AgentKey::from(uuid::Uuid::from_u128(0xA2))],
            "come over",
            now,
        )?;
        client.god_kick_user(AgentKey::from(uuid::Uuid::from_u128(9)), "spam", now)?;
        client.send_godlike_message("setregioninfo", &["1", "2"], now)?;
        client.autopilot_to(256_010.0, 256_020.0, 25.0, now)?;

        let relayed = assert_family(&mut client, &mut sim, now, AGENT_FAMILY)?;
        let AnyMessage::ScriptDialogReply(reply) = find(&relayed, "ScriptDialogReply")? else {
            return Err("expected a ScriptDialogReply".into());
        };
        assert_eq!(reply.data.chat_channel, -1234);
        assert_eq!(reply.data.button_index, 1);
        assert_eq!(trimmed(&reply.data.button_label), b"No");
        Ok(())
    }

    // ----- client → simulator, typed events not covered elsewhere -----------

    #[test]
    fn region_handshake_reply_is_surfaced_during_arrival() -> Result<(), TestError> {
        let now = Instant::now();
        let mut client = new_client()?;
        client.handle_login_response(success()?, now)?;
        client.notify_capabilities_ready(now)?;
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);

        // The client accepts a RegionHandshake only before its arrival
        // completes, so — like a real simulator — send it as soon as the
        // circuit opens, ahead of the AgentMovementComplete answer.
        let use_circuit = client.poll_transmit().ok_or("UseCircuitCode")?;
        sim.handle_datagram(client_addr(), &use_circuit.payload, now)?;
        assert!(
            drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::CircuitOpened { .. }))
        );
        sim.send_region_handshake(&region_identity(), now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_server(&mut sim);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ServerEvent::RegionHandshakeReplied)),
            "the client's RegionHandshakeReply surfaces as RegionHandshakeReplied, got {events:?}"
        );
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::RegionInfoHandshake(info) if info.region_id == uuid::Uuid::from_u128(0xBEEF)
            )),
            "the handshake decoded on the client, got {client_events:?}"
        );
        Ok(())
    }

    #[test]
    fn agent_update_surfaces_controls_and_camera() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;

        let camera = Camera::new_unchecked(
            Vector {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            },
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        );
        client.set_camera(camera.clone(), now)?;
        drain_server(&mut sim);
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);

        let controls = ControlFlags::AT_POS | ControlFlags::FLY;
        client.set_controls(controls, now)?;
        pump(&mut client, &mut sim, now)?;
        let update = drain_server(&mut sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::AgentUpdate(update) => Some(update),
                _ => None,
            })
            .ok_or("expected an AgentUpdate server event")?;
        assert_eq!(update.controls, controls);
        assert_eq!(update.camera, camera);
        Ok(())
    }

    #[test]
    fn attachment_drop_and_single_rez_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        client.drop_attachments(
            &[
                ScopedObjectId::new(circuit, RegionLocalObjectId(11)),
                ScopedObjectId::new(circuit, RegionLocalObjectId(12)),
            ],
            now,
        )?;
        let rez = RezAttachment {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x31)),
            owner_id: own_agent(),
            attachment_point: AttachmentPoint::Default,
            mode: AttachmentMode::Add,
            name: "Hat".to_owned(),
            description: "a hat".to_owned(),
        };
        client.rez_attachment(&rez, now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_server(&mut sim);

        let dropped = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DropAttachments(ids) => Some(ids.clone()),
                _ => None,
            })
            .ok_or("expected a DropAttachments server event")?;
        assert_eq!(
            dropped,
            vec![RegionLocalObjectId(11), RegionLocalObjectId(12)]
        );
        let rezzed = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezAttachment(rez) => Some(rez.as_ref().clone()),
                _ => None,
            })
            .ok_or("expected a RezAttachment server event")?;
        assert_eq!(rezzed, rez);
        Ok(())
    }

    #[test]
    fn spin_start_update_stop_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let object = ObjectKey::from(uuid::Uuid::from_u128(0x0B3));
        // A unit quaternion: the wire packs a normalised rotation, so anything
        // else would not survive the round trip.
        let rotation = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };

        client.spin_object_start(object, now)?;
        client.spin_object_update(object, rotation.clone(), now)?;
        client.spin_object_stop(object, now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_server(&mut sim);

        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::SpinObjectStart { object_id } if *object_id == object
            )),
            "expected SpinObjectStart, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::SpinObjectUpdate { object_id, rotation: seen }
                    if *object_id == object && *seen == rotation
            )),
            "expected SpinObjectUpdate, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::SpinObjectStop { object_id } if *object_id == object
            )),
            "expected SpinObjectStop, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn duplicate_objects_on_ray_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let circuit = client.root_circuit_id().ok_or("no circuit")?;
        let group = GroupKey::from(uuid::Uuid::from_u128(0x6713));
        let target = ObjectKey::from(uuid::Uuid::from_u128(0x0B4));
        let start = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let end = Vector {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        };

        client.duplicate_objects_on_ray(
            &[ScopedObjectId::new(circuit, RegionLocalObjectId(21))],
            Some(group),
            start.clone(),
            end.clone(),
            true,
            false,
            true,
            false,
            Some(target),
            0x40,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_server(&mut sim);
        let ServerEvent::DuplicateObjectsOnRay {
            local_ids,
            group_id,
            ray_start,
            ray_end,
            bypass_raycast,
            ray_end_is_intersection,
            copy_centers,
            copy_rotates,
            ray_target_id,
            duplicate_flags,
        } = events
            .iter()
            .find(|e| matches!(e, ServerEvent::DuplicateObjectsOnRay { .. }))
            .ok_or("expected a DuplicateObjectsOnRay server event")?
        else {
            return Err("unreachable: filtered to DuplicateObjectsOnRay".into());
        };
        assert_eq!(local_ids, &vec![RegionLocalObjectId(21)]);
        assert_eq!(*group_id, Some(group));
        assert_eq!(*ray_start, start);
        assert_eq!(*ray_end, end);
        assert!(*bypass_raycast);
        assert!(!*ray_end_is_intersection);
        assert!(*copy_centers);
        assert!(!*copy_rotates);
        assert_eq!(*ray_target_id, Some(target));
        assert_eq!(*duplicate_flags, 0x40);
        Ok(())
    }

    #[test]
    fn accepting_a_lure_surfaces_teleport_via_lure() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let lure = uuid::Uuid::from_u128(0x1E);

        client.accept_teleport_lure(LureId::from(lure), now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_server(&mut sim);
        let (lure_id, flags) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::TeleportViaLure {
                    lure_id,
                    teleport_flags,
                } => Some((*lure_id, *teleport_flags)),
                _ => None,
            })
            .ok_or("expected a TeleportViaLure server event")?;
        assert_eq!(lure_id, LureId::from(lure));
        assert_ne!(flags & sl_proto::TeleportFlags::VIA_LURE, 0);
        Ok(())
    }

    // ----- simulator → client, senders not covered elsewhere ----------------

    #[test]
    fn offline_notification_reaches_client_store() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let friend = FriendKey::from(uuid::Uuid::from_u128(0xF1));

        sim.send_online_notification(&[friend], now)?;
        pump(&mut client, &mut sim, now)?;
        assert!(client.is_online(friend), "online first");

        let later = after(now, 10)?;
        sim.send_offline_notification(&[friend], later)?;
        pump(&mut client, &mut sim, later)?;
        assert!(
            !client.is_online(friend),
            "send_offline_notification marked the buddy offline"
        );
        let events = drain_client(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::FriendsOffline(friends) if friends.as_slice() == [friend]
            )),
            "expected a FriendOffline event, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn single_parcel_overlay_chunk_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let data: Vec<u8> = (0..=255u8).collect();

        sim.send_parcel_overlay_chunk(2, &data, now)?;
        pump(&mut client, &mut sim, now)?;
        let chunks: Vec<(i32, Vec<u8>)> = drain_client(&mut client)
            .into_iter()
            .filter_map(|e| match e {
                Event::ParcelOverlay(info) => Some((info.sequence_id, info.data)),
                _ => None,
            })
            .collect();
        assert_eq!(chunks, vec![(2, data)]);
        Ok(())
    }

    #[test]
    fn chatterbox_agent_list_updates_reach_client_via_caps() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let group = GroupKey::from(uuid::Uuid::from_u128(0x64019));
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x9EE7));
        let kind = ChatSessionKind::Group { group_id: group };

        client.start_group_session(group, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        sim.enqueue_chatterbox_agent_list_updates(group.uuid(), &[(peer, true)]);
        deliver_caps(&mut client, &mut sim, now)?;
        let members: Vec<AgentKey> = client.session_voice_members(kind).collect();
        assert_eq!(members, vec![peer], "the ENTER update added the peer");

        sim.enqueue_chatterbox_agent_list_updates(group.uuid(), &[(peer, false)]);
        deliver_caps(&mut client, &mut sim, now)?;
        let members: Vec<AgentKey> = client.session_voice_members(kind).collect();
        assert!(members.is_empty(), "the LEAVE update removed the peer");
        Ok(())
    }
}
