//! Outbound HTTP and in-world URLs (`server-lsl-lib-http-url`): the requests
//! a script sends, the URLs it is granted and what arrives at them.
//!
//! The library checks a script's `llHTTPRequest` and hands the host a
//! finished [`Outbound`] — its headers the grid's own and the script's — and
//! the host sends it where it goes and hands the answer back
//! ([`crate::vm::Engine::http_response`]). A URL is minted by the host
//! ([`granted_url`] spells it as each grid does); a request arriving at one is
//! judged by the grid's URL surface ([`arrive`]) before a script hears it
//! ([`crate::vm::Engine::http_request`]), and the script's answer is made into
//! what goes back by [`answer`]. Everything here is each grid's measured rule
//! (`book/src/gridspec/lsl.md` § Outbound HTTP and in-world URLs), so a test
//! host and the fake grid answer alike.

use core::time::Duration;
use std::collections::VecDeque;

use crate::vm::EngineFlavour;

/// The `method` of the `http_request` that answers `llRequestURL` when the
/// URL is granted; its body is the URL.
pub const URL_REQUEST_GRANTED: &str = "URL_REQUEST_GRANTED";
/// The `method` of the `http_request` that answers `llRequestURL` when no URL
/// is granted.
pub const URL_REQUEST_DENIED: &str = "URL_REQUEST_DENIED";

/// The longest answer body a request takes unless `HTTP_BODY_MAXLENGTH` says
/// otherwise, on both grids.
pub const DEFAULT_BODY_MAX: usize = 2048;
/// The longest `HTTP_BODY_MAXLENGTH` Second Life accepts.
pub const SECOND_LIFE_BODY_MAX: usize = 16 * 1024;
/// The longest request body Second Life hands a script: the rest is cut.
pub const SECOND_LIFE_INBOUND_MAX: usize = 2048;

/// The status of a request the grid's own client refused or could not send:
/// OpenSim's, which Second Life does not use for a refusal.
pub const STATUS_CLIENT_ERROR: i32 = 499;

/// A request a script sends, checked, with every header it goes out with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    /// The request's key, which its `http_response` carries.
    pub id: String,
    /// The method, as the script gave it.
    pub method: String,
    /// The URL.
    pub url: String,
    /// The body, already dropped for a `GET` on Second Life.
    pub body: String,
    /// The headers, in the order they are sent, names as the grid spells them.
    pub headers: Vec<(String, String)>,
    /// The longest answer body the script takes (`HTTP_BODY_MAXLENGTH`).
    pub max_body: usize,
}

impl Outbound {
    /// The header `name`, compared without regard to case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A request arriving at a script's URL, as the script's event and
/// `llGetHTTPHeader` read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inbound {
    /// The method.
    pub method: String,
    /// The body, cut as the grid cuts it.
    pub body: String,
    /// Every header, names in lower case, the grid's `x-*` ones among them.
    pub headers: Vec<(String, String)>,
}

impl Inbound {
    /// The header `name` (lower case).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value.as_str())
    }
}

/// What goes back to whoever sent a request to a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The status.
    pub status: i32,
    /// The `Content-Type`, empty for none.
    pub content_type: String,
    /// The body.
    pub body: String,
}

/// What the grid's URL surface makes of a request to a script's URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrival {
    /// The script hears it.
    Script(Inbound),
    /// The grid answers it itself; the script never hears it.
    Answered(Answered),
}

/// The URL `flavour`'s grid grants for `key`, reached at `authority`
/// (`host:port`): Second Life's `…/cap/<key>`, no slash at the end (port 12043
/// and `https` for a secure one); OpenSim's `…/lslhttp/<key>/`, with one.
#[must_use]
pub fn granted_url(flavour: EngineFlavour, authority: &str, key: &str, secure: bool) -> String {
    let scheme = if secure { "https" } else { "http" };
    if flavour.is_opensim() {
        format!("{scheme}://{authority}/lslhttp/{key}/")
    } else {
        format!("{scheme}://{authority}/cap/{key}")
    }
}

/// The key of the script URL `path` names (`/cap/<key>…` or
/// `/lslhttp/<key>/…`, whichever `flavour` grants), and the path after it.
#[must_use]
pub fn url_key(flavour: EngineFlavour, path: &str) -> Option<(&str, &str)> {
    let prefix = if flavour.is_opensim() {
        "/lslhttp/"
    } else {
        "/cap/"
    };
    let rest = path.strip_prefix(prefix)?;
    let end = rest.find('/').unwrap_or(rest.len());
    let key = rest.get(..end)?;
    let after = rest.get(end..).unwrap_or_default();
    (key.len() == 36).then_some((key, after))
}

