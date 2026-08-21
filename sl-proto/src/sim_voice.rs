//! The server-side voice *signalling* stub the three voice capabilities
//! (`ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`,
//! `VoiceSignalingRequest`) serve from.
//!
//! [`SimSession`](crate::SimSession) holds one [`SimVoice`] as a
//! driver-populated serving store (the `SimExperiences` stance): the
//! fixtures say which backends the region speaks and which channel each
//! parcel has, the live state is the set of WebRTC connections the client
//! has provisioned — each with its offer, the answer we minted, and the ICE
//! candidates it trickled. Everything is deterministic (`BTreeMap`s, a
//! serial viewer-session counter) — no clock, no RNG.
//!
//! **What this stub is not.** There is no media plane: no DTLS handshake, no
//! SRTP, no audio. The JSEP *answer* [`WebRtcStub::answer_for`] derives from
//! the client's offer has the right *shape* (mirrored media sections, our
//! ICE credentials and candidates, `a=setup:passive`) so a real WebRTC stack
//! would accept it as an answer and start ICE, but nothing listens on the
//! advertised candidate. That is enough to drive a client's *signalling*
//! state machine end to end (offer → answer → trickle → logout), which is
//! what the fake grid and the loopback tests need.
//!
//! **Protocol shape** (Firestorm `indra/newview/llvoicewebrtc.cpp`): the
//! server's own ICE candidates ride inside the synchronous JSEP answer — the
//! viewer has no inbound ICE-trickle path at all (`VoiceSignalingRequest`
//! is client→server only, its reply is ignored apart from the status). The
//! only voice event-queue push is `RequiredVoiceVersion`
//! ([`SimSession::enqueue_required_voice_version`](crate::SimSession::enqueue_required_voice_version)).

use std::collections::BTreeMap;

use sl_wire::{
    IceCandidate, ParcelVoiceInfo, RegionLocalParcelId, VOICE_CHANNEL_TYPE_LOCAL,
    VOICE_CHANNEL_TYPE_MULTIAGENT, VOICE_SERVER_TYPE_VIVOX, VOICE_SERVER_TYPE_WEBRTC,
    VoiceAccountInfo, VoiceProvisionRequest,
};
use uuid::Uuid;

/// The high bits of the deterministic viewer-session ids this stub mints
/// (`0x5e55` ≈ "sess"); the serial is the low bits.
const VIEWER_SESSION_ID_BASE: u128 = 0x5e55_0000_0000_0000_0000_0000_0000_0000;

/// The WebRTC answerer fixture: the ICE / DTLS identity the stub advertises
/// in every JSEP answer it mints. [`Default`] is a deterministic loopback
/// identity (one UDP host candidate on `127.0.0.1:7000`), fine for tests and
/// the fake grid — nothing listens there, the media plane is out of scope.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebRtcStub {
    /// The `a=ice-ufrag` the answer advertises.
    pub ice_ufrag: String,
    /// The `a=ice-pwd` the answer advertises.
    pub ice_pwd: String,
    /// The `a=fingerprint` line's value (`sha-256 AA:BB:…`).
    pub dtls_fingerprint: String,
    /// The `a=candidate` values (without the `a=candidate:` prefix) the
    /// answer carries — the server's side of ICE, sent non-trickled.
    pub candidates: Vec<String>,
}

impl Default for WebRtcStub {
    fn default() -> Self {
        Self {
            ice_ufrag: "fakegrid".to_owned(),
            ice_pwd: "fakegridfakegridfakegrid".to_owned(),
            dtls_fingerprint: "sha-256 \
                00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:\
                00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF"
                .to_owned(),
            candidates: vec!["1 1 udp 2130706431 127.0.0.1 7000 typ host".to_owned()],
        }
    }
}

