//! Library tranche: chat, listens, dialogs and link messages
//! (`server-lsl-lib-comms`) — how a script talks to residents, to other
//! scripts and to its own linkset, and points a viewer somewhere.
//!
//! Each function checks and cuts what it is given as the region's grid does
//! (book chapter `gridspec/lsl.md`, § Chat, listens, dialogs and link
//! messages, measured by the `comms/*` probes on aditi and both OpenSim
//! engines) and hands the host a finished request; who in the region it
//! reaches is the host's. Second Life's words are aditi's; OpenSim's are
//! `LSL_Api.cs`'s, which the probes confirmed. A forced delay is the keyword
//! file's, charged by the VM after the call; where a grid measured another,
//! that is `server-lsl-forced-delays-per-flavour`'s.

use sl_types::chat::ChatVolume;
use sl_types::lsl::Vector;

use crate::cast::element_to_string;
use crate::library::dotnet;
use crate::library::text::Text;
use crate::library::{CallError, Key};
use crate::value::Element;
use crate::vm::comms::{cut_bytes, is_somebody};
use crate::vm::{
    DEBUG_CHANNEL, Dialog, EngineFlavour, InstantMessage, LinkMessage, ListenFilter, LoadUrl,
    MapDestination, ScriptCtx, Speech, TEXT_BOX_BUTTON, TooManyListens,
};

/// How many bytes of a line a listener hears on Second Life: a say of 1100
/// characters was heard as 1024 (`comms/object-chat`); the documented limit
/// is in bytes.
const SECOND_LIFE_LINE_BYTES: usize = 1024;

/// How many bytes of a line a listener hears on OpenSim: 1023, what
/// `Utils.StringToBytesNoTerm(text, 1023)` keeps, ending at a character.
const OPENSIM_LINE_BYTES: usize = 1023;

/// How many units of a `llRegionSayTo` line OpenSim keeps: it cuts the .NET
/// string to 1023 UTF-16 units before it delivers it.
const OPENSIM_SAY_TO_UNITS: usize = 1023;

/// How many bytes of an instant message's text arrive, on both grids: 1100
/// characters arrived as 1023 (`comms/instant-message`), what one
/// `ImprovedInstantMessage` carries.
const INSTANT_MESSAGE_BYTES: usize = 1023;

/// The most buttons a dialog shows, on both grids.
const MAX_BUTTONS: usize = 12;

/// The longest button label Second Life shows, in characters; a longer one is
/// refused.
const SECOND_LIFE_LABEL_CHARACTERS: usize = 24;

/// The longest button label OpenSim shows, in bytes: it cuts a longer one,
/// without complaint, to fit the `ScriptDialog` field.
const OPENSIM_LABEL_BYTES: usize = 23;

/// A dialog or text box message as long as this is refused on Second Life
/// ("must be less than 512 characters").
const SECOND_LIFE_MESSAGE_CHARACTERS: usize = 512;

/// A dialog or text box message longer than this, in bytes, is complained of
/// on OpenSim: a dialog is still shown, a text box is not.
const OPENSIM_MESSAGE_BYTES: usize = 512;

/// How much of a dialog's message OpenSim shows, in bytes.
const OPENSIM_SHOWN_MESSAGE_BYTES: usize = 511;

/// A line the script's object says to listeners in `flavour`, cut as the grid
/// cuts it.
fn line(flavour: EngineFlavour, text: &str) -> String {
    let bytes = if flavour.is_opensim() {
        OPENSIM_LINE_BYTES
    } else {
        SECOND_LIFE_LINE_BYTES
    };
    cut_bytes(text, bytes).to_owned()
}

/// Say `text` on `channel` as far as `volume` carries, to avatars and
/// scripts alike.
fn speak(ctx: &mut ScriptCtx<'_>, channel: i32, text: &str, volume: ChatVolume) {
    let speech = Speech {
        channel,
        text: line(ctx.flavour(), text),
        to_avatars: volume,
        to_scripts: Some(volume),
    };
    let caller = ctx.caller();
    ctx.host().say(caller, &speech);
}

/// `llWhisper`: say `text` on `channel` within 10 m.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_whisper(ctx: &mut ScriptCtx<'_>, channel: i32, text: String) -> Result<(), CallError> {
    speak(ctx, channel, &text, ChatVolume::Whisper);
    Ok(())
}

/// `llSay`: say `text` on `channel` within 20 m.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_say(ctx: &mut ScriptCtx<'_>, channel: i32, text: String) -> Result<(), CallError> {
    speak(ctx, channel, &text, ChatVolume::Say);
    Ok(())
}

/// `llShout`: say `text` on `channel` within 100 m.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_shout(ctx: &mut ScriptCtx<'_>, channel: i32, text: String) -> Result<(), CallError> {
    speak(ctx, channel, &text, ChatVolume::Shout);
    Ok(())
}

