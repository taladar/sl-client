//! Library tranche: what a script senses (`server-lsl-lib-detection-sensors`)
//! — sweeps for objects and avatars (`llSensor`, `llSensorRepeat`,
//! `llSensorRemove`), rays (`llCastRay`), and OpenSim's region-wide avatar
//! lists. A sweep's answer comes as an event once the engine has swept at the
//! next tick (`vm::sensing`); a ray's at once.

use sl_types::lsl::Vector;

use crate::library::{BuiltinId, CallError, Exception, Key};
use crate::value::Element;
use crate::vm::ScriptCtx;
use crate::vm::sensing::{HitKind, Query, RayHit, Sensed};

/// `RC_REJECT_TYPES`.
const RC_REJECT_TYPES: i32 = 0;
/// `RC_DETECT_PHANTOM`.
const RC_DETECT_PHANTOM: i32 = 1;
/// `RC_DATA_FLAGS`.
const RC_DATA_FLAGS: i32 = 2;
/// `RC_MAX_HITS`.
const RC_MAX_HITS: i32 = 3;
/// `RC_REJECT_AGENTS`.
const RC_REJECT_AGENTS: i32 = 0x1;
/// `RC_REJECT_PHYSICAL`.
const RC_REJECT_PHYSICAL: i32 = 0x2;
/// `RC_REJECT_NONPHYSICAL`.
const RC_REJECT_NONPHYSICAL: i32 = 0x4;
/// `RC_REJECT_LAND`.
const RC_REJECT_LAND: i32 = 0x8;
/// `RC_REJECT_HOST`, which Second Life refuses.
const RC_REJECT_HOST: i32 = 0x2000_0000;
/// `RC_GET_NORMAL`.
const RC_GET_NORMAL: i32 = 0x1;
/// `RC_GET_ROOT_KEY`.
const RC_GET_ROOT_KEY: i32 = 0x2;
/// `RC_GET_LINK_NUM`.
const RC_GET_LINK_NUM: i32 = 0x4;
/// The most hits Second Life returns.
const MAX_HITS: i32 = 256;

/// The query `llSensor`'s arguments make.
fn query_of(name: String, id: &Key, mask: i32, range: f32, arc: f32) -> Query {
    Query {
        name,
        key: id.0.clone(),
        mask,
        range,
        arc,
    }
}

/// `llSensor`: sweep once for what `name`, `id` and `mask` admit within
/// `range` metres (at most 96) and `arc` radians of the prim's forward axis;
/// `sensor` with the nearest first (32 at most on Second Life, 16 on
/// OpenSim) or `no_sensor` follows. Second Life answers three calls in one
/// event once, OpenSim three times; on Second Life a call ends a repeating
/// sweep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sensor(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    id: Key,
    mask: i32,
    range: f32,
    arc: f32,
) -> Result<(), CallError> {
    ctx.sense_once(query_of(name, &id, mask, range, arc));
    Ok(())
}

/// `llSensorRepeat`: [`ll_sensor`] every `rate` seconds, until
/// `llSensorRemove`, a state change or a reset.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sensor_repeat(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    id: Key,
    mask: i32,
    range: f32,
    arc: f32,
    rate: f32,
) -> Result<(), CallError> {
    ctx.sense_repeat(query_of(name, &id, mask, range, arc), rate);
    Ok(())
}

/// `llSensorRemove`: end the repeating sweep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sensor_remove(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    ctx.sense_remove();
    Ok(())
}

/// `llCastRay`'s options, read.
#[derive(Debug, Clone, Copy)]
struct RayOptions {
    /// What is left out.
    reject: i32,
    /// Whether phantom prims are hit.
    phantom: bool,
    /// What each hit carries.
    data: i32,
    /// How many hits at most.
    max_hits: i32,
}

/// Why Second Life refused a ray's options, in its words.
const fn refusal(code: u8) -> &'static str {
    match code {
        0 => "Syntax error in llCastRay call. Incorrect number of elements in param list.",
        1 => "Invalid filter settings.",
        2 => "Invalid filter settings for llCastRay.",
        3 => "You must request at least one result from llCastRay.",
        _ => "Error. No more than 256 hits can be return by llCastRay.",
    }
}

/// Reads `options` as `flavour` does: Second Life refuses what it does not
/// take (the words of [`refusal`]); OpenSim ignores an unknown option and
/// throws at one with no value.
fn read_options(options: &[Element], opensim: bool) -> Result<RayOptions, Result<u8, ()>> {
    let mut read = RayOptions {
        reject: 0,
        phantom: false,
        data: 0,
        max_hits: 1,
    };
    let mut at = 0_usize;
    while let Some(option) = options.get(at) {
        let Some(value) = options.get(at.saturating_add(1)) else {
            return Err(if opensim { Err(()) } else { Ok(0) });
        };
        let value = match value {
            Element::Integer(value) => *value,
            _ if opensim => 0,
            _ => return Err(Ok(0)),
        };
        match option {
            Element::Integer(RC_REJECT_TYPES) => read.reject = value,
            Element::Integer(RC_DETECT_PHANTOM) => read.phantom = value != 0,
            Element::Integer(RC_DATA_FLAGS) => read.data = value,
            Element::Integer(RC_MAX_HITS) => read.max_hits = value,
            _ if opensim => {}
            _ => return Err(Ok(1)),
        }
        at = at.saturating_add(2);
    }
    if opensim {
        // OpenSim treats a request for no hit as one for one.
        if read.max_hits == 0 {
            read.max_hits = 1;
        }
    } else {
        if read.reject & RC_REJECT_HOST != 0 {
            return Err(Ok(2));
        }
        if read.max_hits == 0 {
            return Err(Ok(3));
        }
        if !(0..=MAX_HITS).contains(&read.max_hits) {
            return Err(Ok(4));
        }
    }
    Ok(read)
}