impl WebRtcStub {
    /// Derives the JSEP **answer** SDP for the client's `offer_sdp`.
    ///
    /// Session-level: `v=`, our own `o=`, `s=`, `t=`, and the offer's
    /// `a=group:` / `a=msid-semantic:` lines (so BUNDLE stays in place).
    /// Per media section: the offer's `m=` and `c=` lines, `a=mid`,
    /// `a=rtcp-mux`, `a=rtpmap`/`a=fmtp`/`a=rtcp-fb`/`a=extmap`/`a=maxptime`/
    /// `a=ptime` attributes (accepting every offered codec), the direction
    /// mirrored (`sendonly` ↔ `recvonly`, `sendrecv`/`inactive` kept),
    /// `a=setup:actpass`/`active` answered with `passive`, and our own
    /// `a=ice-ufrag`/`a=ice-pwd`/`a=fingerprint`/`a=candidate` lines plus
    /// `a=end-of-candidates` in place of the offer's ICE/DTLS identity,
    /// candidates, and `a=ssrc`/`a=msid` stream lines. Unknown attributes are
    /// dropped. An offer without media sections yields a session-level-only
    /// answer. Lines are joined with `\r\n` as SDP requires.
    #[must_use]
    pub fn answer_for(&self, offer_sdp: &str) -> String {
        let mut session_lines: Vec<String> = vec!["v=0".to_owned()];
        let mut media_sections: Vec<Vec<String>> = Vec::new();
        let mut saw_origin = false;
        for raw in offer_sdp.lines() {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if line.starts_with("m=") {
                media_sections.push(vec![line.to_owned()]);
                continue;
            }
            if let Some(section) = media_sections.last_mut() {
                if let Some(kept) = Self::answer_media_line(line) {
                    section.push(kept);
                }
                continue;
            }
            if line.starts_with("o=") {
                saw_origin = true;
                session_lines.push("o=- 1 1 IN IP4 127.0.0.1".to_owned());
            } else if line.starts_with("s=")
                || line.starts_with("t=")
                || line.starts_with("a=group:")
                || line.starts_with("a=msid-semantic:")
            {
                session_lines.push(line.to_owned());
            }
        }
        if !saw_origin {
            session_lines.insert(1, "o=- 1 1 IN IP4 127.0.0.1".to_owned());
        }
        if !session_lines.iter().any(|line| line.starts_with("s=")) {
            session_lines.insert(2, "s=-".to_owned());
        }
        if !session_lines.iter().any(|line| line.starts_with("t=")) {
            session_lines.push("t=0 0".to_owned());
        }
        let mut out = session_lines;
        for section in media_sections {
            out.extend(section);
            out.push(format!("a=ice-ufrag:{}", self.ice_ufrag));
            out.push(format!("a=ice-pwd:{}", self.ice_pwd));
            out.push(format!("a=fingerprint:{}", self.dtls_fingerprint));
            out.push("a=setup:passive".to_owned());
            for candidate in &self.candidates {
                out.push(format!("a=candidate:{candidate}"));
            }
            out.push("a=end-of-candidates".to_owned());
        }
        let mut sdp = out.join("\r\n");
        sdp.push_str("\r\n");
        sdp
    }

    /// Maps one line of an offer's media section to its line in the answer,
    /// or `None` to drop it (see [`answer_for`](Self::answer_for)).
    fn answer_media_line(line: &str) -> Option<String> {
        const KEPT_PREFIXES: [&str; 9] = [
            "c=",
            "a=mid:",
            "a=rtcp-mux",
            "a=rtpmap:",
            "a=fmtp:",
            "a=rtcp-fb:",
            "a=extmap:",
            "a=maxptime:",
            "a=ptime:",
        ];
        match line {
            "a=sendonly" => Some("a=recvonly".to_owned()),
            "a=recvonly" => Some("a=sendonly".to_owned()),
            "a=sendrecv" | "a=inactive" => Some(line.to_owned()),
            _ if KEPT_PREFIXES.iter().any(|prefix| line.starts_with(prefix)) => {
                Some(line.to_owned())
            }
            _ => None,
        }
    }
}

