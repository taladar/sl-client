//! Library tranche: outbound HTTP and in-world URLs
//! (`server-lsl-lib-http-url`) — `llHTTPRequest` with each grid's options,
//! refusals, headers and throttle, the URL functions, `llHTTPResponse`,
//! `llGetHTTPHeader`, `llSetContentType` and OpenSim's three.
//!
//! The host carries the request and keeps the URLs; what a grid checks, adds
//! and refuses is here, measured by the `http/*` probes
//! (`book/src/gridspec/lsl.md` § Outbound HTTP and in-world URLs).

use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::http::{
    DEFAULT_BODY_MAX, Outbound, SECOND_LIFE_BODY_MAX, STATUS_CLIENT_ERROR, URL_REQUEST_DENIED,
    URL_REQUEST_GRANTED,
};

/// `HTTP_METHOD`.
const HTTP_METHOD: i32 = 0;
/// `HTTP_MIMETYPE`.
const HTTP_MIMETYPE: i32 = 1;
/// `HTTP_BODY_MAXLENGTH`.
const HTTP_BODY_MAXLENGTH: i32 = 2;
/// `HTTP_VERIFY_CERT`.
const HTTP_VERIFY_CERT: i32 = 3;
/// `HTTP_VERBOSE_THROTTLE`.
const HTTP_VERBOSE_THROTTLE: i32 = 4;
/// `HTTP_CUSTOM_HEADER`.
const HTTP_CUSTOM_HEADER: i32 = 5;
/// `HTTP_PRAGMA_NO_CACHE`.
const HTTP_PRAGMA_NO_CACHE: i32 = 6;
/// `HTTP_USER_AGENT`, Second Life's.
const HTTP_USER_AGENT: i32 = 7;
/// `HTTP_ACCEPT`, Second Life's.
const HTTP_ACCEPT: i32 = 8;
/// `HTTP_EXTENDED_ERROR`, Second Life's.
const HTTP_EXTENDED_ERROR: i32 = 9;

/// The methods Second Life sends; OpenSim sends any.
const SECOND_LIFE_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];

/// The longest user agent Second Life appends (306 characters were refused).
const USER_AGENT_MAX: usize = 255;

/// Custom header names OpenSim refuses outright (`HttpForbiddenHeaders`, the
/// fatal ones), compared without regard to case.
const OPENSIM_FATAL_HEADERS: &[&str] = &[
    "accept",
    "accept-charset",
    "content-type",
    "from",
    "host",
    "referer",
    "te",
    "trailer",
    "upgrade",
    "user-agent",
    "via",
];

/// Custom header names OpenSim drops silently (`HttpForbiddenHeaders`, the
/// rest), Second Life refuses with the fatal ones and every
/// `X-SecondLife-*`: the grid sets them itself.
const QUIET_HEADERS: &[&str] = &[
    "accept-ch",
    "accept-ch-lifetime",
    "access-control-request-headers",
    "access-control-request-method",
    "accept-encoding",
    "accept-patch",
    "accept-post",
    "accept-ranges",
    "cache-control",
    "connection",
    "content-length",
    "cookie",
    "cookie2",
    "date",
    "device-memory",
    "dtn",
    "early-data",
    "expect",
    "feature-policy",
    "keep-alive",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-unmodified-since",
    "max-forwards",
    "origin",
    "pragma",
    "server",
    "set-cookie",
    "set-cookie2",
    "transfer-encoding",
    "vary",
    "viewport-width",
    "warning",
    "width",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
];

/// The headers the grid sets on every request a script sends.
const GRID_HEADERS: &[&str] = &[
    "x-secondlife-shard",
    "x-secondlife-object-name",
    "x-secondlife-object-key",
    "x-secondlife-region",
    "x-secondlife-local-position",
    "x-secondlife-local-rotation",
    "x-secondlife-local-velocity",
    "x-secondlife-owner-name",
    "x-secondlife-owner-key",
];

/// What the script's options asked for.
#[derive(Debug, Default)]
struct Asked {
    /// `HTTP_METHOD`.
    method: Option<String>,
    /// `HTTP_MIMETYPE`.
    mimetype: Option<String>,
    /// `HTTP_BODY_MAXLENGTH`.
    max_body: Option<usize>,
    /// `HTTP_VERBOSE_THROTTLE`; on unless turned off.
    quiet_throttle: bool,
    /// `HTTP_CUSTOM_HEADER`s, in order.
    custom: Vec<(String, String)>,
    /// `HTTP_PRAGMA_NO_CACHE`; on unless turned off.
    no_pragma: bool,
    /// `HTTP_USER_AGENT`, the last given.
    user_agent: Option<String>,
    /// `HTTP_ACCEPT`.
    accept: Option<String>,
    /// `HTTP_EXTENDED_ERROR`, as it stood when the list was read up to a
    /// refusal.
    extended: bool,
}

