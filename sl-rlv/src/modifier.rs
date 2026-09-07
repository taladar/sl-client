//! The **behaviour-modifier value system** — the typed knobs a restriction
//! carries, and the most-restrictive-wins rule that picks the value in force.
//!
//! A modifier is not a restriction: `@fartouch=n` restricts touch range, and
//! `@fartouch:2.5=n` restricts it *and* says how far. The distance is a value in
//! the [`RlvModifier::FartouchDist`] slot, contributed by that object, and the
//! slot answers with the value in force across every object that contributed
//! one. Where "in force" is ambiguous the reference picks the most restrictive
//! — the smallest touch range, the largest minimum IM distance — which is what
//! [`RlvComparator`] encodes (`RlvBehaviourModifierCompMin` /
//! `RlvBehaviourModifierCompMax`, `rlvmodifiers.h:56-73`).
//!
//! There are 21 such slots ([`RlvModifier`], `ERlvBehaviourModifier`). They are
//! *global*: every object writes into the same slot. That is what distinguishes
//! them from the 13
//! [`RlvLocalModifier`](crate::RlvLocalModifier)s, which are per-object values
//! on the object's own [`@setsphere`](crate::RlvBehaviour::Setsphere) or
//! [`@setoverlay`](crate::RlvBehaviour::Setoverlay) effect and never compete.
//!
//! Reference (Firestorm, read-only): `rlvdefines.h` (`ERlvBehaviourModifier`),
//! `rlvhelper.cpp:554-712` (`RlvBehaviourModifier`), `rlvmodifiers.h`.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::behaviour::{RlvBehaviour, RlvValueType};

/// The default `@fartouch` radius in metres (`RLV_MODIFIER_FARTOUCH_DEFAULT`).
pub const FARTOUCH_DEFAULT: f32 = 1.5;
/// The default `@sittp` radius in metres (`RLV_MODIFIER_SITTP_DEFAULT`).
pub const SITTP_DEFAULT: f32 = 1.5;
/// The default `@tplocal` radius in metres (`RLV_MODIFIER_TPLOCAL_DEFAULT`).
///
/// Anything more than a region away is not a local teleport.
pub const TPLOCAL_DEFAULT: f32 = 256.0;
/// The viewer's default vertical field of view, in radians
/// (`DEFAULT_FIELD_OF_VIEW`, `llcamera.h:36` — 60°).
pub const DEFAULT_FIELD_OF_VIEW: f32 = 60.0 * core::f32::consts::PI / 180.0;
/// The "default" texture the viewer draws for an unresolved asset
/// (`IMG_DEFAULT`, `indra_constants.cpp:43`).
pub const IMG_DEFAULT: Uuid = Uuid::from_u128(0xd211_4404_dd59_4a4d_8e6c_4935_9e91_bbf0);

/// The value a behaviour modifier holds.
///
/// The reference stores this as a `boost::variant` and refuses a write whose
/// arm differs from the slot's default (`RlvBehaviourModifier::addValue`,
/// `rlvhelper.cpp:571`); [`RlvModifierValue::value_type`] is that check.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum RlvModifierValue {
    /// A distance, an angle, an alpha or a duration.
    Float(f32),
    /// A mode selector.
    Int(i32),
    /// An offset or a colour, parsed from `x/y/z`.
    Vector3([f32; 3]),
    /// Effect parameters, parsed from `x/y/z/w`.
    Vector4([f32; 4]),
    /// A texture.
    Uuid(Uuid),
}

impl RlvModifierValue {
    /// Which arm this value is — the type a slot checks a write against.
    #[must_use]
    pub const fn value_type(self) -> RlvValueType {
        match self {
            Self::Float(_) => RlvValueType::Float,
            Self::Int(_) => RlvValueType::Int,
            Self::Vector3(_) => RlvValueType::Vector3,
            Self::Vector4(_) => RlvValueType::Vector4,
            Self::Uuid(_) => RlvValueType::Uuid,
        }
    }

