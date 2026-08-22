//! Second Life website auto-login (`viewer-web-openid-auth`).
//!
//! The Second Life login (XML-RPC) response carries an OpenID handshake pair —
//! `openid_url` and `openid_token` — that Linden viewers use to sign the grid
//! account into the SL websites, so the in-viewer browser opens
//! `my.secondlife.com`, the marketplace and search **already authenticated**.
//! OpenSim sends neither field, so this whole path stays dormant off Second
//! Life.
//!
//! The flow mirrors the reference viewer (`LLViewerMedia::openIDSetup`):
//!
//! 1. At login, POST the raw `openid_token` to `openid_url`
//!    (`Content-Type: application/x-www-form-urlencoded`) on a worker thread
//!    and keep the reply's `Set-Cookie` — the grid's OpenID session cookie.
//! 2. Inject that cookie into the embedded browser's **shared** (trusted-UI)
//!    request context, scoped to the OpenID host. CEF refuses a cookie whose
//!    URL and domain differ (reference MAINT-5711), so both come from the
//!    OpenID host; the SL sites then single-sign-on against that host.
//! 3. The isolated in-world media contexts are never touched — a griefer's
//!    media prim must not see the grid session.
//!
//! The injected cookie is a **session** cookie (no expiry), and CEF is left at
//! its default `persist_session_cookies = false`, so it should live only in
//! memory. The session is cleared at two points so the auth data does not
//! linger on disk:
//!
//! - On a **clean exit** (`clear_on_exit`) — the common case, so the store is
//!   empty after a normal logout / quit.
//! - Before **each login injects** its cookie (`apply_web_auth`) — so a login
//!   is always authoritative: it self-heals a session orphaned by a crash (which
//!   writes no `AppExit`, so the exit hook never ran) and covers a future
//!   in-process avatar switch.
//!
//! `--no-web-auth` (or a disabled web engine) leaves the browser surfaces
//! anonymous.
//!
//! Reference (read-only): `llviewermedia.cpp` (`openIDSetup[Coro]`,
//! `openIDCookieResponse`, `setOpenIDCookie`, `getOpenIDCookie`,
//! `parseRawCookie`), `llstartup.cpp` (login response fields).

use bevy::prelude::*;
use crossbeam_channel::{Receiver, bounded};
use sl_cef::SharedCookie;
use sl_client_bevy::SlIdentity;

use crate::media_engine::{MediaEngine, MediaEngineSystems};

/// How the login-time OpenID token POST is progressing.
#[derive(Debug)]
enum WebAuthPhase {
    /// Waiting for the login response's OpenID fields (or disabled).
    Idle,
    /// The token POST is running on a worker thread; the channel delivers the
    /// cookies to inject (or an error string).
    InFlight(Receiver<Result<Vec<SharedCookie>, String>>),
    /// The cookie was injected into the shared context (a session is primed).
    Injected,
    /// The POST failed; the browser surfaces stay anonymous (not retried).
    Failed,
    /// The login succeeded but carried no OpenID fields (OpenSim, or a grid
    /// that does not offer website SSO): nothing to do, checked once.
    NotOffered,
}

/// The web auto-login state (a resource, one per session).
#[derive(Debug, Resource)]
pub struct WebAuth {
    /// Whether the auto-login runs at all (`--no-web-auth` / web engine off
    /// clear it).
    enabled: bool,
    /// The POST progress.
    phase: WebAuthPhase,
}

/// The Second Life website auto-login plugin.
#[derive(Debug)]
pub struct WebAuthPlugin {
    /// Whether the auto-login is enabled this session.
    pub enabled: bool,
}

impl Plugin for WebAuthPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WebAuth {
            enabled: self.enabled,
            phase: WebAuthPhase::Idle,
        })
        .add_systems(
            Update,
            (start_web_auth, apply_web_auth)
                .chain()
                .after(MediaEngineSystems::Pump),
        )
        .add_systems(Last, clear_on_exit);
    }
}

/// Once the login response's OpenID fields land in [`SlIdentity`], spawn the
/// token POST on a worker thread (only once, and only on Second Life).
fn start_web_auth(identity: Res<SlIdentity>, mut auth: ResMut<WebAuth>) {
    if !auth.enabled || !matches!(auth.phase, WebAuthPhase::Idle) {
        return;
    }
    let (Some(url), Some(token)) = (identity.openid_url.clone(), identity.openid_token.clone())
    else {
        // Once the login has landed (agent id known) without OpenID fields —
        // OpenSim, or a grid that does not offer website SSO — stop checking.
        if identity.agent_id.is_some() {
            info!(
                "web auto-login: login response carried no OpenID token; web surfaces browse \
                 anonymously"
            );
            auth.phase = WebAuthPhase::NotOffered;
        }
        return;
    };
    info!(
        "web auto-login: posting the OpenID token ({} bytes) to {}",
        token.len(),
        url
    );
    let (sender, receiver) = bounded(1);
    // A one-shot blocking HTTP POST off the main thread (the reqwest::blocking
    // idiom the driver uses for login), its result polled by `apply_web_auth`.
    let spawned = std::thread::Builder::new()
        .name("web-openid-auth".to_owned())
        .spawn(move || {
            let result = fetch_openid_cookies(&url, &token);
            // The receiver may be gone if the app is exiting; ignore that.
            let _sent = sender.send(result);
        });
    auth.phase = match spawned {
        Ok(_handle) => WebAuthPhase::InFlight(receiver),
        Err(error) => {
            warn!("could not spawn the web auto-login worker: {error}");
            WebAuthPhase::Failed
        }
    };
}

