//! Library tranche: what a prim does with what it carries
//! (`server-lsl-lib-task-inventory`) — rezzing an object out of it, giving
//! items to an avatar or another prim, and loading a script into another
//! prim.
//!
//! Each call is checked and worded here, as its flavour measured
//! (`inventory/rez`, `inventory/give`, `inventory/newer`; `book/src/gridspec/
//! lsl.md` § Rezzing and § Giving), and handed to the host finished: a
//! [`Rez`], an [`Offer`], a [`RemoteLoad`].

use sl_types::lsl::{Rotation, Vector};

use crate::library::inventory::named;
use crate::library::notecards::cut_bytes;
use crate::library::prim_params::set::normalised;
use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::contents::{INVENTORY_OBJECT, INVENTORY_SCRIPT, Offer, RemoteLoad, Rez};

/// How far from the rezzing prim a script may put an object, in metres: one
/// 11 m away rezzes nothing, silently, on both grids.
const REZ_REACH_M: f32 = 10.0;

/// The most of a start string a rezzed object's scripts are given, in bytes:
/// a string of 2048 characters arrived as 1018 on aditi (`inventory/newer`).
const START_STRING_BYTES: usize = 1018;

/// How long giving to an avatar holds the script, which the keyword file
/// states as nothing: 2 s on Second Life, 3 s on OpenSim (`inventory/give`,
/// `throttle/delays`).
const fn offer_delay(opensim: bool) -> f32 {
    if opensim { 3.0 } else { 2.0 }
}

/// `REZ_FLAG_TEMP`.
const REZ_FLAG_TEMP: i32 = 0x1;

/// `DEREZ_DIE`.
const DEREZ_DIE: i32 = 0;

/// The object item and the place an `llRez*` call names, checked: [`None`]
/// when nothing is to be rezzed, with what the flavour says about it said.
fn checked_rez(ctx: &mut ScriptCtx<'_>, item: &str, position: &Vector) -> Option<()> {
    let caller = ctx.caller();
    let here = ctx.host().placement(caller).position;
    let distance = ((position.x - here.x).powi(2)
        + (position.y - here.y).powi(2)
        + (position.z - here.z).powi(2))
    .sqrt();
    if distance
        .partial_cmp(&REZ_REACH_M)
        .is_none_or(core::cmp::Ordering::is_gt)
    {
        return None;
    }
    let opensim = ctx.flavour().is_opensim();
    let Some(found) = named(ctx, item) else {
        if opensim {
            ctx.refuse(format!("llRez(AtRoot/Object): Can't find object '{item}'"));
        } else {
            ctx.refuse(format!("Could not find object '{item}'."));
        }
        ctx.sleep(1.0);
        return None;
    };
    if found.kind != INVENTORY_OBJECT {
        not_an_object(ctx);
        return None;
    }
    Some(())
}

/// What the flavour says of a rez of an item that is no object, or whose
/// object is not there: nothing on Second Life; OpenSim's database complaint
/// and its second.
fn not_an_object(ctx: &mut ScriptCtx<'_>) {
    if ctx.flavour().is_opensim() {
        ctx.refuse(
            "llRez(AtRoot/Object): Can't create requested object; object is missing from database"
                .to_owned(),
        );
        ctx.sleep(1.0);
    }
}

/// Hands `rez` to the host; the root's key, or `NULL_KEY` when nothing was
/// rezzed.
fn rez_with(ctx: &mut ScriptCtx<'_>, rez: &Rez) -> String {
    let caller = ctx.caller();
    match ctx.host().rez(caller, rez) {
        Ok(key) => key,
        Err(_refused) => {
            not_an_object(ctx);
            NULL_KEY.to_owned()
        }
    }
}