    /// The float this value holds, or `None` for any other arm.
    #[must_use]
    pub const fn as_float(self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(value),
            _ => None,
        }
    }

    /// The integer this value holds, or `None` for any other arm.
    #[must_use]
    pub const fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(value),
            _ => None,
        }
    }

    /// The three-component vector this value holds, or `None` for any other arm.
    #[must_use]
    pub const fn as_vector3(self) -> Option<[f32; 3]> {
        match self {
            Self::Vector3(value) => Some(value),
            _ => None,
        }
    }

    /// The four-component vector this value holds, or `None` for any other arm.
    #[must_use]
    pub const fn as_vector4(self) -> Option<[f32; 4]> {
        match self {
            Self::Vector4(value) => Some(value),
            _ => None,
        }
    }

    /// The UUID this value holds, or `None` for any other arm.
    #[must_use]
    pub const fn as_uuid(self) -> Option<Uuid> {
        match self {
            Self::Uuid(value) => Some(value),
            _ => None,
        }
    }

    /// Parse a command option into a value of `value_type`, or `None` when the
    /// option does not spell one.
    ///
    /// This is `RlvBehaviourModifier::convertOptionValue`
    /// (`rlvhelper.cpp:664`), with two deliberate tightenings: the numeric arms
    /// reject trailing garbage where `std::stof` / `std::stoi` would stop at the
    /// first bad character, and a UUID must be the 36-character hyphenated form
    /// `LLUUID::parseUUID` insists on.
    #[must_use]
    pub fn parse(option: &str, value_type: RlvValueType) -> Option<Self> {
        match value_type {
            RlvValueType::Float => option.parse::<f32>().ok().map(Self::Float),
            RlvValueType::Int => option.parse::<i32>().ok().map(Self::Int),
            RlvValueType::Vector3 => parse_floats::<3>(option).map(Self::Vector3),
            RlvValueType::Vector4 => parse_floats::<4>(option).map(Self::Vector4),
            RlvValueType::Uuid => (option.len() == 36)
                .then(|| Uuid::parse_str(option).ok())
                .flatten()
                .map(Self::Uuid),
        }
    }

    /// The bit pattern of every float this value holds, for equality.
    ///
    /// The reference compares modifier values with `operator==` so it can find
    /// the exact value an object contributed and take it away again; comparing
    /// bit patterns is the same test, and unlike `==` it is reflexive, which is
    /// what lets this type be [`Eq`].
    fn key(self) -> (RlvValueType, [u32; 4], i32, u128) {
        let mut bits = [0_u32; 4];
        let mut int = 0_i32;
        let mut uuid = 0_u128;
        match self {
            Self::Float(value) => {
                if let Some(slot) = bits.first_mut() {
                    *slot = value.to_bits();
                }
            }
            Self::Int(value) => int = value,
            Self::Vector3(value) => {
                for (slot, component) in bits.iter_mut().zip(value) {
                    *slot = component.to_bits();
                }
            }
            Self::Vector4(value) => {
                for (slot, component) in bits.iter_mut().zip(value) {
                    *slot = component.to_bits();
                }
            }
            Self::Uuid(value) => uuid = value.as_u128(),
        }
        (self.value_type(), bits, int, uuid)
    }
}

/// Split `option` on `/` into exactly `N` floats, as the reference's
/// `sscanf("%f/%f/%f")` does.
fn parse_floats<const N: usize>(option: &str) -> Option<[f32; N]> {
    let mut out = [0.0_f32; N];
    let mut parts = option.split('/');
    for slot in &mut out {
        *slot = parts.next()?.parse::<f32>().ok()?;
    }
    parts.next().is_none().then_some(out)
}

