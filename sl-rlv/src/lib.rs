//! Pure decoder and restriction state machine for the Second Life / OpenSim
//! **RLV / RLVa** `@`-command chat protocol — the language a worn attachment
//! speaks to control the viewer.
//!
//! RLV is not a wire protocol: the carrier is ordinary **owner-say chat**
//! (`CHAT_TYPE_OWNER` on channel `0`) from an object the agent owns, which is
//! why it works on any grid with no server support. A message is an RLV command
//! line when it starts with `@` ([`RLV_PREFIX`]); the viewer swallows it so it
//! never reaches the chat log. The payload is a **comma-separated list** of
//! commands, each `behaviour[:option]=param`, lower-cased.
//!
//! The crate is three layers:
//!
//! - the **language decoder** turns a chat line into a typed [`RlvCommand`]
//!   stream — behaviour, optional option, and the classified [`RlvParam`] (add
//!   / remove / force / reply-channel / clear);
//! - the **restriction state machine** ([`RlvState`]) holds what those commands
//!   mean: which behaviours are in force, which object put each one there, and
//!   which exceptions poke holes in them. Every enforcement family asks it
//!   rather than re-deriving the answer at its own choke point;
//! - the **query layer** ([`RlvState::answer`]) builds the line a `@get*`
//!   question is answered with.
//!
//! The state machine also reports itself: `@notify` subscribers are told about
//! every change it sees, and the lines they are owed wait in
//! [`RlvState::take_notifications`] for a consumer that can chat.
//!
//! The query layer **answers questions**. `@getoutfit=2222` asks what the agent
//! is wearing and wants it shouted on channel 2222; [`RlvState::answer`] builds
//! that line, reading the state machine for the parts it knows (`@version*`,
//! `@getstatus`, `@getcommand`, the `@getcam_*` limits) and an
//! [`RlvQuerySource`] the consumer implements for the parts it cannot (what is
//! worn, what is shared, where the camera is). Every byte a script sees is
//! built here; only the facts come from outside.
//!
//! What the state machine deliberately does *not* do is obey anything. It never
//! detaches an attachment and never hides a name tag; those are the consumer's,
//! and a `=force` action comes back from [`RlvState::apply`] as
//! [`RlvOutcome::NotAStateChange`] for the consumer to dispatch. Like `sl-prim`
//! and `sl-anim` this is a pure crate — no Bevy, no I/O, no session — so a
//! headless RLV-compliant bot can use exactly this, and it is unit-testable to
//! the letter (the reference's own debug console feeds hand-typed commands
//! through the very same path).
//!
//! ```
//! use sl_rlv::{parse_chat_line, RlvBehaviour, RlvParam, RlvState};
//! use uuid::Uuid;
//!
//! let cmds = parse_chat_line("@detach=n,fly=n").unwrap();
//! assert_eq!(cmds.len(), 2);
//! let detach = cmds[0].as_ref().unwrap();
//! assert_eq!(detach.behaviour, RlvBehaviour::Detach);
//! assert_eq!(detach.param, RlvParam::Add);
//!
//! // A query names the channel its answer is chatted back on.
//! let version = &parse_chat_line("@version=2222").unwrap()[0];
//! assert_eq!(version.as_ref().unwrap().param, RlvParam::Reply { channel: 2222 });
//!
//! // The keyword alone does not identify a behaviour: `@tpto` is an action, so
//! // there is no `tpto` restriction to add.
//! let nonsense = &parse_chat_line("@tpto=n").unwrap()[0];
//! assert_eq!(nonsense.as_ref().unwrap().behaviour, RlvBehaviour::Unknown);
//!
//! // Two collars, one restriction: it stays in force until both let go.
//! let (collar, cuffs) = (Uuid::from_u128(1), Uuid::from_u128(2));
//! let mut state = RlvState::new();
//! for object in [collar, cuffs] {
//!     state.apply(object, parse_chat_line("@fly=n").unwrap()[0].as_ref().unwrap());
//! }
//! state.apply(collar, parse_chat_line("@fly=y").unwrap()[0].as_ref().unwrap());
//! assert!(state.has_behaviour(RlvBehaviour::Fly));
//!
//! // Taking the other one off is what finally lifts it.
//! state.clear_object(cuffs);
//! assert!(!state.has_behaviour(RlvBehaviour::Fly));
//!
//! // An object can ask to hear about all of that as it happens (`@notify`),
//! // and the lines it is owed wait until the consumer takes them to chat.
//! let watcher = Uuid::from_u128(3);
//! state.apply(watcher, parse_chat_line("@notify:2222=n").unwrap()[0].as_ref().unwrap());
//! state.apply(collar, parse_chat_line("@sendchat=n").unwrap()[0].as_ref().unwrap());
//! let owed = state.take_notifications();
//! assert_eq!(owed.last().unwrap().channel, 2222);
//! assert_eq!(owed.last().unwrap().message, "/sendchat=n");
//! ```
//!
//! The grammar, the classification and the state machine follow Firestorm's
//! `rlvhandler.cpp` / `rlvhelper.cpp` / `rlvdefines.h` (`ERlvBehaviour`,
//! `ERlvParamType`, `RLV_CMD_PREFIX`), reimplemented idiomatically rather than
//! copied. The channel-0 owner-say gating is the caller's job — this crate
//! decodes a line it is handed.

