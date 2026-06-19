//! Binary asset fetch over HTTP (textures, mesh, generic assets).

use crate::SlEvent;
use crate::http::{blocking_get_bytes, blocking_get_range};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_proto::Event as SessionEvent;
use sl_proto::{
    Asset, AssetType, DisconnectReason, ImageCodec, Texture, TransferStatus, Uuid, j2c,
};

/// GETs a texture from the `GetTexture` capability and forwards a
/// [`SlSessionEvent::TextureReceived`] or a [`SlSessionEvent::TextureNotFound`]
/// over `asset_tx`. For a non-zero `discard_level` only the level-of-detail
/// prefix is fetched, using HTTP `Range` requests (see [`fetch_texture_lod`]).
pub(crate) fn run_texture_fetch(
    cap_url: &str,
    texture_id: Uuid,
    discard_level: u8,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match fetch_texture_lod(&url, discard_level) {
        Some(data) => SessionEvent::TextureReceived(Box::new(Texture {
            id: texture_id,
            codec: ImageCodec::J2c,
            data,
        })),
        None => SessionEvent::TextureNotFound(texture_id),
    };
    asset_tx.send(event).ok();
}

/// Fetches the codestream bytes for a texture at `discard_level` using HTTP
/// `Range` requests to transfer only the needed LOD prefix: a small probe reads
/// the J2C [`j2c::Header`], from which the prefix length is computed, then a
/// second `Range` request fetches exactly that prefix when the probe did not
/// already cover it. Returns `None` on a 404 / network failure.
pub(crate) fn fetch_texture_lod(url: &str, discard_level: u8) -> Option<Vec<u8>> {
    if discard_level == 0 {
        return blocking_get_bytes(url, None);
    }
    let probe = blocking_get_bytes(url, Some(j2c::FIRST_PACKET_SIZE))?;
    let Some(header) = j2c::parse_header(&probe) else {
        return Some(probe);
    };
    let target = header.discard_data_size(discard_level);
    if probe.len() >= target {
        return Some(probe.get(..target).unwrap_or(&probe).to_vec());
    }
    let body = blocking_get_bytes(url, Some(target))?;
    let size = target.min(body.len());
    Some(body.get(..size).unwrap_or(&body).to_vec())
}

/// GETs an asset from `{cap_url}/{query}` and forwards a
/// [`SlSessionEvent::AssetReceived`] (or a [`SlSessionEvent::AssetTransferFailed`]
/// with the 404-equivalent [`TransferStatus::UnknownSource`]) over `asset_tx`.
/// An inclusive `byte_range` issues an HTTP `Range` request for just that span.
pub(crate) fn run_asset_fetch(
    cap_url: &str,
    query: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/{query}");
    let bytes = match byte_range {
        Some((start, end)) => blocking_get_range(&url, start, end),
        None => blocking_get_bytes(&url, None),
    };
    let event = match bytes {
        Some(data) => SessionEvent::AssetReceived(Box::new(Asset {
            id: asset_id,
            asset_type,
            data,
        })),
        None => SessionEvent::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::UnknownSource,
        },
    };
    asset_tx.send(event).ok();
}

/// GETs a generic asset from the `GetAsset` capability using the asset class's
/// query key, forwarding the result over `asset_tx` (or an
/// [`SlSessionEvent::AssetTransferFailed`] for a class the cap cannot serve). An
/// inclusive `byte_range` issues an HTTP `Range` request for just that span.
pub(crate) fn run_generic_asset_fetch(
    cap_url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    match asset_type.get_asset_query_key() {
        Some(key) => {
            run_asset_fetch(
                cap_url,
                &format!("?{key}={asset_id}"),
                asset_id,
                asset_type,
                byte_range,
                asset_tx,
            );
        }
        None => {
            asset_tx
                .send(SessionEvent::AssetTransferFailed {
                    asset_id,
                    asset_type,
                    status: TransferStatus::Error,
                })
                .ok();
        }
    }
}

/// Emits a disconnect event.
pub(crate) fn emit_disconnect(events: &mut EventWriter<SlEvent>, reason: DisconnectReason) {
    events.write(SlEvent(SessionEvent::Disconnected(reason)));
}
