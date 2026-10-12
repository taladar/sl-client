//! The two capabilities a prim's contents are read through: its listing over
//! `RequestTaskInventory`, and a script's experience over `GetMetadata`.
//!
//! **`RequestTaskInventory`** is how a current viewer learns what a prim
//! holds (`LLViewerObject::fetchInventoryFromCapCoro`): a GET of
//! `<cap>?task_id=<object>`, with `&inventory_serial=<n>` when it already
//! holds a listing, answered with `{ inventory_serial, contents: [item, …] }`
//! — the items as LLSD, without the "Contents" folder the legacy listing
//! opens with — or with `304` and no body when the serial it named is still
//! the prim's. It replaces the UDP `RequestTaskInventory` →
//! `ReplyTaskInventory` → `Xfer` road, which a grid without the capability
//! (OpenSim) still takes.
//!
//! **`GetMetadata`** answers one question (`LLExperienceCache::fetchAssociatedExperience`):
//! a POST of `{ "object-id", "item-id", "fields": ["experience"] }` is
//! answered `{ experience }`, the experience a script in a prim was compiled
//! under. A viewer asks before it saves a script, or the save drops the
//! script out of its experience.
//!
//! Both directions of both bodies are here: what the client sends and reads,
//! and what a simulator reads and answers.

use sl_types::key::{AgentKey, ExperienceKey, InventoryKey, ObjectKey};
use sl_types::money::LindenAmount;
use sl_wire::{Llsd, Permissions, Permissions5, WireError};
use uuid::Uuid;

use crate::asset_keys::AssetKey;
use crate::session::conversions::{
    i32_member, llsd_map, string_member, u32_member, uuid_member_lenient,
};
use crate::types::{AssetType, InventoryType, SaleType, TaskInventoryItem};

/// The tag a runtime forwards a `RequestTaskInventory` answer under when the
/// grid said the listing the client holds is still current (`304`): the
/// capability's own name would have the session look for a listing in a body
/// that has none.
pub const TASK_INVENTORY_UNCHANGED_TAG: &str = "RequestTaskInventory/unchanged";

/// The tag a runtime forwards under when a `RequestTaskInventory` GET failed
/// — it could not be sent, was refused, or answered something that is no
/// listing. The session then reads the prim over UDP instead, which every
/// grid still serves: a contents listing nobody answers is one a caller
/// waits on for good.
pub const TASK_INVENTORY_FAILED_TAG: &str = "RequestTaskInventory/failed";

/// The key a runtime stamps the asked-about prim into a `RequestTaskInventory`
/// answer under. The grid's answer does not name it.
const TASK_ID: &str = "task_id";

/// The key the answer states the contents serial under, and the query
/// parameter a request names the serial it holds with.
const INVENTORY_SERIAL: &str = "inventory_serial";

/// The key the answer lists the items under.
const CONTENTS: &str = "contents";

/// The URL of a `RequestTaskInventory` GET for `task`, naming the serial of
/// the listing the client already holds, if it holds one.
#[must_use]
pub fn task_inventory_cap_url(base: &str, task: ObjectKey, held_serial: Option<i16>) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    let task = task.uuid();
    match held_serial {
        Some(serial) => format!("{base}{separator}{TASK_ID}={task}&{INVENTORY_SERIAL}={serial}"),
        None => format!("{base}{separator}{TASK_ID}={task}"),
    }
}

/// What a `RequestTaskInventory` GET asked for, read by a simulator out of
/// the request's query string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskInventoryCapQuery {
    /// The prim whose contents are asked for.
    pub task: ObjectKey,
    /// The serial of the listing the client holds, when it named one.
    pub held_serial: Option<i16>,
}

/// Reads a `RequestTaskInventory` query string. `None` when it names no prim,
/// or names one that is not an id.
#[must_use]
pub fn parse_task_inventory_cap_query(query: &str) -> Option<TaskInventoryCapQuery> {
    let mut task = None;
    let mut held_serial = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            TASK_ID => task = Uuid::parse_str(value.trim()).ok().map(ObjectKey::from),
            INVENTORY_SERIAL => held_serial = value.trim().parse::<i16>().ok(),
            _ => {}
        }
    }
    Some(TaskInventoryCapQuery {
        task: task?,
        held_serial,
    })
}

