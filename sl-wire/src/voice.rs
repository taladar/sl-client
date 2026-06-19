//! Voice-chat *signalling* over the region capabilities.
//!
//! Second Life and (FreeSWITCH/Vivox-configured) OpenSim negotiate voice
//! entirely out of band of the audio media. A viewer asks the region for voice
//! account credentials (`ProvisionVoiceAccountRequest`) and the parcel's voice
//! channel (`ParcelVoiceInfoRequest`) over CAPS, and — for the modern WebRTC
//! path — trickles ICE candidates over a third capability
//! (`VoiceSignalingRequest`).
//!
//! The audio transport itself is **out of scope** here: this client never opens
//! a Vivox SIP/RTP session or a WebRTC peer connection. This module only builds
//! the CAPS request bodies and decodes the signalling replies, so a caller that
//! *does* supply an audio engine has the grid-side protocol handled. The WebRTC
//! JSEP **offer** SDP and the ICE candidate strings such an engine would produce
//! are passed through verbatim (this module neither generates nor interprets
//! them), and the WebRTC **answer** SDP returned by the grid is surfaced as an
//! opaque string for that engine to consume.
//!
//! Field names and request/response shapes are cross-checked against the
//! Firestorm viewer (`llvoicevivox.cpp` / `llvoicewebrtc.cpp`) and OpenSim's
//! `VivoxVoiceModule` / `FreeSwitchVoiceModule`.

use std::collections::HashMap;

use crate::llsd::{Llsd, parse_llsd_xml, push_escaped};

/// The voice server type string for the legacy Vivox (SIP/RTP) backend, sent as
/// the `voice_server_type` field of a [`ProvisionVoiceAccountRequest`](build_provision_voice_account_request).
pub const VOICE_SERVER_TYPE_VIVOX: &str = "vivox";

/// The voice server type string for the modern WebRTC backend.
pub const VOICE_SERVER_TYPE_WEBRTC: &str = "webrtc";

/// Parameters for a `ProvisionVoiceAccountRequest` capability POST.
///
/// The same capability serves both voice backends; which fields are populated
/// selects the path:
///
/// - **Vivox** — only [`voice_server_type`](Self::voice_server_type) is set (to
///   [`VOICE_SERVER_TYPE_VIVOX`]); the grid replies with SIP account
///   credentials. Use [`VoiceProvisionRequest::vivox`].
/// - **WebRTC** — a JSEP **offer** SDP plus a channel type (and optionally a
///   parcel id) are set; the grid replies with a JSEP **answer** SDP and a
///   viewer session id. Use [`VoiceProvisionRequest::webrtc`].
/// - **WebRTC logout** — [`logout`](Self::logout) with the
///   [`viewer_session`](Self::viewer_session) to tear the connection down. Use
///   [`VoiceProvisionRequest::webrtc_logout`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceProvisionRequest {
    /// The voice backend ([`VOICE_SERVER_TYPE_VIVOX`] /
    /// [`VOICE_SERVER_TYPE_WEBRTC`]); omitted from the body when `None` (the
    /// oldest grids infer Vivox).
    pub voice_server_type: Option<String>,
    /// The WebRTC channel type (`"local"` for the spatial parcel channel);
    /// `None` for Vivox.
    pub channel_type: Option<String>,
    /// The parcel's local id to bind the channel to, or `None` to omit it (the
    /// grid then uses the agent's current parcel / the region channel).
    pub parcel_local_id: Option<i32>,
    /// The WebRTC JSEP **offer** SDP, produced by an out-of-scope WebRTC peer
    /// connection and passed through verbatim; `None` for Vivox.
    pub jsep_offer_sdp: Option<String>,
    /// When `true`, this POST tears down the WebRTC connection (`logout: true`)
    /// rather than requesting one.
    pub logout: bool,
    /// The WebRTC viewer session id to tear down (echoed from a prior provision
    /// reply); `None` outside a logout.
    pub viewer_session: Option<String>,
}