/// A rez of `item` at `position`, as both `llRezObject` and `llRezAtRoot`
/// make it: the rotation unit length, the object dying at the region's edge
/// on OpenSim and not on Second Life.
fn rez_of(
    ctx: &ScriptCtx<'_>,
    item: String,
    (position, at_root): (Vector, bool),
    velocity: Vector,
    rotation: &Rotation,
    start_parameter: i32,
) -> Rez {
    Rez {
        item,
        position,
        at_root,
        velocity,
        rotation: normalised(rotation),
        start_parameter,
        start_string: String::new(),
        temporary: false,
        die_at_edge: ctx.flavour().is_opensim(),
    }
}

/// `llRezObject`: rez the object `item` of the prim with the centre of its
/// bounding box at `position` (within 10 m), moving at `velocity` if it is
/// physical, turned `rotation` (made unit length), its scripts given
/// `start_parameter`. The rezzer's `object_rez` follows, after the child's
/// `on_rez`. A missing name is refused, with a second's sleep; a notecard's
/// name rezzes nothing, silently on Second Life.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rez_object(
    ctx: &mut ScriptCtx<'_>,
    item: String,
    position: Vector,
    velocity: Vector,
    rotation: Rotation,
    start_parameter: i32,
) -> Result<(), CallError> {
    if checked_rez(ctx, &item, &position).is_some() {
        let rez = rez_of(
            ctx,
            item,
            (position, false),
            velocity,
            &rotation,
            start_parameter,
        );
        let _key = rez_with(ctx, &rez);
    }
    Ok(())
}

/// `llRezAtRoot`: [`ll_rez_object`] with the root at `position`; for an
/// object of one prim the same.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rez_at_root(
    ctx: &mut ScriptCtx<'_>,
    item: String,
    position: Vector,
    velocity: Vector,
    rotation: Rotation,
    start_parameter: i32,
) -> Result<(), CallError> {
    if checked_rez(ctx, &item, &position).is_some() {
        let rez = rez_of(
            ctx,
            item,
            (position, true),
            velocity,
            &rotation,
            start_parameter,
        );
        let _key = rez_with(ctx, &rez);
    }
    Ok(())
}

/// A rule of `llRezObjectWithParams`' list that is wrong: its number (from
/// 1) and what is wrong with it.
struct BadRule {
    /// The rule's number, counting rules from 1.
    rule: usize,
    /// What is wrong.
    detail: String,
}

