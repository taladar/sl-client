//! Two Second Life region capabilities of the Region/Estate floater's
//! **Region** tab: **`DispatchRegionInfo`**, the region settings written over
//! HTTP, and **`RegionSchedule`**, the region's weekly restart.
//!
//! `DispatchRegionInfo` takes a POST of the General tab's controls
//! (`LLPanelRegionGeneralInfo::sendUpdate`):
//!
//! ```text
//! { block_terraform, block_fly, block_fly_over, allow_damage,
//!   allow_land_resell, agent_limit, prim_bonus, sim_access,
//!   restrict_pushobject, allow_parcel_changes, block_parcel_search }
//! ```
//!
//! — two more settings than the UDP `setregioninfo` it replaces can carry
//! (`block_fly_over`, `block_parcel_search`). The reference sends the booleans
//! as booleans, the two spinners as reals and the maturity as an integer.
//!
//! `RegionSchedule` answers a GET with the restart schedule, and takes a POST
//! of the same shape (`LLFloaterRegionRestartSchedule`):
//!
//! ```text
//! { restart: { type: "W" | "D", days: "SMTWRFA" subset, time: <seconds after midnight> } }
//! ```
//!
//! A `D` (daily) schedule names no days; a `W` one names its days by letter —
//! `s m t w r f a`, Sunday to Saturday — in either case. An answer without
//! `restart` means no schedule — Second Life answers a region with none
//! `undef` — and a POST of type `W` with no days clears it.
//!
//! A third, **`ModifyRegion`**, carries the region's PBR terrain overrides
//! (`LLPBRTerrainFeatures`): a GET answers
//!
//! ```text
//! { success: bool, message?: string, overrides: [ <override>, ×4 ] }
//! ```
//!
//! — one GLTF material override document per terrain material slot, the
//! reference's `LLGLTFMaterial::getOverrideLLSD` form (an empty map for
//! none) — and a POST of `{ overrides: [ ×4 ] }` writes them, answered
//! `{ success, message? }`. The reference asks on every region handshake
//! where the region grants it.
//!
//! This module builds and reads all three, on both sides.

use std::collections::HashMap;

use crate::WireError;
use crate::llsd::Llsd;

/// The settings a `DispatchRegionInfo` POST writes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one of the capability's region toggles"
)]
pub struct RegionInfoDispatch {
    /// Terraforming is blocked (`block_terraform`).
    pub block_terraform: bool,
    /// Flying is blocked (`block_fly`).
    pub block_fly: bool,
    /// Flying over the region's height cap is blocked (`block_fly_over`).
    pub block_fly_over: bool,
    /// Damage is enabled (`allow_damage`).
    pub allow_damage: bool,
    /// Land may be resold (`allow_land_resell`).
    pub allow_land_resell: bool,
    /// The agent limit (`agent_limit`).
    pub agent_limit: i32,
    /// The object bonus factor (`prim_bonus`).
    pub prim_bonus: f32,
    /// The maturity, as the `SimAccess` byte (`sim_access`).
    pub sim_access: u8,
    /// Pushing is restricted (`restrict_pushobject`).
    pub restrict_pushobject: bool,
    /// Parcels may be joined and divided (`allow_parcel_changes`).
    pub allow_parcel_changes: bool,
    /// The region's parcels are kept out of search (`block_parcel_search`).
    pub block_parcel_search: bool,
}

