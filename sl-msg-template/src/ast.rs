//! The abstract syntax tree produced by parsing a Linden Lab `message_template.msg`
//! file (template format version 2.0).
//!
//! The tree is intentionally a faithful, lossless-enough representation of the
//! template for the purpose of generating wire (de)serialization code: it keeps
//! the message frequency and number, the trust and encoding attributes, any
//! trailing flags, and the full block/field structure.

/// A complete parsed message template file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The declared template format version (the token following `version`),
    /// e.g. `"2.0"`. `None` if the file contained no `version` line.
    pub version: Option<String>,
    /// All message definitions in the order they appear in the file.
    pub messages: Vec<MessageDef>,
}

/// The frequency class of a message, which determines how its numeric id is
/// encoded on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frequency {
    /// High frequency: a single-byte id.
    High,
    /// Medium frequency: `0xFF` followed by a one-byte id.
    Medium,
    /// Low frequency: `0xFF 0xFF` followed by a two-byte big-endian id.
    Low,
    /// Fixed frequency: the four-byte id is the literal number given in the
    /// template (e.g. `0xFFFFFFFB`).
    Fixed,
}

/// Whether a message may be sent across a trusted (inter-simulator) circuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The message is only accepted on trusted circuits.
    Trusted,
    /// The message is accepted from untrusted (client) circuits.
    NotTrusted,
}

/// The default body encoding for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// The body is zero-run-length encoded by default.
    Zerocoded,
    /// The body is sent verbatim.
    Unencoded,
}

/// A single message definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDef {
    /// The message name, e.g. `UseCircuitCode`.
    pub name: String,
    /// The frequency class of the message.
    pub frequency: Frequency,
    /// The numeric id within the frequency class for `High`/`Medium`/`Low`, or
    /// the full four-byte id for `Fixed`.
    pub number: u32,
    /// The trust attribute of the message.
    pub trust: Trust,
    /// The default body encoding of the message.
    pub encoding: Encoding,
    /// Any trailing attribute flags on the message header line, verbatim
    /// (e.g. `UDPDeprecated`, `UDPBlackListed`, `Deprecated`).
    pub flags: Vec<String>,
    /// The blocks making up the message body, in order.
    pub blocks: Vec<BlockDef>,
}

impl MessageDef {
    /// Returns `true` if the message carries any flag marking it deprecated.
    #[must_use]
    pub fn is_deprecated(&self) -> bool {
        self.flags
            .iter()
            .any(|flag| flag == "Deprecated" || flag == "UDPDeprecated")
    }
}

/// How many times a block repeats within a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// The block appears exactly once.
    Single,
    /// The block appears a fixed number of times.
    Multiple(u32),
    /// The block appears a variable number of times, prefixed on the wire by a
    /// single count byte.
    Variable,
}

/// A block within a message: a named, possibly repeated group of fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDef {
    /// The block name, e.g. `CircuitCode`.
    pub name: String,
    /// How many times the block repeats.
    pub cardinality: Cardinality,
    /// The fields making up the block, in order.
    pub fields: Vec<FieldDef>,
}

/// A single field within a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// The field name, e.g. `SessionID`.
    pub name: String,
    /// The field's wire type.
    pub ty: FieldType,
}

/// The wire type of a field as declared in the template.
#[expect(
    variant_size_differences,
    reason = "FieldType is a small Copy value enum; boxing the two data-carrying variants would only add indirection"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
    /// Signed 8-bit integer.
    S8,
    /// Signed 16-bit integer.
    S16,
    /// Signed 32-bit integer.
    S32,
    /// 32-bit IEEE float.
    F32,
    /// 64-bit IEEE float.
    F64,
    /// A 128-bit UUID (16 bytes).
    Uuid,
    /// A vector of three 32-bit floats.
    Vector3,
    /// A vector of three 64-bit floats.
    Vector3d,
    /// A vector of four 32-bit floats.
    Vector4,
    /// A quaternion sent as three 32-bit floats (the fourth component is
    /// reconstructed).
    Quaternion,
    /// A one-byte boolean.
    Bool,
    /// A four-byte IPv4 address.
    IpAddr,
    /// A two-byte port number.
    IpPort,
    /// A variable-length byte string prefixed by `length_bytes` length bytes
    /// (either 1 or 2).
    Variable {
        /// The number of bytes in the length prefix (1 or 2).
        length_bytes: u8,
    },
    /// A fixed-length byte array of `bytes` bytes.
    Fixed {
        /// The number of bytes in the array.
        bytes: u32,
    },
}