/// A prim's contents as a `RequestTaskInventory` answer states them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInventoryCapReply {
    /// The prim the listing is of: the one the runtime stamped into the
    /// answer.
    pub task: ObjectKey,
    /// The contents serial the listing was read at.
    pub serial: i16,
    /// The items.
    pub items: Vec<TaskInventoryItem>,
}

/// Stamps the prim a `RequestTaskInventory` GET asked about into its answer,
/// which does not name it. An answer that is not a map becomes one holding
/// the stamp alone, and fails to decode as a listing.
#[must_use]
pub fn stamp_task_inventory_cap_reply(reply: Llsd, task: ObjectKey) -> Llsd {
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => std::collections::HashMap::new(),
    };
    let _previous = map.insert(TASK_ID.to_owned(), Llsd::Uuid(task.uuid()));
    Llsd::Map(map)
}

/// The body a runtime forwards under [`TASK_INVENTORY_UNCHANGED_TAG`]: the
/// prim asked about and the serial the client named, which the grid said is
/// still the prim's.
#[must_use]
pub fn task_inventory_unchanged_body(task: ObjectKey, serial: i16) -> Llsd {
    llsd_map(vec![
        (TASK_ID, Llsd::Uuid(task.uuid())),
        (INVENTORY_SERIAL, Llsd::Integer(i32::from(serial))),
    ])
}

/// The body a runtime forwards under [`TASK_INVENTORY_FAILED_TAG`]: the prim
/// asked about, and the serial the request named if it named one.
#[must_use]
pub fn task_inventory_failed_body(task: ObjectKey, held_serial: Option<i16>) -> Llsd {
    let mut entries = vec![(TASK_ID, Llsd::Uuid(task.uuid()))];
    if let Some(serial) = held_serial {
        entries.push((INVENTORY_SERIAL, Llsd::Integer(i32::from(serial))));
    }
    llsd_map(entries)
}

/// Reads the body [`task_inventory_failed_body`] wrote.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] when the prim is missing or of the wrong
/// kind.
pub fn parse_task_inventory_failed(body: &Llsd) -> Result<(ObjectKey, Option<i16>), WireError> {
    let task = stamped_task(body)?;
    let held = body
        .get(INVENTORY_SERIAL)
        .map(|_serial| serial_of(body))
        .transpose()?;
    Ok((task, held))
}

/// Reads the body [`task_inventory_unchanged_body`] wrote.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] when the prim or the serial is missing or of
/// the wrong kind.
pub fn parse_task_inventory_unchanged(body: &Llsd) -> Result<(ObjectKey, i16), WireError> {
    let task = stamped_task(body)?;
    Ok((task, serial_of(body)?))
}

/// The prim a runtime stamped into an answer.
fn stamped_task(body: &Llsd) -> Result<ObjectKey, WireError> {
    body.field_uuid(TASK_ID, TASK_ID)?
        .map(ObjectKey::from)
        .ok_or(WireError::Llsd(sl_wire::LlsdError::MissingField {
            field: TASK_ID,
        }))
}

/// The contents serial an answer states. It is sixteen bits on the wire and
/// wraps, so a wider number is taken modulo that.
fn serial_of(body: &Llsd) -> Result<i16, WireError> {
    let serial = body
        .field_i32(INVENTORY_SERIAL, INVENTORY_SERIAL)?
        .ok_or(WireError::Llsd(sl_wire::LlsdError::MissingField {
            field: INVENTORY_SERIAL,
        }))?;
    Ok(i16::try_from(serial.rem_euclid(0x1_0000))
        .or_else(|_| i16::try_from(serial.rem_euclid(0x1_0000).saturating_sub(0x1_0000)))
        .unwrap_or(0))
}

/// Decodes a `RequestTaskInventory` answer a runtime stamped with
/// [`stamp_task_inventory_cap_reply`].
///
/// An entry of `contents` without an `item_id` is skipped, as the reference
/// skips it (`LLViewerObject::loadTaskInvLLSD`).
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] when the stamp, the serial or the `contents`
/// array is missing or of the wrong kind.
pub fn parse_task_inventory_cap_reply(body: &Llsd) -> Result<TaskInventoryCapReply, WireError> {
    let task = stamped_task(body)?;
    let serial = serial_of(body)?;
    let contents = body
        .field_array(CONTENTS, CONTENTS)?
        .ok_or(WireError::Llsd(sl_wire::LlsdError::MissingField {
            field: CONTENTS,
        }))?;
    let items = contents
        .iter()
        .filter(|entry| entry.get("item_id").is_some())
        .map(|entry| task_item_from_llsd(entry, task))
        .collect();
    Ok(TaskInventoryCapReply {
        task,
        serial,
        items,
    })
}