/// Builds the body of a `DispatchRegionInfo` POST, typed as the reference
/// sends it.
#[must_use]
pub fn build_dispatch_region_info_request(dispatch: &RegionInfoDispatch) -> String {
    let flag = |value: bool| Llsd::Boolean(value);
    Llsd::Map(HashMap::from([
        ("block_terraform".to_owned(), flag(dispatch.block_terraform)),
        ("block_fly".to_owned(), flag(dispatch.block_fly)),
        ("block_fly_over".to_owned(), flag(dispatch.block_fly_over)),
        ("allow_damage".to_owned(), flag(dispatch.allow_damage)),
        (
            "allow_land_resell".to_owned(),
            flag(dispatch.allow_land_resell),
        ),
        (
            "agent_limit".to_owned(),
            Llsd::Real(f64::from(dispatch.agent_limit)),
        ),
        (
            "prim_bonus".to_owned(),
            Llsd::Real(f64::from(dispatch.prim_bonus)),
        ),
        (
            "sim_access".to_owned(),
            Llsd::Integer(i32::from(dispatch.sim_access)),
        ),
        (
            "restrict_pushobject".to_owned(),
            flag(dispatch.restrict_pushobject),
        ),
        (
            "allow_parcel_changes".to_owned(),
            flag(dispatch.allow_parcel_changes),
        ),
        (
            "block_parcel_search".to_owned(),
            flag(dispatch.block_parcel_search),
        ),
    ]))
    .to_llsd_xml()
}

/// A field the body must carry, as an error when it does not.
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, WireError> {
    value.ok_or_else(|| {
        WireError::from(sl_llsd::LlsdError::MalformedField {
            field,
            value: "absent".to_owned(),
        })
    })
}

/// Reads the body of a `DispatchRegionInfo` POST — the server side of
/// [`build_dispatch_region_info_request`]. The spinners are read as reals or
/// integers and rounded, as a simulator storing whole agents would.
///
/// # Errors
/// Returns [`WireError`] if a field is absent, of the wrong LLSD kind, or out
/// of range.
pub fn parse_dispatch_region_info_request(body: &Llsd) -> Result<RegionInfoDispatch, WireError> {
    let flag = |key: &'static str| -> Result<bool, WireError> {
        required(body.field_bool(key, key)?, key)
    };
    let agent_limit = required(body.field_f64("agent_limit", "agent_limit")?, "agent_limit")?;
    let sim_access = required(body.field_i32("sim_access", "sim_access")?, "sim_access")?;
    let out_of_range = |field: &'static str, value: String| {
        WireError::from(sl_llsd::LlsdError::MalformedField { field, value })
    };
    Ok(RegionInfoDispatch {
        block_terraform: flag("block_terraform")?,
        block_fly: flag("block_fly")?,
        block_fly_over: flag("block_fly_over")?,
        allow_damage: flag("allow_damage")?,
        allow_land_resell: flag("allow_land_resell")?,
        agent_limit: whole(agent_limit)
            .ok_or_else(|| out_of_range("agent_limit", agent_limit.to_string()))?,
        prim_bonus: required(body.field_f32("prim_bonus", "prim_bonus")?, "prim_bonus")?,
        sim_access: u8::try_from(sim_access)
            .map_err(|_overflow| out_of_range("sim_access", sim_access.to_string()))?,
        restrict_pushobject: flag("restrict_pushobject")?,
        allow_parcel_changes: flag("allow_parcel_changes")?,
        block_parcel_search: flag("block_parcel_search")?,
    })
}

/// `value` rounded to a whole `i32`, or `None` when it is not finite or does
/// not fit.
fn whole(value: f64) -> Option<i32> {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return None;
    }
    // In range by the check above; the string round trip avoids a cast.
    format!("{rounded:.0}").parse().ok()
}

/// The days of the week a weekly restart falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RestartDays {
    /// One bit a day, Sunday the lowest.
    bits: u8,
}

impl RestartDays {
    /// The letters the capability names the days by, Sunday to Saturday.
    pub const LETTERS: [char; 7] = ['s', 'm', 't', 'w', 'r', 'f', 'a'];

    /// No day.
    pub const NONE: Self = Self { bits: 0 };

