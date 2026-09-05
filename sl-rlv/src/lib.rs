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
//!
//! // The keyword alone does not identify a behaviour: `@tpto` is an action, so
//! // there is no `tpto` restriction to add.
//! let nonsense = &parse_chat_line("@tpto=n").unwrap()[0];
//! assert_eq!(nonsense.as_ref().unwrap().behaviour, RlvBehaviour::Unknown);
//! ```
//!
//! The grammar and classification follow Firestorm's `rlvhandler.cpp` /
//! `rlvhelper.cpp` / `rlvdefines.h` (`ERlvBehaviour`, `ERlvParamType`,
//! `RLV_CMD_PREFIX`), reimplemented idiomatically rather than copied. The
//! channel-0 owner-say gating is the caller's job — this crate decodes a line
//! it is handed.

mod behaviour;
mod command;

pub use behaviour::{RlvBehaviour, RlvLocalModifier, RlvResolvedBehaviour};
pub use command::{RLV_PREFIX, RlvCommand, RlvParam, RlvParamKind, RlvParseError};

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
    fn every_table_row_roundtrips() -> Result<(), TestError> {
        let mut seen: std::collections::HashMap<&str, RlvBehaviour> =
            std::collections::HashMap::new();

        for &behaviour in RlvBehaviour::ALL {
            let keyword = behaviour.keyword().ok_or("a declared row has no keyword")?;

            assert_eq!(
                RlvBehaviour::from_keyword(keyword),
                Some(behaviour),
                "`{keyword}` does not look up to {behaviour:?}"
            );
            assert_eq!(
                seen.insert(keyword, behaviour),
                None,
                "`{keyword}` is declared twice"
            );

            let kinds = behaviour.param_kinds();
            assert!(
                !kinds.is_empty(),
                "{behaviour:?} is declared for no param kind, so it can never resolve"
            );
            for (index, kind) in kinds.iter().enumerate() {
                assert!(
                    !kinds
                        .iter()
                        .skip(index.saturating_add(1))
                        .any(|it| it == kind),
                    "{behaviour:?} lists {kind:?} twice"
                );
            }
        }

        assert_eq!(
            seen.len(),
            RlvBehaviour::ALL.len(),
            "the keyword set is smaller than the table"
        );
        Ok(())
    }

    #[test]
    fn every_table_row_answers_only_its_own_param_kinds() -> Result<(), TestError> {
        for &behaviour in RlvBehaviour::ALL {
            let keyword = behaviour.keyword().ok_or("a declared row has no keyword")?;

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

                let expected = if behaviour.accepts(kind) {
                    behaviour
                } else {
                    RlvBehaviour::Unknown
                };
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
    fn every_table_row_answers_the_strict_suffix_it_declares() -> Result<(), TestError> {
        for &behaviour in RlvBehaviour::ALL {
            let keyword = behaviour.keyword().ok_or("a declared row has no keyword")?;
            let field = format!("{keyword}_sec=n");
            let cmd = RlvCommand::parse_field(&field)?;

            // A strict keyword is only a keyword at all when the behaviour
            // declares strict mode *and* is a restriction to begin with.
            let strict_ok = behaviour.has_strict() && behaviour.accepts(RlvParamKind::AddRem);
            assert_eq!(
                cmd.strict, strict_ok,
                "`{field}` reported strict={}, expected {strict_ok}",
                cmd.strict
            );
            assert_eq!(
                cmd.behaviour,
                if strict_ok {
                    behaviour
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
