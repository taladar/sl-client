//! How a grid's `NewFileAgentInventory` answers: which classes it takes, what
//! it charges, what it refuses and in which words.
//!
//! The two live grids agree on the shape of an upload — a first request
//! answered with an `uploader`, the bytes posted there — and on almost
//! nothing else. Both were measured on 2026-10-13 by the conformance case
//! `upload-http` and are written up in `book/src/gridspec/assets.md`
//! § Uploads; [`NewFilePolicy`] says which of them a [`SimCaps`](crate::SimCaps)
//! surface is.
//!
//! - **Second Life** takes five classes (a texture or snapshot, a sound, an
//!   animation, a body part, clothing — and a mesh, whose costing is not
//!   modelled here) and charges for each. The first request is checked for
//!   its fields and against the balance, and answered with the price and the
//!   balance; the **bytes** are what the stated `expected_upload_cost` is
//!   held to, because a texture's price depends on its size. The bytes are
//!   validated, a refusal costs nothing, and an uploader serves once.
//! - **OpenSim** takes every class and charges nothing. It validates
//!   nothing, files a class it has no name for as a texture, and its
//!   uploader does not come back from an object that is no mesh model: every
//!   later request of the session is answered "busy".
//!
//! [`NewFilePolicy::Plain`] is neither: the minimal uploader the surface
//! served before either grid was measured, kept as the default so a test
//! that wants an upload and no grid gets one.

use std::collections::HashMap;

use sl_wire::{Llsd, NewFileAgentInventoryRequest, parse_llsd_xml};
use uuid::Uuid;

use crate::MIN_2K_TEXTURE_AREA;
use crate::sim_caps::CapsResponse;

/// Which grid's `NewFileAgentInventory` a surface serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewFilePolicy {
    /// Neither grid's: every class taken, nothing charged, nothing checked,
    /// `{ state, uploader }` and `{ state, new_asset, new_inventory_item }`.
    #[default]
    Plain,
    /// Second Life's, charging `prices`.
    SecondLife(UploadPrices),
    /// OpenSim's.
    OpenSim,
}

/// What a Second-Life-flavoured grid charges for an upload, in L$.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadPrices {
    /// A texture below [`MIN_2K_TEXTURE_AREA`] texels, a body part and
    /// clothing.
    pub texture: i32,
    /// A texture of [`MIN_2K_TEXTURE_AREA`] texels or more.
    pub large_texture: i32,
    /// A sound.
    pub sound: i32,
    /// An animation.
    pub animation: i32,
}

impl Default for UploadPrices {
    /// The prices aditi quoted a basic account on 2026-10-13.
    fn default() -> Self {
        Self {
            texture: 10,
            large_texture: 50,
            sound: 10,
            animation: 10,
        }
    }
}

/// The longest sound Second Life takes, in seconds.
const LONGEST_SOUND_SECS: u64 = 30;

/// The mask a Second Life completion states for the base and owner
/// permissions of the item it made.
const FULL_MASK: i32 = i32::MAX;

/// The page OpenSim answers a request it has no handler for with: its
/// title. The page proper is a paragraph of the server's own humour, which
/// nothing reads.
pub(crate) const OPENSIM_NOT_FOUND: &str =
    "<html><head><title>404 Page not found</title></head><body></body></html>";

/// The first request of a two-step upload that is not an asset upload,
/// parked until its bytes arrive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SideUpload {
    /// An `InventoryThumbnailUpload`, and what it named — `None` for a
    /// request that named nothing, which is handed an uploader all the same.
    Thumbnail(Option<crate::ThumbnailTarget>),
    /// A `SendPostcard`, and its card.
    Postcard(Box<crate::Postcard>),
    /// An `UploadAgentProfileImage`, and the profile field it named — `None`
    /// for a request that named none.
    ProfileImage(Option<crate::ProfileImage>),
}

/// The widest and the tallest a thumbnail may be, in pixels.
const LARGEST_THUMBNAIL: u32 = 256;

/// Why Second Life's thumbnail uploader refuses `data`, where it does.
///
/// The words are its verifier's, and the same as for a texture: an image
/// wider than [`LARGEST_THUMBNAIL`] is "too large", and neither a small one
/// nor one that is not square is refused.
pub(crate) fn thumbnail_refusal(data: &[u8]) -> Option<&'static str> {
    if data.is_empty() {
        return Some("File is empty FILE: /tmp/upload/thumbnail");
    }
    let Some(header) = crate::j2c::parse_header(data) else {
        return Some("LLKDUMessageError::flush()");
    };
    if header.width > LARGEST_THUMBNAIL {
        return Some("Invalid width: Value too large.");
    }
    if header.height > LARGEST_THUMBNAIL {
        return Some("Invalid height: Value too large.");
    }
    None
}

