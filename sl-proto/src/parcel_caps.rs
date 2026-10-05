//! The `ParcelPropertiesUpdate` capability body: a parcel edit sent as LLSD.
//!
//! The reference viewer POSTs the parcel here whenever the region grants the
//! capability (`LLViewerParcelMgr::sendParcelPropertiesUpdate`), and falls
//! back to the UDP `ParcelPropertiesUpdate` only without it. The body is
//! `LLParcel::packMessage(LLSD&)`: the **whole** record, including what the UDP
//! block cannot carry — the media type, size and looping, the shared-browsing
//! settings, who can see and hear avatars, and whether outside prim media is
//! hidden. Both grids grant it (`book/src/gridspec/capabilities.md`).
//!
//! [`build_parcel_properties_update_request`] writes it the way the reference
//! does, and [`parse_parcel_properties_update_request`] reads it back for a
//! server. A field the [`ParcelUpdate`] leaves `None` — one the client never
//! learned from the grid — is written with the reference's own default, as
//! `packMessage` always writes every field.

use sl_types::key::{AgentKey, GroupKey, TextureKey};
use sl_types::money::LindenAmount;
use sl_wire::{Llsd, RegionLocalParcelId};
use uuid::Uuid;

use crate::session::conversions::{
    direction_from_llsd, direction_to_llsd, llsd_map, llsd_u32, region_coords_from_llsd,
    region_coords_to_llsd,
};
use crate::types::{ParcelCategory, ParcelMediaData, ParcelMediaSharing, ParcelUpdate};

/// The `flags` the reference sends with every update: "request the new
/// properties back" (`0x01`).
const REQUEST_PROPERTIES_BACK: u32 = 0x01;

/// A `u32` as the reference's `ll_sd_from_U32` writes it: four big-endian bytes
/// of binary.
fn u32_binary(value: u32) -> Llsd {
    Llsd::Binary(
        [24_u32, 16, 8, 0]
            .iter()
            .map(|shift| u8::try_from(value.checked_shr(*shift).unwrap_or(0) & 0xff).unwrap_or(0))
            .collect(),
    )
}

/// `key` as an LLSD id, nil for `None`.
fn optional_uuid(key: Option<Uuid>) -> Llsd {
    Llsd::Uuid(key.unwrap_or_else(Uuid::nil))
}

/// Builds the `ParcelPropertiesUpdate` capability body for `update`, as the
/// reference viewer's `LLParcel::packMessage(LLSD&)` writes it.
#[must_use]
pub fn build_parcel_properties_update_request(update: &ParcelUpdate) -> String {
    let media = update
        .media_data
        .clone()
        .unwrap_or_else(ParcelMediaData::legacy_default);
    let sharing = update.media_sharing.clone().unwrap_or_default();
    let money = |amount: &LindenAmount| Llsd::Integer(i32::try_from(amount.0).unwrap_or(i32::MAX));
    llsd_map(vec![
        ("flags", u32_binary(REQUEST_PROPERTIES_BACK)),
        ("local_id", Llsd::Integer(update.local_id.0)),
        ("parcel_flags", u32_binary(update.parcel_flags.bits())),
        (
            "sale_price",
            update.sale_price.as_ref().map_or(Llsd::Integer(0), money),
        ),
        ("name", Llsd::String(update.name.clone())),
        ("description", Llsd::String(update.description.clone())),
        (
            "music_url",
            Llsd::String(sl_wire::optional_url_to_wire(update.music_url.as_ref())),
        ),
        (
            "media_url",
            Llsd::String(sl_wire::optional_url_to_wire(update.media_url.as_ref())),
        ),
        ("media_desc", Llsd::String(media.description)),
        ("media_type", Llsd::String(media.media_type)),
        ("media_width", Llsd::Integer(media.width)),
        ("media_height", Llsd::Integer(media.height)),
        (
            "auto_scale",
            Llsd::Integer(i32::from(update.media_auto_scale)),
        ),
        ("media_loop", Llsd::Integer(i32::from(media.looping))),
        ("media_current_url", Llsd::String(sharing.current_url)),
        // Obsolete in the reference, which always sends `false`.
        ("obscure_media", Llsd::Boolean(false)),
        ("obscure_music", Llsd::Boolean(false)),
        (
            "media_id",
            optional_uuid(update.media_id.map(|key| key.uuid())),
        ),
        (
            "media_allow_navigate",
            Llsd::Integer(i32::from(sharing.allow_navigate)),
        ),
        (
            "media_prevent_camera_zoom",
            Llsd::Integer(i32::from(sharing.prevent_camera_zoom)),
        ),
        (
            "media_url_timeout",
            Llsd::Real(f64::from(sharing.url_timeout)),
        ),
        (
            "group_id",
            optional_uuid(update.group_id.map(|key| key.uuid())),
        ),
        ("pass_price", money(&update.pass_price)),
        ("pass_hours", Llsd::Real(f64::from(update.pass_hours))),
        (
            "category",
            Llsd::Integer(i32::from(update.category.to_u8())),
        ),
        (
            "auth_buyer_id",
            optional_uuid(update.auth_buyer_id.map(|key| key.uuid())),
        ),
        (
            "snapshot_id",
            optional_uuid(update.snapshot_id.map(|key| key.uuid())),
        ),
        ("user_location", region_coords_to_llsd(update.user_location)),
        ("user_look_at", direction_to_llsd(update.user_look_at)),
        (
            "landing_type",
            Llsd::Integer(i32::from(update.landing_type)),
        ),
        // The reference defaults all three to `true` ("legacy server
        // behaviour") when the grid never sent them.
        ("see_avs", Llsd::Boolean(update.see_avs.unwrap_or(true))),
        (
            "group_av_sounds",
            Llsd::Boolean(update.group_av_sounds.unwrap_or(true)),
        ),
        (
            "any_av_sounds",
            Llsd::Boolean(update.any_av_sounds.unwrap_or(true)),
        ),
        (
            "obscure_moap",
            Llsd::Boolean(update.obscure_moap.unwrap_or(false)),
        ),
    ])
    .to_llsd_xml()
}