/// Why Second Life refused a request, with what `HTTP_EXTENDED_ERROR` reports.
struct Refusal {
    /// The `DEBUG_CHANNEL` line.
    line: String,
    /// The problem report's status.
    status: i32,
    /// Its title.
    title: &'static str,
    /// Its detail.
    detail: String,
}

/// The element `list[at]` as text, as both grids read an option's value.
fn text_at(list: &[Element], at: usize) -> String {
    list.get(at)
        .map(crate::cast::element_to_string)
        .unwrap_or_default()
}

/// The element `list[at]` as an integer.
fn int_at(list: &[Element], at: usize) -> i32 {
    match list.get(at) {
        Some(Element::Integer(value)) => *value,
        Some(Element::Float(value)) => crate::num::float_to_int(*value),
        Some(Element::String(text) | Element::Key(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Second Life's reading of the option list, stopping at the first bad
/// option; positions count options from 1.
fn second_life_options(options: &[Element]) -> Result<Asked, (Refusal, bool)> {
    let mut asked = Asked::default();
    let mut at = 0_usize;
    let mut position = 0_i32;
    while at < options.len() {
        position = position.saturating_add(1);
        let flag = int_at(options, at);
        let value = text_at(options, at.saturating_add(1));
        let width = match flag {
            HTTP_METHOD => {
                if !SECOND_LIFE_METHODS.contains(&value.as_str()) {
                    return Err((
                        Refusal {
                            line: format!("HTTP method {value} is unsupported"),
                            status: 405,
                            title: "Unrecognized HTTP Method.",
                            detail: format!(
                                "HTTP method {value} is unsupported at position {position}."
                            ),
                        },
                        asked.extended,
                    ));
                }
                asked.method = Some(value);
                2
            }
            HTTP_MIMETYPE => {
                asked.mimetype = Some(value);
                2
            }
            HTTP_BODY_MAXLENGTH => {
                let max = int_at(options, at.saturating_add(1));
                match usize::try_from(max) {
                    Ok(max) if max > 0 && max <= SECOND_LIFE_BODY_MAX => asked.max_body = Some(max),
                    _ => {
                        return Err((
                            Refusal {
                                line: "HTTP_BODY_MAXLENGTH value is invalid".to_owned(),
                                status: 472,
                                title: "llHTTPRequest parameter error.",
                                detail: format!(
                                    "Maximum body length must be in the rage (0, 16384] in position {position}"
                                ),
                            },
                            asked.extended,
                        ));
                    }
                }
                2
            }
            HTTP_VERIFY_CERT => 2,
            HTTP_VERBOSE_THROTTLE => {
                asked.quiet_throttle = int_at(options, at.saturating_add(1)) == 0;
                2
            }
            HTTP_CUSTOM_HEADER => {
                let header = value.to_ascii_lowercase();
                if OPENSIM_FATAL_HEADERS.contains(&header.as_str())
                    || QUIET_HEADERS.contains(&header.as_str())
                    || header.starts_with("x-secondlife-")
                    || header.starts_with("proxy-")
                    || header.starts_with("sec-")
                {
                    return Err((
                        Refusal {
                            line: "HTTP_CUSTOM_HEADER value is an invalid header".to_owned(),
                            status: 473,
                            title: "llHTTPRequest parameter error.",
                            detail: format!("Invalid custom header in position {position}"),
                        },
                        asked.extended,
                    ));
                }
                asked
                    .custom
                    .push((value, text_at(options, at.saturating_add(2))));
                3
            }
            HTTP_PRAGMA_NO_CACHE => {
                asked.no_pragma = int_at(options, at.saturating_add(1)) == 0;
                2
            }
            HTTP_USER_AGENT => {
                if value.is_empty() || value.contains(' ') || value.chars().count() > USER_AGENT_MAX
                {
                    return Err((
                        Refusal {
                            line: format!("HTTP_USER_AGENT '{value}' is invalid"),
                            status: 472,
                            title: "llHTTPRequest parameter error.",
                            detail: format!("Invalid user agent {value} in position {position}"),
                        },
                        asked.extended,
                    ));
                }
                asked.user_agent = Some(value);
                2
            }
            HTTP_ACCEPT => {
                let line = if value.is_empty() || value.contains(',') {
                    Some(format!("HTTP_ACCEPT: value '{value}' is invalid."))
                } else if value.contains('*') {
                    Some(format!("HTTP_ACCEPT: Unsupported mime type '{value}'"))
                } else {
                    None
                };
                if let Some(line) = line {
                    return Err((
                        Refusal {
                            detail: format!("{line} in position {position}"),
                            line,
                            status: 472,
                            title: "llHTTPRequest parameter error.",
                        },
                        asked.extended,
                    ));
                }
                asked.accept = Some(value);
                2
            }
            HTTP_EXTENDED_ERROR => {
                asked.extended = int_at(options, at.saturating_add(1)) != 0;
                2
            }
            _ => {
                return Err((
                    Refusal {
                        line: format!("HTTP parameter key {position} is not recognized"),
                        status: 472,
                        title: "llHTTPRequest parameter error.",
                        detail: format!("Parameter key in position {position} is not recognized."),
                    },
                    asked.extended,
                ));
            }
        };
        at = at.saturating_add(width);
    }
    Ok(asked)
}

/// OpenSim's reading of the option list (`LSL_Api.llHTTPRequest`): the
/// refusal's line, or what was asked.
fn opensim_options(options: &[Element]) -> Result<Asked, String> {
    let mut asked = Asked::default();
    let mut at = 0_usize;
    while at < options.len() {
        let flag = int_at(options, at);
        if !(0..=HTTP_PRAGMA_NO_CACHE).contains(&flag) {
            return Err(format!("Parameter {at} is an invalid flag"));
        }
        let value = text_at(options, at.saturating_add(1));
        match flag {
            HTTP_METHOD => asked.method = Some(value),
            HTTP_MIMETYPE => asked.mimetype = Some(value),
            HTTP_BODY_MAXLENGTH => {
                asked.max_body = usize::try_from(int_at(options, at.saturating_add(1))).ok();
            }
            HTTP_VERBOSE_THROTTLE => {
                asked.quiet_throttle = int_at(options, at.saturating_add(1)) == 0;
            }
            HTTP_PRAGMA_NO_CACHE => {
                asked.no_pragma = int_at(options, at.saturating_add(1)) == 0;
            }
            HTTP_CUSTOM_HEADER => {
                // The name is at the next index, which the error counts.
                let name_at = at.saturating_add(1);
                let header = value.to_ascii_lowercase();
                if options.len().saturating_sub(name_at) < 2 {
                    return Err(format!(
                        "Missing name/value for custom header at parameter {name_at}"
                    ));
                }
                if OPENSIM_FATAL_HEADERS.contains(&header.as_str())
                    || header.starts_with("proxy-")
                    || header.starts_with("sec-")
                {
                    return Err(format!(
                        "Name is invalid as a custom header at parameter {name_at}"
                    ));
                }
                let header_value = text_at(options, name_at.saturating_add(1));
                if !QUIET_HEADERS.contains(&header.as_str())
                    && !GRID_HEADERS.contains(&header.as_str())
                {
                    asked.custom.push((value, header_value));
                }
                at = at.saturating_add(1);
            }
            _ => {}
        }
        at = at.saturating_add(2);
    }
    Ok(asked)
}

/// A URL's scheme and authority (`host[:port]`), if it has both.
fn parts(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (!scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        && !authority.is_empty()
        && !authority.contains(char::is_whitespace))
    .then_some((scheme, authority))
}

/// Whether `authority`'s host is on the simulator's own network: a loopback
/// or private address, or `localhost`.
fn private(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let octets: Vec<u8> = host
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    match octets.as_slice() {
        [127 | 10, ..] | [192, 168, ..] => true,
        [172, second, ..] => (16..=31).contains(second),
        _ => false,
    }
}

/// A fixed-point number with six decimals, as both grids write a header's
/// vector.
fn six(value: f32) -> String {
    format!("{value:.6}")
}

/// The headers the grid adds to a request from the caller's prim, in the
/// order Second Life sends them, with the user agent first.
fn grid_headers(ctx: &mut ScriptCtx<'_>, user_agent: Option<&str>) -> Vec<(String, String)> {
    let caller = ctx.caller();
    let opensim = ctx.flavour().is_opensim();
    let own = crate::library::prim_params::own_key(ctx).unwrap_or_default();
    let details = ctx.host().object_details(caller, &own);
    let owner = ctx.host().owner(caller);
    let owner_name = ctx
        .host()
        .object_details(caller, &owner)
        .map(|avatar| avatar.name)
        .unwrap_or_default();
    let region = ctx.host().region(caller);
    let simulator = ctx.host().simulator();
    let mut headers = Vec::new();
    if !opensim {
        let mut agent = format!(
            "Second-Life-LSL/{} (https://secondlife.com)",
            simulator.version
        );
        if let Some(extra) = user_agent {
            agent.push(' ');
            agent.push_str(extra);
        }
        headers.push(("User-Agent".to_owned(), agent));
    }
    let shard = if opensim {
        "OpenSim"
    } else if simulator.grid_nick == "aditi" {
        "Testing"
    } else {
        "Production"
    };
    headers.push(("X-SecondLife-Shard".to_owned(), shard.to_owned()));
    let (name, position, rotation, velocity) = details.map_or_else(
        || {
            (
                String::new(),
                crate::value::ZERO_VECTOR,
                crate::value::ZERO_ROTATION,
                crate::value::ZERO_VECTOR,
            )
        },
        |prim| (prim.name, prim.position, prim.rotation, prim.velocity),
    );
    headers.push(("X-SecondLife-Object-Name".to_owned(), name));
    headers.push(("X-SecondLife-Object-Key".to_owned(), own));
    headers.push((
        "X-SecondLife-Region".to_owned(),
        format!("{} ({}, {})", region.name, region.corner.0, region.corner.1),
    ));
    headers.push((
        "X-SecondLife-Local-Position".to_owned(),
        format!(
            "({}, {}, {})",
            six(position.x),
            six(position.y),
            six(position.z)
        ),
    ));
    headers.push((
        "X-SecondLife-Local-Rotation".to_owned(),
        format!(
            "({}, {}, {}, {})",
            six(rotation.x),
            six(rotation.y),
            six(rotation.z),
            six(rotation.s)
        ),
    ));
    headers.push((
        "X-SecondLife-Local-Velocity".to_owned(),
        format!(
            "({}, {}, {})",
            six(velocity.x),
            six(velocity.y),
            six(velocity.z)
        ),
    ));
    headers.push(("X-SecondLife-Owner-Name".to_owned(), owner_name));
    headers.push(("X-SecondLife-Owner-Key".to_owned(), owner));
    headers
}

/// The RFC 7807 problem report Second Life answers a refused request with
/// under `HTTP_EXTENDED_ERROR`.
fn problem(refusal: &Refusal) -> String {
    let quote = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    // Pretty-printed as Second Life's JSON writer prints it.
    format!(
        "{{\n   \"detail\" : \"{}\",\n   \"status\" : {},\n   \"title\" : \"{}\",\n   \"type\" : \"http://wiki.secondlife.com/wiki/llHTTPRequest\"\n}}\n",
        quote(&refusal.detail),
        refusal.status,
        quote(refusal.title)
    )
}

/// Second Life's refusal: `NULL_KEY` and the line, or — under
/// `HTTP_EXTENDED_ERROR` — a key and the problem report at once.
fn refuse_second_life(ctx: &mut ScriptCtx<'_>, refusal: &Refusal, extended: bool) -> Key {
    ctx.refuse(refusal.line.clone());
    if !extended {
        return Key(NULL_KEY.to_owned());
    }
    let id = ctx.fresh_key();
    ctx.raise_http_response(id.clone(), refusal.status, problem(refusal));
    Key(id)
}

/// `llHTTPRequest`: send `body` to `url` as `options` ask, from the
/// caller's prim with the grid's headers; the answer comes as
/// `http_response`. Each grid's refusals, throttle and quirks:
///
/// - Second Life reads the options in order and refuses at the first bad
///   one — a method it does not send, a reserved custom header, a body
///   limit over 16384, a user agent or `Accept` it will not take, an unknown
///   option — with `NULL_KEY` and a line; a URL that is not `http(s)` or is
///   on a private network but the grid's own is refused the same way. Under
///   `HTTP_EXTENDED_ERROR` a refusal is a key and an RFC 7807 answer. A
///   `GET`'s body is dropped. Its throttle shouts `Too many HTTP requests too
///   fast.` unless `HTTP_VERBOSE_THROTTLE` is off.
/// - OpenSim throttles first, silently; a refusal says its line and returns
///   an empty key no answer ever follows; a `GET` with a body is answered 499
///   by its own client; it sends `Pragma: no-cache` only when the option
///   turns it off, and no user agent.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_http_request(
    ctx: &mut ScriptCtx<'_>,
    url: String,
    options: Vec<Element>,
    body: String,
) -> Result<Key, CallError> {
    let flavour = ctx.flavour();
    let opensim = flavour.is_opensim();
    let caller = ctx.caller();
    let at = ctx.region_time();
    if opensim && !ctx.host().http_throttle(caller).admit(flavour, at) {
        return Ok(Key(NULL_KEY.to_owned()));
    }
    let own_authority = ctx.host().url_authority(caller);
    let asked = if opensim {
        match parts(&url) {
            None => {
                ctx.refuse("llHTTPRequest: Invalid url".to_owned());
                return Ok(Key(String::new()));
            }
            Some((scheme, _)) if scheme != "http" && scheme != "https" => {
                ctx.refuse("llHTTPRequest: Invalid url schema".to_owned());
                return Ok(Key(String::new()));
            }
            Some(_) => {}
        }
        match opensim_options(&options) {
            Ok(asked) => asked,
            Err(line) => {
                ctx.refuse(format!("llHTTPRequest: {line}"));
                // The one refusal that also takes a `ScriptSleep`.
                if line.ends_with("is an invalid flag") {
                    ctx.delay(0.2);
                }
                return Ok(Key(String::new()));
            }
        }
    } else {
        let asked = match second_life_options(&options) {
            Ok(asked) => asked,
            Err((refusal, extended)) => return Ok(refuse_second_life(ctx, &refusal, extended)),
        };
        let valid = parts(&url).is_some_and(|(scheme, authority)| {
            (scheme == "http" || scheme == "https")
                && (!private(authority) || authority == own_authority)
        });
        if !valid {
            let refusal = Refusal {
                line: "URL passed to llHTTPRequest is not valid".to_owned(),
                status: 471,
                title: "llHTTPRequest URL is invalid.",
                detail: "URL passed to llHTTPRequest is not valid".to_owned(),
            };
            return Ok(refuse_second_life(ctx, &refusal, asked.extended));
        }
        if !ctx.host().http_throttle(caller).admit(flavour, at) {
            if !asked.quiet_throttle {
                ctx.refuse("Too many HTTP requests too fast.".to_owned());
            }
            return Ok(Key(NULL_KEY.to_owned()));
        }
        asked
    };
    let mut method = asked.method.clone().unwrap_or_else(|| "GET".to_owned());
    // OpenSim's client (.NET's `HttpWebRequest`) spells a method it knows in
    // capitals (`get` arrives as `GET`) and sends any other as given.
    let upper = method.to_ascii_uppercase();
    if opensim
        && SECOND_LIFE_METHODS
            .iter()
            .chain(&["OPTIONS"])
            .any(|known| *known == upper)
    {
        method = upper;
    }
    let id = ctx.fresh_key();
    if opensim {
        if parts(&url)
            .is_some_and(|(_, authority)| private(authority) && authority != own_authority)
        {
            ctx.refuse(format!(
                "llHttpRequest: Request to {url} disallowed by filter"
            ));
            return Ok(Key(String::new()));
        }
        if method == "GET" && !body.is_empty() {
            ctx.raise_http_response(
                id.clone(),
                STATUS_CLIENT_ERROR,
                "Cannot send a content-body with this verb-type.".to_owned(),
            );
            return Ok(Key(id));
        }
    }
    let mut headers = grid_headers(ctx, asked.user_agent.as_deref());
    for (name, value) in &asked.custom {
        if !GRID_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            headers.push((name.clone(), value.clone()));
        }
    }
    headers.push((
        "Content-Type".to_owned(),
        asked
            .mimetype
            .clone()
            .unwrap_or_else(|| "text/plain;charset=utf-8".to_owned()),
    ));
    if let Some(accept) = &asked.accept {
        headers.push(("Accept".to_owned(), accept.clone()));
    }
    // OpenSim adds the pragma only when the option is off
    // (`ScriptsHttpRequests.cs`); Second Life sends it unless it is.
    if asked.no_pragma == opensim {
        headers.push(("Pragma".to_owned(), "no-cache".to_owned()));
    }
    let body = if !opensim && method == "GET" {
        String::new()
    } else {
        body
    };
    let max_body = asked.max_body.unwrap_or(DEFAULT_BODY_MAX);
    let _previous = ctx.http().pending.insert(id.clone(), max_body);
    let request = Outbound {
        id: id.clone(),
        method,
        url,
        body,
        headers,
        max_body,
    };
    ctx.host().send_http(caller, &request);
    Ok(Key(id))
}

/// Ask the host for a URL, secure or not, and answer the script with
/// `http_request(key, URL_REQUEST_GRANTED, url)` or `URL_REQUEST_DENIED`.
fn request_url(ctx: &mut ScriptCtx<'_>, secure: bool) -> Key {
    let caller = ctx.caller();
    let id = ctx.fresh_key();
    match ctx.host().request_url(caller, &id, secure) {
        Some(url) => {
            ctx.http().urls.push(url.clone());
            ctx.raise_url_answer(id.clone(), URL_REQUEST_GRANTED, url);
        }
        None => ctx.raise_url_answer(id.clone(), URL_REQUEST_DENIED, String::new()),
    }
    Key(id)
}

/// `llRequestURL`: a URL of the script's own; `http_request` answers it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_url(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(request_url(ctx, false))
}

/// `llRequestSecureURL`: an `https` URL of the script's own, which a grid
/// with no HTTPS listener denies.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_secure_url(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(request_url(ctx, true))
}

