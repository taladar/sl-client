//! Errors surfaced by the audio mixer.
#![expect(
    clippy::module_name_repetitions,
    reason = "AudioError is the crate's public error type, re-exported at the crate root"
)]

/// An error from the audio backend.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// Decoding an encoded sound clip failed (unrecognised format, corrupt
    /// data, or a decode error). Carries the underlying message.
    #[error("audio clip decode failed: {0}")]
    Decode(String),

    /// No suitable output device was found, or the requested device is gone.
    #[error("no audio output device available: {0}")]
    NoDevice(String),

    /// Starting or restarting the audio stream on the device failed.
    #[error("failed to start audio stream: {0}")]
    Stream(String),

    /// Building or mutating the audio graph failed.
    #[error("audio graph error: {0}")]
    Graph(String),
}
