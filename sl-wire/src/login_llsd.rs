//! The LLSD variant of the `login_to_simulator` call.
//!
//! Grids accept the login request in two codecs at the **same URL**, selected
//! purely by the POST's `Content-Type`: `text/xml` is dispatched to the
//! XML-RPC handler ([`build_login_request`](crate::login::build_login_request)
//! / [`parse_login_request`](crate::login::parse_login_request)), while
//! `application/llsd+xml` is dispatched to the LLSD handler modelled here
//! (OpenSim serves it as its default LLSD handler; Second Life accepts it
//! too). The reference viewer (Firestorm) only ever sends XML-RPC, so this
//! variant exists for server-side completeness — a fake grid that wants to
//! accept *any* conformant client — and for clients that prefer LLSD. The
//! HTTP dispatch itself is the transport's job, not this module's.
//!
//! Field names and shapes match the XML-RPC variant; the differences are
//! purely representational, following OpenSim's `ToOSDMap` conventions:
//! ids are native [`Llsd::Uuid`]s, ports/codes/counts native
//! [`Llsd::Integer`]s, request booleans native [`Llsd::Boolean`]s, and the
//! array-of-one-struct response sections (`login-flags`, `ui-config`, …)
//! are a one-element [`Llsd::Array`] wrapping an [`Llsd::Map`]. The `home`
//! and `look_at` fields stay the quasi-notation *strings* the XML-RPC
//! variant uses, and yes/no flags stay `"Y"`/`"N"` strings.

use std::collections::HashMap;

use uuid::Uuid;

use crate::CircuitCode;
use crate::llsd::{Llsd, parse_llsd_xml};
use crate::login::{
    BuddyListEntry, GestureEntry, GlobalTextures, InitialOutfit, LoginCategory, LoginFailure,
    LoginFlags, LoginParseError, LoginRedirect, LoginRequest, LoginResponse, LoginSuccess,
    MfaChallenge, NewUserConfig, ParsedLoginRequest, SkeletonFolder, TutorialSetting, UiConfig,
    VoiceConfig, home_to_string, parse_direction, parse_home, parse_start_member, password_hash,
    vector3_to_string, yn_str, yn_wire_flag,
};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, TextureKey};

/// Builds the LLSD-XML request body for a `login_to_simulator` call — the
/// same fields as [`build_login_request`](crate::login::build_login_request),
/// POSTed with `Content-Type: application/llsd+xml` instead of `text/xml`.
#[must_use]
pub fn build_login_request_llsd(request: &LoginRequest) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let mut put = |key: &str, value: Llsd| {
        map.insert(key.to_owned(), value);
    };
    put("first", Llsd::String(request.first_name.clone()));
    put("last", Llsd::String(request.last_name.clone()));
    put("passwd", Llsd::String(password_hash(&request.password)));
    put("start", Llsd::String(request.start.to_wire_string()));
    put("channel", Llsd::String(request.channel.clone()));
    put("version", Llsd::String(request.version.clone()));
    put("platform", Llsd::String(request.platform.clone()));
    put(
        "platform_string",
        Llsd::String(request.platform_string.clone()),
    );
    put(
        "platform_version",
        Llsd::String(request.platform_version.clone()),
    );
    put("address_size", Llsd::Integer(request.address_size));
    put("host_id", Llsd::String(request.host_id.clone()));
    put("mac", Llsd::String(request.mac.clone()));
    put("id0", Llsd::String(request.id0.clone()));
    if let Some(event) = request.last_exec_event {
        put("last_exec_event", Llsd::Integer(event));
    }
    if let Some(duration) = request.last_exec_duration {
        put("last_exec_duration", Llsd::Integer(duration));
    }
    if let Some(session_id) = request.last_exec_session_id {
        put("last_exec_session_id", Llsd::Uuid(session_id));
    }
    put("token", Llsd::String(request.token.clone()));
    put("mfa_hash", Llsd::String(request.mfa_hash.clone()));
    put("agree_to_tos", Llsd::Boolean(request.agree_to_tos));
    put("read_critical", Llsd::Boolean(request.read_critical));
    // Request structured error reasons (e.g. `mfa_challenge`).
    put("extended_errors", Llsd::Boolean(true));
    put(
        "options",
        Llsd::Array(
            request
                .options
                .iter()
                .map(|option| Llsd::String(option.clone()))
                .collect(),
        ),
    );
    Llsd::Map(map).to_llsd_xml()
}

