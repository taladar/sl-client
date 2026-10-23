//! The sans-I/O **asset-delivery** CAPS surface: the four capabilities that
//! stream stored asset bytes to the viewer — `GetTexture`, `GetMesh`,
//! `GetMesh2` and `ViewerAsset`.
//!
//! [`AssetCaps`] is the session-free sibling of [`SimCaps`](crate::SimCaps).
//! Where [`SimCaps`] routes stateful capabilities against a
//! [`SimSession`](crate::SimSession), an [`AssetCaps`] routes byte fetches
//! against an [`AssetSource`] and holds no session state at all. That split
//! is deliberate: on Second Life these caps are served by a **content
//! delivery network on a different host** from the simulator, so the asset
//! surface must be constructible — and dispatchable — with nothing but a base
//! URL, its token map, and a byte source. A CDN process builds one with
//! [`AssetCaps::from_tokens`] from the token map the simulator minted and
//! advertised in its seed grant; nothing else crosses that boundary. (Avatar
//! *baking* is yet another, separate service and is out of scope here.)
//!
//! [`SimCaps`] composes one [`AssetCaps`] purely so a single seed grant
//! advertises every capability, sim and asset alike; the asset dispatch path
//! stays independent.
//!
//! The wire contract (mirroring the client fetchers in
//! `sl-client-tokio/src/{textures,meshes,assets}.rs`): a `GET` on the cap URL
//! with a `?<class>_id=<uuid>` selector and an optional `Range: bytes=s-e`
//! header. No range → `200` whole; a range the asset has bytes for → `206`
//! with the byte slice and a `Content-Range` header; a missing asset → `404`.
//!
//! Everything past that is where the two live grids part, and an
//! [`AssetHttpPolicy`] says which way a surface goes: how a `Range` header is
//! read, what a request that names no asset is answered with, whether a
//! `POST` is answered, and whether an answer states what a cache needs. Both
//! were measured with the `asset-http` conformance case (2026-10-12,
//! `book/src/gridspec/assets.md`). One thing they agree on is not what HTTP
//! says: **a range that starts at or past the end of the asset is answered
//! `200` with the whole asset**, never `416`.

use std::collections::{BTreeMap, HashMap};

use url::Url;
use uuid::Uuid;

use crate::asset_source::AssetSource;
use crate::sim_caps::{CapsRequest, CapsResponse};
use crate::{AssetKey, AssetType, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_VIEWER_ASSET};

/// The `Content-Type` a `GetTexture` fetch is served with — the viewer's
/// JPEG2000 codestream media type (its `Accept` value too).
const J2C_CONTENT_TYPE: &str = "image/x-j2c";

/// The `Content-Type` a `GetMesh` / `GetMesh2` fetch is served with (the
/// viewer's `HTTP_CONTENT_VND_LL_MESH`).
const MESH_CONTENT_TYPE: &str = "application/vnd.ll.mesh";

/// The `Content-Type` a generic `ViewerAsset` fetch is served with where the
/// class has none of its own below.
const OCTET_STREAM_CONTENT_TYPE: &str = "application/octet-stream";

/// The `Content-Type` a `ViewerAsset` fetch under `key` is served with: the
/// class's own media type for the classes the live grids were measured
/// serving, the generic one for the rest.
fn viewer_asset_content_type(key: &str) -> &'static str {
    match key {
        "texture_id" => J2C_CONTENT_TYPE,
        "mesh_id" => MESH_CONTENT_TYPE,
        "sound_id" => "application/ogg",
        "bodypart_id" => "application/vnd.ll.bodypart",
        "clothing_id" => "application/vnd.ll.clothing",
        _ => OCTET_STREAM_CONTENT_TYPE,
    }
}

/// The `Last-Modified` an answer states where the policy states one. An
/// [`AssetSource`] keeps no dates, so every asset has the same.
const LAST_MODIFIED: &str = "Thu, 01 Jan 2015 00:00:00 GMT";

/// How an asset host reads a `Range` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RangeReading {
    /// As Second Life's asset hosts do: one range, a suffix (`bytes=-10`, the
    /// last ten bytes) or several (answered `206` with a multipart body under
    /// the asset's own media type). A range that ends before it starts, or
    /// that is no number, is no range.
    #[default]
    ByteRanges,
    /// As OpenSim's `Util.TryParseHttpRange` does: the header split at every
    /// `-` must make two parts, so several ranges are no range; an absent
    /// start is `0`, so a suffix `bytes=-10` is the **first eleven** bytes;
    /// an end that is not above zero is no range, so `bytes=0-0` is the whole
    /// asset; and an end before the start is the one byte at the end.
    OpenSim,
}