/// Whether `hit` is one `options` let through in `flavour`.
const fn keeps(hit: &RayHit, options: &RayOptions, opensim: bool) -> bool {
    // Second Life never reports a prim the ray starts inside; OpenSim reports
    // where it leaves it, the casting prim included.
    if hit.from_inside && !opensim {
        return false;
    }
    if hit.host && options.reject & RC_REJECT_HOST != 0 {
        return false;
    }
    match hit.kind {
        HitKind::Land => options.reject & RC_REJECT_LAND == 0,
        HitKind::Agent => options.reject & RC_REJECT_AGENTS == 0,
        HitKind::Prim { physical, phantom } => {
            if phantom && !options.phantom {
                return false;
            }
            let rejected = if physical {
                RC_REJECT_PHYSICAL
            } else {
                RC_REJECT_NONPHYSICAL
            };
            options.reject & rejected == 0
        }
    }
}

/// A vector as .NET's `Vector3.ToString` writes it, which OpenSim's
/// `llCastRay` returns its positions as: `<124.75, 128, 27.5>`.
fn dotnet_vector(v: &Vector) -> String {
    format!("<{}, {}, {}>", v.x, v.y, v.z)
}

/// `llCastRay`: what the segment from `start` to `end` hits, nearest first,
/// as many as `RC_MAX_HITS` asks (one by default), and last the number of
/// hits. Each hit is `[key, position]`, with the link number and the normal
/// where `RC_DATA_FLAGS` asks for them — the link before the position on
/// Second Life and after it on OpenSim, whose position is a string. Second
/// Life never reports a prim the ray starts inside and refuses options it
/// does not take with an empty list; OpenSim reports where a ray leaves the
/// prim it starts in, ignores an unknown option and throws at one with no
/// value. Its default ray's two hits on land past the ray's end are a bug of
/// its own, not imitated.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for an option with no value.
pub fn ll_cast_ray(
    ctx: &mut ScriptCtx<'_>,
    start: Vector,
    end: Vector,
    options: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let opensim = ctx.flavour().is_opensim();
    let read = match read_options(&options, opensim) {
        Ok(read) => read,
        Err(Ok(code)) => {
            ctx.refuse(refusal(code).to_owned());
            return Ok(Vec::new());
        }
        Err(Err(())) => {
            return Err(CallError::Exception {
                id: BuiltinId::LlCastRay,
                exception: Exception::IndexOutOfRange,
            });
        }
    };
    if start == end {
        return Ok(vec![Element::Integer(0)]);
    }
    let caller = ctx.caller();
    let limit = usize::try_from(read.max_hits.max(0)).unwrap_or(0);
    let hits: Vec<RayHit> = ctx
        .host()
        .cast_ray(caller, &start, &end)
        .into_iter()
        .filter(|hit| keeps(hit, &read, opensim))
        .take(limit)
        .collect();
    let mut out = Vec::new();
    for hit in &hits {
        let key = if read.data & RC_GET_ROOT_KEY != 0 {
            &hit.root
        } else {
            &hit.key
        };
        out.push(Element::Key(key.clone()));
        let link = Element::Integer(hit.link);
        let position = if opensim {
            Element::String(dotnet_vector(&hit.position))
        } else {
            Element::Vector(hit.position.clone())
        };
        if read.data & RC_GET_LINK_NUM != 0 && !opensim {
            out.push(link.clone());
        }
        out.push(position);
        if read.data & RC_GET_LINK_NUM != 0 && opensim {
            out.push(link);
        }
        if read.data & RC_GET_NORMAL != 0 {
            out.push(Element::Vector(hit.normal.clone()));
        }
    }
    out.push(Element::Integer(
        i32::try_from(hits.len()).unwrap_or(i32::MAX),
    ));
    Ok(out)
}

/// The avatars of the caller's region.
fn avatars(ctx: &mut ScriptCtx<'_>) -> Vec<Sensed> {
    let caller = ctx.caller();
    ctx.host()
        .sense(caller)
        .into_iter()
        .filter(|sensed| sensed.agent)
        .collect()
}

/// `osGetAgents`: the name of every avatar in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_agents(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    Ok(avatars(ctx)
        .into_iter()
        .map(|avatar| Element::String(avatar.name))
        .collect())
}

/// `osGetAvatarList`: every avatar in the region but the script's owner, as
/// its key (a string), its position and its name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_avatar_list(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    Ok(avatars(ctx)
        .into_iter()
        .filter(|avatar| !avatar.key.eq_ignore_ascii_case(&owner))
        .flat_map(|avatar| {
            [
                Element::String(avatar.key),
                Element::Vector(avatar.position),
                Element::String(avatar.name),
            ]
        })
        .collect())
}
