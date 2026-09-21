//! Round-trip and dispatch tests for the generated LLUDP message types,
//! focused on the messages the login MVP needs.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::{Rotation, Vector};
    use uuid::Uuid;

    use sl_wire::messages::{
        AgentUpdate, AgentUpdateAgentDataBlock, AvatarGroupsReply, CompletePingCheck,
        CompletePingCheckPingIDBlock, LogoutRequest, LogoutRequestAgentDataBlock, PacketAck,
        PacketAckPacketsBlock, RegionInfo, RegionInfoAgentDataBlock, RegionInfoRegionInfo2Block,
        RegionInfoRegionInfoBlock, TestMessage, TestMessageNeighborBlockBlock,
        TestMessageTestBlock1Block, UseCircuitCode, UseCircuitCodeCircuitCodeBlock,
    };
    use sl_wire::{AnyMessage, Message, MessageId, Reader, Writer};

    /// Encodes a message body to bytes.
    fn encode<M: Message>(message: &M) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut writer = Writer::new();
        message.encode_body(&mut writer)?;
        Ok(writer.into_bytes())
    }

    #[test]
    fn use_circuit_code_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let message = UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: 0x0102_0304,
                session_id: Uuid::from_u128(0x1111_2222),
                id: Uuid::from_u128(0x3333_4444),
            },
        };
        let bytes = encode(&message)?;
        // u32 code (little-endian) + two 16-byte UUIDs.
        assert_eq!(bytes.len(), 4 + 16 + 16);
        assert_eq!(bytes.get(0..4), Some(&[0x04, 0x03, 0x02, 0x01][..]));

        let mut reader = Reader::new(&bytes);
        let decoded = UseCircuitCode::decode_body(&mut reader)?;
        assert_eq!(decoded, message);
        assert!(reader.is_empty());
        assert_eq!(UseCircuitCode::ID, MessageId::Low(3));
        Ok(())
    }

    #[test]
    fn packet_ack_variable_block_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let message = PacketAck {
            packets: vec![
                PacketAckPacketsBlock { id: 7 },
                PacketAckPacketsBlock { id: 8 },
                PacketAckPacketsBlock { id: 9 },
            ],
        };
        let bytes = encode(&message)?;
        // One count byte (3) then three little-endian u32s.
        assert_eq!(bytes.first(), Some(&3u8));
        assert_eq!(bytes.len(), 1 + 3 * 4);

        let mut reader = Reader::new(&bytes);
        let decoded = PacketAck::decode_body(&mut reader)?;
        assert_eq!(decoded, message);
        assert_eq!(PacketAck::ID, MessageId::Fixed(0xFFFF_FFFB));
        Ok(())
    }

    #[test]
    fn complete_ping_check_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let message = CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock { ping_id: 42 },
        };
        let bytes = encode(&message)?;
        let mut reader = Reader::new(&bytes);
        assert_eq!(CompletePingCheck::decode_body(&mut reader)?, message);
        assert_eq!(CompletePingCheck::ID, MessageId::High(2));
        Ok(())
    }

    #[test]
    fn agent_update_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
        let message = AgentUpdate {
            agent_data: AgentUpdateAgentDataBlock {
                agent_id: Uuid::from_u128(1),
                session_id: Uuid::from_u128(2),
                body_rotation: identity.clone(),
                head_rotation: identity,
                state: 0,
                camera_center: Vector {
                    x: 128.0,
                    y: 64.0,
                    z: 32.0,
                },
                camera_at_axis: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                camera_left_axis: Vector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
                camera_up_axis: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                far: 256.0,
                control_flags: 0,
                flags: 0,
            },
        };
        let bytes = encode(&message)?;
        let mut reader = Reader::new(&bytes);
        // The quaternion `s` is reconstructed; the identity rotation reconstructs
        // exactly, so the decoded message equals the original.
        assert_eq!(AgentUpdate::decode_body(&mut reader)?, message);
        assert_eq!(AgentUpdate::ID, MessageId::High(4));
        Ok(())
    }

    #[test]
    fn message_id_codes_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let cases = [
            (MessageId::High(4), vec![0x04]),
            (MessageId::Medium(7), vec![0xFF, 0x07]),
            (MessageId::Low(3), vec![0xFF, 0xFF, 0x00, 0x03]),
            (MessageId::Fixed(0xFFFF_FFFB), vec![0xFF, 0xFF, 0xFF, 0xFB]),
        ];
        for (id, expected) in cases {
            let mut writer = Writer::new();
            id.encode(&mut writer)?;
            let bytes = writer.into_bytes();
            assert_eq!(bytes, expected);
            let mut reader = Reader::new(&bytes);
            assert_eq!(MessageId::decode(&mut reader)?, id);
        }
        Ok(())
    }

    /// The frequency coding is not injective over every `Low` value a caller
    /// could build by hand: `Low(n)` for `n >= 0xFF00` writes `FF FF FF xx`,
    /// which [`MessageId::decode`] reads back as a `Fixed` id. No such message
    /// exists — the template's largest `Low` is `0x01AF` — so the encoder
    /// refuses the value rather than emitting bytes that name something else.
    #[test]
    fn a_low_id_that_would_decode_as_fixed_is_refused() {
        let mut writer = Writer::new();
        assert_eq!(
            MessageId::Low(0xFF00).encode(&mut writer),
            Err(sl_wire::WireError::ValueOutOfRange {
                field: "MessageId::Low",
                value: 0xFF00,
            })
        );
        // The value one below it is still representable, and still round-trips.
        let mut writer = Writer::new();
        assert_eq!(MessageId::Low(0xFEFF).encode(&mut writer), Ok(()));
        let bytes = writer.into_bytes();
        assert_eq!(bytes, vec![0xFF, 0xFF, 0xFE, 0xFF]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(MessageId::decode(&mut reader), Ok(MessageId::Low(0xFEFF)));
    }

    #[test]
    fn any_message_dispatch_decodes_by_id() -> Result<(), Box<dyn std::error::Error>> {
        let message = LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: Uuid::from_u128(5),
                session_id: Uuid::from_u128(6),
            },
        };

        // Build id prefix + body, the way a full datagram body is laid out.
        let mut writer = Writer::new();
        LogoutRequest::ID.encode(&mut writer)?;
        message.encode_body(&mut writer)?;
        let bytes = writer.into_bytes();

        let mut reader = Reader::new(&bytes);
        let id = MessageId::decode(&mut reader)?;
        let decoded = AnyMessage::decode(id, &mut reader)?;
        assert_eq!(decoded, AnyMessage::LogoutRequest(message));
        assert_eq!(decoded.id(), MessageId::Low(252));
        assert_eq!(decoded.name(), "LogoutRequest");
        Ok(())
    }

    /// A `Multiple N` block's repeat count is fixed by the template and never
    /// written, so the decoder always reads exactly `N`. Handing the encoder a
    /// vector of any other length would emit a packet that decodes as
    /// something else — borrowing bytes from whatever follows, or stranding
    /// bytes before it — so the encoder refuses instead of writing it.
    #[test]
    fn a_fixed_count_block_refuses_a_vector_of_the_wrong_length()
    -> Result<(), Box<dyn std::error::Error>> {
        let block = |test0| TestMessageNeighborBlockBlock {
            test0,
            test1: 0,
            test2: 0,
        };
        let exact = TestMessage {
            test_block1: TestMessageTestBlock1Block { test1: 42 },
            neighbor_block: vec![block(0), block(1), block(2), block(3)],
        };
        // The template fixes four, so four encode with no count byte at all:
        // the U32 of TestBlock1 plus four blocks of three U32s.
        assert_eq!(encode(&exact)?.len(), 4 + 4 * 3 * 4);

        for short_or_long in [vec![block(0)], vec![block(0); 5]] {
            let found = short_or_long.len();
            let wrong = TestMessage {
                test_block1: TestMessageTestBlock1Block { test1: 42 },
                neighbor_block: short_or_long,
            };
            let mut writer = Writer::new();
            assert_eq!(
                wrong.encode_body(&mut writer),
                Err(sl_wire::WireError::BlockCountMismatch {
                    block: "NeighborBlock",
                    expected: 4,
                    found,
                })
            );
        }
        Ok(())
    }

    /// A `Variable` block's repeat-count byte is only optional where the block
    /// may be omitted altogether — the message's *trailing* run of `Variable`
    /// blocks, which is what OpenSim's shorter `RegionInfo` drops. Elsewhere
    /// the count is required, and a body that ends there is short: the
    /// zero-tail reader that stands in for the reference's "ran off the end of
    /// the packet" behaviour counts the missing byte as one it filled in, so
    /// the decode is reported rather than silent.
    #[test]
    fn a_missing_count_byte_is_only_free_for_a_trailing_variable_block()
    -> Result<(), Box<dyn std::error::Error>> {
        // AvatarGroupsReply is {AgentData Single}{GroupData Variable}
        // {NewGroupData Single}: GroupData's count is not optional.
        let mut reader = Reader::with_zero_tail(&[]);
        let decoded = AvatarGroupsReply::decode_body(&mut reader)?;
        assert!(decoded.group_data.is_empty());
        // Two UUIDs, the count byte, and NewGroupData's BOOL.
        assert_eq!(reader.zero_filled(), 16 + 16 + 1 + 1);

        // RegionInfo ends in three Variable blocks, and a sender that stops
        // after RegionInfo2 is sending a legal shorter message rather than a
        // truncated one — so a *strict* reader still decodes it.
        let shorter = region_info_through_region_info2();
        let mut reader = Reader::new(&shorter);
        let decoded = RegionInfo::decode_body(&mut reader)?;
        assert_eq!(decoded.region_info2.product_name, b"Mainland");
        assert!(decoded.region_info3.is_empty());
        assert!(decoded.region_info5.is_empty());
        assert!(decoded.combat_settings.is_empty());
        assert!(reader.is_empty());
        Ok(())
    }

    /// A `RegionInfo` body carrying only the blocks OpenSim's shorter form
    /// sends: `AgentData`, `RegionInfo` and `RegionInfo2`, and nothing after.
    fn region_info_through_region_info2() -> Vec<u8> {
        let message = RegionInfo {
            agent_data: RegionInfoAgentDataBlock {
                agent_id: Uuid::from_u128(1),
                session_id: Uuid::from_u128(2),
            },
            region_info: RegionInfoRegionInfoBlock {
                sim_name: b"Test Region".to_vec(),
                estate_id: 1,
                parent_estate_id: 1,
                region_flags: 0,
                sim_access: 13,
                max_agents: 40,
                billable_factor: 1.0,
                object_bonus_factor: 1.0,
                water_height: 20.0,
                terrain_raise_limit: 4.0,
                terrain_lower_limit: -4.0,
                price_per_meter: 1,
                redirect_grid_x: 0,
                redirect_grid_y: 0,
                use_estate_sun: true,
                sun_hour: 12.0,
            },
            region_info2: RegionInfoRegionInfo2Block {
                product_sku: b"023".to_vec(),
                product_name: b"Mainland".to_vec(),
                max_agents32: 40,
                hard_max_agents: 100,
                hard_max_objects: 15000,
            },
            region_info3: Vec::new(),
            region_info5: Vec::new(),
            combat_settings: Vec::new(),
        };
        let mut writer = Writer::new();
        // The three trailing blocks encode as a `0` count byte each; drop them
        // to get the body a sender that omits them entirely would send.
        let Ok(()) = message.encode_body(&mut writer) else {
            return Vec::new();
        };
        let mut bytes = writer.into_bytes();
        bytes.truncate(bytes.len().saturating_sub(3));
        bytes
    }

    #[test]
    fn unknown_message_id_is_reported() {
        // High 200 is not a defined message.
        let mut reader = Reader::new(&[]);
        let result = AnyMessage::decode(MessageId::High(200), &mut reader);
        assert!(matches!(
            result,
            Err(sl_wire::WireError::UnknownMessage { .. })
        ));
    }
}
