//! What the smaller upload capabilities carry: the mesh-upload flag, an
//! inventory thumbnail's target and a postcard's request.
//!
//! `NewFileAgentInventory` and the `Update*Inventory` family have their
//! request types in `sl-wire`; these three capabilities were adopted later
//! (measured 2026-10-13, `book/src/gridspec/assets.md` § Uploads) and their
//! bodies are small enough to live beside the types they carry.

use std::collections::HashMap;

use sl_types::key::{InventoryFolderKey, InventoryKey, ObjectKey};
use sl_wire::{GlobalCoordinates, Llsd, WireError, parse_llsd_xml};
use uuid::Uuid;

use super::Postcard;

/// What a grid's `MeshUploadFlag` capability says of the agent.
///
/// Second Life answers with the account's status — `valid` once the account
/// has done what Linden Lab asks before a first mesh upload, something else
/// until then — and refuses the upload itself for an account that is not
/// `valid` ("Mesh upload status is not valid."). OpenSim always answers
/// `valid`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MeshUploadStatus {
    /// `valid`: the agent may upload mesh.
    Valid,
    /// The answer stated no status. The reference viewer takes that for
    /// "allowed" (`LLFloaterModelUploadBase::onPermissionsReceived`).
    Unstated,
    /// Any other status, verbatim (aditi: `invalid`): the agent may not.
    Other(String),
}

impl MeshUploadStatus {
    /// Reads the status out of a `MeshUploadFlag` answer.
    #[must_use]
    pub fn from_reply(reply: &Llsd) -> Self {
        match reply.get("mesh_upload_status").and_then(Llsd::as_str) {
            Some("valid") => Self::Valid,
            None | Some("") => Self::Unstated,
            Some(other) => Self::Other(other.to_owned()),
        }
    }

    /// Whether a viewer should offer a mesh upload: the reference viewer does
    /// for `valid` and for an answer that states nothing.
    #[must_use]
    pub const fn allows_upload(&self) -> bool {
        matches!(self, Self::Valid | Self::Unstated)
    }
}

/// What an inventory thumbnail is uploaded for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ThumbnailTarget {
    /// An item of the agent's inventory (`{ item_id }`).
    Item(InventoryKey),
    /// A folder of the agent's inventory (`{ category_id }`).
    Folder(InventoryFolderKey),
    /// An item in a prim's contents (`{ item_id, task_id }`).
    TaskItem {
        /// The prim.
        task: ObjectKey,
        /// The item.
        item: InventoryKey,
    },
}

/// Builds the first request of an `InventoryThumbnailUpload`: the ids of what
/// the thumbnail is for.
#[must_use]
pub fn build_thumbnail_upload_request(target: &ThumbnailTarget) -> String {
    let mut body: HashMap<String, Llsd> = HashMap::new();
    match target {
        ThumbnailTarget::Item(item) => {
            let _previous = body.insert("item_id".to_owned(), Llsd::Uuid(item.uuid()));
        }
        ThumbnailTarget::Folder(folder) => {
            let _previous = body.insert("category_id".to_owned(), Llsd::Uuid(folder.uuid()));
        }
        ThumbnailTarget::TaskItem { task, item } => {
            let _previous = body.insert("item_id".to_owned(), Llsd::Uuid(item.uuid()));
            let _previous = body.insert("task_id".to_owned(), Llsd::Uuid(task.uuid()));
        }
    }
    Llsd::Map(body).to_llsd_xml()
}

/// Reads the first request of an `InventoryThumbnailUpload` — the inverse of
/// [`build_thumbnail_upload_request`]. `None` for a request that names
/// nothing: Second Life hands out an uploader for one all the same, and
/// refuses the image.
///
/// # Errors
///
/// Returns [`WireError::Xml`] if the body is not well-formed XML.
pub fn parse_thumbnail_upload_request(xml: &str) -> Result<Option<ThumbnailTarget>, WireError> {
    let root = parse_llsd_xml(xml)?;
    let id = |key: &str| {
        root.get(key).and_then(|value| {
            value
                .as_uuid()
                .or_else(|| value.as_str().and_then(|text| Uuid::parse_str(text).ok()))
        })
    };
    Ok(match (id("item_id"), id("task_id"), id("category_id")) {
        (Some(item), Some(task), _) => Some(ThumbnailTarget::TaskItem {
            task: ObjectKey::from(task),
            item: InventoryKey::from(item),
        }),
        (Some(item), None, _) => Some(ThumbnailTarget::Item(InventoryKey::from(item))),
        (None, _, Some(folder)) => Some(ThumbnailTarget::Folder(InventoryFolderKey::from(folder))),
        (None, _, None) => None,
    })
}

