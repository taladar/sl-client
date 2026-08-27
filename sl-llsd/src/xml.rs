//! The nesting guard every XML entry point in the workspace parses through.
//!
//! roxmltree's element parsing recurses and overflows the stack somewhere
//! between 1000 and 2000 levels of nesting, and a stack overflow aborts the
//! process rather than raising a catchable panic — so the depth has to be
//! bounded *before* the document is handed to it, not walked afterwards.
//!
//! Its own guards do not cover this: the parser's `depth` field bounds *entity
//! references* (the billion-laughs case, limit 10) and `nodes_limit` bounds node
//! **count**, defaulting to `u32::MAX`, which a deep-but-narrow document — one
//! node per level — never approaches.
//!
//! The bodies reaching these parsers are unauthenticated: an XML-RPC login
//! response arrives before a session exists, and CAPS bodies arrive from
//! whatever the seed capability points at.

/// Parses `xml` into a document, refusing one nested past
/// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH) first.
///
/// The single XML entry point for this workspace — see the module docs for why
/// calling [`roxmltree::Document::parse`] directly on a body off the wire is a
/// remote abort.
///
/// # Errors
///
/// Returns [`roxmltree::Error::NodesLimitReached`] for a document nested past
/// the limit — the nearest thing roxmltree's error type has to "this document
/// is too big to parse", and reported in preference to a silent success so the
/// refusal is not hidden — and otherwise whatever roxmltree reports for a body
/// that is not well-formed.
pub fn parse_guarded_xml(xml: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    if !xml_nesting_within(xml, crate::MAX_NESTING_DEPTH) {
        return Err(roxmltree::Error::NodesLimitReached);
    }
    roxmltree::Document::parse(xml)
}

/// Whether `xml`'s element nesting stays within `limit`.
///
/// A byte scan, run before the document is handed to roxmltree — see
/// [`parse_guarded_xml`] for why it cannot be left to a walk over the parsed
/// tree.
///
/// Comments, CDATA sections, processing instructions and the doctype are
/// skipped rather than counted, and a self-closing `<x/>` opens nothing — so a
/// document of many sibling empty elements is not mistaken for a deep one.
/// Being a scan rather than a parse it does not validate: anything malformed
/// enough to confuse it is rejected by roxmltree immediately afterwards.
#[must_use]
pub fn xml_nesting_within(xml: &str, limit: usize) -> bool {
    let bytes = xml.as_bytes();
    let mut depth: usize = 0;
    let mut index: usize = 0;
    while index < bytes.len() {
        let Some(rest) = bytes.get(index..) else {
            break;
        };
        if !rest.starts_with(b"<") {
            index = index.saturating_add(1);
            continue;
        }
        if rest.starts_with(b"<!--") {
            index = skip_past(bytes, index, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            index = skip_past(bytes, index, b"]]>");
        } else if rest.starts_with(b"<?") {
            index = skip_past(bytes, index, b"?>");
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            index = skip_tag(bytes, index).0;
        } else if rest.starts_with(b"<!") {
            index = skip_tag(bytes, index).0;
        } else {
            let (next, self_closing) = skip_tag(bytes, index);
            if !self_closing {
                depth = depth.saturating_add(1);
                if depth > limit {
                    return false;
                }
            }
            index = next;
        }
    }
    true
}

/// Advances past the tag starting at `start`, reporting the offset just after
/// its `>` and whether it closed itself (`<x/>`).
///
/// Quoted attribute values are honoured, so a `>` or a trailing `/` inside one
/// neither ends the tag nor reads as self-closing.
fn skip_tag(bytes: &[u8], start: usize) -> (usize, bool) {
    let mut index = start.saturating_add(1);
    let mut quote: Option<u8> = None;
    let mut last_significant = b'<';
    while let Some(&byte) = bytes.get(index) {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_open) => {}
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return (index.saturating_add(1), last_significant == b'/'),
                _other => {}
            },
        }
        if !byte.is_ascii_whitespace() {
            last_significant = byte;
        }
        index = index.saturating_add(1);
    }
    // Unterminated: roxmltree rejects it a moment later.
    (index, false)
}

/// The offset just past the next `needle` at or after `start`, or the end of
/// input if there is none.
fn skip_past(bytes: &[u8], start: usize, needle: &[u8]) -> usize {
    let mut index = start;
    while index < bytes.len() {
        if bytes
            .get(index..)
            .is_some_and(|rest| rest.starts_with(needle))
        {
            return index.saturating_add(needle.len());
        }
        index = index.saturating_add(1);
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{parse_guarded_xml, xml_nesting_within};

    /// A document deep enough to overflow roxmltree's recursion is refused
    /// before it reaches roxmltree — this test aborts the process rather than
    /// failing if the guard is removed.
    #[test]
    fn deeply_nested_xml_is_refused_before_roxmltree() {
        let depth = 4_000_usize;
        let xml = format!(
            "<root>{}<leaf/>{}</root>",
            "<array>".repeat(depth),
            "</array>".repeat(depth)
        );
        assert_eq!(
            parse_guarded_xml(&xml).err(),
            Some(roxmltree::Error::NodesLimitReached)
        );
    }

    /// Nesting the protocol actually produces parses as before.
    #[test]
    fn ordinary_xml_nesting_is_untouched_by_the_limit() {
        let xml = "<root><array><leaf>7</leaf></array></root>";
        let Ok(document) = parse_guarded_xml(xml) else {
            unreachable!("the fixture is well-formed and shallow")
        };
        assert_eq!(document.root_element().tag_name().name(), "root");
    }

    /// The scan counts *nesting*, not element starts: a flat document of many
    /// self-closing siblings is not mistaken for a deep one.
    #[test]
    fn self_closing_siblings_do_not_accumulate_depth() {
        let flat = format!("<llsd><array>{}</array></llsd>", "<undef/>".repeat(500));
        assert!(xml_nesting_within(&flat, crate::MAX_NESTING_DEPTH));
    }

    /// Comments, CDATA and processing instructions carry no depth, and a `>` or
    /// a trailing `/` inside a quoted attribute value ends nothing.
    #[test]
    fn markup_that_is_not_nesting_is_skipped() {
        let noise = concat!(
            "<?xml version=\"1.0\"?>",
            "<llsd><!-- <array><array> --><string>",
            "<![CDATA[ <array><array> ]]>",
            "</string><uri href=\"a>b/\"/></llsd>",
        );
        assert!(xml_nesting_within(noise, 3));
        assert!(!xml_nesting_within(noise, 1));
    }
}