/// A type an item's LLSD names either by its code or, as
/// `LLInventoryItem::asLLSD` writes it, by its short name.
fn type_member<T>(item: &Llsd, key: &str, by_code: fn(i32) -> T, by_name: fn(&str) -> T) -> T {
    match item.get(key) {
        Some(Llsd::String(name)) => by_name(name),
        Some(other) => by_code(other.as_i32().unwrap_or(-1)),
        None => by_code(-1),
    }
}

/// One item of a `RequestTaskInventory` answer.
///
/// The ids are read leniently, as an agent inventory item's are: Second Life
/// writes them as LLSD strings. An item that names no `parent_id` is the
/// prim's own.
fn task_item_from_llsd(item: &Llsd, task: ObjectKey) -> TaskInventoryItem {
    let permissions = item.get("permissions");
    let sale_info = item.get("sale_info");
    let mask = |key: &str| Permissions::from_bits(permissions.map_or(0, |p| u32_member(p, key)));
    let id = |key: &str| permissions.map_or_else(Uuid::nil, |p| uuid_member_lenient(p, key));
    let group_owned = permissions
        .and_then(|p| p.get("is_owner_group"))
        .and_then(Llsd::as_bool)
        .unwrap_or(false);
    let parent = uuid_member_lenient(item, "parent_id");
    let asset_id = uuid_member_lenient(item, "asset_id");
    let sale_type = match sale_info.and_then(|sale| sale.get("sale_type")) {
        Some(Llsd::String(name)) => SaleType::from_sale_name(name),
        Some(other) => SaleType::from_code(u8::try_from(other.as_i32().unwrap_or(0)).unwrap_or(0)),
        None => SaleType::NotForSale,
    };
    TaskInventoryItem {
        item_id: InventoryKey::from(uuid_member_lenient(item, "item_id")),
        parent_task: if parent.is_nil() {
            task
        } else {
            ObjectKey::from(parent)
        },
        permissions: Permissions5 {
            base: mask("base_mask"),
            owner: mask("owner_mask"),
            group: mask("group_mask"),
            everyone: mask("everyone_mask"),
            next_owner: mask("next_owner_mask"),
        },
        creator_id: AgentKey::from(id("creator_id")),
        last_owner_id: AgentKey::from(id("last_owner_id")),
        owner: crate::types::inventory_owner_from_wire(id("owner_id"), id("group_id"), group_owned),
        group: crate::types::group_from_wire(id("group_id")),
        group_owned,
        asset_id: (!asset_id.is_nil()).then(|| AssetKey::from(asset_id)),
        asset_type: type_member(
            item,
            "type",
            AssetType::from_code,
            AssetType::from_type_name,
        ),
        inv_type: type_member(
            item,
            "inv_type",
            InventoryType::from_code,
            InventoryType::from_type_name,
        ),
        flags: u32_member(item, "flags"),
        sale_type,
        sale_price: LindenAmount(
            u64::try_from(sale_info.map_or(0, |sale| i32_member(sale, "sale_price"))).unwrap_or(0),
        ),
        name: string_member(item, "name"),
        description: string_member(item, "desc"),
        creation_date: i32_member(item, "created_at"),
    }
}