mod behaviour;
mod command;
mod modifier;
mod notify;
mod query;
mod restriction;
mod state;
mod version;

pub use behaviour::{
    RlvBehaviour, RlvBehaviourFlags, RlvEntry, RlvLocalModifier, RlvResolvedBehaviour, RlvValueType,
};
pub use command::{RLV_PREFIX, RlvCommand, RlvParam, RlvParamKind, RlvParseError};
pub use modifier::{
    DEFAULT_FIELD_OF_VIEW, FARTOUCH_DEFAULT, IMG_DEFAULT, RlvComparator, RlvModifier,
    RlvModifierState, RlvModifierValue, SITTP_DEFAULT, TPLOCAL_DEFAULT,
};
pub use notify::RlvNotification;
pub use query::{
    CHAT_CHANNEL_DEBUG, FOLDER_INVALID_CHAR, FOLDER_PREFIX_HIDDEN, MAX_CHAT_BYTES,
    OPTION_SEPARATOR, RlvAnswer, RlvAttachGroup, RlvAttachmentPoint, RlvFolderWear,
    RlvFolderWearChild, RlvFolderWearCounts, RlvImQuery, RlvNamesQuery, RlvPathTarget, RlvQuery,
    RlvQuerySource, RlvReply, RlvVersionNum, RlvWearableSlot, SHARED_ROOT_FOLDER, STATUS_SEPARATOR,
    is_valid_reply_channel, split_chat, truncate_chat,
};
pub use restriction::{RlvOptionArity, RlvOptionMeaning, RlvRestrictionRule};
pub use state::{
    RlvException, RlvExceptionCheck, RlvExceptionOption, RlvHeldCommand, RlvOutcome, RlvState,
    is_state_change,
};
pub use version::{
    RLV_VERSION, RLV_VERSION_COMPAT, RLVA_IMPL_ID, RLVA_VERSION, version_impl_num_reply,
    version_num_reply, version_reply,
};

/// Whether `line` is an RLV command line — i.e. begins with the `@`
/// ([`RLV_PREFIX`]).
///
/// This is only the prefix test. The reference additionally requires the chat
/// to be a channel-0 owner-say from an owned, non-temporary object; that
/// gating is the caller's responsibility.
#[must_use]
pub fn is_rlv_line(line: &str) -> bool {
    line.starts_with(RLV_PREFIX)
}