/// Parses an LLSD-XML `login_to_simulator` request body into its fields —
/// the LLSD counterpart of
/// [`parse_login_request`](crate::login::parse_login_request), with the same
/// missing-member defaults.
///
/// # Errors
///
/// Returns a [`LoginParseError`] if the body is not well-formed LLSD-XML or
/// its top-level value is not a map.
pub fn parse_login_request_llsd(body: &str) -> Result<ParsedLoginRequest, LoginParseError> {
    let value = parse_llsd_xml(body).map_err(|_error| LoginParseError::NoStruct)?;
    let Llsd::Map(map) = value else {
        return Err(LoginParseError::NoStruct);
    };
    Ok(ParsedLoginRequest {
        first_name: llsd_string(&map, "first"),
        last_name: llsd_string(&map, "last"),
        password_hash: llsd_string(&map, "passwd"),
        start: parse_start_member(llsd_string(&map, "start")),
        channel: llsd_string(&map, "channel"),
        version: llsd_string(&map, "version"),
        platform: llsd_string(&map, "platform"),
        platform_string: llsd_string(&map, "platform_string"),
        platform_version: llsd_string(&map, "platform_version"),
        address_size: llsd_i32(&map, "address_size"),
        host_id: llsd_string(&map, "host_id"),
        mac: llsd_string(&map, "mac"),
        id0: llsd_string(&map, "id0"),
        last_exec_event: llsd_i32(&map, "last_exec_event"),
        last_exec_duration: llsd_i32(&map, "last_exec_duration"),
        last_exec_session_id: llsd_uuid(&map, "last_exec_session_id"),
        scope_id: llsd_uuid(&map, "scope_id"),
        web_login_key: llsd_uuid(&map, "web_login_key"),
        token: llsd_string(&map, "token"),
        mfa_hash: llsd_string(&map, "mfa_hash"),
        agree_to_tos: llsd_bool(&map, "agree_to_tos"),
        read_critical: llsd_bool(&map, "read_critical"),
        extended_errors: llsd_bool(&map, "extended_errors"),
        options: map
            .get("options")
            .and_then(Llsd::as_array)
            .map(|options| options.iter().filter_map(llsd_scalar_string).collect())
            .unwrap_or_default(),
    })
}

