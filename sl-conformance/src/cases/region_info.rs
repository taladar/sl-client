//! A census of the region's own record: what a grid says of the region in the
//! `RegionHandshake` and in its `RegionInfo` answer, field by field and block
//! by block, to an estate manager and to a resident; what the two Second Life
//! capabilities of the Region tab answer (`DispatchRegionInfo`,
//! `RegionSchedule`); and how a change of the region's settings is told.
//!
//! Avatar **A** is an estate manager where the test avatars hold estate rights
//! (OpenSim: `--avatar estate-owner --secondary resident`, both in the Default
//! Region); avatar **B** is a resident. On Second Life neither holds rights
//! (`gridspec-aditi-test-land`), so both stand at the fixtures' build location
//! and only what a resident sees and is refused is measured.
//!
//! One question a leg:
//!
//! 1. **The handshake**, as each avatar read it on arrival: its flags (the
//!    legacy field and the extended block, or its absence), protocols,
//!    product and the rest.
//! 2. **The record**: A and B each send `RequestRegionInfo`; every field of
//!    the answer, whether each optional block came, and who else was told.
//! 3. **The capabilities**, read raw: the restart schedule by both; B's write
//!    of the region's settings and of the schedule, each the values it read.
//! 4. **B's refusals over UDP**: `setregioninfo` and `setregiondebug`, each the
//!    values B read.
//! 5. **A's changes** (A's rights only): every setting of the Region tab
//!    flipped, read back and restored; scripts switched off on the Debug tab
//!    and back on. What each avatar is told: a new `RegionInfo`, a new
//!    handshake, a change in the flags `SimStats` carries.
//!
//! Every id is described by whose it is and every name is scrubbed, so no
//! metric names an avatar. What each live grid answered is stated as a
//! [`Measured`] and written up in `book/src/gridspec/estate.md` § Region info;
//! every run is held to it.
//!
//! `2av`.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, RegionFlags, RegionIdentity, RegionInfoUpdate, RegionLimits, Vector,
};
use sl_wire::Llsd;

use crate::cases::estate::{Scene, describe_llsd, raw_estate_message};
use crate::cases::groups::names_of;
use crate::cases::land_transactions::{region_named, teleport};
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim};

/// How long both avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(8);

/// Where both avatars stand on OpenSim: the Default Region's centre.
const OPENSIM_SPOT: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 25.0,
};

/// What a grid without estate rights for the test avatars records for a leg
/// only an estate manager can run.
const NO_RIGHTS: &str = "not measured: no estate rights on aditi (gridspec-aditi-test-land)";