/// The answer of a side upload's second step that failed: `{ message, state:
/// "failure" }`, with no `error` beside it.
pub(crate) fn side_upload_failure(message: &str) -> CapsResponse {
    CapsResponse::llsd_xml_status(
        200,
        map([("message", text(message)), ("state", text("failure"))]).to_llsd_xml(),
    )
}

/// Second Life's answer to a method a capability does not take: `501`, and
/// a page saying so.
pub(crate) fn not_implemented() -> CapsResponse {
    CapsResponse::html(
        501,
        "<html><body><p>message</p><p>Not implemented</p><p>error</p><p>True</p></body></html>",
    )
}

/// Second Life's answer to a thumbnail for nothing it can find: `500`, and
/// its handler's stack trace — which this one does not have, and says so.
pub(crate) fn server_error() -> CapsResponse {
    CapsResponse::html(500, "<html><body><p>stack-trace</p></body></html>")
}

/// The answer to a request an uploader that is spent, or never was, gets on
/// Second Life.
pub(crate) fn spent_uploader(ticket: u128) -> CapsResponse {
    CapsResponse::words(
        404,
        &format!("cap not found: '{}'", Uuid::from_u128(ticket)),
    )
}

/// Whether a session's `NewFileAgentInventory` uploader still takes requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum UploaderState {
    /// It does.
    #[default]
    Ready,
    /// It answers every request "busy", as OpenSim's does after an object
    /// that is no mesh model, until the agent logs in again.
    Stuck,
}

/// What the first request of an upload was found to be.
#[derive(Debug)]
pub(crate) enum FirstStep {
    /// Refused: the answer to send.
    Refused(CapsResponse),
    /// Taken: park `request`, and answer with an uploader, stating
    /// `upload_price` (and the balance, on Second Life).
    Taken {
        /// The request, read leniently.
        request: NewFileAgentInventoryRequest,
        /// What the bytes are held to.
        expected: Expected,
        /// The price the first answer states.
        upload_price: i32,
    },
}

/// What the first request said the upload would cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expected {
    /// Nothing: the request carried no `expected_upload_cost`.
    Unstated,
    /// This much.
    Stated(i32),
}

