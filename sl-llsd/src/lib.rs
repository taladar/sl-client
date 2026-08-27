#![doc = include_str!("../README.md")]

mod binary;
mod error;
mod notation;
mod value;
mod xml;

pub use binary::{parse_llsd_binary, parse_llsd_binary_prefix};
pub use error::LlsdError;
pub use notation::{Scan, parse_llsd_notation};
pub use value::{Llsd, parse_llsd_xml, push_escaped};
pub use xml::{parse_guarded_xml, xml_nesting_within};

/// How deeply arrays and maps may nest before a parse is rejected.
///
/// Every parser here is recursive descent, so nesting depth is stack depth and
/// an unbounded one is a remote crash: notation costs a **single byte** per
/// level (`[[[[[…`), binary five, and the input is unauthenticated — a CAPS
/// body, or a mesh header, which any resident can upload and which the viewer
/// fetches automatically. A stack overflow is not a catchable panic, so the
/// limit has to be enforced before the recursion, not recovered from after it.
///
/// XML is the same hazard one layer down: the recursion that overflows is
/// roxmltree's own, before any tree is handed back — see
/// [`parse_guarded_xml`], which is the entry point every XML body in the
/// workspace is parsed through.
///
/// The reference threads an equivalent `max_depth` through
/// `LLSDParser::doParse` / `parseMap` / `parseArray` and fails the parse when it
/// reaches zero (`llsdserialize.cpp`), but defaults it to `-1` — unlimited — so
/// the concrete ceiling here is ours. It is far above anything the protocol
/// produces (a mesh header nests twice; the deepest CAPS bodies single digits)
/// and far below what overflows a thread stack.
pub const MAX_NESTING_DEPTH: usize = 128;