/// `osRequestURL`: `llRequestURL` with options (`allowXss`, which the fake
/// grid serves nothing differently for).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_request_url(ctx: &mut ScriptCtx<'_>, _options: Vec<Element>) -> Result<Key, CallError> {
    Ok(request_url(ctx, false))
}

/// `osRequestSecureURL`: `llRequestSecureURL` with options.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_request_secure_url(
    ctx: &mut ScriptCtx<'_>,
    _options: Vec<Element>,
) -> Result<Key, CallError> {
    Ok(request_url(ctx, true))
}

/// `llReleaseURL`: give a URL of the script's back; it is free again at once.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_release_url(ctx: &mut ScriptCtx<'_>, url: String) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.http().urls.retain(|held| *held != url);
    ctx.host().release_url(caller, &url);
    Ok(())
}

/// `llGetFreeURLs`: how many more URLs the script's land allows.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_free_urls(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().free_urls(caller))
}

/// `llHTTPResponse`: answer the request `id` that arrived at one of the
/// script's URLs; an id the script was not asked by is ignored.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_http_response(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
    status: i32,
    body: String,
) -> Result<(), CallError> {
    let flavour = ctx.flavour();
    let caller = ctx.caller();
    let Some(inbound) = ctx.http().inbound.remove(&id.0) else {
        return Ok(());
    };
    let served = ctx
        .http()
        .content_types
        .remove(&id.0)
        .unwrap_or_else(|| crate::vm::http::content_type(flavour, 0));
    // A single type asked for (`HTTP_ACCEPT`); a browser's list asks nothing.
    let accept = inbound
        .header("accept")
        .filter(|accept| !accept.contains(',') && !accept.contains('*'));
    let answered = crate::vm::http::answer(flavour, status, &body, served, accept);
    ctx.host().answer_http(caller, &id.0, answered);
    Ok(())
}