/// A map of `entries`.
fn map<const N: usize>(entries: [(&str, Llsd); N]) -> Llsd {
    Llsd::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// An LLSD text.
fn text(value: &str) -> Llsd {
    Llsd::String(value.to_owned())
}

/// A Second Life refusal: `{ error: { identifier, message, … }, message,
/// state: "failure" }` under `status`.
pub(crate) fn second_life_refusal(
    status: u16,
    identifier: &str,
    message: &str,
    more: Vec<(&str, Llsd)>,
) -> CapsResponse {
    let mut error: HashMap<String, Llsd> = HashMap::from([
        ("identifier".to_owned(), text(identifier)),
        ("message".to_owned(), text(message)),
    ]);
    error.extend(more.into_iter().map(|(key, value)| (key.to_owned(), value)));
    CapsResponse::llsd_xml_status(
        status,
        map([
            ("error", Llsd::Map(error)),
            ("message", text(message)),
            ("state", text("failure")),
        ])
        .to_llsd_xml(),
    )
}

/// An OpenSim refusal: `{ error: { identifier: nil, message }, state:
/// "error", upload_price: 0, uploader: "" }`.
pub(crate) fn open_sim_refusal(message: &str) -> CapsResponse {
    CapsResponse::llsd_xml_status(
        200,
        map([
            (
                "error",
                map([
                    ("identifier", Llsd::Uuid(Uuid::nil())),
                    ("message", text(message)),
                ]),
            ),
            ("state", text("error")),
            ("upload_price", Llsd::Integer(0)),
            ("uploader", text("")),
        ])
        .to_llsd_xml(),
    )
}

/// What OpenSim says of every request once its uploader is stuck.
pub(crate) const OPENSIM_BUSY: &str = "Uploader busy processing previous request";

/// The classes Second Life's uploader takes, each with the inventory classes
/// it may be filed as.
const SECOND_LIFE_CLASSES: &[(&str, &[&str])] = &[
    ("texture", &["texture", "snapshot"]),
    ("sound", &["sound"]),
    ("animatn", &["animation"]),
    ("bodypart", &["wearable"]),
    ("clothing", &["wearable"]),
    ("mesh", &["object"]),
];

/// The classes Second Life's uploader names as not its own; any other it
/// does not take is answered as a server error.
const SECOND_LIFE_OTHER_UPLOADERS: &[&str] =
    &["notecard", "lsltext", "gesture", "settings", "material"];

/// Reads the first request of an upload as `policy`'s grid does. `balance`
/// is the agent's, where the grid keeps one.
pub(crate) fn first_step(
    policy: NewFilePolicy,
    method: &str,
    body: &[u8],
    balance: Option<i64>,
) -> FirstStep {
    match policy {
        NewFilePolicy::Plain => plain_first_step(method, body),
        NewFilePolicy::OpenSim => open_sim_first_step(method, body),
        NewFilePolicy::SecondLife(prices) => second_life_first_step(prices, method, body, balance),
    }
}

/// The request as the lenient parser reads it.
fn lenient(root: &Llsd) -> NewFileAgentInventoryRequest {
    sl_wire::parse_new_file_agent_inventory_request(&root.to_llsd_xml()).unwrap_or_else(|_error| {
        NewFileAgentInventoryRequest {
            folder_id: crate::InventoryFolderKey::from(Uuid::nil()),
            asset_type: String::new(),
            inventory_type: String::new(),
            name: String::new(),
            description: String::new(),
            next_owner_mask: 0,
            group_mask: 0,
            everyone_mask: 0,
            expected_upload_cost: 0,
        }
    })
}

/// What `root` states as its `expected_upload_cost`.
fn expected_of(root: &Llsd) -> Expected {
    match root.get("expected_upload_cost").and_then(Llsd::as_i32) {
        Some(cost) => Expected::Stated(cost),
        None => Expected::Unstated,
    }
}

/// The first step of the plain uploader.
fn plain_first_step(method: &str, body: &[u8]) -> FirstStep {
    if method != "POST" {
        return FirstStep::Refused(CapsResponse::method_not_allowed());
    }
    let Some(root) = std::str::from_utf8(body)
        .ok()
        .and_then(|text| parse_llsd_xml(text).ok())
    else {
        return FirstStep::Refused(CapsResponse::status(400));
    };
    FirstStep::Taken {
        request: lenient(&root),
        expected: Expected::Unstated,
        upload_price: 0,
    }
}

/// The first step of OpenSim's uploader.
fn open_sim_first_step(method: &str, body: &[u8]) -> FirstStep {
    if method != "POST" {
        return FirstStep::Refused(CapsResponse::html(404, OPENSIM_NOT_FOUND));
    }
    let root = std::str::from_utf8(body)
        .ok()
        .and_then(|text| parse_llsd_xml(text).ok());
    let Some(root @ Llsd::Map(_)) = root else {
        return FirstStep::Refused(CapsResponse::status(500));
    };
    let request = lenient(&root);
    if request.asset_type == "mesh" {
        let has = |key: &str| {
            root.get("asset_resources")
                .and_then(|resources| resources.get(key))
                .and_then(Llsd::as_array)
                .is_some_and(|entries| !entries.is_empty())
        };
        if !(has("mesh_list") && has("instance_list")) {
            return FirstStep::Refused(open_sim_refusal("missing model information."));
        }
    }
    FirstStep::Taken {
        request,
        expected: Expected::Unstated,
        upload_price: 0,
    }
}

/// The first step of Second Life's uploader.
fn second_life_first_step(
    prices: UploadPrices,
    method: &str,
    body: &[u8],
    balance: Option<i64>,
) -> FirstStep {
    let refused = |identifier: &str, message: &str| {
        FirstStep::Refused(second_life_refusal(200, identifier, message, Vec::new()))
    };
    if method != "POST" {
        return FirstStep::Refused(not_implemented());
    }
    if body.is_empty() {
        return refused(
            "Upload_BadParameterType",
            "Parameter 'body' is <type 'bool'>, expected <type 'dict'>.",
        );
    }
    let Some(root) = std::str::from_utf8(body)
        .ok()
        .and_then(|text| parse_llsd_xml(text).ok())
    else {
        return refused("Upload_InvalidRequestBody", "Invalid request bodyxxx");
    };
    for parameter in ["folder_id", "asset_type", "inventory_type", "name"] {
        if root.get(parameter).is_none() {
            return refused(
                "Upload_MissingParameter",
                &format!("Missing parameter '{parameter}'"),
            );
        }
    }
    if let Some(cost) = root.get("expected_upload_cost")
        && cost.as_i32().is_none()
    {
        return refused(
            "Upload_BadParameterType",
            "Parameter 'expected_upload_cost' is <type 'str'>, expected <type 'int'>.",
        );
    }
    let request = lenient(&root);
    let Some((_class, filed_as)) = SECOND_LIFE_CLASSES
        .iter()
        .find(|(class, _filed_as)| *class == request.asset_type)
    else {
        if SECOND_LIFE_OTHER_UPLOADERS.contains(&request.asset_type.as_str()) {
            return refused(
                "Upload_InvalidAssetType",
                &format!("Invalid asset type: {}", request.asset_type),
            );
        }
        return FirstStep::Refused(second_life_refusal(
            500,
            "Upload_ServerError",
            "Unable to upload asset.",
            Vec::new(),
        ));
    };
    if request.asset_type == "mesh"
        && let Some(refused) = second_life_model_refusal(&root)
    {
        return FirstStep::Refused(refused);
    }
    if !filed_as.contains(&request.inventory_type.as_str()) {
        let listed: Vec<String> = filed_as.iter().map(|class| format!("'{class}'")).collect();
        let message = format!(
            "invalid inventory type: {} must be in: [{}]",
            request.inventory_type,
            listed.join(", ")
        );
        return FirstStep::Refused(second_life_refusal(
            200,
            "Upload_InvalidInventoryType",
            &message,
            vec![(
                "valid_inventory_types",
                Llsd::Array(filed_as.iter().map(|class| text(class)).collect()),
            )],
        ));
    }
    // The price the first answer states is the class's: a texture's size is
    // not known until its bytes arrive.
    let upload_price = match request.asset_type.as_str() {
        "sound" => prices.sound,
        "animatn" => prices.animation,
        _ => prices.texture,
    };
    if let Some(balance) = balance
        && balance < i64::from(upload_price)
    {
        return FirstStep::Refused(insufficient(balance, upload_price));
    }
    FirstStep::Taken {
        request,
        expected: expected_of(&root),
        upload_price,
    }
}

/// Second Life's refusal of a model whose request lacks its lists, or whose
/// lists are empty. What it makes of the meshes themselves is not modelled.
fn second_life_model_refusal(root: &Llsd) -> Option<CapsResponse> {
    let list = |key: &str| {
        root.get("asset_resources")
            .and_then(|resources| resources.get(key))
            .and_then(Llsd::as_array)
    };
    let missing: Vec<Llsd> = ["instance_list", "mesh_list"]
        .into_iter()
        .filter(|key| list(key).is_none())
        .map(text)
        .collect();
    if !missing.is_empty() {
        return Some(second_life_refusal(
            200,
            "Upload_InvalidAsset",
            "Invalid parameters.",
            vec![
                ("BadValueTypes", Llsd::Array(Vec::new())),
                ("MissingParameters", Llsd::Array(missing)),
                ("error", text("INVALID_MESH_CONTENTS")),
            ],
        ));
    }
    let empty = if list("mesh_list").is_some_and(<[Llsd]>::is_empty) {
        "The model contains no meshes."
    } else if list("instance_list").is_some_and(<[Llsd]>::is_empty) {
        "The model contains no instances."
    } else {
        return None;
    };
    Some(second_life_refusal(
        200,
        "Upload_InvalidAsset",
        empty,
        vec![("error", text("INVALID_MESH_CONTENTS"))],
    ))
}

/// Second Life's refusal of an upload the balance does not cover.
fn insufficient(balance: i64, upload_price: i32) -> CapsResponse {
    second_life_refusal(
        200,
        "Upload_InsufficientLindenDollarBalance",
        "Insufficient L$ balance to upload",
        vec![
            (
                "balance",
                Llsd::Integer(i32::try_from(balance).unwrap_or(i32::MAX)),
            ),
            ("upload_price", Llsd::Integer(upload_price)),
        ],
    )
}

/// The first answer of an upload that was taken.
pub(crate) fn uploader_answer(
    policy: NewFilePolicy,
    uploader: &str,
    upload_price: i32,
    balance: Option<i64>,
) -> CapsResponse {
    let mut body: HashMap<String, Llsd> = HashMap::from([
        ("state".to_owned(), text("upload")),
        ("uploader".to_owned(), text(uploader)),
    ]);
    match policy {
        NewFilePolicy::Plain => {}
        NewFilePolicy::OpenSim => {
            let _previous = body.insert("upload_price".to_owned(), Llsd::Integer(0));
        }
        NewFilePolicy::SecondLife(_) => {
            let _previous = body.insert("upload_price".to_owned(), Llsd::Integer(upload_price));
            if let Some(balance) = balance {
                let _previous = body.insert(
                    "balance".to_owned(),
                    Llsd::Integer(i32::try_from(balance).unwrap_or(i32::MAX)),
                );
            }
        }
    }
    CapsResponse::llsd_xml_status(200, Llsd::Map(body).to_llsd_xml())
}

/// What Second Life makes of the bytes of an upload: the price to charge, or
/// the refusal to answer with.
pub(crate) fn second_life_bytes(
    prices: UploadPrices,
    request: &NewFileAgentInventoryRequest,
    expected: Expected,
    data: &[u8],
    balance: Option<i64>,
) -> Result<i32, CapsResponse> {
    let invalid = |message: &str| {
        second_life_refusal(
            200,
            "Upload_InvalidAsset",
            message,
            vec![("success", Llsd::Boolean(false))],
        )
    };
    let upload_price = match request.asset_type.as_str() {
        "texture" => {
            if data.is_empty() {
                return Err(invalid("File is empty FILE: /tmp/upload/texture"));
            }
            let Some(header) = crate::j2c::parse_header(data) else {
                return Err(invalid("LLKDUMessageError::flush()"));
            };
            if !header.width.is_power_of_two() {
                return Err(invalid("Invalid width: Value not a power of 2."));
            }
            if !header.height.is_power_of_two() {
                return Err(invalid("Invalid height: Value not a power of 2."));
            }
            let area = u64::from(header.width).saturating_mul(u64::from(header.height));
            if area >= MIN_2K_TEXTURE_AREA {
                prices.large_texture
            } else {
                prices.texture
            }
        }
        "sound" => match vorbis_seconds(data) {
            None => return Err(invalid("Internal error from verifier.")),
            Some(seconds) if seconds > LONGEST_SOUND_SECS => {
                return Err(invalid("Sample too long."));
            }
            Some(_) => prices.sound,
        },
        "animatn" => {
            if let Err(reason) = animation_reads(data) {
                return Err(invalid(&format!(
                    "Bad .anim file {}: {reason}",
                    request.name
                )));
            }
            prices.animation
        }
        _ => prices.texture,
    };
    if let Expected::Stated(stated) = expected {
        if let Some(balance) = balance
            && i64::from(stated) > balance
        {
            return Err(insufficient(balance, stated));
        }
        if stated != upload_price {
            return Err(second_life_refusal(
                200,
                "Upload_UploadPriceDiffers",
                "The server expects a different upload fee",
                vec![
                    ("expected_upload_price", Llsd::Integer(stated)),
                    ("upload_price", Llsd::Integer(upload_price)),
                ],
            ));
        }
    }
    if let Some(balance) = balance
        && balance < i64::from(upload_price)
    {
        return Err(insufficient(balance, upload_price));
    }
    Ok(upload_price)
}

/// The length of an Ogg Vorbis stream in whole seconds, rounded up, or `None`
/// where `data` is no such stream: the last page's granule position over the
/// sample rate the identification header states.
#[expect(
    clippy::little_endian_bytes,
    reason = "every multi-byte field of the format is defined little-endian"
)]
fn vorbis_seconds(data: &[u8]) -> Option<u64> {
    // The first page: its header is 27 bytes and a table of segment lengths,
    // and the identification packet follows — a type byte, `vorbis`, a
    // version (four bytes), the channels (one) and the sample rate (four).
    if data.get(..4)? != b"OggS" {
        return None;
    }
    let segments = usize::from(*data.get(26)?);
    let packet = data.get(27_usize.checked_add(segments)?..)?;
    if packet.get(1..7)? != b"vorbis" {
        return None;
    }
    let rate = u64::from(u32::from_le_bytes(packet.get(12..16)?.try_into().ok()?));
    if rate == 0 {
        return None;
    }
    // The last page's granule position is the stream's length in samples.
    let last = data.windows(4).rposition(|window| window == b"OggS")?;
    let granule = u64::from_le_bytes(
        data.get(last.checked_add(6)?..last.checked_add(14)?)?
            .try_into()
            .ok()?,
    );
    Some(granule.div_ceil(rate))
}