/// `llRegionSay`: say `text` on `channel` to the whole region — refused on
/// channel 0, on both grids, in each one's words.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_region_say(ctx: &mut ScriptCtx<'_>, channel: i32, text: String) -> Result<(), CallError> {
    if channel == 0 {
        let refusal = if ctx.flavour().is_opensim() {
            "llRegionSay: Cannot use on channel 0"
        } else {
            "Cannot use llRegionSay() on channel 0."
        };
        ctx.refuse(refusal.to_owned());
        return Ok(());
    }
    speak(ctx, channel, &text, ChatVolume::RegionSay);
    Ok(())
}

/// `llRegionSayTo`: say `text` on `channel` to `target` alone, at any
/// distance in the region. A target that is no key, `NULL_KEY` and the
/// script's own prim get nothing, and nothing is said of it; OpenSim says
/// nothing on `DEBUG_CHANNEL` this way either.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_region_say_to(
    ctx: &mut ScriptCtx<'_>,
    target: Key,
    channel: i32,
    text: String,
) -> Result<(), CallError> {
    let flavour = ctx.flavour();
    let caller = ctx.caller();
    if !is_somebody(&target.0)
        || (flavour.is_opensim() && channel == DEBUG_CHANNEL)
        || ctx
            .host()
            .whereabouts(caller)
            .prim
            .eq_ignore_ascii_case(&target.0)
    {
        return Ok(());
    }
    let text = if flavour.is_opensim() {
        let units = Text::new(flavour, &text);
        units.string(
            units
                .units()
                .get(..OPENSIM_SAY_TO_UNITS)
                .unwrap_or_else(|| units.units()),
        )
    } else {
        line(flavour, &text)
    };
    ctx.host().say_to(caller, &target.0, channel, &text);
    Ok(())
}

/// `llOwnerSay`: tell the owner `text`, and nobody else — only while the
/// owner is in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_owner_say(ctx: &mut ScriptCtx<'_>, text: String) -> Result<(), CallError> {
    let text = line(ctx.flavour(), &text);
    let caller = ctx.caller();
    ctx.host().owner_say(caller, &text);
    Ok(())
}

/// `llInstantMessage`: send `text` to the avatar `user`, wherever it is, cut
/// to 1023 bytes. `NULL_KEY` and a string that is no key are refused in each
/// grid's words; a key that is nobody's is nothing. The two seconds it costs
/// are charged either way.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_instant_message(
    ctx: &mut ScriptCtx<'_>,
    user: Key,
    text: String,
) -> Result<(), CallError> {
    let opensim = ctx.flavour().is_opensim();
    // OpenSim takes any form `UUID.TryParse` reads, and refuses the zero key
    // in each of them.
    let valid = if opensim {
        dotnet::is_uuid(&user.0) && user.0.chars().any(|c| c.is_ascii_hexdigit() && c != '0')
    } else {
        is_somebody(&user.0)
    };
    if !valid {
        let refusal = if opensim {
            "llInstantMessage: An invalid key  was passed to llInstantMessage"
        } else {
            "Invalid Key passed to instant message."
        };
        ctx.refuse(refusal.to_owned());
        return Ok(());
    }
    let message = InstantMessage {
        agent: user.0,
        text: cut_bytes(&text, INSTANT_MESSAGE_BYTES).to_owned(),
    };
    let caller = ctx.caller();
    ctx.host().instant_message(caller, &message);
    Ok(())
}

/// `llListen`: hear lines on `channel` from `name` (anyone when empty), whose
/// key is `id` (anyone when `NULL_KEY`), saying `message` (anything when
/// empty); the listen's handle. A filter the script already listens with
/// answers the same handle.
///
/// # Errors
///
/// [`CallError::TooManyListens`] on Second Life when the script holds 65
/// already, which stops it; OpenSim answers `-1` past 64 and the script runs
/// on.
pub fn ll_listen(
    ctx: &mut ScriptCtx<'_>,
    channel: i32,
    name: String,
    id: Key,
    message: String,
) -> Result<i32, CallError> {
    let flavour = ctx.flavour();
    ctx.listens()
        .open(ListenFilter::new(channel, name, &id.0, message), flavour)
        .map_err(|TooManyListens| CallError::TooManyListens)
}

/// `llListenRemove`: close the listen `handle`; one the script does not hold
/// is nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_listen_remove(ctx: &mut ScriptCtx<'_>, handle: i32) -> Result<(), CallError> {
    ctx.listens().remove(handle);
    Ok(())
}

/// `llListenControl`: turn the listen `handle` off (`FALSE`) or back on.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_listen_control(
    ctx: &mut ScriptCtx<'_>,
    handle: i32,
    active: i32,
) -> Result<(), CallError> {
    ctx.listens().control(handle, active != 0);
    Ok(())
}