/// `llGetHTTPHeader`: the header `name` of the request `id` that arrived at
/// one of the script's URLs — on Second Life only its `x-*` headers and
/// `user-agent`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_http_header(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
    name: String,
) -> Result<String, CallError> {
    let flavour = ctx.flavour();
    let name = name.to_ascii_lowercase();
    if !crate::vm::http::header_visible(flavour, &name) {
        return Ok(String::new());
    }
    Ok(ctx
        .http()
        .inbound
        .get(&id.0)
        .and_then(|inbound| inbound.header(&name))
        .unwrap_or_default()
        .to_owned())
}

/// `llSetContentType`: the type the answer to the request `id` is served as
/// — on Second Life; OpenSim's call has no effect.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_content_type(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
    content_type: i32,
) -> Result<(), CallError> {
    let flavour = ctx.flavour();
    if flavour.is_opensim() {
        return Ok(());
    }
    let served = crate::vm::http::content_type(flavour, content_type);
    let _previous = ctx.http().content_types.insert(id.0, served);
    Ok(())
}

/// `osSetContentType`: the `Content-Type` the answer to the request `id` is
/// served with, as given.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_content_type(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
    content_type: String,
) -> Result<(), CallError> {
    let _previous = ctx.http().content_types.insert(id.0, content_type);
    Ok(())
}