/// Whether `data` begins as a keyframe motion does, and why not where it
/// does not.
#[expect(
    clippy::little_endian_bytes,
    reason = "every multi-byte field of the format is defined little-endian"
)]
fn animation_reads(data: &[u8]) -> Result<(), String> {
    let word = |at: usize| {
        data.get(at..at.saturating_add(2))
            .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
            .map(u16::from_le_bytes)
    };
    let (Some(version), Some(sub_version)) = (word(0), word(2)) else {
        return Err("the file ends before its header does".to_owned());
    };
    if !matches!((version, sub_version), (1, 0) | (0, 1)) {
        return Err(format!(
            "Bad combination of version, sub_version: {version} {sub_version}"
        ));
    }
    // Walk the file: every field has to be there, though none is judged.
    let short = || "the file ends before its motion does".to_owned();
    let mut at = 4_usize;
    let skip = |at: &mut usize, bytes: usize| -> Result<(), String> {
        *at = at
            .checked_add(bytes)
            .filter(|end| *end <= data.len())
            .ok_or_else(short)?;
        Ok(())
    };
    let text = |at: &mut usize| -> Result<(), String> {
        let rest = data.get(*at..).ok_or_else(short)?;
        let end = rest.iter().position(|byte| *byte == 0).ok_or_else(short)?;
        *at = at.saturating_add(end).saturating_add(1);
        Ok(())
    };
    let count = |at: &mut usize| -> Result<usize, String> {
        let bytes = data
            .get(*at..at.saturating_add(4))
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .ok_or_else(short)?;
        *at = at.saturating_add(4);
        usize::try_from(u32::from_le_bytes(bytes)).map_err(|_overflow| short())
    };
    // The priority and the duration, the emote's name, the loop's two points
    // and its flag, the two eases and the hand pose.
    skip(&mut at, 8)?;
    text(&mut at)?;
    skip(&mut at, 24)?;
    // A key is a time and three components: sixteen-bit each in the modern
    // encoding, floats in the legacy one.
    let key = if version == 1 { 8 } else { 16 };
    for _joint in 0..count(&mut at)? {
        text(&mut at)?;
        skip(&mut at, 4)?;
        for _track in 0..2 {
            let keys = count(&mut at)?;
            skip(&mut at, keys.checked_mul(key).ok_or_else(short)?)?;
        }
    }
    let _constraints = count(&mut at)?;
    Ok(())
}