impl PartialEq for RlvModifierValue {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for RlvModifierValue {}

impl core::hash::Hash for RlvModifierValue {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

/// How a slot orders the values several objects contributed, so the one in
/// force is at the front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvComparator {
    /// No ordering beyond the primary object's precedence — the value in force
    /// is whichever arrived first (`RlvBehaviourModifierComp`).
    Insertion,
    /// The smallest value wins (`RlvBehaviourModifierCompMin`) — a maximum
    /// distance, a maximum field of view.
    Min,
    /// The largest value wins (`RlvBehaviourModifierCompMax`) — a minimum
    /// distance, a minimum field of view.
    Max,
}

/// Declarative table of the global behaviour modifiers
/// (`ERlvBehaviourModifier`).
///
/// Each row is
/// `Variant = Behaviour "name" <type> default <expr> add_default <bool> <comparator>`:
/// the restriction that owns the slot, the reference's display name for it, the
/// type its values have, the value in force when no object has contributed one,
/// whether a bare `@bhvr=n` contributes that default, and how competing values
/// are ordered.
macro_rules! rlv_modifiers {
    (
        $(
            $variant:ident = $base:ident $name:literal $ty:ident
                default $default:expr, add_default $add_default:literal, $comparator:ident ;
        )*
    ) => {
        /// A global behaviour-modifier slot (`ERlvBehaviourModifier`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum RlvModifier {
            $(
                #[doc = concat!("`", $name, "` — the modifier of `@", stringify!($base), "`.")]
                $variant,
            )*
        }

        impl RlvModifier {
            /// Every modifier slot, in table order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The restriction that owns this slot.
            #[must_use]
            pub const fn behaviour(self) -> RlvBehaviour {
                match self {
                    $( Self::$variant => RlvBehaviour::$base, )*
                }
            }

            /// The reference's display name for this slot, as the RLVa
            /// behaviour floater shows it.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $( Self::$variant => $name, )*
                }
            }

            /// The type this slot's values have.
            #[must_use]
            pub const fn value_type(self) -> RlvValueType {
                match self {
                    $( Self::$variant => RlvValueType::$ty, )*
                }
            }

            /// The value in force when no object has contributed one.
            #[must_use]
            pub const fn default_value(self) -> RlvModifierValue {
                match self {
                    $( Self::$variant => $default, )*
                }
            }

            /// Whether a bare `@<behaviour>=n` contributes
            /// [`RlvModifier::default_value`] on this slot
            /// (`RlvBehaviourModifier::getAddDefault`).
            ///
            /// Where this is `false` the restriction is on but the slot stays
            /// empty until some object names a value — which is how
            /// `@setcam_avdist=n` can be held without pinning a distance.
            #[must_use]
            pub const fn add_default_on_empty(self) -> bool {
                match self {
                    $( Self::$variant => $add_default, )*
                }
            }

            /// How this slot orders competing values.
            #[must_use]
            pub const fn comparator(self) -> RlvComparator {
                match self {
                    $( Self::$variant => RlvComparator::$comparator, )*
                }
            }

            /// The slot `behaviour` owns, or `None` when it has no modifier.
            ///
            /// This is `getModifierFromBehaviour` (`rlvhelper.cpp:498`).
            #[must_use]
            pub fn of_behaviour(behaviour: RlvBehaviour) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|modifier| modifier.behaviour() == behaviour)
            }
        }
    };
}