/// A request from `remote` (an address) to the script URL `script_url`,
/// arriving with `path` after the URL, `query`, the sender's own `headers`
/// and `body`, as `flavour`'s grid hands it on — or answers it itself:
/// Second Life answers `HEAD` 400 and `OPTIONS` 200 without the script, and
/// cuts a body to 2048 bytes; OpenSim decodes the query.
#[must_use]
pub fn arrive(
    flavour: EngineFlavour,
    script_url: &str,
    (method, path, query): (&str, &str, &str),
    remote: &str,
    headers: &[(String, String)],
    body: &str,
) -> Arrival {
    let opensim = flavour.is_opensim();
    if !opensim {
        match method {
            "HEAD" => {
                return Arrival::Answered(Answered {
                    status: 400,
                    content_type: String::new(),
                    body: String::new(),
                });
            }
            "OPTIONS" => {
                return Arrival::Answered(Answered {
                    status: 200,
                    content_type: "text/plain".to_owned(),
                    body: "OK".to_owned(),
                });
            }
            _ => {}
        }
    }
    let body = if opensim {
        body.to_owned()
    } else {
        cut(body, SECOND_LIFE_INBOUND_MAX)
    };
    let mut held: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .filter(|(name, _)| !GRID_HEADERS.contains(&name.as_str()))
        .collect();
    let query = if opensim {
        crate::library::encoding::os_unescape(query)
    } else {
        query.to_owned()
    };
    // OpenSim's URL ends in a slash `x-script-url` leaves off; Second
    // Life's ends in none.
    held.push((
        "x-script-url".to_owned(),
        script_url.trim_end_matches('/').to_owned(),
    ));
    held.push(("x-path-info".to_owned(), path.to_owned()));
    held.push(("x-query-string".to_owned(), query));
    held.push(("x-remote-ip".to_owned(), remote.to_owned()));
    Arrival::Script(Inbound {
        method: method.to_owned(),
        body,
        headers: held,
    })
}

/// The headers a grid sets on an arriving request itself, whatever the
/// sender said.
const GRID_HEADERS: &[&str] = &[
    "x-script-url",
    "x-path-info",
    "x-query-string",
    "x-remote-ip",
    "host",
    "content-length",
    "via",
    "x-forwarded-for",
];

/// Whether `flavour`'s `llGetHTTPHeader` gives the arriving header `name`:
/// on Second Life only the grid's own `x-*` headers and `user-agent` — never
/// one the sender added, `x-` or not — on OpenSim every one.
#[must_use]
pub fn header_visible(flavour: EngineFlavour, name: &str) -> bool {
    flavour.is_opensim()
        || name.starts_with("x-secondlife-")
        || matches!(
            name,
            "x-script-url" | "x-path-info" | "x-query-string" | "x-remote-ip" | "user-agent"
        )
}

/// `CONTENT_TYPE_*` as each grid serves it: Second Life the type with
/// `; charset=utf-8` (HTML served as plain text, since it is drawn as HTML
/// only in the owner's own viewer); OpenSim `text/plain` whatever was set.
#[must_use]
pub fn content_type(flavour: EngineFlavour, code: i32) -> String {
    if flavour.is_opensim() {
        return "text/plain".to_owned();
    }
    let kind = match code {
        2 => "application/xml",
        3 => "application/xhtml+xml",
        4 => "application/atom+xml",
        5 => "application/json",
        6 => "application/llsd+xml",
        7 => "application/x-www-form-urlencoded",
        _ => "text/plain",
    };
    format!("{kind}; charset=utf-8")
}

/// What goes back for a script's `llHTTPResponse(status, body)`, served as
/// `served`, to a request that asked for `accept` (Second Life's
/// `HTTP_ACCEPT`): Second Life replaces an answer of another type with 415.
#[must_use]
pub fn answer(
    flavour: EngineFlavour,
    status: i32,
    body: &str,
    served: String,
    accept: Option<&str>,
) -> Answered {
    if !flavour.is_opensim()
        && let Some(accept) = accept
        && !served.starts_with(accept)
    {
        return Answered {
            status: 415,
            content_type: "text/plain".to_owned(),
            body: "Unsupported or unknown Content-Type.".to_owned(),
        };
    }
    Answered {
        status,
        content_type: served,
        body: body.to_owned(),
    }
}

