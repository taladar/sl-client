//! Round-trip and dispatch tests for the generated LLUDP message types,
//! focused on the messages the login MVP needs.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::{Rotation, Vector};
    use uuid::Uuid;

    use sl_wire::messages::{
        AgentUpdate, AgentUpdateAgentDataBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
        LogoutRequest, LogoutRequestAgentDataBlock, PacketAck, PacketAckPacketsBlock,
        UseCircuitCode, UseCircuitCodeCircuitCodeBlock,
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
            id.encode(&mut writer);
            let bytes = writer.into_bytes();
            assert_eq!(bytes, expected);
            let mut reader = Reader::new(&bytes);
            assert_eq!(MessageId::decode(&mut reader)?, id);
        }
        Ok(())
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
        LogoutRequest::ID.encode(&mut writer);
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