    /// The days `letters` name (either case); a letter that names no day is
    /// ignored, as the reference ignores it.
    #[must_use]
    pub fn from_letters(letters: &str) -> Self {
        let mut bits = 0_u8;
        for letter in letters.chars().map(|letter| letter.to_ascii_lowercase()) {
            for (day, name) in (0_u8..).zip(Self::LETTERS) {
                if letter == name {
                    bits |= 1 << day;
                }
            }
        }
        Self { bits }
    }

    /// The days as the capability names them: upper-case letters, Sunday
    /// first.
    #[must_use]
    pub fn letters(self) -> String {
        (0_u8..)
            .zip(Self::LETTERS)
            .filter(|(day, _letter)| self.bits & (1 << day) != 0)
            .map(|(_day, letter)| letter.to_ascii_uppercase())
            .collect()
    }

    /// Whether `day` (0 = Sunday … 6 = Saturday) is among the days.
    #[must_use]
    pub const fn contains(self, day: u8) -> bool {
        day < 7 && self.bits & (1 << day) != 0
    }

    /// A copy with `day` (0 = Sunday … 6 = Saturday) included when `on`; a day
    /// past Saturday changes nothing.
    #[must_use]
    pub const fn with(self, day: u8, on: bool) -> Self {
        if day >= 7 {
            return self;
        }
        if on {
            Self {
                bits: self.bits | (1 << day),
            }
        } else {
            Self {
                bits: self.bits & !(1 << day),
            }
        }
    }

    /// Whether no day is included.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Whether every day is included.
    #[must_use]
    pub const fn is_every_day(self) -> bool {
        self.bits == 0x7f
    }
}

/// When a region restarts by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RestartWhen {
    /// Every day (`type: "D"`).
    Daily,
    /// On the given days (`type: "W"`).
    Weekly(RestartDays),
}

/// A region's restart schedule, as `RegionSchedule` answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegionSchedule {
    /// On which days.
    pub when: RestartWhen,
    /// At what time of day, in seconds after midnight (the grid's time).
    pub seconds_after_midnight: u32,
}

/// The `restart` map of a schedule.
fn schedule_map(schedule: &RegionSchedule) -> Llsd {
    let mut map = HashMap::from([(
        "time".to_owned(),
        Llsd::Integer(i32::try_from(schedule.seconds_after_midnight).unwrap_or(i32::MAX)),
    )]);
    match schedule.when {
        RestartWhen::Daily => {
            let _previous = map.insert("type".to_owned(), Llsd::String("D".to_owned()));
        }
        RestartWhen::Weekly(days) => {
            let _previous = map.insert("type".to_owned(), Llsd::String("W".to_owned()));
            let _previous = map.insert("days".to_owned(), Llsd::String(days.letters()));
        }
    }
    Llsd::Map(map)
}

/// Builds a `RegionSchedule` body: a GET's answer (server side) or a POST
/// (client side). `None` is no schedule — an answer of `undef`, as Second
/// Life sends (`region-info`, 2026-10-17), and a POST that clears it (type
/// `W`, no days), as the reference sends.
#[must_use]
pub fn build_region_schedule(schedule: Option<&RegionSchedule>, for_post: bool) -> String {
    let body = match (schedule, for_post) {
        (Some(schedule), _) => HashMap::from([("restart".to_owned(), schedule_map(schedule))]),
        (None, true) => HashMap::from([(
            "restart".to_owned(),
            Llsd::Map(HashMap::from([
                ("type".to_owned(), Llsd::String("W".to_owned())),
                ("days".to_owned(), Llsd::String(String::new())),
                ("time".to_owned(), Llsd::Integer(0)),
            ])),
        )]),
        // Second Life answers a region with no schedule `undef`.
        (None, false) => return Llsd::Undef.to_llsd_xml(),
    };
    Llsd::Map(body).to_llsd_xml()
}

