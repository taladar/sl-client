//! The `POST /` login endpoint: the HTTP transport around
//! [`sl_wire::LoginServer`], serving both codecs at the same URL — XML-RPC
//! for `text/xml` requests, LLSD for `application/llsd+xml` — the way real
//! grids do.

use std::sync::Arc;

use sl_wire::{
    LoginFailure, LoginResponse, LoginServer, ParsedLoginRequest, build_login_response,
    build_login_response_llsd, parse_login_request, parse_login_request_llsd,
};

use crate::runtime::{GridCore, LoginNotice};

/// A finished login answer for the HTTP layer to send.
pub(crate) struct LoginHttpAnswer {
    /// The HTTP status (200 even for protocol-level failures, per XML-RPC;
    /// 400 only when the body cannot be parsed at all).
    pub(crate) status: u16,
    /// The response content type (mirrors the request codec).
    pub(crate) content_type: &'static str,
    /// The serialized response document.
    pub(crate) body: String,
}

/// The XML-RPC login content type.
const XML_RPC_CONTENT_TYPE: &str = "text/xml";
/// The LLSD login content type.
const LLSD_CONTENT_TYPE: &str = "application/llsd+xml";

/// Serves one `POST /` login request body.
pub(crate) async fn handle_login(
    core: &Arc<GridCore>,
    content_type: &str,
    body: &[u8],
) -> LoginHttpAnswer {
    let Ok(text) = std::str::from_utf8(body) else {
        return LoginHttpAnswer {
            status: 400,
            content_type: XML_RPC_CONTENT_TYPE,
            body: String::new(),
        };
    };
    // Real grids serve both codecs at one URL, keyed on the request's
    // Content-Type; parameters (e.g. `; charset=utf-8`) are tolerated.
    if content_type
        .split(';')
        .next()
        .is_some_and(|main| main.trim().eq_ignore_ascii_case(LLSD_CONTENT_TYPE))
    {
        match parse_login_request_llsd(text) {
            Ok(parsed) => {
                let response = respond(core, &parsed).await;
                LoginHttpAnswer {
                    status: 200,
                    content_type: LLSD_CONTENT_TYPE,
                    body: build_login_response_llsd(&response),
                }
            }
            Err(error) => {
                tracing::debug!("unparsable LLSD login request: {error}");
                LoginHttpAnswer {
                    status: 400,
                    content_type: LLSD_CONTENT_TYPE,
                    body: String::new(),
                }
            }
        }
    } else {
        match parse_login_request(text) {
            Ok(parsed) => {
                let response = respond(core, &parsed).await;
                LoginHttpAnswer {
                    status: 200,
                    content_type: XML_RPC_CONTENT_TYPE,
                    body: build_login_response(&response),
                }
            }
            Err(error) => {
                tracing::debug!("unparsable XML-RPC login request: {error}");
                LoginHttpAnswer {
                    status: 400,
                    content_type: XML_RPC_CONTENT_TYPE,
                    body: String::new(),
                }
            }
        }
    }
}

/// Maps a parsed login request to the [`LoginResponse`], creating and
/// activating a session when the login server lets it through.
async fn respond(core: &Arc<GridCore>, parsed: &ParsedLoginRequest) -> LoginResponse {
    // An unknown account answers exactly like a wrong password, so the
    // endpoint does not leak which accounts exist.
    let Some(account) = core.accounts.iter().find(|account| {
        account.config.first_name == parsed.first_name
            && account.config.last_name == parsed.last_name
    }) else {
        return LoginResponse::Failure(LoginFailure::new(
            LoginServer::BAD_CREDENTIALS_REASON,
            "Could not authenticate your avatar. Please check your username and password.",
        ));
    };
    let account = account.clone();
    let Some(region) = core.start_region(&account) else {
        return LoginResponse::Failure(LoginFailure::new(
            "key",
            "The account's start region is not part of this grid.",
        ));
    };
    let (prepared, mut success) = match core.prepare_session(&account, region).await {
        Ok(pair) => pair,
        Err(error) => {
            tracing::error!("preparing a session failed: {error}");
            return LoginResponse::Failure(LoginFailure::new(
                "key",
                "The grid failed to create a session.",
            ));
        }
    };
    if core.honor_options {
        success.filter_options(&parsed.options);
    }
    let response = LoginServer::respond(parsed, &account.credential, &core.gates, success);
    if matches!(response, LoginResponse::Success(_)) {
        core.activate_session(&prepared).await;
        let notice = LoginNotice {
            session_seq: prepared.seq,
            agent_id: account.agent_id,
            first_name: account.config.first_name.clone(),
            last_name: account.config.last_name.clone(),
            region_name: prepared.region_name.clone(),
        };
        tracing::info!(
            "login: {} {} into {} (session {})",
            notice.first_name,
            notice.last_name,
            notice.region_name,
            notice.session_seq
        );
        // Only lagging subscribers error; login proceeds regardless.
        drop(core.logins_tx.send(notice));
    }
    response
}