/// Which voice channel a WebRTC connection was provisioned for.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum VoiceChannel {
    /// The spatial channel (`channel_type: "local"`), optionally bound to a
    /// parcel.
    Spatial {
        /// The parcel the client asked to bind to, if it named one.
        parcel_local_id: Option<RegionLocalParcelId>,
    },
    /// A chat session's channel (`channel_type: "multiagent"`): group,
    /// conference, or P2P voice.
    MultiAgent {
        /// The session's voice channel id (the `channel_uri` of its
        /// `voice_channel_info`).
        channel: String,
    },
}

/// One provisioned WebRTC voice connection — the stub's record of a live
/// `viewer_session`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoiceConnection {
    /// The channel the connection was provisioned for.
    pub channel: VoiceChannel,
    /// The client's JSEP offer SDP, verbatim.
    pub offer_sdp: String,
    /// The JSEP answer SDP the stub minted for it.
    pub answer_sdp: String,
    /// Every ICE candidate the client trickled over `VoiceSignalingRequest`,
    /// in arrival order.
    pub ice_candidates: Vec<IceCandidate>,
    /// Whether the client signalled end-of-gathering (`candidate.completed`).
    pub ice_completed: bool,
}

/// Why a `ProvisionVoiceAccountRequest` was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum VoiceProvisionRefusal {
    /// The region does not serve the requested backend (no
    /// [`WebRtcStub`] / no Vivox account fixture).
    BackendUnavailable,
    /// A WebRTC request that is neither a logout nor carries a JSEP offer.
    MissingOffer,
    /// A WebRTC request with an unknown `channel_type`, or a multi-agent
    /// request without a `channel`.
    UnknownChannel,
    /// A logout for a `viewer_session` this stub never provisioned (or
    /// already tore down).
    UnknownSession,
    /// A multi-agent request whose `credentials` do not match the ones
    /// registered for the channel — the viewer reports `401` as "channel
    /// locked".
    BadCredentials,
}

/// What a `ProvisionVoiceAccountRequest` did, as surfaced on
/// [`ServerEvent::VoiceProvisionRequested`](crate::ServerEvent::VoiceProvisionRequested).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum VoiceProvisionOutcome {
    /// A WebRTC connection was provisioned and answered.
    WebRtcOpened {
        /// The minted `viewer_session` the client will correlate on.
        viewer_session: String,
        /// The channel it was provisioned for.
        channel: VoiceChannel,
    },
    /// A WebRTC connection was torn down (`logout: true`).
    WebRtcClosed {
        /// The `viewer_session` that was closed.
        viewer_session: String,
    },
    /// The Vivox account fixture was handed out.
    VivoxAccount,
    /// The request was refused.
    Refused(VoiceProvisionRefusal),
}

/// The voice serving store: the backend fixtures (WebRTC answerer, Vivox
/// account), the per-parcel channel table, the chat-session channel
/// credentials, and the live WebRTC connections the client provisioned.
/// Held as `SimSession::voice[_mut]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SimVoice {
    /// The WebRTC answerer, when the region speaks WebRTC.
    webrtc: Option<WebRtcStub>,
    /// The Vivox SIP account fixture, when the region speaks Vivox.
    vivox: Option<VoiceAccountInfo>,
    /// Per-parcel voice channels (`ParcelVoiceInfoRequest`), keyed by the
    /// parcel's region-local id.
    parcel_channels: BTreeMap<RegionLocalParcelId, ParcelVoiceInfo>,
    /// The parcel the agent currently stands on, whose entry the
    /// `ParcelVoiceInfoRequest` reply describes. `None` = unknown parcel
    /// (`-1` on the wire).
    agent_parcel: Option<RegionLocalParcelId>,
    /// Credentials a multi-agent provision must present, keyed by channel
    /// id. A channel with no entry accepts any credentials.
    channel_credentials: BTreeMap<String, String>,
    /// The live WebRTC connections, keyed by `viewer_session`.
    connections: BTreeMap<String, VoiceConnection>,
    /// The serial of the next `viewer_session` to mint.
    next_session: u32,
}

impl SimVoice {
    /// Enables the WebRTC backend with the given answerer identity.
    pub fn enable_webrtc(&mut self, stub: WebRtcStub) {
        self.webrtc = Some(stub);
    }

