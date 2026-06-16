//! Build script: parses the vendored `message_template.msg` and generates Rust
//! types and (de)serialization code for every LLUDP message into `OUT_DIR`.
//!
//! The generated file is `include!`d by `src/messages.rs`. Because the output is
//! machine-generated, it begins with a set of blanket lint relaxations rather
//! than trying to satisfy every pedantic crate lint by construction.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sl_msg_template::{BlockDef, Cardinality, FieldType, MessageDef, Template, parse};

/// Entry point: read the template, generate code, write it to `OUT_DIR`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=message_template.msg");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let template_path = PathBuf::from(&manifest_dir).join("message_template.msg");
    let source = fs_err::read_to_string(&template_path)?;
    let template = parse(&source)?;

    let code = generate(&template);

    let out_dir = std::env::var("OUT_DIR")?;
    let out_path = PathBuf::from(out_dir).join("messages.rs");
    fs_err::write(&out_path, code)?;
    Ok(())
}

/// Appends formatted text to `out`. Writing to a `String` is infallible, so the
/// (impossible) error is discarded.
fn emit(out: &mut String, args: core::fmt::Arguments<'_>) {
    use core::fmt::Write as _;
    out.write_fmt(args).unwrap_or_default();
}

/// The imports that head the generated file. The lint relaxations are applied
/// by the hand-written `src/messages.rs` module that `include!`s this output
/// (inner attributes cannot be introduced through `include!`).
const PREAMBLE: &str = "\
use crate::error::WireError;
use crate::field::{Reader, Writer};
use crate::message::{Message, MessageId};

";

/// Generates the full contents of the `messages.rs` include file.
fn generate(template: &Template) -> String {
    let mut out = String::from(PREAMBLE);

    // Per-message type definitions and `Message` impls.
    for message in &template.messages {
        push_message_types(&mut out, message);
    }

    push_any_message(&mut out, template);
    out
}

/// Emits the block structs, the message struct, and the `Message` impl.
fn push_message_types(out: &mut String, message: &MessageDef) {
    for block in &message.blocks {
        push_block_struct(out, &message.name, block);
    }

    // The message struct: one field per block.
    emit(
        out,
        format_args!(
            "#[derive(Debug, Clone, PartialEq{})]\npub struct {} {{\n",
            eq_suffix(message.blocks.iter().all(block_is_eq)),
            message.name
        ),
    );
    for block in &message.blocks {
        let field = escape_ident(to_snake_case(&block.name));
        let ty = block_struct_name(&message.name, &block.name);
        let stored = match block.cardinality {
            Cardinality::Single => ty,
            Cardinality::Multiple(_) | Cardinality::Variable => format!("Vec<{ty}>"),
        };
        emit(out, format_args!("    pub {field}: {stored},\n"));
    }
    out.push_str("}\n\n");

    push_message_impl(out, message);
}

/// Emits the struct for one block of a message.
fn push_block_struct(out: &mut String, message_name: &str, block: &BlockDef) {
    let name = block_struct_name(message_name, &block.name);
    emit(
        out,
        format_args!(
            "#[derive(Debug, Clone, PartialEq{})]\npub struct {name} {{\n",
            eq_suffix(block.fields.iter().all(|f| field_is_eq(&f.ty)))
        ),
    );
    for field in &block.fields {
        let field_name = escape_ident(to_snake_case(&field.name));
        emit(
            out,
            format_args!("    pub {field_name}: {},\n", field_rust_type(&field.ty)),
        );
    }
    out.push_str("}\n\n");
}

/// Emits the `impl Message` block for a message.
fn push_message_impl(out: &mut String, message: &MessageDef) {
    emit(out, format_args!("impl Message for {} {{\n", message.name));
    emit(
        out,
        format_args!("    const NAME: &'static str = \"{}\";\n", message.name),
    );
    emit(
        out,
        format_args!("    const ID: MessageId = {};\n", message_id_expr(message)),
    );
    emit(
        out,
        format_args!(
            "    const ZEROCODED: bool = {};\n\n",
            matches!(message.encoding, sl_msg_template::Encoding::Zerocoded)
        ),
    );

    // encode_body
    out.push_str("    fn encode_body(&self, writer: &mut Writer) -> Result<(), WireError> {\n");
    for block in &message.blocks {
        push_block_encode(out, block);
    }
    out.push_str("        Ok(())\n    }\n\n");

    // decode_body
    out.push_str("    fn decode_body(reader: &mut Reader) -> Result<Self, WireError> {\n");
    out.push_str("        Ok(Self {\n");
    for block in &message.blocks {
        let field = escape_ident(to_snake_case(&block.name));
        emit(
            out,
            format_args!(
                "            {field}: {},\n",
                block_decode_expr(&message.name, block)
            ),
        );
    }
    out.push_str("        })\n    }\n}\n\n");
}