/// The metrics only an avatar with estate rights records: what a grid
/// without rights records instead.
const OWNER_KEYS: [&str; 23] = [
    "dispatch_a",
    "set_all_told_a",
    "set_all_told_b",
    "set_all_handshake_b",
    "set_all_read_told_a",
    "set_all_read_told_b",
    "set_all_changed",
    "set_all_flags_kept",
    "restore_told_a",
    "restore_told_b",
    "restore_read_told_a",
    "restore_read_told_b",
    "restore_matches",
    "debug_off_told_a",
    "debug_off_told_b",
    "debug_off_read_told_a",
    "debug_off_read_told_b",
    "debug_off_changed",
    "debug_on_told_a",
    "debug_on_told_b",
    "debug_on_read_told_a",
    "debug_on_read_told_b",
    "debug_on_matches",
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/estate.md § Region info (region-info, 2026-10-18)";

/// What a Second-Life-flavoured fake grid is not held to: who holds estate
/// rights — its first account is an estate manager, as on OpenSim, where on
/// aditi neither test avatar is (`gridspec-aditi-test-land`), so the census
/// treats it as the resident aditi's A is and skips the manager's legs — and
/// the rating Mauve's handshake states (`Mature`) against the one its record
/// and the world map state (`Adult`), which one region showed and nothing
/// explains; the fake region states one rating everywhere.
const NOT_IMITATED_AS_SECOND_LIFE: &[&str] = &[
    "a_holds_rights",
    "handshake_estate_manager",
    "handshake_b_differs",
    "handshake_owner",
    "info_access_vs_handshake",
    "map_access_vs_handshake",
];

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
///
/// Not held: the region's configuration — its flags, rating, product, limits,
/// prices, estate ids, water, terrain limits and the simulator's CPU — which
/// is the region's, not the grid's. What relates them is held instead (the
/// record's flags, rating and product against the handshake's, the legacy
/// agent cap against the wide one).
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "a_holds_rights",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "debug_off_changed",
        each(Answer::Text(NO_RIGHTS), Answer::Text("+SKIP_SCRIPTS")),
    ),
    (
        "debug_off_read_told_a",
        each(Answer::Text(NO_RIGHTS), Answer::Text("region info")),
    ),
    (
        "debug_off_read_told_b",
        each(Answer::Text(NO_RIGHTS), Answer::Text("nothing")),
    ),
    (
        "debug_off_told_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("sim stats flags +SKIP_SCRIPTS"),
        ),
    ),
    (
        "debug_off_told_b",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("sim stats flags +SKIP_SCRIPTS"),
        ),
    ),
    (
        "debug_on_matches",
        each(Answer::Text(NO_RIGHTS), Answer::Flag(true)),
    ),
    (
        "debug_on_read_told_a",
        each(Answer::Text(NO_RIGHTS), Answer::Text("region info")),
    ),
    (
        "debug_on_read_told_b",
        each(Answer::Text(NO_RIGHTS), Answer::Text("nothing")),
    ),
    (
        "debug_on_told_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("sim stats flags -SKIP_SCRIPTS"),
        ),
    ),
    (
        "debug_on_told_b",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("sim stats flags -SKIP_SCRIPTS"),
        ),
    ),
    (
        "dispatch_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("no capability: setregioninfo"),
        ),
    ),
    (
        "dispatch_b",
        each(
            Answer::Text("HTTP 403, not LLSD \"Forbidden\""),
            Answer::Text("no capability"),
        ),
    ),
    ("dispatch_b_told_a", both(Answer::Text("nothing"))),
    ("dispatch_b_told_b", both(Answer::Text("nothing"))),
    (
        "handshake_b_differs",
        each(
            Answer::Text("nothing"),
            Answer::Text("estate_manager: false (A: true)"),
        ),
    ),
    (
        "handshake_colo",
        each(Answer::Text("named"), Answer::Text("none")),
    ),
    (
        "handshake_estate_manager",
        each(Answer::Text("false"), Answer::Text("true")),
    ),
    ("handshake_extended_block", both(Answer::Text("sent"))),
    (
        "handshake_owner",
        each(Answer::Text("nobody"), Answer::Text("A")),
    ),
    (
        "handshake_protocols",
        each(Answer::Text("0x1"), Answer::Text("0x8000000000000000")),
    ),
    ("info_a_answered", both(Answer::Flag(true))),
    ("info_a_told_a", both(Answer::Text("region info"))),
    ("info_a_told_b", both(Answer::Text("nothing"))),
    (
        "info_access_vs_handshake",
        each(
            Answer::Text("info Adult, handshake Mature"),
            Answer::Text("same"),
        ),
    ),
    ("info_agents_vs_legacy", both(Answer::Text("same"))),
    ("info_b_answered", both(Answer::Flag(true))),
    ("info_b_differs", both(Answer::Text("nothing"))),
    ("info_b_told_a", both(Answer::Text("nothing"))),
    ("info_b_told_b", both(Answer::Text("region info"))),
    (
        "info_chat",
        each(
            Answer::Text("ranges 10/20/100, offsets 0/0/0, flags 0x0"),
            Answer::Text("no block"),
        ),
    ),
    (
        "info_combat",
        each(
            Answer::Text(
                "flags 0x5, on death 0, throttle 100, regeneration 1, invulnerable 0, limit 0",
            ),
            Answer::Text("no block"),
        ),
    ),
    ("info_estate_sun", both(Answer::Text("false"))),
    (
        "info_extended_block",
        each(Answer::Text("sent"), Answer::Text("no block")),
    ),
    ("info_name", both(Answer::Text("the region's"))),
    ("info_product_vs_handshake", both(Answer::Text("same"))),
    (
        "info_redirect",
        each(Answer::Text("-1,-1"), Answer::Text("0,0")),
    ),
    ("info_sun_hour", both(Answer::Text("0"))),
    (
        "info_vs_handshake_flags",
        each(
            Answer::Text("nothing"),
            Answer::Text("-ALLOW_ACCESS_OVERRIDE -EXTERNALLY_VISIBLE -ALLOW_DIRECT_TELEPORT"),
        ),
    ),
    (
        "map_access_vs_handshake",
        each(
            Answer::Text("map Adult, handshake Mature"),
            Answer::Text("same"),
        ),
    ),
    (
        "owner_legs_run",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("refuse_debug_told_a", both(Answer::Text("nothing"))),
    ("refuse_debug_told_b", both(Answer::Text("nothing"))),
    ("refuse_set_told_a", both(Answer::Text("nothing"))),
    ("refuse_set_told_b", both(Answer::Text("nothing"))),
    (
        "restore_matches",
        each(Answer::Text(NO_RIGHTS), Answer::Flag(true)),
    ),
    (
        "restore_read_told_a",
        each(Answer::Text(NO_RIGHTS), Answer::Text("region info")),
    ),
    (
        "restore_read_told_b",
        each(Answer::Text(NO_RIGHTS), Answer::Text("nothing")),
    ),
    (
        "restore_told_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text(
                "region info; sim stats flags +ALLOW_PARCEL_CHANGES -ALLOW_DAMAGE -BLOCK_TERRAFORM -BLOCK_LAND_RESELL -BLOCK_FLY -RESTRICT_PUSHOBJECT",
            ),
        ),
    ),
    (
        "restore_told_b",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text(
                "region info; sim stats flags +ALLOW_PARCEL_CHANGES -ALLOW_DAMAGE -BLOCK_TERRAFORM -BLOCK_LAND_RESELL -BLOCK_FLY -RESTRICT_PUSHOBJECT",
            ),
        ),
    ),
    (
        "schedule_get_a",
        each(
            Answer::Text("HTTP 200, undef"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "schedule_get_b",
        each(
            Answer::Text("HTTP 200, undef"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "schedule_set_b",
        each(
            Answer::Text(
                "HTTP 403, not LLSD \"Forbidden: You don't have permission to make this change.\"",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "set_all_changed",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text(
                "+ALLOW_DAMAGE +BLOCK_TERRAFORM +BLOCK_LAND_RESELL +BLOCK_FLY +RESTRICT_PUSHOBJECT -ALLOW_PARCEL_CHANGES; agents 40 -> 39; bonus 1 -> 1.5",
            ),
        ),
    ),
    (
        "set_all_flags_kept",
        each(Answer::Text(NO_RIGHTS), Answer::Flag(false)),
    ),
    (
        "set_all_handshake_b",
        each(Answer::Text(NO_RIGHTS), Answer::Text("none")),
    ),
    (
        "set_all_read_told_a",
        each(Answer::Text(NO_RIGHTS), Answer::Text("region info")),
    ),
    (
        "set_all_read_told_b",
        each(Answer::Text(NO_RIGHTS), Answer::Text("nothing")),
    ),
    (
        "set_all_told_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text(
                "region info; sim stats flags +ALLOW_DAMAGE +BLOCK_TERRAFORM +BLOCK_LAND_RESELL +BLOCK_FLY +RESTRICT_PUSHOBJECT -ALLOW_PARCEL_CHANGES",
            ),
        ),
    ),
    (
        "set_all_told_b",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text(
                "region info; sim stats flags +ALLOW_DAMAGE +BLOCK_TERRAFORM +BLOCK_LAND_RESELL +BLOCK_FLY +RESTRICT_PUSHOBJECT -ALLOW_PARCEL_CHANGES",
            ),
        ),
    ),
];

/// The region's record, its capabilities and how a change of it is told.
#[derive(Debug)]
pub struct RegionInfo;

impl GridTest for RegionInfo {
    fn name(&self) -> &'static str {
        "region-info"
    }

    fn description(&self) -> &'static str {
        "The region's handshake and RegionInfo field by field, the DispatchRegionInfo and \
         RegionSchedule capabilities, refusals, and how a change of the settings is told"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(900)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| {
                    !(grid == Grid::FakeSl && NOT_IMITATED_AS_SECOND_LIFE.contains(key))
                })
                .copied()
                .collect();
            outcome.and_then(|()| hold_to_measured(grid, &metrics, &held, SOURCE))
        })
    }
}

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The avatar with estate rights, where there is one.
    A,
    /// The resident.
    B,
}