/// One item as a `RequestTaskInventory` answer carries it: as Second Life
/// writes it (`task-inventory`, 2026-10-12), with the ids as ids, the masks
/// as integers, the two types and the sale type by their short names, and
/// an empty `metadata` map.
fn task_item_to_llsd(item: &TaskInventoryItem) -> Llsd {
    let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
    let mask = |mask: Permissions| Llsd::Integer(mask.bits().cast_signed());
    let permissions = llsd_map(vec![
        ("base_mask", mask(item.permissions.base)),
        ("owner_mask", mask(item.permissions.owner)),
        ("group_mask", mask(item.permissions.group)),
        ("everyone_mask", mask(item.permissions.everyone)),
        ("next_owner_mask", mask(item.permissions.next_owner)),
        ("owner_id", Llsd::Uuid(owner_id)),
        ("last_owner_id", Llsd::Uuid(item.last_owner_id.uuid())),
        ("creator_id", Llsd::Uuid(item.creator_id.uuid())),
        ("group_id", Llsd::Uuid(group_id)),
        ("is_owner_group", Llsd::Boolean(item.group_owned)),
    ]);
    let sale_info = llsd_map(vec![
        (
            "sale_type",
            Llsd::String(item.sale_type.to_sale_name().to_owned()),
        ),
        (
            "sale_price",
            Llsd::Integer(i32::try_from(item.sale_price.0).unwrap_or(i32::MAX)),
        ),
    ]);
    llsd_map(vec![
        ("item_id", Llsd::Uuid(item.item_id.uuid())),
        ("parent_id", Llsd::Uuid(item.parent_task.uuid())),
        ("name", Llsd::String(item.name.clone())),
        ("desc", Llsd::String(item.description.clone())),
        (
            "asset_id",
            Llsd::Uuid(
                item.asset_id
                    .as_ref()
                    .map_or_else(Uuid::nil, AssetKey::uuid),
            ),
        ),
        (
            "type",
            Llsd::String(item.asset_type.to_type_name().to_owned()),
        ),
        (
            "inv_type",
            Llsd::String(item.inv_type.to_type_name().to_owned()),
        ),
        ("flags", Llsd::Integer(item.flags.cast_signed())),
        ("created_at", Llsd::Integer(item.creation_date)),
        ("permissions", permissions),
        ("sale_info", sale_info),
        // Second Life sends one with every item, and it was empty for every
        // item measured.
        ("metadata", llsd_map(vec![])),
    ])
}

/// Builds a simulator's `RequestTaskInventory` answer: the serial and the
/// items, and no "Contents" folder.
#[must_use]
pub fn build_task_inventory_cap_reply(serial: i16, items: &[TaskInventoryItem]) -> String {
    llsd_map(vec![
        (INVENTORY_SERIAL, Llsd::Integer(i32::from(serial))),
        (
            CONTENTS,
            Llsd::Array(items.iter().map(task_item_to_llsd).collect()),
        ),
    ])
    .to_llsd_xml()
}

/// Builds the `GetMetadata` POST that asks which experience the script `item`
/// in the prim `task` was compiled under.
#[must_use]
pub fn build_get_metadata_request(task: ObjectKey, item: InventoryKey) -> String {
    llsd_map(vec![
        ("object-id", Llsd::Uuid(task.uuid())),
        ("item-id", Llsd::Uuid(item.uuid())),
        (
            "fields",
            Llsd::Array(vec![Llsd::String("experience".to_owned())]),
        ),
    ])
    .to_llsd_xml()
}

/// Reads a `GetMetadata` POST: the prim and the script asked about.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] when either id is missing or of the wrong
/// kind.
pub fn parse_get_metadata_request(body: &Llsd) -> Result<(ObjectKey, InventoryKey), WireError> {
    let task = body
        .field_uuid("object-id", "object-id")?
        .ok_or(WireError::Llsd(sl_wire::LlsdError::MissingField {
            field: "object-id",
        }))?;
    let item = body
        .field_uuid("item-id", "item-id")?
        .ok_or(WireError::Llsd(sl_wire::LlsdError::MissingField {
            field: "item-id",
        }))?;
    Ok((ObjectKey::from(task), InventoryKey::from(item)))
}

/// Builds a simulator's `GetMetadata` answer. A script under no experience is
/// answered with an undefined value and no map at all, as Second Life answers
/// it (`task-inventory`, 2026-10-12) — which the reference reads as "none",
/// asking only whether the answer has an `experience`.
#[must_use]
pub fn build_get_metadata_reply(experience: Option<ExperienceKey>) -> String {
    match experience {
        Some(experience) => llsd_map(vec![("experience", Llsd::Uuid(experience.uuid()))]),
        None => Llsd::Undef,
    }
    .to_llsd_xml()
}

/// A script's experience, as a stamped `GetMetadata` answer states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptMetadata {
    /// The prim holding the script.
    pub task: ObjectKey,
    /// The script.
    pub item: InventoryKey,
    /// The experience it was compiled under, or `None` for none.
    pub experience: Option<ExperienceKey>,
}

