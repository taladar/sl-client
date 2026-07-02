//! Binary asset fetch over HTTP (textures, mesh, generic assets).

use reqwest::Client as ReqwestClient;
use sl_proto::{
    Asset, AssetType, DiscardLevel, Event, ImageCodec, Texture, TextureKey, TransferStatus, Uuid,
    j2c,
};
use tokio::sync::mpsc;

/// GETs a texture from the `GetTexture` capability and surfaces it as an
/// [`Event::TextureReceived`], or an [`Event::TextureNotFound`] on a 404 /
/// network failure.
///
/// For a non-zero `discard_level` this fetches only the level-of-detail prefix
/// using real HTTP `Range` requests (so the rest of the codestream is never
/// transferred): a small first request reads the J2C [`j2c::Header`], from which
/// the prefix byte length is computed, then a second `Range` request fetches
/// exactly that prefix when the first did not already cover it. A server that
/// ignores `Range` (replying `200` with the whole image) still yields the right
/// prefix, just without the bandwidth saving.
pub(crate) async fn fetch_texture_http(
    cap_url: String,
    texture_id: TextureKey,
    discard_level: DiscardLevel,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match fetch_texture_bytes(&http, &url, discard_level).await {
        Some(data) => Event::TextureReceived(Box::new(Texture {
            id: texture_id,
            codec: ImageCodec::J2c,
            data,
        })),
        None => Event::TextureNotFound(texture_id),
    };
    events.send(event).await.ok();
}

/// Fetches the codestream bytes for a texture at `discard_level`, using HTTP
/// `Range` requests to transfer only the needed LOD prefix. Returns `None` on a
/// 404 / network failure.
pub(crate) async fn fetch_texture_bytes(
    http: &ReqwestClient,
    url: &str,
    discard_level: DiscardLevel,
) -> Option<Vec<u8>> {
    // Full resolution: one plain GET of the entire codestream.
    if discard_level.is_full() {
        return http_get_prefix(http, url, None).await;
    }
    // Probe the header with a small Range request, then size the LOD prefix.
    let probe = http_get_prefix(http, url, Some(j2c::FIRST_PACKET_SIZE)).await?;
    let Some(header) = j2c::parse_header(&probe) else {
        // Not a recognisable J2C codestream: return whatever the probe yielded.
        return Some(probe);
    };
    let target = discard_level.data_size(&header);
    if probe.len() >= target {
        // The probe already covers the prefix (a coarse LOD, or a server that
        // ignored Range and sent the whole image).
        return Some(probe.get(..target).unwrap_or(&probe).to_vec());
    }
    // Fetch exactly the prefix the LOD needs.
    let body = http_get_prefix(http, url, Some(target)).await?;
    let size = target.min(body.len());
    Some(body.get(..size).unwrap_or(&body).to_vec())
}

/// Performs an HTTP `GET` for `url`, optionally requesting only the first
/// `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header. Returns the response
/// body on a success status (`200` or `206`), or `None` on any failure.
pub(crate) async fn http_get_prefix(
    http: &ReqwestClient,
    url: &str,
    max_bytes: Option<usize>,
) -> Option<Vec<u8>> {
    let mut request = http.get(url).header("Accept", "image/x-j2c");
    if let Some(max) = max_bytes {
        request = request.header("Range", format!("bytes=0-{}", max.saturating_sub(1)));
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|bytes| bytes.to_vec())
}

/// GETs a mesh asset from the `GetMesh2`/`GetMesh` capability and surfaces it as
/// an [`Event::AssetReceived`] (or [`Event::AssetTransferFailed`] on failure).
/// An inclusive `byte_range` issues an HTTP `Range` request for just that span.
pub(crate) async fn fetch_mesh_http(
    cap_url: String,
    mesh_id: Uuid,
    byte_range: Option<(u32, u32)>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?mesh_id={mesh_id}");
    let event = http_asset_event(&http, &url, mesh_id, AssetType::Mesh, byte_range).await;
    events.send(event).await.ok();
}

/// GETs a generic asset from the `GetAsset` capability (using the asset class's
/// query parameter) and surfaces it as an [`Event::AssetReceived`] (or
/// [`Event::AssetTransferFailed`] on failure / an unsupported class). An
/// inclusive `byte_range` issues an HTTP `Range` request for just that span.
pub(crate) async fn fetch_asset_http(
    cap_url: String,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = match asset_type.get_asset_query_key() {
        Some(key) => {
            let url = format!("{cap_url}/?{key}={asset_id}");
            http_asset_event(&http, &url, asset_id, asset_type, byte_range).await
        }
        None => Event::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::Error,
        },
    };
    events.send(event).await.ok();
}

/// Performs an HTTP `GET` for an asset and builds the resulting event: an
/// [`Event::AssetReceived`] on success, or an [`Event::AssetTransferFailed`]
/// (with [`TransferStatus::UnknownSource`], the 404 equivalent) on any failure.
/// An inclusive `byte_range` adds a `Range: bytes=start-end` header.
pub(crate) async fn http_asset_event(
    http: &ReqwestClient,
    url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
) -> Event {
    let failed = Event::AssetTransferFailed {
        asset_id,
        asset_type,
        status: TransferStatus::UnknownSource,
    };
    let mut request = http.get(url);
    if let Some((start, end)) = byte_range {
        request = request.header("Range", format!("bytes={start}-{end}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(bytes) => Event::AssetReceived(Box::new(Asset {
                id: asset_id,
                asset_type,
                data: bytes.to_vec(),
            })),
            Err(_error) => failed,
        },
        _ => failed,
    }
}
