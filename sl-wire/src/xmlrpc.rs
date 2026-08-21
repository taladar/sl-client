//! A small generic XML-RPC codec over [`Llsd`] values.
//!
//! Second Life's XML-RPC surfaces — the login endpoint, the `get_grid_info`
//! sibling, and the economy helper URIs (`currency.php`, `landtool.php`) —
//! all exchange plain structs of scalars, so one value bridge covers them:
//! structs ↔ [`Llsd::Map`], arrays ↔ [`Llsd::Array`], `i4`/`int` ↔
//! [`Llsd::Integer`], `boolean` ↔ [`Llsd::Boolean`], `double` ↔
//! [`Llsd::Real`], `base64` ↔ [`Llsd::Binary`], everything else (including
//! `dateTime.iso8601`) ↔ [`Llsd::String`]. Map members are emitted in sorted
//! key order so a builder's output is byte-stable.
//!
//! The typed login codec ([`build_login_request`](crate::build_login_request) and friends) predates this module and
//! keeps its hand-written member emitters; it shares the value bridge.

use std::collections::BTreeMap;

use sl_llsd::{Llsd, push_escaped};

/// A decoded `<methodCall>`: the method name and its positional parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlRpcCall {
    /// The `<methodName>` text.
    pub method: String,
    /// The `<params>` in order, each converted to an [`Llsd`] value.
    pub params: Vec<Llsd>,
}

/// A decoded `<methodResponse>`: either the positional result parameters or a
/// `<fault>`.
#[derive(Debug, Clone, PartialEq)]
pub enum XmlRpcResponse {
    /// A successful response: the `<params>` in order.
    Params(Vec<Llsd>),
    /// An XML-RPC fault.
    Fault {
        /// The `faultCode` member (0 when absent or unparsable).
        code: i32,
        /// The `faultString` member.
        message: String,
    },
}

impl XmlRpcResponse {
    /// The first result parameter of a successful response, the shape every
    /// Second Life XML-RPC method answers with (a single struct).
    #[must_use]
    pub fn first_param(&self) -> Option<&Llsd> {
        match self {
            Self::Params(params) => params.first(),
            Self::Fault { .. } => None,
        }
    }
}

/// A fault decoding an XML-RPC document.
#[derive(Debug, thiserror::Error)]
pub enum XmlRpcError {
    /// The document was not well-formed XML.
    #[error("malformed XML-RPC document: {0}")]
    Xml(#[from] roxmltree::Error),
    /// A `<methodCall>` carried no `<methodName>`.
    #[error("XML-RPC call has no methodName")]
    NoMethodName,
    /// The document was neither a `<methodCall>` nor a `<methodResponse>`.
    #[error("document is not an XML-RPC call or response")]
    NotXmlRpc,
    /// A typed decoder found a field absent or of the wrong kind.
    #[error(transparent)]
    Llsd(#[from] sl_llsd::LlsdError),
    /// A typed decoder was handed a response for a different method, or a
    /// call naming a method it does not implement.
    #[error("unexpected XML-RPC method {method:?}")]
    UnexpectedMethod {
        /// The method name found.
        method: String,
    },
}

/// Builds a `<methodCall>` document for `method` with the given positional
/// parameters.
#[must_use]
pub fn build_method_call(method: &str, params: &[Llsd]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<methodCall>\n<methodName>");
    push_escaped(&mut out, method);
    out.push_str("</methodName>\n");
    push_params(&mut out, params);
    out.push_str("</methodCall>\n");
    out
}

/// Builds a successful `<methodResponse>` document carrying the given
/// positional result parameters.
#[must_use]
pub fn build_method_response(params: &[Llsd]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<methodResponse>\n");
    push_params(&mut out, params);
    out.push_str("</methodResponse>\n");
    out
}

/// Builds a `<methodResponse>` carrying a `<fault>` with the given code and
/// message.
#[must_use]
pub fn build_fault(code: i32, message: &str) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<methodResponse>\n<fault>\n");
    let mut fault = std::collections::HashMap::new();
    fault.insert("faultCode".to_owned(), Llsd::Integer(code));
    fault.insert("faultString".to_owned(), Llsd::String(message.to_owned()));
    push_value(&mut out, &Llsd::Map(fault));
    out.push_str("\n</fault>\n</methodResponse>\n");
    out
}

/// Appends a `<params>` block, one `<param>` per value.
fn push_params(out: &mut String, params: &[Llsd]) {
    out.push_str("<params>\n");
    for param in params {
        out.push_str("<param>");
        push_value(out, param);
        out.push_str("</param>\n");
    }
    out.push_str("</params>\n");
}

/// Returns the `<methodName>` of a `<methodCall>` document without decoding
/// its parameters — a cheap peek for routing a shared endpoint. `None` for a
/// non-XML body, a response, or a call lacking a method name.
#[must_use]
pub fn method_name(xml: &str) -> Option<String> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let root = document.root_element();
    if !root.has_tag_name("methodCall") {
        return None;
    }
    root.children()
        .find(|n| n.has_tag_name("methodName"))
        .and_then(|n| n.text())
        .map(|text| text.trim().to_owned())
}