/// Emits the encode statements for one block.
fn push_block_encode(out: &mut String, block: &BlockDef) {
    let field = escape_ident(to_snake_case(&block.name));
    match block.cardinality {
        Cardinality::Single => {
            for f in &block.fields {
                let fname = escape_ident(to_snake_case(&f.name));
                emit(
                    out,
                    format_args!(
                        "        {}\n",
                        field_write_stmt(&f.ty, &format!("self.{field}.{fname}"))
                    ),
                );
            }
        }
        Cardinality::Multiple(_) => {
            emit(out, format_args!("        for item in &self.{field} {{\n"));
            for f in &block.fields {
                let fname = escape_ident(to_snake_case(&f.name));
                emit(
                    out,
                    format_args!(
                        "            {}\n",
                        field_write_stmt(&f.ty, &format!("item.{fname}"))
                    ),
                );
            }
            out.push_str("        }\n");
        }
        Cardinality::Variable => {
            emit(
                out,
                format_args!(
                    "        writer.put_u8(u8::try_from(self.{field}.len()).map_err(|_e| WireError::VariableTooLong {{ len: self.{field}.len(), max: 255 }})?);\n"
                ),
            );
            emit(out, format_args!("        for item in &self.{field} {{\n"));
            for f in &block.fields {
                let fname = escape_ident(to_snake_case(&f.name));
                emit(
                    out,
                    format_args!(
                        "            {}\n",
                        field_write_stmt(&f.ty, &format!("item.{fname}"))
                    ),
                );
            }
            out.push_str("        }\n");
        }
    }
}

/// Builds the decode expression yielding the stored value for one block.
fn block_decode_expr(message_name: &str, block: &BlockDef) -> String {
    let struct_name = block_struct_name(message_name, &block.name);
    let construct = block_construct_expr(&struct_name, block);
    match block.cardinality {
        Cardinality::Single => construct,
        Cardinality::Multiple(count) => format!(
            "{{ let mut items = Vec::with_capacity({count}); for _ in 0..{count}u32 {{ items.push({construct}); }} items }}"
        ),
        // A missing count byte (end of data) yields an empty block rather than
        // an error, so messages that omit trailing optional `Variable` blocks
        // (e.g. OpenSim's shorter `RegionInfo`) still decode.
        Cardinality::Variable => format!(
            "{{ let count = reader.u8().unwrap_or(0); let mut items = Vec::with_capacity(usize::from(count)); for _ in 0..count {{ items.push({construct}); }} items }}"
        ),
    }
}

/// Builds a single block-struct construction expression reading each field.
fn block_construct_expr(struct_name: &str, block: &BlockDef) -> String {
    let mut out = format!("{struct_name} {{ ");
    for f in &block.fields {
        let fname = escape_ident(to_snake_case(&f.name));
        emit(
            &mut out,
            format_args!("{fname}: {}, ", field_read_expr(&f.ty)),
        );
    }
    out.push('}');
    out
}

