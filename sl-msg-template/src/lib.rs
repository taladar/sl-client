#![doc = include_str!("../README.md")]

mod ast;
mod error;
mod lexer;
mod parser;

pub use ast::{
    BlockDef, Cardinality, Encoding, FieldDef, FieldType, Frequency, MessageDef, Template, Trust,
};
pub use error::ParseError;
pub use parser::parse;

#[cfg(test)]
mod test {
    use super::{Cardinality, Encoding, FieldType, Frequency, ParseError, Trust, parse};
    use pretty_assertions::assert_eq;

    /// Parsing a single fixed-frequency message with one variable block.
    #[test]
    fn parses_packet_ack() -> Result<(), ParseError> {
        let src = "
            {
                PacketAck Fixed 0xFFFFFFFB NotTrusted Unencoded
                {
                    Packets Variable
                    {   ID  U32 }
                }
            }
        ";
        let template = parse(src)?;
        assert_eq!(template.messages.len(), 1);
        let message = template
            .messages
            .first()
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "a message".to_owned(),
            })?;
        assert_eq!(message.name, "PacketAck");
        assert_eq!(message.frequency, Frequency::Fixed);
        assert_eq!(message.number, 0xFFFF_FFFB);
        assert_eq!(message.trust, Trust::NotTrusted);
        assert_eq!(message.encoding, Encoding::Unencoded);
        assert_eq!(message.blocks.len(), 1);
        let block = message
            .blocks
            .first()
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "a block".to_owned(),
            })?;
        assert_eq!(block.name, "Packets");
        assert_eq!(block.cardinality, Cardinality::Variable);
        assert_eq!(block.fields.len(), 1);
        Ok(())
    }

    /// The `version` line, comments, `Multiple` blocks and `Variable N` fields.
    #[test]
    fn parses_version_comments_and_variants() -> Result<(), ParseError> {
        let src = "
            // a leading comment
            version 2.0

            {
                UseCircuitCode Low 3 NotTrusted Unencoded // trailing comment
                {
                    CircuitCode Single
                    {   Code        U32     }
                    {   SessionID   LLUUID  }
                    {   ID          LLUUID  }
                }
            }
            {
                Neighbors Medium 7 Trusted Zerocoded
                {
                    Block Multiple 4
                    {   Name   Variable 1 }
                    {   Big    Variable 2 }
                    {   Color  Fixed    4 }
                }
            }
        ";
        let template = parse(src)?;
        assert_eq!(template.version.as_deref(), Some("2.0"));
        assert_eq!(template.messages.len(), 2);

        let neighbors = template
            .messages
            .get(1)
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "the second message".to_owned(),
            })?;
        assert_eq!(neighbors.frequency, Frequency::Medium);
        assert_eq!(neighbors.trust, Trust::Trusted);
        assert_eq!(neighbors.encoding, Encoding::Zerocoded);
        let block = neighbors
            .blocks
            .first()
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "a block".to_owned(),
            })?;
        assert_eq!(block.cardinality, Cardinality::Multiple(4));
        assert_eq!(
            block.fields.first().map(|field| field.ty),
            Some(FieldType::Variable { length_bytes: 1 })
        );
        assert_eq!(
            block.fields.get(1).map(|field| field.ty),
            Some(FieldType::Variable { length_bytes: 2 })
        );
        assert_eq!(
            block.fields.get(2).map(|field| field.ty),
            Some(FieldType::Fixed { bytes: 4 })
        );
        Ok(())
    }

    /// A message with no blocks (e.g. `CloseCircuit`) and a trailing flag.
    #[test]
    fn parses_empty_message_and_flags() -> Result<(), ParseError> {
        let src = "
            {
                CloseCircuit Fixed 0xFFFFFFFD NotTrusted Unencoded
            }
            {
                Deprecated Low 5 NotTrusted Unencoded UDPDeprecated
            }
        ";
        let template = parse(src)?;
        assert_eq!(template.messages.len(), 2);
        let close = template
            .messages
            .first()
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "a message".to_owned(),
            })?;
        assert_eq!(close.blocks.len(), 0);
        let deprecated = template
            .messages
            .get(1)
            .ok_or_else(|| ParseError::UnexpectedEof {
                expected: "a message".to_owned(),
            })?;
        assert_eq!(deprecated.flags, vec!["UDPDeprecated".to_owned()]);
        assert!(deprecated.is_deprecated());
        Ok(())
    }

    /// Unknown keywords surface as errors rather than panics.
    #[test]
    fn rejects_unknown_frequency() {
        let src = "{ Bad Wat 1 NotTrusted Unencoded }";
        assert!(matches!(
            parse(src),
            Err(ParseError::UnknownFrequency { .. })
        ));
    }
}
