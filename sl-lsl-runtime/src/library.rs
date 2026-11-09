//! The LSL library: one generated table, typed implementations, an erased
//! dispatch and a coverage count.
//!
//! **The table** is generated at build time from the vendored
//! `keywords_lsl_default.xml` — Linden Lab's own `LSLSyntax` document as
//! Second Life serves it — and `keywords_lsl_unserved.xml`, the five
//! functions the grid compiles and that document leaves out (provenance of
//! both in the crate `README.md`). It yields one
//! [`BuiltinId`] and one [`Builtin`] descriptor per function (name, typed
//! parameters, return type, forced sleep, energy, flags, description), every
//! [`Constant`] with its typed value, and every [`Event`] with its
//! parameters. Everything that needs to know the library reads it: the
//! lowering resolves a call to a [`BuiltinId`], the dispatch checks
//! arguments against it, and the grid's own `LSLSyntax` document is to be
//! rendered from it.
//!
//! **Implementations** are ordinary typed functions in the tranche modules,
//! registered in `library/registry.rs`; see [`dispatch`] for how the generated
//! [`Signature`] holds each one to the table at compile time.
//!
//! **Coverage** — implemented, stubbed or missing, per function — is
//! [`status`], and a test prints the counts and fails if a function falls
//! back from the committed baseline (`library/coverage.txt`).

pub mod avatars;
pub mod calendar;
pub mod clocks;
pub(crate) mod collation;
pub mod comms;
pub mod contents;
pub mod control;
pub mod detection;
pub mod dispatch;
pub(crate) mod dotnet;
pub mod effects;
pub mod encoding;
pub mod experiences;
mod generated;
pub mod http;
pub mod inventory;
pub mod json_text;
pub mod land;
pub mod linking;
pub mod linkset;
pub mod lists;
pub mod math;
pub mod money_permissions;
pub mod notecards;
pub mod ossl;
pub mod ossl_inventory;
pub mod ossl_prim;
pub(crate) mod prim_params;
pub mod prim_state;
mod range;
mod registry;
pub mod rezzing;
pub mod rotations;
pub mod sensors;
pub mod sitting;
pub mod strings;
pub mod table;
pub mod targets;
mod text;
pub mod usage;

pub use dispatch::{
    CallError, Called, Exception, FromArgs, FromValue, Handler, IntoReturn, Key, Signature, Status,
    invoke, stub,
};
pub use generated::{BUILTINS, BuiltinId, CONSTANTS, EVENTS, signatures};
pub use prim_params::faces_of;
pub use registry::{call, status};
pub use table::{
    Argument, Builtin, Constant, ConstantValue, Event, Family, Gate, ThreatLevel, builtin,
    constant, event, overloads,
};

use crate::value::Element;
use crate::vm::{EngineFlavour, ScriptCtx};

/// What a library function that touches nothing of the world sees of its
/// caller: which grid's engine the region imitates, where the grids answer
/// differently (book chapter `gridspec/lsl.md`, § Strings, lists, maths and
/// rotations), and a way to refuse its arguments in that grid's words — which
/// on OpenSim also costs the script a second (`LSL_Api.Error`).
///
/// The pure tranches are generic over it, so a test calls them with a
/// `bare::Bare` context of any flavour and the VM with the calling script's
/// [`ScriptCtx`].
pub trait PureCtx {
    /// The grid's engine the region imitates.
    fn flavour(&self) -> EngineFlavour;

    /// Refuse the call's arguments with `text` on `DEBUG_CHANNEL`; the call
    /// carries on and returns its answer. On OpenSim it costs the script a
    /// second ([`ScriptCtx::refuse`]).
    fn refuse(&mut self, text: String);

    /// The list object handed as argument `argument` now holds `list`: the
    /// function changed it in place, as some of OpenSim's library does
    /// (`vm::objects`).
    fn write_back(&mut self, argument: usize, list: Vec<Element>);

    /// The list the function returns is the very object handed as argument
    /// `argument`, as some of OpenSim's library hands it back.
    fn return_argument(&mut self, argument: usize);
}

impl PureCtx for ScriptCtx<'_> {
    fn flavour(&self) -> EngineFlavour {
        ScriptCtx::flavour(self)
    }

    fn refuse(&mut self, text: String) {
        ScriptCtx::refuse(self, text);
    }

    fn write_back(&mut self, argument: usize, list: Vec<Element>) {
        ScriptCtx::write_back(self, argument, list);
    }

    fn return_argument(&mut self, argument: usize) {
        ScriptCtx::return_argument(self, argument);
    }
}

/// A context for calling the pure tranches outside a VM.
#[cfg(test)]
pub(crate) mod bare {
    use super::PureCtx;
    use crate::value::Element;
    use crate::vm::EngineFlavour;