rlv_modifiers! {
    FartouchDist = Fartouch "Fartouch Distance" Float
        default RlvModifierValue::Float(FARTOUCH_DEFAULT), add_default true, Min;
    RecvImDistMin = Recvim "RecvIM Distance (Min)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Max;
    RecvImDistMax = Recvim "RecvIM Distance (Max)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Min;
    SendImDistMin = Sendim "SendIM Distance (Min)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Max;
    SendImDistMax = Sendim "SendIM Distance (Max)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Min;
    StartImDistMin = Startim "StartIM Distance (Min)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Max;
    StartImDistMax = Startim "StartIM Distance (Max)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Min;
    SetcamAvdist = SetcamAvdist "Camera - Silhouette Distance" Float
        default RlvModifierValue::Float(0.0), add_default false, Max;
    SetcamAvdistmin = SetcamAvdistmin "Camera - Avatar Distance (Min)" Float
        default RlvModifierValue::Float(0.0), add_default false, Max;
    SetcamAvdistmax = SetcamAvdistmax "Camera - Avatar Distance (Max)" Float
        default RlvModifierValue::Float(f32::MAX), add_default false, Min;
    SetcamOrigindistmin = SetcamOrigindistmin "Camera - Focus Distance (Min)" Float
        default RlvModifierValue::Float(0.0), add_default true, Max;
    SetcamOrigindistmax = SetcamOrigindistmax "Camera - Focus Distance (Max)" Float
        default RlvModifierValue::Float(f32::MAX), add_default true, Min;
    SetcamEyeoffset = SetcamEyeoffset "Camera - Eye Offset" Vector3
        default RlvModifierValue::Vector3([0.0, 0.0, 0.0]), add_default true, Insertion;
    SetcamEyeoffsetscale = SetcamEyeoffsetscale "Camera - Eye Offset Scale" Float
        default RlvModifierValue::Float(0.0), add_default true, Insertion;
    SetcamFocusoffset = SetcamFocusoffset "Camera - Focus Offset" Vector3
        default RlvModifierValue::Vector3([0.0, 0.0, 0.0]), add_default true, Insertion;
    SetcamFovmin = SetcamFovmin "Camera - FOV (Min)" Float
        default RlvModifierValue::Float(DEFAULT_FIELD_OF_VIEW), add_default true, Max;
    SetcamFovmax = SetcamFovmax "Camera - FOV (Max)" Float
        default RlvModifierValue::Float(DEFAULT_FIELD_OF_VIEW), add_default true, Min;
    SetcamTexture = SetcamTextures "Camera - Forced Texture" Uuid
        default RlvModifierValue::Uuid(IMG_DEFAULT), add_default true, Insertion;
    ShownametagsDist = Shownametags "Name Tags - Visible Distance" Float
        default RlvModifierValue::Float(0.0), add_default true, Min;
    SittpDist = Sittp "SitTp Distance" Float
        default RlvModifierValue::Float(SITTP_DEFAULT), add_default true, Min;
    TplocalDist = Tplocal "Local Teleport Distance" Float
        default RlvModifierValue::Float(TPLOCAL_DEFAULT), add_default true, Min;
}

/// One object's contribution to a modifier slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RlvModifierEntry {
    /// The value contributed.
    value: RlvModifierValue,
    /// The object that contributed it.
    object: Uuid,
    /// The restriction it came in on, or `None` when it was written directly
    /// rather than carried by a restriction (the reference's
    /// `RLV_BHVR_UNKNOWN`). Only `None` entries are dropped by
    /// [`RlvModifierSlot::clear_object`], because the others are owned by a
    /// restriction that will take them away itself.
    behaviour: Option<RlvBehaviour>,
}

/// The values contributed to one modifier slot, ordered so the value in force
/// is at the front.
#[derive(Debug, Clone, Default)]
struct RlvModifierSlot {
    /// The contributions, most-restrictive first.
    values: Vec<RlvModifierEntry>,
    /// The object whose contributions outrank everyone else's, if any.
    primary: Option<Uuid>,
}

impl RlvModifierSlot {
    /// Whether `left` sorts before `right` under `comparator`, given the slot's
    /// primary object.
    ///
    /// This is the reference's three comparator functors in one
    /// (`rlvmodifiers.h:41-73`). The primary object's values always sort first;
    /// among values that are all the primary object's — or when there is no
    /// primary object — [`RlvComparator::Min`] and [`RlvComparator::Max`] order
    /// by the value itself and [`RlvComparator::Insertion`] keeps arrival
    /// order.
    fn sorts_first(
        &self,
        left: &RlvModifierEntry,
        right: &RlvModifierEntry,
        comparator: RlvComparator,
    ) -> bool {
        let both_primary = self
            .primary
            .is_some_and(|primary| left.object == primary && right.object == primary);
        if self.primary.is_none() || both_primary {
            let ordered = match comparator {
                RlvComparator::Insertion => None,
                RlvComparator::Min => left
                    .value
                    .as_float()
                    .zip(right.value.as_float())
                    .map(|(left, right)| left < right),
                RlvComparator::Max => left
                    .value
                    .as_float()
                    .zip(right.value.as_float())
                    .map(|(left, right)| right < left),
            };
            if let Some(ordered) = ordered {
                return ordered;
            }
        }
        // The base rule: a value from the primary object never sorts after one
        // that is not, and everything else keeps its relative order.
        !self
            .primary
            .is_some_and(|primary| right.object == primary && left.object != primary)
    }