/// The named bits of the region flag space, for describing a change.
const FLAG_NAMES: [(RegionFlags, &str); 30] = [
    (RegionFlags::ALLOW_DAMAGE, "ALLOW_DAMAGE"),
    (RegionFlags::ALLOW_LANDMARK, "ALLOW_LANDMARK"),
    (RegionFlags::ALLOW_SET_HOME, "ALLOW_SET_HOME"),
    (
        RegionFlags::RESET_HOME_ON_TELEPORT,
        "RESET_HOME_ON_TELEPORT",
    ),
    (RegionFlags::SUN_FIXED, "SUN_FIXED"),
    (RegionFlags::ALLOW_ACCESS_OVERRIDE, "ALLOW_ACCESS_OVERRIDE"),
    (RegionFlags::BLOCK_TERRAFORM, "BLOCK_TERRAFORM"),
    (RegionFlags::BLOCK_LAND_RESELL, "BLOCK_LAND_RESELL"),
    (RegionFlags::SANDBOX, "SANDBOX"),
    (
        RegionFlags::ALLOW_ENVIRONMENT_OVERRIDE,
        "ALLOW_ENVIRONMENT_OVERRIDE",
    ),
    (RegionFlags::SKIP_COLLISIONS, "SKIP_COLLISIONS"),
    (RegionFlags::SKIP_SCRIPTS, "SKIP_SCRIPTS"),
    (RegionFlags::SKIP_PHYSICS, "SKIP_PHYSICS"),
    (RegionFlags::EXTERNALLY_VISIBLE, "EXTERNALLY_VISIBLE"),
    (
        RegionFlags::ALLOW_RETURN_ENCROACHING_OBJECT,
        "ALLOW_RETURN_ENCROACHING_OBJECT",
    ),
    (
        RegionFlags::ALLOW_RETURN_ENCROACHING_ESTATE_OBJECT,
        "ALLOW_RETURN_ENCROACHING_ESTATE_OBJECT",
    ),
    (RegionFlags::BLOCK_DWELL, "BLOCK_DWELL"),
    (RegionFlags::BLOCK_FLY, "BLOCK_FLY"),
    (RegionFlags::ALLOW_DIRECT_TELEPORT, "ALLOW_DIRECT_TELEPORT"),
    (RegionFlags::ESTATE_SKIP_SCRIPTS, "ESTATE_SKIP_SCRIPTS"),
    (RegionFlags::RESTRICT_PUSHOBJECT, "RESTRICT_PUSHOBJECT"),
    (RegionFlags::DENY_ANONYMOUS, "DENY_ANONYMOUS"),
    (RegionFlags::ALLOW_PARCEL_CHANGES, "ALLOW_PARCEL_CHANGES"),
    (RegionFlags::BLOCK_FLYOVER, "BLOCK_FLYOVER"),
    (RegionFlags::ALLOW_VOICE, "ALLOW_VOICE"),
    (RegionFlags::BLOCK_PARCEL_SEARCH, "BLOCK_PARCEL_SEARCH"),
    (RegionFlags::DENY_AGEUNVERIFIED, "DENY_AGEUNVERIFIED"),
    (RegionFlags::DENY_BOTS, "DENY_BOTS"),
    (RegionFlags::from_bits(1 << 10), "bit 10"),
    (RegionFlags::from_bits(1 << 11), "bit 11"),
];