/// Builds the first request of a `SendPostcard` upload: where the snapshot
/// was taken, who it is to, and what it says. `from` is written only where
/// the card states one — the reference viewer leaves it out on Second Life,
/// which knows the sender's address itself.
#[must_use]
pub fn build_postcard_request(postcard: &Postcard) -> String {
    let mut body: HashMap<String, Llsd> = HashMap::from([
        (
            "pos-global".to_owned(),
            Llsd::Array(vec![
                Llsd::Real(postcard.pos_global.x()),
                Llsd::Real(postcard.pos_global.y()),
                Llsd::Real(postcard.pos_global.z()),
            ]),
        ),
        ("to".to_owned(), Llsd::String(postcard.to.clone())),
        ("name".to_owned(), Llsd::String(postcard.name.clone())),
        ("subject".to_owned(), Llsd::String(postcard.subject.clone())),
        ("msg".to_owned(), Llsd::String(postcard.message.clone())),
    ]);
    if !postcard.from.is_empty() {
        let _previous = body.insert("from".to_owned(), Llsd::String(postcard.from.clone()));
    }
    Llsd::Map(body).to_llsd_xml()
}

/// Reads the first request of a `SendPostcard` upload — the inverse of
/// [`build_postcard_request`]. Lenient: a field left out is empty, as Second
/// Life hands out an uploader for a card that says nothing at all.
///
/// # Errors
///
/// Returns [`WireError::Xml`] if the body is not well-formed XML.
pub fn parse_postcard_request(xml: &str) -> Result<Postcard, WireError> {
    let root = parse_llsd_xml(xml)?;
    let text = |key: &str| {
        root.get(key)
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let axis = |index: usize| {
        root.get("pos-global")
            .and_then(Llsd::as_array)
            .and_then(|axes| axes.get(index))
            .and_then(Llsd::as_f64)
            .unwrap_or(0.0)
    };
    Ok(Postcard {
        pos_global: GlobalCoordinates::new(axis(0), axis(1), axis(2)),
        to: text("to"),
        from: text("from"),
        name: text("name"),
        subject: text("subject"),
        message: text("msg"),
        ..Postcard::default()
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// What a test returns when a body it built does not read back.
    type TestError = Box<dyn core::error::Error>;

    /// A status is read as the reference viewer reads it.
    #[test]
    fn a_status_allows_an_upload_when_valid_or_unstated() -> Result<(), TestError> {
        let of =
            |body: &str| parse_llsd_xml(body).map(|reply| MeshUploadStatus::from_reply(&reply));
        let valid =
            of("<llsd><map><key>mesh_upload_status</key><string>valid</string></map></llsd>")?;
        assert_eq!(valid, MeshUploadStatus::Valid);
        assert!(valid.allows_upload());
        let unstated = of("<llsd><map><key>username</key><string>somebody</string></map></llsd>")?;
        assert_eq!(unstated, MeshUploadStatus::Unstated);
        assert!(unstated.allows_upload());
        let invalid =
            of("<llsd><map><key>mesh_upload_status</key><string>invalid</string></map></llsd>")?;
        assert_eq!(invalid, MeshUploadStatus::Other("invalid".to_owned()));
        assert!(!invalid.allows_upload());
        Ok(())
    }

    /// Each target reads back from the request built for it.
    #[test]
    fn a_thumbnail_target_round_trips() -> Result<(), TestError> {
        let item = InventoryKey::from(Uuid::from_u128(1));
        let folder = InventoryFolderKey::from(Uuid::from_u128(2));
        let task = ObjectKey::from(Uuid::from_u128(3));
        for target in [
            ThumbnailTarget::Item(item),
            ThumbnailTarget::Folder(folder),
            ThumbnailTarget::TaskItem { task, item },
        ] {
            let body = build_thumbnail_upload_request(&target);
            assert_eq!(parse_thumbnail_upload_request(&body)?, Some(target));
        }
        assert_eq!(
            parse_thumbnail_upload_request("<llsd><map /></llsd>")?,
            None
        );
        Ok(())
    }

    /// A postcard reads back, and states a sender only where it has one.
    #[test]
    fn a_postcard_round_trips() -> Result<(), TestError> {
        let card = Postcard {
            pos_global: GlobalCoordinates::new(256_000.0, 255_000.5, 30.0),
            to: "somebody@example.org".to_owned(),
            name: "A Resident".to_owned(),
            subject: "A view".to_owned(),
            message: "Look at this.".to_owned(),
            ..Postcard::default()
        };
        let body = build_postcard_request(&card);
        assert!(!body.contains("<key>from</key>"));
        assert_eq!(parse_postcard_request(&body)?, card);
        let signed = Postcard {
            from: "sender@example.org".to_owned(),
            ..card
        };
        assert_eq!(
            parse_postcard_request(&build_postcard_request(&signed))?,
            signed
        );
        Ok(())
    }
}