    /// Insert `entry` at the position `comparator` puts it.
    fn insert(&mut self, entry: RlvModifierEntry, comparator: RlvComparator) {
        let at = self
            .values
            .iter()
            .position(|existing| !self.sorts_first(existing, &entry, comparator))
            .unwrap_or(self.values.len());
        self.values.insert(at, entry);
    }

    /// Re-order every contribution, after the primary object changed.
    fn resort(&mut self, comparator: RlvComparator) {
        let mut sorted: Vec<RlvModifierEntry> = Vec::with_capacity(self.values.len());
        for entry in core::mem::take(&mut self.values) {
            let at = sorted
                .iter()
                .position(|existing| !self.sorts_first(existing, &entry, comparator))
                .unwrap_or(sorted.len());
            sorted.insert(at, entry);
        }
        self.values = sorted;
    }
}

/// Every global modifier slot and the values objects have contributed to it.
///
/// This is the reference's array of `RlvBehaviourModifier*`
/// (`RlvBehaviourDictionary::m_BehaviourModifiers`), lifted out of the
/// dictionary: the table of slots is static, the values in them are state.
#[derive(Debug, Clone, Default)]
pub struct RlvModifierState {
    /// One slot per [`RlvModifier`] that anything has been contributed to.
    slots: BTreeMap<RlvModifier, RlvModifierSlot>,
}

impl RlvModifierState {
    /// An empty set of slots — every modifier at its default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Contribute `value` to `modifier` on behalf of `object`.
    ///
    /// `behaviour` is the restriction the value arrived on, or `None` when it
    /// was written directly (`RlvBehaviourModifier::addValue`,
    /// `rlvhelper.cpp:569`). A value whose type does not match the slot is
    /// refused, and `false` comes back.
    pub fn add_value(
        &mut self,
        modifier: RlvModifier,
        value: RlvModifierValue,
        object: Uuid,
        behaviour: Option<RlvBehaviour>,
    ) -> bool {
        if value.value_type() != modifier.value_type() {
            return false;
        }
        self.slots.entry(modifier).or_default().insert(
            RlvModifierEntry {
                value,
                object,
                behaviour,
            },
            modifier.comparator(),
        );
        true
    }

    /// Take back the exact contribution `add_value` made
    /// (`RlvBehaviourModifier::removeValue`, `rlvhelper.cpp:613`).
    ///
    /// Returns whether a matching contribution was found.
    pub fn remove_value(
        &mut self,
        modifier: RlvModifier,
        value: RlvModifierValue,
        object: Uuid,
        behaviour: Option<RlvBehaviour>,
    ) -> bool {
        if value.value_type() != modifier.value_type() {
            return false;
        }
        let slot = self.slots.entry(modifier).or_default();
        let found = slot.values.iter().position(|entry| {
            entry.value == value && entry.object == object && entry.behaviour == behaviour
        });
        match found {
            Some(index) => {
                slot.values.remove(index);
                true
            }
            None => false,
        }
    }

