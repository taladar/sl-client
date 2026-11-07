//! What a script does with its prim's contents, applied to the region
//! (`server-lsl-lib-task-inventory`): the [`Host`](sl_lsl_runtime::vm::Host)
//! methods the task-inventory tranche added, each a thin translation of what
//! the library decided onto the region's store.
//!
//! A write into a prim's contents happens at once, so the next call of the
//! same slice reads it; who is told of it, and the `changed` it raises, wait
//! for the tick to end ([`Effect`]).

use sl_lsl_runtime::vm::{CallerId, Item, Masks, Offer, RemoteLoad, Rez, RezRefusal};
use sl_proto::{
    AssetKey, AssetSource as _, AssetType, ImDialog, InventoryType, RegionLocalObjectId,
    TaskInventoryItem,
};
use sl_types::key::{AgentKey, InventoryKey};
use sl_types::map::RegionCoordinates;

use super::{Delivery, Effect, RegionHost};

/// `CHANGED_INVENTORY`.
const CHANGED_INVENTORY: i32 = 1;

/// The item `held` as the library reads it.
fn item_of(held: &TaskInventoryItem) -> Item {
    let mask = |value: sl_wire::Permissions| value.bits().cast_signed();
    Item {
        name: held.name.clone(),
        kind: held.asset_type.to_code(),
        item: held.item_id.uuid().to_string(),
        asset: held.asset_id.map_or_else(
            || uuid::Uuid::nil().to_string(),
            |asset| asset.uuid().to_string(),
        ),
        creator: held.creator_id.uuid().to_string(),
        last_owner: held.last_owner_id.uuid().to_string(),
        masks: Masks {
            base: mask(held.permissions.base),
            owner: mask(held.permissions.owner),
            group: mask(held.permissions.group),
            everyone: mask(held.permissions.everyone),
            next: mask(held.permissions.next_owner),
        },
        description: held.description.clone(),
        acquired: i64::from(held.creation_date),
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// The region-local id of `caller`'s prim.
    fn prim_of_caller(&self, caller: CallerId) -> Option<RegionLocalObjectId> {
        self.origin(caller).map(|origin| origin.local_id)
    }

    /// The items of `caller`'s prim.
    pub(super) fn contents_of(&self, caller: CallerId) -> Vec<Item> {
        self.prim_of_caller(caller)
            .and_then(|local_id| self.world.objects.contents(local_id))
            .map(|contents| contents.items.iter().map(item_of).collect())
            .unwrap_or_default()
    }

    /// The item of `caller`'s prim named `name`.
    fn held(&self, caller: CallerId, name: &str) -> Option<TaskInventoryItem> {
        let local_id = self.prim_of_caller(caller)?;
        self.world
            .objects
            .contents(local_id)?
            .items
            .iter()
            .find(|item| item.name == name)
            .cloned()
    }

    /// Tells the region of a write a script made into `local_id`'s contents —
    /// `landed_or_left` when a script landed or an item left, which OpenSim
    /// sends the prim again for — and raises `changed(bits)` there per element of `changed`, once the
    /// tick is over.
    fn written(&mut self, local_id: RegionLocalObjectId, landed_or_left: bool, changed: &[i32]) {
        self.effects.push(Effect::Told {
            prim: local_id,
            script: landed_or_left,
        });
        for bits in changed {
            self.effects.push(Effect::Changed {
                prim: local_id,
                bits: *bits,
            });
        }
    }

    /// `llRemoveInventory`.
    pub(super) fn remove_from(&mut self, caller: CallerId, name: &str, changed: &[i32]) -> bool {
        let Some(local_id) = self.prim_of_caller(caller) else {
            return false;
        };
        let Some(contents) = self.world.objects.contents_mut(local_id) else {
            return false;
        };
        let Some(item) = contents
            .items
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.item_id)
        else {
            return false;
        };
        let _removed = contents.remove(item);
        // An item leaving is a write both grids send the prim again for.
        self.written(local_id, true, changed);
        true
    }

    /// `llGiveInventory` and `llGiveInventoryList` into the prim keyed
    /// `target`: a copy of each item under a free name, a script stopped,
    /// for a prim of the caller's owner or one that allows drops.
    pub(super) fn give_into(&mut self, caller: CallerId, target: &str, items: &[String]) -> bool {
        let Some(origin) = self.origin(caller) else {
            return false;
        };
        let Some(to) = self.prim_keyed(target) else {
            return false;
        };
        let Some(receiver) = self.world.objects.get(to).cloned() else {
            return false;
        };
        let allowed = receiver.owner_id == origin.object.owner_id
            || receiver.update_flags & sl_proto::prim_flags::ALLOW_INVENTORY_DROP != 0;
        if !allowed {
            return false;
        }
        let copies: Vec<TaskInventoryItem> = items
            .iter()
            .filter_map(|name| self.held(caller, name))
            .collect();
        let mut script = false;
        for mut copy in copies {
            let source = copy.item_id;
            copy.item_id = InventoryKey::from(self.core.minter.uuid());
            copy.parent_task = receiver.full_id;
            if let Some(linkset) = self.core.assets.taken_linkset(source) {
                self.core.assets.insert_taken_linkset(copy.item_id, linkset);
            }
            script |= copy.inv_type == InventoryType::Script;
            let Some(contents) = self.world.objects.contents_or_empty(to) else {
                return false;
            };
            copy.name = contents.free_name(&copy.name);
            contents.write(copy);
            self.written(to, script, &[CHANGED_INVENTORY]);
        }
        true
    }

    /// `llGiveInventory` and `llGiveInventoryList` to an avatar: the offer as
    /// both grids send it, an instant message of dialog
    /// `TaskInventoryOffered` under the object owner's id and the object's
    /// name, its bucket the asset type of what is offered (a folder for a
    /// list). Filing the items when the offer is accepted is
    /// `test-fake-grid-lsl-offline-cases`'s.
    pub(super) fn offer_to(&mut self, caller: CallerId, offer: &Offer) -> bool {
        let Some(origin) = self.origin(caller) else {
            return false;
        };
        let Ok(agent) = uuid::Uuid::parse_str(&offer.agent) else {
            return false;
        };
        let agent = AgentKey::from(agent);
        if self.core.account_by_agent(agent).is_none() {
            return false;
        }
        let bucket = match &offer.folder {
            Some(_folder) => AssetType::Folder,
            None => offer
                .items
                .first()
                .and_then(|name| self.held(caller, name))
                .map_or(AssetType::Folder, |item| item.asset_type),
        };
        let position = self.position(caller);
        let region_id = self.core.region(self.index).map(|region| region.region_id);
        self.deliveries.push(Delivery::Message {
            agent,
            message: Box::new(sl_proto::InstantMessage {
                from_agent_id: AgentKey::from(origin.object.owner_id),
                from_agent_name: origin.name,
                to_agent_id: agent,
                dialog: ImDialog::TaskInventoryOffered,
                from_group: false,
                region_id,
                position: RegionCoordinates::new(position.x, position.y, position.z),
                offline: offer.offline,
                timestamp: None,
                id: self.core.minter.uuid(),
                parent_estate_id: 1,
                message: offer.message.clone(),
                binary_bucket: vec![u8::try_from(bucket.to_code()).unwrap_or(u8::MAX)],
            }),
        });
        true
    }

    /// `llSetRemoteScriptAccessPin`.
    pub(super) fn set_pin(&mut self, caller: CallerId, pin: i32) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let _was = self.world.script_instances.pins.insert(origin.prim, pin);
    }

    /// The pin of the prim keyed `prim` and its owner.
    pub(super) fn pin_of(&self, prim: &str) -> Option<(i32, String)> {
        let local_id = self.prim_keyed(prim)?;
        let object = self.world.objects.get(local_id)?;
        Some((
            self.world
                .script_instances
                .pins
                .get(&object.full_id)
                .copied()
                .unwrap_or(0),
            object.owner_id.to_string(),
        ))
    }

    /// `llRemoteLoadScriptPin`, the pin checked: the script copied over one
    /// of its name and started at the next reconcile with its start
    /// parameter, running once the tick after is over if it is to.
    pub(super) fn load_into(&mut self, caller: CallerId, load: &RemoteLoad) {
        let Some(mut copy) = self.held(caller, &load.script) else {
            return;
        };
        let Some(to) = self.prim_keyed(&load.target) else {
            return;
        };
        let Some(receiver) = self.world.objects.get(to).map(|object| object.full_id) else {
            return;
        };
        let opensim = self.opensim();
        let Some(contents) = self.world.objects.contents_or_empty(to) else {
            return;
        };
        let replaced = contents
            .items
            .iter()
            .find(|item| item.name == load.script)
            .map(|item| item.item_id);
        if let Some(old) = replaced {
            let _old = contents.remove(old);
        }
        copy.item_id = InventoryKey::from(self.core.minter.uuid());
        copy.parent_task = receiver;
        let item = copy.item_id;
        contents.write(copy);
        self.world
            .script_instances
            .load_script(receiver, item, load.start_parameter, load.running);
        // OpenSim's `RezScriptFromPrim` removes the script it replaces and
        // adds the new one, each raising `changed` (`inventory/give`).
        let changed: &[i32] = if replaced.is_some() && opensim {
            &[CHANGED_INVENTORY, CHANGED_INVENTORY]
        } else {
            &[CHANGED_INVENTORY]
        };
        self.written(to, true, changed);
    }

    /// `llRezObject` and its family.
    pub(super) fn rez_from(&mut self, caller: CallerId, rez: &Rez) -> Result<String, RezRefusal> {
        let origin = self.origin(caller).ok_or(RezRefusal::Missing)?;
        let item = self.held(caller, &rez.item).ok_or(RezRefusal::Missing)?;
        if item.inv_type != InventoryType::Object {
            return Err(RezRefusal::NotAnObject);
        }
        let handle = self
            .core
            .region(self.index)
            .map(crate::runtime::RegionEntry::handle)
            .ok_or(RezRefusal::Missing)?;
        let minter = self.core.minter.clone();
        let mint = move || minter.uuid();
        let how = crate::world::ScriptRez {
            position: rez.position.clone(),
            rotation: rez.rotation.clone(),
            start_parameter: rez.start_parameter,
            start_string: &rez.start_string,
            temporary: rez.temporary,
        };
        let now = (self.core.clock)();
        let rezzed = crate::world::script_rez(
            self.world,
            &self.core.assets,
            handle,
            &mint,
            &item,
            &how,
            now,
        )
        .ok_or(RezRefusal::Missing)?;
        let root = rezzed
            .first()
            .map(|object| object.full_id)
            .ok_or(RezRefusal::Missing)?;
        let rezzer_root = self
            .world
            .objects
            .get(origin.root)
            .map_or(origin.prim, |object| object.full_id);
        let _was = self
            .world
            .script_instances
            .rezzers
            .insert(root, rezzer_root);
        self.world
            .script_instances
            .rezzes_waiting
            .push((caller, root, 0));
        self.effects.push(Effect::Rezzed(rezzed));
        Ok(root.uuid().to_string())
    }

    /// `llDerezObject(id, DEREZ_DIE)` of an object `caller`'s object rezzed.
    pub(super) fn derez_of(&mut self, caller: CallerId, object: &str) -> bool {
        let Some(origin) = self.origin(caller) else {
            return false;
        };
        let Some(local_id) = self.prim_keyed(object) else {
            return false;
        };
        let root = self.world.root_of(local_id);
        let Some(root_key) = self.world.objects.get(root).map(|prim| prim.full_id) else {
            return false;
        };
        let rezzer_root = self
            .world
            .objects
            .get(origin.root)
            .map_or(origin.prim, |prim| prim.full_id);
        if self.world.script_instances.rezzers.get(&root_key) != Some(&rezzer_root) {
            return false;
        }
        let _gone = self.world.script_instances.rezzers.remove(&root_key);
        self.effects.push(Effect::Die(root));
        true
    }

    /// The text of the notecard stored as `asset`, which the region has now
    /// read.
    pub(super) fn read_notecard(&mut self, asset: &str) -> Option<String> {
        let key = AssetKey::from(uuid::Uuid::parse_str(asset).ok()?);
        let body = self.core.assets.read().get(key).map(<[u8]>::to_vec)?;
        let notecard = sl_notecard::Notecard::decode(&body).ok()?;
        let _read = self.world.script_instances.notecards_read.insert(key);
        Some(notecard.text)
    }

    /// The text of the notecard stored as `asset` if the region has read it.
    pub(super) fn notecard_if_read(&self, asset: &str) -> Option<String> {
        let key = AssetKey::from(uuid::Uuid::parse_str(asset).ok()?);
        if !self.world.script_instances.notecards_read.contains(&key) {
            return None;
        }
        let body = self.core.assets.read().get(key).map(<[u8]>::to_vec)?;
        sl_notecard::Notecard::decode(&body)
            .ok()
            .map(|notecard| notecard.text)
    }

    /// `osMakeNotecard`: a notecard of `text`, stored and put in `caller`'s
    /// prim under a free name, the prim's owner its creator.
    pub(super) fn make_notecard_in(
        &mut self,
        caller: CallerId,
        (name, description, text): (&str, &str, &str),
        changed: &[i32],
    ) {
        let Some(origin) = self.origin(caller) else {
            return;
        };
        let body = sl_notecard::Notecard {
            source_version: sl_notecard::NotecardVersion::V2,
            items: Vec::new(),
            text: text.to_owned(),
        }
        .encode();
        let asset = AssetKey::from(self.core.minter.uuid());
        let _previous = self.core.assets.write().insert(asset, body);
        let owner = AgentKey::from(origin.object.owner_id);
        let all = sl_wire::Permissions::ALL;
        let mut item = TaskInventoryItem {
            item_id: InventoryKey::from(self.core.minter.uuid()),
            parent_task: origin.prim,
            permissions: sl_wire::Permissions5 {
                base: all,
                owner: all,
                group: sl_wire::Permissions::NONE,
                everyone: sl_wire::Permissions::NONE,
                next_owner: all,
            },
            creator_id: owner,
            last_owner_id: owner,
            owner: sl_types::key::OwnerKey::Agent(owner),
            group: None,
            group_owned: false,
            asset_id: Some(asset),
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            flags: 0,
            sale_type: sl_proto::SaleType::NotForSale,
            sale_price: sl_types::money::LindenAmount(0),
            name: name.to_owned(),
            description: description.to_owned(),
            creation_date: self.world.wall.unix_seconds_i32(),
        };
        let Some(contents) = self.world.objects.contents_or_empty(origin.local_id) else {
            return;
        };
        item.name = contents.free_name(&item.name);
        contents.write(item);
        self.written(origin.local_id, false, changed);
    }
}