/// Where the asset-delivery capabilities of the two live grids part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetHttpPolicy {
    /// How a `Range` header is read.
    pub ranges: RangeReading,
    /// The status of a request that names no asset: no query, a key the
    /// capability does not know, or text that is no id.
    pub unreadable_status: u16,
    /// Whether a `POST` is answered as a `GET` is, rather than refused.
    pub answers_post: bool,
    /// Whether an answer states `Accept-Ranges`, an `ETag` and a
    /// `Last-Modified`, and a request whose `If-Modified-Since` is that date
    /// is answered `304`. An `If-None-Match` is not honoured either way.
    pub validators: bool,
    /// The `ViewerAsset` query keys the grid refuses whatever the asset, and
    /// the status it refuses them with: Second Life serves no notecard over
    /// HTTP (`notecard_id`, `503` — `book/src/gridspec/inventory.md`
    /// § Notecards and gestures); a viewer reads one through its item over
    /// the UDP transfer instead.
    pub refused_keys: (&'static [&'static str], u16),
}

impl Default for AssetHttpPolicy {
    /// What the surface did before the grids were measured, where that is not
    /// something both were found doing otherwise.
    fn default() -> Self {
        Self {
            ranges: RangeReading::ByteRanges,
            unreadable_status: 404,
            answers_post: false,
            validators: false,
            refused_keys: (&[], 503),
        }
    }
}

/// The four asset-delivery capabilities this surface serves — the registry
/// keys, grown alongside the pinned coverage table in
/// [`sim_caps`](crate::sim_caps).
const ASSET_CAPABILITIES: &[&str] = &[
    CAP_GET_TEXTURE,
    CAP_GET_MESH,
    CAP_GET_MESH2,
    CAP_VIEWER_ASSET,
];

/// How the asset surface serves one capability name.
///
/// `GetMesh` and `GetMesh2` share one variant: they differ only in which cap
/// URL the client fetches (the region advertises `GetMesh2` when it supports
/// it), not in the wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssetCapHandler {
    /// `GetTexture` — `?texture_id=<uuid>`, served `image/x-j2c`.
    GetTexture,
    /// `GetMesh` / `GetMesh2` — `?mesh_id=<uuid>`, served
    /// `application/vnd.ll.mesh`.
    GetMesh,
    /// `ViewerAsset` — `?<class>_id=<uuid>`, served
    /// `application/octet-stream`.
    ViewerAsset,
}

/// The server-side asset-delivery CAPS surface: the four asset-cap tokens and
/// the base URL they are minted under.
///
/// Session-free and byte-source-driven, so it dispatches against a plain
/// [`AssetSource`] and can run in a **different process** (a CDN) from the
/// simulator. Construct it with [`AssetCaps::new`] (in-process, sharing the
/// simulator's token mint) or [`AssetCaps::from_tokens`] (a CDN process,
/// rebuilding the surface the simulator advertised).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCaps {
    /// The base under which the four asset cap URLs are minted
    /// (`{base}/cap/{token}`). May be a CDN host distinct from the simulator's
    /// base URL. Must be an HTTP(S) URL.
    base_url: Url,
    /// One pre-minted URL token per asset capability
    /// ([`ASSET_CAPABILITIES`]).
    tokens: BTreeMap<&'static str, Uuid>,
    /// Which live grid's HTTP the surface answers with.
    policy: AssetHttpPolicy,
    /// Whether the four capabilities are one URL, the base itself
    /// ([`AssetCaps::at_the_root`]), rather than a token each under it.
    at_the_root: bool,
}

impl AssetCaps {
    /// Mints one unguessable token per asset capability under `base_url`.
    ///
    /// `mint_token` supplies the randomness — sans-I/O purity means the caller
    /// owns it (a runtime passes `Uuid::new_v4`, tests a deterministic
    /// counter), exactly like [`SimCaps::new`](crate::SimCaps::new). In the
    /// in-process case [`SimCaps::new`](crate::SimCaps::new) passes its own
    /// mint here so all tokens come from one stream.
    pub fn new(base_url: Url, mut mint_token: impl FnMut() -> Uuid) -> Self {
        let tokens = ASSET_CAPABILITIES
            .iter()
            .map(|name| (*name, mint_token()))
            .collect::<BTreeMap<&'static str, Uuid>>();
        Self {
            base_url,
            tokens,
            policy: AssetHttpPolicy::default(),
            at_the_root: false,
        }
    }

    /// The surface as Second Life lays it out: **one URL for all four
    /// capabilities, the bare `base_url`**, with no token and no path — the
    /// same for every agent of the grid (measured on aditi 2026-10-12, where
    /// each of the four is the root of the grid's asset host). A request is
    /// told apart by its query key alone, so any of the four answers an
    /// asset of any class, as that host does.
    #[must_use]
    pub fn at_the_root(base_url: Url) -> Self {
        Self {
            base_url,
            tokens: BTreeMap::new(),
            policy: AssetHttpPolicy::default(),
            at_the_root: true,
        }
    }

