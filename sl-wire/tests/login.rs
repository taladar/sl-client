//! Tests for the XML-RPC login request builder and response parser.

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use pretty_assertions::assert_eq;
    use sl_wire::{
        LoginRequest, LoginResponse, build_login_request, parse_login_response, password_hash,
    };

    /// Asserts two three-component vectors are equal within a small tolerance
    /// (the login reals round-trip through `f64` parsing then narrow to `f32`).
    fn assert_vec3_approx(actual: [f32; 3], expected: [f32; 3]) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!((a - e).abs() < 1e-4, "{actual:?} != {expected:?}");
        }
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
        let request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3");
        let body = build_login_request(&request);
        assert!(body.contains("<value><string>buddy-list</string></value>"));
    }

    #[test]
    fn request_carries_library_options() {
        let request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3");
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
        assert_eq!(home.region_handle, (256_000, 256_256));
        assert_vec3_approx(home.position, [128.5, 127.0, 25.75]);
        assert_vec3_approx(home.look_at, [1.0, 0.0, 0.0]);
        let look_at = success.look_at.ok_or("start look-at")?;
        assert_vec3_approx(look_at, [0.9994, 0.0316, 0.0]);
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
            Some("00000112-000f-0000-0000-000100bba000".parse::<uuid::Uuid>()?)
        );
        assert_eq!(
            success.library_owner,
            Some("11111111-1111-0000-0000-000000000000".parse::<uuid::Uuid>()?)
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
    fn parse_login_request_round_trips_the_builder() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{parse_login_request, password_hash};

        let mut request = LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3")
            .with_mfa("123456", Some("storedhash".to_owned()));
        request.options = vec!["inventory-root".to_owned(), "buddy-list".to_owned()];
        let body = build_login_request(&request);

        let parsed = parse_login_request(&body)?;
        assert_eq!(parsed.first_name, "Test");
        assert_eq!(parsed.last_name, "User");
        // The server only ever sees the hashed password, never the plaintext.
        assert_eq!(parsed.password_hash, password_hash("secret"));
        assert_eq!(parsed.start, "last");
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

    /// A full success with every optional payload, to exercise `build_login_response`.
    fn full_success() -> Result<sl_wire::LoginSuccess, Box<dyn std::error::Error>> {
        use sl_wire::{BuddyListEntry, HomeLocation, SkeletonFolder};

        let folder = |id: &str,
                      parent: &str,
                      name: &str,
                      type_default,
                      version|
         -> Result<SkeletonFolder, Box<dyn std::error::Error>> {
            Ok(SkeletonFolder {
                folder_id: id.parse()?,
                parent_id: parent.parse()?,
                name: name.to_owned(),
                type_default,
                version,
            })
        };
        Ok(sl_wire::LoginSuccess {
            agent_id: "11111111-1111-1111-1111-111111111111".parse()?,
            session_id: "22222222-2222-2222-2222-222222222222".parse()?,
            secure_session_id: "33333333-3333-3333-3333-333333333333".parse()?,
            circuit_code: 123_456,
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/CAPS/seed".to_owned(),
            message: Some("Welcome <home> & enjoy".to_owned()),
            mfa_hash: Some("rememberme".to_owned()),
            inventory_root: Some("aaaaaaaa-0000-0000-0000-000000000000".parse()?),
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
                region_handle: (256_000, 256_256),
                position: [128.5, 127.0, 25.75],
                look_at: [1.0, 0.0, 0.0],
            }),
            look_at: Some([0.9994, 0.0316, 0.0]),
            agent_access: Some("M".to_owned()),
            agent_access_max: Some("A".to_owned()),
            max_agent_groups: Some(42),
            library_root: Some("00000112-000f-0000-0000-000100bba000".parse()?),
            library_owner: Some("11111111-1111-0000-0000-000000000000".parse()?),
            library_skeleton: vec![folder(
                "00000112-000f-0000-0000-000100bba000",
                "00000000-0000-0000-0000-000000000000",
                "Library",
                8,
                1,
            )?],
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
        assert_eq!(home.region_handle, (256_000, 256_256));
        assert_vec3_approx(home.position, [128.5, 127.0, 25.75]);
        assert_vec3_approx(home.look_at, [1.0, 0.0, 0.0]);
        assert_vec3_approx(parsed.look_at.ok_or("look_at")?, [0.9994, 0.0316, 0.0]);
        assert_eq!(parsed.agent_access.as_deref(), Some("M"));
        assert_eq!(parsed.agent_access_max.as_deref(), Some("A"));
        assert_eq!(parsed.max_agent_groups, Some(42));
        assert_eq!(parsed.library_root, success.library_root);
        assert_eq!(parsed.library_owner, success.library_owner);
        assert_eq!(parsed.library_skeleton, success.library_skeleton);
        Ok(())
    }

    #[test]
    fn build_login_response_round_trips_a_failure() -> Result<(), Box<dyn std::error::Error>> {
        use sl_wire::{LoginFailure, build_login_response, parse_login_response};

        let failure = LoginFailure {
            reason: "key".to_owned(),
            message: "Could not authenticate your avatar.".to_owned(),
        };
        let xml = build_login_response(&LoginResponse::Failure(failure.clone()));
        let LoginResponse::Failure(parsed) = parse_login_response(&xml)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(parsed, failure);
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
        use sl_wire::{Credential, LoginServer, MfaPolicy, parse_login_request, password_hash};

        let make_request = |password: &str, token: &str, mfa_hash: Option<String>| {
            let request = LoginRequest::new("Test", "User", password, "last", "MyViewer", "1.2.3")
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
            LoginServer::respond(&ok, &no_mfa, Box::new(full_success()?)),
            LoginResponse::Success(_)
        ));

        // Wrong password → failure with the "key" reason.
        let bad = make_request("wrong", "", None)?;
        let LoginResponse::Failure(failure) =
            LoginServer::respond(&bad, &no_mfa, Box::new(full_success()?))
        else {
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
        let LoginResponse::MfaChallenge(challenge) =
            LoginServer::respond(&first, &mfa, Box::new(full_success()?))
        else {
            return Err("expected an MFA challenge".into());
        };
        assert_eq!(challenge.mfa_hash.as_deref(), Some("remember-this-device"));

        // MFA satisfied by the one-time token → success.
        let with_token = make_request("secret", "999999", None)?;
        assert!(matches!(
            LoginServer::respond(&with_token, &mfa, Box::new(full_success()?)),
            LoginResponse::Success(_)
        ));

        // MFA satisfied by echoing the remembered hash → success.
        let with_hash = make_request("secret", "", Some("remember-this-device".to_owned()))?;
        assert!(matches!(
            LoginServer::respond(&with_hash, &mfa, Box::new(full_success()?)),
            LoginResponse::Success(_)
        ));
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
