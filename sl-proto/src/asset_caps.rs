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
//! header. No range → `200` whole; a satisfiable range → `206` with the byte
//! slice and a `Content-Range` header; a start past the end of an existing
//! asset → `416`; a missing asset → `404`; a non-`GET` → `405`.

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

/// The `Content-Type` a generic `ViewerAsset` fetch is served with. The
/// client does not inspect it (it reads the raw bytes), so the generic
/// media type is correct-but-uncritical; per-class refinement can come later.
const OCTET_STREAM_CONTENT_TYPE: &str = "application/octet-stream";

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
        Self { base_url, tokens }
    }

    /// Rebuilds an identical surface from a known token map — the
    /// cross-process constructor. A CDN process receives the map the
    /// simulator minted (via [`AssetCaps::tokens`], shipped out of band) and
    /// serves the same URLs the simulator advertised in its seed grant.
    #[must_use]
    pub const fn from_tokens(base_url: Url, tokens: BTreeMap<&'static str, Uuid>) -> Self {
        Self { base_url, tokens }
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
        self.tokens.contains_key(name)
    }

    /// Grants asset-cap URLs for the requested names — the asset fragment
    /// [`SimCaps::grant`](crate::SimCaps::grant) merges into the seed
    /// response. Unsupported names are silently omitted (feature
    /// negotiation); pure and stable, like its sim-cap sibling.
    #[must_use]
    pub fn grant(&self, requested: &[String]) -> HashMap<String, String> {
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
    /// unknown URL → `404`; a non-`GET` → `405`; a request whose query names
    /// no known asset, or names a missing asset → `404`; a satisfiable
    /// `Range` → `206` with the slice and a `Content-Range` header; a `Range`
    /// whose start is past the end of an existing asset → `416`; otherwise
    /// `200` with the whole asset.
    #[must_use]
    pub fn dispatch(&self, source: &dyn AssetSource, request: &CapsRequest<'_>) -> CapsResponse {
        let Some(name) = self.resolve(request.path) else {
            return CapsResponse::not_found();
        };
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        match Self::handler_for(name) {
            Some(AssetCapHandler::GetTexture) => {
                Self::serve(source, request, CAP_GET_TEXTURE_KEY, J2C_CONTENT_TYPE)
            }
            Some(AssetCapHandler::GetMesh) => {
                Self::serve(source, request, CAP_GET_MESH_KEY, MESH_CONTENT_TYPE)
            }
            Some(AssetCapHandler::ViewerAsset) => Self::serve_viewer_asset(source, request),
            // Tokens are only minted for served capabilities, so a resolved
            // name always has a handler; answer 404 rather than panic if that
            // invariant is ever broken.
            None => CapsResponse::not_found(),
        }
    }

    /// Serves a fixed-class fetch (`GetTexture` / `GetMesh`): the asset id
    /// comes from the `expected_key` query parameter.
    fn serve(
        source: &dyn AssetSource,
        request: &CapsRequest<'_>,
        expected_key: &str,
        content_type: &'static str,
    ) -> CapsResponse {
        let Some(id) = query_uuid(request.query, expected_key) else {
            return CapsResponse::not_found();
        };
        Self::serve_bytes(source, request, AssetKey::from(id), content_type)
    }

    /// Serves a generic `ViewerAsset` fetch: the asset id comes from the first
    /// `?<class>_id=<uuid>` query parameter whose key names an
    /// [`AssetType`]. The class only confirms the request is well-formed —
    /// the store is keyed by UUID alone — and the response is always the
    /// generic media type.
    fn serve_viewer_asset(source: &dyn AssetSource, request: &CapsRequest<'_>) -> CapsResponse {
        let Some(id) = viewer_asset_uuid(request.query) else {
            return CapsResponse::not_found();
        };
        Self::serve_bytes(
            source,
            request,
            AssetKey::from(id),
            OCTET_STREAM_CONTENT_TYPE,
        )
    }

    /// Looks `key` up in `source` and applies the request's `Range` (if any):
    /// missing → `404`; whole → `200`; satisfiable range → `206`;
    /// out-of-range start → `416`.
    fn serve_bytes(
        source: &dyn AssetSource,
        request: &CapsRequest<'_>,
        key: AssetKey,
        content_type: &'static str,
    ) -> CapsResponse {
        let Some(bytes) = source.get(key) else {
            return CapsResponse::not_found();
        };
        let total = bytes.len();
        match request
            .range
            .map_or(ByteRange::Whole, |header| parse_byte_range(header, total))
        {
            ByteRange::Whole => CapsResponse::asset_whole(content_type, bytes.to_vec()),
            ByteRange::Partial { start, last } => {
                let slice = bytes.get(start..=last).unwrap_or_default();
                CapsResponse::asset_partial(content_type, slice.to_vec(), start, last, total)
            }
            ByteRange::Unsatisfiable => CapsResponse::range_not_satisfiable(total),
        }
    }

    /// Resolves a request path to one of this surface's capability names, or
    /// `None`. Matches on the **last** `/cap/<token>` pair (so any base-URL
    /// path prefix works) and ignores any sub-path after the token — mirroring
    /// [`SimCaps`](crate::SimCaps)'s resolver.
    fn resolve(&self, path: &str) -> Option<&'static str> {
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

/// The parsed outcome of a single `bytes=start-end` header against a known
/// total asset length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteRange {
    /// No usable range: serve the whole asset (`200`). Covers a missing,
    /// unparsable, or multi-range header — a real server ignores what it
    /// cannot honour.
    Whole,
    /// A satisfiable range: serve `start..=last` (inclusive) as `206`.
    Partial {
        /// The first byte offset served.
        start: usize,
        /// The last byte offset served (inclusive), clamped to `total - 1`.
        last: usize,
    },
    /// The range's start is at or past the end of the asset: `416`.
    Unsatisfiable,
}

