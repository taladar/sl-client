//! The `POST /` login endpoint: the HTTP transport around
//! [`sl_wire::LoginServer`], serving both codecs at the same URL — XML-RPC
//! for `text/xml` requests, LLSD for `application/llsd+xml` — the way real
//! grids do.

use std::sync::Arc;

use sl_wire::{
    LoginFailure, LoginResponse, LoginServer, ParsedLoginRequest, build_login_response,
    build_login_response_llsd, parse_login_request, parse_login_request_llsd,
};

use crate::http_answer::HttpAnswer;
use crate::imitates::SecondLogin;
use crate::runtime::{GridCore, LoginNotice};

/// The XML-RPC login content type.
const XML_RPC_CONTENT_TYPE: &str = "text/xml";
/// The LLSD login content type.
const LLSD_CONTENT_TYPE: &str = "application/llsd+xml";

/// Serves one `POST /` login request body. The status is 200 even for
/// protocol-level failures, per XML-RPC; 400 only when the body cannot be
/// parsed at all.
pub(crate) async fn handle_login(
    core: &Arc<GridCore>,
    content_type: &str,
    body: &[u8],
) -> HttpAnswer {
    let Ok(text) = std::str::from_utf8(body) else {
        return HttpAnswer::with_status(400, XML_RPC_CONTENT_TYPE, "");
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
                HttpAnswer::ok(LLSD_CONTENT_TYPE, build_login_response_llsd(&response))
            }
            Err(error) => {
                tracing::debug!("unparsable LLSD login request: {error}");
                HttpAnswer::with_status(400, LLSD_CONTENT_TYPE, "")
            }
        }
    } else {
        match parse_login_request(text) {
            Ok(parsed) => {
                let response = respond(core, &parsed).await;
                HttpAnswer::ok(XML_RPC_CONTENT_TYPE, build_login_response(&response))
            }
            Err(error) => {
                tracing::debug!("unparsable XML-RPC login request: {error}");
                HttpAnswer::with_status(400, XML_RPC_CONTENT_TYPE, "")
            }
        }
    }
}

/// Maps a parsed login request to the [`LoginResponse`], creating and
/// activating a session when the login server lets it through.
async fn respond(core: &Arc<GridCore>, parsed: &ParsedLoginRequest) -> LoginResponse {
    // An unknown account answers exactly like a wrong password, as on both
    // live grids, so the endpoint does not leak which accounts exist.
    let Some(account) = core.accounts.iter().find(|account| {
        account.config.first_name == parsed.first_name
            && account.config.last_name == parsed.last_name
    }) else {
        return LoginResponse::Failure(bad_credentials(core));
    };
    let account = account.clone();
    // The password, the gates and MFA decide before anything is minted: a
    // wrong-password POST must not bind a socket, consume a session sequence
    // number and deep clone the scenario only to throw all of it away.
    match LoginServer::rejection(parsed, &account.credential, &core.gates) {
        // The decision is the login server's; the words are the grid's.
        Some(LoginResponse::Failure(failure))
            if failure.reason == LoginServer::BAD_CREDENTIALS_REASON =>
        {
            return LoginResponse::Failure(bad_credentials(core));
        }
        Some(rejection) => return rejection,
        None => {}
    }
    // The ghost: a presence left behind by a session that did not log out
    // cleanly. It is checked here rather than through `LoginGates` because it
    // is not a policy but a piece of state the refusal itself consumes — the
    // login service evicts the stale presence on its way to reporting it, so
    // the next attempt succeeds. Only a login that would otherwise have gone
    // through meets it, so a wrong password does not spend the eviction.
    if core
        .stale_presence
        .swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        tracing::info!(
            "login: {} {} refused as already-logged-in; the stale presence is now evicted",
            account.config.first_name,
            account.config.last_name,
        );
        return LoginResponse::Failure(LoginFailure::new(
            LoginServer::PRESENCE_REASON,
            core.login_refusals.already_logged_in_message,
        ));
    }
    // The avatar is in world already. Both live grids end that session; what
    // they tell the new login differs.
    if kick_live_sessions(core, account.agent_id).await
        && core.login_refusals.second_login == SecondLogin::Refused
    {
        tracing::info!(
            "login: {} {} refused as already-logged-in; the session it had is kicked",
            account.config.first_name,
            account.config.last_name,
        );
        return LoginResponse::Failure(LoginFailure::new(
            LoginServer::PRESENCE_REASON,
            core.login_refusals.already_logged_in_message,
        ));
    }
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
    success.buddy_list = core.buddies_of(account.agent_id);
    // Refuse rather than answer `login: true` without a field the reference
    // viewer requires. Such a response is accepted by the login machinery,
    // opens the circuit, and only then fails the viewer's own success check —
    // reported to the user as a bare "Login failed." with the cause a whole
    // startup state behind it. A grid that refuses says which field, here.
    //
    // Checked on the response the grid *built*, before `filter_options`
    // trims it: `inventory-root` is both mandatory to the viewer and
    // omittable when the client did not ask for it, and a client that did not
    // ask is not one this can be wrong for.
    let missing = success.missing_required_fields();
    if !missing.is_empty() {
        let missing = missing.join(", ");
        tracing::error!(
            "login: {} {} refused: the grid built a success response without {missing}, \
             which the reference viewer requires",
            account.config.first_name,
            account.config.last_name,
        );
        return LoginResponse::Failure(LoginFailure::new(
            "key",
            format!("The grid built a login response without {missing}."),
        ));
    }
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

/// The refusal for a wrong password or an unknown name, as the imitated grid
/// words it.
fn bad_credentials(core: &GridCore) -> LoginFailure {
    let refusals = core.login_refusals;
    let mut failure = LoginFailure::new(
        LoginServer::BAD_CREDENTIALS_REASON,
        refusals.bad_credentials_message,
    );
    failure.message_id = refusals.bad_credentials_message_id.map(str::to_owned);
    if refusals.stamps_error_code {
        // The shape of Second Life's incident ids: `1-`, eight hex digits,
        // twenty-four more, different on every response.
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let (head, tail) = unique.split_at(8);
        failure.error_code = Some(format!("1-{head}-{tail}"));
    }
    failure
}

/// Ends every session `agent_id` has, root and child, and reports whether the
/// avatar was **in world** — whether one of them was its root.
///
/// The children alone do not put an avatar in world: a root that logged out a
/// moment ago can leave a neighbour's session behind it for a while, and that
/// must not refuse the login that follows. They are ended all the same, since
/// the new login is about to be given neighbours of its own.
async fn kick_live_sessions(core: &Arc<GridCore>, agent_id: sl_proto::AgentKey) -> bool {
    let in_world = core.root_session_of(agent_id).await.is_some();
    let kick = sl_proto::Kick {
        agent: agent_id,
        reason: core.login_refusals.second_login_kick_reason.to_owned(),
    };
    for shared in core.sessions_of(agent_id).await {
        shared
            .with_sim(|sim| {
                // A session nobody opened a circuit to — a neighbour the
                // client has not reached yet — cannot be told, and ends all
                // the same.
                if let Err(error) = sim.kick(&kick, shared.now()) {
                    tracing::debug!("a session with no circuit to kick is abandoned: {error}");
                    sim.abandon();
                }
            })
            .await;
    }
    in_world
}