    /// A flavour and the refusals said under it.
    #[derive(Debug, Default)]
    pub(crate) struct Bare {
        /// The flavour the calls are made under.
        pub(crate) flavour: EngineFlavour,
        /// What the calls refused, in order.
        pub(crate) refusals: Vec<String>,
        /// The seconds the calls' refusals cost, in all.
        pub(crate) slept: f32,
        /// The argument the last call changed in place, and to what.
        pub(crate) written_back: Option<(usize, Vec<Element>)>,
        /// The argument whose own list the last call returned.
        pub(crate) returned: Option<usize>,
    }

    impl Bare {
        /// No refusals yet, under `flavour`.
        pub(crate) const fn of(flavour: EngineFlavour) -> Self {
            Self {
                flavour,
                refusals: Vec::new(),
                slept: 0.0,
                written_back: None,
                returned: None,
            }
        }
    }

    impl PureCtx for Bare {
        fn flavour(&self) -> EngineFlavour {
            self.flavour
        }

        fn refuse(&mut self, text: String) {
            self.refusals.push(text);
            if self.flavour.is_opensim() {
                self.slept += 1.0;
            }
        }

        fn write_back(&mut self, argument: usize, list: Vec<Element>) {
            self.written_back = Some((argument, list));
        }

        fn return_argument(&mut self, argument: usize) {
            self.returned = Some(argument);
        }
    }

    /// Second Life.
    pub(crate) const fn sl() -> Bare {
        Bare::of(EngineFlavour::SecondLife)
    }