/// `flags` in words: the named bits set, then any other bit by number.
fn flag_words(flags: RegionFlags) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut rest = flags.bits();
    for (flag, name) in FLAG_NAMES {
        if flags.contains(flag) {
            words.push(name.to_owned());
            rest &= !flag.bits();
        }
    }
    for bit in 0..64_u32 {
        if rest & (1_u64 << bit) != 0 {
            words.push(format!("bit {bit}"));
        }
    }
    if words.is_empty() {
        "none".to_owned()
    } else {
        words.join(" ")
    }
}

/// What changed from `before` to `after`, in words: `+NAME` for a bit set,
/// `-NAME` for one cleared.
fn flag_change(before: RegionFlags, after: RegionFlags) -> String {
    let set = RegionFlags::from_bits(after.bits() & !before.bits());
    let cleared = RegionFlags::from_bits(before.bits() & !after.bits());
    let mut words: Vec<String> = Vec::new();
    if set.bits() != 0 {
        words.extend(flag_words(set).split(' ').map(|word| format!("+{word}")));
    }
    if cleared.bits() != 0 {
        words.extend(
            flag_words(cleared)
                .split(' ')
                .map(|word| format!("-{word}")),
        );
    }
    if words.is_empty() {
        "nothing".to_owned()
    } else {
        words.join(" ")
    }
}

/// An `f32` in words, without a trailing `.0` hiding a fraction.
fn real(value: f32) -> String {
    format!("{value}")
}

/// A handshake's fields in words, by the key each is recorded under.
fn handshake_fields(scene: &Scene, identity: &RegionIdentity) -> Vec<(&'static str, String)> {
    vec![
        ("flags", flag_words(identity.region_flags)),
        (
            "extended_flags",
            identity
                .region_flags_extended
                .map_or_else(|| "no block".to_owned(), flag_words),
        ),
        ("protocols", format!("{:#x}", identity.region_protocols)),
        ("access", format!("{:?}", identity.maturity)),
        ("product_sku", format!("\"{}\"", identity.product_sku)),
        ("product_name", format!("\"{}\"", identity.product_name)),
        (
            "colo",
            if identity.colo_name.is_empty() {
                "none".to_owned()
            } else {
                "named".to_owned()
            },
        ),
        ("cpu_class", identity.cpu_class_id.to_string()),
        ("cpu_ratio", identity.cpu_ratio.to_string()),
        ("owner", scene.whose(identity.sim_owner).to_owned()),
        ("estate_manager", identity.is_estate_manager.to_string()),
        ("water", real(identity.water_height)),
        ("billable", real(identity.billable_factor)),
    ]
}

/// A `RegionInfo`'s fields in words, by the key each is recorded under; the
/// region's name compared with the handshake's.
fn info_fields(limits: &RegionLimits, region_name: &str) -> Vec<(&'static str, String)> {
    let name = limits
        .sim_name
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    vec![
        (
            "name",
            if name == region_name {
                "the region's".to_owned()
            } else {
                format!("\"{name}\"")
            },
        ),
        ("estate", limits.estate_id.to_string()),
        ("parent_estate", limits.parent_estate_id.to_string()),
        ("flags", flag_words(limits.region_flags)),
        (
            "extended_flags",
            limits
                .region_flags_extended
                .map_or_else(|| "no block".to_owned(), flag_words),
        ),
        ("access", format!("{:?}", limits.maturity)),
        ("agents", limits.legacy_max_agents.to_string()),
        ("agents32", limits.max_agents.to_string()),
        ("hard_agents", limits.hard_max_agents.to_string()),
        ("hard_objects", limits.hard_max_objects.to_string()),
        ("billable", real(limits.billable_factor)),
        ("bonus", real(limits.object_bonus_factor)),
        ("water", real(limits.water_height)),
        ("raise", real(limits.terrain_raise_limit)),
        ("lower", real(limits.terrain_lower_limit)),
        ("price", limits.price_per_meter.0.to_string()),
        (
            "redirect",
            format!("{},{}", limits.redirect_grid_x, limits.redirect_grid_y),
        ),
        ("estate_sun", limits.use_estate_sun.to_string()),
        ("sun_hour", real(limits.sun_hour)),
        ("product_sku", format!("\"{}\"", limits.product_sku)),
        ("product_name", format!("\"{}\"", limits.product_name)),
        (
            "chat",
            limits.chat_settings.as_ref().map_or_else(
                || "no block".to_owned(),
                |chat| {
                    format!(
                        "ranges {}/{}/{}, offsets {}/{}/{}, flags {:#x}",
                        real(chat.whisper_range),
                        real(chat.normal_range),
                        real(chat.shout_range),
                        real(chat.whisper_offset),
                        real(chat.normal_offset),
                        real(chat.shout_offset),
                        chat.flags
                    )
                },
            ),
        ),
        (
            "combat",
            limits.combat_settings.as_ref().map_or_else(
                || "no block".to_owned(),
                |combat| {
                    format!(
                        "flags {:#x}, on death {}, throttle {}, regeneration {}, \
                         invulnerable {}, limit {}",
                        combat.flags,
                        combat.on_death,
                        real(combat.damage_throttle),
                        real(combat.regeneration_rate),
                        real(combat.invulnerability_time),
                        real(combat.damage_limit)
                    )
                },
            ),
        ),
    ]
}

