//! Integration test: the parser must handle the real, vendored
//! `message_template.msg` shipped with `sl-wire` in its entirety.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_msg_template::{Frequency, parse};

    /// The vendored template that `sl-wire` uses for code generation, embedded
    /// at compile time so the test needs no filesystem access.
    const TEMPLATE: &str = include_str!("../../sl-wire/message_template.msg");

    #[test]
    fn parses_the_whole_vendored_template() -> Result<(), Box<dyn std::error::Error>> {
        let template = parse(TEMPLATE)?;

        // The template declares format version 2.0.
        assert_eq!(template.version.as_deref(), Some("2.0"));

        // The real template defines several hundred messages; assert a generous
        // lower bound so this stays meaningful without being brittle.
        assert!(
            template.messages.len() > 400,
            "expected >400 messages, got {}",
            template.messages.len()
        );

        // Names are unique.
        let mut names: Vec<&str> = template.messages.iter().map(|m| m.name.as_str()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(total, names.len(), "duplicate message names found");

        Ok(())
    }

    #[test]
    fn known_messages_parse_as_expected() -> Result<(), Box<dyn std::error::Error>> {
        let template = parse(TEMPLATE)?;
        let find = |name: &str| template.messages.iter().find(|m| m.name == name).cloned();

        let use_circuit_code = find("UseCircuitCode").ok_or("UseCircuitCode missing")?;
        assert_eq!(use_circuit_code.frequency, Frequency::Low);
        assert_eq!(use_circuit_code.number, 3);
        let block = use_circuit_code.blocks.first().ok_or("no block")?;
        assert_eq!(block.name, "CircuitCode");
        assert_eq!(block.fields.len(), 3);

        let packet_ack = find("PacketAck").ok_or("PacketAck missing")?;
        assert_eq!(packet_ack.frequency, Frequency::Fixed);
        assert_eq!(packet_ack.number, 0xFFFF_FFFB);

        let agent_update = find("AgentUpdate").ok_or("AgentUpdate missing")?;
        assert_eq!(agent_update.frequency, Frequency::High);
        assert_eq!(agent_update.number, 4);

        Ok(())
    }
}