    /// Disables the WebRTC backend; live connections are dropped.
    pub fn disable_webrtc(&mut self) {
        self.webrtc = None;
        self.connections.clear();
    }

    /// The WebRTC answerer, when enabled.
    #[must_use]
    pub const fn webrtc(&self) -> Option<&WebRtcStub> {
        self.webrtc.as_ref()
    }

    /// Enables the Vivox backend with the SIP account fixture every
    /// provision request receives.
    pub fn set_vivox_account(&mut self, account: VoiceAccountInfo) {
        self.vivox = Some(account);
    }

    /// The Vivox account fixture, when set.
    #[must_use]
    pub const fn vivox_account(&self) -> Option<&VoiceAccountInfo> {
        self.vivox.as_ref()
    }

    /// The backend `SimulatorFeatures.VoiceServerType` should advertise:
    /// WebRTC when enabled, else Vivox when its fixture is set, else `None`.
    #[must_use]
    pub const fn advertised_server_type(&self) -> Option<&'static str> {
        if self.webrtc.is_some() {
            Some(VOICE_SERVER_TYPE_WEBRTC)
        } else if self.vivox.is_some() {
            Some(VOICE_SERVER_TYPE_VIVOX)
        } else {
            None
        }
    }

    /// Inserts (or replaces) a parcel's voice channel, filed under its
    /// `parcel_local_id`.
    pub fn set_parcel_voice_info(&mut self, info: ParcelVoiceInfo) {
        let _previous = self.parcel_channels.insert(info.parcel_local_id, info);
    }

    /// Removes a parcel's voice channel entry (the parcel falls back to
    /// "no voice").
    pub fn clear_parcel_voice_info(&mut self, parcel_local_id: RegionLocalParcelId) {
        let _previous = self.parcel_channels.remove(&parcel_local_id);
    }

    /// Records which parcel the agent stands on (`None` = unknown).
    pub const fn set_agent_parcel(&mut self, parcel_local_id: Option<RegionLocalParcelId>) {
        self.agent_parcel = parcel_local_id;
    }

    /// The parcel the agent stands on, as last recorded.
    #[must_use]
    pub const fn agent_parcel(&self) -> Option<RegionLocalParcelId> {
        self.agent_parcel
    }

    /// The `ParcelVoiceInfoRequest` reply for the agent's current parcel:
    /// its stored entry, or the "no voice here" form (`channel_uri` empty,
    /// `region_name` unknown) when the parcel has none.
    #[must_use]
    pub fn parcel_voice_info(&self) -> ParcelVoiceInfo {
        let parcel_local_id = self.agent_parcel.unwrap_or(RegionLocalParcelId(-1));
        self.parcel_channels
            .get(&parcel_local_id)
            .cloned()
            .unwrap_or(ParcelVoiceInfo {
                parcel_local_id,
                region_name: None,
                channel_uri: None,
                channel_credentials: None,
            })
    }

    /// Requires multi-agent provisions for `channel` to present
    /// `credentials`.
    pub fn set_channel_credentials(
        &mut self,
        channel: impl Into<String>,
        credentials: impl Into<String>,
    ) {
        let _previous = self
            .channel_credentials
            .insert(channel.into(), credentials.into());
    }

    /// The live connection for `viewer_session`, if provisioned.
    #[must_use]
    pub fn connection(&self, viewer_session: &str) -> Option<&VoiceConnection> {
        self.connections.get(viewer_session)
    }

    /// Every live connection, keyed by `viewer_session`.
    #[must_use]
    pub const fn connections(&self) -> &BTreeMap<String, VoiceConnection> {
        &self.connections
    }

    /// Serves one `ProvisionVoiceAccountRequest`: the reply body on success,
    /// the refusal otherwise, and the outcome to surface either way.
    pub(crate) fn provision(
        &mut self,
        request: &VoiceProvisionRequest,
    ) -> (
        Result<VoiceAccountInfo, VoiceProvisionRefusal>,
        VoiceProvisionOutcome,
    ) {
        let result = self.try_provision(request);
        let outcome = match &result {
            Ok(info) if request.logout => VoiceProvisionOutcome::WebRtcClosed {
                viewer_session: info.viewer_session.clone().unwrap_or_default(),
            },
            Ok(info) if info.is_webrtc() => {
                let viewer_session = info.viewer_session.clone().unwrap_or_default();
                let channel = self.connections.get(&viewer_session).map_or(
                    VoiceChannel::Spatial {
                        parcel_local_id: None,
                    },
                    |connection| connection.channel.clone(),
                );
                VoiceProvisionOutcome::WebRtcOpened {
                    viewer_session,
                    channel,
                }
            }
            Ok(_vivox) => VoiceProvisionOutcome::VivoxAccount,
            Err(refusal) => VoiceProvisionOutcome::Refused(*refusal),
        };
        (result, outcome)
    }

    /// The backend dispatch behind [`provision`](Self::provision).
    fn try_provision(
        &mut self,
        request: &VoiceProvisionRequest,
    ) -> Result<VoiceAccountInfo, VoiceProvisionRefusal> {
        match request.voice_server_type.as_deref() {
            Some(VOICE_SERVER_TYPE_WEBRTC) => self.provision_webrtc(request),
            // The oldest grids infer Vivox from an absent server type.
            None | Some(VOICE_SERVER_TYPE_VIVOX) => self
                .vivox
                .clone()
                .ok_or(VoiceProvisionRefusal::BackendUnavailable),
            Some(_other) => Err(VoiceProvisionRefusal::BackendUnavailable),
        }
    }

    /// The WebRTC half of [`provision`](Self::provision): logout, or offer
    /// in → answer out.
    fn provision_webrtc(
        &mut self,
        request: &VoiceProvisionRequest,
    ) -> Result<VoiceAccountInfo, VoiceProvisionRefusal> {
        let Some(stub) = &self.webrtc else {
            return Err(VoiceProvisionRefusal::BackendUnavailable);
        };
        if request.logout {
            let viewer_session = request.viewer_session.clone().unwrap_or_default();
            return match self.connections.remove(&viewer_session) {
                Some(_closed) => Ok(VoiceAccountInfo {
                    voice_server_type: Some(VOICE_SERVER_TYPE_WEBRTC.to_owned()),
                    viewer_session: Some(viewer_session),
                    ..VoiceAccountInfo::default()
                }),
                None => Err(VoiceProvisionRefusal::UnknownSession),
            };
        }
        let Some(offer_sdp) = request.jsep_offer_sdp.clone() else {
            return Err(VoiceProvisionRefusal::MissingOffer);
        };
        let channel = match request.channel_type.as_deref() {
            Some(VOICE_CHANNEL_TYPE_LOCAL) => VoiceChannel::Spatial {
                parcel_local_id: request.parcel_local_id,
            },
            Some(VOICE_CHANNEL_TYPE_MULTIAGENT) => {
                let Some(channel) = request.channel.clone() else {
                    return Err(VoiceProvisionRefusal::UnknownChannel);
                };
                if let Some(required) = self.channel_credentials.get(&channel)
                    && request.credentials.as_deref() != Some(required.as_str())
                {
                    return Err(VoiceProvisionRefusal::BadCredentials);
                }
                VoiceChannel::MultiAgent { channel }
            }
            _ => return Err(VoiceProvisionRefusal::UnknownChannel),
        };
        let answer_sdp = stub.answer_for(&offer_sdp);
        let viewer_session =
            Uuid::from_u128(VIEWER_SESSION_ID_BASE | u128::from(self.next_session)).to_string();
        self.next_session = self.next_session.wrapping_add(1);
        let _previous = self.connections.insert(
            viewer_session.clone(),
            VoiceConnection {
                channel,
                offer_sdp,
                answer_sdp: answer_sdp.clone(),
                ice_candidates: Vec::new(),
                ice_completed: false,
            },
        );
        Ok(VoiceAccountInfo {
            voice_server_type: Some(VOICE_SERVER_TYPE_WEBRTC.to_owned()),
            viewer_session: Some(viewer_session),
            jsep_type: Some("answer".to_owned()),
            jsep_sdp: Some(answer_sdp),
            ..VoiceAccountInfo::default()
        })
    }

    /// Records one `VoiceSignalingRequest` (ICE trickle) against its
    /// connection. Returns `false` when the `viewer_session` is unknown.
    pub(crate) fn record_signaling(
        &mut self,
        viewer_session: &str,
        candidates: &[IceCandidate],
        completed: bool,
    ) -> bool {
        let Some(connection) = self.connections.get_mut(viewer_session) else {
            return false;
        };
        connection.ice_candidates.extend_from_slice(candidates);
        connection.ice_completed |= completed;
        true
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_wire::{RegionLocalParcelId, VoiceChannelUri, VoiceProvisionRequest};

    use super::{SimVoice, VoiceChannel, VoiceProvisionOutcome, VoiceProvisionRefusal, WebRtcStub};

    /// A Firestorm-shaped audio offer (one bundled Opus section, trickle ICE).
    const OFFER: &str = "v=0\r\n\
        o=- 4611731400430051336 2 IN IP4 127.0.0.1\r\n\
        s=-\r\n\
        t=0 0\r\n\
        a=group:BUNDLE 0\r\n\
        a=extmap-allow-mixed\r\n\
        a=msid-semantic: WMS stream\r\n\
        m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n\
        c=IN IP4 0.0.0.0\r\n\
        a=rtcp:9 IN IP4 0.0.0.0\r\n\
        a=candidate:1 1 udp 2122260223 192.168.1.10 51234 typ host generation 0\r\n\
        a=ice-ufrag:viewer\r\n\
        a=ice-pwd:viewerviewerviewerviewer\r\n\
        a=ice-options:trickle\r\n\
        a=fingerprint:sha-256 AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA\r\n\
        a=setup:actpass\r\n\
        a=mid:0\r\n\
        a=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\r\n\
        a=sendrecv\r\n\
        a=msid:stream track\r\n\
        a=rtcp-mux\r\n\
        a=rtpmap:111 opus/48000/2\r\n\
        a=rtcp-fb:111 transport-cc\r\n\
        a=fmtp:111 minptime=10;useinbandfec=1\r\n\
        a=ssrc:1234 cname:abcd\r\n";

    /// The answer mirrors the offer's media section with our own ICE / DTLS
    /// identity: candidates and `ssrc`/`msid` stream lines are replaced,
    /// `setup` flips to passive, the codec and `mid` survive.
    #[test]
    fn answer_mirrors_the_offer() {
        let stub = WebRtcStub::default();
        let answer = stub.answer_for(OFFER);
        let lines: Vec<&str> = answer.lines().collect();
        assert_eq!(lines.first().copied(), Some("v=0"));
        assert!(lines.contains(&"a=group:BUNDLE 0"));
        assert!(lines.contains(&"m=audio 9 UDP/TLS/RTP/SAVPF 111"));
        assert!(lines.contains(&"a=mid:0"));
        assert!(lines.contains(&"a=rtpmap:111 opus/48000/2"));
        assert!(lines.contains(&"a=fmtp:111 minptime=10;useinbandfec=1"));
        assert!(lines.contains(&"a=sendrecv"));
        assert!(lines.contains(&"a=setup:passive"));
        assert!(lines.contains(&"a=ice-ufrag:fakegrid"));
        assert!(lines.contains(&"a=candidate:1 1 udp 2130706431 127.0.0.1 7000 typ host"));
        assert!(lines.contains(&"a=end-of-candidates"));
        assert!(!lines.iter().any(|line| line.contains("viewer")));
        assert!(!lines.iter().any(|line| line.contains("192.168.1.10")));
        assert!(!lines.iter().any(|line| line.starts_with("a=ssrc")));
        assert!(!lines.iter().any(|line| line.starts_with("a=msid:")));
        assert!(!lines.iter().any(|line| line.starts_with("a=setup:actpass")));
        assert!(answer.ends_with("\r\n"));
    }

    /// A send-only offer is answered receive-only; an offer with no media
    /// sections still yields a well-formed session-level answer.
    #[test]
    fn answer_direction_and_empty_offer() {
        let stub = WebRtcStub::default();
        let answer = stub.answer_for("v=0\r\nm=audio 9 RTP/AVP 0\r\na=sendonly\r\n");
        assert!(answer.contains("a=recvonly\r\n"));
        assert!(!answer.contains("a=sendonly"));
        let empty = stub.answer_for("");
        assert_eq!(empty, "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n");
    }

    /// Provisioning mints consecutive deterministic viewer sessions, records
    /// the connection, and logout removes it — twice is an unknown session.
    #[test]
    fn webrtc_provision_and_logout() -> Result<(), String> {
        let mut voice = SimVoice::default();
        voice.enable_webrtc(WebRtcStub::default());
        let request = VoiceProvisionRequest::webrtc(OFFER, "local", Some(RegionLocalParcelId(3)));
        let (result, outcome) = voice.provision(&request);
        let info = result.map_err(|refusal| format!("{refusal:?}"))?;
        let session = info.viewer_session.clone().ok_or("viewer session")?;
        assert_eq!(info.jsep_type.as_deref(), Some("answer"));
        assert_eq!(
            outcome,
            VoiceProvisionOutcome::WebRtcOpened {
                viewer_session: session.clone(),
                channel: VoiceChannel::Spatial {
                    parcel_local_id: Some(RegionLocalParcelId(3)),
                },
            }
        );
        let connection = voice.connection(&session).ok_or("live connection")?;
        assert_eq!(connection.offer_sdp, OFFER);
        assert_eq!(Some(&connection.answer_sdp), info.jsep_sdp.as_ref());

        let (second, _outcome) = voice.provision(&request);
        let second = second.map_err(|refusal| format!("{refusal:?}"))?;
        assert_ne!(second.viewer_session, Some(session.clone()));
        assert_eq!(voice.connections().len(), 2);

        let logout = VoiceProvisionRequest::webrtc_logout(session.clone());
        let (closed, outcome) = voice.provision(&logout);
        let closed = closed.map_err(|refusal| format!("{refusal:?}"))?;
        assert_eq!(closed.viewer_session.as_deref(), Some(session.as_str()));
        assert_eq!(
            outcome,
            VoiceProvisionOutcome::WebRtcClosed {
                viewer_session: session.clone(),
            }
        );
        assert!(voice.connection(&session).is_none());
        let (again, outcome) = voice.provision(&logout);
        assert_eq!(again, Err(VoiceProvisionRefusal::UnknownSession));
        assert_eq!(
            outcome,
            VoiceProvisionOutcome::Refused(VoiceProvisionRefusal::UnknownSession)
        );
        Ok(())
    }

    /// Multi-agent provisions are gated by the channel's registered
    /// credentials; unknown channel types and missing offers are refused;
    /// a backend that is not enabled is unavailable.
    #[test]
    fn refusals() -> Result<(), String> {
        let mut voice = SimVoice::default();
        let webrtc = VoiceProvisionRequest::webrtc(OFFER, "local", None);
        assert_eq!(
            voice.provision(&webrtc).0,
            Err(VoiceProvisionRefusal::BackendUnavailable)
        );
        assert_eq!(
            voice.provision(&VoiceProvisionRequest::vivox()).0,
            Err(VoiceProvisionRefusal::BackendUnavailable)
        );
        voice.enable_webrtc(WebRtcStub::default());
        voice.set_channel_credentials("room-1", "secret");
        let good =
            VoiceProvisionRequest::webrtc_channel(OFFER, "room-1", Some("secret".to_owned()));
        let _opened = voice
            .provision(&good)
            .0
            .map_err(|refusal| format!("{refusal:?}"))?;
        let bad = VoiceProvisionRequest::webrtc_channel(OFFER, "room-1", Some("nope".to_owned()));
        assert_eq!(
            voice.provision(&bad).0,
            Err(VoiceProvisionRefusal::BadCredentials)
        );
        let open = VoiceProvisionRequest::webrtc_channel(OFFER, "room-2", None);
        let _opened = voice
            .provision(&open)
            .0
            .map_err(|refusal| format!("{refusal:?}"))?;
        let odd = VoiceProvisionRequest::webrtc(OFFER, "estate", None);
        assert_eq!(
            voice.provision(&odd).0,
            Err(VoiceProvisionRefusal::UnknownChannel)
        );
        let no_offer = VoiceProvisionRequest {
            jsep_offer_sdp: None,
            ..VoiceProvisionRequest::webrtc(OFFER, "local", None)
        };
        assert_eq!(
            voice.provision(&no_offer).0,
            Err(VoiceProvisionRefusal::MissingOffer)
        );
        Ok(())
    }

    /// Trickled candidates accumulate on their connection; an unknown
    /// session is reported, not created.
    #[test]
    fn signaling_accumulates() -> Result<(), String> {
        let mut voice = SimVoice::default();
        voice.enable_webrtc(WebRtcStub::default());
        let (info, _outcome) =
            voice.provision(&VoiceProvisionRequest::webrtc(OFFER, "local", None));
        let session = info
            .map_err(|refusal| format!("{refusal:?}"))?
            .viewer_session
            .ok_or("session")?;
        let candidate = sl_wire::IceCandidate {
            sdp_mid: "0".to_owned(),
            sdp_mline_index: 0,
            candidate: "candidate:1 1 udp 1 10.0.0.1 5000 typ host".to_owned(),
        };
        assert!(voice.record_signaling(&session, std::slice::from_ref(&candidate), false));
        assert!(voice.record_signaling(&session, &[], true));
        let connection = voice.connection(&session).ok_or("connection")?;
        assert_eq!(connection.ice_candidates, vec![candidate]);
        assert!(connection.ice_completed);
        assert!(!voice.record_signaling("nope", &[], true));
        Ok(())
    }

    /// The parcel reply follows the agent's parcel: a stored entry verbatim,
    /// otherwise the "no voice" form for that parcel id (or `-1`).
    #[test]
    fn parcel_voice_info_follows_the_agent() {
        let mut voice = SimVoice::default();
        assert_eq!(
            voice.parcel_voice_info().parcel_local_id,
            RegionLocalParcelId(-1)
        );
        let region = uuid::Uuid::from_u128(0x1e6);
        voice.set_parcel_voice_info(sl_wire::ParcelVoiceInfo {
            parcel_local_id: RegionLocalParcelId(7),
            region_name: None,
            channel_uri: Some(VoiceChannelUri::Id(region)),
            channel_credentials: None,
        });
        voice.set_agent_parcel(Some(RegionLocalParcelId(7)));
        assert_eq!(
            voice.parcel_voice_info().channel_uri,
            Some(VoiceChannelUri::Id(region))
        );
        voice.set_agent_parcel(Some(RegionLocalParcelId(8)));
        let quiet = voice.parcel_voice_info();
        assert_eq!(quiet.parcel_local_id, RegionLocalParcelId(8));
        assert_eq!(quiet.channel_uri, None);
        voice.clear_parcel_voice_info(RegionLocalParcelId(7));
        voice.set_agent_parcel(Some(RegionLocalParcelId(7)));
        assert_eq!(voice.parcel_voice_info().channel_uri, None);
    }

    /// The advertised server type prefers WebRTC, then Vivox, then nothing.
    #[test]
    fn advertised_server_type() {
        let mut voice = SimVoice::default();
        assert_eq!(voice.advertised_server_type(), None);
        voice.set_vivox_account(sl_wire::VoiceAccountInfo::default());
        assert_eq!(voice.advertised_server_type(), Some("vivox"));
        voice.enable_webrtc(WebRtcStub::default());
        assert_eq!(voice.advertised_server_type(), Some("webrtc"));
        voice.disable_webrtc();
        assert_eq!(voice.advertised_server_type(), Some("vivox"));
    }
}