/// Stamps the prim and the script a `GetMetadata` POST asked about into its
/// answer, which names neither.
#[must_use]
pub fn stamp_get_metadata_reply(reply: Llsd, task: ObjectKey, item: InventoryKey) -> Llsd {
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => std::collections::HashMap::new(),
    };
    let _previous = map.insert("object-id".to_owned(), Llsd::Uuid(task.uuid()));
    let _previous = map.insert("item-id".to_owned(), Llsd::Uuid(item.uuid()));
    Llsd::Map(map)
}

/// Decodes a `GetMetadata` answer a runtime stamped with
/// [`stamp_get_metadata_reply`]. No `experience`, or a nil one, is no
/// experience.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] when a stamp is missing.
pub fn parse_get_metadata_reply(body: &Llsd) -> Result<ScriptMetadata, WireError> {
    let (task, item) = parse_get_metadata_request(body)?;
    let experience = body
        .get("experience")
        .map(crate::session::conversions::llsd_uuid)
        .filter(|experience| !experience.is_nil())
        .map(ExperienceKey::from);
    Ok(ScriptMetadata {
        task,
        item,
        experience,
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, ExperienceKey, InventoryKey, ObjectKey, OwnerKey};
    use sl_types::money::LindenAmount;
    use sl_wire::{Llsd, Permissions, Permissions5, parse_llsd_xml};
    use uuid::Uuid;

    use crate::asset_keys::AssetKey;

    use super::{
        ScriptMetadata, TaskInventoryCapQuery, build_get_metadata_reply,
        build_get_metadata_request, build_task_inventory_cap_reply, parse_get_metadata_reply,
        parse_get_metadata_request, parse_task_inventory_cap_query, parse_task_inventory_cap_reply,
        parse_task_inventory_unchanged, stamp_get_metadata_reply, stamp_task_inventory_cap_reply,
        task_inventory_cap_url, task_inventory_unchanged_body,
    };
    use crate::types::{AssetType, InventoryType, SaleType, TaskInventoryItem};

    /// A prim.
    fn task() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x7A5C))
    }

    /// A script in it.
    fn script() -> TaskInventoryItem {
        TaskInventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(0x17E0)),
            parent_task: task(),
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7fff_ffff),
                owner: Permissions::from_bits(0x7fff_ffff),
                group: Permissions::from_bits(0),
                everyone: Permissions::from_bits(0),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
            creator_id: AgentKey::from(Uuid::from_u128(0xC0)),
            last_owner_id: AgentKey::from(Uuid::from_u128(0xC0)),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xC0))),
            group: None,
            group_owned: false,
            asset_id: Some(AssetKey::from(Uuid::from_u128(0xA55E7))),
            asset_type: AssetType::ScriptText,
            inv_type: InventoryType::Script,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: LindenAmount(0),
            name: "New Script".to_owned(),
            description: "2026-10-11 12:00:00 lsl2 script".to_owned(),
            creation_date: 1_760_000_000,
        }
    }

    /// The URL names the prim, and the held serial only when there is one.
    #[test]
    fn the_url_names_the_prim_and_the_held_serial() {
        assert_eq!(
            task_inventory_cap_url("https://sim/cap/1", task(), None),
            format!("https://sim/cap/1?task_id={}", task().uuid())
        );
        assert_eq!(
            task_inventory_cap_url("https://sim/cap/1", task(), Some(7)),
            format!(
                "https://sim/cap/1?task_id={}&inventory_serial=7",
                task().uuid()
            )
        );
    }

    /// A simulator reads back what the URL asked.
    #[test]
    fn the_query_round_trips() {
        for held_serial in [None, Some(7), Some(-3)] {
            let url = task_inventory_cap_url("https://sim/cap/1", task(), held_serial);
            let query = url.split_once('?').map(|(_path, query)| query);
            assert_eq!(
                query.and_then(parse_task_inventory_cap_query),
                Some(TaskInventoryCapQuery {
                    task: task(),
                    held_serial,
                })
            );
        }
        assert_eq!(parse_task_inventory_cap_query("inventory_serial=7"), None);
        assert_eq!(parse_task_inventory_cap_query("task_id=nonsense"), None);
    }

    /// A listing a simulator builds decodes to the items it was built from.
    #[test]
    fn a_listing_round_trips() -> Result<(), String> {
        let body = parse_llsd_xml(&build_task_inventory_cap_reply(4, &[script()]))
            .map_err(|error| error.to_string())?;
        let reply = parse_task_inventory_cap_reply(&stamp_task_inventory_cap_reply(body, task()))
            .map_err(|error| error.to_string())?;
        assert_eq!(reply.task, task());
        assert_eq!(reply.serial, 4);
        assert_eq!(reply.items, vec![script()]);
        Ok(())
    }

    /// The reference's own writer names the types and the sale type by name
    /// and the ids as strings; an entry without an item id is skipped.
    #[test]
    fn a_listing_with_named_types_and_string_ids_decodes() -> Result<(), String> {
        let item = script();
        let named = super::llsd_map(vec![
            ("item_id", Llsd::String(item.item_id.uuid().to_string())),
            ("asset_id", Llsd::String(Uuid::nil().to_string())),
            ("type", Llsd::String("lsltext".to_owned())),
            ("inv_type", Llsd::String("script".to_owned())),
            ("name", Llsd::String(item.name.clone())),
            (
                "sale_info",
                super::llsd_map(vec![("sale_type", Llsd::String("copy".to_owned()))]),
            ),
        ]);
        let body = super::llsd_map(vec![
            ("inventory_serial", Llsd::Integer(2)),
            (
                "contents",
                Llsd::Array(vec![named, super::llsd_map(vec![])]),
            ),
        ]);
        let reply = parse_task_inventory_cap_reply(&stamp_task_inventory_cap_reply(body, task()))
            .map_err(|error| error.to_string())?;
        let [decoded] = reply.items.as_slice() else {
            return Err(format!("expected one item, got {:?}", reply.items));
        };
        assert_eq!(decoded.item_id, item.item_id);
        assert_eq!(decoded.parent_task, task());
        assert_eq!(decoded.asset_id, None);
        assert_eq!(decoded.asset_type, AssetType::ScriptText);
        assert_eq!(decoded.inv_type, InventoryType::Script);
        assert_eq!(decoded.sale_type, SaleType::Copy);
        Ok(())
    }

    /// An answer that is no listing fails to decode rather than reading as an
    /// empty prim.
    #[test]
    fn an_answer_without_contents_is_an_error() {
        let stamped = stamp_task_inventory_cap_reply(Llsd::Undef, task());
        assert!(matches!(
            parse_task_inventory_cap_reply(&stamped),
            Err(sl_wire::WireError::Llsd(_))
        ));
    }

    /// The failed body round-trips, with and without a held serial.
    #[test]
    fn the_failed_body_round_trips() -> Result<(), String> {
        for held in [None, Some(9)] {
            assert_eq!(
                super::parse_task_inventory_failed(&super::task_inventory_failed_body(
                    task(),
                    held
                ))
                .map_err(|error| error.to_string())?,
                (task(), held)
            );
        }
        Ok(())
    }

    /// The unchanged body round-trips.
    #[test]
    fn the_unchanged_body_round_trips() -> Result<(), String> {
        assert_eq!(
            parse_task_inventory_unchanged(&task_inventory_unchanged_body(task(), 9))
                .map_err(|error| error.to_string())?,
            (task(), 9)
        );
        Ok(())
    }

    /// A metadata request and both answers round-trip.
    #[test]
    fn script_metadata_round_trips() -> Result<(), String> {
        let item = script().item_id;
        let request = parse_llsd_xml(&build_get_metadata_request(task(), item))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            parse_get_metadata_request(&request).map_err(|error| error.to_string())?,
            (task(), item)
        );
        let experience = ExperienceKey::from(Uuid::from_u128(0xE4));
        for answer in [Some(experience), None] {
            let body = parse_llsd_xml(&build_get_metadata_reply(answer))
                .map_err(|error| error.to_string())?;
            assert_eq!(
                parse_get_metadata_reply(&stamp_get_metadata_reply(body, task(), item))
                    .map_err(|error| error.to_string())?,
                ScriptMetadata {
                    task: task(),
                    item,
                    experience: answer,
                }
            );
        }
        Ok(())
    }
}