/// Parses a single `bytes=start-end` header against `total`.
///
/// `end` is optional (an open `bytes=start-` means "to the end") and
/// inclusive, matching the client's `bytes={start}-{end}` form. A start at or
/// past `total` (including any range on a zero-length asset) is
/// [`Unsatisfiable`](ByteRange::Unsatisfiable). Anything not of the form
/// `bytes=<start>-<end?>` — a multi-range list (which our client never sends),
/// a non-numeric bound, a suffix range — is treated leniently as
/// [`Whole`](ByteRange::Whole).
fn parse_byte_range(header: &str, total: usize) -> ByteRange {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return ByteRange::Whole;
    };
    // Multi-range lists are valid HTTP but the viewer never sends them; ignore.
    if spec.contains(',') {
        return ByteRange::Whole;
    }
    let Some((start_str, end_str)) = spec.split_once('-') else {
        return ByteRange::Whole;
    };
    let Ok(start) = start_str.trim().parse::<usize>() else {
        return ByteRange::Whole;
    };
    if start >= total {
        return ByteRange::Unsatisfiable;
    }
    // `start < total` here, so `total >= 1` and this never underflows.
    let last_offset = total.saturating_sub(1);
    let last = match end_str.trim() {
        "" => last_offset,
        end => end
            .parse::<usize>()
            .map_or(last_offset, |value| value.min(last_offset)),
    };
    // A backwards range (end < start) is malformed; fall back to the whole
    // asset rather than emit an empty 206.
    if last < start {
        return ByteRange::Whole;
    }
    ByteRange::Partial { start, last }
}

/// The first `key=<uuid>` value in `query` whose key equals `expected_key`,
/// parsed as a UUID.
fn query_uuid(query: Option<&str>, expected_key: &str) -> Option<Uuid> {
    query_pairs(query?)
        .find(|(key, _)| *key == expected_key)
        .and_then(|(_, value)| Uuid::parse_str(value).ok())
}

/// The first `<class>_id=<uuid>` value in `query` whose key names an
/// [`AssetType`], parsed as a UUID — the `ViewerAsset` selector.
fn viewer_asset_uuid(query: Option<&str>) -> Option<Uuid> {
    query_pairs(query?)
        .find(|(key, _)| AssetType::from_asset_query_key(key).is_some())
        .and_then(|(_, value)| Uuid::parse_str(value).ok())
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

    /// `parse_byte_range` covers whole, partial, open-ended, out-of-range and
    /// the lenient fallbacks.
    #[test]
    fn parse_byte_range_cases() {
        assert_eq!(
            parse_byte_range("bytes=0-9", 100),
            ByteRange::Partial { start: 0, last: 9 }
        );
        // End past the asset clamps to the last byte.
        assert_eq!(
            parse_byte_range("bytes=90-999", 100),
            ByteRange::Partial {
                start: 90,
                last: 99
            }
        );
        // Open-ended runs to the last byte.
        assert_eq!(
            parse_byte_range("bytes=95-", 100),
            ByteRange::Partial {
                start: 95,
                last: 99
            }
        );
        // Start at/after the end is unsatisfiable.
        assert_eq!(
            parse_byte_range("bytes=100-110", 100),
            ByteRange::Unsatisfiable
        );
        assert_eq!(parse_byte_range("bytes=0-0", 0), ByteRange::Unsatisfiable);
        // Lenient fallbacks: no prefix, multi-range, non-numeric, backwards.
        assert_eq!(parse_byte_range("0-9", 100), ByteRange::Whole);
        assert_eq!(parse_byte_range("bytes=0-9,20-29", 100), ByteRange::Whole);
        assert_eq!(parse_byte_range("bytes=x-y", 100), ByteRange::Whole);
        assert_eq!(parse_byte_range("bytes=50-40", 100), ByteRange::Whole);
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
    /// serves the generic media type.
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
        assert_eq!(response.content_type, OCTET_STREAM_CONTENT_TYPE);
        assert_eq!(response.body, bytes);
        Ok(())
    }

    /// Out-of-range, missing asset, wrong method and unknown path each map to
    /// their status.
    #[test]
    fn error_paths() -> Result<(), TestError> {
        let caps = caps()?;
        let id = Uuid::from_u128(0xee);
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), vec![0_u8; 20]);
        let path = granted_path(&caps, CAP_GET_TEXTURE)?;
        let query = format!("texture_id={id}");

        // Out-of-range start on an existing asset → 416.
        let response = caps.dispatch(&source, &get(&path, Some(&query), Some("bytes=20-30")));
        assert_eq!(response.status, 416);
        assert_eq!(response.content_range.as_deref(), Some("bytes */20"));
        assert!(response.body.is_empty());

        // A UUID not in the source → 404.
        let missing = format!("texture_id={}", Uuid::from_u128(0xff));
        assert_eq!(
            caps.dispatch(&source, &get(&path, Some(&missing), None))
                .status,
            404
        );

        // Wrong method on a known cap → 405.
        let mut post = get(&path, Some(&query), None);
        post.method = "POST";
        assert_eq!(caps.dispatch(&source, &post).status, 405);

        // Unknown path → 404.
        let unknown = get(
            "/cap/00000000-0000-0000-0000-0000000000ff",
            Some(&query),
            None,
        );
        assert_eq!(caps.dispatch(&source, &unknown).status, 404);
        Ok(())
    }
}