/// The completion of an upload Second Life took.
pub(crate) fn second_life_completion(
    request: &NewFileAgentInventoryRequest,
    new_asset: Uuid,
    new_inventory_item: Uuid,
    transaction: Uuid,
    upload_price: i32,
) -> CapsResponse {
    let mask = |bits: u32| Llsd::Integer(bits.cast_signed());
    CapsResponse::llsd_xml_status(
        200,
        map([
            ("balance", Llsd::Undef),
            ("inventory_flags", Llsd::Integer(0)),
            ("new_asset", Llsd::Uuid(new_asset)),
            ("new_base_mask", Llsd::Integer(FULL_MASK)),
            ("new_everyone_mask", mask(request.everyone_mask)),
            ("new_group_mask", mask(request.group_mask)),
            ("new_inventory_item", Llsd::Uuid(new_inventory_item)),
            ("new_next_owner_mask", mask(request.next_owner_mask)),
            ("new_owner_mask", Llsd::Integer(FULL_MASK)),
            ("state", text("complete")),
            ("transaction_id", Llsd::Uuid(transaction)),
            ("upload_price", Llsd::Integer(upload_price)),
        ])
        .to_llsd_xml(),
    )
}

/// The completion of an upload OpenSim took.
pub(crate) fn open_sim_completion(
    request: &NewFileAgentInventoryRequest,
    new_asset: Uuid,
    new_inventory_item: Uuid,
) -> CapsResponse {
    let mask = |bits: u32| Llsd::Integer(bits.cast_signed());
    CapsResponse::llsd_xml_status(
        200,
        map([
            ("inventory_item_flags", Llsd::Integer(0)),
            ("new_asset", text(&new_asset.to_string())),
            ("new_everyone_mask", mask(request.everyone_mask)),
            ("new_group_mask", mask(request.group_mask)),
            ("new_inventory_item", Llsd::Uuid(new_inventory_item)),
            ("new_next_owner_mask", mask(request.next_owner_mask)),
            ("state", text("complete")),
        ])
        .to_llsd_xml(),
    )
}