/// Why a `ParcelPropertiesUpdate` capability body could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParcelUpdateBodyError {
    /// The body is not an LLSD map.
    #[error("the parcel update body is not a map")]
    NotAMap,
    /// The body names no parcel.
    #[error("the parcel update body has no local_id")]
    NoLocalId,
    /// A field holds a value no client sends (a negative price, an
    /// unparsable URL).
    #[error("the parcel update field {field} is malformed")]
    Malformed {
        /// The offending key.
        field: &'static str,
    },
}

/// Reads a `ParcelPropertiesUpdate` capability body back into the update it
/// carries — the server side of [`build_parcel_properties_update_request`].
/// Every field this body form carries comes back `Some`.
///
/// # Errors
///
/// Returns [`ParcelUpdateBodyError`] for a body that is not a map, names no
/// parcel, or carries a malformed price or URL.
pub fn parse_parcel_properties_update_request(
    body: &Llsd,
) -> Result<ParcelUpdate, ParcelUpdateBodyError> {
    let Llsd::Map(map) = body else {
        return Err(ParcelUpdateBodyError::NotAMap);
    };
    let int = |key: &str| map.get(key).and_then(Llsd::as_i32).unwrap_or(0);
    let flag = |key: &str| map.get(key).and_then(Llsd::as_bool).unwrap_or(false);
    let real = |key: &str| map.get(key).and_then(Llsd::as_f32).unwrap_or(0.0);
    let text = |key: &str| {
        map.get(key)
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let id = |key: &str| {
        map.get(key)
            .and_then(Llsd::as_uuid)
            .filter(|uuid| !uuid.is_nil())
    };
    let amount = |field: &'static str| {
        u64::try_from(int(field))
            .map(LindenAmount)
            .map_err(|_negative| ParcelUpdateBodyError::Malformed { field })
    };
    let url = |field: &'static str| {
        sl_wire::optional_url_from_wire(field, &text(field))
            .map_err(|_unparsable| ParcelUpdateBodyError::Malformed { field })
    };
    let local_id = map
        .get("local_id")
        .and_then(Llsd::as_i32)
        .ok_or(ParcelUpdateBodyError::NoLocalId)?;
    let parcel_flags = sl_wire::ParcelFlags::from_bits(map.get("parcel_flags").map_or(0, llsd_u32));
    let for_sale = parcel_flags.contains(sl_wire::ParcelFlags::FOR_SALE);
    Ok(ParcelUpdate {
        local_id: RegionLocalParcelId(local_id),
        parcel_flags,
        sale_price: if for_sale {
            Some(amount("sale_price")?)
        } else {
            None
        },
        name: text("name"),
        description: text("description"),
        music_url: url("music_url")?,
        media_url: url("media_url")?,
        media_id: id("media_id").map(TextureKey::from),
        media_auto_scale: flag("auto_scale"),
        group_id: id("group_id").map(GroupKey::from),
        pass_price: amount("pass_price")?,
        pass_hours: real("pass_hours"),
        category: ParcelCategory::from_u8(u8::try_from(int("category")).unwrap_or(0)),
        auth_buyer_id: id("auth_buyer_id").map(AgentKey::from),
        snapshot_id: id("snapshot_id").map(TextureKey::from),
        user_location: region_coords_from_llsd(map.get("user_location")),
        user_look_at: direction_from_llsd(map.get("user_look_at")),
        landing_type: u8::try_from(int("landing_type")).unwrap_or(0),
        media_data: Some(ParcelMediaData {
            description: text("media_desc"),
            media_type: text("media_type"),
            width: int("media_width"),
            height: int("media_height"),
            looping: flag("media_loop"),
        }),
        media_sharing: Some(ParcelMediaSharing {
            current_url: text("media_current_url"),
            allow_navigate: flag("media_allow_navigate"),
            prevent_camera_zoom: flag("media_prevent_camera_zoom"),
            url_timeout: real("media_url_timeout"),
        }),
        see_avs: Some(flag("see_avs")),
        any_av_sounds: Some(flag("any_av_sounds")),
        group_av_sounds: Some(flag("group_av_sounds")),
        obscure_moap: Some(flag("obscure_moap")),
    })
}