/// Hand `dialog` to the host, unless it is for nobody.
fn put(ctx: &mut ScriptCtx<'_>, dialog: Dialog) {
    if is_somebody(&dialog.agent) {
        let caller = ctx.caller();
        ctx.host().dialog(caller, &dialog);
    }
}

/// `llDialog`: put `message` with `buttons` to the avatar `avatar`, whose
/// press is said back on `channel`.
///
/// Second Life refuses, showing nothing: more than twelve buttons, an empty
/// message, one of 512 characters or more, a button that is not a string, an
/// empty label and one of more than 24 characters. OpenSim refuses more than
/// twelve buttons and an empty label; it complains of an empty message and
/// one of more than 512 bytes but shows the dialog, the message cut to 511
/// bytes and every label to 23, and shows a button that is not a string as
/// its text. No buttons is one, `OK` on Second Life and `Ok` on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_dialog(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: String,
    buttons: Vec<Element>,
    channel: i32,
) -> Result<(), CallError> {
    let dialog = if ctx.flavour().is_opensim() {
        opensim_dialog(ctx, avatar, &message, buttons, channel)
    } else {
        second_life_dialog(ctx, avatar, message, buttons, channel)
    };
    if let Some(dialog) = dialog {
        put(ctx, dialog);
    }
    Ok(())
}

/// Second Life's `llDialog`: the dialog, or [`None`] with the refusal said.
fn second_life_dialog(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: String,
    buttons: Vec<Element>,
    channel: i32,
) -> Option<Dialog> {
    let refusal = if buttons.len() > MAX_BUTTONS {
        Some("button list too long, must be 12 or fewer entries")
    } else if message.is_empty() {
        Some("must supply a message")
    } else if message.chars().count() >= SECOND_LIFE_MESSAGE_CHARACTERS {
        Some("message too long, must be less than 512 characters")
    } else if buttons
        .iter()
        .any(|button| !matches!(button, Element::String(_) | Element::Key(_)))
    {
        Some("button list must contain only strings")
    } else if buttons
        .iter()
        .any(|button| element_to_string(button).is_empty())
    {
        Some("all buttons must have label strings")
    } else if buttons
        .iter()
        .any(|button| element_to_string(button).chars().count() > SECOND_LIFE_LABEL_CHARACTERS)
    {
        Some("button labels must be 24 or fewer characters long")
    } else {
        None
    };
    if let Some(refusal) = refusal {
        ctx.refuse(format!("llDialog: {refusal}"));
        return None;
    }
    let buttons = if buttons.is_empty() {
        vec!["OK".to_owned()]
    } else {
        buttons.iter().map(element_to_string).collect()
    };
    Some(Dialog {
        agent: avatar.0,
        message,
        buttons,
        channel,
    })
}

/// OpenSim's `llDialog` (`LSL_Api.llDialog`, then the `ScriptDialog`'s
/// fields): the dialog, or [`None`] with the refusal said.
fn opensim_dialog(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: &str,
    buttons: Vec<Element>,
    channel: i32,
) -> Option<Dialog> {
    if !dotnet::is_uuid(&avatar.0) {
        ctx.refuse("llDialog: First parameter must be a key".to_owned());
        return None;
    }
    if buttons.len() > MAX_BUTTONS {
        ctx.refuse("llDialog: No more than 12 buttons can be shown".to_owned());
        return None;
    }
    if message.is_empty() {
        ctx.refuse("llDialog: Empty message".to_owned());
    } else if message.len() > OPENSIM_MESSAGE_BYTES {
        ctx.refuse("llDialog: Message longer than 512 bytes".to_owned());
    }
    let labels: Vec<String> = if buttons.is_empty() {
        vec!["Ok".to_owned()]
    } else {
        buttons.iter().map(element_to_string).collect()
    };
    if labels.iter().any(String::is_empty) {
        ctx.refuse("llDialog: Button label cannot be blank".to_owned());
        return None;
    }
    Some(Dialog {
        agent: avatar.0,
        message: cut_bytes(message, OPENSIM_SHOWN_MESSAGE_BYTES).to_owned(),
        buttons: labels
            .iter()
            .map(|label| cut_bytes(label, OPENSIM_LABEL_BYTES).to_owned())
            .collect(),
        channel,
    })
}