impl VoiceProvisionRequest {
    /// A Vivox provision request (`{ voice_server_type: "vivox" }`).
    #[must_use]
    pub fn vivox() -> Self {
        Self {
            voice_server_type: Some(VOICE_SERVER_TYPE_VIVOX.to_owned()),
            ..Self::default()
        }
    }

    /// A WebRTC provision request carrying the JSEP `offer` SDP, the channel
    /// type (typically `"local"`), and an optional parcel id.
    #[must_use]
    pub fn webrtc(
        offer_sdp: impl Into<String>,
        channel_type: impl Into<String>,
        parcel_local_id: Option<i32>,
    ) -> Self {
        Self {
            voice_server_type: Some(VOICE_SERVER_TYPE_WEBRTC.to_owned()),
            channel_type: Some(channel_type.into()),
            parcel_local_id,
            jsep_offer_sdp: Some(offer_sdp.into()),
            logout: false,
            viewer_session: None,
        }
    }

    /// A WebRTC logout request tearing down the session `viewer_session`.
    #[must_use]
    pub fn webrtc_logout(viewer_session: impl Into<String>) -> Self {
        Self {
            voice_server_type: Some(VOICE_SERVER_TYPE_WEBRTC.to_owned()),
            logout: true,
            viewer_session: Some(viewer_session.into()),
            ..Self::default()
        }
    }
}

/// A single ICE candidate trickled over the `VoiceSignalingRequest` capability
/// (WebRTC only). The fields mirror the browser `RTCIceCandidate` shape the
/// viewer forwards (`sdpMid` / `sdpMLineIndex` / `candidate`); their contents
/// come from the out-of-scope WebRTC peer connection and are passed through
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceCandidate {
    /// The media stream identification of the candidate (`sdpMid`).
    pub sdp_mid: String,
    /// The index of the media description in the SDP this candidate is
    /// associated with (`sdpMLineIndex`).
    pub sdp_mline_index: i32,
    /// The candidate string itself (`candidate:` line, RFC 5245).
    pub candidate: String,
}

/// Builds the LLSD-XML body for a `ProvisionVoiceAccountRequest` capability POST
/// (see [`VoiceProvisionRequest`]). Only the populated fields are emitted, so a
/// [`VoiceProvisionRequest::vivox`] produces just `{ voice_server_type: "vivox"
/// }` while a [`VoiceProvisionRequest::webrtc`] additionally carries the nested
/// `jsep` offer, the `channel_type` and any `parcel_local_id`.
#[must_use]
pub fn build_provision_voice_account_request(request: &VoiceProvisionRequest) -> String {
    let mut out = String::from("<llsd><map>");
    if request.logout {
        out.push_str("<key>logout</key><boolean>1</boolean>");
    }
    if let Some(sdp) = &request.jsep_offer_sdp {
        out.push_str(
            "<key>jsep</key><map><key>type</key><string>offer</string><key>sdp</key><string>",
        );
        push_escaped(&mut out, sdp);
        out.push_str("</string></map>");
    }
    if let Some(parcel_local_id) = request.parcel_local_id {
        out.push_str("<key>parcel_local_id</key><integer>");
        out.push_str(&parcel_local_id.to_string());
        out.push_str("</integer>");
    }
    if let Some(channel_type) = &request.channel_type {
        out.push_str("<key>channel_type</key><string>");
        push_escaped(&mut out, channel_type);
        out.push_str("</string>");
    }
    if let Some(viewer_session) = &request.viewer_session {
        out.push_str("<key>viewer_session</key><string>");
        push_escaped(&mut out, viewer_session);
        out.push_str("</string>");
    }
    if let Some(server_type) = &request.voice_server_type {
        out.push_str("<key>voice_server_type</key><string>");
        push_escaped(&mut out, server_type);
        out.push_str("</string>");
    }
    out.push_str("</map></llsd>");
    out
}

