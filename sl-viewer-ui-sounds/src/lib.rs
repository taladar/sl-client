//! The viewer's own **UI feedback sounds**: the typing chirp, money paid /
//! received, teleport out, the snapshot shutter.
//!
//! It is its own crate rather than one more module of `sl-viewer-ui-core`
//! because three lines of it — `use sl_audio::…` and `use sl_client_bevy::…` —
//! were the only reason the UI's shared vocabulary depended on the audio engine
//! and on the whole protocol runtime behind it (`sl-proto`, `sl-wire`,
//! `sl-asset`, `reqwest`, `tokio`). Twenty-odd crates read that vocabulary,
//! most of them only for `ui::column()` and `ui_font`, and every one of them
//! recompiled behind the protocol stack for a wire type (`AssetKey`) that the
//! crate whose doc says "Nothing here knows what a floater or a tab is" has no
//! business naming. It names nothing in `ui-core` beyond the UI root, and
//! `ui-core` names nothing here, so the two are siblings and compile in
//! parallel.
//!
//! Everything the catalogue is — the resolution order, the skin overrides, the
//! wired emitters — is documented on [`ui_sounds`].

#![expect(
    clippy::module_name_repetitions,
    reason = "the module owns one concept and is named for it, so its types read \
              as `ui_sounds::UiSound` and `ui_sounds::UiSoundsPlugin` — the names \
              every call site in the viewer already spells. `sl-viewer-ui-core` \
              carries the same exemption for the same reason; it came with the \
              module rather than being new here"
)]

pub mod ui_sounds;