/// Builds the LLSD-XML response body for a [`LoginResponse`] — the LLSD
/// counterpart of [`build_login_response`](crate::login::build_login_response),
/// served with `Content-Type: application/llsd+xml`.
#[must_use]
pub fn build_login_response_llsd(response: &LoginResponse) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    match response {
        LoginResponse::Success(success) => insert_success_members(&mut map, success),
        LoginResponse::MfaChallenge(challenge) => {
            map.insert("login".to_owned(), Llsd::String("false".to_owned()));
            map.insert(
                "reason".to_owned(),
                Llsd::String("mfa_challenge".to_owned()),
            );
            map.insert(
                "message".to_owned(),
                Llsd::String(challenge.message.clone()),
            );
            if let Some(mfa_hash) = &challenge.mfa_hash {
                map.insert("mfa_hash".to_owned(), Llsd::String(mfa_hash.clone()));
            }
        }
        LoginResponse::Redirect(redirect) => {
            map.insert("login".to_owned(), Llsd::String("indeterminate".to_owned()));
            map.insert(
                "next_url".to_owned(),
                Llsd::String(redirect.next_url.as_str().to_owned()),
            );
            map.insert(
                "next_method".to_owned(),
                Llsd::String(redirect.next_method.clone()),
            );
            if let Some(message) = &redirect.message {
                map.insert("message".to_owned(), Llsd::String(message.clone()));
            }
            if !redirect.next_options.is_empty() {
                map.insert(
                    "next_options".to_owned(),
                    Llsd::Array(
                        redirect
                            .next_options
                            .iter()
                            .map(|option| Llsd::String(option.clone()))
                            .collect(),
                    ),
                );
            }
        }
        LoginResponse::Failure(failure) => {
            map.insert("login".to_owned(), Llsd::String("false".to_owned()));
            map.insert("reason".to_owned(), Llsd::String(failure.reason.clone()));
            map.insert("message".to_owned(), Llsd::String(failure.message.clone()));
            if let Some(message_id) = &failure.message_id {
                map.insert("message_id".to_owned(), Llsd::String(message_id.clone()));
            }
            if !failure.message_args.is_empty() {
                map.insert(
                    "message_args".to_owned(),
                    Llsd::Map(
                        failure
                            .message_args
                            .iter()
                            .map(|(key, value)| (key.clone(), Llsd::String(value.clone())))
                            .collect(),
                    ),
                );
            }
        }
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Parses an LLSD-XML `login_to_simulator` response body — the LLSD
/// counterpart of [`parse_login_response`](crate::login::parse_login_response),
/// with the same variant selection (`login` = `"true"` / `"indeterminate"` /
/// anything else) and redirect degradation.
///
/// # Errors
///
/// Returns a [`LoginParseError`] if the body is not well-formed LLSD-XML, its
/// top-level value is not a map, or a required success field is missing or
/// invalid.
pub fn parse_login_response_llsd(body: &str) -> Result<LoginResponse, LoginParseError> {
    let value = parse_llsd_xml(body).map_err(|_error| LoginParseError::NoStruct)?;
    let Llsd::Map(map) = value else {
        return Err(LoginParseError::NoStruct);
    };
    let login = llsd_string(&map, "login");
    if login == "indeterminate" {
        let message = map.get("message").and_then(llsd_scalar_string);
        if let Some(next_url) = map
            .get("next_url")
            .and_then(llsd_scalar_string)
            .and_then(|url| url::Url::parse(url.trim()).ok())
        {
            return Ok(LoginResponse::Redirect(LoginRedirect {
                next_url,
                next_method: map
                    .get("next_method")
                    .and_then(llsd_scalar_string)
                    .unwrap_or_else(|| "login_to_simulator".to_owned()),
                message,
                next_options: map
                    .get("next_options")
                    .and_then(Llsd::as_array)
                    .map(|options| options.iter().filter_map(llsd_scalar_string).collect())
                    .unwrap_or_default(),
            }));
        }
        return Ok(LoginResponse::Failure(LoginFailure::new(
            "indeterminate",
            message.unwrap_or_default(),
        )));
    }
    if login != "true" {
        let reason = llsd_string(&map, "reason");
        let message = llsd_string(&map, "message");
        if reason == "mfa_challenge" {
            return Ok(LoginResponse::MfaChallenge(MfaChallenge {
                mfa_hash: map.get("mfa_hash").and_then(llsd_scalar_string),
                message,
            }));
        }
        return Ok(LoginResponse::Failure(LoginFailure {
            reason,
            message,
            message_id: map.get("message_id").and_then(llsd_scalar_string),
            message_args: map
                .get("message_args")
                .and_then(Llsd::as_map)
                .map(|args| {
                    args.iter()
                        .filter_map(|(key, value)| Some((key.clone(), llsd_scalar_string(value)?)))
                        .collect()
                })
                .unwrap_or_default(),
        }));
    }
    parse_success_members(&map).map(|success| LoginResponse::Success(Box::new(success)))
}

/// Inserts a successful login's members into the response map, mirroring the
/// XML-RPC emitter's field set with the LLSD-native representations.
fn insert_success_members(map: &mut HashMap<String, Llsd>, success: &LoginSuccess) {
    let mut put = |key: &str, value: Llsd| {
        map.insert(key.to_owned(), value);
    };
    let mut put_opt_string = |key: &str, value: Option<&str>| {
        if let Some(value) = value {
            put(key, Llsd::String(value.to_owned()));
        }
    };
    put_opt_string("message", success.message.as_deref());
    put_opt_string("mfa_hash", success.mfa_hash.as_deref());
    put_opt_string("agent_access", success.agent_access.as_deref());
    put_opt_string("agent_access_max", success.agent_access_max.as_deref());
    put_opt_string(
        "agent_region_access",
        success.agent_region_access.as_deref(),
    );
    put_opt_string("start_location", success.start_location.as_deref());
    put_opt_string("first_name", success.first_name.as_deref());
    put_opt_string("last_name", success.last_name.as_deref());
    put_opt_string("display_name", success.display_name.as_deref());
    put_opt_string("help_url_format", success.help_url_format.as_deref());
    put_opt_string("currency", success.currency.as_deref());
    put_opt_string("account_type", success.account_type.as_deref());
    put_opt_string("openid_token", success.openid_token.as_deref());
    put_opt_string("seed_capability", Some(success.seed_capability.as_str()));
    put_opt_string(
        "agent_appearance_service",
        success
            .agent_appearance_service
            .as_ref()
            .map(url::Url::as_str),
    );
    put_opt_string(
        "map-server-url",
        success.map_server_url.as_ref().map(url::Url::as_str),
    );
    put_opt_string(
        "openid_url",
        success.openid_url.as_ref().map(url::Url::as_str),
    );
    put_opt_string(
        "web_profile_url",
        success.web_profile_url.as_ref().map(url::Url::as_str),
    );
    put_opt_string(
        "profile-server-url",
        success.profile_server_url.as_ref().map(url::Url::as_str),
    );
    put_opt_string("search", success.search_url.as_ref().map(url::Url::as_str));
    put_opt_string(
        "destination_guide_url",
        success.destination_guide_url.as_ref().map(url::Url::as_str),
    );
    put_opt_string(
        "avatar_picker_url",
        success.avatar_picker_url.as_ref().map(url::Url::as_str),
    );

    put("login", Llsd::String("true".to_owned()));
    put("agent_id", Llsd::Uuid(success.agent_id.uuid()));
    put("session_id", Llsd::Uuid(success.session_id));
    put("secure_session_id", Llsd::Uuid(success.secure_session_id));
    put("circuit_code", circuit_code_to_llsd(success.circuit_code));
    put("sim_ip", Llsd::String(success.sim_ip.to_string()));
    put("sim_port", Llsd::Integer(i32::from(success.sim_port)));
    if let Some(real_id) = success.real_id {
        put("real_id", Llsd::Uuid(real_id.uuid()));
    }
    if let Some(home) = &success.home {
        put("home", Llsd::String(home_to_string(home)));
    }
    if let Some(look_at) = success.look_at {
        put(
            "look_at",
            Llsd::String(vector3_to_string([look_at.x(), look_at.y(), look_at.z()])),
        );
    }
    if let Some(region_x) = success.region_x {
        put("region_x", u32_to_llsd(region_x));
    }
    if let Some(region_y) = success.region_y {
        put("region_y", u32_to_llsd(region_y));
    }
    if let Some(size) = success.region_size_x {
        put("region_size_x", u32_to_llsd(size));
    }
    if let Some(size) = success.region_size_y {
        put("region_size_y", u32_to_llsd(size));
    }
    if let Some(port) = success.http_port {
        put("http_port", Llsd::Integer(i32::from(port)));
    }
    if let Some(seconds) = success.seconds_since_epoch {
        put(
            "seconds_since_epoch",
            i32::try_from(seconds).map_or_else(
                |_out_of_range| Llsd::Real(seconds_as_real(seconds)),
                Llsd::Integer,
            ),
        );
    }
    if let Some(groups) = success.max_agent_groups {
        put("max-agent-groups", u32_to_llsd(groups));
    }
    if !success.udp_blacklist.is_empty() {
        put(
            "udp_blacklist",
            Llsd::String(success.udp_blacklist.join(",")),
        );
    }
    if let Some(fee) = success.classified_fee {
        put("classified_fee", Llsd::Integer(fee));
    }
    if let Some(fee) = success.directory_fee {
        put("directory_fee", Llsd::Integer(fee));
    }
    if let Some(root) = success.inventory_root {
        put("inventory-root", id_section("folder_id", root.uuid()));
    }
    if let Some(root) = success.library_root {
        put("inventory-lib-root", id_section("folder_id", root.uuid()));
    }
    if let Some(owner) = success.library_owner {
        put("inventory-lib-owner", id_section("agent_id", owner.uuid()));
    }
    if !success.inventory_skeleton.is_empty() {
        put(
            "inventory-skeleton",
            skeleton_to_llsd(&success.inventory_skeleton),
        );
    }
    if !success.library_skeleton.is_empty() {
        put(
            "inventory-skel-lib",
            skeleton_to_llsd(&success.library_skeleton),
        );
    }
    if !success.buddy_list.is_empty() {
        put(
            "buddy-list",
            Llsd::Array(
                success
                    .buddy_list
                    .iter()
                    .map(|buddy| {
                        Llsd::Map(
                            [
                                ("buddy_id".to_owned(), Llsd::Uuid(buddy.buddy_id)),
                                (
                                    "buddy_rights_given".to_owned(),
                                    Llsd::Integer(buddy.rights_granted),
                                ),
                                (
                                    "buddy_rights_has".to_owned(),
                                    Llsd::Integer(buddy.rights_has),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if !success.gestures.is_empty() {
        put(
            "gestures",
            Llsd::Array(
                success
                    .gestures
                    .iter()
                    .map(|gesture| {
                        Llsd::Map(
                            [
                                ("item_id".to_owned(), Llsd::Uuid(gesture.item_id.uuid())),
                                ("asset_id".to_owned(), Llsd::Uuid(gesture.asset_id)),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if !success.event_categories.is_empty() {
        put(
            "event_categories",
            categories_to_llsd(&success.event_categories),
        );
    }
    if !success.classified_categories.is_empty() {
        put(
            "classified_categories",
            categories_to_llsd(&success.classified_categories),
        );
    }
    if !success.event_notifications.is_empty() {
        put(
            "event_notifications",
            Llsd::Array(success.event_notifications.clone()),
        );
    }
    if !success.tutorial_settings.is_empty() {
        put(
            "tutorial_setting",
            Llsd::Array(
                success
                    .tutorial_settings
                    .iter()
                    .map(|setting| {
                        Llsd::Map(
                            [(
                                "tutorial_url".to_owned(),
                                Llsd::String(setting.tutorial_url.clone()),
                            )]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if let Some(flags) = &success.login_flags {
        put(
            "login-flags",
            wrap_section(
                [
                    (
                        "ever_logged_in".to_owned(),
                        Llsd::String(yn_str(flags.ever_logged_in).to_owned()),
                    ),
                    (
                        "daylight_savings".to_owned(),
                        Llsd::String(yn_str(flags.daylight_savings).to_owned()),
                    ),
                    (
                        "gendered".to_owned(),
                        Llsd::String(yn_str(flags.gendered).to_owned()),
                    ),
                    (
                        "stipend_since_login".to_owned(),
                        Llsd::String(flags.stipend_since_login.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(textures) = &success.global_textures {
        put(
            "global-textures",
            wrap_section(
                [
                    (
                        "sun_texture_id".to_owned(),
                        Llsd::Uuid(textures.sun_texture_id.uuid()),
                    ),
                    (
                        "cloud_texture_id".to_owned(),
                        Llsd::Uuid(textures.cloud_texture_id.uuid()),
                    ),
                    (
                        "moon_texture_id".to_owned(),
                        Llsd::Uuid(textures.moon_texture_id.uuid()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(ui_config) = &success.ui_config {
        put(
            "ui-config",
            wrap_section(
                [(
                    "allow_first_life".to_owned(),
                    Llsd::String(yn_str(ui_config.allow_first_life).to_owned()),
                )]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(outfit) = &success.initial_outfit {
        put(
            "initial-outfit",
            wrap_section(
                [
                    (
                        "folder_name".to_owned(),
                        Llsd::String(outfit.folder_name.clone()),
                    ),
                    ("gender".to_owned(), Llsd::String(outfit.gender.clone())),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(config) = &success.newuser_config {
        let mut section: HashMap<String, Llsd> = HashMap::new();
        if let Some(female) = &config.default_female_avatar {
            section.insert(
                "DefaultFemaleAvatar".to_owned(),
                Llsd::String(female.clone()),
            );
        }
        if let Some(male) = &config.default_male_avatar {
            section.insert("DefaultMaleAvatar".to_owned(), Llsd::String(male.clone()));
        }
        put("newuser-config", wrap_section(section));
    }
    if let Some(voice) = &success.voice_config {
        put(
            "voice-config",
            wrap_section(
                [(
                    "VoiceServerType".to_owned(),
                    Llsd::String(voice.voice_server_type.clone()),
                )]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(benefits) = &success.account_level_benefits {
        put("account_level_benefits", benefits.clone());
    }
    if let Some(packages) = &success.premium_packages {
        put("premium_packages", packages.clone());
    }
}

/// Parses the members of a successful LLSD login response into a
/// [`LoginSuccess`], the inverse of [`insert_success_members`].
fn parse_success_members(map: &HashMap<String, Llsd>) -> Result<LoginSuccess, LoginParseError> {
    let mut success = LoginSuccess::minimal(
        AgentKey::from(require_uuid(map, "agent_id")?),
        require_uuid(map, "session_id")?,
        require_uuid(map, "secure_session_id")?,
        require_circuit_code(map)?,
        require_parsed(map, "sim_ip")?,
        require_parsed(map, "sim_port")?,
        require_parsed(map, "seed_capability")?,
    );
    success.message = map.get("message").and_then(llsd_scalar_string);
    success.mfa_hash = map.get("mfa_hash").and_then(llsd_scalar_string);
    success.agent_access = map.get("agent_access").and_then(llsd_scalar_string);
    success.agent_access_max = map.get("agent_access_max").and_then(llsd_scalar_string);
    success.agent_region_access = map.get("agent_region_access").and_then(llsd_scalar_string);
    success.start_location = map.get("start_location").and_then(llsd_scalar_string);
    success.first_name = map.get("first_name").and_then(llsd_scalar_string);
    success.last_name = map.get("last_name").and_then(llsd_scalar_string);
    success.display_name = map.get("display_name").and_then(llsd_scalar_string);
    success.help_url_format = map.get("help_url_format").and_then(llsd_scalar_string);
    success.currency = map.get("currency").and_then(llsd_scalar_string);
    success.account_type = map.get("account_type").and_then(llsd_scalar_string);
    success.openid_token = map.get("openid_token").and_then(llsd_scalar_string);
    success.real_id = llsd_uuid(map, "real_id").map(AgentKey::from);
    success.home = map
        .get("home")
        .and_then(llsd_scalar_string)
        .and_then(|home| parse_home(&home));
    success.look_at = map
        .get("look_at")
        .and_then(llsd_scalar_string)
        .and_then(|look_at| parse_direction(&look_at));
    success.region_x = llsd_u32(map, "region_x");
    success.region_y = llsd_u32(map, "region_y");
    success.region_size_x = llsd_u32(map, "region_size_x");
    success.region_size_y = llsd_u32(map, "region_size_y");
    success.http_port = llsd_i32(map, "http_port").and_then(|port| u16::try_from(port).ok());
    success.seconds_since_epoch = map.get("seconds_since_epoch").and_then(llsd_to_i64);
    success.max_agent_groups =
        llsd_u32(map, "max-agent-groups").or_else(|| llsd_u32(map, "max_groups"));
    success.udp_blacklist = map
        .get("udp_blacklist")
        .and_then(llsd_scalar_string)
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    success.classified_fee = llsd_i32(map, "classified_fee");
    success.directory_fee = llsd_i32(map, "directory_fee");
    success.agent_appearance_service = llsd_url(map, "agent_appearance_service");
    success.map_server_url = llsd_url(map, "map-server-url");
    success.openid_url = llsd_url(map, "openid_url");
    success.web_profile_url = llsd_url(map, "web_profile_url");
    success.profile_server_url = llsd_url(map, "profile-server-url");
    success.search_url = llsd_url(map, "search");
    success.destination_guide_url = llsd_url(map, "destination_guide_url");
    success.avatar_picker_url = llsd_url(map, "avatar_picker_url");
    success.inventory_root =
        section_uuid(map, "inventory-root", "folder_id").map(InventoryFolderKey::from);
    success.library_root =
        section_uuid(map, "inventory-lib-root", "folder_id").map(InventoryFolderKey::from);
    success.library_owner =
        section_uuid(map, "inventory-lib-owner", "agent_id").map(AgentKey::from);
    success.inventory_skeleton = skeleton_from_llsd(map.get("inventory-skeleton"));
    success.library_skeleton = skeleton_from_llsd(map.get("inventory-skel-lib"));
    success.buddy_list = array_maps(map.get("buddy-list"))
        .filter_map(|entry| {
            Some(BuddyListEntry {
                buddy_id: entry.get("buddy_id").and_then(llsd_to_uuid)?,
                rights_granted: entry
                    .get("buddy_rights_given")
                    .and_then(llsd_to_i32)
                    .unwrap_or(0),
                rights_has: entry
                    .get("buddy_rights_has")
                    .and_then(llsd_to_i32)
                    .unwrap_or(0),
            })
        })
        .collect();
    success.gestures = array_maps(map.get("gestures"))
        .filter_map(|entry| {
            Some(GestureEntry {
                item_id: InventoryKey::from(entry.get("item_id").and_then(llsd_to_uuid)?),
                asset_id: entry.get("asset_id").and_then(llsd_to_uuid)?,
            })
        })
        .collect();
    success.event_categories = categories_from_llsd(map.get("event_categories"));
    success.classified_categories = categories_from_llsd(map.get("classified_categories"));
    success.event_notifications = map
        .get("event_notifications")
        .and_then(Llsd::as_array)
        .map(<[Llsd]>::to_vec)
        .unwrap_or_default();
    success.tutorial_settings = array_maps(map.get("tutorial_setting"))
        .filter_map(|entry| {
            Some(TutorialSetting {
                tutorial_url: entry.get("tutorial_url").and_then(llsd_scalar_string)?,
            })
        })
        .collect();
    success.login_flags = section_map(map, "login-flags").map(|section| LoginFlags {
        ever_logged_in: section_yn(section, "ever_logged_in"),
        daylight_savings: section_yn(section, "daylight_savings"),
        gendered: section_yn(section, "gendered"),
        stipend_since_login: section
            .get("stipend_since_login")
            .and_then(llsd_scalar_string)
            .unwrap_or_default(),
    });
    success.global_textures = section_map(map, "global-textures").and_then(|section| {
        Some(GlobalTextures {
            sun_texture_id: TextureKey::from(section.get("sun_texture_id").and_then(llsd_to_uuid)?),
            cloud_texture_id: TextureKey::from(
                section.get("cloud_texture_id").and_then(llsd_to_uuid)?,
            ),
            moon_texture_id: TextureKey::from(
                section.get("moon_texture_id").and_then(llsd_to_uuid)?,
            ),
        })
    });
    success.ui_config = section_map(map, "ui-config").map(|section| UiConfig {
        allow_first_life: section_yn(section, "allow_first_life"),
    });
    success.initial_outfit = section_map(map, "initial-outfit").map(|section| InitialOutfit {
        folder_name: section
            .get("folder_name")
            .and_then(llsd_scalar_string)
            .unwrap_or_default(),
        gender: section
            .get("gender")
            .and_then(llsd_scalar_string)
            .unwrap_or_default(),
    });
    success.newuser_config = section_map(map, "newuser-config").map(|section| NewUserConfig {
        default_female_avatar: section
            .get("DefaultFemaleAvatar")
            .and_then(llsd_scalar_string),
        default_male_avatar: section
            .get("DefaultMaleAvatar")
            .and_then(llsd_scalar_string),
    });
    success.voice_config = section_map(map, "voice-config").and_then(|section| {
        Some(VoiceConfig {
            voice_server_type: section
                .get("VoiceServerType")
                .and_then(llsd_scalar_string)?,
        })
    });
    success.account_level_benefits = map.get("account_level_benefits").cloned();
    success.premium_packages = map.get("premium_packages").cloned();
    Ok(success)
}

// ---------------------------------------------------------------------------
// Representation helpers
// ---------------------------------------------------------------------------

/// Encodes a circuit code the way OpenSim's LLSD serializer does: as a native
/// integer with the `u32` bit pattern reinterpreted (values above `i32::MAX`
/// wrap negative, exactly like its C# `(int)` cast).
const fn circuit_code_to_llsd(code: CircuitCode) -> Llsd {
    Llsd::Integer(i32::from_ne_bytes(code.get().to_ne_bytes()))
}

/// Encodes a `u32` as an LLSD integer when it fits, else as a real — region
/// coordinates and group limits are far below `i32::MAX` in practice, but a
/// hostile value must not panic or truncate.
fn u32_to_llsd(value: u32) -> Llsd {
    i32::try_from(value).map_or_else(|_out_of_range| Llsd::Real(f64::from(value)), Llsd::Integer)
}

/// Converts an out-of-`i32`-range `seconds_since_epoch` to the real-number
/// fallback representation.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "epoch seconds are far below 2^52, where f64 is exact"
)]
const fn seconds_as_real(seconds: i64) -> f64 {
    seconds as f64
}

/// Wraps a section map in the one-element array OpenSim's `WrapOSDMap` uses
/// for the config-like login response sections.
fn wrap_section(section: HashMap<String, Llsd>) -> Llsd {
    Llsd::Array(vec![Llsd::Map(section)])
}

/// An id-carrying one-struct section (`inventory-root`, `inventory-lib-owner`).
fn id_section(field: &str, id: Uuid) -> Llsd {
    wrap_section([(field.to_owned(), Llsd::Uuid(id))].into_iter().collect())
}

/// Encodes an inventory folder skeleton as its LLSD array of folder maps.
fn skeleton_to_llsd(folders: &[SkeletonFolder]) -> Llsd {
    Llsd::Array(
        folders
            .iter()
            .map(|folder| {
                Llsd::Map(
                    [
                        ("folder_id".to_owned(), Llsd::Uuid(folder.folder_id.uuid())),
                        ("parent_id".to_owned(), Llsd::Uuid(folder.parent_id.uuid())),
                        ("name".to_owned(), Llsd::String(folder.name.clone())),
                        (
                            "type_default".to_owned(),
                            Llsd::Integer(i32::from(folder.type_default)),
                        ),
                        ("version".to_owned(), Llsd::Integer(folder.version)),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

/// Decodes an inventory folder skeleton from its LLSD array, skipping
/// malformed entries.
fn skeleton_from_llsd(value: Option<&Llsd>) -> Vec<SkeletonFolder> {
    array_maps(value)
        .filter_map(|entry| {
            Some(SkeletonFolder {
                folder_id: InventoryFolderKey::from(entry.get("folder_id").and_then(llsd_to_uuid)?),
                parent_id: InventoryFolderKey::from(
                    entry
                        .get("parent_id")
                        .and_then(llsd_to_uuid)
                        .unwrap_or_else(Uuid::nil),
                ),
                name: entry
                    .get("name")
                    .and_then(llsd_scalar_string)
                    .unwrap_or_default(),
                type_default: entry
                    .get("type_default")
                    .and_then(llsd_to_i32)
                    .and_then(|t| i8::try_from(t).ok())
                    .unwrap_or(-1),
                version: entry.get("version").and_then(llsd_to_i32).unwrap_or(0),
            })
        })
        .collect()
}

/// Encodes an event/classified category list as its LLSD array.
fn categories_to_llsd(categories: &[LoginCategory]) -> Llsd {
    Llsd::Array(
        categories
            .iter()
            .map(|category| {
                Llsd::Map(
                    [
                        (
                            "category_name".to_owned(),
                            Llsd::String(category.category_name.clone()),
                        ),
                        (
                            "category_id".to_owned(),
                            Llsd::Integer(category.category_id),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

/// Decodes an event/classified category list, skipping malformed entries.
fn categories_from_llsd(value: Option<&Llsd>) -> Vec<LoginCategory> {
    array_maps(value)
        .filter_map(|entry| {
            Some(LoginCategory {
                category_id: entry.get("category_id").and_then(llsd_to_i32)?,
                category_name: entry.get("category_name").and_then(llsd_scalar_string)?,
            })
        })
        .collect()
}

/// Iterates the maps inside an LLSD array value.
fn array_maps(value: Option<&Llsd>) -> impl Iterator<Item = &HashMap<String, Llsd>> {
    value
        .and_then(Llsd::as_array)
        .into_iter()
        .flatten()
        .filter_map(Llsd::as_map)
}

/// Returns the first map of the named one-element-array section
/// (the `WrapOSDMap` shape).
fn section_map<'a>(map: &'a HashMap<String, Llsd>, key: &str) -> Option<&'a HashMap<String, Llsd>> {
    array_maps(map.get(key)).next()
}

/// Reads a `"Y"`/`"N"` flag member of a section (absence reads as no).
fn section_yn(section: &HashMap<String, Llsd>, key: &str) -> bool {
    section
        .get(key)
        .and_then(llsd_scalar_string)
        .is_some_and(|value| yn_wire_flag(&value))
}

/// Extracts the id field of an id-carrying one-struct section.
fn section_uuid(map: &HashMap<String, Llsd>, key: &str, field: &str) -> Option<Uuid> {
    section_map(map, key)?.get(field).and_then(llsd_to_uuid)
}

/// Renders any scalar LLSD value as a string — the leniency mirror of the
/// XML-RPC side, where every scalar arrives as text. Containers yield `None`.
fn llsd_scalar_string(value: &Llsd) -> Option<String> {
    match value {
        Llsd::String(text) | Llsd::Uri(text) | Llsd::Date(text) => Some(text.clone()),
        Llsd::Uuid(id) => Some(id.to_string()),
        Llsd::Integer(number) => Some(number.to_string()),
        Llsd::Real(number) => Some(number.to_string()),
        Llsd::Boolean(flag) => Some(if *flag { "true" } else { "false" }.to_owned()),
        Llsd::Undef | Llsd::Binary(_) | Llsd::Array(_) | Llsd::Map(_) => None,
    }
}

/// Reads the named member as a string, defaulting to empty when absent (the
/// request-side convention shared with the XML-RPC parser).
fn llsd_string(map: &HashMap<String, Llsd>, key: &str) -> String {
    map.get(key)
        .and_then(llsd_scalar_string)
        .unwrap_or_default()
}

/// Converts a scalar LLSD value to an `i32` (native integer, or a numeric
/// string).
fn llsd_to_i32(value: &Llsd) -> Option<i32> {
    match value {
        Llsd::Integer(number) => Some(*number),
        Llsd::String(text) => text.trim().parse().ok(),
        _other => None,
    }
}

/// Converts a scalar LLSD value to an `i64` (native integer, real, or a
/// numeric string) — for `seconds_since_epoch`, whose value can exceed `i32`.
fn llsd_to_i64(value: &Llsd) -> Option<i64> {
    match value {
        Llsd::Integer(number) => Some(i64::from(*number)),
        Llsd::Real(number) => real_to_i64(*number),
        Llsd::String(text) => text.trim().parse().ok(),
        _other => None,
    }
}

/// Narrows a real to an integer when it is one (the real-number fallback
/// representation of large integers).
#[expect(
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "guarded: the value is checked to be integral and within i64"
)]
fn real_to_i64(value: f64) -> Option<i64> {
    ((value - value.trunc()).abs() < f64::EPSILON
        && (-9_007_199_254_740_992.0..=9_007_199_254_740_992.0).contains(&value))
    .then_some(value as i64)
}

/// Converts a scalar LLSD value to a UUID (native, or a UUID string).
fn llsd_to_uuid(value: &Llsd) -> Option<Uuid> {
    match value {
        Llsd::Uuid(id) => Some(*id),
        Llsd::String(text) => Uuid::parse_str(text.trim()).ok(),
        _other => None,
    }
}

/// Reads the named member as an `i32`.
fn llsd_i32(map: &HashMap<String, Llsd>, key: &str) -> Option<i32> {
    map.get(key).and_then(llsd_to_i32)
}

/// Reads the named member as a `u32`, reinterpreting a negative native
/// integer's bit pattern (the inverse of the OpenSim-style `(int)` wrap).
fn llsd_u32(map: &HashMap<String, Llsd>, key: &str) -> Option<u32> {
    match map.get(key)? {
        Llsd::Integer(number) => Some(u32::from_ne_bytes(number.to_ne_bytes())),
        Llsd::Real(number) => real_to_i64(*number).and_then(|n| u32::try_from(n).ok()),
        Llsd::String(text) => text.trim().parse().ok(),
        _other => None,
    }
}

/// Reads the named member as a UUID.
fn llsd_uuid(map: &HashMap<String, Llsd>, key: &str) -> Option<Uuid> {
    map.get(key).and_then(llsd_to_uuid)
}

/// Reads the named member as a URL, ignoring unparsable values (as the
/// XML-RPC parser does).
fn llsd_url(map: &HashMap<String, Llsd>, key: &str) -> Option<url::Url> {
    map.get(key)
        .and_then(llsd_scalar_string)
        .and_then(|text| url::Url::parse(text.trim()).ok())
}

/// Reads the named member as a boolean (native, `1`, or `"true"`).
fn llsd_bool(map: &HashMap<String, Llsd>, key: &str) -> bool {
    match map.get(key) {
        Some(Llsd::Boolean(flag)) => *flag,
        Some(Llsd::Integer(number)) => *number != 0,
        Some(Llsd::String(text)) => matches!(text.trim(), "1" | "true"),
        _other => false,
    }
}

/// Returns the named member as a required UUID or the corresponding
/// [`LoginParseError`].
fn require_uuid(map: &HashMap<String, Llsd>, name: &'static str) -> Result<Uuid, LoginParseError> {
    let value = map
        .get(name)
        .ok_or(LoginParseError::MissingField { name })?;
    llsd_to_uuid(value).ok_or_else(|| LoginParseError::InvalidField {
        name,
        value: llsd_scalar_string(value).unwrap_or_default(),
    })
}

/// Returns the required circuit code, un-wrapping the OpenSim-style `(int)`
/// bit reinterpretation.
fn require_circuit_code(map: &HashMap<String, Llsd>) -> Result<CircuitCode, LoginParseError> {
    let name = "circuit_code";
    let value = map
        .get(name)
        .ok_or(LoginParseError::MissingField { name })?;
    match value {
        Llsd::Integer(number) => Ok(CircuitCode(u32::from_ne_bytes(number.to_ne_bytes()))),
        other => llsd_scalar_string(other)
            .and_then(|text| text.trim().parse().ok())
            .map(CircuitCode)
            .ok_or_else(|| LoginParseError::InvalidField {
                name,
                value: llsd_scalar_string(other).unwrap_or_default(),
            }),
    }
}

/// Returns the named member parsed via [`std::str::FromStr`] from its scalar
/// string form, or the corresponding [`LoginParseError`].
fn require_parsed<T>(map: &HashMap<String, Llsd>, name: &'static str) -> Result<T, LoginParseError>
where
    T: std::str::FromStr,
{
    let value = map
        .get(name)
        .ok_or(LoginParseError::MissingField { name })?;
    let text = llsd_scalar_string(value).ok_or_else(|| LoginParseError::InvalidField {
        name,
        value: String::new(),
    })?;
    text.trim()
        .parse::<T>()
        .map_err(|_ignored| LoginParseError::InvalidField { name, value: text })
}