/// Emits the `AnyMessage` enum and its dispatch impl.
fn push_any_message(out: &mut String, template: &Template) {
    out.push_str("/// Any decoded incoming message.\n#[derive(Debug, Clone, PartialEq)]\npub enum AnyMessage {\n");
    for message in &template.messages {
        emit(out, format_args!("    {0}({0}),\n", message.name));
    }
    out.push_str("}\n\n");

    out.push_str("impl AnyMessage {\n");

    // id()
    out.push_str("    pub fn id(&self) -> MessageId {\n        match self {\n");
    for message in &template.messages {
        emit(
            out,
            format_args!("            Self::{0}(_) => {0}::ID,\n", message.name),
        );
    }
    out.push_str("        }\n    }\n\n");

    // name()
    out.push_str("    pub fn name(&self) -> &'static str {\n        match self {\n");
    for message in &template.messages {
        emit(
            out,
            format_args!("            Self::{0}(_) => {0}::NAME,\n", message.name),
        );
    }
    out.push_str("        }\n    }\n\n");

    // encode_body()
    out.push_str("    pub fn encode_body(&self, writer: &mut Writer) -> Result<(), WireError> {\n        match self {\n");
    for message in &template.messages {
        emit(
            out,
            format_args!(
                "            Self::{0}(message) => message.encode_body(writer),\n",
                message.name
            ),
        );
    }
    out.push_str("        }\n    }\n\n");

    // decode()
    out.push_str("    pub fn decode(id: MessageId, reader: &mut Reader) -> Result<Self, WireError> {\n        match id {\n");
    let mut seen = BTreeSet::new();
    for message in &template.messages {
        let pattern = message_id_pattern(message);
        if seen.insert(pattern.clone()) {
            emit(
                out,
                format_args!(
                    "            {pattern} => Ok(Self::{0}({0}::decode_body(reader)?)),\n",
                    message.name
                ),
            );
        }
    }
    out.push_str("            other => Err(WireError::UnknownMessage { id: other }),\n");
    out.push_str("        }\n    }\n}\n");
}

/// Returns `", Eq"` when a struct may also derive `Eq`, otherwise `""`.
const fn eq_suffix(is_eq: bool) -> &'static str {
    if is_eq { ", Eq" } else { "" }
}

/// Whether a block's fields all permit deriving `Eq`.
fn block_is_eq(block: &BlockDef) -> bool {
    block.fields.iter().all(|f| field_is_eq(&f.ty))
}

/// Whether a field type permits deriving `Eq` (no floating point).
const fn field_is_eq(ty: &FieldType) -> bool {
    !matches!(
        ty,
        FieldType::F32
            | FieldType::F64
            | FieldType::Vector3
            | FieldType::Vector3d
            | FieldType::Vector4
            | FieldType::Quaternion
    )
}

/// The generated struct name for a block within a message.
fn block_struct_name(message_name: &str, block_name: &str) -> String {
    format!("{message_name}{block_name}Block")
}

/// The Rust type for a field.
fn field_rust_type(ty: &FieldType) -> String {
    match ty {
        FieldType::U8 => "u8".to_owned(),
        FieldType::U16 | FieldType::IpPort => "u16".to_owned(),
        FieldType::U32 => "u32".to_owned(),
        FieldType::U64 => "u64".to_owned(),
        FieldType::S8 => "i8".to_owned(),
        FieldType::S16 => "i16".to_owned(),
        FieldType::S32 => "i32".to_owned(),
        FieldType::F32 => "f32".to_owned(),
        FieldType::F64 => "f64".to_owned(),
        FieldType::Uuid => "uuid::Uuid".to_owned(),
        FieldType::Vector3 => "sl_types::lsl::Vector".to_owned(),
        FieldType::Vector3d => "[f64; 3]".to_owned(),
        FieldType::Vector4 => "[f32; 4]".to_owned(),
        FieldType::Quaternion => "sl_types::lsl::Rotation".to_owned(),
        FieldType::Bool => "bool".to_owned(),
        FieldType::IpAddr => "[u8; 4]".to_owned(),
        FieldType::Variable { .. } => "Vec<u8>".to_owned(),
        FieldType::Fixed { bytes } => format!("[u8; {bytes}]"),
    }
}

/// The expression reading a field value from `reader`.
fn field_read_expr(ty: &FieldType) -> String {
    match ty {
        FieldType::U8 => "reader.u8()?".to_owned(),
        FieldType::U16 | FieldType::IpPort => "reader.u16()?".to_owned(),
        FieldType::U32 => "reader.u32()?".to_owned(),
        FieldType::U64 => "reader.u64()?".to_owned(),
        FieldType::S8 => "reader.i8()?".to_owned(),
        FieldType::S16 => "reader.i16()?".to_owned(),
        FieldType::S32 => "reader.i32()?".to_owned(),
        FieldType::F32 => "reader.f32()?".to_owned(),
        FieldType::F64 => "reader.f64()?".to_owned(),
        FieldType::Uuid => "reader.uuid()?".to_owned(),
        FieldType::Vector3 => "reader.vector3()?".to_owned(),
        FieldType::Vector3d => "reader.vector3d()?".to_owned(),
        FieldType::Vector4 => "reader.vector4()?".to_owned(),
        FieldType::Quaternion => "reader.quaternion()?".to_owned(),
        FieldType::Bool => "reader.bool()?".to_owned(),
        FieldType::IpAddr => "reader.take_array::<4>()?".to_owned(),
        FieldType::Variable { length_bytes: 2 } => "reader.variable2()?.to_vec()".to_owned(),
        FieldType::Variable { .. } => "reader.variable1()?.to_vec()".to_owned(),
        FieldType::Fixed { bytes } => format!("reader.take_array::<{bytes}>()?"),
    }
}

