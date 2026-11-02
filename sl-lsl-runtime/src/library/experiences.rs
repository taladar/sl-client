//! Library tranche: experiences and the experience key-value store
//! (`server-lsl-lib-experience`) — asking an avatar into the script's
//! experience, reading experiences, the key-value store the experience keeps
//! on the grid, and seating an avatar that allowed it.
//!
//! What a script in **no** experience is answered is aditi's (book chapter
//! `gridspec/lsl.md`, § Experiences and the key-value store; probes
//! `experience/no-experience`, `experience/sit-on-link` and
//! `experience/unserved`). What a script in one is answered is Linden Lab's
//! documentation, unmeasured (see [`crate::vm::experiences`]). OpenSim's
//! engines compile none of these functions, so every one is Second Life's.

use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::experiences::{
    self, Admission, EXPERIENCE_PERMISSIONS, ExperienceDetails, ExperienceError,
    ExperienceQuestion, KeyValueStore, SIT_INVALID_LINK, SIT_NO_EXPERIENCE_PERMISSION,
    SIT_NOT_EXPERIENCE, SIT_OK,
};

/// `LINK_ROOT`.
const LINK_ROOT: i32 = 1;
/// `LINK_THIS`.
const LINK_THIS: i32 = -4;

/// The key of the experience the script was compiled into, when it may run
/// where the script is; otherwise why not — [`ExperienceError::NoExperience`]
/// for a script in none.
fn experience_here(ctx: &mut ScriptCtx<'_>) -> Result<String, ExperienceError> {
    let caller = ctx.caller();
    let experience = ctx
        .host()
        .experience(caller)
        .ok_or(ExperienceError::NoExperience)?;
    experience.standing.map(|()| experience.key)
}

/// `llRequestExperiencePermissions`: ask `agent` into the script's
/// experience. A script in no experience is denied at once with
/// [`ExperienceError::NoExperience`], no question put, nothing held and no
/// complaint (`experience/no-experience`, the owner and another avatar
/// alike).
///
/// In an experience (unmeasured; the library table's tooltip and the wiki):
/// one that may not run here is denied with why; an avatar that allowed it
/// before is granted at once and one that blocked it denied
/// ([`ExperienceError::NotPermitted`]), with no question; anyone else is put
/// the question, and an avatar not in the region, who cannot be, is denied
/// as one that refused. The grant holds [`EXPERIENCE_PERMISSIONS`] for
/// `agent`. The name is unused.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_experience_permissions(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    _name: String,
) -> Result<(), CallError> {
    let experience = match experience_here(ctx) {
        Ok(experience) => experience,
        Err(error) => {
            ctx.deny_experience(agent.0, error);
            return Ok(());
        }
    };
    match ctx.host().admission(&agent.0, &experience) {
        Admission::Allowed => ctx.grant_experience(agent.0, EXPERIENCE_PERMISSIONS),
        Admission::Blocked => ctx.deny_experience(agent.0, ExperienceError::NotPermitted),
        Admission::Undecided => {
            let question = ExperienceQuestion {
                agent: agent.0.clone(),
                experience,
                mask: EXPERIENCE_PERMISSIONS,
            };
            if !ctx.ask_experience(question) {
                ctx.deny_experience(agent.0, ExperienceError::NotPermitted);
            }
        }
    }
    Ok(())
}

/// `llAgentInExperience`: whether `agent` allowed the script's experience
/// and it may run here — `FALSE` for a script in none
/// (`experience/no-experience`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_agent_in_experience(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<i32, CallError> {
    let Ok(experience) = experience_here(ctx) else {
        return Ok(0);
    };
    Ok(i32::from(
        ctx.host().admission(&agent.0, &experience) == Admission::Allowed,
    ))
}

/// The list `llGetExperienceDetails` answers for `details`, in aditi's
/// order: name, owner, experience, state, its message, group — not the
/// library table's tooltip's, which puts the group third.
fn details_list(details: ExperienceDetails) -> Vec<Element> {
    vec![
        Element::String(details.name),
        Element::Key(details.owner),
        Element::Key(details.id),
        Element::Integer(details.state.code()),
        Element::String(details.state.message().to_owned()),
        Element::Key(details.group),
    ]
}

/// `llGetExperienceDetails`: the details of the experience `id`, or of the
/// script's own for `NULL_KEY` — an empty list for a script in none, and
/// while the region has yet to look `id` up (`experience/no-experience`:
/// a public experience answered at once, an id no experience has empty and
/// then invalid).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_experience_details(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let id = if id.0.eq_ignore_ascii_case(NULL_KEY) {
        match ctx.host().experience(caller) {
            Some(own) => own.key,
            None => return Ok(Vec::new()),
        }
    } else {
        id.0
    };
    Ok(ctx
        .host()
        .experience_details(caller, &id)
        .map(details_list)
        .unwrap_or_default())
}

/// `llGetExperienceErrorMessage`: the text of an `XP_ERROR_*` code, in
/// aditi's words, and `unknown error id` for any other.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_experience_error_message(
    _ctx: &mut ScriptCtx<'_>,
    code: i32,
) -> Result<String, CallError> {
    Ok(experiences::error_message(code).to_owned())
}