/// Where `theirs` differs from `ours`, in words; `nothing` when it does not.
fn differences(ours: &[(&'static str, String)], theirs: &[(&'static str, String)]) -> String {
    let words: Vec<String> = ours
        .iter()
        .zip(theirs)
        .filter(|((_key, mine), (_other_key, other))| mine != other)
        .map(|((key, mine), (_other_key, other))| format!("{key}: {other} (A: {mine})"))
        .collect();
    if words.is_empty() {
        "nothing".to_owned()
    } else {
        words.join("; ")
    }
}

/// How the record's rating relates to the handshake's.
fn access_relation(
    info: sl_client_tokio::Maturity,
    handshake: sl_client_tokio::Maturity,
) -> String {
    if info == handshake {
        "same".to_owned()
    } else {
        format!("info {info:?}, handshake {handshake:?}")
    }
}

/// The settings the Region tab would show for `limits`: what B echoes back.
fn update_from(limits: &RegionLimits) -> RegionInfoUpdate {
    let flags = limits.flags();
    RegionInfoUpdate {
        block_terraform: flags.contains(RegionFlags::BLOCK_TERRAFORM),
        block_fly: flags.contains(RegionFlags::BLOCK_FLY),
        allow_damage: flags.contains(RegionFlags::ALLOW_DAMAGE),
        allow_land_resell: !flags.contains(RegionFlags::BLOCK_LAND_RESELL),
        agent_limit: i32::try_from(limits.max_agents).unwrap_or(i32::MAX),
        object_bonus: limits.object_bonus_factor,
        maturity: limits.maturity,
        restrict_pushobject: flags.contains(RegionFlags::RESTRICT_PUSHOBJECT),
        allow_parcel_changes: flags.contains(RegionFlags::ALLOW_PARCEL_CHANGES),
        block_fly_over: flags.contains(RegionFlags::BLOCK_FLYOVER),
        block_parcel_search: flags.contains(RegionFlags::BLOCK_PARCEL_SEARCH),
    }
}

/// The nine `setregioninfo` parameters for `update`, as the reference writes
/// them.
fn setregioninfo_params(update: &RegionInfoUpdate) -> Vec<String> {
    let yn = |flag: bool| if flag { "Y" } else { "N" }.to_owned();
    vec![
        yn(update.block_terraform),
        yn(update.block_fly),
        yn(update.allow_damage),
        yn(update.allow_land_resell),
        format!("{:.6}", f64::from(update.agent_limit)),
        format!("{:.6}", update.object_bonus),
        update.maturity.to_sim_access().to_string(),
        yn(update.restrict_pushobject),
        yn(update.allow_parcel_changes),
    ]
}

/// The three `setregiondebug` parameters: scripts, collisions and physics
/// off, each from `flags` unless `scripts_off` overrides the first.
fn setregiondebug_params(flags: RegionFlags, scripts_off: Option<bool>) -> Vec<String> {
    let yn = |flag: bool| if flag { "Y" } else { "N" }.to_owned();
    vec![
        yn(scripts_off.unwrap_or_else(|| flags.contains(RegionFlags::SKIP_SCRIPTS))),
        yn(flags.contains(RegionFlags::SKIP_COLLISIONS)),
        yn(flags.contains(RegionFlags::SKIP_PHYSICS)),
    ]
}

/// What one session was told during a window, in words, in arrival order.
#[derive(Debug, Default)]
struct Told {
    /// One line per message the census reads.
    lines: Vec<String>,
    /// The last `RegionInfo` among them.
    info: Option<RegionLimits>,
    /// The last handshake among them.
    handshake: Option<RegionIdentity>,
}

/// Folds one event into `told`; `None` for an event the census does not
/// read. `stats_flags` is the flags the last `SimStats` carried: a `SimStats`
/// is only a line when they changed.
fn fold(
    scene: &Scene,
    told: &mut Told,
    stats_flags: &mut Option<RegionFlags>,
    event: &Event,
) -> Option<()> {
    let line = match event {
        // The record's flags are the region's configuration, recorded on
        // their own (`info_flags`); a line says only that it came.
        Event::RegionLimits(limits) => {
            told.info = Some(limits.clone());
            "region info".to_owned()
        }
        Event::RegionInfoHandshake(identity) => {
            if Some(identity.region_handle) != scene.region {
                return None;
            }
            told.handshake = Some(identity.as_ref().clone());
            "handshake".to_owned()
        }
        Event::SimStats(stats) => {
            let flags = stats.flags();
            let previous = stats_flags.replace(flags);
            match previous {
                Some(previous) if previous != flags => {
                    format!("sim stats flags {}", flag_change(previous, flags))
                }
                _ => return None,
            }
        }
        Event::AlertMessage { message, .. } => format!("alert \"{}\"", scene.scrub(message)),
        Event::AgentAlertMessage { message, .. } => {
            format!("agent alert \"{}\"", scene.scrub(message))
        }
        Event::RegionSchedule(schedule) => format!("schedule {schedule:?}"),
        _ => return None,
    };
    told.lines.push(line);
    Some(())
}

