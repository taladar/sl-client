//! The shared command-line surface for the optional local chat-log feature.
//!
//! Both REPL binaries (`sl-repl-tokio` / `sl-repl-bevy`) flatten [`ChatLogArgs`]
//! into their `RunArgs` so the chat-log toggles are exposed identically, and turn
//! it into a [`ChatLogConfig`] with [`ChatLogArgs::to_config`] to hand to the
//! runtime. Everything is **off by default**, mirroring the underlying config.

use sl_proto::{ChatLogConfig, LoggedChatType, TimestampFormat};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The chat-log command-line toggles, flattened into each REPL's argument parser.
/// All flags are off by default (so the feature stays disabled unless asked for),
/// except seconds-in-timestamps which is on unless `--chat-log-no-seconds` is given.
#[derive(clap::Args, Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line on/off flag, not packed state"
)]
pub struct ChatLogArgs {
    /// Log region-local nearby chat to `chat.txt`.
    #[clap(long)]
    chat_log_nearby: bool,
    /// Log 1:1 instant messages to `<name>.txt`.
    #[clap(long)]
    chat_log_im: bool,
    /// Log group session messages to `<group> (group).txt`.
    #[clap(long)]
    chat_log_group: bool,
    /// Log ad-hoc conference messages to `Ad-hoc Conference hash<md5>.txt`.
    #[clap(long)]
    chat_log_conference: bool,
    /// Directory to write transcripts directly under. Unset disables chat-log file
    /// output (there is no built-in default directory).
    #[clap(long)]
    chat_log_dir: Option<PathBuf>,
    /// Use the legacy `firstname.lastname` IM filename scheme.
    #[clap(long)]
    chat_log_legacy_names: bool,
    /// Append a date suffix to transcript filenames.
    #[clap(long)]
    chat_log_date_suffix: bool,
    /// Omit seconds from log timestamps (seconds are included by default).
    #[clap(long)]
    chat_log_no_seconds: bool,
    /// Maintain the per-account `conversation.log` index.
    #[clap(long)]
    conversation_log: bool,
}

impl ChatLogArgs {
    /// The directory transcripts should be written under (`--chat-log-dir`), or
    /// `None` to disable chat-log file output. Threaded into the runtime via
    /// [`ClientDirectories::agent_chat_log_dir`](sl_proto::ClientDirectories), no
    /// longer through [`ChatLogConfig`].
    #[must_use]
    pub fn chat_log_dir(&self) -> Option<PathBuf> {
        self.chat_log_dir.clone()
    }

    /// Builds the [`ChatLogConfig`] these flags describe, layered over the config's
    /// own defaults (so the unset format knobs keep their Firestorm defaults).
    #[must_use]
    pub fn to_config(&self) -> ChatLogConfig {
        let mut enabled = BTreeSet::new();
        if self.chat_log_nearby {
            enabled.insert(LoggedChatType::Nearby);
        }
        if self.chat_log_im {
            enabled.insert(LoggedChatType::InstantMessage);
        }
        if self.chat_log_group {
            enabled.insert(LoggedChatType::Group);
        }
        if self.chat_log_conference {
            enabled.insert(LoggedChatType::Conference);
        }
        let defaults = ChatLogConfig::default();
        let timestamp = defaults.timestamp.map(|format| TimestampFormat {
            seconds: !self.chat_log_no_seconds,
            ..format
        });
        ChatLogConfig {
            enabled,
            legacy_im_names: self.chat_log_legacy_names,
            date_suffix: self.chat_log_date_suffix,
            timestamp,
            conversation_log: self.conversation_log,
            ..defaults
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use pretty_assertions::assert_eq;
    use sl_proto::LoggedChatType;

    use super::ChatLogArgs;

    /// A parser for [`ChatLogArgs`] alone, standing in for the `RunArgs` each
    /// binary flattens it into.
    #[derive(clap::Parser, Debug)]
    struct Harness {
        /// The flattened chat-log flags under test.
        #[clap(flatten)]
        chat_log: ChatLogArgs,
    }

    /// Parse `flags` (without a program name) into the chat-log arguments.
    fn parse(flags: &[&str]) -> ChatLogArgs {
        let mut argv = vec!["harness"];
        argv.extend_from_slice(flags);
        Harness::parse_from(argv).chat_log
    }

    #[test]
    fn the_feature_is_off_unless_a_kind_is_asked_for() {
        let config = parse(&[]).to_config();
        assert!(
            !config.any_enabled(),
            "no --chat-log-* flag should leave logging fully off"
        );
        assert_eq!(parse(&[]).chat_log_dir(), None);
    }

    #[test]
    fn each_flag_enables_exactly_its_own_kind() {
        for (flag, kind) in [
            ("--chat-log-nearby", LoggedChatType::Nearby),
            ("--chat-log-im", LoggedChatType::InstantMessage),
            ("--chat-log-group", LoggedChatType::Group),
            ("--chat-log-conference", LoggedChatType::Conference),
        ] {
            let enabled = parse(&[flag]).to_config().enabled;
            assert_eq!(
                enabled.iter().copied().collect::<Vec<_>>(),
                vec![kind],
                "{flag} should enable {kind:?} and nothing else"
            );
        }
    }

    #[test]
    fn seconds_are_on_until_they_are_turned_off() {
        assert_eq!(
            parse(&[])
                .to_config()
                .timestamp
                .map(|format| format.seconds),
            Some(true),
            "seconds are included by default"
        );
        assert_eq!(
            parse(&["--chat-log-no-seconds"])
                .to_config()
                .timestamp
                .map(|format| format.seconds),
            Some(false)
        );
    }

    #[test]
    fn the_format_knobs_left_alone_keep_their_defaults() {
        let defaults = sl_proto::ChatLogConfig::default();
        let config = parse(&["--chat-log-nearby"]).to_config();
        assert_eq!(config.recall_window, defaults.recall_window);
        assert_eq!(
            config.conversation_log_retention_days,
            defaults.conversation_log_retention_days
        );
        assert!(!config.legacy_im_names);
        assert!(!config.date_suffix);
        assert!(!config.conversation_log);
    }

    #[test]
    fn the_remaining_flags_each_set_their_own_field() {
        let config = parse(&[
            "--chat-log-legacy-names",
            "--chat-log-date-suffix",
            "--conversation-log",
        ])
        .to_config();
        assert!(config.legacy_im_names);
        assert!(config.date_suffix);
        assert!(config.conversation_log);
    }

    #[test]
    fn the_transcript_directory_is_whatever_was_passed() {
        assert_eq!(
            parse(&["--chat-log-dir", "/tmp/transcripts"]).chat_log_dir(),
            Some(std::path::PathBuf::from("/tmp/transcripts"))
        );
    }
}