    /// The same surface answering as `policy` says.
    #[must_use]
    pub const fn with_http_policy(mut self, policy: AssetHttpPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Rebuilds an identical surface from a known token map — the
    /// cross-process constructor. A CDN process receives the map the
    /// simulator minted (via [`AssetCaps::tokens`], shipped out of band) and
    /// serves the same URLs the simulator advertised in its seed grant.
    #[must_use]
    pub fn from_tokens(base_url: Url, tokens: BTreeMap<&'static str, Uuid>) -> Self {
        Self {
            base_url,
            tokens,
            policy: AssetHttpPolicy::default(),
            at_the_root: false,
        }
    }

    /// The minted token map, so the simulator process can ship it to a CDN
    /// process that will [`AssetCaps::from_tokens`] an identical surface.
    #[must_use]
    pub const fn tokens(&self) -> &BTreeMap<&'static str, Uuid> {
        &self.tokens
    }

    /// The handler for an asset capability name — the asset half of the
    /// dispatch registry (and of the pinned coverage table in
    /// [`sim_caps`](crate::sim_caps)).
    pub(crate) fn handler_for(name: &str) -> Option<AssetCapHandler> {
        match name {
            CAP_GET_TEXTURE => Some(AssetCapHandler::GetTexture),
            CAP_GET_MESH | CAP_GET_MESH2 => Some(AssetCapHandler::GetMesh),
            CAP_VIEWER_ASSET => Some(AssetCapHandler::ViewerAsset),
            _ => None,
        }
    }

    /// Whether this surface serves the named capability.
    #[must_use]
    pub fn supports(&self, name: &str) -> bool {
        if self.at_the_root {
            ASSET_CAPABILITIES.contains(&name)
        } else {
            self.tokens.contains_key(name)
        }
    }

    /// Grants asset-cap URLs for the requested names — the asset fragment
    /// [`SimCaps::grant`](crate::SimCaps::grant) merges into the seed
    /// response. Unsupported names are silently omitted (feature
    /// negotiation); pure and stable, like its sim-cap sibling.
    #[must_use]
    pub fn grant(&self, requested: &[String]) -> HashMap<String, String> {
        if self.at_the_root {
            // The base with no path, written without the `/` a URL's text
            // ends in: a viewer appends `/?texture_id=…` to what it is given.
            let root = self.base_url.as_str().trim_end_matches('/').to_owned();
            return requested
                .iter()
                .filter(|name| self.supports(name))
                .map(|name| (name.clone(), root.clone()))
                .collect();
        }
        requested
            .iter()
            .filter_map(|name| {
                self.tokens
                    .get(name.as_str())
                    .map(|token| (name.clone(), self.cap_url(*token).to_string()))
            })
            .collect()
    }

    /// Whether `path` resolves to one of this surface's cap tokens. The
    /// in-process HTTP glue uses it to route asset requests here (it holds the
    /// [`AssetSource`]) and everything else through
    /// [`SimCaps::dispatch`](crate::SimCaps::dispatch).
    #[must_use]
    pub fn handles_path(&self, path: &str) -> bool {
        self.resolve(path).is_some()
    }

    /// Serves one asset request from `source`.
    ///
    /// `&self`: asset fetches read only, so nothing mutates. Outcomes: an
    /// unknown URL → `404`; a `POST` → `405` unless the policy answers one; a
    /// request that names no asset → the policy's status; a missing asset →
    /// `404`; a range the asset has bytes for → `206` with the slice and a
    /// `Content-Range` header; otherwise — a range that starts at or past the
    /// end among them — `200` with the whole asset. A `HEAD` is answered as a
    /// `GET` is: the transport sends the headers and keeps the body.
    #[must_use]
    pub fn dispatch(&self, source: &dyn AssetSource, request: &CapsRequest<'_>) -> CapsResponse {
        let Some(name) = self.resolve(request.path) else {
            return CapsResponse::not_found();
        };
        let answered = match request.method {
            "GET" | "HEAD" => true,
            "POST" => self.policy.answers_post,
            _ => false,
        };
        if !answered {
            return CapsResponse::method_not_allowed();
        }
        let named = match Self::handler_for(name) {
            Some(AssetCapHandler::GetTexture) => {
                query_uuid(request.query, CAP_GET_TEXTURE_KEY).map(|id| (id, J2C_CONTENT_TYPE))
            }
            Some(AssetCapHandler::GetMesh) => {
                query_uuid(request.query, CAP_GET_MESH_KEY).map(|id| (id, MESH_CONTENT_TYPE))
            }
            Some(AssetCapHandler::ViewerAsset) => {
                let (refused, status) = self.policy.refused_keys;
                if request.query.is_some_and(|query| {
                    query_pairs(query).any(|(key, _value)| refused.contains(&key))
                }) {
                    return CapsResponse::status(status);
                }
                viewer_asset_uuid(request.query)
            }
            // Tokens are only minted for served capabilities, so a resolved
            // name always has a handler; answer 404 rather than panic if that
            // invariant is ever broken.
            None => return CapsResponse::not_found(),
        };
        let Some((id, content_type)) = named else {
            return CapsResponse::status(self.policy.unreadable_status);
        };
        self.serve_bytes(source, request, AssetKey::from(id), content_type)
    }