/// What `told` holds, in words; `nothing` for an empty window.
fn describe_told(told: &Told) -> String {
    if told.lines.is_empty() {
        "nothing".to_owned()
    } else {
        told.lines.join("; ")
    }
}

/// Listens on `session` for `window`.
async fn watch(
    scene: &Scene,
    session: &mut Session,
    stats_flags: &mut Option<RegionFlags>,
    window: Duration,
) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(told);
        }
        match session
            .wait_for(remaining, |event| {
                fold(scene, &mut told, stats_flags, event)
            })
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(told),
            Err(other) => return Err(other),
        }
    }
}

/// A capability's raw answer to `session`, in words: the status and the body
/// described; with the body parsed where it was LLSD. `post` is the body of a
/// POST; `None` sends a GET.
async fn raw_cap(
    scene: &Scene,
    session: &Session,
    name: &str,
    post: Option<String>,
) -> (String, Option<Llsd>) {
    let Some(url) = session.cap(name) else {
        return ("no capability".to_owned(), None);
    };
    let client = reqwest::Client::new();
    let request = match post {
        Some(body) => client
            .post(url)
            .header("Content-Type", "application/llsd+xml")
            .body(body),
        None => client.get(url),
    };
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return (
                format!("unreachable ({})", scene.scrub(&error.to_string())),
                None,
            );
        }
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if body.trim().is_empty() {
        return (format!("HTTP {status}, empty"), None);
    }
    if !body.contains("<llsd") {
        return (
            format!(
                "HTTP {status}, not LLSD \"{}\"",
                scene.scrub(&body.chars().take(80).collect::<String>())
            ),
            None,
        );
    }
    let Ok(llsd) = sl_wire::parse_llsd_xml(&body) else {
        return (format!("HTTP {status}, unparsable LLSD"), None);
    };
    (
        format!("HTTP {status}, {}", describe_llsd(scene, &llsd)),
        Some(llsd),
    )
}

/// The census's state: the two sessions and who is who.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who is who.
    scene: Scene,
    /// The region's name, as the handshake gave it.
    region_name: String,
    /// The flags A's last `SimStats` carried.
    stats_a: Option<RegionFlags>,
    /// The flags B's last `SimStats` carried.
    stats_b: Option<RegionFlags>,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// Listens to both avatars for a window.
    async fn hear(&mut self) -> Result<(Told, Told), TestFailure> {
        let scene = self.scene.clone();
        let (told_a, told_b) = tokio::join!(
            watch(&scene, self.a, &mut self.stats_a, WINDOW),
            watch(&scene, self.b, &mut self.stats_b, WINDOW)
        );
        Ok((told_a?, told_b?))
    }

    /// Sends `commands` as `who`, listens to both avatars and records what
    /// each was told under `key`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(Told, Told), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let (told_a, told_b) = self.hear().await?;
        metrics.set(&format!("{key}_told_a"), describe_told(&told_a));
        metrics.set(&format!("{key}_told_b"), describe_told(&told_b));
        Ok((told_a, told_b))
    }

    /// Reads the region's record as `who`, recording what both were told
    /// under `key`; the record, if one came.
    async fn read_info(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<Option<RegionLimits>, TestFailure> {
        let (told_a, told_b) = self
            .leg(metrics, key, who, vec![Command::RequestRegionInfo])
            .await?;
        Ok(match who {
            Who::A => told_a.info,
            Who::B => told_b.info,
        })
    }

    /// An `EstateOwnerMessage` `method` with `params`, sent raw as `who`.
    async fn send_estate(
        &mut self,
        who: Who,
        method: &str,
        params: &[String],
    ) -> Result<Command, TestFailure> {
        let params: Vec<&str> = params.iter().map(String::as_str).collect();
        let message = raw_estate_message(self.session(who), method, &params)?;
        Ok(Command::Send {
            message: Box::new(message),
            reliability: sl_client_tokio::Reliability::Reliable,
        })
    }
}