/// Reads a `RegionSchedule` body — a GET's answer, or a POST on the server
/// side. `None` is no schedule: no `restart`, or a weekly one with no days.
/// A type other than `D` reads as weekly, as the reference reads it.
///
/// # Errors
/// Returns [`WireError`] if a field is of the wrong LLSD kind or the time is
/// negative.
pub fn parse_region_schedule(body: &Llsd) -> Result<Option<RegionSchedule>, WireError> {
    let Some(restart) = body.field_map("restart", "restart")? else {
        return Ok(None);
    };
    let restart = Llsd::Map(restart.clone());
    let kind = restart.field_str("type", "type")?.unwrap_or_default();
    let when = if kind == "D" {
        RestartWhen::Daily
    } else {
        let days =
            RestartDays::from_letters(restart.field_str("days", "days")?.unwrap_or_default());
        if days.is_empty() {
            return Ok(None);
        }
        RestartWhen::Weekly(days)
    };
    let time = restart.field_i32("time", "time")?.unwrap_or_default();
    let seconds_after_midnight = u32::try_from(time).map_err(|_negative| {
        WireError::from(sl_llsd::LlsdError::MalformedField {
            field: "time",
            value: time.to_string(),
        })
    })?;
    Ok(Some(RegionSchedule {
        when,
        seconds_after_midnight,
    }))
}

/// How many terrain material slots a region has: the four corners of the
/// classic terrain composition.
pub const TERRAIN_MATERIAL_SLOTS: usize = 4;

/// A region's PBR terrain material overrides, as `ModifyRegion` carries them:
/// one GLTF material override document per terrain material slot, passed
/// through as LLSD (the reference's `getOverrideLLSD` map — texture
/// transforms under `ti`, factors under `bc`, `mf`, `rf` and the rest); an
/// empty map is no override.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TerrainMaterialOverrides(pub [Llsd; TERRAIN_MATERIAL_SLOTS]);

impl Default for TerrainMaterialOverrides {
    /// No override on any slot.
    fn default() -> Self {
        Self(core::array::from_fn(|_slot| Llsd::Map(HashMap::new())))
    }
}

impl TerrainMaterialOverrides {
    /// Reads the four overrides out of an `overrides` array, as the
    /// reference reads them: an array of at least four, the rest ignored.
    fn from_array(overrides: Option<&Llsd>) -> Result<Self, WireError> {
        let slots = overrides
            .and_then(Llsd::as_array)
            .filter(|slots| slots.len() >= TERRAIN_MATERIAL_SLOTS)
            .ok_or_else(|| {
                WireError::from(sl_llsd::LlsdError::MalformedField {
                    field: "overrides",
                    value: "not an array of four".to_owned(),
                })
            })?;
        let mut read = Self::default();
        for (slot, value) in read.0.iter_mut().zip(slots) {
            slot.clone_from(value);
        }
        Ok(read)
    }

    /// The overrides as an `overrides` array.
    fn to_array(&self) -> Llsd {
        Llsd::Array(self.0.to_vec())
    }
}

/// A `ModifyRegion` answer: to a GET, the overrides; to a POST, whether they
/// were taken.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModifyRegionAnswer {
    /// Whether the request succeeded (`success`).
    pub success: bool,
    /// Why it did not, where the grid says (`message`).
    pub message: Option<String>,
    /// The region's overrides: a successful GET's answer; `None` for a POST's
    /// or a failure.
    pub overrides: Option<TerrainMaterialOverrides>,
}

/// Builds the body of a `ModifyRegion` POST.
#[must_use]
pub fn build_modify_region_request(overrides: &TerrainMaterialOverrides) -> String {
    Llsd::Map(HashMap::from([(
        "overrides".to_owned(),
        overrides.to_array(),
    )]))
    .to_llsd_xml()
}

/// Reads the body of a `ModifyRegion` POST — the server side of
/// [`build_modify_region_request`].
///
/// # Errors
/// Returns [`WireError`] if `overrides` is not an array of at least four.
pub fn parse_modify_region_request(body: &Llsd) -> Result<TerrainMaterialOverrides, WireError> {
    TerrainMaterialOverrides::from_array(body.get("overrides"))
}

