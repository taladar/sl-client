//! Tests for the LLSD variant of the login request builder and response
//! parser, pairing it against the XML-RPC variant for equivalence.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_wire::{
        LoginFailure, LoginRedirect, LoginRequest, LoginResponse, MfaChallenge, StartLocation,
        build_login_request, build_login_request_llsd, build_login_response_llsd,
        parse_login_request, parse_login_request_llsd, parse_login_response_llsd,
    };

    /// A viewer-shaped login request exercising the identification fields.
    fn request() -> Result<LoginRequest, Box<dyn std::error::Error>> {
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
        request.host_id = "host-42".to_owned();
        request.last_exec_event = Some(3);
        request.last_exec_duration = Some(1200);
        request.last_exec_session_id =
            Some("55555555-5555-5555-5555-555555555555".parse::<uuid::Uuid>()?);
        Ok(request)
    }

    #[test]
    fn llsd_request_parses_like_the_xmlrpc_request() -> Result<(), Box<dyn std::error::Error>> {
        // Both codecs must carry the identical field set: the same request
        // built through either transport parses to the same server-side view.
        let request = request()?;
        let via_llsd = parse_login_request_llsd(&build_login_request_llsd(&request))?;
        let via_xmlrpc = parse_login_request(&build_login_request(&request))?;
        assert_eq!(via_llsd, via_xmlrpc);
        Ok(())
    }

    #[test]
    fn llsd_request_uses_native_types() -> Result<(), Box<dyn std::error::Error>> {
        let body = build_login_request_llsd(&request()?);
        // Booleans and integers ride natively, not as strings.
        assert!(body.contains("<key>agree_to_tos</key><boolean>true</boolean>"));
        assert!(body.contains("<key>address_size</key><integer>64</integer>"));
        assert!(body.contains("<key>last_exec_session_id</key><uuid>"));
        Ok(())
    }

    #[test]
    fn llsd_response_round_trips_a_full_success() -> Result<(), Box<dyn std::error::Error>> {
        // The one shared fixture: the XML-RPC suite's `full_success` is not
        // importable across test binaries, so round-trip the response the
        // XML-RPC builder+parser produce — an equality that also proves the
        // two variants carry the same information.
        let xmlrpc_suite = sl_wire::build_login_response(&minimal_like_success()?);
        let via_xmlrpc = sl_wire::parse_login_response(&xmlrpc_suite)?;
        let via_llsd = parse_login_response_llsd(&build_login_response_llsd(&via_xmlrpc))?;
        assert_eq!(via_llsd, via_xmlrpc);
        Ok(())
    }

    /// A success populated across every section, built through the public
    /// parser so the fixture stays construction-site-free.
    fn minimal_like_success() -> Result<LoginResponse, Box<dyn std::error::Error>> {
        use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, TextureKey};
        use sl_wire::{
            BuddyListEntry, GestureEntry, GlobalTextures, HomeLocation, InitialOutfit, Llsd,
            LoginCategory, LoginFlags, LoginSuccess, NewUserConfig, SkeletonFolder,
            TutorialSetting, UiConfig, VoiceConfig,
        };

        let mut success = LoginSuccess::minimal(
            AgentKey::from("11111111-1111-1111-1111-111111111111".parse::<uuid::Uuid>()?),
            "22222222-2222-2222-2222-222222222222".parse()?,
            "33333333-3333-3333-3333-333333333333".parse()?,
            sl_wire::CircuitCode(0xC0DE_C0DE),
            std::net::Ipv4Addr::new(127, 0, 0, 1),
            9000,
            "http://127.0.0.1:9000/CAPS/seed".parse()?,
        );
        success.message = Some("Welcome <home> & enjoy".to_owned());
        success.mfa_hash = Some("rememberme".to_owned());
        success.first_name = Some("Test".to_owned());
        success.last_name = Some("User".to_owned());
        success.display_name = Some("Test User".to_owned());
        success.real_id = Some(AgentKey::from(
            "44444444-4444-4444-4444-444444444444".parse::<uuid::Uuid>()?,
        ));
        success.agent_access = Some("M".to_owned());
        success.agent_access_max = Some("A".to_owned());
        success.agent_region_access = Some("PG".to_owned());
        success.start_location = Some("last".to_owned());
        success.seconds_since_epoch = Some(1_755_000_000);
        success.udp_blacklist = vec!["EnableSimulator".to_owned()];
        success.http_port = Some(9001);
        success.region_x = Some(256_000);
        success.region_y = Some(256_256);
        success.region_size_x = Some(256);
        success.region_size_y = Some(512);
        success.max_agent_groups = Some(42);
        success.home = Some(HomeLocation {
            region_handle: sl_wire::RegionHandle::from_global(256_000, 256_256),
            position: sl_types::map::RegionCoordinates::new(128.5, 127.0, 25.75),
            look_at: sl_wire::Direction::new(1.0, 0.0, 0.0),
        });
        success.look_at = Some(sl_wire::Direction::new(1.0, 0.0, 0.0));
        success.inventory_root = Some(InventoryFolderKey::from(
            "aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?,
        ));
        success.inventory_skeleton = vec![SkeletonFolder {
            folder_id: InventoryFolderKey::from(
                "aaaaaaaa-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?,
            ),
            parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
            name: "My Inventory".to_owned(),
            type_default: 8,
            version: 5,
        }];
        success.library_root = Some(InventoryFolderKey::from(
            "00000112-000f-0000-0000-000100bba000".parse::<uuid::Uuid>()?,
        ));
        success.library_owner = Some(AgentKey::from(
            "11111111-1111-0000-0000-000000000000".parse::<uuid::Uuid>()?,
        ));
        success.library_skeleton = vec![SkeletonFolder {
            folder_id: InventoryFolderKey::from(
                "00000112-000f-0000-0000-000100bba000".parse::<uuid::Uuid>()?,
            ),
            parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
            name: "Library".to_owned(),
            type_default: 8,
            version: 1,
        }];
        success.buddy_list = vec![BuddyListEntry {
            buddy_id: "cccccccc-0000-0000-0000-000000000000".parse()?,
            rights_granted: 3,
            rights_has: 1,
        }];
        success.gestures = vec![GestureEntry {
            item_id: InventoryKey::from(
                "dddddddd-0000-0000-0000-000000000000".parse::<uuid::Uuid>()?,
            ),
            asset_id: "eeeeeeee-0000-0000-0000-000000000000".parse()?,
        }];
        success.login_flags = Some(LoginFlags {
            ever_logged_in: true,
            daylight_savings: false,
            gendered: true,
            stipend_since_login: "N".to_owned(),
        });
        success.global_textures = Some(GlobalTextures {
            sun_texture_id: TextureKey::from(
                "cce0f112-878f-4586-a2e2-a8f104bba271".parse::<uuid::Uuid>()?,
            ),
            cloud_texture_id: TextureKey::from(
                "dc4b9f0b-d008-45c6-96a4-01dd947ac621".parse::<uuid::Uuid>()?,
            ),
            moon_texture_id: TextureKey::from(
                "ec4b9f0b-d008-45c6-96a4-01dd947ac621".parse::<uuid::Uuid>()?,
            ),
        });
        success.ui_config = Some(UiConfig {
            allow_first_life: true,
        });
        success.initial_outfit = Some(InitialOutfit {
            folder_name: "Nightclub Female".to_owned(),
            gender: "female".to_owned(),
        });
        success.newuser_config = Some(NewUserConfig {
            default_female_avatar: Some("Ruth".to_owned()),
            default_male_avatar: None,
        });
        success.voice_config = Some(VoiceConfig {
            voice_server_type: "webrtc".to_owned(),
        });
        success.event_categories = vec![LoginCategory {
            category_id: 18,
            category_name: "Discussion".to_owned(),
        }];
        success.classified_categories = vec![LoginCategory {
            category_id: 1,
            category_name: "Shopping".to_owned(),
        }];
        success.event_notifications = vec![Llsd::Map(
            [("event_id".to_owned(), Llsd::Integer(7))]
                .into_iter()
                .collect(),
        )];
        success.tutorial_settings = vec![TutorialSetting {
            tutorial_url: "http://example.com/tutorial/".to_owned(),
        }];
        success.help_url_format = Some("https://help.example.com/[TOPIC]".to_owned());
        success.web_profile_url = Some(url::Url::parse("https://my.example.com/")?);
        success.profile_server_url = Some(url::Url::parse("http://127.0.0.1:9000/profiles")?);
        success.search_url = Some(url::Url::parse("http://127.0.0.1:9000/search")?);
        success.destination_guide_url = Some(url::Url::parse("https://guide.example.com/")?);
        success.avatar_picker_url = Some(url::Url::parse("https://picker.example.com/")?);
        success.currency = Some("L$".to_owned());
        success.classified_fee = Some(50);
        success.directory_fee = Some(30);
        success.account_type = Some("Premium".to_owned());
        success.account_level_benefits = Some(Llsd::Map(
            [("attachment_limit".to_owned(), Llsd::Integer(38))]
                .into_iter()
                .collect(),
        ));
        success.premium_packages = Some(Llsd::Map(
            [
                (
                    "Base".to_owned(),
                    Llsd::Map(std::collections::HashMap::new()),
                ),
                (
                    "Premium".to_owned(),
                    Llsd::Map(std::collections::HashMap::new()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        success.agent_appearance_service =
            Some(url::Url::parse("https://appearance.example.com/")?);
        success.map_server_url = Some(url::Url::parse("http://127.0.0.1:9000/")?);
        success.openid_url = Some(url::Url::parse("https://id.example.com/openid")?);
        success.openid_token = Some("open-id-token-blob".to_owned());
        Ok(LoginResponse::Success(Box::new(success)))
    }

    #[test]
    fn llsd_success_uses_native_types_and_wrapped_sections()
    -> Result<(), Box<dyn std::error::Error>> {
        let body = build_login_response_llsd(&minimal_like_success()?);
        // Ids are native uuids, ports native integers.
        assert!(
            body.contains("<key>agent_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>")
        );
        assert!(body.contains("<key>sim_port</key><integer>9000</integer>"));
        // The circuit code wraps negative through the OpenSim (int) cast
        // (0xC0DEC0DE > i32::MAX) rather than truncating or panicking.
        assert!(body.contains("<key>circuit_code</key><integer>-1059143458</integer>"));
        // The config-like sections keep the one-element-array (WrapOSDMap)
        // shape: the section key is immediately followed by an array holding
        // one map.
        assert!(body.contains("<key>login-flags</key><array><map>"));
        assert!(body.contains("<key>ui-config</key><array><map>"));
        Ok(())
    }

    #[test]
    fn llsd_response_round_trips_a_failure() -> Result<(), Box<dyn std::error::Error>> {
        let mut failure = LoginFailure::new("key", "Could not authenticate your avatar.");
        failure.message_id = Some("LoginFailedAccountSuspended".to_owned());
        failure.message_args = [("TIME".to_owned(), "December 12, 2026".to_owned())]
            .into_iter()
            .collect();
        let body = build_login_response_llsd(&LoginResponse::Failure(failure.clone()));
        let LoginResponse::Failure(parsed) = parse_login_response_llsd(&body)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(parsed, failure);
        Ok(())
    }

    #[test]
    fn llsd_response_round_trips_an_mfa_challenge() -> Result<(), Box<dyn std::error::Error>> {
        let challenge = MfaChallenge {
            mfa_hash: Some("challengehash".to_owned()),
            message: "Enter your token".to_owned(),
        };
        let body = build_login_response_llsd(&LoginResponse::MfaChallenge(challenge.clone()));
        let LoginResponse::MfaChallenge(parsed) = parse_login_response_llsd(&body)? else {
            return Err("expected an MFA challenge".into());
        };
        assert_eq!(parsed, challenge);
        Ok(())
    }

    #[test]
    fn llsd_response_round_trips_a_redirect() -> Result<(), Box<dyn std::error::Error>> {
        let redirect = LoginRedirect {
            next_url: "https://login.example.com/second".parse()?,
            next_method: "login_to_simulator".to_owned(),
            message: Some("Redirecting…".to_owned()),
            next_options: vec!["inventory-root".to_owned()],
        };
        let body = build_login_response_llsd(&LoginResponse::Redirect(redirect.clone()));
        let LoginResponse::Redirect(parsed) = parse_login_response_llsd(&body)? else {
            return Err("expected a redirect".into());
        };
        assert_eq!(parsed, redirect);
        Ok(())
    }

    #[test]
    fn llsd_indeterminate_without_next_url_degrades_to_a_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let body = "<llsd><map><key>login</key><string>indeterminate</string>\
                    <key>message</key><string>try again</string></map></llsd>";
        let LoginResponse::Failure(failure) = parse_login_response_llsd(body)? else {
            return Err("expected a failure".into());
        };
        assert_eq!(failure.reason, "indeterminate");
        assert_eq!(failure.message, "try again");
        Ok(())
    }

    #[test]
    fn llsd_non_map_body_is_rejected() {
        assert!(matches!(
            parse_login_response_llsd("<llsd><array/></llsd>"),
            Err(sl_wire::LoginParseError::NoStruct)
        ));
        assert!(matches!(
            parse_login_request_llsd("<llsd><string>nope</string></llsd>"),
            Err(sl_wire::LoginParseError::NoStruct)
        ));
    }
}