/// Records every field of `fields` under `prefix`.
fn record_fields(metrics: &mut Metrics, prefix: &str, fields: &[(&'static str, String)]) {
    for (key, value) in fields {
        metrics.set(&format!("{prefix}_{key}"), value.clone());
    }
}

/// The census: both avatars at the spot, then the legs.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "region-info needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for name in names_of(&a.avatar_legacy_name()) {
        names.push((name, "<A>"));
    }
    for name in names_of(&b.avatar_legacy_name()) {
        names.push((name, "<B>"));
    }
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    if !is_opensim(grid) && !grid.is_fake() {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("region-info needs a build_location in the fixtures".to_owned())
        })?;
        let handle = region_named(a, &region).await?;
        teleport(a, handle, &point).await?;
        teleport(b, handle, &point).await?;
    } else if is_opensim(grid) && !grid.is_fake() {
        let handle = a
            .region_handle()
            .ok_or_else(|| TestFailure::State("no region".to_owned()))?;
        teleport(b, handle, &OPENSIM_SPOT).await?;
    }
    let region = a.region_handle();
    let mut census = Census {
        a,
        b,
        scene: Scene {
            a: a_id,
            b: b_id,
            region,
            names,
        },
        region_name: String::new(),
        stats_a: None,
        stats_b: None,
    };
    let _settled = census.hear().await?;
    legs(&mut census, metrics, grid).await
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    grid: Grid,
) -> Result<(), TestFailure> {
    // 1. The handshake each avatar read on arrival.
    let handshake_a = census
        .a
        .region_identity()
        .cloned()
        .ok_or_else(|| TestFailure::State("A read no handshake".to_owned()))?;
    let handshake_b = census
        .b
        .region_identity()
        .cloned()
        .ok_or_else(|| TestFailure::State("B read no handshake".to_owned()))?;
    census.region_name = handshake_a
        .sim_name
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    metrics.set("a_holds_rights", handshake_a.is_estate_manager);
    // A fake grid imitating Second Life answers as aditi does to a resident:
    // its manager's legs are as unmeasured as aditi's.
    let rights = handshake_a.is_estate_manager && grid != Grid::FakeSl;
    metrics.set("owner_legs_run", rights);
    let fields_a = handshake_fields(&census.scene, &handshake_a);
    let fields_b = handshake_fields(&census.scene, &handshake_b);
    record_fields(metrics, "handshake", &fields_a);
    metrics.set("handshake_b_differs", differences(&fields_a, &fields_b));

    // The world map's rating of the region, beside the handshake's.
    census
        .a
        .send(Command::RequestMapByName {
            name: census.region_name.clone(),
        })
        .await?;
    let region_name = census.region_name.clone();
    let map_access = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::MapBlock(block)
                if block
                    .name
                    .as_ref()
                    .is_some_and(|name| name.to_string().eq_ignore_ascii_case(&region_name)) =>
            {
                Some(format!("{:?}", block.maturity))
            }
            _ => None,
        })
        .await
        .or_else(|failure| match failure {
            TestFailure::Timeout(_) => Ok("no map block".to_owned()),
            other => Err(other),
        })?;
    metrics.set("map_access_vs_handshake", {
        let handshake = format!("{:?}", handshake_a.maturity);
        if map_access == handshake {
            "same".to_owned()
        } else {
            format!("map {map_access}, handshake {handshake}")
        }
    });
    metrics.set("map_access", map_access);
    let _settled = census.hear().await?;

    // 2. The record, as each reads it.
    let info_a = census.read_info(metrics, "info_a", Who::A).await?;
    let info_b = census.read_info(metrics, "info_b", Who::B).await?;
    let region_name = census.region_name.clone();
    let info_fields_a = info_a
        .as_ref()
        .map(|limits| info_fields(limits, &region_name));
    let info_fields_b = info_b
        .as_ref()
        .map(|limits| info_fields(limits, &region_name));
    metrics.set("info_a_answered", info_a.is_some());
    metrics.set("info_b_answered", info_b.is_some());
    if let Some(fields) = &info_fields_a {
        record_fields(metrics, "info", fields);
    }
    metrics.set(
        "info_b_differs",
        match (&info_fields_a, &info_fields_b) {
            (Some(ours), Some(theirs)) => differences(ours, theirs),
            (None, Some(_)) => "A was not answered".to_owned(),
            (_, None) => "not answered".to_owned(),
        },
    );
    metrics.set(
        "handshake_extended_block",
        if handshake_a.region_flags_extended.is_some() {
            "sent"
        } else {
            "no block"
        },
    );
    metrics.set(
        "info_extended_block",
        info_a.as_ref().map_or("no record", |limits| {
            if limits.region_flags_extended.is_some() {
                "sent"
            } else {
                "no block"
            }
        }),
    );
    metrics.set(
        "info_product_vs_handshake",
        info_a.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| {
                if limits.product_sku == handshake_a.product_sku
                    && limits.product_name == handshake_a.product_name
                {
                    "same".to_owned()
                } else {
                    format!(
                        "info \"{}\" \"{}\", handshake \"{}\" \"{}\"",
                        limits.product_sku,
                        limits.product_name,
                        handshake_a.product_sku,
                        handshake_a.product_name
                    )
                }
            },
        ),
    );
    metrics.set(
        "info_access_vs_handshake",
        info_a.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| access_relation(limits.maturity, handshake_a.maturity),
        ),
    );
    metrics.set(
        "info_agents_vs_legacy",
        info_a.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| {
                if u32::from(limits.legacy_max_agents) == limits.max_agents {
                    "same".to_owned()
                } else {
                    format!("{} and {}", limits.legacy_max_agents, limits.max_agents)
                }
            },
        ),
    );
    metrics.set(
        "info_vs_handshake_flags",
        info_a.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| flag_change(handshake_a.flags(), limits.flags()),
        ),
    );

    // 3. The capabilities, raw.
    let scene = census.scene.clone();
    let (schedule_a, _llsd) = raw_cap(&scene, census.a, "RegionSchedule", None).await;
    metrics.set("schedule_get_a", schedule_a);
    let (schedule_b, schedule_llsd) = raw_cap(&scene, census.b, "RegionSchedule", None).await;
    metrics.set("schedule_get_b", schedule_b);
    let echo = info_b
        .as_ref()
        .map(update_from)
        .ok_or_else(|| TestFailure::State("B read no region record".to_owned()))?;
    let (dispatch_b, _llsd) = raw_cap(
        &scene,
        census.b,
        "DispatchRegionInfo",
        Some(sl_wire::build_dispatch_region_info_request(
            &echo.to_dispatch(),
        )),
    )
    .await;
    metrics.set("dispatch_b", dispatch_b);
    let _told = census
        .leg(metrics, "dispatch_b", Who::B, Vec::new())
        .await?;
    // Write back what B read: should a grid ever take it, nothing changes.
    let schedule_echo = schedule_llsd
        .as_ref()
        .and_then(|llsd| sl_wire::parse_region_schedule(llsd).ok())
        .flatten();
    let (schedule_post_b, _llsd) = raw_cap(
        &scene,
        census.b,
        "RegionSchedule",
        Some(sl_wire::build_region_schedule(schedule_echo.as_ref(), true)),
    )
    .await;
    metrics.set("schedule_set_b", schedule_post_b);

    // 4. B's refusals over UDP, each what B read.
    let set = census
        .send_estate(Who::B, "setregioninfo", &setregioninfo_params(&echo))
        .await?;
    let _told = census.leg(metrics, "refuse_set", Who::B, vec![set]).await?;
    let b_flags = info_b
        .as_ref()
        .map_or_else(RegionFlags::default, RegionLimits::flags);
    let debug = census
        .send_estate(
            Who::B,
            "setregiondebug",
            &setregiondebug_params(b_flags, None),
        )
        .await?;
    let _told = census
        .leg(metrics, "refuse_debug", Who::B, vec![debug])
        .await?;

    // 5. A's changes.
    if rights {
        if let Some(before) = &info_a {
            owner_legs(census, metrics, before, &handshake_b).await?;
        }
    } else {
        for key in OWNER_KEYS {
            metrics.set(key, NO_RIGHTS);
        }
    }
    Ok(())
}

