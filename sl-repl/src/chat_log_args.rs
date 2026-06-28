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