    /// OpenSim's YEngine (the library is shared with XEngine).
    pub(crate) const fn os() -> Bare {
        Bare::of(EngineFlavour::YEngine)
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::print_stderr,
        reason = "the coverage test reports the library's progress to the operator"
    )]

    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use sl_lsl::ast::TypeName;
    use sl_types::lsl::{Rotation, Vector};

    use super::*;
    use crate::value::Value;
    use crate::vm::{
        Admission, CallerId, Dialog, ExperienceDetails, ExperienceError, ExperienceQuestion, Host,
        InstantMessage, KeyValueStore, Links, LinksetData, LinksetId, LoadUrl, MapDestination,
        Memory, MoneyRefusal, Overheard, PayPrice, Payment, Peers, Question, ScriptData,
        ScriptExperience, SitRefusal, Speech, Stack, Tick, Whereabouts,
    };

    /// A host for calls that never reach it: the dispatch reports stubs
    /// through `Called::stubbed`, and only the VM tells the host.
    #[derive(Default)]
    struct Unreached {
        /// The one linkset's store, never written by these tests.
        store: LinksetData,
        /// The prim's HTTP throttle, never asked by these tests.
        throttle: crate::vm::http::Throttle,
    }

    impl Host for Unreached {
        fn prim(&self, _caller: CallerId, _prim: &str) -> Option<crate::vm::prim::Prim> {
            None
        }

        fn rezzed_in(&self, _prim: &str) -> Option<crate::vm::Tick> {
            None
        }

        fn set_prim(&mut self, _caller: CallerId, _state: &crate::vm::prim::Prim) {}

        fn set_region_position(&mut self, _caller: CallerId, _position: Vector) -> Option<Vector> {
            None
        }

        fn linkset_of(&self, _key: &str) -> Vec<String> {
            Vec::new()
        }

        fn object_details(
            &self,
            _caller: CallerId,
            _key: &str,
        ) -> Option<crate::vm::prim::ObjectDetails> {
            None
        }

        fn inventory_key(
            &self,
            _caller: CallerId,
            _name: &str,
            _asset_type: i32,
        ) -> Option<String> {
            None
        }

        fn sound(&mut self, _caller: CallerId, _prim: &str, _cue: &crate::vm::prim::SoundCue) {}

        fn standing(&self, _caller: CallerId) -> crate::vm::Standing {
            crate::vm::Standing::default()
        }

        fn simulator(&self) -> crate::vm::Simulator {
            crate::vm::Simulator::default()
        }

        fn region_size(&self, _caller: CallerId) -> (u32, u32) {
            (256, 256)
        }

        fn terrain_height(&self, _caller: CallerId, _x: u32, _y: u32) -> f32 {
            0.0
        }

        fn day_length(&self, _caller: CallerId) -> f32 {
            14_400.0
        }

        fn prim_scripts(&self, _prim: &str) -> Option<Vec<CallerId>> {
            None
        }

        fn agent_ip(&self, _agent: &str) -> String {
            String::new()
        }

        fn contents(&self, _caller: CallerId) -> Vec<crate::vm::Item> {
            Vec::new()
        }

        fn remove_item(&mut self, _caller: CallerId, _name: &str, _changed: &[i32]) -> bool {
            false
        }

        fn give_to_prim(&mut self, _caller: CallerId, _target: &str, _items: &[String]) -> bool {
            false
        }

        fn offer_items(&mut self, _caller: CallerId, _offer: &crate::vm::Offer) -> bool {
            false
        }

        fn set_remote_pin(&mut self, _caller: CallerId, _pin: i32) {}

        fn remote_pin(&self, _prim: &str) -> Option<(i32, String)> {
            None
        }

        fn load_script(&mut self, _caller: CallerId, _load: &crate::vm::RemoteLoad) {}

        fn rez(
            &mut self,
            _caller: CallerId,
            _rez: &crate::vm::Rez,
        ) -> Result<String, crate::vm::RezRefusal> {
            Err(crate::vm::RezRefusal::Missing)
        }

        fn derez(&mut self, _caller: CallerId, _object: &str) -> bool {
            false
        }

        fn notecard_text(&mut self, _asset: &str) -> Option<String> {
            None
        }

        fn notecard_read(&self, _asset: &str) -> Option<String> {
            None
        }

        fn make_notecard(
            &mut self,
            _caller: CallerId,
            _name: &str,
            _description: &str,
            _text: &str,
            _changed: &[i32],
        ) {
        }

        fn epoch_micros(&self) -> i64 {
            0
        }

        fn local_offset(&self, _unix: i64) -> i32 {
            0
        }

        fn day_offset(&self, _caller: CallerId) -> f32 {
            57_600.0
        }

        fn parcel_at(
            &self,
            _caller: CallerId,
            _x: f32,
            _y: f32,
        ) -> Option<crate::vm::land::Parcel> {
            None
        }

        fn region(&self, _caller: CallerId) -> crate::vm::land::Region {
            crate::vm::land::Region::default()
        }

        fn sim_data(&self, _caller: CallerId, _name: &str) -> Option<crate::vm::land::SimData> {
            None
        }

        fn ground(&self, _caller: CallerId, _x: f32, _y: f32) -> f32 {
            0.0
        }

        fn wind(&self, _caller: CallerId, _position: &Vector) -> Vector {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }

        fn environment(
            &self,
            _caller: CallerId,
            _position: Option<&Vector>,
            _unix: i64,
        ) -> crate::vm::land::Environment {
            crate::vm::land::Environment {
                day_length: 14_400.0,
                day_offset: 57_600.0,
                sun: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                moon: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
            }
        }

        fn change_land(
            &mut self,
            _caller: CallerId,
            _position: &Vector,
            _change: &crate::vm::land::LandChange,
        ) -> bool {
            false
        }

        fn parcel_media(
            &self,
            _caller: CallerId,
            _position: &Vector,
        ) -> crate::vm::land::ParcelMedia {
            crate::vm::land::ParcelMedia::default()
        }

        fn sim_stats(&self, _caller: CallerId) -> Vec<f32> {
            Vec::new()
        }

        fn environment_rule(
            &self,
            _caller: CallerId,
            _position: &Vector,
            _rule: i32,
        ) -> Option<Vec<crate::value::Element>> {
            None
        }

        fn map_texture(&self, _caller: CallerId, _region: &str) -> Option<String> {
            None
        }

        fn send_http(&mut self, _caller: CallerId, _request: &crate::vm::http::Outbound) {}

        fn request_url(&mut self, _caller: CallerId, _key: &str, _secure: bool) -> Option<String> {
            None
        }

        fn release_url(&mut self, _caller: CallerId, _url: &str) {}

        fn free_urls(&self, _caller: CallerId) -> i32 {
            0
        }

        fn answer_http(
            &mut self,
            _caller: CallerId,
            _request: &str,
            _answered: crate::vm::http::Answered,
        ) {
        }

        fn http_throttle(&mut self, _caller: CallerId) -> &mut crate::vm::http::Throttle {
            &mut self.throttle
        }

        fn url_authority(&self, _caller: CallerId) -> String {
            String::new()
        }

        fn agent(&self, _caller: CallerId, _agent: &str) -> Option<crate::vm::avatars::Agent> {
            None
        }

        fn agent_record(
            &self,
            _caller: CallerId,
            _agent: &str,
        ) -> Option<crate::vm::avatars::AgentRecord> {
            None
        }

        fn agent_named(&self, _caller: CallerId, _name: &str) -> Option<(String, bool)> {
            None
        }

        fn animate(&mut self, _caller: CallerId, _agent: &str, _animation: &str, _start: bool) {}

        fn set_animation_override(
            &mut self,
            _caller: CallerId,
            _agent: &str,
            _state: &str,
            _animation: Option<(&str, &str)>,
        ) {
        }

        fn animation_override(
            &self,
            _caller: CallerId,
            _agent: &str,
            _state: &str,
        ) -> Option<String> {
            None
        }

        fn object_animate(
            &mut self,
            _caller: CallerId,
            _name: &str,
            _animation: &str,
            _start: bool,
        ) {
        }

        fn object_animations(&self, _caller: CallerId) -> Vec<String> {
            Vec::new()
        }

        fn control_changes(
            &mut self,
            _caller: CallerId,
            _agent: &str,
            _changes: &[crate::vm::avatars::ControlChange],
            _taken: Option<i32>,
        ) {
        }

        fn set_camera(
            &mut self,
            _caller: CallerId,
            _agent: &str,
            _rules: &[crate::vm::avatars::CameraRule],
        ) {
        }

        fn clear_camera(&mut self, _caller: CallerId, _agent: &str) {}

        fn camera(
            &self,
            _caller: CallerId,
            _agent: &str,
        ) -> Option<(Vector, sl_types::lsl::Rotation)> {
            None
        }

        fn teleport_agent(
            &mut self,
            _caller: CallerId,
            _agent: &str,
            _destination: &crate::vm::avatars::Destination,
        ) {
        }

        fn attached_list(&self, _caller: CallerId, _agent: &str) -> Option<Vec<String>> {
            None
        }

        fn sense(&self, _caller: CallerId) -> Vec<crate::vm::sensing::Sensed> {
            Vec::new()
        }

        fn cast_ray(
            &self,
            _caller: CallerId,
            _start: &Vector,
            _end: &Vector,
        ) -> Vec<crate::vm::sensing::RayHit> {
            Vec::new()
        }

        fn print(&mut self, _caller: CallerId, _text: &str) {}

        fn stubbed(&mut self, _caller: CallerId, _id: BuiltinId) {}

        fn left_state(&mut self, _caller: CallerId) {}

        fn die(&mut self, _caller: CallerId) -> bool {
            false
        }

        fn script_named(&self, _caller: CallerId, _name: &str) -> Option<CallerId> {
            None
        }

        fn say(&mut self, _caller: CallerId, _speech: &Speech) -> Option<Overheard> {
            None
        }

        fn owner_say(&mut self, _caller: CallerId, _text: &str) {}

        fn say_to(
            &mut self,
            _caller: CallerId,
            _target: &str,
            _channel: i32,
            _text: &str,
        ) -> Option<Overheard> {
            None
        }

        fn instant_message(&mut self, _caller: CallerId, _message: &InstantMessage) {}

        fn dialog(&mut self, _caller: CallerId, _dialog: &Dialog) {}

        fn load_url(&mut self, _caller: CallerId, _offer: &LoadUrl) {}

        fn map_destination(&mut self, _caller: CallerId, _destination: &MapDestination) {}

        fn worn_by(&self, _caller: CallerId) -> Option<String> {
            None
        }

        fn links(&self, _caller: CallerId) -> Links {
            Links {
                this: 0,
                prims: Vec::new(),
                sitters: Vec::new(),
            }
        }

        fn create_link(
            &mut self,
            _caller: CallerId,
            _target: &str,
            _parent: bool,
        ) -> Result<(), crate::vm::InvalidLinkTarget> {
            Err(crate::vm::InvalidLinkTarget)
        }

        fn break_link(&mut self, _caller: CallerId, _prim: &str) {}

        fn break_all_links(&mut self, _caller: CallerId) {}

        fn allow_inventory_drop(&mut self, _caller: CallerId, _allow: bool) {}

        fn seat(&mut self, _caller: CallerId, _link: i32) -> Option<&mut crate::vm::seating::Seat> {
            None
        }

        fn avatar_on_sit_target(&self, _caller: CallerId, _link: i32) -> Option<String> {
            None
        }

        fn unsit(&mut self, _caller: CallerId, _agent: &str) {}

        fn set_pie_text(&mut self, _caller: CallerId, _sit: bool, _text: &str) {}

        fn attached(&self, _caller: CallerId) -> i32 {
            0
        }

        fn attach(&mut self, _caller: CallerId, _point: i32) {}

        fn detach(&mut self, _caller: CallerId) {}

        fn pass_touches(&mut self, _caller: CallerId, _pass: i32) {}

        fn placement(&self, _caller: CallerId) -> crate::vm::seating::Placement {
            crate::vm::seating::Placement {
                position: crate::value::ZERO_VECTOR,
                local: crate::value::ZERO_VECTOR,
                root: crate::value::ZERO_VECTOR,
            }
        }

        fn set_position(&mut self, _caller: CallerId, _local: sl_types::lsl::Vector) {}

        fn prim_count(&self, _object: &str) -> Option<i32> {
            None
        }

        fn whereabouts(&self, _caller: CallerId) -> Whereabouts {
            Whereabouts {
                object: String::new(),
                prim: String::new(),
                name: String::new(),
                script: String::new(),
                item: String::new(),
                region: String::new(),
                position: crate::value::ZERO_VECTOR,
            }
        }

        fn random_bits(&mut self, _caller: CallerId) -> u32 {
            0
        }

        fn mint_scene_id(&mut self) -> i32 {
            0
        }

        fn linkset(&self, _caller: CallerId) -> LinksetId {
            LinksetId(0)
        }

        fn linkset_data(&mut self, _caller: CallerId) -> &mut LinksetData {
            &mut self.store
        }

        fn owner(&self, _caller: CallerId) -> String {
            String::new()
        }

        fn seated_on(&self, _caller: CallerId, _agent: &str) -> bool {
            false
        }

        fn ask_permissions(&mut self, _caller: CallerId, _question: &Question) -> bool {
            false
        }

        fn give_money(&mut self, _caller: CallerId, _payment: &Payment) {}

        fn transfer_money(
            &mut self,
            _caller: CallerId,
            _payment: &Payment,
        ) -> Result<(), MoneyRefusal> {
            Err(MoneyRefusal::NoSuchEntity)
        }

        fn set_pay_price(&mut self, _caller: CallerId, _layout: PayPrice) {}

        fn experience(&self, _caller: CallerId) -> Option<ScriptExperience> {
            None
        }

        fn experience_details(
            &mut self,
            _caller: CallerId,
            _id: &str,
        ) -> Option<ExperienceDetails> {
            None
        }

        fn admission(&self, _agent: &str, _experience: &str) -> Admission {
            Admission::Undecided
        }

        fn ask_experience(&mut self, _caller: CallerId, _question: &ExperienceQuestion) -> bool {
            false
        }

        fn key_value_store(
            &mut self,
            _experience: &str,
        ) -> Result<&mut KeyValueStore, ExperienceError> {
            Err(ExperienceError::StoreDisabled)
        }

        fn sit_on_link(
            &mut self,
            _caller: CallerId,
            _agent: &str,
            _link: i32,
        ) -> Result<(), SitRefusal> {
            Err(SitRefusal::InvalidAgent)
        }
    }

    #[test]
    fn the_table_is_sorted_and_indexed_consistently() {
        assert_eq!(BUILTINS.len(), BuiltinId::ALL.len());
        for (index, id) in BuiltinId::ALL.iter().enumerate() {
            assert_eq!(id.index(), index);
            assert_eq!(id.descriptor().id, *id);
        }
        // An overloaded name's entries sit together, in their document's
        // order.
        assert!(BUILTINS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name || a.name == b.name && a.family.is_opensim(),
            _ => true,
        }));
        assert_eq!(overloads("osApproxEquals").len(), 6);
        assert_eq!(
            builtin("osApproxEquals").map(|b| b.id),
            Some(BuiltinId::OsApproxEquals)
        );
        assert!(overloads("llAbs").len() == 1 && overloads("nope").is_empty());
        assert!(CONSTANTS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name,
            _ => true,
        }));
        assert!(EVENTS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name,
            _ => true,
        }));
    }

    #[test]
    fn lookups_by_name() {
        assert_eq!(
            builtin("llSetPos").map(|b| (b.args.len(), b.ret, b.sleep.to_bits())),
            Some((1, None, 0.2_f32.to_bits()))
        );
        assert_eq!(
            builtin("llAbs").map(|b| (b.args, b.ret)),
            Some((
                &[Argument {
                    name: "val",
                    ty: TypeName::Integer
                }][..],
                Some(TypeName::Integer)
            ))
        );
        assert_eq!(builtin("llNoSuchFunction"), None);
        assert_eq!(event("touch_start").map(|e| e.args.len()), Some(1));
        assert_eq!(event("listen").map(|e| e.args.len()), Some(4));
        assert!(builtin("llMakeFire").is_some_and(|b| b.deprecated));
    }

    #[test]
    fn constants_decode_the_documents_notations() {
        let value = |name: &str| constant(name).map(|c| c.value.to_value());
        assert_eq!(value("TRUE"), Some(Value::Integer(1)));
        assert_eq!(value("CHANGED_INVENTORY"), Some(Value::Integer(1)));
        assert_eq!(value("PI"), Some(Value::Float(core::f32::consts::PI)));
        // `NULL_KEY` is a string in LSL, served as a UUID member.
        assert_eq!(
            value("NULL_KEY"),
            Some(Value::String(crate::value::NULL_KEY.to_owned()))
        );
        // Escaped twice: `EOF` and `NAK` with `\\n` and `\\x15`, the `JSON_*`
        // markers with `\\ufdd2`.
        assert_eq!(value("EOF"), Some(Value::String("\n\n\n".to_owned())));
        assert_eq!(value("NAK"), Some(Value::String("\n\u{15}\n".to_owned())));
        assert_eq!(
            value("JSON_ARRAY"),
            Some(Value::String("\u{FDD2}".to_owned()))
        );
        assert_eq!(
            value("TOUCH_INVALID_TEXCOORD"),
            Some(Value::Vector(Vector {
                x: -1.0,
                y: -1.0,
                z: 0.0,
            }))
        );
        assert_eq!(
            value("ZERO_ROTATION"),
            Some(Value::Rotation(Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            }))
        );
        // `default` is listed among the constants for highlighting only.
        assert_eq!(constant("default"), None);
    }

    /// The functions the grid compiles and serves no document for are in the
    /// table with the signatures tailslide's `builtins.txt` gives them, and
    /// in no served document.
    #[test]
    fn the_unserved_functions_have_tailslides_signatures() -> Result<(), String> {
        use TypeName::{Integer, Key, List, String as Text, Vector as Vec3};
        let expected: [(&str, &[TypeName], Option<TypeName>, f32); 5] = [
            ("llClearExperiencePermissions", &[Key], None, 0.0),
            ("llGetExperienceList", &[Key], Some(List), 0.0),
            ("llPointAt", &[Vec3], None, 0.0),
            (
                "llRemoteLoadScript",
                &[Key, Text, Integer, Integer],
                None,
                3.0,
            ),
            ("llStopPointAt", &[], None, 0.0),
        ];
        let syntax = decoded()?;
        let mut unserved = unserved()?;
        unserved.sort();
        let names: Vec<&str> = expected.iter().map(|(name, ..)| *name).collect();
        assert_eq!(unserved, names);
        for (name, args, ret, sleep) in expected {
            assert!(!syntax.functions.contains_key(name), "{name} is served");
            let descriptor = builtin(name).ok_or(name)?;
            let ours: Vec<TypeName> = descriptor.args.iter().map(|arg| arg.ty).collect();
            assert_eq!(ours, args, "{name}");
            assert_eq!(descriptor.ret, ret, "{name}");
            assert_eq!(descriptor.sleep.to_bits(), sleep.to_bits(), "{name}");
        }
        Ok(())
    }

    /// The vendored document, as `sl-wire`'s independent `LSLSyntax`
    /// decoder reads it.
    fn decoded() -> Result<sl_lsl::LslSyntax, String> {
        let source = include_str!("../keywords_lsl_default.xml");
        let document = sl_wire::parse_llsd_xml(source).map_err(|error| error.to_string())?;
        sl_wire::parse_lsl_syntax(&document).map_err(|error| error.to_string())
    }

    /// The names of the functions `keywords_lsl_unserved.xml` adds.
    fn unserved() -> Result<Vec<String>, String> {
        let source = include_str!("../keywords_lsl_unserved.xml");
        let document = sl_wire::parse_llsd_xml(source).map_err(|error| error.to_string())?;
        let functions = document
            .get("functions")
            .and_then(sl_wire::Llsd::as_map)
            .ok_or("the supplement has no functions map")?;
        Ok(functions.keys().cloned().collect())
    }

    /// Second Life's served document's part of the table, function by
    /// function — the supplement's functions are in no served document to
    /// compare with, and OpenSim's are [`opensim_part_matches_its_document`]'s.
    #[test]
    fn the_generated_table_matches_an_independent_decoder() -> Result<(), String> {
        let syntax = decoded()?;
        let unserved = unserved()?;
        let linden = || {
            BUILTINS
                .iter()
                .filter(|descriptor| descriptor.family == Family::Linden)
        };
        assert_eq!(
            syntax.functions.len().saturating_add(unserved.len()),
            linden().count()
        );
        for descriptor in
            linden().filter(|descriptor| !unserved.iter().any(|name| name == descriptor.name))
        {
            let function = syntax
                .functions
                .get(descriptor.name)
                .ok_or_else(|| format!("{} missing", descriptor.name))?;
            let types: Vec<Option<TypeName>> =
                function.arguments.iter().map(|a| a.arg_type).collect();
            let ours: Vec<Option<TypeName>> = descriptor.args.iter().map(|a| Some(a.ty)).collect();
            assert_eq!(ours, types, "{}", descriptor.name);
            assert_eq!(descriptor.ret, function.return_type, "{}", descriptor.name);
            assert_eq!(
                Some(descriptor.sleep.to_bits()),
                function.sleep.map(f32::to_bits),
                "{}",
                descriptor.name
            );
            assert_eq!(
                Some(descriptor.energy.to_bits()),
                function.energy.map(f32::to_bits),
                "{}",
                descriptor.name
            );
            assert_eq!(
                descriptor.deprecated, function.deprecated,
                "{}",
                descriptor.name
            );
            assert_eq!(
                descriptor.god_mode, function.god_mode,
                "{}",
                descriptor.name
            );
        }
        assert_eq!(syntax.events.len(), EVENTS.len());
        for event in &EVENTS {
            let decoded = syntax.events.get(event.name).ok_or(event.name)?;
            let names: Vec<&str> = decoded.arguments.iter().map(|a| a.name.as_str()).collect();
            let ours: Vec<&str> = event.args.iter().map(|a| a.name).collect();
            assert_eq!(ours, names, "{}", event.name);
        }
        // Every constant but the `default` placeholder, with the served text
        // cast by the runtime's own LSL parsers — except the escaped strings
        // the table decodes.
        let linden_constants = || CONSTANTS.iter().filter(|constant| !constant.opensim_only);
        assert_eq!(
            syntax.constants.len(),
            linden_constants().count().saturating_add(1)
        );
        for constant in linden_constants() {
            let decoded = syntax.constants.get(constant.name).ok_or(constant.name)?;
            assert_eq!(
                decoded.constant_type,
                Some(constant.value.type_name()),
                "{}",
                constant.name
            );
            let text = decoded.value.clone().unwrap_or_default();
            let notation = text.contains('\\');
            if !notation {
                let parsed = crate::cast(Value::String(text), constant.value.type_name())
                    .map_err(|error| error.to_string())?;
                assert_eq!(parsed, constant.value.to_value(), "{}", constant.name);
            }
        }
        Ok(())
    }

    /// OpenSim's part of the table against its document as `sl-wire`'s
    /// decoder reads it: every function only OpenSim has is in the table,
    /// one of its overloads with the decoder's signature (the decoder keeps
    /// one entry of a name), and in the family its prefix says; every
    /// constant only OpenSim has with the decoder's type.
    #[test]
    fn opensim_part_matches_its_document() -> Result<(), String> {
        let file = include_str!("../opensim/ScriptSyntax.xml");
        let (_id, text) = file.split_once('\n').ok_or("no syntax id line")?;
        let document = sl_wire::parse_llsd_xml(text).map_err(|error| error.to_string())?;
        let syntax = sl_wire::parse_lsl_syntax(&document).map_err(|error| error.to_string())?;
        let mut checked = 0_usize;
        for (name, function) in &syntax.functions {
            let ours = overloads(name);
            let Some(first) = ours.first() else {
                return Err(format!("{name} is not in the table"));
            };
            if first.family == Family::Linden {
                continue;
            }
            let family = match name.get(..2) {
                Some("os") => Family::Ossl,
                Some("ls") => Family::LightShare,
                _ => Family::OpenSim,
            };
            assert_eq!(first.family, family, "{name}");
            let types: Vec<Option<TypeName>> =
                function.arguments.iter().map(|a| a.arg_type).collect();
            assert!(
                ours.iter().any(|overload| {
                    overload.ret == function.return_type
                        && overload.args.iter().map(|a| Some(a.ty)).collect::<Vec<_>>() == types
                }),
                "{name}"
            );
            checked = checked.saturating_add(1);
        }
        assert_eq!(checked, 228);
        for constant in CONSTANTS.iter().filter(|constant| constant.opensim_only) {
            let decoded = syntax.constants.get(constant.name).ok_or(constant.name)?;
            assert_eq!(
                decoded.constant_type,
                Some(constant.value.type_name()),
                "{}",
                constant.name
            );
        }
        // The supplement's: the MOD family and the OSSL functions the document
        // leaves out.
        for (name, family) in [
            ("modSendCommand", Family::Mod),
            ("osListenRegex", Family::Ossl),
            ("osTriggerSoundLimited", Family::Ossl),
        ] {
            assert_eq!(builtin(name).map(|b| b.family), Some(family), "{name}");
        }
        Ok(())
    }

    #[test]
    fn calls_are_checked_against_the_table() {
        let mut host = Unreached::default();
        let mut data = ScriptData::default();
        let mut memory = Memory::bare();
        let mut ctx = ScriptCtx::new(
            CallerId(1),
            Tick(0),
            core::time::Duration::from_millis(100),
            &mut host,
            &mut data,
            Peers::NONE,
            (&mut memory, Stack::default()),
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![Value::Integer(-3)]),
            Ok(Called {
                value: Some(Value::Integer(3)),
                stubbed: false,
            })
        );
        assert_eq!(
            call(
                BuiltinId::LlGetListLength,
                &mut ctx,
                vec![Value::List(vec![Element::Integer(1)])]
            ),
            Ok(Called {
                value: Some(Value::Integer(1)),
                stubbed: false,
            })
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![]),
            Err(CallError::Arity {
                id: BuiltinId::LlAbs,
                expected: 1,
                found: 0,
            })
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![Value::Float(1.0)]),
            Err(CallError::Argument {
                id: BuiltinId::LlAbs,
                index: 0,
                expected: TypeName::Integer,
                found: TypeName::Float,
            })
        );
        assert_eq!(
            call(BuiltinId::LlSetBuoyancy, &mut ctx, vec![Value::Float(1.0)]),
            Err(CallError::Missing(BuiltinId::LlSetBuoyancy))
        );
    }

    /// A registry of its own, to exercise the stub path.
    #[allow(
        unreachable_pub,
        clippy::allow_attributes,
        reason = "the macro declares `pub` items for the crate's registry"
    )]
    mod with_stubs {
        crate::registry! {
            implemented { LlAbs => crate::library::math::ll_abs }
            stubbed { LlGetPos, LlSay }
        }
    }

    #[test]
    fn a_stub_checks_its_arguments_and_returns_the_default() {
        let mut host = Unreached::default();
        let mut data = ScriptData::default();
        let mut memory = Memory::bare();
        let mut ctx = ScriptCtx::new(
            CallerId(1),
            Tick(0),
            core::time::Duration::from_millis(100),
            &mut host,
            &mut data,
            Peers::NONE,
            (&mut memory, Stack::default()),
        );
        assert_eq!(
            with_stubs::call(BuiltinId::LlGetPos, &mut ctx, vec![]),
            Ok(Called {
                value: Some(Value::Vector(crate::value::ZERO_VECTOR)),
                stubbed: true,
            })
        );
        assert_eq!(
            with_stubs::call(
                BuiltinId::LlSay,
                &mut ctx,
                vec![Value::Integer(0), Value::String("hi".to_owned())]
            ),
            Ok(Called {
                value: None,
                stubbed: true,
            })
        );
        assert_eq!(
            with_stubs::call(BuiltinId::LlSay, &mut ctx, vec![Value::Integer(0)]),
            Err(CallError::Arity {
                id: BuiltinId::LlSay,
                expected: 2,
                found: 1,
            })
        );
        assert_eq!(with_stubs::status(BuiltinId::LlSay), Status::Stubbed);
        assert_eq!(with_stubs::status(BuiltinId::LlAbs), Status::Implemented);
        assert_eq!(with_stubs::status(BuiltinId::LlFabs), Status::Missing);
    }

    /// The committed baseline: function name to the status it must not fall
    /// below.
    fn baseline() -> Result<BTreeMap<&'static str, Status>, String> {
        let mut entries = BTreeMap::new();
        for line in include_str!("library/coverage.txt").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (status, name) = line
                .split_once(' ')
                .ok_or_else(|| format!("coverage.txt: `{line}`"))?;
            let status = match status {
                "implemented" => Status::Implemented,
                "stubbed" => Status::Stubbed,
                other => return Err(format!("coverage.txt: status `{other}`")),
            };
            if builtin(name).is_none() {
                return Err(format!("coverage.txt: no function `{name}`"));
            }
            let _previous = entries.insert(name, status);
        }
        Ok(entries)
    }

    /// The committed scope of OpenSim's own library: function name to
    /// `in`, `later` or `out`.
    fn opensim_scope() -> Result<BTreeMap<&'static str, &'static str>, String> {
        let mut entries = BTreeMap::new();
        for line in include_str!("library/opensim-scope.txt").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (scope, name) = match fields.as_slice() {
                ["in", name] => ("in", *name),
                ["later" | "out", _why, name] => (fields.first().copied().unwrap_or(""), *name),
                _ => return Err(format!("opensim-scope.txt: `{line}`")),
            };
            if builtin(name).is_none_or(|builtin| !builtin.family.is_opensim()) {
                return Err(format!("opensim-scope.txt: no OpenSim function `{name}`"));
            }
            if entries.insert(name, scope).is_some() {
                return Err(format!("opensim-scope.txt: `{name}` twice"));
            }
        }
        Ok(entries)
    }

    #[test]
    fn coverage_never_regresses() -> Result<(), String> {
        let baseline = baseline()?;
        let mut counts: BTreeMap<Status, usize> = BTreeMap::new();
        let mut regressed = Vec::new();
        let mut unlisted = Vec::new();
        let linden = BUILTINS
            .iter()
            .filter(|descriptor| descriptor.family == Family::Linden);
        for descriptor in linden.clone() {
            let now = status(descriptor.id);
            let entry = counts.entry(now).or_default();
            *entry = entry.saturating_add(1);
            let expected = baseline
                .get(descriptor.name)
                .copied()
                .unwrap_or(Status::Missing);
            if now < expected {
                regressed.push(format!("{} ({expected:?} → {now:?})", descriptor.name));
            } else if now > expected {
                unlisted.push(format!("{now:?} {}", descriptor.name));
            }
        }
        let count = |status| counts.get(&status).copied().unwrap_or(0);
        eprintln!(
            "LSL library coverage: {} implemented / {} stubbed / {} missing of {}",
            count(Status::Implemented),
            count(Status::Stubbed),
            count(Status::Missing),
            linden.count()
        );

        // OpenSim's own families are held to their scope instead: what is
        // in is implemented, overload by overload, and nothing else is.
        let scope = opensim_scope()?;
        let mut scoped: BTreeMap<&str, usize> = BTreeMap::new();
        for descriptor in BUILTINS
            .iter()
            .filter(|descriptor| descriptor.family.is_opensim())
        {
            let Some(in_scope) = scope.get(descriptor.name).copied() else {
                unlisted.push(format!(
                    "{} has no line in opensim-scope.txt",
                    descriptor.name
                ));
                continue;
            };
            let implemented = status(descriptor.id) == Status::Implemented;
            if in_scope == "in" && !implemented {
                regressed.push(format!(
                    "{} ({:?} is in scope)",
                    descriptor.name, descriptor.id
                ));
            } else if in_scope != "in" && implemented {
                unlisted.push(format!("{} is implemented: mark it `in`", descriptor.name));
            }
            if overloads(descriptor.name).first().map(|first| first.id) == Some(descriptor.id) {
                let entry = scoped.entry(in_scope).or_default();
                *entry = entry.saturating_add(1);
            }
        }
        let scoped_count = |scope| scoped.get(scope).copied().unwrap_or(0);
        eprintln!(
            "OpenSim's own library: {} implemented / {} planned / {} out of scope of {}",
            scoped_count("in"),
            scoped_count("later"),
            scoped_count("out"),
            scope.len()
        );
        if !unlisted.is_empty() {
            eprintln!("list them: {}", unlisted.join(", "));
        }
        if regressed.is_empty() {
            Ok(())
        } else {
            Err(format!("coverage regressed: {}", regressed.join(", ")))
        }
    }
}