    /// Looks `key` up in `source` and applies the request's `Range` (if any):
    /// missing → `404`; whole → `200`; a range → `206`.
    fn serve_bytes(
        &self,
        source: &dyn AssetSource,
        request: &CapsRequest<'_>,
        key: AssetKey,
        content_type: &'static str,
    ) -> CapsResponse {
        let Some(bytes) = source.get(key) else {
            return CapsResponse::not_found();
        };
        let total = bytes.len();
        let validators = if self.policy.validators {
            vec![
                ("accept-ranges", "bytes".to_owned()),
                ("etag", entity_tag(bytes)),
                ("last-modified", LAST_MODIFIED.to_owned()),
            ]
        } else {
            Vec::new()
        };
        if self.policy.validators && request.if_modified_since == Some(LAST_MODIFIED) {
            return CapsResponse::status(304).with_headers(validators);
        }
        let range = request
            .range
            .map_or(ByteRange::Whole, |header| match self.policy.ranges {
                RangeReading::ByteRanges => parse_byte_ranges(header, total),
                RangeReading::OpenSim => parse_opensim_range(header, total),
            });
        let response = match range {
            ByteRange::Whole => CapsResponse::asset_whole(content_type, bytes.to_vec()),
            ByteRange::Partial { start, last } => {
                let slice = bytes.get(start..=last).unwrap_or_default();
                CapsResponse::asset_partial(content_type, slice.to_vec(), start, last, total)
            }
            ByteRange::Several(spans) => {
                CapsResponse::asset_multipart(content_type, multipart(bytes, &spans, content_type))
            }
        };
        response.with_headers(validators)
    }

    /// Resolves a request path to one of this surface's capability names, or
    /// `None`. Matches on the **last** `/cap/<token>` pair (so any base-URL
    /// path prefix works) and ignores any sub-path after the token — mirroring
    /// [`SimCaps`](crate::SimCaps)'s resolver.
    fn resolve(&self, path: &str) -> Option<&'static str> {
        if self.at_the_root {
            // One URL for every class: the generic handler, which reads the
            // class off the query key.
            return matches!(path, "" | "/").then_some(CAP_VIEWER_ASSET);
        }
        let (_, after) = path.rsplit_once("/cap/")?;
        let token_str = after.split_once('/').map_or(after, |(token, _)| token);
        let token = Uuid::parse_str(token_str).ok()?;
        self.tokens
            .iter()
            .find(|(_, minted)| **minted == token)
            .map(|(name, _)| *name)
    }

    /// Mints the URL for one capability token: `{base}/cap/{token}`. Built via
    /// `path_segments_mut` (not `Url::join`, whose trailing-slash semantics
    /// would drop the base's last path segment), exactly like
    /// [`SimCaps`](crate::SimCaps).
    fn cap_url(&self, token: Uuid) -> Url {
        let mut url = self.base_url.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().push("cap").push(&token.to_string());
        }
        url
    }
}

/// The query-parameter name `GetTexture` selects the asset by.
const CAP_GET_TEXTURE_KEY: &str = "texture_id";

/// The query-parameter name `GetMesh` / `GetMesh2` selects the asset by.
const CAP_GET_MESH_KEY: &str = "mesh_id";

/// What a `Range` header asks for, read against a known total asset length.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ByteRange {
    /// No usable range: serve the whole asset (`200`). Covers a missing or
    /// unreadable header, and a range that starts at or past the end of the
    /// asset — which both live grids answer with the whole asset.
    Whole,
    /// One range: serve `start..=last` (inclusive) as `206`.
    Partial {
        /// The first byte offset served.
        start: usize,
        /// The last byte offset served (inclusive), within the asset.
        last: usize,
    },
    /// Several ranges, each first and last: serve them as one `206` with a
    /// multipart body.
    Several(Vec<(usize, usize)>),
}