/// Builds a `ModifyRegion` answer (server side).
#[must_use]
pub fn build_modify_region_answer(answer: &ModifyRegionAnswer) -> String {
    let mut map = HashMap::from([("success".to_owned(), Llsd::Boolean(answer.success))]);
    if let Some(message) = &answer.message {
        let _previous = map.insert("message".to_owned(), Llsd::String(message.clone()));
    }
    if let Some(overrides) = &answer.overrides {
        let _previous = map.insert("overrides".to_owned(), overrides.to_array());
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Reads a `ModifyRegion` answer. `get` says whether it answers a GET, whose
/// success carries the overrides.
///
/// # Errors
/// Returns [`WireError`] if a field is of the wrong LLSD kind, or a
/// successful GET's answer carries no array of four overrides — what the
/// reference calls an invalid composition and treats as a failure.
pub fn parse_modify_region_answer(body: &Llsd, get: bool) -> Result<ModifyRegionAnswer, WireError> {
    let success = body.field_bool("success", "success")?.unwrap_or(false);
    let message = body.field_str("message", "message")?.map(ToOwned::to_owned);
    let overrides = if get && success {
        Some(TerrainMaterialOverrides::from_array(body.get("overrides"))?)
    } else {
        None
    };
    Ok(ModifyRegionAnswer {
        success,
        message,
        overrides,
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use std::collections::HashMap;

    use super::{
        ModifyRegionAnswer, RegionInfoDispatch, RegionSchedule, RestartDays, RestartWhen,
        TerrainMaterialOverrides, build_dispatch_region_info_request, build_modify_region_answer,
        build_modify_region_request, build_region_schedule, parse_dispatch_region_info_request,
        parse_modify_region_answer, parse_modify_region_request, parse_region_schedule,
    };
    use crate::llsd::{Llsd, parse_llsd_xml};

    /// Overrides with a texture transform on the second slot, the form the
    /// reference's terrain tab writes.
    fn transformed() -> TerrainMaterialOverrides {
        let mut overrides = TerrainMaterialOverrides::default();
        if let Some(slot) = overrides.0.get_mut(1) {
            *slot = Llsd::Map(HashMap::from([(
                "ti".to_owned(),
                Llsd::Array(vec![Llsd::Map(HashMap::from([(
                    "s".to_owned(),
                    Llsd::Array(vec![Llsd::Real(2.0), Llsd::Real(2.0)]),
                )]))]),
            )]));
        }
        overrides
    }

    /// A write survives the trip through its body.
    #[test]
    fn modify_region_request_round_trips() -> Result<(), String> {
        let overrides = transformed();
        let body = build_modify_region_request(&overrides);
        let parsed =
            parse_modify_region_request(&parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, overrides);
        Ok(())
    }

    /// A GET's answer carries the overrides; a POST's and a failure's do not.
    #[test]
    fn modify_region_answers_round_trip() -> Result<(), String> {
        for (answer, get) in [
            (
                ModifyRegionAnswer {
                    success: true,
                    message: None,
                    overrides: Some(transformed()),
                },
                true,
            ),
            (
                ModifyRegionAnswer {
                    success: true,
                    message: None,
                    overrides: None,
                },
                false,
            ),
            (
                ModifyRegionAnswer {
                    success: false,
                    message: Some("no".to_owned()),
                    overrides: None,
                },
                true,
            ),
        ] {
            let body = build_modify_region_answer(&answer);
            let parsed = parse_modify_region_answer(
                &parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?,
                get,
            )
            .map_err(|error| format!("{error:?}"))?;
            assert_eq!(parsed, answer);
        }
        Ok(())
    }

    /// A successful GET without four overrides is refused, as the reference
    /// refuses an invalid composition.
    #[test]
    fn a_short_composition_is_refused() -> Result<(), String> {
        let body = "<llsd><map><key>success</key><boolean>1</boolean>\
                    <key>overrides</key><array><map /></array></map></llsd>";
        let llsd = parse_llsd_xml(body).map_err(|error| format!("{error:?}"))?;
        let Err(_refused) = parse_modify_region_answer(&llsd, true) else {
            return Err("a short composition was read".to_owned());
        };
        Ok(())
    }

    /// A settings write survives the trip through its body, the two settings
    /// UDP cannot carry among them.
    #[test]
    fn dispatch_round_trips() -> Result<(), String> {
        let dispatch = RegionInfoDispatch {
            block_terraform: true,
            block_fly: false,
            block_fly_over: true,
            allow_damage: false,
            allow_land_resell: true,
            agent_limit: 40,
            prim_bonus: 1.5,
            sim_access: 21,
            restrict_pushobject: true,
            allow_parcel_changes: false,
            block_parcel_search: true,
        };
        let body = build_dispatch_region_info_request(&dispatch);
        assert!(body.contains("<key>agent_limit</key><real>"), "{body}");
        let parsed = parse_dispatch_region_info_request(
            &parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, dispatch);
        Ok(())
    }

    /// A body missing a setting is refused rather than read as `false`.
    #[test]
    fn dispatch_without_a_setting_is_refused() -> Result<(), String> {
        let body = "<llsd><map><key>block_fly</key><boolean>1</boolean></map></llsd>";
        let llsd = parse_llsd_xml(body).map_err(|error| format!("{error:?}"))?;
        let Err(_refused) = parse_dispatch_region_info_request(&llsd) else {
            return Err("a body missing settings was read".to_owned());
        };
        Ok(())
    }

    /// The reference's example answer reads as Tuesday and Thursday at 02:00.
    #[test]
    fn the_references_example_reads() -> Result<(), String> {
        let body = "<llsd><map><key>restart</key><map><key>days</key><string>TR</string>\
                    <key>time</key><integer>7200</integer><key>type</key><string>W</string>\
                    </map></map></llsd>";
        let parsed = parse_region_schedule(&parse_llsd_xml(body).map_err(|e| format!("{e:?}"))?)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            parsed,
            Some(RegionSchedule {
                when: RestartWhen::Weekly(RestartDays::from_letters("tr")),
                seconds_after_midnight: 7200,
            })
        );
        Ok(())
    }

    /// Every schedule form round-trips, and "none" is no `restart` in an
    /// answer but a cleared weekly schedule in a POST — both reading as none.
    #[test]
    fn schedules_round_trip() -> Result<(), String> {
        for schedule in [
            Some(RegionSchedule {
                when: RestartWhen::Daily,
                seconds_after_midnight: 3600,
            }),
            Some(RegionSchedule {
                when: RestartWhen::Weekly(RestartDays::from_letters("SA")),
                seconds_after_midnight: 0,
            }),
            None,
        ] {
            for for_post in [false, true] {
                let body = build_region_schedule(schedule.as_ref(), for_post);
                let parsed =
                    parse_region_schedule(&parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?)
                        .map_err(|error| format!("{error:?}"))?;
                assert_eq!(parsed, schedule, "{body}");
            }
        }
        assert!(build_region_schedule(None, false).contains("<undef"));
        assert!(build_region_schedule(None, true).contains("<key>days</key><string></string>"));
        Ok(())
    }

    /// Days are named Sunday first, upper case, whatever order they came in.
    #[test]
    fn days_are_named_in_order() {
        let days = RestartDays::from_letters("afs");
        assert_eq!(days.letters(), "SFA");
        assert!(days.contains(0));
        assert!(!days.contains(1));
        assert!(RestartDays::from_letters("smtwrfa").is_every_day());
        assert_eq!(RestartDays::NONE.with(1, true).letters(), "M");
        assert_eq!(RestartDays::NONE.with(9, true), RestartDays::NONE);
    }
}