/// The statement writing a field value `access` to `writer`.
fn field_write_stmt(ty: &FieldType, access: &str) -> String {
    match ty {
        FieldType::U8 => format!("writer.put_u8({access});"),
        FieldType::U16 | FieldType::IpPort => format!("writer.put_u16({access});"),
        FieldType::U32 => format!("writer.put_u32({access});"),
        FieldType::U64 => format!("writer.put_u64({access});"),
        FieldType::S8 => format!("writer.put_i8({access});"),
        FieldType::S16 => format!("writer.put_i16({access});"),
        FieldType::S32 => format!("writer.put_i32({access});"),
        FieldType::F32 => format!("writer.put_f32({access});"),
        FieldType::F64 => format!("writer.put_f64({access});"),
        FieldType::Uuid => format!("writer.put_uuid({access});"),
        FieldType::Vector3 => format!("writer.put_vector3(&{access});"),
        FieldType::Vector3d => format!("writer.put_vector3d({access});"),
        FieldType::Vector4 => format!("writer.put_vector4({access});"),
        FieldType::Quaternion => format!("writer.put_quaternion(&{access});"),
        FieldType::Bool => format!("writer.put_bool({access});"),
        FieldType::IpAddr | FieldType::Fixed { .. } => format!("writer.bytes(&{access});"),
        FieldType::Variable { length_bytes: 2 } => format!("writer.put_variable2(&{access})?;"),
        FieldType::Variable { .. } => format!("writer.put_variable1(&{access})?;"),
    }
}

/// The `MessageId` constant expression for a message.
fn message_id_expr(message: &MessageDef) -> String {
    match message.frequency {
        sl_msg_template::Frequency::High => format!("MessageId::High({})", message.number),
        sl_msg_template::Frequency::Medium => format!("MessageId::Medium({})", message.number),
        sl_msg_template::Frequency::Low => format!("MessageId::Low({})", message.number),
        sl_msg_template::Frequency::Fixed => {
            format!("MessageId::Fixed({:#010X})", message.number)
        }
    }
}

/// The `MessageId` match pattern for a message (identical form to its constant).
fn message_id_pattern(message: &MessageDef) -> String {
    message_id_expr(message)
}

/// Converts a `PascalCase`/mixed template identifier to `snake_case`.
fn to_snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (index, &current) in chars.iter().enumerate() {
        if current.is_uppercase() {
            let prev = index.checked_sub(1).and_then(|k| chars.get(k)).copied();
            let next = index.checked_add(1).and_then(|k| chars.get(k)).copied();
            let prev_is_word = prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit());
            let prev_is_upper = prev.is_some_and(char::is_uppercase);
            let next_is_lower = next.is_some_and(char::is_lowercase);
            if !out.is_empty() && (prev_is_word || (prev_is_upper && next_is_lower)) {
                out.push('_');
            }
            out.extend(current.to_lowercase());
        } else {
            out.push(current);
        }
    }
    out
}

/// Escapes a snake-case identifier that collides with a Rust keyword.
fn escape_ident(name: String) -> String {
    match name.as_str() {
        // Keywords that cannot be raw identifiers get a trailing underscore.
        "crate" | "self" | "super" | "Self" => format!("{name}_"),
        // Other (reserved) keywords are escaped as raw identifiers.
        "as" | "break" | "const" | "continue" | "else" | "enum" | "extern" | "false" | "fn"
        | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut"
        | "pub" | "ref" | "return" | "static" | "struct" | "trait" | "true" | "type" | "unsafe"
        | "use" | "where" | "while" | "async" | "await" | "dyn" | "abstract" | "become" | "box"
        | "do" | "final" | "macro" | "override" | "priv" | "typeof" | "unsized" | "virtual"
        | "yield" | "try" | "union" => format!("r#{name}"),
        _ => name,
    }
}