#[cfg(test)]
mod tests {
    use super::{build_parcel_properties_update_request, parse_parcel_properties_update_request};
    use crate::types::{ParcelCategory, ParcelMediaData, ParcelMediaSharing, ParcelUpdate};
    use pretty_assertions::assert_eq;
    use sl_types::money::LindenAmount;
    use sl_wire::RegionLocalParcelId;

    /// An update every field of which is set, round-tripped through the body:
    /// what the server reads is what the client meant, the fields the UDP
    /// block cannot carry included.
    #[test]
    fn a_full_update_round_trips() -> Result<(), String> {
        let update = ParcelUpdate {
            local_id: RegionLocalParcelId(7),
            parcel_flags: sl_wire::ParcelFlags::from_bits(0x0400_0001),
            name: "Garden".to_owned(),
            description: "Flowers".to_owned(),
            media_url: Some(
                "https://example.com/stream"
                    .parse()
                    .map_err(|error| format!("{error}"))?,
            ),
            pass_price: LindenAmount(25),
            pass_hours: 2.5,
            category: ParcelCategory::from_u8(3),
            landing_type: 2,
            media_data: Some(ParcelMediaData {
                description: "A stream".to_owned(),
                media_type: "text/html".to_owned(),
                width: 1024,
                height: 768,
                looping: false,
            }),
            media_sharing: Some(ParcelMediaSharing {
                current_url: "https://example.com/now".to_owned(),
                allow_navigate: true,
                prevent_camera_zoom: true,
                url_timeout: 30.0,
            }),
            see_avs: Some(false),
            any_av_sounds: Some(true),
            group_av_sounds: Some(false),
            obscure_moap: Some(true),
            ..ParcelUpdate::default()
        };
        let body = sl_wire::parse_llsd_xml(&build_parcel_properties_update_request(&update))
            .map_err(|error| format!("{error}"))?;
        let parsed =
            parse_parcel_properties_update_request(&body).map_err(|error| format!("{error}"))?;
        assert_eq!(parsed, update);
        Ok(())
    }

    /// A field the client never learned is written with the reference's
    /// default — never left out, since the server reads a missing key as
    /// its own default and the edit would then reset it.
    #[test]
    fn unknown_fields_take_the_reference_defaults() -> Result<(), String> {
        let update = ParcelUpdate {
            local_id: RegionLocalParcelId(1),
            ..ParcelUpdate::default()
        };
        let body = sl_wire::parse_llsd_xml(&build_parcel_properties_update_request(&update))
            .map_err(|error| format!("{error}"))?;
        let parsed =
            parse_parcel_properties_update_request(&body).map_err(|error| format!("{error}"))?;
        assert_eq!(
            parsed.media_data.map(|media| media.media_type),
            Some("video/vnd.secondlife.qt.legacy".to_owned())
        );
        assert_eq!(parsed.see_avs, Some(true));
        assert_eq!(parsed.obscure_moap, Some(false));
        Ok(())
    }
}