/// Reads the `REZ_*` rule list of `llRezObjectWithParams` into `rez`,
/// relative to `here`, the rezzing prim's position and rotation.
fn read_rules(
    rules: &[Element],
    rez: &mut Rez,
    (here, turned): (&Vector, &Rotation),
) -> Result<(), BadRule> {
    let mut at = 0_usize;
    let mut rule = 0_usize;
    while let Some(code) = rules.get(at) {
        rule = rule.saturating_add(1);
        let arg = |offset: usize| rules.get(at.saturating_add(offset));
        let mismatch = |offset: usize, expected: &str| BadRule {
            rule,
            detail: format!(
                "arg #{offset} {expected} expected but {} given.",
                arg(offset).map_or("nothing", |element| element.type_name().keyword())
            ),
        };
        let integer = |offset: usize| match arg(offset) {
            Some(Element::Integer(value)) => Ok(*value),
            _ => Err(mismatch(offset, "integer")),
        };
        let vector = |offset: usize| match arg(offset) {
            Some(Element::Vector(value)) => Ok(value.clone()),
            _ => Err(mismatch(offset, "vector")),
        };
        let float = |offset: usize| match arg(offset) {
            Some(Element::Float(value)) => Ok(*value),
            Some(Element::Integer(value)) => Ok(crate::num::int_to_float(*value)),
            _ => Err(mismatch(offset, "float")),
        };
        let string = |offset: usize| match arg(offset) {
            Some(Element::String(value)) => Ok(value.clone()),
            _ => Err(mismatch(offset, "string")),
        };
        let Element::Integer(code) = code else {
            return Err(BadRule {
                rule,
                detail: "rule id is not an integer.".to_owned(),
            });
        };
        let width = match *code {
            // REZ_PARAM
            0 => {
                rez.start_parameter = integer(1)?;
                2
            }
            // REZ_FLAGS
            1 => {
                rez.temporary = integer(1)? & REZ_FLAG_TEMP != 0;
                2
            }
            // REZ_POS: the position, whether it is from the rezzer, whether
            // it places the root.
            2 => {
                let position = vector(1)?;
                let relative = integer(2)? != 0;
                rez.at_root = integer(3)? != 0;
                rez.position = if relative {
                    Vector {
                        x: here.x + position.x,
                        y: here.y + position.y,
                        z: here.z + position.z,
                    }
                } else {
                    position
                };
                4
            }
            // REZ_ROT: the rotation, whether it is the rezzer's turned.
            3 => {
                let rotation = match arg(1) {
                    Some(Element::Rotation(value)) => value.clone(),
                    _ => return Err(mismatch(1, "rotation")),
                };
                let relative = integer(2)? != 0;
                rez.rotation = normalised(&if relative {
                    crate::ops::compose(&rotation, turned)
                } else {
                    rotation
                });
                3
            }
            // REZ_VEL: the velocity, whether it is local, whether the
            // rezzer's is added.
            4 => {
                rez.velocity = vector(1)?;
                let _local = integer(2)?;
                let _inherit = integer(3)?;
                4
            }
            // REZ_ACCEL, REZ_LOCK_AXES: a vector and a flag, or a vector.
            5 => {
                let _accel = vector(1)?;
                let _local = integer(2)?;
                3
            }
            11 => {
                let _axes = vector(1)?;
                2
            }
            // REZ_OMEGA: the axis, whether it is local, the rate and gain.
            7 => {
                let _axis = vector(1)?;
                let _local = integer(2)?;
                let _rate = float(3)?;
                let _gain = float(4)?;
                5
            }
            // REZ_DAMAGE, REZ_DAMAGE_TYPE.
            8 => {
                let _damage = float(1)?;
                2
            }
            12 => {
                let _kind = integer(1)?;
                2
            }
            // REZ_SOUND: the sound, its volume, whether it loops;
            // REZ_SOUND_COLLIDE: the sound and its volume.
            9 => {
                let _sound = string(1)?;
                let _volume = float(2)?;
                let _loop = integer(3)?;
                4
            }
            10 => {
                let _sound = string(1)?;
                let _volume = float(2)?;
                3
            }
            // REZ_PARAM_STRING.
            13 => {
                rez.start_string = cut_bytes(&string(1)?, START_STRING_BYTES);
                2
            }
            other => {
                return Err(BadRule {
                    rule,
                    detail: format!("unknown rule id {other}."),
                });
            }
        };
        at = at.saturating_add(width);
    }
    Ok(())
}

/// `llRezObjectWithParams`: rez the object `item` as the `REZ_*` rules say —
/// where (`REZ_POS`, from the rezzer or the region, root or centre), turned
/// how, moving how, its start parameter and start string (cut at 1018 bytes),
/// whether it is temporary — and answer the key of its root. An unknown rule
/// is `llRezObjectWithParams Error running rule #2: unknown rule id 999.` and
/// rezzes nothing (aditi); the wording of a rule whose argument is the wrong
/// type follows it, not measured. Second Life's, which OpenSim does not
/// compile. The rules that act only on a physical object — acceleration,
/// spin, damage, sounds, locked axes — are read and checked and change
/// nothing on the fake grid, which has no physics.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rez_object_with_params(
    ctx: &mut ScriptCtx<'_>,
    item: String,
    rules: Vec<Element>,
) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let placement = ctx.host().placement(caller);
    let here = placement.position;
    let turned = ctx.host().whereabouts(caller).prim;
    let rotation = ctx
        .host()
        .prim(caller, &turned)
        .map_or(crate::value::ZERO_ROTATION, |prim| prim.rotation);
    let mut rez = rez_of(
        ctx,
        item.clone(),
        (here.clone(), false),
        crate::value::ZERO_VECTOR,
        &crate::value::ZERO_ROTATION,
        0,
    );
    if let Err(bad) = read_rules(&rules, &mut rez, (&here, &rotation)) {
        ctx.refuse(format!(
            "llRezObjectWithParams Error running rule #{}: {}",
            bad.rule, bad.detail
        ));
        return Ok(Key(NULL_KEY.to_owned()));
    }
    if checked_rez(ctx, &item, &rez.position).is_none() {
        return Ok(Key(NULL_KEY.to_owned()));
    }
    Ok(Key(rez_with(ctx, &rez)))
}