/// Reads one `start-end`, `start-` or `-suffix` range against `total`, as
/// Second Life's asset hosts do. `None` where it is no range the asset has
/// bytes for.
fn parse_one_range(spec: &str, total: usize) -> Option<(usize, usize)> {
    let (start, end) = spec.trim().split_once('-')?;
    let last_offset = total.checked_sub(1)?;
    if start.trim().is_empty() {
        let suffix = end.trim().parse::<usize>().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((total.saturating_sub(suffix), last_offset));
    }
    let start = start.trim().parse::<usize>().ok()?;
    if start >= total {
        return None;
    }
    let last = match end.trim() {
        "" => last_offset,
        end => end.parse::<usize>().ok()?.min(last_offset),
    };
    (last >= start).then_some((start, last))
}

/// Reads a `Range` header against `total` as Second Life's asset hosts were
/// measured reading it ([`RangeReading::ByteRanges`]).
fn parse_byte_ranges(header: &str, total: usize) -> ByteRange {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return ByteRange::Whole;
    };
    let spans: Option<Vec<(usize, usize)>> = spec
        .split(',')
        .map(|one| parse_one_range(one, total))
        .collect();
    match spans.as_deref() {
        Some([(start, last)]) => ByteRange::Partial {
            start: *start,
            last: *last,
        },
        Some([_, _, ..]) => ByteRange::Several(spans.unwrap_or_default()),
        _ => ByteRange::Whole,
    }
}

/// Reads a `Range` header against `total` as OpenSim's
/// `Util.TryParseHttpRange` and `GetAssetsHandler` do
/// ([`RangeReading::OpenSim`]).
fn parse_opensim_range(header: &str, total: usize) -> ByteRange {
    let Some(spec) = header.strip_prefix("bytes=") else {
        return ByteRange::Whole;
    };
    let parts: Vec<&str> = spec.split('-').collect();
    let [raw_start, raw_end] = parts.as_slice() else {
        return ByteRange::Whole;
    };
    let start = match raw_start.trim() {
        "" => 0,
        text => match text.parse::<usize>() {
            Ok(start) => start,
            Err(_) => return ByteRange::Whole,
        },
    };
    let end = match raw_end.trim() {
        "" => None,
        text => match text.parse::<usize>() {
            // An end that is not above zero is no range at all.
            Ok(end) if end > 0 => Some(end),
            _ => return ByteRange::Whole,
        },
    };
    if start >= total {
        return ByteRange::Whole;
    }
    let last_offset = total.saturating_sub(1);
    let last = end.map_or(last_offset, |end| end.min(last_offset));
    ByteRange::Partial {
        start: start.min(last),
        last,
    }
}

/// The boundary between the parts of a multipart answer.
const MULTIPART_BOUNDARY: &str = "sl-asset-byteranges";