/// `llTextBox`: put `message` with a text field to the avatar `avatar`,
/// whose text is said back on `channel` — a dialog with the one button
/// [`TEXT_BOX_BUTTON`]. An empty message and a long one (512 characters or
/// more on Second Life, more than 512 bytes on OpenSim) are refused in each
/// grid's words, and nothing is shown.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_text_box(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: String,
    channel: i32,
) -> Result<(), CallError> {
    let opensim = ctx.flavour().is_opensim();
    let refusal = match (opensim, message.is_empty()) {
        (false, true) => Some("must supply a message"),
        (true, true) => Some("Empty message"),
        (false, false) if message.chars().count() >= SECOND_LIFE_MESSAGE_CHARACTERS => {
            Some("message too long, must be less than 512 characters")
        }
        (true, false) if message.len() > OPENSIM_MESSAGE_BYTES => {
            Some("Message longer than 512 bytes")
        }
        (true | false, false) => None,
    };
    if opensim && !dotnet::is_uuid(&avatar.0) {
        ctx.refuse("llTextBox: First parameter must be a key".to_owned());
        return Ok(());
    }
    if let Some(refusal) = refusal {
        ctx.refuse(format!("llTextBox: {refusal}"));
        return Ok(());
    }
    put(
        ctx,
        Dialog {
            agent: avatar.0,
            message,
            buttons: vec![TEXT_BOX_BUTTON.to_owned()],
            channel,
        },
    );
    Ok(())
}

/// Whether OpenSim's `new Uri(url)` takes `url`, and with which scheme:
/// [`None`] for a string it throws on (no scheme, or `http`/`https` without
/// a host).
fn uri_scheme(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    let mut characters = scheme.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    if matches!(scheme.as_str(), "http" | "https") {
        let host = rest.strip_prefix("//")?;
        if host.split(['/', '?', '#']).next().is_none_or(str::is_empty) {
            return None;
        }
    }
    Some(scheme)
}

/// `llLoadURL`: offer the avatar `avatar` the page `url`, with `message`.
/// OpenSim refuses a URL that is not `http` or `https` (`Invalid url schema`)
/// or no URL at all (`Invalid url`); Second Life's check is not measured, so
/// its flavour passes every URL on for the viewer to judge.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_load_url(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: String,
    url: String,
) -> Result<(), CallError> {
    if ctx.flavour().is_opensim() {
        match uri_scheme(&url).as_deref() {
            Some("http" | "https") => {}
            Some(_) => {
                ctx.refuse("llLoadURL: Invalid url schema".to_owned());
                return Ok(());
            }
            None => {
                ctx.refuse("llLoadURL: Invalid url".to_owned());
                return Ok(());
            }
        }
    }
    if is_somebody(&avatar.0) {
        let offer = LoadUrl {
            agent: avatar.0,
            message,
            url,
        };
        let caller = ctx.caller();
        ctx.host().load_url(caller, &offer);
    }
    Ok(())
}

/// `llMapDestination`: open a world map at `position` in the region
/// `region`, for the avatar whose touch the script is handling or, outside a
/// touch, the one wearing the object; otherwise nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_map_destination(
    ctx: &mut ScriptCtx<'_>,
    region: String,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let agent = match ctx.detected(0) {
        Some(detected) => Some(detected.key.clone()),
        None => ctx.host().worn_by(caller),
    };
    if let Some(agent) = agent {
        let destination = MapDestination {
            agent,
            region,
            position,
            look_at,
        };
        ctx.host().map_destination(caller, &destination);
    }
    Ok(())
}

/// `llMessageLinked`: send `num`, `text` and `id` to the scripts of the
/// prims `link` names — a link number, or `LINK_SET`, `LINK_ALL_OTHERS`,
/// `LINK_ALL_CHILDREN`, `LINK_THIS` — as `link_message`, the sender's own
/// prim's scripts, itself included, when its prim is among them. The text
/// and the key arrive whole, the key whether or not it is one.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_message_linked(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    num: i32,
    text: String,
    id: Key,
) -> Result<(), CallError> {
    ctx.send_linked(LinkMessage {
        target: link,
        num,
        text,
        id: id.0,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_line_is_cut_to_each_grids_bytes_at_a_character() {
        let long = "l".repeat(1100);
        assert_eq!(line(EngineFlavour::SecondLife, &long).len(), 1024);
        assert_eq!(line(EngineFlavour::YEngine, &long).len(), 1023);
        let wide = "é".repeat(600);
        assert_eq!(line(EngineFlavour::SecondLife, &wide).len(), 1024);
        assert_eq!(line(EngineFlavour::XEngine, &wide).len(), 1022);
    }

    #[test]
    fn opensims_uri_takes_http_and_https_with_a_host() {
        assert_eq!(uri_scheme("https://example.com/").as_deref(), Some("https"));
        assert_eq!(uri_scheme("HTTP://example.com").as_deref(), Some("http"));
        assert_eq!(uri_scheme("ftp://example.com").as_deref(), Some("ftp"));
        assert_eq!(uri_scheme("mailto:someone").as_deref(), Some("mailto"));
        assert_eq!(uri_scheme("example.com"), None);
        assert_eq!(uri_scheme("http://"), None);
        assert_eq!(uri_scheme("1http://x"), None);
    }
}