/// A key-value call: a fresh handle now, and `dataserver(handle, answer)`
/// once the slice is over — `"0,5"` for a script in no experience
/// (`experience/no-experience`, all seven calls), the error that stops the
/// experience here or the store's, or what `call` answers of the
/// experience's store.
fn key_value(ctx: &mut ScriptCtx<'_>, call: impl FnOnce(&mut KeyValueStore) -> String) -> Key {
    let handle = ctx.fresh_key();
    let answer = experience_here(ctx)
        .and_then(|experience| ctx.host().key_value_store(&experience).map(call));
    let data = answer.unwrap_or_else(|error| format!("0,{}", error.code()));
    ctx.raise_dataserver(handle.clone(), data);
    Key(handle)
}

/// `llCreateKeyValue`: create the pair `k` = `v` in the experience's store.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_create_key_value(
    ctx: &mut ScriptCtx<'_>,
    k: String,
    v: String,
) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.create(k, v)))
}

/// `llReadKeyValue`: read the pair `k` of the experience's store.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_read_key_value(ctx: &mut ScriptCtx<'_>, k: String) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.read(&k)))
}

/// `llUpdateKeyValue`: write `k` = `v` in the experience's store; `checked`,
/// only while it holds `original_value`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_update_key_value(
    ctx: &mut ScriptCtx<'_>,
    k: String,
    v: String,
    checked: i32,
    original_value: String,
) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| {
        store.update(k, v, checked != 0, &original_value)
    }))
}

/// `llDeleteKeyValue`: delete the pair `k` of the experience's store.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_delete_key_value(ctx: &mut ScriptCtx<'_>, k: String) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.delete(&k)))
}

/// `llKeyCountKeyValue`: how many pairs the experience's store holds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_key_count_key_value(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.count()))
}

/// `llKeysKeyValue`: `count` keys of the experience's store from the
/// `first`th.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_keys_key_value(
    ctx: &mut ScriptCtx<'_>,
    first: i32,
    count: i32,
) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.keys(first, count)))
}

/// `llDataSizeKeyValue`: the bytes the experience's store uses, and its
/// quota.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_data_size_key_value(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(key_value(ctx, |store| store.size()))
}

/// The link number `link` names a single prim of the caller's linkset by, as
/// `llSitOnLink` reads it: `LINK_THIS` the caller's, `LINK_ROOT` the root,
/// a number the prim of that number (0 and 1 alike for a prim linked to
/// nothing); [`None`] for no prim and for a constant naming several.
pub(crate) fn single_prim(ctx: &mut ScriptCtx<'_>, link: i32) -> Option<i32> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let alone = links.prims.len() == 1;
    match link {
        LINK_THIS => Some(links.this),
        LINK_ROOT if alone => links.prims.first().map(|prim| prim.link),
        number if number >= 0 => links
            .prims
            .iter()
            .find(|prim| prim.link == number || (alone && number <= 1))
            .map(|prim| prim.link),
        _ => None,
    }
}

/// `llSitOnLink`: sit `agent` on the sit target of the prim `link`. A
/// script in no experience is refused `SIT_NOT_EXPERIENCE` (−1) before the
/// agent or the link is looked at — every link number, `NULL_KEY`, a key
/// nobody has, the prim's own — and nobody sits, with no complaint
/// (`experience/sit-on-link`).
///
/// In an experience (unmeasured; the wiki, in the order it lists them): one
/// that may not run here is `SIT_NOT_EXPERIENCE`, an avatar that has not
/// allowed it `SIT_NO_EXPERIENCE_PERMISSION` (−2), a link naming no prim or
/// several `SIT_INVALID_LINK` (−5), and what the region refuses its
/// [`SitRefusal`](crate::vm::SitRefusal); `SIT_OK` (1) when the avatar sits.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sit_on_link(ctx: &mut ScriptCtx<'_>, agent: Key, link: i32) -> Result<i32, CallError> {
    let Ok(experience) = experience_here(ctx) else {
        return Ok(SIT_NOT_EXPERIENCE);
    };
    if ctx.host().admission(&agent.0, &experience) != Admission::Allowed {
        return Ok(SIT_NO_EXPERIENCE_PERMISSION);
    }
    let Some(link) = single_prim(ctx, link) else {
        return Ok(SIT_INVALID_LINK);
    };
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .sit_on_link(caller, &agent.0, link)
        .map_or_else(|refusal| refusal.code(), |()| SIT_OK))
}

/// `llClearExperiencePermissions`, which no document Second Life serves
/// lists: nothing — for the owner, another avatar and `NULL_KEY`, no event,
/// no complaint, and an ordinary grant held is left as it is
/// (`experience/unserved`). Measured only in no experience; nothing is known
/// of it in one, so it is nothing there too.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_clear_experience_permissions(
    _ctx: &mut ScriptCtx<'_>,
    _agent: Key,
) -> Result<(), CallError> {
    Ok(())
}

/// `llGetExperienceList`, which no document Second Life serves lists: an
/// empty list, for the owner, another avatar and `NULL_KEY`
/// (`experience/unserved`). Measured only in no experience; nothing is known
/// of it in one, so it is empty there too.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_experience_list(
    _ctx: &mut ScriptCtx<'_>,
    _agent: Key,
) -> Result<Vec<Element>, CallError> {
    Ok(Vec::new())
}
