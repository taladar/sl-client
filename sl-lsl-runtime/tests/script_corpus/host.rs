//! The corpus's world: one prim standing alone in a region, owned by one
//! avatar who is there, and a [`Host`] that writes down every call a script
//! makes of it as one line of the transcript.
//!
//! What a script *does* — says, tells, moves, asks, pays — is a line. What a
//! script *reads* is answered from the scene below and leaves no line: a
//! transcript is the script's behaviour, and a query is only behaviour through
//! what the script then does with the answer.
//!
//! The scene is fixed rather than stated per case. Every case that needs more
//! of a world than this belongs on the fake grid, which is a world.

use std::collections::BTreeMap;

use sl_lsl_runtime::library::BuiltinId;
use sl_lsl_runtime::value::Element;
use sl_lsl_runtime::vm::comms::{
    Dialog, InstantMessage, InvalidLinkTarget, LinkedPrim, Links, LoadUrl, MapDestination,
};
use sl_lsl_runtime::vm::prim::{ObjectDetails, Prim, SoundCue};
use sl_lsl_runtime::vm::seating::{Placement, Seat};
use sl_lsl_runtime::vm::{
    Admission, CallerId, ExperienceDetails, ExperienceError, ExperienceQuestion, Host,
    KeyValueStore, LinksetData, LinksetId, MoneyRefusal, PayPrice, Payment, Question,
    ScriptExperience, Simulator, SitRefusal, Speech, Standing, Whereabouts,
};
use sl_lsl_runtime::{NULL_KEY, Value, ZERO_VECTOR};
use sl_types::lsl::{Rotation, Vector};

/// The avatar who owns the prim, and who touches it and talks to it.
pub(crate) const OWNER: &str = "0a0a0a0a-0000-4000-8000-000000000001";

/// The owner's name, as a listen hears it.
pub(crate) const OWNER_NAME: &str = "Corpus Resident";

/// The prim's key.
pub(crate) const PRIM: &str = "c0c0c0c0-0000-4000-8000-000000000002";

/// The prim's name.
const PRIM_NAME: &str = "Corpus Prim";

/// The script's inventory item.
const ITEM: &str = "c0c0c0c0-0000-4000-8000-000000000003";

/// The script's inventory name.
const SCRIPT_NAME: &str = "corpus";

/// The region's name.
const REGION: &str = "Corpus Region";

/// The ground's height everywhere in the region.
const GROUND: f32 = 20.0;

/// Where the prim stands.
pub(crate) const AT: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 21.0,
};

/// A value as a transcript writes it: its type, then the value with every
/// float in Rust's shortest form that reads back as the same `f32` — so a
/// transcript pins the bits, not a rounding of them.
pub(crate) fn value(value: &Value) -> String {
    match value {
        Value::Integer(number) => format!("integer {number}"),
        Value::Float(number) => format!("float {number:?}"),
        Value::String(text) => format!("string {text:?}"),
        Value::Key(text) => format!("key {text:?}"),
        Value::Vector(vector) => format!("vector {}", vector_text(vector)),
        Value::Rotation(rotation) => format!("rotation {}", rotation_text(rotation)),
        Value::List(elements) => {
            let shown: Vec<String> = elements.iter().map(element).collect();
            format!("list [{}]", shown.join(", "))
        }
    }
}

/// A list element as [`value`] writes it.
fn element(element: &Element) -> String {
    let as_value = match element {
        Element::Integer(number) => Value::Integer(*number),
        Element::Float(number) => Value::Float(*number),
        Element::String(text) => Value::String(text.clone()),
        Element::Key(text) => Value::Key(text.clone()),
        Element::Vector(vector) => Value::Vector(vector.clone()),
        Element::Rotation(rotation) => Value::Rotation(rotation.clone()),
    };
    value(&as_value)
}

/// A vector, each component in its shortest exact form.
pub(crate) fn vector_text(vector: &Vector) -> String {
    format!("<{:?}, {:?}, {:?}>", vector.x, vector.y, vector.z)
}

/// A rotation, each component in its shortest exact form.
fn rotation_text(rotation: &Rotation) -> String {
    format!(
        "<{:?}, {:?}, {:?}, {:?}>",
        rotation.x, rotation.y, rotation.z, rotation.s
    )
}

