//! Shared GStreamer bus-message helpers for the video surface and the audio
//! stream player: `missing-plugin` collection, friendly error text, and tag
//! ("now playing") extraction.

/// The human-readable description out of a `missing-plugin` element message's
/// structure (`gst_pbutils` posts these when decodebin finds no decoder /
/// demuxer / URI source), or [`None`] for any other element message.
pub(crate) fn missing_plugin_description(message: &gstreamer::Message) -> Option<String> {
    let gstreamer::MessageView::Element(element) = message.view() else {
        return None;
    };
    let structure = element.structure()?;
    if structure.name() != "missing-plugin" {
        return None;
    }
    // The structure carries a localised human-readable "name" (e.g.
    // "H.264 (High Profile) decoder"); fall back to the technical "detail".
    structure
        .get::<String>("name")
        .ok()
        .or_else(|| structure.get::<String>("detail").ok())
}

/// Compose the user-facing error text for a bus error, folding in any
/// `missing-plugin` descriptions collected earlier — so a codec gap reads
/// *"needs an H.264 decoder — install the matching GStreamer plugin"*, not a
/// bare internal stream error.
pub(crate) fn friendly_error(
    error: &gstreamer::message::Error,
    missing_plugins: &[String],
) -> String {
    let base = error.error().to_string();
    if missing_plugins.is_empty() {
        // The commonest bare failure worth translating: no HTTP source
        // element installed at all.
        if base.contains("No URI handler") {
            return format!(
                "{base} — GStreamer cannot fetch this URL scheme; install the GStreamer HTTP \
                 plugin (soup)"
            );
        }
        return base;
    }
    format!(
        "needs {} — install the matching GStreamer plugin(s)",
        missing_plugins.join(", ")
    )
}

/// The stream / track title out of a tag-list message, if it carries one —
/// for radio streams this is the ICY "now playing" metadata `icydemux`
/// re-emits as a title tag.
pub(crate) fn title_from_tags(tags: &gstreamer::TagList) -> Option<String> {
    tags.get::<gstreamer::tags::Title>()
        .map(|title| String::from(title.get()))
}
