//! Every XML body sl-wire parses is bounded before roxmltree sees it.
//!
//! roxmltree's element parsing recurses and overflows the stack somewhere
//! between 1000 and 2000 levels, which aborts the process rather than raising a
//! catchable panic — so each of these tests *aborts the whole test binary*, not
//! merely fails, if its entry point stops going through
//! `sl_llsd::parse_guarded_xml`. That is the point: an XML-RPC login response
//! arrives from an unauthenticated endpoint before a session exists, and the
//! grid-info and CAPS bodies come from wherever the grid points.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_wire::{
        LoginParseError, XmlRpcError, parse_grid_info_xml, parse_login_request,
        parse_login_response, parse_method_call, parse_method_response,
    };

    /// How deep to nest: past roxmltree's own recursion ceiling, so an unguarded
    /// parser dies here rather than returning anything.
    const OVERFLOWING_DEPTH: usize = 4_000;

    /// Wraps `inner` in `OVERFLOWING_DEPTH` nested `<value><array><data>`
    /// levels under `root`, the shape an XML-RPC body nests in.
    fn deeply_nested(root: &str, inner: &str) -> String {
        format!(
            "<{root}>{}{inner}{}</{root}>",
            "<value><array><data>".repeat(OVERFLOWING_DEPTH),
            "</data></array></value>".repeat(OVERFLOWING_DEPTH),
        )
    }

    /// A `<methodCall>` nested past the limit is refused, not parsed.
    #[test]
    fn method_call_refuses_a_deeply_nested_body() {
        let xml = deeply_nested("methodCall", "<methodName>login_to_simulator</methodName>");
        assert!(matches!(
            parse_method_call(&xml),
            Err(XmlRpcError::Xml(roxmltree::Error::NodesLimitReached))
        ));
    }

    /// A `<methodResponse>` nested past the limit is refused, not parsed.
    #[test]
    fn method_response_refuses_a_deeply_nested_body() {
        let xml = deeply_nested("methodResponse", "<value><string>hi</string></value>");
        assert!(matches!(
            parse_method_response(&xml),
            Err(XmlRpcError::Xml(roxmltree::Error::NodesLimitReached))
        ));
    }

    /// A `<gridinfo>` document nested past the limit is refused, not parsed.
    #[test]
    fn grid_info_refuses_a_deeply_nested_body() {
        let xml = deeply_nested("gridinfo", "<gridname>Deep</gridname>");
        assert!(matches!(
            parse_grid_info_xml(&xml),
            Err(XmlRpcError::Xml(roxmltree::Error::NodesLimitReached))
        ));
    }

    /// The login *response* — the one body that arrives before a session exists
    /// — is refused rather than parsed.
    #[test]
    fn login_response_refuses_a_deeply_nested_body() {
        let xml = deeply_nested("methodResponse", "<value><string>hi</string></value>");
        assert!(matches!(
            parse_login_response(&xml),
            Err(LoginParseError::Xml(roxmltree::Error::NodesLimitReached))
        ));
    }

    /// The login *request*, which the simulator side of this crate parses from
    /// whatever POSTs to its login endpoint.
    #[test]
    fn login_request_refuses_a_deeply_nested_body() {
        let xml = deeply_nested("methodCall", "<methodName>login_to_simulator</methodName>");
        assert!(matches!(
            parse_login_request(&xml),
            Err(LoginParseError::Xml(roxmltree::Error::NodesLimitReached))
        ));
    }

    /// The guard is a ceiling, not a change of behaviour: a body nested the way
    /// the protocol actually nests still parses.
    #[test]
    fn ordinary_nesting_still_parses() -> Result<(), XmlRpcError> {
        let xml = concat!(
            "<methodResponse><params><param><value><struct>",
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>options</name><value><array><data>",
            "<value><string>inventory-root</string></value>",
            "</data></array></value></member>",
            "</struct></value></param></params></methodResponse>",
        );
        let sl_wire::XmlRpcResponse::Params(params) = parse_method_response(xml)? else {
            unreachable!("the fixture is not a fault")
        };
        let [response] = params.as_slice() else {
            unreachable!("the fixture has one param")
        };
        assert_eq!(
            response.get("login").and_then(sl_wire::Llsd::as_str),
            Some("true")
        );
        Ok(())
    }
}
