//! Tests for the XML-RPC login request builder and response parser.

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use pretty_assertions::assert_eq;
    use sl_wire::{
        LoginRequest, LoginResponse, build_login_request, parse_login_response, password_hash,
    };

    /// A minimal XML-RPC response struct wrapper around the given members.
    fn response(members: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?>\n<methodResponse><params><param><value><struct>{members}</struct></value></param></params></methodResponse>"
        )
    }

    #[test]
    fn password_hash_uses_the_md5_dollar_one_scheme() {
        // MD5("secret") = 5ebe2294ecd0e0f08eab7690d2a6ee69.
        assert_eq!(
            password_hash("secret"),
            "$1$5ebe2294ecd0e0f08eab7690d2a6ee69"
        );
    }

    #[test]
    fn request_contains_method_and_escaped_fields() {
        let mut request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3");
        request.options = vec!["inventory-root".to_owned()];
        let body = build_login_request(&request);

        assert!(body.contains("<methodName>login_to_simulator</methodName>"));
        assert!(body.contains("<name>first</name><value><string>Test</string>"));
        assert!(body.contains("<name>last</name><value><string>User</string>"));
        assert!(body.contains("$1$5ebe2294ecd0e0f08eab7690d2a6ee69"));
        assert!(body.contains("<name>start</name><value><string>last</string>"));
        assert!(body.contains("<value><string>inventory-root</string></value>"));
    }

    #[test]
    fn user_agent_joins_channel_and_version() {
        let request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3");
        assert_eq!(request.user_agent(), "MyViewer 1.2.3");
    }

    #[test]
    fn request_escapes_xml_metacharacters() {
        let request = LoginRequest::new("A&B", "C<D", "p", "last", "MyViewer", "1.2.3");
        let body = build_login_request(&request);
        assert!(body.contains("<string>A&amp;B</string>"));
        assert!(body.contains("<string>C&lt;D</string>"));
    }

    #[test]
    fn parses_a_successful_response() -> Result<(), Box<dyn std::error::Error>> {
        let xml = r#"<?xml version="1.0"?>
<methodResponse><params><param><value><struct>
  <member><name>login</name><value><string>true</string></value></member>
  <member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>
  <member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>
  <member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>
  <member><name>circuit_code</name><value><i4>123456</i4></value></member>
  <member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>
  <member><name>sim_port</name><value><i4>9000</i4></value></member>
  <member><name>seed_capability</name><value><string>http://127.0.0.1:9000/CAPS/seed</string></value></member>
  <member><name>message</name><value><string>Welcome</string></value></member>
</struct></value></param></params></methodResponse>"#;

        let LoginResponse::Success(success) = parse_login_response(xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.circuit_code, 123_456);
        assert_eq!(success.sim_ip, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(success.sim_port, 9000);
        assert_eq!(success.seed_capability, "http://127.0.0.1:9000/CAPS/seed");
        assert_eq!(success.message.as_deref(), Some("Welcome"));
        Ok(())
    }

    #[test]
    fn parses_inventory_root_and_skeleton() -> Result<(), Box<dyn std::error::Error>> {
        // A minimal success response carrying inventory-root (array of one struct)
        // and inventory-skeleton (array of folder structs).
        let members = concat!(
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>",
            "<member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>",
            "<member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>",
            "<member><name>circuit_code</name><value><i4>1</i4></value></member>",
            "<member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>",
            "<member><name>sim_port</name><value><i4>9000</i4></value></member>",
            "<member><name>seed_capability</name><value><string>http://x/seed</string></value></member>",
            "<member><name>inventory-root</name><value><array><data>",
            "<value><struct><member><name>folder_id</name><value><string>aaaaaaaa-0000-0000-0000-000000000000</string></value></member></struct></value>",
            "</data></array></value></member>",
            "<member><name>inventory-skeleton</name><value><array><data>",
            "<value><struct>",
            "<member><name>folder_id</name><value><string>aaaaaaaa-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>parent_id</name><value><string>00000000-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>name</name><value><string>My Inventory</string></value></member>",
            "<member><name>type_default</name><value><i4>8</i4></value></member>",
            "<member><name>version</name><value><i4>5</i4></value></member>",
            "</struct></value>",
            "<value><struct>",
            "<member><name>folder_id</name><value><string>bbbbbbbb-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>parent_id</name><value><string>aaaaaaaa-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>name</name><value><string>Objects</string></value></member>",
            "<member><name>type_default</name><value><i4>6</i4></value></member>",
            "<member><name>version</name><value><i4>2</i4></value></member>",
            "</struct></value>",
            "</data></array></value></member>",
        );
        let xml = response(members);

        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(
            success.inventory_root,
            Some("aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?)
        );
        assert_eq!(success.inventory_skeleton.len(), 2);
        let root = success.inventory_skeleton.first().ok_or("root folder")?;
        assert_eq!(root.name, "My Inventory");
        assert_eq!(root.type_default, 8);
        assert_eq!(root.version, 5);
        let objects = success.inventory_skeleton.get(1).ok_or("objects folder")?;
        assert_eq!(objects.name, "Objects");
        assert_eq!(
            objects.parent_id,
            "aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?
        );
        Ok(())
    }

    #[test]
    fn parses_a_failure_response() -> Result<(), Box<dyn std::error::Error>> {
        let xml = r#"<?xml version="1.0"?>
<methodResponse><params><param><value><struct>
  <member><name>login</name><value><string>false</string></value></member>
  <member><name>reason</name><value><string>key</string></value></member>
  <member><name>message</name><value><string>Could not authenticate your avatar.</string></value></member>
</struct></value></param></params></methodResponse>"#;

        let LoginResponse::Failure(failure) = parse_login_response(xml)? else {
            return Err("expected a failed login".into());
        };
        assert_eq!(failure.reason, "key");
        assert_eq!(failure.message, "Could not authenticate your avatar.");
        Ok(())
    }

    #[test]
    fn request_carries_mfa_fields() {
        let request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3")
            .with_mfa("123456", Some("storedhash".to_owned()));
        let body = build_login_request(&request);
        assert!(body.contains("<name>token</name><value><string>123456</string>"));
        assert!(body.contains("<name>mfa_hash</name><value><string>storedhash</string>"));
        assert!(body.contains("<name>extended_errors</name><value><boolean>1</boolean>"));
    }

    #[test]
    fn parses_an_mfa_challenge() -> Result<(), Box<dyn std::error::Error>> {
        let xml = response(
            "<member><name>login</name><value><string>false</string></value></member>\
             <member><name>reason</name><value><string>mfa_challenge</string></value></member>\
             <member><name>message</name><value><string>Enter your token</string></value></member>\
             <member><name>mfa_hash</name><value><string>challengehash</string></value></member>",
        );
        let LoginResponse::MfaChallenge(challenge) = parse_login_response(&xml)? else {
            return Err("expected an MFA challenge".into());
        };
        assert_eq!(challenge.message, "Enter your token");
        assert_eq!(challenge.mfa_hash.as_deref(), Some("challengehash"));
        Ok(())
    }

    #[test]
    fn parses_success_mfa_hash_to_remember() -> Result<(), Box<dyn std::error::Error>> {
        let xml = response(
            "<member><name>login</name><value><string>true</string></value></member>\
             <member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>\
             <member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>\
             <member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>\
             <member><name>circuit_code</name><value><i4>1</i4></value></member>\
             <member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>\
             <member><name>sim_port</name><value><i4>9000</i4></value></member>\
             <member><name>seed_capability</name><value><string>http://x/seed</string></value></member>\
             <member><name>mfa_hash</name><value><string>rememberme</string></value></member>",
        );
        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected success".into());
        };
        assert_eq!(success.mfa_hash.as_deref(), Some("rememberme"));
        Ok(())
    }

    #[test]
    fn round_trips_through_the_builder_field_names() -> Result<(), Box<dyn std::error::Error>> {
        // The fields the builder writes must match the names OpenSim expects.
        let request = LoginRequest::new("First", "Last", "pw", "home", "MyViewer", "1.2.3");
        let body = build_login_request(&request);
        for name in [
            "first", "last", "passwd", "start", "channel", "version", "mac", "id0",
        ] {
            assert!(
                body.contains(&format!("<name>{name}</name>")),
                "missing {name}"
            );
        }
        // The caller-supplied channel and version are carried verbatim.
        assert!(body.contains("<name>channel</name><value><string>MyViewer</string>"));
        assert!(body.contains("<name>version</name><value><string>1.2.3</string>"));
        Ok(())
    }
}