/// The corpus host: the scene, and the lines the script's calls left.
#[derive(Debug)]
pub(crate) struct CorpusHost {
    /// The tick being run, which every line is stamped with.
    pub(crate) now: u64,
    /// The transcript so far.
    pub(crate) lines: Vec<String>,
    /// `changed(bits)` the host owes the prim's scripts, raised once the tick
    /// that wrote them is over — as a grid raises them.
    pub(crate) owed_changes: Vec<i32>,
    /// Whether the script's object has died; the driver removes the script.
    pub(crate) died: bool,
    /// The state of the xorshift the random draws come from.
    draws: u32,
    /// Where the prim stands.
    placement: Placement,
    /// The prim, as the library reads and writes it.
    prim: Prim,
    /// The prim's linkset data store.
    store: LinksetData,
    /// The prim's seat settings.
    seat: Seat,
    /// Every experience's key-value store, by key.
    key_values: BTreeMap<String, KeyValueStore>,
    /// The script, as `llGetScriptState` can name it.
    script: CallerId,
}

impl CorpusHost {
    /// The scene, before anything has run, for the script `script`.
    pub(crate) fn new(script: CallerId) -> Self {
        let mut prim = Prim::new_box(PRIM, OWNER);
        PRIM_NAME.clone_into(&mut prim.name);
        prim.position = AT;
        prim.local_position = AT;
        Self {
            now: 0,
            lines: Vec::new(),
            owed_changes: Vec::new(),
            died: false,
            draws: 0,
            placement: Placement {
                position: AT,
                local: AT,
                root: AT,
            },
            prim,
            store: LinksetData::default(),
            seat: Seat::default(),
            key_values: BTreeMap::new(),
            script,
        }
    }

    /// Writes one line of the transcript, stamped with the tick.
    fn line(&mut self, text: impl core::fmt::Display) {
        self.lines.push(format!("@{} {text}", self.now));
    }

    /// Keeps a position out of the ground, as both grids keep an unworn root.
    const fn above_ground(mut position: Vector) -> Vector {
        position.z = position.z.max(GROUND);
        position
    }
}

impl Host for CorpusHost {
    fn print(&mut self, _caller: CallerId, text: &str) {
        self.line(format_args!("print {text:?}"));
    }

    fn stubbed(&mut self, _caller: CallerId, id: BuiltinId) {
        self.line(format_args!("stubbed {id:?}"));
    }

    fn left_state(&mut self, _caller: CallerId) {
        self.line("left-state");
    }

    fn die(&mut self, _caller: CallerId) -> bool {
        self.line("die");
        self.died = true;
        true
    }

    fn script_named(&self, _caller: CallerId, name: &str) -> Option<CallerId> {
        (name == SCRIPT_NAME).then_some(self.script)
    }

    fn say(&mut self, _caller: CallerId, speech: &Speech) {
        self.line(format_args!(
            "say channel={} volume={:?} {:?}",
            speech.channel, speech.to_avatars, speech.text
        ));
    }

    fn say_to(&mut self, _caller: CallerId, target: &str, channel: i32, text: &str) {
        self.line(format_args!("say-to {target} channel={channel} {text:?}"));
    }

    fn instant_message(&mut self, _caller: CallerId, message: &InstantMessage) {
        self.line(format_args!("instant-message {message:?}"));
    }

    fn dialog(&mut self, _caller: CallerId, dialog: &Dialog) {
        self.line(format_args!("dialog {dialog:?}"));
    }

    fn load_url(&mut self, _caller: CallerId, offer: &LoadUrl) {
        self.line(format_args!("load-url {offer:?}"));
    }

    fn map_destination(&mut self, _caller: CallerId, destination: &MapDestination) {
        self.line(format_args!("map-destination {destination:?}"));
    }

    fn worn_by(&self, _caller: CallerId) -> Option<String> {
        None
    }

    fn links(&self, caller: CallerId) -> Links {
        Links {
            this: 0,
            prims: vec![LinkedPrim {
                link: 0,
                key: PRIM.to_owned(),
                name: self.prim.name.clone(),
                scripts: vec![caller],
            }],
            sitters: Vec::new(),
        }
    }

    fn create_link(
        &mut self,
        _caller: CallerId,
        target: &str,
        _parent: bool,
    ) -> Result<(), InvalidLinkTarget> {
        self.line(format_args!("create-link {target} refused"));
        Err(InvalidLinkTarget)
    }