/// What OpenSim files an upload as: the request with the classes its
/// `UploadCompleteHandler` gives the item, or `None` for an object that is
/// no mesh model — which it stores nothing for, and does not come back from.
///
/// The handler knows five inventory classes by name (`sound`, `snapshot`,
/// `animation`, `wearable`, `object`) and leaves every other at its starting
/// value, which is a texture's.
pub(crate) fn open_sim_filed_as(
    request: &NewFileAgentInventoryRequest,
) -> Option<NewFileAgentInventoryRequest> {
    let (asset_type, inventory_type) = match request.inventory_type.as_str() {
        "sound" => ("sound", "sound"),
        "snapshot" => ("texture", "snapshot"),
        "animation" => ("animatn", "animation"),
        "wearable" => (
            match request.asset_type.as_str() {
                "bodypart" => "bodypart",
                "clothing" => "clothing",
                _ => "texture",
            },
            "wearable",
        ),
        "object" if request.asset_type == "mesh" => ("object", "object"),
        "object" => return None,
        _ => ("texture", "texture"),
    };
    Some(NewFileAgentInventoryRequest {
        asset_type: asset_type.to_owned(),
        inventory_type: inventory_type.to_owned(),
        ..request.clone()
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A first request in `asset_type` and `inventory_type`, stating `cost`.
    fn request(asset_type: &str, inventory_type: &str, cost: Option<Llsd>) -> Vec<u8> {
        let mut body = HashMap::from([
            ("folder_id".to_owned(), Llsd::Uuid(Uuid::from_u128(7))),
            ("asset_type".to_owned(), text(asset_type)),
            ("inventory_type".to_owned(), text(inventory_type)),
            ("name".to_owned(), text("a name")),
        ]);
        if let Some(cost) = cost {
            let _previous = body.insert("expected_upload_cost".to_owned(), cost);
        }
        Llsd::Map(body).to_llsd_xml().into_bytes()
    }

    /// The text of a refusal.
    fn refusal(step: FirstStep) -> String {
        match step {
            FirstStep::Refused(response) => String::from_utf8_lossy(&response.body).into_owned(),
            FirstStep::Taken { .. } => "taken".to_owned(),
        }
    }

    /// Second Life's first step does not look at the stated cost at all,
    /// but for its type: the bytes are what it is held to.
    #[test]
    fn second_life_takes_any_stated_cost_at_the_first_step() {
        let policy = NewFilePolicy::SecondLife(UploadPrices::default());
        for cost in [None, Some(Llsd::Integer(0)), Some(Llsd::Integer(1_000_000))] {
            let step = first_step(
                policy,
                "POST",
                &request("texture", "texture", cost),
                Some(20),
            );
            assert!(matches!(
                step,
                FirstStep::Taken {
                    upload_price: 10,
                    ..
                }
            ));
        }
        let step = first_step(
            policy,
            "POST",
            &request("texture", "texture", Some(text("ten"))),
            Some(20),
        );
        assert!(refusal(step).contains("Upload_BadParameterType"));
    }

    /// Second Life refuses at the first step what the balance does not
    /// cover, and the classes it has other uploaders for.
    #[test]
    fn second_life_refuses_the_poor_and_the_other_classes() {
        let policy = NewFilePolicy::SecondLife(UploadPrices::default());
        let poor = first_step(
            policy,
            "POST",
            &request("texture", "texture", None),
            Some(3),
        );
        assert!(refusal(poor).contains("Upload_InsufficientLindenDollarBalance"));
        let notecard = first_step(policy, "POST", &request("notecard", "notecard", None), None);
        assert!(refusal(notecard).contains("Invalid asset type: notecard"));
        let landmark = first_step(policy, "POST", &request("landmark", "landmark", None), None);
        assert!(refusal(landmark).contains("Upload_ServerError"));
        let wrong = first_step(policy, "POST", &request("texture", "sound", None), None);
        assert!(refusal(wrong).contains("Upload_InvalidInventoryType"));
    }

    /// The bytes are what a stated cost is held to.
    #[test]
    fn second_life_holds_the_bytes_to_the_stated_cost() {
        let prices = UploadPrices::default();
        let sound = lenient(
            &parse_llsd_xml(&String::from_utf8_lossy(&request("sound", "sound", None)))
                .unwrap_or(Llsd::Undef),
        );
        let noise = second_life_bytes(prices, &sound, Expected::Stated(10), b"noise", Some(100));
        assert!(noise.is_err_and(|refused| {
            String::from_utf8_lossy(&refused.body).contains("Internal error from verifier.")
        }));
        let wearable = NewFileAgentInventoryRequest {
            asset_type: "clothing".to_owned(),
            ..sound
        };
        assert_eq!(
            second_life_bytes(prices, &wearable, Expected::Unstated, b"", Some(100)).ok(),
            Some(10)
        );
        let differs = second_life_bytes(prices, &wearable, Expected::Stated(0), b"", Some(100));
        assert!(differs.is_err_and(|refused| {
            String::from_utf8_lossy(&refused.body).contains("Upload_UploadPriceDiffers")
        }));
        let poor = second_life_bytes(prices, &wearable, Expected::Stated(1000), b"", Some(100));
        assert!(poor.is_err_and(|refused| {
            String::from_utf8_lossy(&refused.body)
                .contains("Upload_InsufficientLindenDollarBalance")
        }));
    }

    /// OpenSim files a class it has no name for as a texture, and nothing at
    /// all for an object that is no model.
    #[test]
    fn open_sim_files_unknown_classes_as_textures() {
        let of = |asset_type: &str, inventory_type: &str| {
            let request = lenient(
                &parse_llsd_xml(&String::from_utf8_lossy(&request(
                    asset_type,
                    inventory_type,
                    None,
                )))
                .unwrap_or(Llsd::Undef),
            );
            open_sim_filed_as(&request).map(|filed| (filed.asset_type, filed.inventory_type))
        };
        assert_eq!(
            of("notecard", "notecard"),
            Some(("texture".to_owned(), "texture".to_owned()))
        );
        assert_eq!(
            of("clothing", "wearable"),
            Some(("clothing".to_owned(), "wearable".to_owned()))
        );
        assert_eq!(
            of("mesh", "object"),
            Some(("object".to_owned(), "object".to_owned()))
        );
        assert_eq!(of("object", "object"), None);
    }

    /// A keyframe motion of one joint with one rotation key, in the modern
    /// encoding.
    #[expect(
        clippy::little_endian_bytes,
        reason = "every multi-byte field of the format is defined little-endian"
    )]
    fn motion() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        // The priority and the duration.
        out.extend_from_slice(&[0; 8]);
        // No emote; the loop, the eases and the hand pose.
        out.push(0);
        out.extend_from_slice(&[0; 24]);
        // One joint: its name, its priority, one rotation key, no position
        // key.
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.extend_from_slice(b"mChest\0");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&0_u32.to_le_bytes());
        // No constraint.
        out.extend_from_slice(&0_u32.to_le_bytes());
        out
    }

    /// A motion is read to its end, and one cut short anywhere is not one.
    #[test]
    fn an_animation_cut_short_is_refused() {
        let whole = motion();
        assert_eq!(animation_reads(&whole), Ok(()));
        for length in [0, 3, 12, 40, whole.len().saturating_sub(1)] {
            let cut = whole.get(..length).unwrap_or_default();
            assert!(
                animation_reads(cut).is_err(),
                "{length} bytes read as a motion"
            );
        }
        assert!(
            animation_reads(b"noise that is no motion at all")
                .is_err_and(|reason| reason.starts_with("Bad combination of version"))
        );
    }

    /// A model has to bring its lists, and something in them.
    #[test]
    fn second_life_refuses_a_model_with_nothing_in_it() {
        let policy = NewFilePolicy::SecondLife(UploadPrices::default());
        let model = |resources: Option<Llsd>| {
            let mut body = HashMap::from([
                ("folder_id".to_owned(), Llsd::Uuid(Uuid::from_u128(7))),
                ("asset_type".to_owned(), text("mesh")),
                ("inventory_type".to_owned(), text("object")),
                ("name".to_owned(), text("a model")),
            ]);
            if let Some(resources) = resources {
                let _previous = body.insert("asset_resources".to_owned(), resources);
            }
            Llsd::Map(body).to_llsd_xml().into_bytes()
        };
        let lists = |meshes: Vec<Llsd>, instances: Vec<Llsd>| {
            map([
                ("mesh_list", Llsd::Array(meshes)),
                ("instance_list", Llsd::Array(instances)),
            ])
        };
        let none = first_step(policy, "POST", &model(None), None);
        assert!(refusal(none).contains("Invalid parameters."));
        let empty = first_step(
            policy,
            "POST",
            &model(Some(lists(Vec::new(), Vec::new()))),
            None,
        );
        assert!(refusal(empty).contains("The model contains no meshes."));
        let unplaced = first_step(
            policy,
            "POST",
            &model(Some(lists(vec![Llsd::Binary(vec![1])], Vec::new()))),
            None,
        );
        assert!(refusal(unplaced).contains("The model contains no instances."));
    }

    /// A sound's length is read from its last page.
    #[test]
    fn a_stream_that_is_no_vorbis_has_no_length() {
        assert_eq!(vorbis_seconds(b"RIFF....WAVE"), None);
        assert_eq!(vorbis_seconds(b""), None);
    }
}