/// How long `flavour`'s grid waits for a script's answer, and what it sends
/// when none came: Second Life's 504 after 26 s, OpenSim's 500 after 25 s.
#[must_use]
pub fn timeout(flavour: EngineFlavour) -> (Duration, Answered) {
    if flavour.is_opensim() {
        (
            Duration::from_secs(25),
            Answered {
                status: 500,
                content_type: "text/plain".to_owned(),
                body: "Script timeout".to_owned(),
            },
        )
    } else {
        (
            Duration::from_secs(26),
            Answered {
                status: 504,
                content_type: "text/plain".to_owned(),
                body: "Timed out waiting for response from internal server.".to_owned(),
            },
        )
    }
}

/// What a request to a URL no script holds (released, or never granted)
/// gets: Second Life's `cap not found`, OpenSim's HTML page.
#[must_use]
pub fn not_found(flavour: EngineFlavour, key: &str) -> Answered {
    if flavour.is_opensim() {
        Answered {
            status: 404,
            content_type: "text/html".to_owned(),
            body: "<html><head><title>Page not found</title></head>\
                   <body>Page not found</body></html>"
                .to_owned(),
        }
    } else {
        Answered {
            status: 404,
            content_type: String::new(),
            body: format!("cap not found: '{key}'\n"),
        }
    }
}

/// An answer's body as a script's `http_response` gets it, cut at `max`
/// characters, and its metadata: `[HTTP_BODY_TRUNCATED, max]` when it was cut,
/// on Second Life only.
#[must_use]
pub fn response_body(flavour: EngineFlavour, body: &str, max: usize) -> (String, Vec<i32>) {
    if body.chars().count() <= max {
        return (body.to_owned(), Vec::new());
    }
    let metadata = if flavour.is_opensim() {
        Vec::new()
    } else {
        vec![0, i32::try_from(max).unwrap_or(i32::MAX)]
    };
    (cut(body, max), metadata)
}

/// The first `max` characters of `text`.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A prim's `llHTTPRequest` throttle, kept by the host for the prim.
///
/// Second Life allows 25 requests in 20 s and counts the refused ones too,
/// so a script that keeps retrying stays refused until it is quiet for 20 s.
/// OpenSim's bucket (`ScriptsHttpRequests.CheckThrottle`) starts with two of
/// its burst of three after the first request and refills one a second; it
/// lets a request through whenever it holds more than nothing, so the time a
/// request itself takes refills a sliver and the fourth of a burst goes
/// through. That sliver is a microsecond per request here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Throttle {
    /// Second Life: the region times of every request in the window.
    attempts: VecDeque<Duration>,
    /// OpenSim: the bucket's tokens and when it was last looked at, once the
    /// first request was made.
    bucket: Option<(f64, Duration)>,
}

/// Second Life's window.
const SECOND_LIFE_WINDOW: Duration = Duration::from_secs(20);
/// Second Life's allowance in the window.
const SECOND_LIFE_ALLOWANCE: usize = 25;
/// OpenSim's burst (`PrimRequestsBurst`).
const OPENSIM_BURST: f64 = 3.0;
/// OpenSim's refill a second (`PrimRequestsPerSec`).
const OPENSIM_PER_SECOND: f64 = 1.0;

impl Throttle {
    /// Whether a request made at region time `at` goes through.
    pub fn admit(&mut self, flavour: EngineFlavour, at: Duration) -> bool {
        if flavour.is_opensim() {
            let Some((tokens, last)) = self.bucket else {
                self.bucket = Some((OPENSIM_BURST - 1.0, at));
                return true;
            };
            let elapsed = at.saturating_sub(last).max(Duration::from_micros(1));
            let mut tokens = elapsed.as_secs_f64().mul_add(OPENSIM_PER_SECOND, tokens);
            let admitted = if tokens > OPENSIM_BURST {
                tokens = OPENSIM_BURST - 1.0;
                true
            } else if tokens > 0.0 {
                tokens -= 1.0;
                true
            } else {
                false
            };
            self.bucket = Some((tokens, at));
            admitted
        } else {
            while self
                .attempts
                .front()
                .is_some_and(|first| at.saturating_sub(*first) >= SECOND_LIFE_WINDOW)
            {
                let _gone = self.attempts.pop_front();
            }
            let admitted = self.attempts.len() < SECOND_LIFE_ALLOWANCE;
            self.attempts.push_back(at);
            admitted
        }
    }
}