    /// Write `object`'s standalone value for `modifier`, replacing the one it
    /// already wrote (`RlvBehaviourModifier::setValue`, `rlvhelper.cpp:638`).
    ///
    /// "Standalone" is the reference's `RLV_BHVR_UNKNOWN` tag: a value not
    /// carried by a restriction, and so the only kind
    /// [`RlvModifierState::clear_object`] takes away.
    pub fn set_value(
        &mut self,
        modifier: RlvModifier,
        value: RlvModifierValue,
        object: Uuid,
    ) -> bool {
        if value.value_type() != modifier.value_type() {
            return false;
        }
        let comparator = modifier.comparator();
        let slot = self.slots.entry(modifier).or_default();
        let existing = slot
            .values
            .iter()
            .position(|entry| entry.object == object && entry.behaviour.is_none());
        match existing {
            Some(index) => {
                if let Some(entry) = slot.values.get_mut(index) {
                    entry.value = value;
                }
                slot.resort(comparator);
                true
            }
            None => self.add_value(modifier, value, object, None),
        }
    }

    /// Drop every standalone value `object` wrote, across every slot
    /// (`RlvBehaviourDictionary::clearModifiers`, `rlvhelper.cpp:413`).
    ///
    /// Values an object contributed *through a restriction* survive: the
    /// restriction takes them away when it is lifted, and lifting the last one
    /// is what calls this.
    pub fn clear_object(&mut self, object: Uuid) {
        for slot in self.slots.values_mut() {
            slot.values
                .retain(|entry| !(entry.object == object && entry.behaviour.is_none()));
        }
    }

    /// The value in force on `modifier` — the front contribution, or the slot's
    /// default when nothing has been contributed
    /// (`RlvBehaviourModifier::getValue`).
    #[must_use]
    pub fn value(&self, modifier: RlvModifier) -> RlvModifierValue {
        self.slots
            .get(&modifier)
            .and_then(|slot| slot.values.first())
            .map_or_else(|| modifier.default_value(), |entry| entry.value)
    }

    /// Whether `modifier` has a contributed value at all.
    ///
    /// With a primary object set this narrows to "does the *primary* object
    /// have one", exactly as `RlvBehaviourModifier::hasValue`
    /// (`rlvhelper.cpp:601`) does: once an object has exclusive control, the
    /// others' values are not in force even though they are still on the list.
    #[must_use]
    pub fn has_value(&self, modifier: RlvModifier) -> bool {
        self.slots.get(&modifier).is_some_and(|slot| {
            slot.primary.map_or(!slot.values.is_empty(), |primary| {
                slot.values
                    .first()
                    .is_some_and(|entry| entry.object == primary)
            })
        })
    }

    /// Whether `object` has contributed anything to `modifier`.
    #[must_use]
    pub fn has_value_from(&self, modifier: RlvModifier, object: Uuid) -> bool {
        self.slots
            .get(&modifier)
            .is_some_and(|slot| slot.values.iter().any(|entry| entry.object == object))
    }

    /// The object whose contributions to `modifier` outrank everyone else's.
    #[must_use]
    pub fn primary_object(&self, modifier: RlvModifier) -> Option<Uuid> {
        self.slots.get(&modifier).and_then(|slot| slot.primary)
    }

    /// Give `object` — or nobody, for `None` — the last word on `modifier`, and
    /// re-order the slot (`RlvBehaviourModifier::setPrimaryObject`,
    /// `rlvhelper.cpp:627`).
    ///
    /// This is how `@setcam=n` works: the object that took exclusive control of
    /// the camera becomes the primary object of every camera slot, so its
    /// values win outright instead of competing on most-restrictive-wins.
    pub fn set_primary_object(&mut self, modifier: RlvModifier, object: Option<Uuid>) {
        let comparator = modifier.comparator();
        let slot = self.slots.entry(modifier).or_default();
        slot.primary = object;
        slot.resort(comparator);
    }

    /// Every slot that has at least one contribution, in [`RlvModifier`] order,
    /// paired with the value in force.
    pub fn active(&self) -> impl Iterator<Item = (RlvModifier, RlvModifierValue)> {
        self.slots.iter().filter_map(|(&modifier, slot)| {
            slot.values.first().map(|entry| (modifier, entry.value))
        })
    }
}