/// Builds the LLSD-XML body for a `ParcelVoiceInfoRequest` capability POST. The
/// viewer sends an empty (`undef`) body — the region answers for the agent's
/// current parcel — so this takes no parameters.
#[must_use]
pub fn build_parcel_voice_info_request() -> String {
    String::from("<llsd><undef /></llsd>")
}

/// Builds the LLSD-XML body for a `VoiceSignalingRequest` capability POST (the
/// WebRTC ICE-candidate trickle). If `candidates` is non-empty they are sent as
/// the `candidates` array; otherwise, if `completed` is set, an end-of-gathering
/// `{ candidate: { completed: true } }` is sent (mirroring the viewer, which
/// sends one or the other). Both forms carry the `viewer_session` and the
/// `webrtc` server type.
#[must_use]
pub fn build_voice_signaling_request(
    viewer_session: &str,
    candidates: &[IceCandidate],
    completed: bool,
) -> String {
    let mut out = String::from("<llsd><map>");
    if candidates.is_empty() {
        if completed {
            out.push_str("<key>candidate</key><map><key>completed</key><boolean>1</boolean></map>");
        }
    } else {
        out.push_str("<key>candidates</key><array>");
        for candidate in candidates {
            out.push_str("<map><key>sdpMid</key><string>");
            push_escaped(&mut out, &candidate.sdp_mid);
            out.push_str("</string><key>sdpMLineIndex</key><integer>");
            out.push_str(&candidate.sdp_mline_index.to_string());
            out.push_str("</integer><key>candidate</key><string>");
            push_escaped(&mut out, &candidate.candidate);
            out.push_str("</string></map>");
        }
        out.push_str("</array>");
    }
    out.push_str("<key>viewer_session</key><string>");
    push_escaped(&mut out, viewer_session);
    out.push_str("</string><key>voice_server_type</key><string>");
    out.push_str(VOICE_SERVER_TYPE_WEBRTC);
    out.push_str("</string></map></llsd>");
    out
}

/// Parses a `ProvisionVoiceAccountRequest` POST body back into a
/// [`VoiceProvisionRequest`] — the inverse of
/// [`build_provision_voice_account_request`]. The populated fields select the
/// backend (a lone `voice_server_type` is Vivox; a nested `jsep` offer plus
/// `channel_type` is WebRTC; `logout` is a WebRTC teardown), mirroring the
/// lenient field-by-field decoding elsewhere in this module: a missing field is
/// left at its default rather than failing. The `jsep` offer SDP is read
/// regardless of the (always `"offer"`) nested `type`.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_provision_voice_account_request(
    xml: &str,
) -> Result<VoiceProvisionRequest, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let string = |key: &str| root.get(key).and_then(Llsd::as_str).map(str::to_owned);
    let jsep_offer_sdp = root
        .get("jsep")
        .and_then(|jsep| jsep.get("sdp"))
        .and_then(Llsd::as_str)
        .map(str::to_owned);
    Ok(VoiceProvisionRequest {
        voice_server_type: string("voice_server_type"),
        channel_type: string("channel_type"),
        parcel_local_id: root.get("parcel_local_id").and_then(Llsd::as_i32),
        jsep_offer_sdp,
        logout: root.get("logout").and_then(Llsd::as_bool).unwrap_or(false),
        viewer_session: string("viewer_session"),
    })
}