/// `llDerezObject`: take an object the caller's object rezzed out of the
/// region (`DEREZ_DIE`): 1 when it goes, 0 for one already gone and for
/// `NULL_KEY` (`inventory/newer`). The other ways — into the owner's
/// inventory, made temporary — are not measured, and answer 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_derez_object(ctx: &mut ScriptCtx<'_>, object: Key, how: i32) -> Result<i32, CallError> {
    if how != DEREZ_DIE || object.0 == NULL_KEY || !crate::library::dotnet::is_uuid(&object.0) {
        return Ok(0);
    }
    let caller = ctx.caller();
    Ok(i32::from(ctx.host().derez(caller, &object.0)))
}

/// `llGetStartString`: the start string the object was rezzed with
/// (`REZ_PARAM_STRING`), `""` for any other rez.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_start_string(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(ctx.start_string().to_owned())
}

/// The words of an offer of `what` (an item's name, or a folder's) from the
/// caller's object: Second Life names the place as a SLurl (and the folder
/// too), OpenSim names a single item's place as region and position and a
/// folder's not at all.
fn offer_message(ctx: &mut ScriptCtx<'_>, what: &str, folder: bool) -> String {
    let caller = ctx.caller();
    let at = ctx.host().whereabouts(caller);
    if ctx.flavour().is_opensim() {
        if folder {
            return format!("'{what}'");
        }
        return format!(
            "'{what}'. ({} is located at {} <{}, {}, {}>)",
            at.name, at.region, at.position.x, at.position.y, at.position.z
        );
    }
    let whole = |value: f32| -> i64 { crate::num::double_to_long(f64::from(value).trunc()) };
    format!(
        "'{what}'  ( http://slurl.com/secondlife/{}/{}/{}/{} )",
        at.region.replace(' ', "%20"),
        whole(at.position.x),
        whole(at.position.y),
        whole(at.position.z)
    )
}

/// `llGiveInventory`: give the item `name` to `destination` — into a prim's
/// contents (a name it holds already becomes `Gift 1`, a script arrives
/// stopped, `changed(CHANGED_INVENTORY)` there), or as an offer to an avatar,
/// marked offline, which holds the script 2 s on Second Life and 3 s on
/// OpenSim. A missing name is refused: on Second Life
/// `Unable to give inventory: 'No item named 'Nope'.'.`, on OpenSim with a
/// second's sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_give_inventory(
    ctx: &mut ScriptCtx<'_>,
    destination: Key,
    name: String,
) -> Result<(), CallError> {
    let opensim = ctx.flavour().is_opensim();
    if opensim && !crate::library::dotnet::is_uuid(&destination.0) {
        ctx.refuse(format!(
            "llGiveInventory: Can't parse destination key '{}'",
            destination.0
        ));
        ctx.sleep(1.0);
        return Ok(());
    }
    if named(ctx, &name).is_none() {
        if opensim {
            ctx.refuse(format!(
                "llGiveInventory: Can't find inventory object '{name}'"
            ));
            ctx.sleep(1.0);
        } else {
            ctx.refuse(format!(
                "Unable to give inventory: 'No item named '{name}'.'."
            ));
        }
        return Ok(());
    }
    let caller = ctx.caller();
    let items = [name.clone()];
    if ctx.host().prim_scripts(&destination.0).is_some() {
        let _given = ctx.host().give_to_prim(caller, &destination.0, &items);
        return Ok(());
    }
    let message = offer_message(ctx, &name, false);
    let offer = Offer {
        agent: destination.0,
        items: items.to_vec(),
        folder: None,
        message,
        offline: true,
    };
    if ctx.host().offer_items(caller, &offer) {
        ctx.sleep(offer_delay(opensim));
    } else if opensim {
        ctx.refuse(format!(
            "llGiveInventory: Can't find destination '{}'",
            offer.agent
        ));
        ctx.sleep(1.0);
    }
    Ok(())
}