/// The legs only an estate manager runs: every setting flipped, read back and
/// restored; scripts off and on again.
async fn owner_legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    before: &RegionLimits,
    handshake_b: &RegionIdentity,
) -> Result<(), TestFailure> {
    let original = update_from(before);
    let flipped = RegionInfoUpdate {
        block_terraform: !original.block_terraform,
        block_fly: !original.block_fly,
        allow_damage: !original.allow_damage,
        allow_land_resell: !original.allow_land_resell,
        agent_limit: original.agent_limit.saturating_sub(1),
        object_bonus: original.object_bonus + 0.5,
        maturity: original.maturity,
        restrict_pushobject: !original.restrict_pushobject,
        allow_parcel_changes: !original.allow_parcel_changes,
        block_fly_over: !original.block_fly_over,
        block_parcel_search: !original.block_parcel_search,
    };
    // A's writes go the way the client sends them: over the capability where
    // the region grants it, over UDP otherwise.
    metrics.set(
        "dispatch_a",
        if census.a.cap("DispatchRegionInfo").is_some() {
            "capability"
        } else {
            "no capability: setregioninfo"
        },
    );
    let (_told_a, told_b) = census
        .leg(
            metrics,
            "set_all",
            Who::A,
            vec![Command::SetRegionInfo(flipped.clone())],
        )
        .await?;
    metrics.set(
        "set_all_handshake_b",
        told_b.handshake.as_ref().map_or_else(
            || "none".to_owned(),
            |identity| flag_change(handshake_b.flags(), identity.flags()),
        ),
    );
    let after = census.read_info(metrics, "set_all_read", Who::A).await?;
    let kept = after.as_ref().map(update_from);
    metrics.set(
        "set_all_changed",
        after.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| {
                format!(
                    "{}; agents {} -> {}; bonus {} -> {}",
                    flag_change(before.flags(), limits.flags()),
                    before.max_agents,
                    limits.max_agents,
                    real(before.object_bonus_factor),
                    real(limits.object_bonus_factor)
                )
            },
        ),
    );
    metrics.set(
        "set_all_flags_kept",
        kept.as_ref().is_some_and(|kept| *kept == flipped),
    );
    let _told = census
        .leg(
            metrics,
            "restore",
            Who::A,
            vec![Command::SetRegionInfo(original.clone())],
        )
        .await?;
    let restored = census.read_info(metrics, "restore_read", Who::A).await?;
    metrics.set(
        "restore_matches",
        restored
            .as_ref()
            .is_some_and(|limits| update_from(limits) == original),
    );

    // Scripts off on the Debug tab, then back.
    let flags = before.flags();
    let off = census
        .send_estate(
            Who::A,
            "setregiondebug",
            &setregiondebug_params(flags, Some(true)),
        )
        .await?;
    let _told = census.leg(metrics, "debug_off", Who::A, vec![off]).await?;
    let debugged = census.read_info(metrics, "debug_off_read", Who::A).await?;
    metrics.set(
        "debug_off_changed",
        debugged.as_ref().map_or_else(
            || "no record".to_owned(),
            |limits| flag_change(flags, limits.flags()),
        ),
    );
    let on = census
        .send_estate(
            Who::A,
            "setregiondebug",
            &setregiondebug_params(flags, None),
        )
        .await?;
    let _told = census.leg(metrics, "debug_on", Who::A, vec![on]).await?;
    let undone = census.read_info(metrics, "debug_on_read", Who::A).await?;
    metrics.set(
        "debug_on_matches",
        undone
            .as_ref()
            .is_some_and(|limits| limits.flags() == flags),
    );
    Ok(())
}