    fn break_link(&mut self, _caller: CallerId, prim: &str) {
        self.line(format_args!("break-link {prim}"));
    }

    fn break_all_links(&mut self, _caller: CallerId) {
        self.line("break-all-links");
    }

    fn allow_inventory_drop(&mut self, _caller: CallerId, allow: bool) {
        self.line(format_args!("allow-inventory-drop {allow}"));
    }

    fn owner_say(&mut self, _caller: CallerId, text: &str) {
        self.line(format_args!("owner-say {text:?}"));
    }

    fn whereabouts(&self, _caller: CallerId) -> Whereabouts {
        Whereabouts {
            object: PRIM.to_owned(),
            prim: PRIM.to_owned(),
            name: self.prim.name.clone(),
            script: SCRIPT_NAME.to_owned(),
            item: ITEM.to_owned(),
            region: REGION.to_owned(),
            position: self.placement.position.clone(),
        }
    }

    fn random_bits(&mut self, _caller: CallerId) -> u32 {
        // xorshift32 from a fixed seed: the corpus's draws are the same on
        // every run.
        let mut state = if self.draws == 0 {
            0x9E37_79B9
        } else {
            self.draws
        };
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        self.draws = state;
        state
    }

    fn linkset(&self, _caller: CallerId) -> LinksetId {
        LinksetId(0)
    }

    fn linkset_data(&mut self, _caller: CallerId) -> &mut LinksetData {
        &mut self.store
    }

    fn owner(&self, _caller: CallerId) -> String {
        OWNER.to_owned()
    }

    fn seated_on(&self, _caller: CallerId, _agent: &str) -> bool {
        false
    }

    fn ask_permissions(&mut self, _caller: CallerId, question: &Question) -> bool {
        self.line(format_args!("ask-permissions {question:?}"));
        true
    }

    fn give_money(&mut self, _caller: CallerId, payment: &Payment) {
        self.line(format_args!("give-money {payment:?}"));
    }