/// `llGiveInventoryList`: give the items named in `inventory` that the prim
/// holds to `destination` — into a prim's contents one by one, no folder;
/// to an avatar as an offer of a folder named `folder`, not marked offline.
/// OpenSim refuses a destination that is neither, and a list none of whose
/// items the prim holds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_give_inventory_list(
    ctx: &mut ScriptCtx<'_>,
    destination: Key,
    folder: String,
    inventory: Vec<Element>,
) -> Result<(), CallError> {
    if inventory.is_empty() {
        return Ok(());
    }
    let opensim = ctx.flavour().is_opensim();
    let caller = ctx.caller();
    let held = ctx.host().contents(caller);
    let items: Vec<String> = inventory
        .iter()
        .map(crate::cast::element_to_string)
        .filter_map(|wanted| {
            held.iter()
                .find(|item| item.name == wanted || item.item.eq_ignore_ascii_case(&wanted))
                .map(|item| item.name.clone())
        })
        .collect();
    let to_prim = ctx.host().prim_scripts(&destination.0).is_some();
    if items.is_empty() {
        if opensim {
            ctx.refuse("llGiveInventoryList: Unable to give list, no items found".to_owned());
            ctx.sleep(1.0);
        }
        return Ok(());
    }
    if to_prim {
        let _given = ctx.host().give_to_prim(caller, &destination.0, &items);
        return Ok(());
    }
    let message = offer_message(ctx, &folder, true);
    let offer = Offer {
        agent: destination.0,
        items,
        folder: Some(folder),
        message,
        offline: false,
    };
    if !ctx.host().offer_items(caller, &offer) && opensim {
        ctx.refuse("llGiveInventoryList: Unable to give list, destination not found".to_owned());
        ctx.sleep(1.0);
    }
    Ok(())
}

/// `llRemoteLoadScriptPin`: load the script `name` of the prim into the prim
/// keyed `target`, which must have set `pin` (`llSetRemoteScriptAccessPin`)
/// and be the same owner's: it arrives over a script of that name, running or
/// not, and starts afresh with `start_parameter`. A wrong pin is
/// `Task '…' trying to illegally load script onto task '…'!` on Second Life
/// and silence on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_remote_load_script_pin(
    ctx: &mut ScriptCtx<'_>,
    target: Key,
    name: String,
    pin: i32,
    running: i32,
    start_parameter: i32,
) -> Result<(), CallError> {
    let opensim = ctx.flavour().is_opensim();
    let caller = ctx.caller();
    let here = ctx.host().whereabouts(caller);
    if here.prim.eq_ignore_ascii_case(&target.0) {
        return Ok(());
    }
    if named(ctx, &name).is_none_or(|item| item.kind != INVENTORY_SCRIPT) {
        if opensim {
            ctx.refuse(format!("llRemoteLoadScriptPin: Can't find script '{name}'"));
            ctx.sleep(1.0);
        }
        return Ok(());
    }
    let Some((set, owner)) = ctx.host().remote_pin(&target.0) else {
        return Ok(());
    };
    if set == 0 || set != pin || owner != ctx.host().owner(caller) {
        if !opensim {
            let theirs = ctx
                .host()
                .prim(caller, &target.0)
                .map(|prim| prim.name)
                .unwrap_or_default();
            ctx.refuse(format!(
                "Task '{}' trying to illegally load script onto task '{theirs}'!",
                here.name
            ));
        }
        return Ok(());
    }
    ctx.host().load_script(
        caller,
        &RemoteLoad {
            target: target.0,
            script: name,
            running: running != 0,
            start_parameter,
        },
    );
    Ok(())
}