/// Parses a `VoiceSignalingRequest` POST body (the WebRTC ICE-candidate trickle)
/// back into its `(viewer_session, candidates, completed)` parts — the inverse
/// of [`build_voice_signaling_request`]. The viewer sends one of two forms: a
/// `candidates` array (then `completed` is `false`) or an end-of-gathering
/// `candidate.completed` flag (then `candidates` is empty), so a body never
/// carries both.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_voice_signaling_request(
    xml: &str,
) -> Result<(String, Vec<IceCandidate>, bool), roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let viewer_session = root
        .get("viewer_session")
        .and_then(Llsd::as_str)
        .unwrap_or_default()
        .to_owned();
    let completed = root
        .get("candidate")
        .and_then(|candidate| candidate.get("completed"))
        .and_then(Llsd::as_bool)
        .unwrap_or(false);
    let candidates = root
        .get("candidates")
        .and_then(Llsd::as_array)
        .map(|array| {
            array
                .iter()
                .map(|entry| IceCandidate {
                    sdp_mid: entry
                        .get("sdpMid")
                        .and_then(Llsd::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    sdp_mline_index: entry
                        .get("sdpMLineIndex")
                        .and_then(Llsd::as_i32)
                        .unwrap_or(0),
                    candidate: entry
                        .get("candidate")
                        .and_then(Llsd::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((viewer_session, candidates, completed))
}

/// The decoded reply to a `ProvisionVoiceAccountRequest`. The same capability
/// answers both backends, so every field is optional and the populated set
/// distinguishes them: the Vivox reply fills the SIP-account fields
/// ([`username`](Self::username) … [`account_server_name`](Self::account_server_name)),
/// while the WebRTC reply fills [`viewer_session`](Self::viewer_session) and the
/// JSEP **answer** ([`jsep_type`](Self::jsep_type) / [`jsep_sdp`](Self::jsep_sdp)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceAccountInfo {
    /// The backend the grid answered for, when it echoes `voice_server_type`.
    pub voice_server_type: Option<String>,
    /// The Vivox SIP account username.
    pub username: Option<String>,
    /// The Vivox SIP account password.
    pub password: Option<String>,
    /// The Vivox SIP domain / hostname (`voice_sip_uri_hostname`).
    pub sip_uri_hostname: Option<String>,
    /// The Vivox account API endpoint (`voice_account_server_name` — despite the
    /// name, a full URI, as the viewer notes).
    pub account_server_name: Option<String>,
    /// The WebRTC viewer session id, echoed back on
    /// [`VoiceSignalingRequest`](build_voice_signaling_request) and logout.
    pub viewer_session: Option<String>,
    /// The WebRTC JSEP answer type (`"answer"`).
    pub jsep_type: Option<String>,
    /// The WebRTC JSEP **answer** SDP, surfaced opaque for the out-of-scope
    /// WebRTC peer connection to apply.
    pub jsep_sdp: Option<String>,
}

impl VoiceAccountInfo {
    /// Decodes a [`VoiceAccountInfo`] from the LLSD body of a
    /// `ProvisionVoiceAccountRequest` reply. Missing fields are left `None`
    /// rather than failing, so a partial (or backend-specific) reply still
    /// decodes.
    #[must_use]
    pub fn from_llsd(body: &Llsd) -> Self {
        let string = |key: &str| body.get(key).and_then(Llsd::as_str).map(str::to_owned);
        let jsep = body.get("jsep");
        Self {
            voice_server_type: string("voice_server_type"),
            username: string("username"),
            password: string("password"),
            sip_uri_hostname: string("voice_sip_uri_hostname"),
            account_server_name: string("voice_account_server_name"),
            viewer_session: string("viewer_session"),
            jsep_type: jsep
                .and_then(|j| j.get("type"))
                .and_then(Llsd::as_str)
                .map(str::to_owned),
            jsep_sdp: jsep
                .and_then(|j| j.get("sdp"))
                .and_then(Llsd::as_str)
                .map(str::to_owned),
        }
    }

    /// Whether this reply carries a WebRTC JSEP answer (vs. Vivox credentials).
    #[must_use]
    pub const fn is_webrtc(&self) -> bool {
        self.jsep_sdp.is_some()
    }

    /// Builds the LLSD reply body for a `ProvisionVoiceAccountRequest` — the
    /// inverse of [`from_llsd`](Self::from_llsd). Only the populated fields are
    /// emitted (so a Vivox reply carries just the SIP-account keys and a WebRTC
    /// reply just the session id + nested JSEP `answer`), and the result
    /// round-trips back through [`from_llsd`](Self::from_llsd).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let mut map: HashMap<String, Llsd> = HashMap::new();
        for (key, value) in [
            ("voice_server_type", &self.voice_server_type),
            ("username", &self.username),
            ("password", &self.password),
            ("voice_sip_uri_hostname", &self.sip_uri_hostname),
            ("voice_account_server_name", &self.account_server_name),
            ("viewer_session", &self.viewer_session),
        ] {
            if let Some(value) = value {
                let _previous = map.insert(key.to_owned(), Llsd::String(value.clone()));
            }
        }
        if self.jsep_type.is_some() || self.jsep_sdp.is_some() {
            let mut jsep: HashMap<String, Llsd> = HashMap::new();
            if let Some(value) = &self.jsep_type {
                let _previous = jsep.insert("type".to_owned(), Llsd::String(value.clone()));
            }
            if let Some(value) = &self.jsep_sdp {
                let _previous = jsep.insert("sdp".to_owned(), Llsd::String(value.clone()));
            }
            let _previous = map.insert("jsep".to_owned(), Llsd::Map(jsep));
        }
        Llsd::Map(map)
    }
}

/// Builds the LLSD-XML reply body for a `ProvisionVoiceAccountRequest`
/// capability POST from a [`VoiceAccountInfo`] — the inverse of
/// [`VoiceAccountInfo::from_llsd`]. Built on [`Llsd::to_llsd_xml`], so it
/// round-trips through [`VoiceAccountInfo::from_llsd`].
#[must_use]
pub fn build_provision_voice_account_response(info: &VoiceAccountInfo) -> String {
    info.to_llsd().to_llsd_xml()
}

/// The decoded reply to a `ParcelVoiceInfoRequest`: the parcel's voice channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParcelVoiceInfo {
    /// The parcel the channel belongs to (`-1` if the reply omitted it).
    pub parcel_local_id: i32,
    /// The region's name.
    pub region_name: String,
    /// The channel URI to connect to (a `sip:` URI for Vivox/FreeSWITCH), or
    /// `None`/empty when the parcel has no voice (the viewer then drops out of
    /// spatial voice).
    pub channel_uri: Option<String>,
    /// Optional per-channel credentials (rarely sent — OpenSim leaves it unset).
    pub channel_credentials: Option<String>,
}

impl ParcelVoiceInfo {
    /// Decodes a [`ParcelVoiceInfo`] from the LLSD body of a
    /// `ParcelVoiceInfoRequest` reply (`{ parcel_local_id, region_name,
    /// voice_credentials: { channel_uri, channel_credentials? } }`). Returns
    /// `None` if the body is not a map (the cap returned `undef` / no voice).
    #[must_use]
    pub fn from_llsd(body: &Llsd) -> Option<Self> {
        if !matches!(body, Llsd::Map(_)) {
            return None;
        }
        let credentials = body.get("voice_credentials");
        let channel_uri = credentials
            .and_then(|map| map.get("channel_uri"))
            .and_then(Llsd::as_str)
            .filter(|uri| !uri.is_empty())
            .map(str::to_owned);
        let channel_credentials = credentials
            .and_then(|map| map.get("channel_credentials"))
            .and_then(Llsd::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Some(Self {
            parcel_local_id: body
                .get("parcel_local_id")
                .and_then(Llsd::as_i32)
                .unwrap_or(-1),
            region_name: body
                .get("region_name")
                .and_then(Llsd::as_str)
                .unwrap_or_default()
                .to_owned(),
            channel_uri,
            channel_credentials,
        })
    }

    /// Builds the LLSD reply body for a `ParcelVoiceInfoRequest`
    /// (`{ parcel_local_id, region_name, voice_credentials: { channel_uri,
    /// channel_credentials? } }`) — the inverse of [`from_llsd`](Self::from_llsd).
    /// A parcel with no voice ([`channel_uri`](Self::channel_uri) is `None`)
    /// emits an empty `channel_uri` string, the form the grid sends to drop a
    /// viewer out of spatial voice; the optional
    /// [`channel_credentials`](Self::channel_credentials) is emitted only when
    /// present. The result round-trips back through [`from_llsd`](Self::from_llsd).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let mut credentials: HashMap<String, Llsd> = HashMap::from([(
            "channel_uri".to_owned(),
            Llsd::String(self.channel_uri.clone().unwrap_or_default()),
        )]);
        if let Some(value) = &self.channel_credentials {
            let _previous = credentials.insert(
                "channel_credentials".to_owned(),
                Llsd::String(value.clone()),
            );
        }
        Llsd::Map(HashMap::from([
            (
                "parcel_local_id".to_owned(),
                Llsd::Integer(self.parcel_local_id),
            ),
            (
                "region_name".to_owned(),
                Llsd::String(self.region_name.clone()),
            ),
            ("voice_credentials".to_owned(), Llsd::Map(credentials)),
        ]))
    }
}