    fn transfer_money(&mut self, _caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal> {
        self.line(format_args!("transfer-money {payment:?} refused"));
        Err(MoneyRefusal::NoSuchEntity)
    }

    fn set_pay_price(&mut self, _caller: CallerId, layout: PayPrice) {
        self.line(format_args!("set-pay-price {layout:?}"));
    }

    fn pass_touches(&mut self, _caller: CallerId, pass: i32) {
        self.line(format_args!("pass-touches {pass}"));
    }

    fn experience(&self, _caller: CallerId) -> Option<ScriptExperience> {
        None
    }

    fn experience_details(&mut self, _caller: CallerId, id: &str) -> Option<ExperienceDetails> {
        Some(ExperienceDetails::invalid(id))
    }

    fn admission(&self, _agent: &str, _experience: &str) -> Admission {
        Admission::Undecided
    }

    fn ask_experience(&mut self, _caller: CallerId, question: &ExperienceQuestion) -> bool {
        self.line(format_args!("ask-experience {question:?}"));
        true
    }

    fn key_value_store(&mut self, experience: &str) -> Result<&mut KeyValueStore, ExperienceError> {
        Ok(self.key_values.entry(experience.to_owned()).or_default())
    }

    fn sit_on_link(&mut self, _caller: CallerId, agent: &str, link: i32) -> Result<(), SitRefusal> {
        self.line(format_args!("sit-on-link {agent} {link} refused"));
        Err(SitRefusal::InvalidAgent)
    }

    fn seat(&mut self, _caller: CallerId, link: i32) -> Option<&mut Seat> {
        (link == 0).then_some(&mut self.seat)
    }

    fn avatar_on_sit_target(&self, _caller: CallerId, _link: i32) -> Option<String> {
        None
    }

    fn unsit(&mut self, _caller: CallerId, agent: &str) {
        self.line(format_args!("unsit {agent}"));
    }

    fn set_pie_text(&mut self, _caller: CallerId, sit: bool, text: &str) {
        let which = if sit { "sit" } else { "touch" };
        self.line(format_args!("pie-text {which} {text:?}"));
    }

    fn attached(&self, _caller: CallerId) -> i32 {
        0
    }

    fn attach(&mut self, _caller: CallerId, point: i32) {
        self.line(format_args!("attach {point}"));
    }

    fn detach(&mut self, _caller: CallerId) {
        self.line("detach");
    }

    fn placement(&self, _caller: CallerId) -> Placement {
        self.placement.clone()
    }

    fn set_position(&mut self, _caller: CallerId, local: Vector) {
        let to = Self::above_ground(local);
        self.line(format_args!("set-position {}", vector_text(&to)));
        self.placement = Placement {
            position: to.clone(),
            local: to.clone(),
            root: to.clone(),
        };
        self.prim.position = to.clone();
        self.prim.local_position = to;
    }

    fn prim(&self, _caller: CallerId, prim: &str) -> Option<Prim> {
        (prim == PRIM).then(|| self.prim.clone())
    }

    fn set_prim(&mut self, _caller: CallerId, state: &Prim, changed: &[i32]) {
        let mut state = state.clone();
        if state.local_position != self.placement.local {
            let to = Self::above_ground(state.local_position.clone());
            self.placement = Placement {
                position: to.clone(),
                local: to.clone(),
                root: to.clone(),
            };
            state.position = to.clone();
            state.local_position = to;
        }
        state.rotation = state.local_rotation.clone();
        state.root_rotation = state.local_rotation.clone();
        self.line(format_args!("set-prim changed={changed:?}"));
        self.owed_changes.extend_from_slice(changed);
        self.prim = state;
    }

    fn set_region_position(&mut self, caller: CallerId, position: Vector) -> Option<Vector> {
        if !(0.0..256.0).contains(&position.x) || !(0.0..256.0).contains(&position.y) {
            self.line(format_args!(
                "set-region-position {} refused",
                vector_text(&position)
            ));
            return None;
        }
        self.set_position(caller, position);
        Some(self.placement.position.clone())
    }

    fn linkset_of(&self, key: &str) -> Vec<String> {
        if key == PRIM {
            vec![PRIM.to_owned()]
        } else {
            Vec::new()
        }
    }

    fn object_details(&self, _caller: CallerId, key: &str) -> Option<ObjectDetails> {
        (key == PRIM).then(|| ObjectDetails {
            avatar: false,
            name: self.prim.name.clone(),
            description: self.prim.description.clone(),
            position: self.prim.position.clone(),
            rotation: self.prim.rotation.clone(),
            velocity: ZERO_VECTOR,
            owner: OWNER.to_owned(),
            group: NULL_KEY.to_owned(),
            creator: self.prim.creator.clone(),
            last_owner: NULL_KEY.to_owned(),
            root: PRIM.to_owned(),
            attached_point: 0,
            scripts: (1, 1),
            script_memory: 0x0001_0000,
            prim_count: 1,
            inventory_count: 1,
            click_action: self.prim.click_action,
            omega: ZERO_VECTOR,
            scale: self.prim.scale.clone(),
            material: self.prim.material,
            flags: (self.prim.physics, self.prim.phantom, self.prim.temp_on_rez),
            text: self.prim.text.clone(),
            costs: (1.0, 0.05, 1.0, 0.5),
            selected: (0, 0),
            rez_time: String::new(),
            rezzer: OWNER.to_owned(),
            masks: [i32::MAX, i32::MAX, 0, 0, 532_480],
            hover_height: 0.0,
            body_shape: -1.0,
            animated: 0,
        })
    }

    fn inventory_key(&self, _caller: CallerId, _name: &str, _asset_type: i32) -> Option<String> {
        None
    }

    fn sound(&mut self, _caller: CallerId, prim: &str, cue: &SoundCue) {
        self.line(format_args!("sound {prim} {cue:?}"));
    }

    fn prim_count(&self, object: &str) -> Option<i32> {
        (object == PRIM).then_some(1)
    }

    fn standing(&self, _caller: CallerId) -> Standing {
        Standing {
            owner: OWNER.to_owned(),
            ..Standing::default()
        }
    }

    fn simulator(&self) -> Simulator {
        Simulator::default()
    }

    fn region_size(&self, _caller: CallerId) -> (u32, u32) {
        (256, 256)
    }

    fn terrain_height(&self, _caller: CallerId, _x: u32, _y: u32) -> f32 {
        GROUND
    }

    fn day_length(&self, _caller: CallerId) -> f32 {
        14_400.0
    }

    fn prim_scripts(&self, prim: &str) -> Option<Vec<CallerId>> {
        (prim == PRIM).then(|| vec![self.script])
    }

    fn agent_ip(&self, _agent: &str) -> String {
        String::new()
    }
}
