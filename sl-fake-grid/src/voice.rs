//! Which spatial-voice backend the grid's regions speak.
//!
//! Second Life is WebRTC, and says so three ways —
//! `SimulatorFeatures.VoiceServerType`, the login response's `voice-config`
//! section, and the `RequiredVoiceVersion` event-queue push on region entry.
//!
//! A stock OpenSim region speaks **nothing**. Its two voice modules
//! (`VivoxVoiceModule`, `FreeSwitchVoiceModule`) are optional and off by
//! default, so a region nobody configured serves no
//! `ProvisionVoiceAccountRequest` at all — and OpenSim names no backend even
//! when one is loaded: neither `VoiceServerType` nor `RequiredVoiceVersion`
//! appears anywhere in its sources. A viewer told nothing falls back to Vivox
//! by itself (`LLVoiceClient::handleSimulatorFeaturesReceived` turns an empty
//! `VoiceServerType` into `VIVOX_VOICE_SERVER_TYPE`), which is presumably why
//! it never needed the field.
//!
//! # Why there is no Vivox side here
//!
//! Both of OpenSim's modules answer with the Vivox SIP account shape, so
//! modelling "OpenSim with voice" would mean a Vivox-shaped fixture. This
//! workspace does not implement Vivox-shaped voice anywhere — Second Life
//! removed Vivox for WebRTC, and OpenSim support for a leaf feature like voice
//! is not a priority — so a grid nobody configured would be serving a path
//! nothing here will ever speak, and a fixture nothing consumes is a promise
//! the crate cannot keep.
//!
//! Modelling *stock* OpenSim is also the choice made elsewhere in this crate
//! for the same reason: [`open_sim_prices`](crate::open_sim_prices) is what a
//! region running the money module nobody reconfigured answers with, not the
//! numbers some deployment set. So the OpenSim flavour is
//! [`VoiceBackend::Silent`], and the divergence a viewer meets is the real one
//! — a grid that offers voice and a grid that does not.

/// The spatial-voice backend a region's `ProvisionVoiceAccountRequest` serves.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `sl_fake_grid::VoiceBackend`, \
              `FakeGridBuilder::voice_backend(VoiceBackend::Silent)` -- where a bare \
              `Backend` would say nothing about what it is a backend for"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceBackend {
    /// WebRTC, served by [`WebRtcStub`](sl_proto::WebRtcStub) — the offer /
    /// answer / ICE-trickle signalling plane with no media behind it. Second
    /// Life's backend, and the fake grid's default.
    #[default]
    WebRtc,
    /// No voice backend: nothing is advertised —
    /// `SimulatorFeatures.VoiceServerType`, the login `voice-config` and the
    /// arrival `RequiredVoiceVersion` are all absent — and a provision request
    /// is refused with
    /// [`VoiceProvisionRefusal::BackendUnavailable`](sl_proto::VoiceProvisionRefusal::BackendUnavailable).
    ///
    /// A stock OpenSim region, and the OpenSim flavour's default. See the
    /// module docs for why there is no third variant.
    Silent,
}