/// Builds the LLSD-XML reply body for a `ParcelVoiceInfoRequest` capability POST
/// from a [`ParcelVoiceInfo`] — the inverse of [`ParcelVoiceInfo::from_llsd`].
/// Built on [`Llsd::to_llsd_xml`], so it round-trips through
/// [`ParcelVoiceInfo::from_llsd`].
#[must_use]
pub fn build_parcel_voice_info_response(info: &ParcelVoiceInfo) -> String {
    info.to_llsd().to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        IceCandidate, ParcelVoiceInfo, VoiceAccountInfo, VoiceProvisionRequest,
        build_parcel_voice_info_request, build_parcel_voice_info_response,
        build_provision_voice_account_request, build_provision_voice_account_response,
        build_voice_signaling_request, parse_provision_voice_account_request,
        parse_voice_signaling_request,
    };
    use crate::llsd::parse_llsd_xml;

    /// A Vivox provision body carries only the server type and decodes its
    /// credentials reply.
    #[test]
    fn vivox_provision_round_trip() -> Result<(), String> {
        let body = build_provision_voice_account_request(&VoiceProvisionRequest::vivox());
        assert_eq!(
            body,
            "<llsd><map><key>voice_server_type</key><string>vivox</string></map></llsd>"
        );

        let reply = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>username</key><string>xMjQ1</string>",
            "<key>password</key><string>secret</string>",
            "<key>voice_sip_uri_hostname</key><string>sip.example.com</string>",
            "<key>voice_account_server_name</key><string>https://vivox.example/api</string>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let info = VoiceAccountInfo::from_llsd(&reply);
        assert_eq!(info.username.as_deref(), Some("xMjQ1"));
        assert_eq!(info.password.as_deref(), Some("secret"));
        assert_eq!(info.sip_uri_hostname.as_deref(), Some("sip.example.com"));
        assert_eq!(
            info.account_server_name.as_deref(),
            Some("https://vivox.example/api")
        );
        assert!(!info.is_webrtc());
        Ok(())
    }

    /// A WebRTC provision body nests the JSEP offer and decodes the JSEP answer.
    #[test]
    fn webrtc_provision_round_trip() -> Result<(), String> {
        let request = VoiceProvisionRequest::webrtc("v=0 offer", "local", Some(7));
        let body = build_provision_voice_account_request(&request);
        // The offer SDP, channel type, parcel id and server type are all present.
        assert!(body.contains("<key>jsep</key><map><key>type</key><string>offer</string>"));
        assert!(body.contains("<key>sdp</key><string>v=0 offer</string>"));
        assert!(body.contains("<key>parcel_local_id</key><integer>7</integer>"));
        assert!(body.contains("<key>channel_type</key><string>local</string>"));
        assert!(body.contains("<key>voice_server_type</key><string>webrtc</string>"));

        let reply = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>viewer_session</key><string>abc-123</string>",
            "<key>jsep</key><map><key>type</key><string>answer</string>",
            "<key>sdp</key><string>v=0 answer</string></map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let info = VoiceAccountInfo::from_llsd(&reply);
        assert_eq!(info.viewer_session.as_deref(), Some("abc-123"));
        assert_eq!(info.jsep_type.as_deref(), Some("answer"));
        assert_eq!(info.jsep_sdp.as_deref(), Some("v=0 answer"));
        assert!(info.is_webrtc());
        Ok(())
    }

    /// The parcel-voice request is an empty `undef`; the reply decodes the
    /// channel URI out of the nested `voice_credentials` map.
    #[test]
    fn parcel_voice_info_round_trip() -> Result<(), String> {
        assert_eq!(build_parcel_voice_info_request(), "<llsd><undef /></llsd>");

        let reply = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>parcel_local_id</key><integer>42</integer>",
            "<key>region_name</key><string>Default Region</string>",
            "<key>voice_credentials</key><map>",
            "<key>channel_uri</key><string>sip:Region@sip.example.com</string>",
            "</map></map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let info = ParcelVoiceInfo::from_llsd(&reply).ok_or("expected a parcel voice info")?;
        assert_eq!(info.parcel_local_id, 42);
        assert_eq!(info.region_name, "Default Region");
        assert_eq!(
            info.channel_uri.as_deref(),
            Some("sip:Region@sip.example.com")
        );
        assert_eq!(info.channel_credentials, None);
        Ok(())
    }

    /// An empty `channel_uri` (no voice on the parcel) decodes to `None`.
    #[test]
    fn parcel_voice_info_no_voice() -> Result<(), String> {
        let reply = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>parcel_local_id</key><integer>1</integer>",
            "<key>region_name</key><string>Quiet</string>",
            "<key>voice_credentials</key><map><key>channel_uri</key><string /></map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let info = ParcelVoiceInfo::from_llsd(&reply).ok_or("expected a parcel voice info")?;
        assert_eq!(info.channel_uri, None);
        Ok(())
    }

    /// A signaling body sends the candidate array with the session and server
    /// type; the end-of-gathering form sends `candidate.completed` instead.
    #[test]
    fn voice_signaling_bodies() {
        let candidates = [IceCandidate {
            sdp_mid: "0".to_owned(),
            sdp_mline_index: 0,
            candidate: "candidate:1 1 udp".to_owned(),
        }];
        let body = build_voice_signaling_request("sess-1", &candidates, false);
        assert!(body.contains("<key>candidates</key><array><map>"));
        assert!(body.contains("<key>sdpMid</key><string>0</string>"));
        assert!(body.contains("<key>sdpMLineIndex</key><integer>0</integer>"));
        assert!(body.contains("<key>candidate</key><string>candidate:1 1 udp</string>"));
        assert!(body.contains("<key>viewer_session</key><string>sess-1</string>"));
        assert!(body.contains("<key>voice_server_type</key><string>webrtc</string>"));

        let completed = build_voice_signaling_request("sess-1", &[], true);
        assert!(
            completed.contains("<key>candidate</key><map><key>completed</key><boolean>1</boolean>")
        );
        assert!(!completed.contains("<key>candidates</key>"));
    }

    /// The server-side request parser is the inverse of the Vivox/WebRTC and
    /// logout provision builders.
    #[test]
    fn provision_request_parse_round_trip() -> Result<(), String> {
        for request in [
            VoiceProvisionRequest::vivox(),
            VoiceProvisionRequest::webrtc("v=0 offer", "local", Some(7)),
            VoiceProvisionRequest::webrtc("v=0 offer", "local", None),
            VoiceProvisionRequest::webrtc_logout("sess-9"),
        ] {
            let body = build_provision_voice_account_request(&request);
            let parsed = parse_provision_voice_account_request(&body)
                .map_err(|error| format!("{error:?}"))?;
            assert_eq!(parsed, request);
        }
        Ok(())
    }

    /// The signaling parser recovers the candidate array and the session, and
    /// the end-of-gathering form recovers `completed` with no candidates.
    #[test]
    fn signaling_request_parse_round_trip() -> Result<(), String> {
        let candidates = vec![
            IceCandidate {
                sdp_mid: "0".to_owned(),
                sdp_mline_index: 0,
                candidate: "candidate:1 1 udp".to_owned(),
            },
            IceCandidate {
                sdp_mid: "audio".to_owned(),
                sdp_mline_index: 1,
                candidate: "candidate:2 1 udp".to_owned(),
            },
        ];
        let body = build_voice_signaling_request("sess-1", &candidates, false);
        let (session, parsed, completed) =
            parse_voice_signaling_request(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(session, "sess-1");
        assert_eq!(parsed, candidates);
        assert!(!completed);

        let done = build_voice_signaling_request("sess-1", &[], true);
        let (session, parsed, completed) =
            parse_voice_signaling_request(&done).map_err(|error| format!("{error:?}"))?;
        assert_eq!(session, "sess-1");
        assert!(parsed.is_empty());
        assert!(completed);
        Ok(())
    }

    /// The provision reply builder is the inverse of `VoiceAccountInfo::from_llsd`
    /// for both the Vivox-credentials and WebRTC-answer shapes.
    #[test]
    fn provision_response_round_trip() -> Result<(), String> {
        let vivox = VoiceAccountInfo {
            voice_server_type: Some("vivox".to_owned()),
            username: Some("xMjQ1".to_owned()),
            password: Some("secret".to_owned()),
            sip_uri_hostname: Some("sip.example.com".to_owned()),
            account_server_name: Some("https://vivox.example/api".to_owned()),
            ..VoiceAccountInfo::default()
        };
        let reply = parse_llsd_xml(&build_provision_voice_account_response(&vivox))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(VoiceAccountInfo::from_llsd(&reply), vivox);

        let webrtc = VoiceAccountInfo {
            voice_server_type: Some("webrtc".to_owned()),
            viewer_session: Some("abc-123".to_owned()),
            jsep_type: Some("answer".to_owned()),
            jsep_sdp: Some("v=0 answer".to_owned()),
            ..VoiceAccountInfo::default()
        };
        let reply = parse_llsd_xml(&build_provision_voice_account_response(&webrtc))
            .map_err(|error| format!("{error:?}"))?;
        let decoded = VoiceAccountInfo::from_llsd(&reply);
        assert_eq!(decoded, webrtc);
        assert!(decoded.is_webrtc());
        Ok(())
    }

    /// The parcel-voice reply builder round-trips through `from_llsd`, including
    /// the no-voice (empty `channel_uri` → `None`) case.
    #[test]
    fn parcel_voice_response_round_trip() -> Result<(), String> {
        let info = ParcelVoiceInfo {
            parcel_local_id: 42,
            region_name: "Default Region".to_owned(),
            channel_uri: Some("sip:Region@sip.example.com".to_owned()),
            channel_credentials: Some("creds".to_owned()),
        };
        let reply = parse_llsd_xml(&build_parcel_voice_info_response(&info))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            ParcelVoiceInfo::from_llsd(&reply).ok_or("expected a parcel voice info")?,
            info
        );

        let no_voice = ParcelVoiceInfo {
            parcel_local_id: 1,
            region_name: "Quiet".to_owned(),
            channel_uri: None,
            channel_credentials: None,
        };
        let reply = parse_llsd_xml(&build_parcel_voice_info_response(&no_voice))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            ParcelVoiceInfo::from_llsd(&reply).ok_or("expected a parcel voice info")?,
            no_voice
        );
        Ok(())
    }
}