/// The body of an answer to several ranges: each span of `bytes` as a part
/// with its media type and its `Content-Range`.
fn multipart(bytes: &[u8], spans: &[(usize, usize)], content_type: &str) -> Vec<u8> {
    let total = bytes.len();
    let mut body = Vec::new();
    for (start, last) in spans {
        body.extend_from_slice(
            format!(
                "\r\n--{MULTIPART_BOUNDARY}\r\nContent-Type: {content_type}\r\n\
                 Content-Range: bytes {start}-{last}/{total}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes.get(*start..=*last).unwrap_or_default());
    }
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

/// An entity tag for `bytes`: a quoted hash of them (FNV-1a), which is all a
/// tag has to be — the same for the same bytes.
fn entity_tag(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("\"{hash:016x}\"")
}

/// The first `key=<uuid>` value in `query` whose key equals `expected_key`,
/// parsed as a UUID.
fn query_uuid(query: Option<&str>, expected_key: &str) -> Option<Uuid> {
    query_pairs(query?)
        .find(|(key, _)| *key == expected_key)
        .and_then(|(_, value)| Uuid::parse_str(value).ok())
}

/// The first `<class>_id=<uuid>` value in `query` whose key names an
/// [`AssetType`], parsed as a UUID, with the media type of that class — the
/// `ViewerAsset` selector.
fn viewer_asset_uuid(query: Option<&str>) -> Option<(Uuid, &'static str)> {
    query_pairs(query?)
        .find(|(key, _)| AssetType::from_asset_query_key(key).is_some())
        .and_then(|(key, value)| {
            Some((Uuid::parse_str(value).ok()?, viewer_asset_content_type(key)))
        })
}

/// Splits a raw query string (no leading `?`) into its `key`/`value` pairs.
/// Pairs without an `=` are dropped; values are used verbatim (asset ids are
/// UUIDs, never percent-encoded).
fn query_pairs(query: &str) -> impl Iterator<Item = (&str, &str)> {
    query.split('&').filter_map(|pair| pair.split_once('='))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::InMemoryAssetSource;

    /// The test-error type: any assertion helper failure propagates via `?`.
    type TestError = Box<dyn std::error::Error>;

    /// Builds an [`AssetCaps`] with a deterministic token mint, matching the
    /// `SimCaps` test fixture.
    fn caps() -> Result<AssetCaps, TestError> {
        let base: Url = "http://cdn.example/".parse()?;
        let mut next: u128 = 0;
        let mint = move || {
            next = next.wrapping_add(1);
            Uuid::from_u128(next)
        };
        Ok(AssetCaps::new(base, mint))
    }

    /// The granted path for a capability name.
    fn granted_path(caps: &AssetCaps, name: &str) -> Result<String, TestError> {
        let granted = caps.grant(&[name.to_owned()]);
        let url: Url = granted.get(name).ok_or("capability not granted")?.parse()?;
        Ok(url.path().to_owned())
    }

    /// A `GET` asset request with an optional query and `Range`.
    fn get<'a>(path: &'a str, query: Option<&'a str>, range: Option<&'a str>) -> CapsRequest<'a> {
        CapsRequest {
            method: "GET",
            path,
            query,
            range,
            if_modified_since: None,
            destination: None,
            body: b"",
        }
    }

    /// Every asset capability is registered and handled, and none is minted
    /// twice.
    #[test]
    fn every_asset_capability_is_handled() {
        for name in ASSET_CAPABILITIES {
            assert!(
                AssetCaps::handler_for(name).is_some(),
                "asset capability {name:?} has no handler"
            );
        }
    }

    /// One range, as Second Life's hosts read it.
    #[test]
    fn byte_ranges_as_second_life_reads_them() {
        let partial = |start, last| ByteRange::Partial { start, last };
        assert_eq!(parse_byte_ranges("bytes=0-9", 100), partial(0, 9));
        assert_eq!(parse_byte_ranges("bytes=0-0", 100), partial(0, 0));
        // An end past the asset is the asset's end; so is an absent one.
        assert_eq!(parse_byte_ranges("bytes=90-999", 100), partial(90, 99));
        assert_eq!(parse_byte_ranges("bytes=95-", 100), partial(95, 99));
        // A suffix is the last bytes.
        assert_eq!(parse_byte_ranges("bytes=-10", 100), partial(90, 99));
        // Several ranges are several.
        assert_eq!(
            parse_byte_ranges("bytes=0-9,20-29", 100),
            ByteRange::Several(vec![(0, 9), (20, 29)])
        );
        // A start at or past the end, a backwards range, text that is no
        // number and another unit are each no range: the whole asset.
        for header in [
            "bytes=100-110",
            "bytes=110-",
            "bytes=50-40",
            "bytes=x-y",
            "items=0-9",
            "0-9",
        ] {
            assert_eq!(parse_byte_ranges(header, 100), ByteRange::Whole, "{header}");
        }
        assert_eq!(parse_byte_ranges("bytes=0-0", 0), ByteRange::Whole);
    }

    /// One range, as OpenSim's parser reads it — its mistakes included.
    #[test]
    fn byte_ranges_as_opensim_reads_them() {
        let partial = |start, last| ByteRange::Partial { start, last };
        assert_eq!(parse_opensim_range("bytes=0-9", 100), partial(0, 9));
        assert_eq!(parse_opensim_range("bytes=90-999", 100), partial(90, 99));
        assert_eq!(parse_opensim_range("bytes=95-", 100), partial(95, 99));
        // A suffix is read as a range from the start, one byte too long.
        assert_eq!(parse_opensim_range("bytes=-10", 100), partial(0, 10));
        // A backwards range is the one byte at its end.
        assert_eq!(parse_opensim_range("bytes=19-10", 100), partial(10, 10));
        // An end of zero, several ranges, a start at or past the end, text
        // that is no number and another unit: the whole asset.
        for header in [
            "bytes=0-0",
            "bytes=0-9,20-29",
            "bytes=100-110",
            "bytes=110-",
            "bytes=x-y",
            "items=0-9",
        ] {
            assert_eq!(
                parse_opensim_range(header, 100),
                ByteRange::Whole,
                "{header}"
            );
        }
    }

    /// A whole GetTexture fetch answers `200` with the full bytes and the J2C
    /// content type.
    #[test]
    fn get_texture_whole() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xaa);
        let bytes = (0..50_u8).collect::<Vec<u8>>();
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), bytes.clone());
        let path = granted_path(&caps, CAP_GET_TEXTURE)?;
        let query = format!("texture_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), None));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, J2C_CONTENT_TYPE);
        assert_eq!(response.content_range, None);
        assert_eq!(response.body, bytes);
        Ok(())
    }

    /// A ranged fetch answers `206` with the slice and a `Content-Range`.
    #[test]
    fn get_mesh_partial() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xbb);
        let bytes = (0..100_u8).collect::<Vec<u8>>();
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), bytes.clone());
        let path = granted_path(&caps, CAP_GET_MESH)?;
        let query = format!("mesh_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), Some("bytes=10-19")));
        assert_eq!(response.status, 206);
        assert_eq!(response.content_type, MESH_CONTENT_TYPE);
        assert_eq!(response.content_range.as_deref(), Some("bytes 10-19/100"));
        assert_eq!(
            response.body.as_slice(),
            bytes.get(10..=19).unwrap_or_default()
        );
        Ok(())
    }

    /// `GetMesh2` shares the `GetMesh` handler and content type.
    #[test]
    fn get_mesh2_is_handled() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xcc);
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), vec![1, 2, 3]);
        let path = granted_path(&caps, CAP_GET_MESH2)?;
        let query = format!("mesh_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), None));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, MESH_CONTENT_TYPE);
        Ok(())
    }

    /// A `ViewerAsset` fetch classifies by the `<class>_id` query key and
    /// serves that class's media type, the generic one where it has none.
    #[test]
    fn viewer_asset_by_class_key() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xdd);
        let bytes = b"sound-bytes".to_vec();
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), bytes.clone());
        let path = granted_path(&caps, CAP_VIEWER_ASSET)?;
        let query = format!("sound_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), None));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/ogg");
        assert_eq!(response.body, bytes);
        let query = format!("notecard_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), None));
        assert_eq!(response.content_type, OCTET_STREAM_CONTENT_TYPE);
        Ok(())
    }

    /// A range past the end, a missing asset, a request that names none, a
    /// `POST` and an unknown path, as each policy answers them.
    #[test]
    fn what_is_not_there() -> Result<(), TestError> {
        let id = Uuid::from_u128(0xee);
        let bytes = (0..20_u8).collect::<Vec<u8>>();
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), bytes.clone());
        let query = format!("texture_id={id}");
        let opensim = AssetHttpPolicy {
            ranges: RangeReading::OpenSim,
            unreadable_status: 400,
            answers_post: true,
            validators: false,
            refused_keys: (&[], 503),
        };
        for (policy, unreadable, post) in
            [(AssetHttpPolicy::default(), 404, 405), (opensim, 400, 200)]
        {
            let caps = caps()?.with_http_policy(policy);
            let path = granted_path(&caps, CAP_GET_TEXTURE)?;

            // A start at the end of an existing asset → the whole asset.
            let response = caps.dispatch(&source, &get(&path, Some(&query), Some("bytes=20-30")));
            assert_eq!(response.status, 200);
            assert_eq!(response.content_range, None);
            assert_eq!(response.body, bytes);

            // A UUID not in the source → 404.
            let missing = format!("texture_id={}", Uuid::from_u128(0xff));
            assert_eq!(
                caps.dispatch(&source, &get(&path, Some(&missing), None))
                    .status,
                404
            );

            // No query, a key the capability does not know, text that is no id.
            for query in [None, Some("asset_id=1"), Some("texture_id=plywood")] {
                assert_eq!(
                    caps.dispatch(&source, &get(&path, query, None)).status,
                    unreadable,
                    "{query:?}"
                );
            }

            let mut request = get(&path, Some(&query), None);
            request.method = "POST";
            assert_eq!(caps.dispatch(&source, &request).status, post);
            request.method = "HEAD";
            assert_eq!(caps.dispatch(&source, &request).status, 200);
            request.method = "DELETE";
            assert_eq!(caps.dispatch(&source, &request).status, 405);

            // Unknown path → 404.
            let unknown = get(
                "/cap/00000000-0000-0000-0000-0000000000ff",
                Some(&query),
                None,
            );
            assert_eq!(caps.dispatch(&source, &unknown).status, 404);
        }
        Ok(())
    }

    /// A surface at the root grants one bare URL for all four capabilities
    /// and serves an asset of any class from it, by the query key.
    #[test]
    fn a_surface_at_the_root_is_one_url() -> Result<(), TestError> {
        let caps = AssetCaps::at_the_root("http://cdn.example:8080/".parse()?);
        let names: Vec<String> = ASSET_CAPABILITIES
            .iter()
            .map(|name| (*name).to_owned())
            .chain(["EventQueueGet".to_owned()])
            .collect();
        let granted = caps.grant(&names);
        assert_eq!(granted.len(), ASSET_CAPABILITIES.len());
        for name in ASSET_CAPABILITIES {
            assert_eq!(
                granted.get(*name).map(String::as_str),
                Some("http://cdn.example:8080"),
                "{name}"
            );
        }
        let id = Uuid::from_u128(0xad);
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), vec![3_u8; 40]);
        for (key, content_type) in [
            ("texture_id", J2C_CONTENT_TYPE),
            ("mesh_id", MESH_CONTENT_TYPE),
            ("sound_id", "application/ogg"),
        ] {
            let query = format!("{key}={id}");
            let response = caps.dispatch(&source, &get("/", Some(&query), None));
            assert_eq!(response.status, 200, "{key}");
            assert_eq!(response.content_type, content_type);
        }
        assert!(caps.handles_path("/"));
        assert!(!caps.handles_path("/cap/00000000-0000-0000-0000-000000000001"));
        assert!(!caps.handles_path("/get_grid_info"));
        Ok(())
    }

    /// Several ranges are answered with a multipart body under the asset's
    /// own media type, and no `Content-Range` of its own.
    #[test]
    fn several_ranges_are_a_multipart_body() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xab);
        let source = InMemoryAssetSource::new().with_asset(
            AssetKey::from(id),
            b"0123456789abcdefghijklmnopqrstuvwxyz".to_vec(),
        );
        let path = granted_path(&caps, CAP_GET_TEXTURE)?;
        let query = format!("texture_id={id}");
        let response = caps.dispatch(&source, &get(&path, Some(&query), Some("bytes=0-3,20-23")));
        assert_eq!(response.status, 206);
        assert_eq!(response.content_type, J2C_CONTENT_TYPE);
        assert_eq!(response.content_range, None);
        let body = String::from_utf8(response.body)?;
        assert!(
            body.contains("Content-Range: bytes 0-3/36\r\n\r\n0123\r\n"),
            "{body}"
        );
        assert!(
            body.contains("Content-Range: bytes 20-23/36\r\n\r\nklmn\r\n"),
            "{body}"
        );
        Ok(())
    }

    /// A refused key is refused whatever the asset, held or not, while the
    /// same asset under another key is served.
    #[test]
    fn a_refused_key_is_refused_whatever_the_asset() -> Result<(), TestError> {
        let caps = caps()?.with_http_policy(AssetHttpPolicy {
            refused_keys: (&["notecard_id"], 503),
            ..AssetHttpPolicy::default()
        });
        let id = Uuid::from_u128(0xad);
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), vec![1_u8; 4]);
        let path = granted_path(&caps, CAP_VIEWER_ASSET)?;
        let refused = caps.dispatch(
            &source,
            &get(&path, Some(&format!("notecard_id={id}")), None),
        );
        assert_eq!(refused.status, 503);
        let served = caps.dispatch(
            &source,
            &get(&path, Some(&format!("gesture_id={id}")), None),
        );
        assert_eq!(served.status, 200);
        Ok(())
    }

    /// A surface that states validators states them on a whole answer and a
    /// partial one, and answers the date it stated with `304`.
    #[test]
    fn validators_are_stated_and_the_date_honoured() -> Result<(), TestError> {
        let caps = caps()?.with_http_policy(AssetHttpPolicy {
            validators: true,
            ..AssetHttpPolicy::default()
        });
        let id = Uuid::from_u128(0xac);
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), vec![7_u8; 40]);
        let path = granted_path(&caps, CAP_GET_TEXTURE)?;
        let query = format!("texture_id={id}");
        let names = |response: &CapsResponse| {
            response
                .headers
                .iter()
                .map(|(name, _value)| *name)
                .collect::<Vec<_>>()
        };
        let whole = caps.dispatch(&source, &get(&path, Some(&query), None));
        assert_eq!(names(&whole), ["accept-ranges", "etag", "last-modified"]);
        let partial = caps.dispatch(&source, &get(&path, Some(&query), Some("bytes=0-9")));
        assert_eq!(names(&partial), ["accept-ranges", "etag", "last-modified"]);

        let mut request = get(&path, Some(&query), None);
        request.if_modified_since = whole
            .headers
            .iter()
            .find(|(name, _value)| *name == "last-modified")
            .map(|(_name, value)| value.as_str());
        let unchanged = caps.dispatch(&source, &request);
        assert_eq!(unchanged.status, 304);
        assert!(unchanged.body.is_empty());

        // A surface that states none has none to honour.
        let plain = self::caps()?;
        let path = granted_path(&plain, CAP_GET_TEXTURE)?;
        let mut request = get(&path, Some(&query), None);
        request.if_modified_since = Some(LAST_MODIFIED);
        let response = plain.dispatch(&source, &request);
        assert_eq!(response.status, 200);
        assert!(response.headers.is_empty());
        Ok(())
    }
}
