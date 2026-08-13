//! Tests for the XML-RPC login request builder and response parser.

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, InventoryFolderKey};
    use sl_types::map::RegionCoordinates;
    use sl_wire::{
        Direction, LoginRequest, LoginResponse, RegionHandle, StartLocation, build_login_request,
        parse_login_response, password_hash,
    };

    /// Asserts two three-component vectors are equal within a small tolerance
    /// (the login reals round-trip through `f64` parsing then narrow to `f32`).
    fn assert_vec3_approx(actual: [f32; 3], expected: [f32; 3]) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!((a - e).abs() < 1e-4, "{actual:?} != {expected:?}");
        }
    }

    /// Asserts region-local coordinates equal `expected` within tolerance.
    fn assert_region_approx(actual: RegionCoordinates, expected: [f32; 3]) {
        assert_vec3_approx([actual.x(), actual.y(), actual.z()], expected);
    }

    /// Asserts a facing direction equals `expected` within tolerance.
    fn assert_direction_approx(actual: Direction, expected: [f32; 3]) {
        assert_vec3_approx([actual.x(), actual.y(), actual.z()], expected);
    }

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
        let mut request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        );
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
        let request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        );
        assert_eq!(request.user_agent(), "MyViewer 1.2.3");
    }

    #[test]
    fn request_escapes_xml_metacharacters() {
        let request =
            LoginRequest::new("A&B", "C<D", "p", StartLocation::Last, "MyViewer", "1.2.3");
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
  <member><name>agent_appearance_service</name><value><string>https://appearance.example/</string></value></member>
  <member><name>map-server-url</name><value><string>http://127.0.0.1:9000/</string></value></member>
  <member><name>openid_url</name><value><string>https://id.secondlife.com/openid/webkit</string></value></member>
  <member><name>openid_token</name><value><string>a-one-time-token</string></value></member>
  <member><name>message</name><value><string>Welcome</string></value></member>
</struct></value></param></params></methodResponse>"#;

        let LoginResponse::Success(success) = parse_login_response(xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.circuit_code, sl_wire::CircuitCode(123_456));
        assert_eq!(success.sim_ip, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(success.sim_port, 9000);
        assert_eq!(
            success.seed_capability.as_str(),
            "http://127.0.0.1:9000/CAPS/seed"
        );
        assert_eq!(
            success
                .agent_appearance_service
                .as_ref()
                .map(url::Url::as_str),
            Some("https://appearance.example/")
        );
        assert_eq!(
            success.map_server_url.as_ref().map(url::Url::as_str),
            Some("http://127.0.0.1:9000/")
        );
        assert_eq!(success.message.as_deref(), Some("Welcome"));
        assert_eq!(
            success.openid_url.as_ref().map(url::Url::as_str),
            Some("https://id.secondlife.com/openid/webkit")
        );
        assert_eq!(success.openid_token.as_deref(), Some("a-one-time-token"));
        Ok(())
    }

    #[test]
    fn openid_fields_absent_parse_to_none() -> Result<(), Box<dyn std::error::Error>> {
        // An OpenSim-style response carries neither `openid_url` nor
        // `openid_token`; both must parse to `None` so the web auto-login stays
        // dormant off Second Life.
        let xml = r#"<?xml version="1.0"?>
<methodResponse><params><param><value><struct>
  <member><name>login</name><value><string>true</string></value></member>
  <member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>
  <member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>
  <member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>
  <member><name>circuit_code</name><value><i4>1</i4></value></member>
  <member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>
  <member><name>sim_port</name><value><i4>9000</i4></value></member>
  <member><name>seed_capability</name><value><string>http://127.0.0.1:9000/CAPS/seed</string></value></member>
</struct></value></param></params></methodResponse>"#;

        let LoginResponse::Success(success) = parse_login_response(xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.openid_url, None);
        assert_eq!(success.openid_token, None);
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
            Some(InventoryFolderKey::from(
                "aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?
            ))
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
            InventoryFolderKey::from("aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?)
        );
        Ok(())
    }

    #[test]
    fn parses_buddy_list() -> Result<(), Box<dyn std::error::Error>> {
        // A minimal success response carrying a buddy-list (array of friend
        // structs with the two rights ints).
        let members = concat!(
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>",
            "<member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>",
            "<member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>",
            "<member><name>circuit_code</name><value><i4>1</i4></value></member>",
            "<member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>",
            "<member><name>sim_port</name><value><i4>9000</i4></value></member>",
            "<member><name>seed_capability</name><value><string>http://x/seed</string></value></member>",
            "<member><name>buddy-list</name><value><array><data>",
            "<value><struct>",
            "<member><name>buddy_id</name><value><string>cccccccc-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>buddy_rights_given</name><value><i4>3</i4></value></member>",
            "<member><name>buddy_rights_has</name><value><i4>1</i4></value></member>",
            "</struct></value>",
            "</data></array></value></member>",
        );
        let xml = response(members);

        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.buddy_list.len(), 1);
        let buddy = success.buddy_list.first().ok_or("first buddy")?;
        assert_eq!(
            buddy.buddy_id,
            "cccccccc-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?
        );
        assert_eq!(buddy.rights_granted, 3);
        assert_eq!(buddy.rights_has, 1);
        Ok(())
    }

    #[test]
    fn request_carries_buddy_list_option() {
        let request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        );
        let body = build_login_request(&request);
        assert!(body.contains("<value><string>buddy-list</string></value>"));
    }

    #[test]
    fn request_carries_library_options() {
        let request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        );
        let body = build_login_request(&request);
        for option in [
            "inventory-lib-root",
            "inventory-lib-owner",
            "inventory-skel-lib",
        ] {
            assert!(
                body.contains(&format!("<value><string>{option}</string></value>")),
                "missing {option} option"
            );
        }
    }

    #[test]
    fn parses_home_look_at_access_and_groups() -> Result<(), Box<dyn std::error::Error>> {
        // The home/look_at fields are quasi-LLSD strings with `r`-prefixed reals,
        // exactly as OpenSim/Second Life format them.
        let members = concat!(
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>",
            "<member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>",
            "<member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>",
            "<member><name>circuit_code</name><value><i4>1</i4></value></member>",
            "<member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>",
            "<member><name>sim_port</name><value><i4>9000</i4></value></member>",
            "<member><name>seed_capability</name><value><string>http://x/seed</string></value></member>",
            "<member><name>home</name><value><string>{'region_handle':[r256000,r256256], 'position':[r128.5,r127.0,r25.75], 'look_at':[r1.0,r0.0,r0.0]}</string></value></member>",
            "<member><name>look_at</name><value><string>[r0.9994,r0.0316,r0]</string></value></member>",
            "<member><name>agent_access</name><value><string>M</string></value></member>",
            "<member><name>agent_access_max</name><value><string>A</string></value></member>",
            "<member><name>max-agent-groups</name><value><i4>42</i4></value></member>",
        );
        let xml = response(members);

        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        let home = success.home.ok_or("home location")?;
        assert_eq!(
            home.region_handle,
            RegionHandle::from_global(256_000, 256_256)
        );
        assert_region_approx(home.position, [128.5, 127.0, 25.75]);
        assert_direction_approx(home.look_at, [1.0, 0.0, 0.0]);
        let look_at = success.look_at.ok_or("start look-at")?;
        assert_direction_approx(look_at, [0.9994, 0.0316, 0.0]);
        assert_eq!(success.agent_access.as_deref(), Some("M"));
        assert_eq!(success.agent_access_max.as_deref(), Some("A"));
        assert_eq!(success.max_agent_groups, Some(42));
        Ok(())
    }

    #[test]
    fn parses_library_roots_and_skeleton() -> Result<(), Box<dyn std::error::Error>> {
        let members = concat!(
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>",
            "<member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>",
            "<member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>",
            "<member><name>circuit_code</name><value><i4>1</i4></value></member>",
            "<member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>",
            "<member><name>sim_port</name><value><i4>9000</i4></value></member>",
            "<member><name>seed_capability</name><value><string>http://x/seed</string></value></member>",
            "<member><name>inventory-lib-root</name><value><array><data>",
            "<value><struct><member><name>folder_id</name><value><string>00000112-000f-0000-0000-000100bba000</string></value></member></struct></value>",
            "</data></array></value></member>",
            "<member><name>inventory-lib-owner</name><value><array><data>",
            "<value><struct><member><name>agent_id</name><value><string>11111111-1111-0000-0000-000000000000</string></value></member></struct></value>",
            "</data></array></value></member>",
            "<member><name>inventory-skel-lib</name><value><array><data>",
            "<value><struct>",
            "<member><name>folder_id</name><value><string>00000112-000f-0000-0000-000100bba000</string></value></member>",
            "<member><name>parent_id</name><value><string>00000000-0000-0000-0000-000000000000</string></value></member>",
            "<member><name>name</name><value><string>Library</string></value></member>",
            "<member><name>type_default</name><value><i4>8</i4></value></member>",
            "<member><name>version</name><value><i4>1</i4></value></member>",
            "</struct></value>",
            "</data></array></value></member>",
        );
        let xml = response(members);

        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(
            success.library_root,
            Some(InventoryFolderKey::from(
                "00000112-000f-0000-0000-000100bba000".parse::<uuid::Uuid>()?
            ))
        );
        assert_eq!(
            success.library_owner,
            Some(AgentKey::from(
                "11111111-1111-0000-0000-000000000000".parse::<uuid::Uuid>()?
            ))
        );
        assert_eq!(success.library_skeleton.len(), 1);
        let root = success.library_skeleton.first().ok_or("library root")?;
        assert_eq!(root.name, "Library");
        Ok(())
    }

    #[test]
    fn tolerates_a_missing_or_malformed_home() -> Result<(), Box<dyn std::error::Error>> {
        // A success with no home/look_at/access fields leaves them as None.
        let members = concat!(
            "<member><name>login</name><value><string>true</string></value></member>",
            "<member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>",
            "<member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>",
            "<member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>",
            "<member><name>circuit_code</name><value><i4>1</i4></value></member>",
            "<member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>",
            "<member><name>sim_port</name><value><i4>9000</i4></value></member>",
            "<member><name>seed_capability</name><value><string>http://x/seed</string></value></member>",
            "<member><name>home</name><value><string>{'region_handle':[r256000]}</string></value></member>",
        );
        let xml = response(members);
        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        // The home string lacks position/look_at, so it parses to None rather
        // than a partial value.
        assert!(success.home.is_none());
        assert!(success.look_at.is_none());
        assert!(success.agent_access.is_none());
        assert!(success.max_agent_groups.is_none());
        assert!(success.library_root.is_none());
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
        let request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        )
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
    fn parse_login_request_round_trips_the_builder() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{parse_login_request, password_hash};

        let mut request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        )
        .with_mfa("123456", Some("storedhash".to_owned()));
        request.options = vec!["inventory-root".to_owned(), "buddy-list".to_owned()];
        let body = build_login_request(&request);

        let parsed = parse_login_request(&body)?;
        assert_eq!(parsed.first_name, "Test");
        assert_eq!(parsed.last_name, "User");
        // The server only ever sees the hashed password, never the plaintext.
        assert_eq!(parsed.password_hash, password_hash("secret"));
        assert_eq!(parsed.start, Ok(StartLocation::Last));
        assert_eq!(parsed.channel, "MyViewer");
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.platform, "lin");
        assert_eq!(parsed.token, "123456");
        assert_eq!(parsed.mfa_hash, "storedhash");
        assert!(parsed.agree_to_tos);
        assert!(parsed.read_critical);
        assert!(parsed.extended_errors);
        assert_eq!(parsed.options, vec!["inventory-root", "buddy-list"]);
        Ok(())
    }

    #[test]
    fn parse_login_request_keeps_a_uri_start_typed_and_an_unparsable_one_raw()
    -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::parse_login_request;

        // A well-formed `uri:` start (the `&`s are XML-escaped by the builder and
        // unescaped on parse) round-trips into a typed `StartLocation`.
        let start = StartLocation::region("Sandbox", RegionCoordinates::new(128.0, 128.0, 30.0));
        let request =
            LoginRequest::new("Test", "User", "secret", start.clone(), "MyViewer", "1.2.3");
        assert_eq!(
            parse_login_request(&build_login_request(&request))?.start,
            Ok(start)
        );

        // An out-of-grammar value the client sent is preserved verbatim as `Err`,
        // never coerced into a (wrong) typed location.
        let home = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Home,
            "MyViewer",
            "1.2.3",
        );
        let garbled = build_login_request(&home).replace(
            "<name>start</name><value><string>home</string>",
            "<name>start</name><value><string>somewhere</string>",
        );
        assert_eq!(
            parse_login_request(&garbled)?.start,
            Err("somewhere".to_owned())
        );
        Ok(())
    }

    /// A full success with every optional payload, to exercise `build_login_response`.
    /// Deliberately a full struct literal (not `LoginSuccess::minimal` plus
    /// overrides) so adding a `LoginSuccess` field breaks this helper and
    /// forces the round-trip test to cover it.
    fn full_success() -> Result<sl_wire::LoginSuccess, Box<dyn std::error::Error>> {
        use sl_types::key::{InventoryKey, TextureKey};
        use sl_wire::{
            BuddyListEntry, GestureEntry, GlobalTextures, HomeLocation, InitialOutfit, Llsd,
            LoginCategory, LoginFlags, NewUserConfig, SkeletonFolder, TutorialSetting, UiConfig,
            VoiceConfig,
        };

        let benefits = Llsd::Map(
            [
                ("animated_object_limit".to_owned(), Llsd::Integer(2)),
                ("attachment_limit".to_owned(), Llsd::Integer(38)),
            ]
            .into_iter()
            .collect(),
        );
        let premium_packages = Llsd::Map(
            [
                (
                    "Base".to_owned(),
                    Llsd::Map(
                        [("benefits".to_owned(), benefits.clone())]
                            .into_iter()
                            .collect(),
                    ),
                ),
                (
                    "Premium".to_owned(),
                    Llsd::Map(
                        [("benefits".to_owned(), benefits.clone())]
                            .into_iter()
                            .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let folder = |id: &str,
                      parent: &str,
                      name: &str,
                      type_default,
                      version|
         -> Result<SkeletonFolder, Box<dyn std::error::Error>> {
            Ok(SkeletonFolder {
                folder_id: InventoryFolderKey::from(id.parse::<uuid::Uuid>()?),
                parent_id: InventoryFolderKey::from(parent.parse::<uuid::Uuid>()?),
                name: name.to_owned(),
                type_default,
                version,
            })
        };
        Ok(sl_wire::LoginSuccess {
            agent_id: sl_types::key::AgentKey::from(
                "11111111-1111-1111-1111-111111111111".parse::<uuid::Uuid>()?,
            ),
            session_id: "22222222-2222-2222-2222-222222222222".parse()?,
            secure_session_id: "33333333-3333-3333-3333-333333333333".parse()?,
            circuit_code: sl_wire::CircuitCode(123_456),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/CAPS/seed".parse()?,
            message: Some("Welcome <home> & enjoy".to_owned()),
            mfa_hash: Some("rememberme".to_owned()),
            inventory_root: Some(InventoryFolderKey::from(
                "aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?,
            )),
            inventory_skeleton: vec![
                folder(
                    "aaaaaaaa-0000-0000-0000-000000000000",
                    "00000000-0000-0000-0000-000000000000",
                    "My Inventory",
                    8,
                    5,
                )?,
                folder(
                    "bbbbbbbb-0000-0000-0000-000000000000",
                    "aaaaaaaa-0000-0000-0000-000000000000",
                    "Objects",
                    6,
                    2,
                )?,
            ],
            buddy_list: vec![BuddyListEntry {
                buddy_id: "cccccccc-0000-0000-0000-000000000000".parse()?,
                rights_granted: 3,
                rights_has: 1,
            }],
            home: Some(HomeLocation {
                region_handle: RegionHandle::from_global(256_000, 256_256),
                position: RegionCoordinates::new(128.5, 127.0, 25.75),
                look_at: Direction::new(1.0, 0.0, 0.0),
            }),
            look_at: Some(Direction::new(0.9994, 0.0316, 0.0)),
            region_x: Some(256_000),
            region_y: Some(256_256),
            agent_access: Some("M".to_owned()),
            agent_access_max: Some("A".to_owned()),
            max_agent_groups: Some(42),
            library_root: Some(InventoryFolderKey::from(
                "00000112-000f-0000-0000-000100bba000".parse::<uuid::Uuid>()?,
            )),
            library_owner: Some(AgentKey::from(
                "11111111-1111-0000-0000-000000000000".parse::<uuid::Uuid>()?,
            )),
            library_skeleton: vec![folder(
                "00000112-000f-0000-0000-000100bba000",
                "00000000-0000-0000-0000-000000000000",
                "Library",
                8,
                1,
            )?],
            agent_appearance_service: None,
            map_server_url: Some(url::Url::parse("http://127.0.0.1:9000/")?),
            openid_url: Some(url::Url::parse("https://id.secondlife.com/openid/webkit")?),
            openid_token: Some("open-id-token-blob".to_owned()),
            first_name: Some("Test".to_owned()),
            last_name: Some("User".to_owned()),
            display_name: Some("Test User".to_owned()),
            real_id: Some(AgentKey::from(
                "44444444-4444-4444-4444-444444444444".parse::<uuid::Uuid>()?,
            )),
            agent_region_access: Some("PG".to_owned()),
            start_location: Some("last".to_owned()),
            seconds_since_epoch: Some(1_755_000_000),
            udp_blacklist: vec!["EnableSimulator".to_owned(), "TeleportFinish".to_owned()],
            http_port: Some(9001),
            region_size_x: Some(256),
            region_size_y: Some(512),
            login_flags: Some(LoginFlags {
                ever_logged_in: true,
                daylight_savings: false,
                gendered: true,
                stipend_since_login: "N".to_owned(),
            }),
            global_textures: Some(GlobalTextures {
                sun_texture_id: TextureKey::from(
                    "cce0f112-878f-4586-a2e2-a8f104bba271".parse::<uuid::Uuid>()?,
                ),
                cloud_texture_id: TextureKey::from(
                    "dc4b9f0b-d008-45c6-96a4-01dd947ac621".parse::<uuid::Uuid>()?,
                ),
                moon_texture_id: TextureKey::from(
                    "ec4b9f0b-d008-45c6-96a4-01dd947ac621".parse::<uuid::Uuid>()?,
                ),
            }),
            ui_config: Some(UiConfig {
                allow_first_life: true,
            }),
            initial_outfit: Some(InitialOutfit {
                folder_name: "Nightclub Female".to_owned(),
                gender: "female".to_owned(),
            }),
            newuser_config: Some(NewUserConfig {
                default_female_avatar: Some("Ruth".to_owned()),
                default_male_avatar: Some("Roth".to_owned()),
            }),
            voice_config: Some(VoiceConfig {
                voice_server_type: "webrtc".to_owned(),
            }),
            gestures: vec![GestureEntry {
                item_id: InventoryKey::from(
                    "dddddddd-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?,
                ),
                asset_id: "eeeeeeee-0000-0000-0000-000000000000".parse()?,
            }],
            event_categories: vec![LoginCategory {
                category_id: 18,
                category_name: "Discussion".to_owned(),
            }],
            classified_categories: vec![
                LoginCategory {
                    category_id: 1,
                    category_name: "Shopping".to_owned(),
                },
                LoginCategory {
                    category_id: 9,
                    category_name: "Personal".to_owned(),
                },
            ],
            event_notifications: vec![Llsd::Map(
                [
                    ("event_id".to_owned(), Llsd::Integer(7)),
                    ("event_name".to_owned(), Llsd::String("Dance".to_owned())),
                ]
                .into_iter()
                .collect(),
            )],
            tutorial_settings: vec![TutorialSetting {
                tutorial_url: "http://example.com/tutorial/".to_owned(),
            }],
            help_url_format: Some("https://help.example.com/[TOPIC]?lang=[LANGUAGE]".to_owned()),
            web_profile_url: Some(url::Url::parse("https://my.example.com/")?),
            profile_server_url: Some(url::Url::parse("http://127.0.0.1:9000/profiles")?),
            search_url: Some(url::Url::parse("http://127.0.0.1:9000/search")?),
            destination_guide_url: Some(url::Url::parse("https://guide.example.com/")?),
            avatar_picker_url: Some(url::Url::parse("https://picker.example.com/")?),
            currency: Some("L$".to_owned()),
            classified_fee: Some(50),
            directory_fee: Some(30),
            account_type: Some("Premium".to_owned()),
            account_level_benefits: Some(benefits),
            premium_packages: Some(premium_packages),
        })
    }

    #[test]
    fn build_login_response_round_trips_a_full_success() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{build_login_response, parse_login_response};

        let success = full_success()?;
        let xml = build_login_response(&LoginResponse::Success(Box::new(success.clone())));
        let LoginResponse::Success(parsed) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };

        assert_eq!(parsed.agent_id, success.agent_id);
        assert_eq!(parsed.session_id, success.session_id);
        assert_eq!(parsed.secure_session_id, success.secure_session_id);
        assert_eq!(parsed.circuit_code, success.circuit_code);
        assert_eq!(parsed.sim_ip, success.sim_ip);
        assert_eq!(parsed.sim_port, success.sim_port);
        assert_eq!(parsed.seed_capability, success.seed_capability);
        // The metacharacters in the message survive XML escaping.
        assert_eq!(parsed.message.as_deref(), Some("Welcome <home> & enjoy"));
        assert_eq!(parsed.mfa_hash.as_deref(), Some("rememberme"));
        assert_eq!(parsed.inventory_root, success.inventory_root);
        assert_eq!(parsed.inventory_skeleton, success.inventory_skeleton);
        assert_eq!(parsed.buddy_list, success.buddy_list);
        let home = parsed.home.ok_or("home")?;
        assert_eq!(
            home.region_handle,
            RegionHandle::from_global(256_000, 256_256)
        );
        assert_region_approx(home.position, [128.5, 127.0, 25.75]);
        assert_direction_approx(home.look_at, [1.0, 0.0, 0.0]);
        assert_direction_approx(parsed.look_at.ok_or("look_at")?, [0.9994, 0.0316, 0.0]);
        assert_eq!(parsed.region_x, Some(256_000));
        assert_eq!(parsed.region_y, Some(256_256));
        assert_eq!(parsed.agent_access.as_deref(), Some("M"));
        assert_eq!(parsed.agent_access_max.as_deref(), Some("A"));
        assert_eq!(parsed.max_agent_groups, Some(42));
        assert_eq!(parsed.library_root, success.library_root);
        assert_eq!(parsed.library_owner, success.library_owner);
        assert_eq!(parsed.library_skeleton, success.library_skeleton);
        assert_eq!(parsed.map_server_url, success.map_server_url);
        assert_eq!(parsed.openid_url, success.openid_url);
        assert_eq!(parsed.openid_token, success.openid_token);
        assert_eq!(parsed.first_name, success.first_name);
        assert_eq!(parsed.last_name, success.last_name);
        assert_eq!(parsed.display_name, success.display_name);
        assert_eq!(parsed.real_id, success.real_id);
        assert_eq!(parsed.agent_region_access, success.agent_region_access);
        assert_eq!(parsed.start_location, success.start_location);
        assert_eq!(parsed.seconds_since_epoch, success.seconds_since_epoch);
        assert_eq!(parsed.udp_blacklist, success.udp_blacklist);
        assert_eq!(parsed.http_port, success.http_port);
        assert_eq!(parsed.region_size_x, success.region_size_x);
        assert_eq!(parsed.region_size_y, success.region_size_y);
        assert_eq!(parsed.login_flags, success.login_flags);
        assert_eq!(parsed.global_textures, success.global_textures);
        assert_eq!(parsed.ui_config, success.ui_config);
        assert_eq!(parsed.initial_outfit, success.initial_outfit);
        assert_eq!(parsed.newuser_config, success.newuser_config);
        assert_eq!(parsed.voice_config, success.voice_config);
        assert_eq!(parsed.gestures, success.gestures);
        assert_eq!(parsed.event_categories, success.event_categories);
        assert_eq!(parsed.classified_categories, success.classified_categories);
        assert_eq!(parsed.event_notifications, success.event_notifications);
        assert_eq!(parsed.tutorial_settings, success.tutorial_settings);
        assert_eq!(parsed.help_url_format, success.help_url_format);
        assert_eq!(parsed.web_profile_url, success.web_profile_url);
        assert_eq!(parsed.profile_server_url, success.profile_server_url);
        assert_eq!(parsed.search_url, success.search_url);
        assert_eq!(parsed.destination_guide_url, success.destination_guide_url);
        assert_eq!(parsed.avatar_picker_url, success.avatar_picker_url);
        assert_eq!(parsed.currency, success.currency);
        assert_eq!(parsed.classified_fee, success.classified_fee);
        assert_eq!(parsed.directory_fee, success.directory_fee);
        assert_eq!(parsed.account_type, success.account_type);
        assert_eq!(
            parsed.account_level_benefits,
            success.account_level_benefits
        );
        assert_eq!(parsed.premium_packages, success.premium_packages);
        Ok(())
    }

    #[test]
    fn build_login_response_round_trips_a_failure() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{LoginFailure, build_login_response, parse_login_response};

        let failure = LoginFailure::new("key", "Could not authenticate your avatar.");
        let xml = build_login_response(&LoginResponse::Failure(failure.clone()));
        let LoginResponse::Failure(parsed) = parse_login_response(&xml)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(parsed, failure);
        Ok(())
    }

    #[test]
    fn build_login_response_round_trips_extended_error_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{LoginFailure, build_login_response, parse_login_response};

        // The `extended_errors` fields: a localization key plus its
        // substitution arguments (here the suspension-end time).
        let mut failure = LoginFailure::new(
            "key",
            "Your account has been suspended until December 12, 2026.",
        );
        failure.message_id = Some("LoginFailedAccountSuspended".to_owned());
        failure.message_args = [("TIME".to_owned(), "December 12, 2026".to_owned())]
            .into_iter()
            .collect();
        let xml = build_login_response(&LoginResponse::Failure(failure.clone()));
        let LoginResponse::Failure(parsed) = parse_login_response(&xml)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(parsed, failure);
        Ok(())
    }

    #[test]
    fn build_login_response_round_trips_a_redirect() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{LoginRedirect, build_login_response, parse_login_response};

        let redirect = LoginRedirect {
            next_url: "https://login.example.com/second".parse()?,
            next_method: "login_to_simulator".to_owned(),
            message: Some("Redirecting…".to_owned()),
            next_options: vec!["inventory-root".to_owned(), "gestures".to_owned()],
        };
        let xml = build_login_response(&LoginResponse::Redirect(redirect.clone()));
        let LoginResponse::Redirect(parsed) = parse_login_response(&xml)? else {
            return Err("expected a redirect".into());
        };
        assert_eq!(parsed, redirect);
        Ok(())
    }

    #[test]
    fn indeterminate_without_next_url_degrades_to_a_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        // A redirect the client cannot follow must still surface, as a
        // failure carrying the reason and message.
        let xml = response(
            "<member><name>login</name><value><string>indeterminate</string></value></member>\
             <member><name>message</name><value><string>try again</string></value></member>",
        );
        let LoginResponse::Failure(failure) = parse_login_response(&xml)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(failure.reason, "indeterminate");
        assert_eq!(failure.message, "try again");
        Ok(())
    }

    #[test]
    fn build_login_response_round_trips_an_mfa_challenge() -> Result<(), Box<dyn std::error::Error>>
    {
        use sl_wire::{MfaChallenge, build_login_response, parse_login_response};

        let challenge = MfaChallenge {
            mfa_hash: Some("challengehash".to_owned()),
            message: "Enter your token".to_owned(),
        };
        let xml = build_login_response(&LoginResponse::MfaChallenge(challenge.clone()));
        let LoginResponse::MfaChallenge(parsed) = parse_login_response(&xml)? else {
            return Err("expected an MFA challenge".into());
        };
        assert_eq!(parsed, challenge);
        Ok(())
    }

    #[test]
    fn login_server_authenticates_and_challenges() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{
            Credential, LoginGates, LoginServer, MfaPolicy, parse_login_request, password_hash,
        };

        let make_request = |password: &str, token: &str, mfa_hash: Option<String>| {
            let request = LoginRequest::new(
                "Test",
                "User",
                password,
                StartLocation::Last,
                "MyViewer",
                "1.2.3",
            )
            .with_mfa(token, mfa_hash);
            parse_login_request(&build_login_request(&request))
        };

        let no_mfa = Credential {
            password_hash: password_hash("secret"),
            mfa: None,
        };

        // Correct password, no MFA → success.
        let ok = make_request("secret", "", None)?;
        assert!(matches!(
            LoginServer::respond(
                &ok,
                &no_mfa,
                &LoginGates::default(),
                Box::new(full_success()?)
            ),
            LoginResponse::Success(_)
        ));

        // Wrong password → failure with the "key" reason.
        let bad = make_request("wrong", "", None)?;
        let LoginResponse::Failure(failure) = LoginServer::respond(
            &bad,
            &no_mfa,
            &LoginGates::default(),
            Box::new(full_success()?),
        ) else {
            return Err("expected a failure".into());
        };
        assert_eq!(failure.reason, LoginServer::BAD_CREDENTIALS_REASON);

        // MFA required, no token → challenge handing out the remembered hash.
        let mfa = Credential {
            password_hash: password_hash("secret"),
            mfa: Some(MfaPolicy {
                expected_token: "999999".to_owned(),
                mfa_hash: "remember-this-device".to_owned(),
                challenge_message: "Enter your code".to_owned(),
            }),
        };
        let first = make_request("secret", "", None)?;
        let LoginResponse::MfaChallenge(challenge) = LoginServer::respond(
            &first,
            &mfa,
            &LoginGates::default(),
            Box::new(full_success()?),
        ) else {
            return Err("expected an MFA challenge".into());
        };
        assert_eq!(challenge.mfa_hash.as_deref(), Some("remember-this-device"));

        // MFA satisfied by the one-time token → success.
        let with_token = make_request("secret", "999999", None)?;
        assert!(matches!(
            LoginServer::respond(
                &with_token,
                &mfa,
                &LoginGates::default(),
                Box::new(full_success()?)
            ),
            LoginResponse::Success(_)
        ));

        // MFA satisfied by echoing the remembered hash → success.
        let with_hash = make_request("secret", "", Some("remember-this-device".to_owned()))?;
        assert!(matches!(
            LoginServer::respond(
                &with_hash,
                &mfa,
                &LoginGates::default(),
                Box::new(full_success()?)
            ),
            LoginResponse::Success(_)
        ));
        Ok(())
    }

    #[test]
    fn login_server_enforces_the_gates_in_order() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{
            Credential, LoginGates, LoginRedirect, LoginRejectKind, LoginServer,
            parse_login_request, password_hash,
        };

        let make_request = |password: &str, agree_to_tos: bool, read_critical: bool| {
            let mut request = LoginRequest::new(
                "Test",
                "User",
                password,
                StartLocation::Last,
                "MyViewer",
                "1.2.3",
            );
            request.agree_to_tos = agree_to_tos;
            request.read_critical = read_critical;
            parse_login_request(&build_login_request(&request))
        };
        let credential = Credential {
            password_hash: password_hash("secret"),
            mfa: None,
        };
        let all_gates = LoginGates {
            redirect: Some(LoginRedirect {
                next_url: "https://login.example.com/real".parse()?,
                next_method: "login_to_simulator".to_owned(),
                message: None,
                next_options: Vec::new(),
            }),
            tos_message: Some("Please accept the updated terms.".to_owned()),
            critical_message: Some("Grid maintenance tonight.".to_owned()),
            already_logged_in: true,
        };

        // A redirect is served before everything else — even a bad password.
        assert!(matches!(
            LoginServer::respond(
                &make_request("wrong", false, false)?,
                &credential,
                &all_gates,
                Box::new(full_success()?),
            ),
            LoginResponse::Redirect(_)
        ));

        // Without the redirect, a wrong password is reported before the
        // ToS/critical gates.
        let mut gates = all_gates.clone();
        gates.redirect = None;
        let LoginResponse::Failure(failure) = LoginServer::respond(
            &make_request("wrong", false, false)?,
            &credential,
            &gates,
            Box::new(full_success()?),
        ) else {
            return Err("expected a bad-password failure".into());
        };
        assert_eq!(failure.reason, LoginServer::BAD_CREDENTIALS_REASON);

        // Correct password, ToS not yet agreed → the "tos" gate, carrying the
        // ToS text as its message.
        let LoginResponse::Failure(failure) = LoginServer::respond(
            &make_request("secret", false, false)?,
            &credential,
            &gates,
            Box::new(full_success()?),
        ) else {
            return Err("expected a tos failure".into());
        };
        assert_eq!(failure.reason, LoginServer::TOS_REASON);
        assert_eq!(failure.message, "Please accept the updated terms.");
        assert_eq!(failure.kind(), LoginRejectKind::Tos);

        // ToS agreed (the viewer's retry) → the critical-message gate next.
        let LoginResponse::Failure(failure) = LoginServer::respond(
            &make_request("secret", true, false)?,
            &credential,
            &gates,
            Box::new(full_success()?),
        ) else {
            return Err("expected a critical failure".into());
        };
        assert_eq!(failure.reason, LoginServer::CRITICAL_REASON);
        assert_eq!(failure.message, "Grid maintenance tonight.");
        assert_eq!(failure.kind(), LoginRejectKind::CriticalMessage);

        // Both acknowledged → the presence gate, whose message classifies as
        // the retryable already-logged-in rejection.
        let LoginResponse::Failure(failure) = LoginServer::respond(
            &make_request("secret", true, true)?,
            &credential,
            &gates,
            Box::new(full_success()?),
        ) else {
            return Err("expected a presence failure".into());
        };
        assert_eq!(failure.reason, LoginServer::PRESENCE_REASON);
        assert_eq!(failure.kind(), LoginRejectKind::AlreadyLoggedIn);

        // Every gate cleared → success.
        gates.already_logged_in = false;
        assert!(matches!(
            LoginServer::respond(
                &make_request("secret", true, true)?,
                &credential,
                &gates,
                Box::new(full_success()?),
            ),
            LoginResponse::Success(_)
        ));
        Ok(())
    }

    #[test]
    fn filter_options_clears_unrequested_sections() -> Result<(), Box<dyn std::error::Error>> {
        let mut success = full_success()?;
        success.filter_options(&["inventory-root".to_owned(), "gestures".to_owned()]);

        // Requested sections survive.
        assert!(success.inventory_root.is_some());
        assert!(!success.gestures.is_empty());
        // Unrequested optioned sections are cleared…
        assert!(success.inventory_skeleton.is_empty());
        assert!(success.buddy_list.is_empty());
        assert!(success.login_flags.is_none());
        assert!(success.global_textures.is_none());
        assert!(success.ui_config.is_none());
        assert!(success.initial_outfit.is_none());
        assert!(success.newuser_config.is_none());
        assert!(success.voice_config.is_none());
        assert!(success.event_categories.is_empty());
        assert!(success.event_notifications.is_empty());
        assert!(success.classified_categories.is_empty());
        assert!(success.tutorial_settings.is_empty());
        assert!(success.library_root.is_none());
        assert!(success.library_owner.is_none());
        assert!(success.library_skeleton.is_empty());
        assert!(success.map_server_url.is_none());
        assert!(success.max_agent_groups.is_none());
        // …while non-optioned fields stay untouched.
        assert!(success.home.is_some());
        assert!(success.seed_capability.as_str().contains("CAPS"));
        assert!(success.account_type.is_some());
        Ok(())
    }

    #[test]
    fn parses_an_opensim_shaped_response() -> Result<(), Box<dyn std::error::Error>> {
        // Quirks a real OpenSim response exhibits: the `max_groups` alias,
        // `classified_fee` as a string, Y/N section flags, and an *empty*
        // `event_categories` array (parsed as "none provided").
        let xml = response(
            "<member><name>login</name><value><string>true</string></value></member>\
             <member><name>agent_id</name><value><string>11111111-1111-1111-1111-111111111111</string></value></member>\
             <member><name>session_id</name><value><string>22222222-2222-2222-2222-222222222222</string></value></member>\
             <member><name>secure_session_id</name><value><string>33333333-3333-3333-3333-333333333333</string></value></member>\
             <member><name>circuit_code</name><value><i4>123456</i4></value></member>\
             <member><name>sim_ip</name><value><string>127.0.0.1</string></value></member>\
             <member><name>sim_port</name><value><i4>9000</i4></value></member>\
             <member><name>seed_capability</name><value><string>http://127.0.0.1:9000/CAPS/seed</string></value></member>\
             <member><name>max_groups</name><value><i4>42</i4></value></member>\
             <member><name>classified_fee</name><value><string>0</string></value></member>\
             <member><name>login-flags</name><value><array><data>\
             <value><struct>\
             <member><name>daylight_savings</name><value><string>N</string></value></member>\
             <member><name>stipend_since_login</name><value><string>N</string></value></member>\
             <member><name>gendered</name><value><string>Y</string></value></member>\
             <member><name>ever_logged_in</name><value><string>Y</string></value></member>\
             </struct></value>\
             </data></array></value></member>\
             <member><name>event_categories</name><value><array><data></data></array></value></member>\
             <member><name>seconds_since_epoch</name><value><i4>1755000000</i4></value></member>",
        );
        let LoginResponse::Success(success) = parse_login_response(&xml)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.max_agent_groups, Some(42));
        assert_eq!(success.classified_fee, Some(0));
        let flags = success.login_flags.ok_or("login-flags")?;
        assert!(flags.ever_logged_in);
        assert!(flags.gendered);
        assert!(!flags.daylight_savings);
        assert_eq!(flags.stipend_since_login, "N");
        assert!(success.event_categories.is_empty());
        assert_eq!(success.seconds_since_epoch, Some(1_755_000_000));
        Ok(())
    }

    #[test]
    fn request_round_trips_the_viewer_identification_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{build_login_request_with_method, parse_login_request};

        let mut request = LoginRequest::new(
            "Test",
            "User",
            "secret",
            StartLocation::Last,
            "MyViewer",
            "1.2.3",
        );
        request.platform_string = "Linux 6.1".to_owned();
        request.platform_version = "6.1.0".to_owned();
        request.address_size = 32;
        request.host_id = "host-42".to_owned();
        request.last_exec_event = Some(3);
        request.last_exec_duration = Some(1200);
        request.last_exec_session_id =
            Some("55555555-5555-5555-5555-555555555555".parse::<uuid::Uuid>()?);
        request.agree_to_tos = false;
        request.read_critical = false;

        let parsed = parse_login_request(&build_login_request(&request))?;
        assert_eq!(parsed.platform_string, "Linux 6.1");
        assert_eq!(parsed.platform_version, "6.1.0");
        assert_eq!(parsed.address_size, Some(32));
        assert_eq!(parsed.host_id, "host-42");
        assert_eq!(parsed.last_exec_event, Some(3));
        assert_eq!(parsed.last_exec_duration, Some(1200));
        assert_eq!(
            parsed.last_exec_session_id,
            Some("55555555-5555-5555-5555-555555555555".parse::<uuid::Uuid>()?)
        );
        assert!(!parsed.agree_to_tos);
        assert!(!parsed.read_critical);
        // Fields only other clients send parse to their empty defaults.
        assert_eq!(parsed.scope_id, None);
        assert_eq!(parsed.web_login_key, None);

        // A redirect's `next_method` renames the XML-RPC call, nothing else.
        let renamed = build_login_request_with_method(&request, "login_to_simulator_elsewhere");
        assert!(renamed.contains("<methodName>login_to_simulator_elsewhere</methodName>"));
        assert_eq!(
            parse_login_request(&renamed)?,
            parse_login_request(&build_login_request(&request))?
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_the_builder_field_names() -> Result<(), Box<dyn std::error::Error>> {
        // The fields the builder writes must match the names OpenSim expects.
        let request = LoginRequest::new(
            "First",
            "Last",
            "pw",
            StartLocation::Home,
            "MyViewer",
            "1.2.3",
        );
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

    #[test]
    fn start_location_renders_the_three_wire_forms() {
        assert_eq!(StartLocation::Last.to_wire_string(), "last");
        assert_eq!(StartLocation::Home.to_wire_string(), "home");
        assert_eq!(
            StartLocation::region("Hello World", RegionCoordinates::new(128.0, 64.5, 30.0))
                .to_wire_string(),
            "uri:Hello World&128&64.5&30"
        );
    }

    #[test]
    fn start_location_round_trips_through_its_wire_string() -> Result<(), Box<dyn std::error::Error>>
    {
        for location in [
            StartLocation::Last,
            StartLocation::Home,
            StartLocation::region("Sandbox", RegionCoordinates::new(128.0, 128.0, 30.0)),
        ] {
            let wire = location.to_wire_string();
            assert_eq!(wire.parse::<StartLocation>()?, location, "for {wire:?}");
        }
        Ok(())
    }

    #[test]
    fn start_location_parses_a_uri_with_an_ampersand_in_the_region_name() {
        // The region name is taken as everything before the trailing three
        // `&`-separated coordinates, so a stray `&` in the name still parses.
        assert_eq!(
            "uri:A&B&1&2&3".parse::<StartLocation>(),
            Ok(StartLocation::region(
                "A&B",
                RegionCoordinates::new(1.0, 2.0, 3.0)
            ))
        );
    }

    #[test]
    fn start_location_rejects_out_of_grammar_values() {
        // A bare keyword that is neither "last"/"home" nor a "uri:".
        assert!(matches!(
            "nowhere".parse::<StartLocation>(),
            Err(sl_wire::StartLocationParseError::Unrecognized(_))
        ));
        // A "uri:" missing a coordinate.
        assert!(matches!(
            "uri:Sandbox&128&30".parse::<StartLocation>(),
            Err(sl_wire::StartLocationParseError::MalformedUri(_))
        ));
        // A "uri:" with a non-numeric coordinate.
        assert!(matches!(
            "uri:Sandbox&128&128&up".parse::<StartLocation>(),
            Err(sl_wire::StartLocationParseError::MalformedUri(_))
        ));
        // A "uri:" with an empty region name.
        assert!(matches!(
            "uri:&1&2&3".parse::<StartLocation>(),
            Err(sl_wire::StartLocationParseError::MalformedUri(_))
        ));
    }
}