/// Parses a `<methodCall>` document.
///
/// # Errors
///
/// Returns [`XmlRpcError::Xml`] for malformed XML, [`XmlRpcError::NotXmlRpc`]
/// if the root is not a `<methodCall>`, and [`XmlRpcError::NoMethodName`] if
/// the method name is absent.
pub fn parse_method_call(xml: &str) -> Result<XmlRpcCall, XmlRpcError> {
    let document = roxmltree::Document::parse(xml)?;
    let root = document.root_element();
    if !root.has_tag_name("methodCall") {
        return Err(XmlRpcError::NotXmlRpc);
    }
    let method = root
        .children()
        .find(|n| n.has_tag_name("methodName"))
        .and_then(|n| n.text())
        .map(|text| text.trim().to_owned())
        .ok_or(XmlRpcError::NoMethodName)?;
    Ok(XmlRpcCall {
        method,
        params: collect_params(root),
    })
}

/// Parses a `<methodResponse>` document.
///
/// # Errors
///
/// Returns [`XmlRpcError::Xml`] for malformed XML and
/// [`XmlRpcError::NotXmlRpc`] if the root is not a `<methodResponse>`.
pub fn parse_method_response(xml: &str) -> Result<XmlRpcResponse, XmlRpcError> {
    let document = roxmltree::Document::parse(xml)?;
    let root = document.root_element();
    if !root.has_tag_name("methodResponse") {
        return Err(XmlRpcError::NotXmlRpc);
    }
    if let Some(fault) = root.children().find(|n| n.has_tag_name("fault")) {
        let value = fault
            .children()
            .find(|n| n.has_tag_name("value"))
            .map_or(Llsd::Undef, value_to_llsd);
        return Ok(XmlRpcResponse::Fault {
            code: value.get("faultCode").and_then(Llsd::as_i32).unwrap_or(0),
            message: value
                .get("faultString")
                .and_then(Llsd::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(XmlRpcResponse::Params(collect_params(root)))
}

/// Collects the `<params>/<param>/<value>` children of a call or response
/// root, in order.
fn collect_params(root: roxmltree::Node<'_, '_>) -> Vec<Llsd> {
    root.children()
        .find(|n| n.has_tag_name("params"))
        .into_iter()
        .flat_map(|params| params.children().filter(|n| n.has_tag_name("param")))
        .map(|param| {
            param
                .children()
                .find(|n| n.has_tag_name("value"))
                .map_or(Llsd::Undef, value_to_llsd)
        })
        .collect()
}

/// Converts a free-form XML-RPC `<value>` tree into an [`Llsd`] value.
/// Structs become maps, arrays become arrays, `i4`/`int` integers, `boolean`
/// booleans, `double` reals, `base64` binary; everything else (including
/// `dateTime.iso8601`) is kept as its string text, so no value is ever dropped.
#[must_use]
pub fn value_to_llsd(value_node: roxmltree::Node<'_, '_>) -> Llsd {
    let Some(element) = value_node.children().find(roxmltree::Node::is_element) else {
        return Llsd::String(value_node.text().unwrap_or_default().to_owned());
    };
    let text = || element.text().unwrap_or_default().to_owned();
    match element.tag_name().name() {
        "struct" => Llsd::Map(
            element
                .children()
                .filter(|n| n.has_tag_name("member"))
                .filter_map(|member| {
                    let name = member
                        .children()
                        .find(|n| n.has_tag_name("name"))
                        .and_then(|n| n.text())?;
                    let value = member.children().find(|n| n.has_tag_name("value"))?;
                    Some((name.to_owned(), value_to_llsd(value)))
                })
                .collect(),
        ),
        "array" => Llsd::Array(array_value_nodes(value_node).map(value_to_llsd).collect()),
        "i4" | "int" => element
            .text()
            .and_then(|t| t.trim().parse().ok())
            .map_or_else(|| Llsd::String(text()), Llsd::Integer),
        "boolean" => Llsd::Boolean(matches!(element.text().map(str::trim), Some("1" | "true"))),
        "double" => element
            .text()
            .and_then(|t| t.trim().parse().ok())
            .map_or_else(|| Llsd::String(text()), Llsd::Real),
        "base64" => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(text().trim())
                .map_or_else(|_error| Llsd::String(text()), Llsd::Binary)
        }
        _other => Llsd::String(text()),
    }
}

/// Iterates the element `<value>` nodes inside an array `<value>` (`value →
/// array → data → value`).
pub(crate) fn array_value_nodes<'a>(
    value_node: roxmltree::Node<'a, 'a>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    value_node
        .children()
        .find(|n| n.has_tag_name("array"))
        .and_then(|array| array.children().find(|n| n.has_tag_name("data")))
        .into_iter()
        .flat_map(|data| data.children().filter(|n| n.has_tag_name("value")))
}

/// Appends an XML-RPC `<member>` holding a free-form [`Llsd`] value, the
/// emitting inverse of one [`value_to_llsd`] struct member.
pub(crate) fn push_member(out: &mut String, name: &str, value: &Llsd) {
    out.push_str("<member><name>");
    push_escaped(out, name);
    out.push_str("</name>");
    push_value(out, value);
    out.push_str("</member>\n");
}

/// Appends a free-form [`Llsd`] value as an XML-RPC `<value>` tree. Maps
/// become structs (members in sorted key order), arrays arrays, integers
/// `<i4>`, booleans `<boolean>`, reals `<double>`, binary `<base64>`; strings,
/// UUIDs, dates, URIs, and the undefined value are written as `<string>` (the
/// string-degrading subset [`value_to_llsd`] parses back, so values round-trip).
pub fn push_value(out: &mut String, value: &Llsd) {
    out.push_str("<value>");
    match value {
        Llsd::Map(map) => {
            out.push_str("<struct>");
            let sorted: BTreeMap<&String, &Llsd> = map.iter().collect();
            for (key, entry) in sorted {
                push_member(out, key, entry);
            }
            out.push_str("</struct>");
        }
        Llsd::Array(items) => {
            out.push_str("<array><data>\n");
            for item in items {
                push_value(out, item);
                out.push('\n');
            }
            out.push_str("</data></array>");
        }
        Llsd::Integer(value) => {
            out.push_str("<i4>");
            out.push_str(&value.to_string());
            out.push_str("</i4>");
        }
        Llsd::Boolean(value) => {
            out.push_str("<boolean>");
            out.push_str(if *value { "1" } else { "0" });
            out.push_str("</boolean>");
        }
        Llsd::Real(value) => {
            out.push_str("<double>");
            out.push_str(&value.to_string());
            out.push_str("</double>");
        }
        Llsd::Binary(bytes) => {
            use base64::Engine as _;
            out.push_str("<base64>");
            out.push_str(&base64::engine::general_purpose::STANDARD.encode(bytes));
            out.push_str("</base64>");
        }
        Llsd::String(value) | Llsd::Date(value) | Llsd::Uri(value) => {
            out.push_str("<string>");
            push_escaped(out, value);
            out.push_str("</string>");
        }
        Llsd::Uuid(value) => {
            out.push_str("<string>");
            out.push_str(&value.to_string());
            out.push_str("</string>");
        }
        Llsd::Undef => out.push_str("<string></string>"),
    }
    out.push_str("</value>");
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;
    use sl_llsd::Llsd;

    use super::{
        XmlRpcCall, XmlRpcError, XmlRpcResponse, build_fault, build_method_call,
        build_method_response, method_name, parse_method_call, parse_method_response,
    };

    /// A struct exercising every value kind the bridge distinguishes.
    fn sample() -> Llsd {
        let mut map = HashMap::new();
        map.insert("int".to_owned(), Llsd::Integer(-7));
        map.insert("flag".to_owned(), Llsd::Boolean(true));
        map.insert("real".to_owned(), Llsd::Real(2.5));
        map.insert("text".to_owned(), Llsd::String("a < b & c".to_owned()));
        map.insert("bytes".to_owned(), Llsd::Binary(vec![1, 2, 3]));
        map.insert(
            "list".to_owned(),
            Llsd::Array(vec![Llsd::Integer(1), Llsd::String("two".to_owned())]),
        );
        let mut inner = HashMap::new();
        inner.insert("k".to_owned(), Llsd::String("v".to_owned()));
        map.insert("nested".to_owned(), Llsd::Map(inner));
        Llsd::Map(map)
    }

    #[test]
    fn call_round_trips_every_value_kind() -> Result<(), XmlRpcError> {
        let xml = build_method_call("do_thing", &[sample(), Llsd::Integer(3)]);
        assert_eq!(method_name(&xml).as_deref(), Some("do_thing"));
        let call = parse_method_call(&xml)?;
        assert_eq!(
            call,
            XmlRpcCall {
                method: "do_thing".to_owned(),
                params: vec![sample(), Llsd::Integer(3)],
            }
        );
        Ok(())
    }

    #[test]
    fn response_round_trips_and_first_param_is_the_struct() -> Result<(), XmlRpcError> {
        let xml = build_method_response(&[sample()]);
        let response = parse_method_response(&xml)?;
        assert_eq!(response.first_param(), Some(&sample()));
        assert!(method_name(&xml).is_none());
        Ok(())
    }

    #[test]
    fn fault_round_trips() -> Result<(), XmlRpcError> {
        let xml = build_fault(42, "nope & <no>");
        let response = parse_method_response(&xml)?;
        assert_eq!(
            response,
            XmlRpcResponse::Fault {
                code: 42,
                message: "nope & <no>".to_owned()
            }
        );
        assert!(response.first_param().is_none());
        Ok(())
    }

    #[test]
    fn map_members_are_emitted_sorted() {
        let xml = build_method_response(&[sample()]);
        let bytes = xml.find("<name>bytes</name>");
        let text = xml.find("<name>text</name>");
        assert!(bytes < text, "{xml}");
    }

    #[test]
    fn wrong_document_kinds_are_rejected() {
        assert!(matches!(
            parse_method_call(&build_method_response(&[])),
            Err(XmlRpcError::NotXmlRpc)
        ));
        assert!(matches!(
            parse_method_response(&build_method_call("m", &[])),
            Err(XmlRpcError::NotXmlRpc)
        ));
        assert!(matches!(
            parse_method_call("<methodCall><params/></methodCall>"),
            Err(XmlRpcError::NoMethodName)
        ));
        assert!(matches!(parse_method_call("<<"), Err(XmlRpcError::Xml(_))));
        assert!(method_name("not xml").is_none());
        assert!(method_name("<llsd><map/></llsd>").is_none());
    }
}
