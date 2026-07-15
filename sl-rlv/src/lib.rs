//! Pure decoder for the Second Life / OpenSim **RLV / RLVa** `@`-command chat
//! protocol — the language a worn attachment speaks to control the viewer.
//!
//! RLV is not a wire protocol: the carrier is ordinary **owner-say chat**
//! (`CHAT_TYPE_OWNER` on channel `0`) from an object the agent owns, which is
//! why it works on any grid with no server support. A message is an RLV command
//! line when it starts with `@` ([`RLV_PREFIX`]); the viewer swallows it so it
//! never reaches the chat log. The payload is a **comma-separated list** of
//! commands, each `behaviour[:option]=param`, lower-cased.
//!
//! This crate is the **language decoder** only: it turns a chat line into a
//! typed [`RlvCommand`] stream — behaviour, optional option, and the classified
//! [`RlvParam`] (add / remove / force / reply-channel / clear). *Obeying* the
//! commands (the restriction state and the enforcement families) is a separate
//! concern that builds on this. Like `sl-prim` and `sl-anim` it is a pure
//! crate — no Bevy, no I/O, no session — so a headless RLV-compliant client can
//! use exactly this, and it is unit-testable to the letter (the reference's own
//! debug console feeds hand-typed commands through the very same path).
//!
//! ```
//! use sl_rlv::{parse_chat_line, RlvBehaviour, RlvParam};
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
//! ```
//!
//! The grammar and classification follow Firestorm's `rlvhandler.cpp` /
//! `rlvhelper.cpp` / `rlvdefines.h` (`ERlvBehaviour`, `ERlvParamType`,
//! `RLV_CMD_PREFIX`), reimplemented idiomatically rather than copied. The
//! channel-0 owner-say gating is the caller's job — this crate decodes a line
//! it is handed.

mod behaviour;
mod command;

pub use behaviour::RlvBehaviour;
pub use command::{RLV_PREFIX, RlvCommand, RlvParam, RlvParseError};

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
    use pretty_assertions::assert_eq;

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
        // `n` classifies as Add before the `clear` behaviour check, even though
        // the behaviour is still Clear.
        let cmd = RlvCommand::parse_field("clear=n")?;
        assert_eq!(cmd.behaviour, RlvBehaviour::Clear);
        assert_eq!(cmd.param, RlvParam::Add);
        Ok(())
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
        assert_eq!(RlvBehaviour::Detach.keyword(), Some("detach"));
        assert_eq!(RlvBehaviour::Unknown.keyword(), None);
        assert_eq!(
            RlvBehaviour::from_keyword("detach"),
            Some(RlvBehaviour::Detach)
        );
        assert_eq!(RlvBehaviour::from_keyword("nope"), None);
        assert!(RlvBehaviour::Recvim.has_strict());
        assert!(!RlvBehaviour::Fly.has_strict());
    }
}