/// Decode a full owner-say chat line into its command fields.
///
/// Returns `None` if `line` is not an RLV line (no leading `@`). Otherwise the
/// `@` is stripped and the remainder is split on `,` into fields (empty fields
/// are dropped, as in the reference tokeniser); each field is decoded
/// independently, so one malformed command does not sink its neighbours — the
/// returned vector holds a [`Result`] per field in order.
#[must_use]
pub fn parse_chat_line(line: &str) -> Option<Vec<Result<RlvCommand, RlvParseError>>> {
    let payload = line.strip_prefix(RLV_PREFIX)?;
    Some(
        payload
            .split(',')
            .filter(|field| !field.is_empty())
            .map(RlvCommand::parse_field)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

    /// A boxed error so tests can use `?` on `Result` and `Option` instead of
    /// the disallowed `unwrap` / `expect` / indexing.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn detects_prefix() {
        assert!(is_rlv_line("@detach=n"));
        assert!(!is_rlv_line("hello world"));
        assert!(!is_rlv_line(" @detach=n"));
    }

    #[test]
    fn non_rlv_line_is_none() {
        assert_eq!(parse_chat_line("just chatting"), None);
    }

    #[test]
    fn add_and_remove_synonyms() -> Result<(), TestError> {
        assert_eq!(RlvCommand::parse_field("detach=n")?.param, RlvParam::Add);
        assert_eq!(RlvCommand::parse_field("detach=add")?.param, RlvParam::Add);
        assert_eq!(RlvCommand::parse_field("detach=y")?.param, RlvParam::Remove);
        assert_eq!(
            RlvCommand::parse_field("detach=rem")?.param,
            RlvParam::Remove
        );
        Ok(())
    }

    #[test]
    fn force_command_with_uuid_option() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("sit:a3f2c1d4-0000-4000-8000-000000000000=force")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Sit);
        assert_eq!(cmd.param, RlvParam::Force);
        assert_eq!(
            cmd.option.as_deref(),
            Some("a3f2c1d4-0000-4000-8000-000000000000")
        );
        Ok(())
    }

    #[test]
    fn reply_channel_is_typed() -> Result<(), TestError> {
        assert_eq!(
            RlvCommand::parse_field("version=2222")?.param,
            RlvParam::Reply { channel: 2222 }
        );
        assert_eq!(
            RlvCommand::parse_field("getoutfit=1234")?.param,
            RlvParam::Reply { channel: 1234 }
        );
        assert_eq!(
            RlvCommand::parse_field("getoutfit:gloves=1234")?
                .option
                .as_deref(),
            Some("gloves")
        );
        Ok(())
    }

    #[test]
    fn version_handshake_tokens() -> Result<(), TestError> {
        assert_eq!(
            RlvCommand::parse_field("versionnew=5")?.behaviour,
            RlvBehaviour::Versionnew
        );
        assert_eq!(
            RlvCommand::parse_field("versionnum=5")?.behaviour,
            RlvBehaviour::Versionnum
        );
        Ok(())
    }

    #[test]
    fn clear_bare_and_filtered() -> Result<(), TestError> {
        let bare = RlvCommand::parse_field("clear")?;
        assert_eq!(bare.behaviour, RlvBehaviour::Clear);
        assert_eq!(bare.param, RlvParam::Clear { filter: None });

        assert_eq!(
            RlvCommand::parse_field("clear=tp")?.param,
            RlvParam::Clear {
                filter: Some("tp".to_owned())
            }
        );

        // Trailing `=` is a bare clear.
        assert_eq!(
            RlvCommand::parse_field("clear=")?.param,
            RlvParam::Clear { filter: None }
        );
        Ok(())
    }

    #[test]
    fn clear_add_precedence_matches_reference() -> Result<(), TestError> {
        // `n` classifies as Add before the `clear` behaviour check, so the
        // param is Add — and the behaviour lookup then asks for a `clear`
        // *restriction*, which does not exist. The reference lands in exactly
        // the same place: its key is ("clear", RLV_TYPE_ADDREM), which is not
        // in `m_String2InfoMap`, so `@clear=n` is RLV_BHVR_UNKNOWN with param
        // type RLV_TYPE_ADD.
        let cmd = RlvCommand::parse_field("clear=n")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Unknown);
        assert_eq!(cmd.keyword, "clear");
        assert_eq!(cmd.param, RlvParam::Add);
        Ok(())
    }

    #[test]
    fn param_kind_gates_the_behaviour_lookup() -> Result<(), TestError> {
        // Force-only: `@tpto` teleports, there is no restriction by that name.
        assert_eq!(
            RlvCommand::parse_field("tpto:128/128/25=force")?.behaviour,
            RlvBehaviour::Tpto
        );
        let as_restriction = RlvCommand::parse_field("tpto=n")?;
        assert_eq!(as_restriction.behaviour, RlvBehaviour::Unknown);
        assert_eq!(as_restriction.keyword, "tpto");

        // Reply-only: `@version` answers on a channel, it does not restrict.
        assert_eq!(
            RlvCommand::parse_field("version=n")?.behaviour,
            RlvBehaviour::Unknown
        );

        // Restriction-only: `@showloc` blocks, it is not an action.
        assert_eq!(
            RlvCommand::parse_field("showloc=n")?.behaviour,
            RlvBehaviour::Showloc
        );
        assert_eq!(
            RlvCommand::parse_field("showloc=force")?.behaviour,
            RlvBehaviour::Unknown
        );

        // Declared for both: `@sit` is a restriction *and* an action.
        assert_eq!(
            RlvCommand::parse_field("sit=n")?.behaviour,
            RlvBehaviour::Sit
        );
        assert_eq!(
            RlvCommand::parse_field("sit=force")?.behaviour,
            RlvBehaviour::Sit
        );
        Ok(())
    }

    #[test]
    fn local_modifier_fallback() -> Result<(), TestError> {
        // `<behaviour>_<modifier>=force` addresses a modifier of the base
        // restriction, and reports as that base behaviour.
        let cmd = RlvCommand::parse_field("setsphere_mode:1=force")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Setsphere);
        assert_eq!(cmd.modifier, Some(RlvLocalModifier::SphereMode));
        assert_eq!(cmd.keyword, "setsphere_mode");
        assert_eq!(cmd.option.as_deref(), Some("1"));

        let alpha = RlvCommand::parse_field("setoverlay_alpha:0.5=force")?;
        assert_eq!(alpha.behaviour, RlvBehaviour::Setoverlay);
        assert_eq!(alpha.modifier, Some(RlvLocalModifier::OverlayAlpha));

        // A behaviour of its own is not a modifier command, even though it is
        // spelled the same way.
        let tween = RlvCommand::parse_field("setoverlay_tween:2=force")?;
        assert_eq!(tween.behaviour, RlvBehaviour::SetoverlayTween);
        assert_eq!(tween.modifier, None);

        // The fallback is `=force` only, and only for modifiers the base
        // behaviour actually declares.
        assert_eq!(
            RlvCommand::parse_field("setsphere_mode=n")?.behaviour,
            RlvBehaviour::Unknown
        );
        assert_eq!(
            RlvCommand::parse_field("setsphere_frobnicate=force")?.behaviour,
            RlvBehaviour::Unknown
        );
        // `tween` is a modifier of @setsphere, not of @setoverlay.
        assert_eq!(
            RlvCommand::parse_field("setoverlay_distmin=force")?.behaviour,
            RlvBehaviour::Unknown
        );
        // A base with no modifiers at all.
        assert_eq!(
            RlvCommand::parse_field("fly_mode=force")?.behaviour,
            RlvBehaviour::Unknown
        );
        // A strict keyword never takes the fallback.
        assert_eq!(
            RlvCommand::parse_field("setsphere_sec=force")?.behaviour,
            RlvBehaviour::Unknown
        );
        Ok(())
    }

    #[test]
    fn local_modifier_table_roundtrips() {
        for &modifier in RlvLocalModifier::ALL {
            assert_eq!(
                RlvLocalModifier::lookup(modifier.behaviour(), modifier.name()),
                Some(modifier),
                "{modifier:?} does not round-trip through its own base and name"
            );
            // A modifier hangs off a restriction, which is the only row the
            // reference registers modifiers on.
            assert!(
                modifier.behaviour().accepts(RlvParamKind::AddRem),
                "{modifier:?} hangs off a behaviour that is not a restriction"
            );
        }
    }

    #[test]
    fn strict_suffix() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("recvim_sec=n")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Recvim);
        assert!(cmd.strict);
        assert_eq!(cmd.keyword, "recvim_sec");

        // A non-strict behaviour with a `_sec` suffix is unknown, not strict.
        let bad = RlvCommand::parse_field("fly_sec=n")?;
        assert_eq!(bad.behaviour, RlvBehaviour::Unknown);
        assert!(!bad.strict);
        Ok(())
    }

    #[test]
    fn underscore_keywords_are_not_strict() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("sendchannel_except=n")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::SendchannelExcept);
        assert!(!cmd.strict);
        Ok(())
    }

    #[test]
    fn unknown_behaviour_keeps_keyword() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("frobnicate=n")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Unknown);
        assert_eq!(cmd.keyword, "frobnicate");
        assert_eq!(cmd.param, RlvParam::Add);
        Ok(())
    }

    #[test]
    fn case_is_folded() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("@DeTaCh=N")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Detach);
        assert_eq!(cmd.keyword, "detach");
        assert_eq!(cmd.param, RlvParam::Add);
        Ok(())
    }

    /// The classified behaviour of command `index` in a decoded line, or `None`
    /// if the index is out of range or that command failed to parse.
    fn behaviour_at(
        cmds: &[Result<RlvCommand, RlvParseError>],
        index: usize,
    ) -> Option<RlvBehaviour> {
        cmds.get(index)
            .and_then(|result| result.as_ref().ok())
            .map(|cmd| cmd.behaviour)
    }

    #[test]
    fn multiple_commands_and_empty_fields_dropped() -> Result<(), TestError> {
        let cmds = parse_chat_line("@detach=n,,fly=n,").ok_or("not an rlv line")?;
        assert_eq!(cmds.len(), 2);
        assert_eq!(behaviour_at(&cmds, 0), Some(RlvBehaviour::Detach));
        assert_eq!(behaviour_at(&cmds, 1), Some(RlvBehaviour::Fly));
        Ok(())
    }

    #[test]
    fn one_bad_command_does_not_sink_the_line() -> Result<(), TestError> {
        let cmds = parse_chat_line("@detach=n,garbage,fly=y").ok_or("not an rlv line")?;
        assert_eq!(cmds.len(), 3);
        assert_eq!(behaviour_at(&cmds, 0), Some(RlvBehaviour::Detach));
        assert_eq!(cmds.get(1), Some(&Err(RlvParseError::MissingParam)));
        assert_eq!(behaviour_at(&cmds, 2), Some(RlvBehaviour::Fly));
        Ok(())
    }

    #[test]
    fn error_cases() {
        assert_eq!(
            RlvCommand::parse_field("=n"),
            Err(RlvParseError::EmptyBehaviour)
        );
        assert_eq!(
            RlvCommand::parse_field(":opt=n"),
            Err(RlvParseError::EmptyBehaviour)
        );
        assert_eq!(
            RlvCommand::parse_field("fly"),
            Err(RlvParseError::MissingParam)
        );
        assert_eq!(
            RlvCommand::parse_field("fly="),
            Err(RlvParseError::MissingParam)
        );
        assert_eq!(
            RlvCommand::parse_field("detach=bogus"),
            Err(RlvParseError::UnknownParam("bogus".to_owned()))
        );
    }

    #[test]
    fn colon_without_equals_is_part_of_behaviour() {
        // Without a `=`, no option is parsed — the whole field is the behaviour
        // (and so it is a missing-param error), matching the reference.
        assert_eq!(
            RlvCommand::parse_field("clear:tp"),
            Err(RlvParseError::MissingParam)
        );
    }

    #[test]
    fn empty_option_is_none() -> Result<(), TestError> {
        let cmd = RlvCommand::parse_field("detach:=n")?;
        assert_eq!(cmd.option, None);
        assert_eq!(cmd.behaviour, RlvBehaviour::Detach);
        Ok(())
    }

    #[test]
    fn negative_reply_channel() -> Result<(), TestError> {
        assert_eq!(
            RlvCommand::parse_field("getstatus=-1")?.param,
            RlvParam::Reply { channel: -1 }
        );
        Ok(())
    }

    #[test]
    fn behaviour_keyword_roundtrip() {
        assert_eq!(
            RlvBehaviour::Detach.canonical_keyword(RlvParamKind::AddRem),
            Some("detach")
        );
        assert_eq!(
            RlvBehaviour::Unknown.canonical_keyword(RlvParamKind::AddRem),
            None
        );
        assert_eq!(
            RlvBehaviour::from_keyword("detach", RlvParamKind::AddRem),
            Some(RlvBehaviour::Detach)
        );
        assert_eq!(
            RlvBehaviour::from_keyword("nope", RlvParamKind::AddRem),
            None
        );
        assert!(RlvBehaviour::Recvim.has_strict());
        assert!(!RlvBehaviour::Fly.has_strict());
    }

    #[test]
    fn a_synonym_names_the_behaviour_it_is_a_synonym_of() -> Result<(), TestError> {
        // The whole point of the keyword/behaviour split: two spellings, one
        // reference-counting slot.
        assert_eq!(
            RlvCommand::parse_field("touchfar=n")?.behaviour,
            RlvBehaviour::Fartouch
        );
        assert_eq!(
            RlvCommand::parse_field("fartouch=n")?.behaviour,
            RlvBehaviour::Fartouch
        );
        // ... but the spelling that arrived is still there.
        assert_eq!(RlvCommand::parse_field("touchfar=n")?.keyword, "touchfar");
        // A synonym never answers as the canonical spelling.
        assert_eq!(
            RlvBehaviour::Fartouch.canonical_keyword(RlvParamKind::AddRem),
            Some("fartouch")
        );

        // The deprecated camera shims fold onto the modern behaviours.
        assert_eq!(
            RlvCommand::parse_field("camdistmin:2=n")?.behaviour,
            RlvBehaviour::SetcamAvdistmin
        );
        assert_eq!(
            RlvCommand::parse_field("camunlock=n")?.behaviour,
            RlvBehaviour::SetcamUnlock
        );
        // `@camzoommin` is *not* one of them: the reference gives it a
        // behaviour of its own that merely counts as `@setcam_fovmin`.
        assert_eq!(
            RlvCommand::parse_field("camzoommin=n")?.behaviour,
            RlvBehaviour::Camzoommin
        );

        // Every force-wear spelling is one behaviour, as the reference's
        // `RLV_CMD_FORCEWEAR` is.
        for keyword in [
            "attach",
            "attachall",
            "addoutfit",
            "attachthisoverorreplace",
        ] {
            assert_eq!(
                RlvCommand::parse_field(&format!("{keyword}:x=force"))?.behaviour,
                RlvBehaviour::ForceWear,
                "`{keyword}=force` is not a force-wear command"
            );
        }
        // `@addoutfit` is a force-wear synonym but a restriction of its own.
        assert_eq!(
            RlvCommand::parse_field("addoutfit=n")?.behaviour,
            RlvBehaviour::Addoutfit
        );
        Ok(())
    }

    #[test]
    fn behaviour_flags_are_carried_for_getcommand() -> Result<(), TestError> {
        let deprecated =
            RlvEntry::lookup("camzoommin", RlvParamKind::AddRem).ok_or("no `camzoommin` row")?;
        assert!(deprecated.flags.is_deprecated());
        assert!(!deprecated.flags.is_synonym());

        let shim =
            RlvEntry::lookup("camtextures", RlvParamKind::AddRem).ok_or("no `camtextures` row")?;
        assert!(shim.flags.is_deprecated() && shim.flags.is_synonym());

        let experimental =
            RlvEntry::lookup("shownearby", RlvParamKind::AddRem).ok_or("no `shownearby` row")?;
        assert!(experimental.flags.is_experimental());

        let extended =
            RlvEntry::lookup("interact", RlvParamKind::AddRem).ok_or("no `interact` row")?;
        assert!(extended.flags.is_extended());

        assert_eq!(format!("{:?}", RlvBehaviourFlags::NONE), "NONE");
        assert_eq!(
            format!("{:?}", shim.flags),
            "SYNONYM|DEPRECATED",
            "the debug form is what a `@getcommand` audit reads"
        );
        Ok(())
    }

    /// Every param kind, so a table row can be probed for the kinds it does
    /// *not* declare as well as the ones it does.
    const ALL_PARAM_KINDS: [RlvParamKind; 4] = [
        RlvParamKind::AddRem,
        RlvParamKind::Force,
        RlvParamKind::Reply,
        RlvParamKind::Clear,
    ];

    /// A command field that hands `keyword` a param of `kind`, or `None` when
    /// no such field exists.
    ///
    /// Two kinds are unreachable for some keywords, both because of the `clear`
    /// special case in the param classifier: a param-less field is a syntax
    /// error for anything but `@clear`, and conversely `@clear=force` /
    /// `@clear=1234` are *filtered clears*, not a force or a reply.
    fn field_for(keyword: &str, kind: RlvParamKind) -> Option<String> {
        match kind {
            RlvParamKind::AddRem => Some(format!("{keyword}=n")),
            RlvParamKind::Force => (keyword != "clear").then(|| format!("{keyword}=force")),
            RlvParamKind::Reply => (keyword != "clear").then(|| format!("{keyword}=1234")),
            RlvParamKind::Clear => (keyword == "clear").then(|| keyword.to_owned()),
        }
    }

    #[test]
    fn every_dictionary_row_is_unique_and_looks_itself_up() {
        let mut seen: std::collections::HashSet<(&str, RlvParamKind)> =
            std::collections::HashSet::new();

        for entry in RlvEntry::ALL {
            assert_ne!(
                entry.behaviour,
                RlvBehaviour::Unknown,
                "`{}` names no behaviour",
                entry.keyword
            );
            assert_eq!(
                RlvEntry::lookup(entry.keyword, entry.kind),
                Some(entry),
                "`{}` for {:?} does not look itself up",
                entry.keyword,
                entry.kind
            );
            assert!(
                seen.insert((entry.keyword, entry.kind)),
                "`{}` is declared twice for {:?}",
                entry.keyword,
                entry.kind
            );
        }
    }

    #[test]
    fn every_behaviour_is_reachable_and_answers_its_own_kinds() -> Result<(), TestError> {
        for &behaviour in RlvBehaviour::ALL {
            let reached = RlvEntry::ALL
                .iter()
                .any(|entry| entry.behaviour == behaviour);
            assert!(
                reached,
                "{behaviour:?} has no dictionary row, so no command can ever name it"
            );
            for kind in ALL_PARAM_KINDS {
                assert_eq!(
                    behaviour.accepts(kind),
                    RlvEntry::canonical(behaviour, kind).is_some(),
                    "{behaviour:?} disagrees with the dictionary about {kind:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn every_dictionary_row_answers_only_its_own_param_kind() -> Result<(), TestError> {
        for entry in RlvEntry::ALL {
            let keyword = entry.keyword;

            // A bare keyword is a syntax error for everything but `@clear`.
            if keyword != "clear" {
                assert_eq!(
                    RlvCommand::parse_field(keyword),
                    Err(RlvParseError::MissingParam),
                    "a bare `{keyword}` should not decode"
                );
            }

            for kind in ALL_PARAM_KINDS {
                let Some(field) = field_for(keyword, kind) else {
                    continue;
                };
                let cmd = RlvCommand::parse_field(&field)?;
                assert_eq!(cmd.keyword, keyword, "`{field}` lost its keyword");
                assert_eq!(
                    cmd.param.kind(),
                    kind,
                    "`{field}` classified as another kind"
                );

                let expected = RlvEntry::lookup(keyword, kind)
                    .map_or(RlvBehaviour::Unknown, |row| row.behaviour);
                assert_eq!(
                    cmd.behaviour, expected,
                    "`{field}` resolved to {:?}, expected {expected:?}",
                    cmd.behaviour
                );
                assert_eq!(cmd.modifier, None, "`{field}` is not a modifier command");
            }
        }
        Ok(())
    }

    #[test]
    fn every_dictionary_row_answers_the_strict_suffix_it_declares() -> Result<(), TestError> {
        for entry in RlvEntry::ALL {
            if entry.kind != RlvParamKind::AddRem {
                continue;
            }
            let keyword = entry.keyword;
            let field = format!("{keyword}_sec=n");
            let cmd = RlvCommand::parse_field(&field)?;

            let strict_ok = entry.flags.is_strict();
            assert_eq!(
                cmd.strict, strict_ok,
                "`{field}` reported strict={}, expected {strict_ok}",
                cmd.strict
            );
            assert_eq!(
                cmd.behaviour,
                if strict_ok {
                    entry.behaviour
                } else {
                    RlvBehaviour::Unknown
                },
                "`{field}` resolved to {:?}",
                cmd.behaviour
            );
            assert_eq!(cmd.keyword, format!("{keyword}_sec"));
        }
        Ok(())
    }

    #[test]
    fn every_restriction_row_has_a_rule_and_only_restrictions_do() {
        for &behaviour in RlvBehaviour::ALL {
            assert_eq!(
                behaviour.restriction_rule().is_some(),
                behaviour.is_restriction(),
                "{behaviour:?} disagrees with itself about being a restriction"
            );
        }
        assert_eq!(RlvBehaviour::Unknown.restriction_rule(), None);
    }

    #[test]
    fn every_modifier_hangs_off_a_restriction_that_can_reach_it() {
        for &modifier in RlvModifier::ALL {
            assert!(
                modifier.behaviour().is_restriction(),
                "{modifier:?} hangs off a behaviour that cannot be held"
            );
            let resolved = RlvModifier::of_behaviour(modifier.behaviour());
            assert_eq!(
                resolved.map(RlvModifier::behaviour),
                Some(modifier.behaviour()),
                "{modifier:?}'s behaviour resolves to a slot that is not its own"
            );
            assert_eq!(
                modifier.default_value().value_type(),
                modifier.value_type(),
                "{modifier:?} has a default of the wrong type"
            );
        }
        assert_eq!(
            RlvModifier::ALL.len(),
            21,
            "the reference declares 21 slots"
        );

        // A behaviour may own two slots — the IM families own a minimum *and*
        // a maximum distance — and the reference's single-valued
        // behaviour-to-modifier map answers with whichever was declared first.
        assert_eq!(
            RlvModifier::of_behaviour(RlvBehaviour::Recvim),
            Some(RlvModifier::RecvImDistMin)
        );
    }

    #[test]
    fn hostile_owner_say_lines_are_decoded_without_panicking() {
        // This crate's input is chat from an in-world object, so a malformed
        // line has to come back as errors, never as a panic or a hang.
        let long_option = "x".repeat(8192);
        let many_commas = ",".repeat(4096);
        let lines = [
            "@".to_owned(),
            "@,".to_owned(),
            many_commas.clone(),
            format!("@{many_commas}"),
            format!("@detach=n{many_commas}fly=n"),
            "@:::=:::".to_owned(),
            "@====".to_owned(),
            "@=".to_owned(),
            "@:".to_owned(),
            "@_sec=n".to_owned(),
            "@_=force".to_owned(),
            "@setsphere_=force".to_owned(),
            "@sit:=force".to_owned(),
            format!("@detach:{long_option}=n"),
            format!("@{long_option}=n"),
            format!("@{}=n", "a_".repeat(2048)),
            "@version=99999999999999999999".to_owned(),
            "@version=-99999999999999999999".to_owned(),
            "@version=+2222".to_owned(),
            "@version= 2222".to_owned(),
            // Non-ASCII: the decoder lower-cases ASCII only, as the reference
            // does, and must not split a multi-byte character.
            "@ÜBERdetach=n".to_owned(),
            "@detach:🔒=n".to_owned(),
            "@\u{0}detach=n".to_owned(),
            "@detach=n\u{0}".to_owned(),
        ];

        for line in &lines {
            let Some(cmds) = parse_chat_line(line) else {
                assert!(!line.starts_with(RLV_PREFIX), "`{line}` should have parsed");
                continue;
            };
            for cmd in cmds.iter().flatten() {
                // Whatever survives is internally consistent: an unknown
                // keyword is never reported as strict or as a modifier.
                if cmd.behaviour == RlvBehaviour::Unknown {
                    assert!(!cmd.strict, "`{line}` reported an unknown strict behaviour");
                    assert_eq!(cmd.modifier, None, "`{line}` reported a bare modifier");
                }
                assert_ne!(
                    cmd.option.as_deref(),
                    Some(""),
                    "`{line}` kept an empty option"
                );
            }
        }
    }
}