/// Poll the worker; when the cookies arrive, inject them into the shared
/// browser context so the web surfaces open logged in.
fn apply_web_auth(mut auth: ResMut<WebAuth>, mut engine: NonSendMut<MediaEngine>) {
    let WebAuthPhase::InFlight(receiver) = &auth.phase else {
        return;
    };
    let next = match receiver.try_recv() {
        Ok(Ok(cookies)) => {
            if cookies.is_empty() {
                warn!("web auto-login: the OpenID POST returned no Set-Cookie; not signed in");
                WebAuthPhase::Failed
            } else {
                // Clear first so this login is authoritative: a session
                // orphaned by an earlier crash, or a previous avatar's cookie,
                // is wiped before the fresh one lands (the delete is queued
                // ahead of the sets on CEF's cookie thread).
                if let Err(error) = engine.clear_shared_cookies() {
                    warn!("web auto-login: clearing stale shared cookies failed: {error}");
                }
                let mut injected = false;
                for cookie in &cookies {
                    match engine.set_shared_cookie(cookie) {
                        Ok(()) => injected = true,
                        Err(error) => warn!("web auto-login: cookie injection failed: {error}"),
                    }
                }
                if injected {
                    info!(
                        "web auto-login: injected {} session cookie(s); web surfaces are signed in",
                        cookies.len()
                    );
                    WebAuthPhase::Injected
                } else {
                    WebAuthPhase::Failed
                }
            }
        }
        Ok(Err(error)) => {
            warn!("web auto-login failed: {error}");
            WebAuthPhase::Failed
        }
        // Still in flight.
        Err(crossbeam_channel::TryRecvError::Empty) => return,
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            warn!("web auto-login worker vanished before delivering a result");
            WebAuthPhase::Failed
        }
    };
    auth.phase = next;
}

/// On a clean app exit, clear the shared context's cookies if a session was
/// primed, so the grid web session does not linger on disk after a normal
/// logout / quit. (A crash writes no [`AppExit`], so this is skipped then — the
/// next login's clear-before-inject covers that case instead.)
fn clear_on_exit(
    mut exits: MessageReader<AppExit>,
    auth: Res<WebAuth>,
    mut engine: NonSendMut<MediaEngine>,
) {
    if exits.read().next().is_none() {
        return;
    }
    if matches!(auth.phase, WebAuthPhase::Injected)
        && let Err(error) = engine.clear_shared_cookies()
    {
        warn!("web auto-login: clearing the shared cookie store on exit failed: {error}");
    }
}

/// Perform the OpenID token POST and turn the reply's `Set-Cookie` headers into
/// the cookies to inject. Blocking; runs on the worker thread.
fn fetch_openid_cookies(url: &url::Url, token: &str) -> Result<Vec<SharedCookie>, String> {
    // No redirect following: the session cookie rides the immediate response,
    // exactly as the reference reads it (it never sets follow-redirects on the
    // OpenID POST).
    let client = sl_client_bevy::http_proxy::blocking_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("building the HTTP client: {error}"))?;
    let response = client
        .post(url.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(reqwest::header::ACCEPT, "*/*")
        .body(token.to_owned())
        .send()
        .map_err(|error| format!("posting the OpenID token: {error}"))?;

    let status = response.status();
    let set_cookie_count = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .count();
    info!(
        "web auto-login: OpenID POST to {url} returned {status} with {set_cookie_count} \
         Set-Cookie header(s)"
    );

    // Both the cookie URL and its domain come from the OpenID host so CEF
    // accepts the set (MAINT-5711); the SL sites single-sign-on against it.
    let host = url
        .host_str()
        .ok_or_else(|| format!("the OpenID URL {url} has no host"))?
        .to_owned();
    let cookie_url = format!("{}://{host}", url.scheme());

    let mut cookies = Vec::new();
    for header in response.headers().get_all(reqwest::header::SET_COOKIE) {
        let Ok(raw) = header.to_str() else {
            continue;
        };
        if let Some((name, value)) = parse_set_cookie(raw) {
            cookies.push(SharedCookie {
                url: cookie_url.clone(),
                name,
                value,
                domain: host.clone(),
                path: String::from("/"),
                // The reference hard-codes Secure + HttpOnly for the grid
                // session cookie.
                secure: true,
                http_only: true,
            });
        }
    }
    Ok(cookies)
}

/// Extract the `name=value` pair from a raw `Set-Cookie` header value: the name
/// is up to the first `=`, the value is from there to the first `;` (dropping
/// the attributes), matching the reference `parseRawCookie`. Returns `None` for
/// a header with no `=` or an empty name.
fn parse_set_cookie(raw: &str) -> Option<(String, String)> {
    let (name, rest) = raw.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value = rest.split(';').next().unwrap_or(rest).trim();
    Some((name.to_owned(), value.to_owned()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::parse_set_cookie;

    #[test]
    fn parses_name_and_value_dropping_attributes() {
        assert_eq!(
            parse_set_cookie(
                "agni_sl_session_id=abc123; Domain=id.secondlife.com; Path=/; Secure; HttpOnly"
            ),
            Some((String::from("agni_sl_session_id"), String::from("abc123")))
        );
    }

    #[test]
    fn parses_a_bare_pair() {
        assert_eq!(
            parse_set_cookie("name=value"),
            Some((String::from("name"), String::from("value")))
        );
    }

    #[test]
    fn value_may_be_empty() {
        assert_eq!(
            parse_set_cookie("cleared=; Path=/; Max-Age=0"),
            Some((String::from("cleared"), String::new()))
        );
    }

    #[test]
    fn rejects_headers_without_a_name() {
        assert_eq!(parse_set_cookie("no-equals-sign"), None);
        assert_eq!(parse_set_cookie("=orphan-value"), None);
    }
}
