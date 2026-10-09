//! Which live grid this one is imitating.
//!
//! The fake grid exists to fail a viewer the way a real grid would, and there
//! are two real grids that do not agree. Where they differ, one of them has to
//! be picked — and picking it per behaviour, as this crate did until
//! [`ImitatedGrid`] existed, produces a grid that is nobody: a stock fake grid
//! announced `platform: OpenSim`, kept every login field like OpenSim, and
//! withheld a taken object's asset like Second Life, all at once. A viewer
//! passing against that has not been tested against anything.
//!
//! So the grid names the live one it is being, once
//! ([`FakeGridBuilder::imitates`](crate::FakeGridBuilder::imitates)), and every
//! divergent behaviour takes its default from that. A test that wants one
//! deviation still sets that one knob directly; the flavour is what the knob
//! falls back to, not a lock.
//!
//! # What the flavour decides today
//!
//! | behaviour | Second Life | OpenSim |
//! | --- | --- | --- |
//! | a taken object's asset ([`ObjectAssetPolicy`]) | withheld: nil id, an unfetchable Linden-text body | served: minted id, a `<SceneObjectGroup>` XML body under it |
//! | the login response's `options` list ([`honor_options`](crate::FakeGridBuilder::honor_options)) | honoured: the response is trimmed to what was asked for | ignored: every field is sent |
//! | the `OpenSimExtras` block in `SimulatorFeatures` ([`advertises_open_sim_extras`](ImitatedGrid::advertises_open_sim_extras)) | absent | sent, carrying the grid's map-tile and currency-helper URLs |
//! | the rest of `SimulatorFeatures` ([`stock_simulator_features`](ImitatedGrid::stock_simulator_features)) | 29 keys: the PBR, mirror and pathfinding switches, the estate and group limits, the dead-reckoning pair, the host's name, a 2048 px texture limit, four material requests a second | 14 keys and the 24 of `OpenSimExtras`: the grid's name and limits, the simulator's frame rate, `ExportSupported` as a string; five empty `menus`; no texture limit, three material requests a second |
//! | the spatial-voice backend ([`VoiceBackend`]) | WebRTC, named two ways: `SimulatorFeatures.VoiceServerType` and the `RequiredVoiceVersion` push — **not** the login `voice-config`, which aditi does not send even when asked (2026-10-04) | none: a stock region loads no voice module, and nothing is advertised |
//! | the deprecated UDP inventory fetch ([`LegacyUdpInventory`]) | refused with a `FeatureDisabled` | served out of the session's inventory tree |
//! | how a **taken** item is announced ([`InventoryAnnouncement`]) | a `BulkUpdateInventory` over the event queue | the legacy UDP `UpdateCreateInventoryItem` |
//! | how an item a capability upload **rewrote** is announced ([`UploadAnnouncements::saved`]) | the legacy UDP `UpdateCreateInventoryItem` | nothing: the capability's HTTP response is the whole answer |
//! | how an item a capability upload **created** is announced ([`UploadAnnouncements::created`]) | nothing | nothing |
//! | who composites an avatar ([`BakePolicy`]) | the grid: an `agent_appearance_service`, the central-bake protocol bit, an `AppearanceData` block on every appearance, and the `UpdateAvatarAppearance` trigger | every viewer for itself: none of those four |
//! | whether an update capability's completion names the item it rewrote ([`UpdateCompletionItem`]) | omitted: `new_asset` alone, and the client uses the id it sent | echoed: `new_inventory_item` carries the rewritten item |
//! | the rest of `RegionProtocols` ([`region_protocol_bits`](ImitatedGrid::region_protocol_bits)) | nothing else claimed | bit 63, "more than 6 baked textures" |
//! | the `EconomyData` price list ([`prices`](ImitatedGrid::prices)) | measured on aditi: L$ 10 an upload, L$ 100 a group, a 20 000 LI region | its `SampleMoneyModule` defaults: most prices free, no group price stated, a 15 000 LI region |
//! | the capabilities the seed refuses ([`withheld_capabilities`](ImitatedGrid::withheld_capabilities)) | `ObjectAnimation`, `UploadBakedTexture` | 34 of ours: AIS3, the library fetches, experiences, voice, group invites, offline messages, the bake trigger, the interest-list switch |
//! | the login response's fields beyond the `options` list ([`login_fields`](ImitatedGrid::login_fields)) | no `home`, no region size; `max-agent-groups` from the account's package | `home` and the region size; `max-agent-groups` fixed at 42 |
//! | how a login is refused, and a second login of an avatar in world ([`login_refusals`](ImitatedGrid::login_refusals)) | `key` with a localisation key, its (empty) arguments and an incident id; the second login is admitted and the first session kicked | `key` with a text and nothing else; the second login is refused as `presence` and the first session kicked all the same |
//! | whether a logout is answered ([`logout_reply`](ImitatedGrid::logout_reply)) | a `LogoutReply`, always | none: the session closes with the request unanswered and unacknowledged, which is what the live grid does six times in seven |
//! | a circuit the client stops answering ([`circuit_policy`](ImitatedGrid::circuit_policy)) | an unacknowledged packet is sent four times, a second apart, and given up; a silent client is dropped after 100 s without a word | an unacknowledged packet is resent for as long as the circuit lasts; a silent client is kicked after 60 s |
//! | how an `ObjectUpdate` is put on the wire ([`CircuitPolicy::short_zero_tails`]) | zero-coded, its final run of zeros one short | as encoded |
//! | what a region says of itself on arrival ([`arrival_policy`](ImitatedGrid::arrival_policy)) | a product name, SKU and data centre; a `HealthMessage` and an `AgentStateUpdate`; two handshakes down each child circuit; `SimStats` every 2 s and the time every 10 s, with a sun direction | none of the three names; neither message; one handshake; `SimStats` every 3 s with six more statistics and the time every 2.55 s, with no sun direction |
//! | the account's entitlements ([`describes_account_entitlements`](ImitatedGrid::describes_account_entitlements)) | a benefits package, its subscription name, every package's numbers, and the maturity preference | none of the four; a viewer prices uploads from the legacy `EconomyData` instead |
//! | how a teleport runs and how it is refused ([`teleport_policy`](ImitatedGrid::teleport_policy)) | `resolving` and `Sending to destination.` between the start and the finish; a refusal after the start, over the event queue, as a key with an `AlertInfo`; a cancel answered `TPCancelled`; a region above the maturity preference refused; a local teleport flagged `WITHIN_REGION` | no progress lines; a refusal before any start, over UDP, as a sentence with no alert; a cancel unanswered; no maturity check; the request's flags alone |
//! | a parcel listing's flags for an adult region ([`ParcelPolicy::adult_listing_bits`]) | the adult and the mature bit, `0x03` | the adult bit alone, `0x02` |
//! | the world map ([`map_policy`](ImitatedGrid::map_policy)) | every empty cell of a null-sims rectangle reported; a rectangle of more than 256 cells unanswered; a name search by prefix, of any length, in silence; no map layers; an absent tile refused with `403` | an empty cell reported only when asked about alone; any rectangle answered; a search anywhere in the name, of three characters or more, with an alert for a short one and for no match; one whole-grid layer; a blank tile for an absent one |
//! | how a region's ground and wind are sent ([`terrain_policy`](ImitatedGrid::terrain_policy)) | the ground outwards from where the agent stands, a message kept within 1,200 bytes, every patch transformed; the wind with the ground and then every second, unreliably, under a stride of 18 with six bits of prequantization | the ground outwards from the patch the agent is in, a message closed once past 890 bytes, a flat patch as a header alone; the wind every 13.6 s on the region's own clock, reliably, in the ground's encoding |
//! | an arriving agent's own appearance ([`BakePolicy::sends_own_appearance`](crate::BakePolicy::sends_own_appearance)) | sent only to a viewer whose handshake reply set `SUPPORTS_SELF_APPEARANCE` | sent whatever the reply's flags |
//! | the interest-list switch (`InterestList`, in [`withheld_capabilities`](ImitatedGrid::withheld_capabilities)) | granted, and answered with the mode and the one it replaced | not granted |
//! | a sit on an object the region does not have ([`SitPolicy::unknown_target`]) | refused at once with the named alert `SitFailNotSameRegion` | not answered: the client's own sit timeout ends it |
//! | where standing up puts the avatar ([`SitPolicy::stand_forward_m`], [`SitPolicy::stand_up_m`]) | 0.34 m in front of where it sat, at the same height | 0.65 m in front and 0.57 m above |
//! | the seat position in an `AvatarSitResponse` for a seat with a sit target ([`SitPolicy::response_states_seated_position`]) | where the avatar is put: the target raised by 0.35 m | the target as the script set it, 0.35 m below where the avatar is put |
//! | who is sent an object's record when its name or description is edited ([`PropertiesPolicy::rename_answered`]) | the editor, selected or not | nobody |
//! | who is sent it when something is written into its contents ([`PropertiesPolicy::contents_change_told_to`]) | every session holding it selected, the writer among them or not | the writer, selected or not |
//! | what a select and a deselect bring besides the record ([`PropertiesPolicy::strangers_select_resends_object`], [`PropertiesPolicy::deselect_sends_terse`]) | a deselect brings a terse update of the object | a select of somebody else's object brings it again in full |
//! | whose record a link sends the linker ([`PropertiesPolicy::link_sends`]) | each linked child's | the root's |
//! | a child prim's sale state in its record ([`PropertiesPolicy::child_sale`]) | its own | the root's |
//! | a new prim's record ([`PropertiesPolicy::new_prim_owner_mask`], [`PropertiesPolicy::new_prim_last_owner`], [`PropertiesPolicy::ownership_cost`]) | base and owner masks `0x7fffffff`, no last owner, an ownership cost of 10 | masks `0x0009e000`, the rezzer as last owner, a cost of 0 |
//!
//! **The inventory rows are the divergence a viewer is most likely to trip
//! over**, which is why they are three rows rather than one setting. An
//! inventory implementation that still reaches for the UDP fetch, or that only
//! listens for the legacy create, works against OpenSim and fails against the
//! grid this workspace targets — silently, in both directions.
//!
//! **And the two announcement rows do not point the same way**, which is the
//! part worth reading twice: on a take Second Life is the grid that pushes a
//! `BulkUpdateInventory` and OpenSim the one that sends the legacy message,
//! while after a capability upload it is Second Life that sends the legacy
//! message and OpenSim that sends nothing at all. They are two enums because
//! they are two measurements, taken in the [`inventory`](crate::inventory)
//! module docs. Second Life's refusal is
//! the one deliberate deviation from the measurement in this table: aditi
//! empirically *drops* the fetch without a word (2026-08-12), and
//! [`LegacyUdpInventory::Ignored`] reproduces that, but of the two roads a grid
//! without the path has only the refusal leaves something to assert — silence
//! is indistinguishable from a lost packet.
//!
//! **The map and currency URLs survive losing the extras block**, which is the
//! part that had to be checked rather than assumed. Both are reachable by a
//! route that is not `OpenSimExtras` and that both grids serve: the map-tile
//! server through the login response's `map-server-url`
//! (`LLStartUp::process_login_success_response` reads it there and
//! `LFSimFeatureHandler` only *overrides* it from the extras), and the
//! currency helper base through `get_grid_info`'s `economy` key, which is where
//! `LLGridManager::getHelperURI` reads it when no extras block overrode it.
//! Dropping the block on the Second Life side therefore hides no URL — it
//! removes a *second* copy of them.
//!
//! The currency **symbol** is not a divergence at all: neither grid names one.
//! A stock OpenSim grid puts none in the login response or the extras block,
//! and aditi sends no `currency` field even when the request asks for the
//! option (`login-options`, 2026-10-04) — so a viewer shows its own default on
//! both (`L$` on Second Life, `OS$` for Firestorm on OpenSim).
//!
//! **The bake pair is the divergence that cost the most to derive**, and the
//! reason it is a policy type rather than a boolean: withdrawing the appearance
//! service on its own is *worse* than leaving it, because a viewer that has
//! already decided an avatar is server-baked then asks for no bake at all and
//! leaves it a cloud with nothing in the log. Four things move together or none
//! of them may — see the [`bakes`](crate::bakes) module docs for the four and
//! for where each side of each was measured.
//!
//! **The price row is the one measurement that changes a viewer's arithmetic
//! rather than its plumbing**, which is why it is worth a second look: a
//! Second-Life-flavoured grid quotes L$ 10 for an upload and OpenSim quotes
//! free, so a viewer that hard-codes either — or that sends an
//! `expected_upload_cost` copied from the wrong grid — passes against one and
//! is refused by the other. OpenSim is also the only grid of the two that
//! declines to quote a price at all, which is why
//! [`EconomyData::price_group_create`](sl_proto::EconomyData::price_group_create)
//! is an `Option`.
//!
//! # What it does not decide yet, and why
//!
//! Nothing, today. Every divergence this crate has measured is derived here.
//! The economy was the last one outstanding: the price list
//! ([`prices`](ImitatedGrid::prices)). The currency symbol was thought to be a
//! second row until aditi was measured sending none either. What is left of
//! [`EconomyConfig`](crate::EconomyConfig) — the L$-to-dollars rate, whether
//! the site is up, whether buying land demands an upgrade, the confirm token —
//! is deliberately *not* flavour policy but test policy, set per test.
//!
//! One thing is deliberately **not** flavour-decided and is not a to-do:
//! [`GridIdentity::platform`](crate::GridIdentity) stays `OpenSim` whichever
//! grid is being imitated. It is not protocol behaviour — it is what Firestorm's
//! grid manager reads to decide whether it will add the grid at all, and a fake
//! grid nothing can log into tests nothing.

use std::time::Duration;

use crate::assets::ObjectAssetPolicy;
use crate::bakes::{BakePolicy, REGION_PROTOCOL_BAKES_ON_MESH};
use crate::inventory::{
    InventoryAnnouncement, LegacyUdpInventory, UploadAnnouncement, UploadAnnouncements,
};
use crate::login_sections::LoginSections;
use crate::uploads::UpdateCompletionItem;
use crate::voice::VoiceBackend;

/// The live grid a [`FakeGrid`](crate::FakeGrid) imitates where the two real
/// ones disagree.
///
/// The default is [`SecondLife`](Self::SecondLife): it is the grid this
/// workspace targets, and it is the stricter of the two almost everywhere the
/// pair has been measured — so it is the flavour that catches a viewer relying
/// on something only OpenSim allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImitatedGrid {
    /// Second Life, which this workspace targets.
    #[default]
    SecondLife,
    /// OpenSim, which this workspace treats as a safe grid to test against.
    OpenSim,
}

impl ImitatedGrid {
    /// Whether an **update** capability's completion names the item it rewrote.
    ///
    /// OpenSim echoes it; Second Life does not, and the reference client never
    /// asks it to. See [`UpdateCompletionItem`] for both sources — it is the
    /// divergence a client is most likely to depend on **without noticing**,
    /// because the lenient answer makes a broken correlation work.
    #[must_use]
    pub const fn update_completion_item(self) -> UpdateCompletionItem {
        match self {
            Self::SecondLife => UpdateCompletionItem::Omitted,
            Self::OpenSim => UpdateCompletionItem::Echoed,
        }
    }

    /// What this grid does with a taken object's asset.
    ///
    /// Second Life gives a viewer a nil asset id and no way to reach the body;
    /// OpenSim names a minted id and serves the body under it. Measured on
    /// aditi 2026-09-06 — see [`ObjectAssetPolicy`] for the numbers.
    ///
    /// The **format** follows from the same choice, because the class is two
    /// formats on the two grids: a withheld body is the Linden text and a
    /// served one is OpenSim's `<SceneObjectGroup>` XML.
    #[must_use]
    pub const fn object_assets(self) -> ObjectAssetPolicy {
        match self {
            Self::SecondLife => ObjectAssetPolicy::Withheld,
            Self::OpenSim => ObjectAssetPolicy::Served,
        }
    }

    /// Whether this grid trims a login response to the `options` list the
    /// request asked for.
    ///
    /// Second Life does; OpenSim sends every field it has whatever was asked
    /// for. A viewer that reads a field it never requested works against one
    /// grid and not the other, which is the whole reason this is observable.
    #[must_use]
    pub const fn honors_login_options(self) -> bool {
        matches!(self, Self::SecondLife)
    }

    /// The capabilities this grid's seed **refuses** of the ones a client
    /// here asks for — measured by `seed-capabilities` on aditi and the local
    /// OpenSim (2026-10-04, `book/src/gridspec/capabilities.md`). A fake grid
    /// imitating it withholds every one it would otherwise grant, so a client
    /// takes the path it would take on the live grid: no AIS3 and no
    /// experiences on OpenSim, no baked-texture upload on Second Life.
    ///
    /// `ObjectAnimation` is refused by both and served by neither fake
    /// flavour; it is listed for completeness.
    #[must_use]
    pub const fn withheld_capabilities(self) -> &'static [&'static str] {
        match self {
            Self::SecondLife => SECOND_LIFE_REFUSED_CAPABILITIES,
            Self::OpenSim => OPENSIM_REFUSED_CAPABILITIES,
        }
    }

    /// What a region of `product` holds on this grid: its land impact budget
    /// and how many agents it admits.
    ///
    /// Second Life's limits start from the product. A Homestead holds 7 500
    /// land impact and 20 agents, an Openspace 1 000 and 15. A Full Region is
    /// one product name over a range — most commonly 20 000 to 30 000 land
    /// impact and 33 to 44 agents on small mainland regions, up to 175 on event
    /// regions — so this answers with the common low end and a region that is
    /// something else says so itself ([`RegionConfig::capacity`](crate::RegionConfig::capacity)).
    /// These are the product limits as the grid's residents know them,
    /// not a measurement: the aditi test avatars can reach one sandbox, whose
    /// parcel reports its own prim bonus. OpenSim has no products: every region
    /// is `RegionInfo`'s default 15 000 prims and 40 agents (the same local
    /// measurement the price list's `object_capacity` comes from).
    #[must_use]
    pub const fn region_capacity(self, product: sl_proto::ProductType) -> RegionCapacity {
        match self {
            Self::SecondLife => match product {
                sl_proto::ProductType::Homestead => RegionCapacity {
                    land_impact: 7_500,
                    max_agents: 20,
                    hard_max_agents: 20,
                },
                sl_proto::ProductType::Openspace => RegionCapacity {
                    land_impact: 1_000,
                    max_agents: 15,
                    hard_max_agents: 15,
                },
                // A Full Region, and anything this crate does not know yet.
                _ => RegionCapacity {
                    land_impact: 20_000,
                    max_agents: 40,
                    hard_max_agents: 100,
                },
            },
            Self::OpenSim => RegionCapacity {
                land_impact: 15_000,
                max_agents: 40,
                hard_max_agents: 100,
            },
        }
    }

    /// How this grid labels the day of a region nobody set an environment on:
    /// the cycle's name and the environment version it reports.
    ///
    /// OpenSim's is called `Default` and is version 0. Second Life's default
    /// has no one name to measure — the aditi region reached serves a named
    /// day somebody chose, at version 1 — so its flavour keeps the fake grid's
    /// own name at version 1. Both grids' default day is 14 400 s long, offset
    /// 57 600 s, with eight sky keyframes on the ground track and one water
    /// frame; the fake grid's stays a single keyframe on purpose, so a render
    /// capture does not depend on the region clock
    /// (`EnvironmentSettings::default_region`).
    #[must_use]
    pub const fn stock_day(self) -> StockDay {
        match self {
            Self::SecondLife => StockDay {
                name: "Default Daycycle",
                env_version: 1,
            },
            Self::OpenSim => StockDay {
                name: "Default",
                env_version: 0,
            },
        }
    }

    /// What this grid answers an accepted `ExtEnvironment` set or reset with.
    ///
    /// OpenSim sends a bare verdict: `{success: true}` to a PUT, the region and message ids
    /// beside it to a DELETE (measured by `environment` as the local grid's
    /// estate owner, 2026-10-05, `book/src/gridspec/environment.md`). A client
    /// that waits for the stored settings in the reply waits for ever there.
    /// Second Life's answer to an accepted change is unmeasured — the aditi
    /// test avatars may change no land — and is taken to be the stored
    /// settings the reference viewer reads from it.
    #[must_use]
    pub const fn environment_change_reply(self) -> EnvironmentChangeReply {
        match self {
            Self::SecondLife => EnvironmentChangeReply::Settings,
            Self::OpenSim => EnvironmentChangeReply::Bare,
        }
    }

    /// How this grid answers the About Land traffic where the two disagree —
    /// measured by `parcel-edit` and `parcel-edit-refused` on aditi and the
    /// local OpenSim (2026-10-05, `book/src/gridspec/land.md`).
    #[must_use]
    pub const fn parcel_policy(self) -> ParcelPolicy {
        match self {
            Self::SecondLife => ParcelPolicy {
                answers_request_by_id: true,
                edit_echo: EditEcho::Fixed(SELECTED_PARCEL_SEQUENCE_ID),
                udp_edit_nulls_media_type: false,
                unset_media_loops: true,
                sends_extended_blocks: true,
                parcel_return_reads_task_ids: true,
                owners_reply_over_event_queue: true,
                wire_types: sl_proto::ParcelLlsdDialect::SecondLife,
                adult_listing_bits: sl_proto::AdultListingBits::AdultAndMature,
                list_update_sets_use_flag: false,
                empty_list_placeholder_names_its_list: false,
            },
            Self::OpenSim => ParcelPolicy {
                answers_request_by_id: false,
                edit_echo: EditEcho::LastSequence,
                udp_edit_nulls_media_type: true,
                unset_media_loops: false,
                sends_extended_blocks: false,
                parcel_return_reads_task_ids: false,
                owners_reply_over_event_queue: false,
                wire_types: sl_proto::ParcelLlsdDialect::OpenSim,
                adult_listing_bits: sl_proto::AdultListingBits::AdultOnly,
                list_update_sets_use_flag: true,
                empty_list_placeholder_names_its_list: true,
            },
        }
    }

    /// The login response's fields that differ by grid **whatever the
    /// `options` list asked for** — measured by `login-options` on aditi and
    /// the local OpenSim (2026-10-04, `book/src/gridspec/login.md`).
    #[must_use]
    pub const fn login_fields(self) -> LoginFields {
        match self {
            Self::SecondLife => LoginFields {
                home: false,
                region_size: false,
                max_agent_groups: None,
                sections: LoginSections::SECOND_LIFE,
            },
            Self::OpenSim => LoginFields {
                home: true,
                region_size: true,
                max_agent_groups: Some(OPENSIM_MAX_AGENT_GROUPS),
                sections: LoginSections::OPENSIM,
            },
        }
    }

    /// How this grid refuses a login, and what it does with a second login of
    /// an avatar that is already in world — measured by `login-refusals` on
    /// aditi and the local OpenSim (2026-10-06, `book/src/gridspec/login.md`
    /// § Refusals).
    #[must_use]
    pub const fn login_refusals(self) -> LoginRefusals {
        match self {
            Self::SecondLife => LoginRefusals {
                bad_credentials_message: "Sorry! We couldn't log you in.\n\nPlease check to make \
                    sure you entered the right\n\n    * Username (like bobsmith12 or \
                    steller.sunshine)\n\n    * Password\n\n    * Second Factor Token (if \
                    enabled)\n\nAlso, please make sure your Caps Lock key is off. If you feel \
                    this is an error, please contact support@secondlife.com.",
                bad_credentials_message_id: Some("LoginFailedAuthenticationFailed"),
                stamps_error_code: true,
                already_logged_in_message: sl_wire::LoginServer::ALREADY_LOGGED_IN_MESSAGE,
                second_login: SecondLogin::Admitted,
                second_login_kick_reason: "The system has logged you out because you are \
                    attempting to log in from another location.",
            },
            Self::OpenSim => LoginRefusals {
                bad_credentials_message: "Could not authenticate your avatar. Please check your \
                    username and password, and check the grid if problems persist.",
                bad_credentials_message_id: None,
                stamps_error_code: false,
                // The doubled "a a" and the trailing space are OpenSim's own.
                already_logged_in_message: "You appear to be already logged in. Please wait a a \
                    minute or two and retry. If this takes longer than a few minutes please \
                    contact the grid owner. ",
                second_login: SecondLogin::Refused,
                second_login_kick_reason: "New login detected",
            },
        }
    }

    /// Whether this grid answers a `LogoutRequest` — measured by
    /// `logout-clean` and a run of REPL probes on aditi and the local OpenSim
    /// (2026-10-06, `book/src/gridspec/session.md` § Logout).
    ///
    /// Second Life answered every logout, in about 0.17 s. OpenSim's answer is
    /// a race it usually loses: it queues the reply and closes the agent, which
    /// clears the queue — the reply reached the wire in 5 of 35 logouts, and in
    /// none of the 15 made within two seconds of arriving. The flavour takes the
    /// usual outcome, which is also the one a client has to be built for: with
    /// the reply the two grids look alike, without it the client is on its own
    /// timeout.
    #[must_use]
    pub const fn logout_reply(self) -> LogoutReply {
        match self {
            Self::SecondLife => LogoutReply::Sent,
            Self::OpenSim => LogoutReply::Withheld,
        }
    }

    /// How this grid runs a circuit — measured by `keepalive-ping`,
    /// `circuit-unacked-resend` and `circuit-silence` on aditi and the local
    /// OpenSim (2026-10-06, `book/src/gridspec/session.md` § Circuits).
    ///
    /// Second Life's simulators ran on the reference library's numbers: an
    /// unacknowledged reliable packet went out four times, a second apart,
    /// and was then given up with the circuit none the worse; a client that
    /// stopped transmitting was pinged every five seconds for 95 more and
    /// dropped at 100 without a word. OpenSim resent the same packet 140
    /// times in 45 seconds and would not have stopped — its retransmission
    /// timeout is five times the last ping, held between 250 ms and 3 s
    /// (`LLUDPClient.UpdateRoundTrip`), and nothing counts the resends — and
    /// kicked the silent client at 60 seconds (`AckTimeout`).
    #[must_use]
    pub const fn circuit_policy(self) -> CircuitPolicy {
        match self {
            Self::SecondLife => CircuitPolicy {
                link: sl_proto::LinkTuning {
                    inactivity_timeout: Duration::from_secs(100),
                    // Twelve seconds after an arrival the resends were already
                    // a second apart, which the reference's one-second initial
                    // average takes most of a minute to decay to: the
                    // simulator's starts low.
                    ping_initial: Duration::from_millis(100),
                    ..sl_proto::LinkTuning::REFERENCE
                },
                timeout_kick: None,
                short_zero_tails: true,
            },
            Self::OpenSim => CircuitPolicy {
                link: sl_proto::LinkTuning {
                    inactivity_timeout: Duration::from_secs(60),
                    max_transmissions: None,
                    resend_floor: Duration::from_millis(250),
                    ping_initial: Duration::from_millis(50),
                    ping_min: Duration::from_millis(50),
                    ping_max: Duration::from_millis(600),
                },
                timeout_kick: Some("Simulator logged you out due to connection timeout."),
                short_zero_tails: false,
            },
        }
    }

    /// How this grid runs a teleport and how it refuses one — measured by
    /// scripted `sl-repl` probes and the `teleport-*` conformance cases on
    /// aditi and the local OpenSim (2026-10-07,
    /// `book/src/gridspec/teleport.md`). The lure rows are from the
    /// `teleport-offer-*` and `teleport-lure-*` cases of the same day.
    ///
    /// The two grids agree on the skeleton (`TeleportStart`, then either a
    /// `TeleportLocal` or an event-queue `TeleportFinish`) and on nothing
    /// around it. Second Life narrates: `resolving`, then the literal sentence
    /// `Sending to destination.`, after a `sending_home` / `sending_landmark`
    /// for those two kinds. OpenSim sends no progress line at all. Second
    /// Life refuses *after* the start and its first lines, over the event
    /// queue, with a key (`no_host`, `nolandmark_tport`) repeated in an
    /// `AlertInfo`; OpenSim refuses *instead of* starting, over UDP, with an
    /// English sentence and no alert.
    #[must_use]
    pub const fn teleport_policy(self) -> TeleportPolicy {
        match self {
            Self::SecondLife => TeleportPolicy {
                progress: &["resolving", "Sending to destination."],
                names_the_kind: true,
                local_flags: sl_types::map::TeleportFlags::WITHIN_REGION,
                local_look_at: LocalLookAt::TowardsRegionOrigin,
                finish_location_id: sl_proto::TELEPORT_FINISH_LOCATION_ID_SECOND_LIFE,
                finish_states_region_size: false,
                refusals: RefusalTransport::EventQueue,
                unknown_region: "no_host",
                unknown_landmark: "nolandmark_tport",
                no_home: sl_proto::teleport_strings::INVALID_TPORT,
                unknown_lure: None,
                lure_line: Some(sl_proto::teleport_strings::COMPLETING),
                finish_flags: FinishFlags::TheKind,
                cancel: CancelAnswer::Failed {
                    reason: "Teleport cancelled.",
                    alert: "TPCancelled",
                },
                enforces_maturity_preference: true,
            },
            Self::OpenSim => TeleportPolicy {
                progress: &[],
                names_the_kind: false,
                local_flags: 0,
                local_look_at: LocalLookAt::Requested,
                finish_location_id: sl_proto::TELEPORT_FINISH_LOCATION_ID,
                finish_states_region_size: true,
                refusals: RefusalTransport::Udp,
                unknown_region: "The region you tried to teleport to was not found",
                unknown_landmark: "Could not find the landmark asset data",
                no_home: "Home set not",
                unknown_lure: Some("The region you tried to teleport to was not found"),
                lure_line: None,
                finish_flags: FinishFlags::ViaLocation,
                cancel: CancelAnswer::Ignored,
                enforces_maturity_preference: false,
            },
        }
    }

    /// How this grid answers a sit request where the two disagree — measured
    /// by the `sit-stand` conformance case on aditi and the local OpenSim
    /// (2026-10-08, `book/src/gridspec/movement.md` § Sitting).
    ///
    /// They agree on more than they differ in: both answer a sit on a seat
    /// with an `AvatarSitResponse` whose `AutoPilot` flag is set whatever the
    /// distance, and both have the avatar on the seat before the client's
    /// `AgentSit` can have arrived. What a client can tell them apart by is
    /// the answer to a sit on nothing, and which of two positions the
    /// response carries for a scripted seat.
    #[must_use]
    pub const fn sit_policy(self) -> SitPolicy {
        match self {
            Self::SecondLife => SitPolicy {
                unknown_target: Some(SitRefusal {
                    name: "SitFailNotSameRegion",
                    text: "Try moving closer.  Can't sit on object because\n\
                           it is not in the same region as you.",
                }),
                response_states_seated_position: true,
                stand_forward_m: 0.34,
                stand_up_m: 0.0,
            },
            Self::OpenSim => SitPolicy {
                unknown_target: None,
                response_states_seated_position: false,
                stand_forward_m: 0.65,
                stand_up_m: 0.57,
            },
        }
    }

    /// How this grid answers a select and tells of a change to an object's
    /// record where the two disagree — measured by the `object-properties`
    /// conformance case on aditi and the local OpenSim (2026-10-09,
    /// `book/src/gridspec/objects.md` § Properties).
    ///
    /// They agree on the outline: a select is answered with the record and
    /// with the object's physics record over the event queue, whoever owns
    /// the object and however often it is selected; a select of a linkset's
    /// root is answered for the root alone; a family request about a child
    /// prim is answered with the root's record; a child's record carries the
    /// root's permission masks; the creation date is in microseconds; and no
    /// edit of a name, a description, a price or a permission is told to
    /// anybody but the session that made it.
    #[must_use]
    pub const fn properties_policy(self) -> PropertiesPolicy {
        match self {
            Self::SecondLife => PropertiesPolicy {
                rename_answered: true,
                contents_change_told_to: ContentsAudience::Selectors,
                strangers_select_resends_object: false,
                deselect_sends_terse: true,
                link_sends: LinkedRecord::Children,
                child_sale: ChildSale::Own,
                new_prim_owner_mask: sl_wire::Permissions::ALL,
                new_prim_last_owner: LastOwner::Nobody,
                ownership_cost: 10,
            },
            Self::OpenSim => PropertiesPolicy {
                rename_answered: false,
                contents_change_told_to: ContentsAudience::Writer,
                strangers_select_resends_object: true,
                deselect_sends_terse: false,
                link_sends: LinkedRecord::Root,
                child_sale: ChildSale::Roots,
                new_prim_owner_mask: OPENSIM_ALL_PERMISSIONS,
                new_prim_last_owner: LastOwner::Rezzer,
                ownership_cost: 0,
            },
        }
    }

    /// How this grid answers the world map where the two disagree — measured
    /// by the `map-blocks-items` conformance case on aditi and the local
    /// OpenSim (2026-10-08, `book/src/gridspec/world-map.md`).
    ///
    /// They agree on the outline: a block reply echoes the low sixteen bits of
    /// the request's flags and carries a map image id only when those are
    /// zero; water height, agent count and region flags are always zero; a
    /// name search ends with an entry at cell `(0, 0)` carrying the text that
    /// was searched for; a rectangle whose bounds are the wrong way round and
    /// a lone empty cell asked about without the null-sims flag get no reply.
    #[must_use]
    pub const fn map_policy(self) -> MapPolicy {
        match self {
            Self::SecondLife => MapPolicy {
                empty_cells: EmptyCells::Every,
                blocks_per_reply: 255,
                agent_dot_name: DotName::Uuid,
                largest_block_request: Some(MapPolicy::SECOND_LIFE_LARGEST_BLOCK_REQUEST),
                name_match: NameMatch::Prefix,
                shortest_search: 1,
                short_search_alert: None,
                no_match_alert: None,
                layers: MapLayerAnswer::None,
                named_region_sends_agents: false,
                empty_region_dot_m: 0,
                absent_tile: AbsentTile::Forbidden,
                tile_cache_headers: true,
            },
            Self::OpenSim => MapPolicy {
                empty_cells: EmptyCells::LoneCell,
                blocks_per_reply: MapPolicy::OPENSIM_BLOCKS_PER_REPLY,
                agent_dot_name: DotName::Hash,
                largest_block_request: None,
                name_match: NameMatch::Anywhere,
                shortest_search: 3,
                short_search_alert: Some("Use a search string with at least 3 characters"),
                no_match_alert: Some("No regions found with that name."),
                layers: MapLayerAnswer::WholeGrid,
                named_region_sends_agents: true,
                empty_region_dot_m: 1,
                absent_tile: AbsentTile::BlankTile,
                tile_cache_headers: false,
            },
        }
    }

    /// Which neighbouring regions this grid holds a child agent in, and what
    /// it says when an agent walks into one — measured by scripted `sl-repl`
    /// probes and the `draw-distance`, `neighbour-child-circuits` and
    /// `region-crossing` cases on aditi and the local OpenSim (2026-10-07,
    /// `book/src/gridspec/teleport.md` § Neighbours and crossings).
    ///
    /// Both grids announce a neighbour with an event-queue `EnableSimulator`
    /// and take it away with a `DisableSimulator` down its child circuit, and
    /// both decide which from the draw distance in the agent's `AgentUpdate`.
    /// They read that distance differently. OpenSim adds 64 m to it, holds the
    /// sum between 96 and 255, and keeps every region a square of that
    /// half-width around the *avatar* touches
    /// (`ScenePresence.RegionViewDistance`, `EntityTransferModule.RegionsInView`),
    /// answering a change within a second either way. Second Life compares it
    /// with a figure per neighbour — 128 m for one sharing an edge, 128·√2 m
    /// for one touching at a corner, from a spot eight metres from two borders
    /// — announces within a second, and retires fifty seconds late.
    #[must_use]
    pub const fn neighbour_policy(self) -> NeighbourViewPolicy {
        match self {
            Self::SecondLife => NeighbourViewPolicy {
                reach: NeighbourReach::RegionSpacing,
                retire_delay: Duration::from_secs(50),
                states_region_size: false,
                crossing_look_at: CrossingLookAt::Facing,
                handshake_on_crossing: true,
            },
            Self::OpenSim => NeighbourViewPolicy {
                reach: NeighbourReach::SquareAroundAvatar {
                    margin: 64.0,
                    least: 96.0,
                    most: 255.0,
                },
                retire_delay: Duration::ZERO,
                states_region_size: true,
                crossing_look_at: CrossingLookAt::FlightVelocity,
                handshake_on_crossing: false,
            },
        }
    }

    /// What a region says of itself as an agent arrives and afterwards —
    /// measured by `region-arrival` on aditi and the local OpenSim
    /// (2026-10-07, `book/src/gridspec/region-arrival.md`).
    #[must_use]
    pub const fn arrival_policy(self) -> ArrivalPolicy {
        match self {
            Self::SecondLife => ArrivalPolicy {
                product_name: "Mainland / Full Region",
                product_sku: "023",
                colo_name: "aws-us-west-2b",
                cpu_class_id: 1140,
                billable_factor: 1.0,
                child_handshakes: 2,
                health_message: true,
                agent_state_update: true,
                stats_interval: Duration::from_secs(2),
                stat_ids: &[
                    0, 1, 2, 3, 31, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 18, 19, 20, 24,
                    25, 26, 27, 28, 29, 30, 32, 33, 34, 35, 38, 39, 40,
                ],
                frames_per_second: 45.0,
                time_interval: Duration::from_secs(10),
                coarse_interval: Duration::from_millis(1_333),
                coarse_rounding: sl_proto::CoarseRounding::Nearest,
                sun_direction: [0.419_341_44, 0.0, 0.907_828_6],
                sun_phase: 1.114_392_9,
            },
            Self::OpenSim => ArrivalPolicy {
                product_name: "",
                product_sku: "",
                colo_name: "",
                cpu_class_id: 9,
                billable_factor: 0.0,
                child_handshakes: 1,
                health_message: false,
                agent_state_update: false,
                stats_interval: Duration::from_secs(3),
                stat_ids: &[
                    0, 1, 2, 3, 13, 14, 11, 12, 4, 5, 7, 9, 6, 17, 18, 24, 8, 19, 20, 15, 33, 32,
                    27, 21, 22, 23, 25, 26, 31, 38, 34, 35, 36, 37, 39, 40, 30, 10, 16, 28, 29,
                ],
                frames_per_second: 55.0,
                time_interval: Duration::from_millis(2_550),
                coarse_interval: Duration::from_millis(4_545),
                coarse_rounding: sl_proto::CoarseRounding::Down,
                sun_direction: [0.0, 0.0, 0.0],
                sun_phase: 2.665_263,
            },
        }
    }

    /// How a region's ground and wind are sent (`terrain-layerdata`,
    /// 2026-10-08, `book/src/gridspec/terrain.md`).
    #[must_use]
    pub const fn terrain_policy(self) -> TerrainPolicy {
        match self {
            Self::SecondLife => TerrainPolicy {
                land_origin: LandOrigin::AgentPosition,
                land_packing: sl_proto::LayerPacking::KeepWithin(1_200),
                land_encoding: sl_proto::LayerEncoding::REFERENCE,
                wind_start: WindStart::WithTheGround,
                wind_interval: Duration::from_secs(1),
                wind_reliability: sl_proto::Reliability::Unreliable,
                wind_encoding: sl_proto::LayerEncoding {
                    stride: 18,
                    prequant: 6,
                    flat_patches: sl_proto::FlatPatches::Transformed,
                },
            },
            Self::OpenSim => TerrainPolicy {
                land_origin: LandOrigin::AgentPatch,
                land_packing: sl_proto::LayerPacking::CloseOnceOver(890),
                land_encoding: sl_proto::LayerEncoding {
                    flat_patches: sl_proto::FlatPatches::HeaderOnly,
                    ..sl_proto::LayerEncoding::REFERENCE
                },
                wind_start: WindStart::OnTheRegionClock,
                wind_interval: Duration::from_millis(13_636),
                wind_reliability: sl_proto::Reliability::Reliable,
                wind_encoding: sl_proto::LayerEncoding::REFERENCE,
            },
        }
    }

    /// The `SimulatorFeatures` document a stock region of this grid serves —
    /// every key the live grid was measured sending, in the LLSD kind it sent
    /// it in (`simulator-features`, 2026-10-07,
    /// `book/src/gridspec/region-arrival.md`).
    ///
    /// Four things in a live reply are not here, because they are the grid's
    /// own and not its flavour's, and the fake grid fills them in where it
    /// knows them: the `OpenSimExtras` addresses and grid names, the currency
    /// symbol, the voice backend (`VoiceServerType`), and the script syntax
    /// (`LSLSyntaxId`, with Second Life's `LSLSyntaxVersion`), which a region
    /// may only advertise beside a syntax document it serves.
    #[must_use]
    pub fn stock_simulator_features(self) -> sl_proto::SimulatorFeatures {
        let shared = sl_proto::SimulatorFeatures {
            mesh_rez_enabled: Some(true),
            mesh_upload_enabled: Some(true),
            mesh_xfer_enabled: Some(true),
            bakes_on_mesh_enabled: Some(true),
            avatar_hover_height_enabled: Some(true),
            physics_materials_enabled: Some(true),
            physics_shape_types: Some(sl_proto::PhysicsShapeTypes {
                convex: true,
                none: true,
                prim: true,
            }),
            max_agent_attachments: Some(38),
            max_materials_per_transaction: Some(50),
            ..sl_proto::SimulatorFeatures::default()
        };
        match self {
            Self::SecondLife => sl_proto::SimulatorFeatures {
                animated_objects: Some(sl_proto::AnimatedObjects {
                    max_tris: 100_000,
                    max_agent_attachments: 1,
                }),
                dynamic_pathfinding_enabled: Some(true),
                max_agent_groups: Some(50),
                max_agent_groups_basic: Some(42),
                max_agent_groups_premium: Some(70),
                max_estate_access_ids: Some(750),
                max_estate_managers: Some(20),
                max_texture_resolution: Some(2048),
                render_materials_capability: Some(4.0),
                pbr_terrain_enabled: Some(true),
                pbr_terrain_transforms_enabled: Some(true),
                pbr_material_swatch_enabled: Some(true),
                gltf_enabled: Some(false),
                mirrors_enabled: Some(true),
                no_mod_bypass_support: Some(true),
                // Per region on the live grid; this grid runs no Lua.
                lua_scripts_enabled: Some(false),
                dead_reckoning_distance: Some(20.0),
                dead_reckoning_time: Some(1.0),
                host_name: Some("simhost-fake.sl-fake-grid.invalid".to_owned()),
                ..shared
            },
            Self::OpenSim => sl_proto::SimulatorFeatures {
                animated_objects: Some(sl_proto::AnimatedObjects {
                    max_tris: 150_000,
                    max_agent_attachments: 2,
                }),
                max_agent_groups_basic: Some(60),
                max_agent_groups_premium: Some(60),
                render_materials_capability: Some(3.0),
                menus: Some(sl_proto::DynamicMenus::default()),
                open_sim_extras: Some(sl_proto::OpenSimExtras {
                    export_supported: Some(true),
                    animation_set: Some(true),
                    avatar_skeleton: Some(true),
                    grid_url_alias: Some(String::new()),
                    say_range: Some(20),
                    shout_range: Some(100),
                    whisper_range: Some(10),
                    min_prim_scale: Some(0.001),
                    max_prim_scale: Some(256.0),
                    min_phys_prim_scale: Some(0.01),
                    max_phys_prim_scale: Some(64.0),
                    min_sim_height: Some(-100.0),
                    max_sim_height: Some(50_000.0),
                    min_heightmap: Some(-100.0),
                    max_heightmap: Some(4000.0),
                    // The reciprocal of the 0.0909 s frame, and what the region
                    // multiplies it by to report the 55 a viewer expects.
                    simulator_fps: Some(11.001_1),
                    simulator_fps_factor: Some(4.999_5),
                    simulator_fps_warn_percent: Some(60),
                    simulator_fps_crit_percent: Some(40),
                    ..sl_proto::OpenSimExtras::default()
                }),
                ..shared
            },
        }
    }

    /// Whether this grid's `SimulatorFeatures` carries the `OpenSimExtras`
    /// block.
    ///
    /// OpenSim always sends it (`SimulatorFeaturesModule.cs` fills it in
    /// unconditionally, and `GridService` injects the grid-wide URLs into it);
    /// Second Life sends no such key, which is the one structural difference
    /// that reliably tells the two replies apart. What rides in it — the
    /// map-tile server and the currency helper — reaches a viewer on Second
    /// Life by another route, so this is a block to drop rather than a set of
    /// URLs to hide: see the module docs.
    #[must_use]
    pub const fn advertises_open_sim_extras(self) -> bool {
        matches!(self, Self::OpenSim)
    }

    /// The spatial-voice backend this grid's regions serve.
    ///
    /// Second Life is WebRTC. A stock OpenSim region is
    /// [`VoiceBackend::Silent`]: both its voice modules are optional and off by
    /// default, and both answer with the Vivox SIP shape this workspace does
    /// not implement anywhere — so the honest model of the grid nobody
    /// configured is a grid that offers no voice, and every advertisement of it
    /// falls away with the backend. See the [`voice`](crate::voice) module docs.
    #[must_use]
    pub const fn voice_backend(self) -> VoiceBackend {
        match self {
            Self::SecondLife => VoiceBackend::WebRtc,
            Self::OpenSim => VoiceBackend::Silent,
        }
    }

    /// How this grid answers the deprecated UDP inventory fetch.
    ///
    /// OpenSim still serves it (`LLClientView.HandleFetchInventoryDescendents`
    /// is wired to the region's inventory service); Second Life dropped it when
    /// inventory moved behind AIS3. The Second Life side is the *refusal* rather
    /// than the silence aditi was measured giving — see the module docs for why
    /// that deviation is deliberate.
    #[must_use]
    pub const fn legacy_udp_inventory(self) -> LegacyUdpInventory {
        match self {
            Self::SecondLife => LegacyUdpInventory::Refused,
            Self::OpenSim => LegacyUdpInventory::Served,
        }
    }

    /// How this grid announces an inventory item it just created — the item a
    /// take files away.
    ///
    /// OpenSim sends the legacy UDP `UpdateCreateInventoryItem`; Second Life
    /// delivers the new item as a `BulkUpdateInventory` over the event queue,
    /// which is why `object-asset-format`'s take leg waits for either.
    #[must_use]
    pub const fn inventory_announcement(self) -> InventoryAnnouncement {
        match self {
            Self::SecondLife => InventoryAnnouncement::BulkUpdate,
            Self::OpenSim => InventoryAnnouncement::Legacy,
        }
    }

    /// How this grid announces an item a **capability upload** created or
    /// rewrote — which is neither one answer nor how it announces a taken one.
    ///
    /// After a `NewFileAgentInventory` completion both grids say nothing: the
    /// response body carries the whole new item. After an in-place save they
    /// diverge, and it is Second Life that pushes the legacy UDP
    /// `UpdateCreateInventoryItem` while OpenSim stays quiet — the opposite
    /// pairing to [`inventory_announcement`](Self::inventory_announcement).
    /// Every cell is measured rather than reasoned; see the
    /// [`inventory`](crate::inventory) module docs for the table and for what
    /// the push still carries where it survives.
    #[must_use]
    pub const fn upload_announcements(self) -> UploadAnnouncements {
        match self {
            Self::SecondLife => UploadAnnouncements {
                created: UploadAnnouncement::Silent,
                saved: UploadAnnouncement::Legacy,
            },
            Self::OpenSim => UploadAnnouncements::uniform(UploadAnnouncement::Silent),
        }
    }

    /// Who composites this grid's avatars ([`BakePolicy`]).
    ///
    /// Second Life central-bakes; a stock OpenSim region leaves it to each
    /// viewer. Four separate things move with this answer, and moving fewer
    /// than all four leaves avatars silently cloud-shaped — see the
    /// [`bakes`](crate::bakes) module docs.
    #[must_use]
    pub const fn bakes(self) -> BakePolicy {
        match self {
            Self::SecondLife => BakePolicy::ServerSide,
            Self::OpenSim => BakePolicy::ClientSide,
        }
    }

    /// The L$ price list and region object budget this grid answers an
    /// `EconomyDataRequest` with.
    ///
    /// Second Life's is measured (aditi, 2026-09-08, all seventeen fields);
    /// OpenSim's is read off its `SampleMoneyModule` defaults, which are *not*
    /// a table of zeroes — five of the seventeen are the numbers a Linden
    /// simulator was sending when that module was written, and the two grids
    /// still agree on them. Only OpenSim declines to quote a group-creation
    /// price. See the [`economy_policy`](crate::economy_policy) module docs for
    /// both columns side by side.
    #[must_use]
    pub const fn prices(self) -> sl_proto::EconomyData {
        match self {
            Self::SecondLife => crate::economy_policy::second_life_prices(),
            Self::OpenSim => crate::economy_policy::open_sim_prices(),
        }
    }

    /// Whether this grid's login response describes what the account is
    /// entitled to: the benefits package and the maturity *preference*.
    ///
    /// Second Life sends `account_type`, `account_level_benefits` and
    /// `premium_packages`, and separately `agent_region_access`. A stock OpenSim
    /// grid sends none of the four — `agent_region_access` appears nowhere in
    /// its source, and its login service has no notion of a subscription at all.
    ///
    /// The four move together because the reference viewer treats them as one
    /// decision: Firestorm gates its whole benefits init behind
    /// `isInSecondLife()`, because the parse *fails* on a missing field and
    /// insists on both a `Base` and a `Premium` package — so a grid that sent
    /// half of this would make a viewer complain at every login, which is worse
    /// than sending none.
    ///
    /// It leaves the *other* two maturity fields alone. `agent_access` and
    /// `agent_access_max` are sent by both grids; what differs there is that
    /// OpenSim hard-codes them to `M`/`A` for every account while Second Life
    /// answers per account, which is
    /// [`AccountConfig::maturity_ceiling`](crate::AccountConfig::maturity_ceiling)'s
    /// business rather than this knob's.
    #[must_use]
    pub const fn describes_account_entitlements(self) -> bool {
        matches!(self, Self::SecondLife)
    }

    /// The `RegionProtocols` bits this grid claims that are **not** the bake
    /// policy's to claim.
    ///
    /// Only one so far: OpenSim sets bit 63,
    /// [`REGION_PROTOCOL_BAKES_ON_MESH`], on every region handshake. It shares
    /// a field with the central-bake bit and nothing else, which is why the two
    /// halves are contributed separately and OR'd together — a grid told to
    /// bake client-side is still whichever grid it is imitating about Bakes on
    /// Mesh.
    ///
    /// Second Life claims nothing here rather than claiming bit 63 too: the
    /// reference viewer reads that bit as an OpenSim extension and decides the
    /// same question from the grid's identity on Second Life, so whether the
    /// Second Life simulator sets it is unmeasured.
    #[must_use]
    pub const fn region_protocol_bits(self) -> u64 {
        match self {
            Self::SecondLife => 0,
            Self::OpenSim => REGION_PROTOCOL_BAKES_ON_MESH,
        }
    }
}

/// What aditi's seed refused of the client's requested capabilities.
pub const SECOND_LIFE_REFUSED_CAPABILITIES: &[&str] = &["ObjectAnimation", "UploadBakedTexture"];

/// What the local OpenSim's seed refused of the client's requested
/// capabilities: AIS3 and the library fetches, every experience capability,
/// voice (no voice module), the group-invite and offline-message capabilities
/// (OpenSim does both over UDP), and the central-bake trigger.
pub const OPENSIM_REFUSED_CAPABILITIES: &[&str] = &[
    "AcceptGroupInvite",
    "AgentExperiences",
    "ChatSessionRequest",
    "DeclineGroupInvite",
    "DirectDelivery",
    "ExperiencePreferences",
    "ExperienceQuery",
    "FetchLib2",
    "FetchLibDescendents2",
    "FindExperienceByName",
    "GetAdminExperiences",
    "GetCreatorExperiences",
    "GetExperienceInfo",
    "GetExperiences",
    "GroupExperiences",
    "IncrementCOFVersion",
    "InterestList",
    "InventoryAPIv3",
    "IsExperienceAdmin",
    "IsExperienceContributor",
    "LibraryAPIv3",
    "ModifyMaterialParams",
    "ObjectAnimation",
    "ParcelVoiceInfoRequest",
    "ProvisionVoiceAccountRequest",
    "ReadOfflineMsgs",
    "RegionExperiences",
    "SendUserReport",
    "SendUserReportWithScreenshot",
    "UpdateAvatarAppearance",
    "UpdateExperience",
    "UpdateMaterialAgentInventory",
    "UserInfo",
    "VoiceSignalingRequest",
];

/// `max-agent-groups` on a stock OpenSim grid: its login service's
/// `MaxAgentGroups` default, which the local grid answered (2026-10-04).
pub const OPENSIM_MAX_AGENT_GROUPS: u32 = 42;

/// The sequence id Second Life gives the parcel it pushes back after an edit:
/// the reference viewer's `SELECTED_PARCEL_SEQ_ID`.
pub const SELECTED_PARCEL_SEQUENCE_ID: i32 = -10_000;

/// How a grid labels a region's default day ([`ImitatedGrid::stock_day`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockDay {
    /// The day cycle's name.
    pub name: &'static str,
    /// The environment version reported with it.
    pub env_version: i32,
}

/// What a grid answers an accepted environment set or reset with
/// ([`ImitatedGrid::environment_change_reply`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentChangeReply {
    /// The environment now in force.
    Settings,
    /// A verdict and nothing else.
    Bare,
}

/// The region flags both live grids' stock regions were measured sending in
/// their handshake (2026-10-07): landmarks and set-home allowed, direct
/// teleport, parcel changes and voice allowed, externally visible, and bit 5.
/// (The aditi sandbox added bit 9 to these.)
pub const STOCK_REGION_FLAGS: u32 = 0x1410_8026;

/// How a region's ground and wind are sent
/// ([`ImitatedGrid::terrain_policy`]).
///
/// Both live grids send the whole ground of the agent's region and of every
/// neighbour it is shown, reliably, nearest first, and go on sending the wind
/// as two patches at position `(0, 0)`; neither sends a cloud or a water
/// layer. Everything else here is where they part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainPolicy {
    /// What "nearest" is measured from.
    pub land_origin: LandOrigin,
    /// How many land patches go into one `LayerData`.
    pub land_packing: sl_proto::LayerPacking,
    /// How a land patch is written.
    pub land_encoding: sl_proto::LayerEncoding,
    /// When an arriving root agent gets its first wind.
    pub wind_start: WindStart,
    /// How far apart the wind messages are. OpenSim's is 150 frames of its
    /// eleven a second.
    pub wind_interval: Duration,
    /// Whether a wind message is sent reliably.
    pub wind_reliability: sl_proto::Reliability,
    /// How a wind patch is written.
    pub wind_encoding: sl_proto::LayerEncoding,
}

impl TerrainPolicy {
    /// How the ground is streamed at an agent placed at `(east, north)`
    /// region metres.
    #[must_use]
    pub fn land_stream(&self, east: f32, north: f32) -> sl_proto::TerrainStream {
        let cells = crate::terrain::PATCH_CELLS_M;
        let nearest_to = match self.land_origin {
            LandOrigin::AgentPosition => (east, north),
            LandOrigin::AgentPatch => (
                (east / cells).floor().mul_add(cells, cells / 2.0),
                (north / cells).floor().mul_add(cells, cells / 2.0),
            ),
        };
        sl_proto::TerrainStream {
            nearest_to,
            packing: self.land_packing,
            encoding: self.land_encoding,
        }
    }

    /// The wind a region goes on sending, given the wind layer's patches —
    /// or none for a region with no wind.
    #[must_use]
    pub fn wind_feed(&self, patches: Vec<sl_proto::TerrainPatch>) -> Option<sl_proto::WindFeed> {
        (!patches.is_empty()).then_some(sl_proto::WindFeed {
            interval: self.wind_interval,
            patches,
            encoding: self.wind_encoding,
            reliability: self.wind_reliability,
        })
    }
}

/// What the ground is sent outwards from ([`TerrainPolicy::land_origin`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandOrigin {
    /// Where the agent stands: Second Life, roughly — its order is nearest
    /// first with a patch here and there out of turn, and not the same twice.
    AgentPosition,
    /// The patch the agent is in, distances counted in whole patches:
    /// OpenSim, exactly.
    AgentPatch,
}

/// When an arriving root agent gets its first wind
/// ([`TerrainPolicy::wind_start`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindStart {
    /// Behind the last message of the ground, and then on the region's clock.
    WithTheGround,
    /// Whenever the region's clock next says, up to an interval away.
    OnTheRegionClock,
}

/// What a region says of itself as an agent arrives, and on a timer after
/// ([`ImitatedGrid::arrival_policy`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalPolicy {
    /// The stock Full Region's `ProductName` in the handshake. A region
    /// configured as another product keeps that product's own name.
    pub product_name: &'static str,
    /// The stock Full Region's `ProductSKU`.
    pub product_sku: &'static str,
    /// The handshake's `ColoName`: the data centre.
    pub colo_name: &'static str,
    /// The handshake's `CPUClassID`.
    pub cpu_class_id: i32,
    /// The handshake's `BillableFactor`.
    pub billable_factor: f32,
    /// How many `RegionHandshake`s a child circuit is sent as it opens.
    pub child_handshakes: u8,
    /// Whether an arrival is told the agent's health (`HealthMessage`).
    pub health_message: bool,
    /// Whether an arrival is pushed an `AgentStateUpdate` over the event
    /// queue.
    pub agent_state_update: bool,
    /// How far apart the `SimStats` are.
    pub stats_interval: Duration,
    /// The statistic ids a `SimStats` carries, in the order the grid sends
    /// them.
    pub stat_ids: &'static [u32],
    /// The frame rate the statistics report: the live grids' own, 45 and 55.
    pub frames_per_second: f32,
    /// How far apart the `SimulatorViewerTimeMessage`s are.
    pub time_interval: Duration,
    /// How far apart the `CoarseLocationUpdate`s are.
    pub coarse_interval: Duration,
    /// How a position is cut down to a coarse entry: Second Life rounds,
    /// OpenSim cuts off.
    pub coarse_rounding: sl_proto::CoarseRounding,
    /// The sun direction the time message carries; OpenSim's is zero.
    pub sun_direction: [f32; 3],
    /// The sun phase the time message carries.
    pub sun_phase: f32,
}

impl ArrivalPolicy {
    /// The region's periodic telemetry: the statistics of an idle region with
    /// one agent in it, and the time as of `unix_usec`.
    #[must_use]
    pub fn telemetry(
        &self,
        identity: &sl_proto::RegionIdentity,
        object_capacity: u32,
        unix_usec: u64,
    ) -> sl_proto::RegionTelemetry {
        let [x, y, z] = self.sun_direction;
        let zero = sl_proto::Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        sl_proto::RegionTelemetry {
            stats_interval: self.stats_interval,
            stats: sl_proto::RegionStats {
                grid_coordinates: identity.grid_coordinates,
                region_flags: identity.region_flags,
                object_capacity,
                region_flags_extended: identity.region_flags_extended,
                stats: self
                    .stat_ids
                    .iter()
                    .map(|id| {
                        let stat = sl_proto::SimStatId::from_id(*id);
                        let value = match stat {
                            sl_proto::SimStatId::TimeDilation => 1.0,
                            sl_proto::SimStatId::SimFps | sl_proto::SimStatId::PhysicsFps => {
                                self.frames_per_second
                            }
                            _ => 0.0,
                        };
                        (stat, value)
                    })
                    .collect(),
            },
            time_interval: self.time_interval,
            time: sl_proto::SimulatorTime {
                usec_since_start: unix_usec,
                sec_per_day: 14_400,
                sec_per_year: 158_400,
                sun_direction: sl_proto::Vector { x, y, z },
                sun_phase: self.sun_phase,
                sun_ang_velocity: zero,
            },
            coarse_interval: Some(self.coarse_interval),
            coarse_rounding: self.coarse_rounding,
        }
    }
}

/// Which neighbours a grid holds for an agent and how it hands the agent to
/// one ([`ImitatedGrid::neighbour_policy`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeighbourViewPolicy {
    /// How the agent's draw distance decides whether a neighbour is held.
    pub reach: NeighbourReach,
    /// How long after the draw distance stopped reaching a neighbour its child
    /// circuit is retired. A neighbour reached again in the meantime is kept.
    pub retire_delay: Duration,
    /// Whether an `EnableSimulator` and a `CrossedRegion` state the region's
    /// size (`RegionSizeX` / `RegionSizeY`).
    pub states_region_size: bool,
    /// What a `CrossedRegion`'s `LookAt` holds.
    pub crossing_look_at: CrossingLookAt,
    /// Whether the region crossed into sends its `RegionHandshake` again as
    /// the child circuit becomes the root. OpenSim's said everything it had
    /// to when the child circuit opened.
    pub handshake_on_crossing: bool,
}

impl NeighbourViewPolicy {
    /// Whether a draw distance of `far` metres reaches the neighbour
    /// `(dx, dy)` region slots away, for an avatar standing at `(x, y)` in its
    /// own region.
    #[must_use]
    pub fn reaches(&self, far: f32, avatar: (f32, f32), slots: (i16, i16)) -> bool {
        /// A region's side in metres.
        const SIDE: f32 = 256.0;
        let (dx, dy) = (f32::from(slots.0), f32::from(slots.1));
        match self.reach {
            NeighbourReach::RegionSpacing => far >= (SIDE / 2.0) * dx.hypot(dy),
            NeighbourReach::SquareAroundAvatar {
                margin,
                least,
                most,
            } => {
                let view = (far + margin).clamp(least, most);
                // The neighbour's rectangle, in the avatar's region's metres,
                // against the square of half-width `view` around the avatar:
                // they touch unless one lies wholly past the other on an axis.
                let touches = |at: f32, slot: f32| {
                    let low = slot * SIDE;
                    at + view >= low && at - view <= low + SIDE
                };
                touches(avatar.0, dx) && touches(avatar.1, dy)
            }
        }
    }
}

/// How a draw distance is read as reaching a neighbouring region
/// ([`NeighbourViewPolicy::reach`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NeighbourReach {
    /// Second Life: the draw distance has to be at least half the distance
    /// between the two regions' centres — 128 m for a region sharing an edge,
    /// 128·√2 m for one touching at a corner — wherever the avatar stands.
    RegionSpacing,
    /// OpenSim: the draw distance plus `margin`, held between `least` and
    /// `most`, is the half-width of a square around the avatar; a neighbour
    /// that square touches is held.
    SquareAroundAvatar {
        /// Added to the draw distance.
        margin: f32,
        /// The smallest half-width.
        least: f32,
        /// The largest half-width.
        most: f32,
    },
}

/// What a `CrossedRegion`'s `LookAt` holds
/// ([`NeighbourViewPolicy::crossing_look_at`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossingLookAt {
    /// Second Life: the unit direction the agent crossed in.
    Facing,
    /// OpenSim: the agent's horizontal velocity when it crossed flying, and
    /// zero when it walked.
    FlightVelocity,
}

/// How a grid runs and refuses a teleport
/// ([`ImitatedGrid::teleport_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportPolicy {
    /// The `TeleportProgress` lines sent between the `TeleportStart` and the
    /// `TeleportFinish` of an inter-region teleport, in order. A refusal cuts
    /// the list short where the grid found out: an unknown region after the
    /// first, a region the agent may not enter after the last.
    pub progress: &'static [&'static str],
    /// Whether a home or landmark teleport opens with a line naming its kind
    /// (`sending_home`, `sending_landmark`) ahead of
    /// [`progress`](Self::progress).
    pub names_the_kind: bool,
    /// The flags added to the request's own on a local teleport's
    /// `TeleportStart` and `TeleportLocal`.
    pub local_flags: u32,
    /// Which way a `TeleportLocal` says the agent faces.
    pub local_look_at: LocalLookAt,
    /// The `LocationID` of a `TeleportFinish`.
    pub finish_location_id: u32,
    /// Whether a `TeleportFinish` states the destination region's size.
    pub finish_states_region_size: bool,
    /// How a refusal reaches the client.
    pub refusals: RefusalTransport,
    /// The reason given for a region handle no region answers to.
    pub unknown_region: &'static str,
    /// The reason given for a landmark asset the grid does not hold.
    pub unknown_landmark: &'static str,
    /// The reason given for a home teleport by an agent with no home. Second
    /// Life's is not measured (every test avatar has a home) and stays the
    /// key the fake grid always sent.
    pub no_home: &'static str,
    /// The reason given for accepting a lure the grid does not hold, or
    /// `None` for a grid that answers it with nothing at all. Second Life
    /// says nothing — a lure nobody offered and one its target had declined
    /// both went unanswered, and the client's own deadline ended each (a lure
    /// whose offerer had logged out was still honoured). OpenSim's lure id
    /// *is* the destination, so the only lure it cannot honour is one naming
    /// a region that does not exist.
    pub unknown_lure: Option<&'static str>,
    /// The progress line an accepted lure opens with, ahead of
    /// [`progress`](Self::progress) and on a local one too. Second Life sends
    /// `completing` — first, whatever the name suggests.
    pub lure_line: Option<&'static str>,
    /// Which flags a `TeleportFinish` carries.
    pub finish_flags: FinishFlags,
    /// What a `TeleportCancel` that arrives before the finish is answered
    /// with.
    pub cancel: CancelAnswer,
    /// Whether a location teleport into a region rated above the agent's
    /// maturity preference is refused. Second Life refuses it with
    /// `RegionTPAccessBlocked` (a home teleport is let through); OpenSim has
    /// no such check.
    pub enforces_maturity_preference: bool,
}

/// The reason Second Life gives for refusing a region above the agent's
/// maturity preference, and the alert key it repeats it under
/// ([`TeleportPolicy::enforces_maturity_preference`]).
pub const MATURITY_REFUSAL_REASON: &str = "You aren't allowed in that Region due to your \
     maturity Rating. You may need to validate your age and/or install the latest Viewer. \
     Please go to the Knowledge Base for details on accessing areas with this maturity Rating.";

/// The alert key of a maturity refusal ([`MATURITY_REFUSAL_REASON`]).
pub const MATURITY_REFUSAL_ALERT: &str = "RegionTPAccessBlocked";

impl TeleportPolicy {
    /// The `ExtraParams` of a maturity refusal's alert: an LLSD map naming the
    /// refusing region's `SimAccess`, serialized as Second Life sends it.
    #[must_use]
    pub fn maturity_refusal_params(region_access: u8) -> String {
        format!(
            "<? LLSD/XML ?>\n<llsd><map><key>_region_access</key><integer>{region_access}\
             </integer></map></llsd>\n"
        )
    }
}

/// Which flags a `TeleportFinish` carries
/// ([`TeleportPolicy::finish_flags`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishFlags {
    /// The flags of the teleport it finishes: its kind (Second Life).
    TheKind,
    /// `VIA_LOCATION` whatever kind of teleport it finishes, keeping only the
    /// flying bit of the request's own flags (OpenSim:
    /// `EventQueueGetHandlers.TeleportFinishEvent`).
    ViaLocation,
}

/// Which way a `TeleportLocal` says the agent faces
/// ([`TeleportPolicy::local_look_at`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalLookAt {
    /// The direction the request asked for, flattened to the horizontal, and
    /// east when that leaves nothing (OpenSim).
    Requested,
    /// The unit vector from the landing position towards the region's origin,
    /// whatever the request asked for (Second Life: four teleports, four
    /// look-ats equal to the negated, normalised position).
    TowardsRegionOrigin,
}

/// How a teleport refusal reaches the client
/// ([`TeleportPolicy::refusals`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalTransport {
    /// The UDP `TeleportFailed`, sent in place of a `TeleportStart`, its
    /// reason an English sentence and no `AlertInfo` (OpenSim).
    Udp,
    /// A `TeleportFailed` event on the event queue, after the `TeleportStart`
    /// and the progress lines sent so far, its reason repeated as the key of
    /// an `AlertInfo` (Second Life).
    EventQueue,
}

/// What a `TeleportCancel` ahead of the finish is answered with
/// ([`TeleportPolicy::cancel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAnswer {
    /// The teleport is abandoned and reported failed with this reason and
    /// alert key (Second Life).
    Failed {
        /// The `Reason`.
        reason: &'static str,
        /// The `AlertInfo` key.
        alert: &'static str,
    },
    /// The teleport goes through. OpenSim honours a cancel only between
    /// creating the agent at the destination and sending the finish — tens of
    /// milliseconds — and then abandons the teleport without a word; the
    /// measured cancel, sent on the heels of its request, missed that window
    /// and the agent arrived. The flavour takes the measured outcome.
    Ignored,
}

/// How a grid runs a circuit ([`ImitatedGrid::circuit_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitPolicy {
    /// When silence closes the circuit, and how often and how many times an
    /// unacknowledged reliable packet is sent again.
    pub link: sl_proto::LinkTuning,
    /// What a root agent is told as its circuit is closed for silence: the
    /// reason of a `KickUser`, or `None` for a grid that just stops sending.
    pub timeout_kick: Option<&'static str>,
    /// Whether an `ObjectUpdate` goes out zero-coded with its final run of
    /// zeros counted one short, as Second Life's simulators send it a few
    /// times per login — the message a client must zero-fill to read
    /// (`viewer-objectupdate-truncated-block-drops-message`). A flavour that
    /// does sends *every* one so, so that no test of it can miss the case.
    pub short_zero_tails: bool,
}

/// What a grid answers a `LogoutRequest` with
/// ([`ImitatedGrid::logout_reply`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutReply {
    /// A `LogoutReply`.
    Sent,
    /// Nothing: the session closes, and the request is not even acknowledged.
    Withheld,
}

/// What one region holds ([`ImitatedGrid::region_capacity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionCapacity {
    /// The region's land impact budget — `RegionInfo`'s object limit, and what
    /// its parcels' prim allowances add up to.
    pub land_impact: u32,
    /// How many agents the region admits.
    pub max_agents: u32,
    /// How many the estate could raise that to.
    pub hard_max_agents: u32,
}

impl RegionCapacity {
    /// Gives a region's parcels their share of the budget, by area: each
    /// parcel's own allowance, and for each owner the total over every parcel
    /// they hold in the region (the two numbers About Land's Objects tab shows
    /// as the parcel's and the region's).
    pub(crate) fn allot(self, parcels: &mut [sl_proto::ParcelInfo]) {
        /// The area of a standard region, in square metres.
        const REGION_AREA: u64 = 256 * 256;
        let share = |area: u64| {
            i32::try_from(u64::from(self.land_impact).saturating_mul(area) / REGION_AREA)
                .unwrap_or(i32::MAX)
        };
        let held: Vec<(sl_types::key::OwnerKey, u64)> = parcels
            .iter()
            .map(|parcel| (parcel.owner, u64::from(parcel.area.0)))
            .collect();
        for parcel in parcels {
            parcel.max_prims = share(u64::from(parcel.area.0));
            parcel.sim_wide_max_prims = share(
                held.iter()
                    .filter(|(owner, _)| *owner == parcel.owner)
                    .map(|(_, area)| *area)
                    .sum(),
            );
        }
    }
}

/// How a grid answers the About Land traffic ([`ImitatedGrid::parcel_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent measured behaviour of a live grid, not a state machine"
)]
pub struct ParcelPolicy {
    /// Whether a `ParcelPropertiesRequestByID` is answered at all. OpenSim has
    /// no handler for it and logs "Unhandled packet"; a client that refetches
    /// by id there waits forever.
    pub answers_request_by_id: bool,
    /// The sequence id of the parcel pushed back to the editing agent after a
    /// `ParcelPropertiesUpdate` — the push the update's `flags` ask for.
    pub edit_echo: EditEcho,
    /// Whether an edit over the UDP message (which has no media type) wipes
    /// the parcel's media type. OpenSim stores the missing field as `NULL`,
    /// which its own database then refuses on every later commit of the region
    /// — the fake keeps the record and empties the type, which a client reads
    /// back as the loss it is.
    pub udp_edit_nulls_media_type: bool,
    /// The loop flag of a parcel nobody set media on. Both grids send the
    /// `MediaData` block for it, typed `none/none` and sized 0×0; Second Life
    /// has it loop and OpenSim does not.
    pub unset_media_loops: bool,
    /// Whether the grid has the newer blocks at all: `ParcelExtendedFlags`
    /// (`obscure_moap`), which Second Life sends for every parcel, and
    /// `MediaLinkSharing`, which it sends for none without media. OpenSim's
    /// encoder writes neither, and an edit that carries them changes nothing a
    /// client can read back.
    pub sends_extended_blocks: bool,
    /// Whether a return addressed to a parcel takes the objects it names one
    /// by one. OpenSim's `LandObject.ReturnLandObjects` matches by class and
    /// owner only and never reads the task list (a whole-region return, the
    /// top-objects window's, does). Second Life is unmeasured — the test
    /// avatars own no land on aditi — and keeps the reference viewer's reading
    /// that a named object is returned.
    pub parcel_return_reads_task_ids: bool,
    /// Whether the object-owner tally (`ParcelObjectOwnersReply`) comes over
    /// the event queue, as the template's `UDPDeprecated` says Second Life's
    /// does, or as the UDP message, as OpenSim's does
    /// (`LLClientView.SendLandObjectOwners`).
    pub owners_reply_over_event_queue: bool,
    /// Which LLSD types the event-queue `ParcelProperties` writes the six
    /// fields the grids disagree about in ([`sl_proto::ParcelLlsdDialect`]).
    pub wire_types: sl_proto::ParcelLlsdDialect,
    /// Which maturity bits a `ParcelInfoReply` listing carries for a parcel in
    /// an adult region: Second Life sets the mature bit beside the adult one
    /// (measured by `parcel-info-dwell` on aditi, 2026-10-05), OpenSim the
    /// adult bit alone (`Util.ConvertAccessLevelToMaturity`, read from source:
    /// the local grid has no adult region). The rest of the byte — group-owned
    /// `0x04`, for-sale `0x80` — the two pack alike.
    pub adult_listing_bits: sl_proto::AdultListingBits,
    /// Whether saving a non-empty allow or ban list switches the parcel's own
    /// `USE_ACCESS_LIST` / `USE_BAN_LIST` flag on, and emptying it switches the
    /// flag off again. OpenSim's `LandObject.UpdateAccessList` does both
    /// (measured by `parcel-access-list`, 2026-10-05), so giving a parcel an
    /// allow list there closes it to everybody else without the About Land
    /// checkbox being touched. Second Life is unmeasured — the test avatars
    /// own no land on aditi — and keeps the reference viewer's reading that
    /// the flags are the checkboxes and nothing else.
    pub list_update_sets_use_flag: bool,
    /// Whether the nil-agent placeholder a grid answers an empty list with
    /// carries the list's own bit in its `Flags`. OpenSim's does; Second
    /// Life's is `0` (both measured by `parcel-access-list`, 2026-10-05).
    pub empty_list_placeholder_names_its_list: bool,
}

impl ParcelPolicy {
    /// Fills in what the imitated grid sends for a parcel its fixture or an
    /// edit left unset: the media block of a parcel without media, the
    /// avatar-visibility booleans, and the extended flags where the grid has
    /// them — and drops the newer blocks where it does not. A value that was
    /// set and that the grid has is kept.
    pub(crate) fn dress(self, parcel: &mut sl_proto::ParcelInfo) {
        if parcel.media_data.is_none() {
            parcel.media_data = Some(sl_proto::ParcelMediaData {
                description: String::new(),
                media_type: "none/none".to_owned(),
                width: 0,
                height: 0,
                looping: self.unset_media_loops,
            });
        }
        // Both grids send the avatar-visibility booleans, and an unset parcel
        // shows its avatars and their sounds.
        parcel.see_avs = parcel.see_avs.or(Some(true));
        parcel.any_av_sounds = parcel.any_av_sounds.or(Some(true));
        parcel.group_av_sounds = parcel.group_av_sounds.or(Some(true));
        if self.sends_extended_blocks {
            parcel.obscure_moap = parcel.obscure_moap.or(Some(false));
        } else {
            parcel.obscure_moap = None;
            parcel.media_sharing = None;
        }
    }
}

/// How a grid answers the world map ([`ImitatedGrid::map_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPolicy {
    /// Which empty cells a `MapBlockRequest` carrying the null-sims flag is
    /// told about.
    pub empty_cells: EmptyCells,
    /// The most entries one `MapBlockReply` carries before the answer goes on
    /// in another. OpenSim cuts at ten; Second Life was not seen to cut one
    /// (the most the case drew from it was nine entries), so its figure is
    /// what the message allows.
    pub blocks_per_reply: usize,
    /// What an agent-location item is called.
    pub agent_dot_name: DotName,
    /// The most cells a `MapBlockRequest` may name and still be answered, or
    /// `None` on a grid that answers a rectangle of any size.
    pub largest_block_request: Option<u32>,
    /// Which part of a region's name a search has to match.
    pub name_match: NameMatch,
    /// The fewest characters a search may have and still be run.
    pub shortest_search: usize,
    /// The `AlertMessage` a search shorter than that is answered with, beside
    /// the entry that ends it.
    pub short_search_alert: Option<&'static str>,
    /// The modal `AgentAlertMessage` a search that matched nothing is
    /// answered with, beside the entry that ends it.
    pub no_match_alert: Option<&'static str>,
    /// What a `MapLayerRequest` is answered with.
    pub layers: MapLayerAnswer,
    /// Whether a `MapItemRequest` that names the agent's own region is
    /// answered with that region's agent locations whatever type it asked
    /// for. OpenSim does it for every type; Second Life answers the type
    /// asked for and nothing else.
    pub named_region_sends_agents: bool,
    /// How far inside a region's south-west corner the one agent-location
    /// item of a region with nobody to show sits, in metres along each axis.
    /// Both grids send such an item with an `Extra` of zero; Second Life puts
    /// it on the corner and OpenSim a metre in.
    pub empty_region_dot_m: u32,
    /// What the tile server answers for a tile it does not have.
    pub absent_tile: AbsentTile,
    /// Whether a tile comes with `Cache-Control`, `ETag` and `Last-Modified`.
    /// Second Life's content network sends all three; OpenSim sends none.
    pub tile_cache_headers: bool,
}

impl MapPolicy {
    /// The most cells Second Life answers a `MapBlockRequest` for.
    pub const SECOND_LIFE_LARGEST_BLOCK_REQUEST: u32 = 256;

    /// The image id of OpenSim's one map layer, fixed in its source.
    pub const OPENSIM_LAYER_IMAGE: uuid::Uuid =
        uuid::Uuid::from_u128(0x0000_0000_0000_1111_9999_0000_0000_0006);

    /// The upper bound, on each axis, of the rectangle OpenSim's one map
    /// layer claims to cover.
    pub const OPENSIM_LAYER_EXTENT: u32 = 30_000;

    /// The most matches OpenSim returns for one name search.
    pub const OPENSIM_SEARCH_LIMIT: usize = 20;

    /// The most entries OpenSim puts in one `MapBlockReply`.
    pub const OPENSIM_BLOCKS_PER_REPLY: usize = 10;

    /// The `Access` byte both grids give an entry that is not a region.
    pub const NON_EXISTENT_ACCESS: u8 = 255;
}

/// Which empty cells a null-sims `MapBlockRequest` reports
/// ([`MapPolicy::empty_cells`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyCells {
    /// Every empty cell of the rectangle, beside the regions in it: Second
    /// Life.
    Every,
    /// A cell only when the request names that one cell and nothing is there:
    /// OpenSim, which answers a larger empty rectangle with nothing.
    LoneCell,
}

/// What an agent-location item's `Name` holds ([`MapPolicy::agent_dot_name`]).
/// No viewer reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotName {
    /// A UUID in text: Second Life, for a region other than the agent's own.
    Uuid,
    /// Thirty-two hexadecimal digits: OpenSim's MD5 of the region's name and
    /// a clock tick.
    Hash,
}

/// How a name search matches ([`MapPolicy::name_match`]). Both ignore case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMatch {
    /// The name has to begin with the text: Second Life.
    Prefix,
    /// The name has to contain it: OpenSim.
    Anywhere,
}

/// What a `MapLayerRequest` is answered with ([`MapPolicy::layers`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapLayerAnswer {
    /// A reply with no layers in it, echoing the request's flags: Second
    /// Life, whose map is tiles alone.
    None,
    /// One layer from `(0, 0)` to
    /// [`MapPolicy::OPENSIM_LAYER_EXTENT`] with the image
    /// [`MapPolicy::OPENSIM_LAYER_IMAGE`], under flags of zero whatever the
    /// request carried: OpenSim.
    WholeGrid,
}

/// What a tile server answers for a tile it does not have
/// ([`MapPolicy::absent_tile`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbsentTile {
    /// `403` with an XML `AccessDenied` body: Second Life's content network,
    /// for open ocean, for a zoom it does not serve and for a tile named by a
    /// region that is not its corner alike.
    #[default]
    Forbidden,
    /// `200` with a tile of plain water: OpenSim, for all three.
    BlankTile,
}

/// How a grid answers a select and tells of a change to an object's record
/// ([`ImitatedGrid::properties_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertiesPolicy {
    /// Whether an `ObjectName` or an `ObjectDescription` is answered with the
    /// new record. Second Life answers every edit of the record to the
    /// session that made it, whether or not it holds the object selected;
    /// OpenSim's two handlers store the text and send nothing
    /// (`SceneGraph.PrimName`), while its sale and permission handlers answer.
    pub rename_answered: bool,
    /// Who is sent the record, with its advanced contents serial, when
    /// something is written into the object's task inventory.
    pub contents_change_told_to: ContentsAudience,
    /// Whether a select of an object the agent does not own sends it the
    /// object again in a full `ObjectUpdate` ahead of the record: OpenSim's
    /// `SelectPrim`, "if a friend got or lost edit rights after login, a full
    /// update is needed".
    pub strangers_select_resends_object: bool,
    /// Whether a deselect is answered with a terse update of the object.
    pub deselect_sends_terse: bool,
    /// Whose record a link sends the session that linked.
    pub link_sends: LinkedRecord,
    /// Whose sale type and price a child prim's record states.
    pub child_sale: ChildSale,
    /// The base and owner masks of a prim nobody has changed the permissions
    /// of. Second Life's "everything" is thirty-one bits; OpenSim's is the
    /// five it defines ([`OPENSIM_ALL_PERMISSIONS`]).
    pub new_prim_owner_mask: sl_wire::Permissions,
    /// Who a new prim's record names as its last owner.
    pub new_prim_last_owner: LastOwner,
    /// The `OwnershipCost` of a record and of a family record, in L$.
    pub ownership_cost: u64,
}

impl Default for PropertiesPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        ImitatedGrid::SecondLife.properties_policy()
    }
}

/// OpenSim's `PermissionMask.All`: transfer, modify, copy, export and move.
pub const OPENSIM_ALL_PERMISSIONS: sl_wire::Permissions = sl_wire::Permissions::TRANSFER
    .union(sl_wire::Permissions::MODIFY)
    .union(sl_wire::Permissions::COPY)
    .union(sl_wire::Permissions::EXPORT)
    .union(sl_wire::Permissions::MOVE);

/// Who a write into an object's contents is told to
/// ([`PropertiesPolicy::contents_change_told_to`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentsAudience {
    /// Every session holding the object selected, and nobody else: Second
    /// Life. The writer is told only if it is one of them.
    Selectors,
    /// The session that wrote, selected or not, and nobody else: OpenSim.
    Writer,
}

/// Whose sale state a child prim's record carries
/// ([`PropertiesPolicy::child_sale`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSale {
    /// The child's own: Second Life.
    Own,
    /// The root's: OpenSim reads the sale type and price off the root part.
    Roots,
}

/// Who a new prim's record names as its last owner
/// ([`PropertiesPolicy::new_prim_last_owner`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastOwner {
    /// Nobody: Second Life.
    Nobody,
    /// Whoever rezzed it: OpenSim.
    Rezzer,
}

/// Whose record a link sends ([`PropertiesPolicy::link_sends`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedRecord {
    /// Each child's that came under the root: Second Life.
    Children,
    /// The root's: OpenSim.
    Root,
}

/// How a grid answers a sit request ([`ImitatedGrid::sit_policy`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SitPolicy {
    /// What a sit on an object the region does not have is refused with, or
    /// `None` on a grid that does not answer it. Second Life uses the alert
    /// it refuses a neighbour region's seat with, which is what an object it
    /// cannot find in the agent's own region amounts to; OpenSim logs "Sit
    /// requested on unknown object" and sends nothing.
    pub unknown_target: Option<SitRefusal>,
    /// Whether the `SitPosition` of an `AvatarSitResponse` for a seat with a
    /// sit target is where the avatar ends up. Both grids put the avatar
    /// [`SIT_TARGET_RAISE_M`] above the target a script sets; Second Life's
    /// response says that position and OpenSim's says the target, so only
    /// the avatar's own object update says the same thing on both.
    pub response_states_seated_position: bool,
    /// How far in front of where it sat an avatar that stands up is put, in
    /// metres, along the way it faced on the seat. The same for two avatars
    /// on each grid, from a seat with a sit target and from one without.
    pub stand_forward_m: f32,
    /// How far above where it sat it is put, in metres. Second Life stands
    /// it at the height it sat at and lets it drop; OpenSim lifts it first.
    pub stand_up_m: f32,
}

/// How far above a script's sit target both grids put the seated avatar, in
/// metres: `llSitTarget(<0, 0, 1>, …)` seated both test avatars at 1.35 m.
/// OpenSim's arithmetic is its `SIT_TARGET_ADJUSTMENT` of 0.4 m less 0.05 m
/// along the target's own up axis, "empirically determined to be what is
/// used in SL".
pub const SIT_TARGET_RAISE_M: f32 = 0.35;

/// A named alert refusing a sit ([`SitPolicy::unknown_target`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SitRefusal {
    /// The alert's name: its `AlertInfo` message.
    pub name: &'static str,
    /// The alert's text.
    pub text: &'static str,
}

/// Which sequence id a grid's post-edit push carries ([`ParcelPolicy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditEcho {
    /// Always this id: Second Life's
    /// [`SELECTED_PARCEL_SEQUENCE_ID`], measured on an edit it refused (the
    /// test avatars own no land on aditi, so an accepted edit is unmeasured).
    Fixed(i32),
    /// The id the parcel was last sent under: OpenSim's `LandObject` keeps
    /// `m_lastSeqId` and reuses it for a snap-selection push with sequence 0,
    /// so the echo carries the id of the client's last properties request.
    LastSequence,
}

/// How a grid refuses a login ([`ImitatedGrid::login_refusals`]).
///
/// Both grids answer a wrong password and a name they have never heard of
/// **identically**, with the reason `key`, so neither tells a caller which
/// accounts exist; and both end the session an avatar already had when the
/// same account logs in again. They differ in everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginRefusals {
    /// The text of a `key` refusal.
    pub bad_credentials_message: &'static str,
    /// The localisation key sent with it (`message_id`, and with it an empty
    /// `message_args`). Second Life only.
    pub bad_credentials_message_id: Option<&'static str>,
    /// Whether a refusal carries a per-response incident id
    /// (`Linden_Error_Code`). Second Life only.
    pub stamps_error_code: bool,
    /// The text of a `presence` refusal for an avatar the grid believes is
    /// already logged in. OpenSim's was measured; Second Life never gave one
    /// (it admits the second login), so its flavour keeps the wire crate's.
    pub already_logged_in_message: &'static str,
    /// What becomes of a login of an avatar that is in world.
    pub second_login: SecondLogin,
    /// The `KickUser` reason the session already in world is ended with —
    /// which both grids do, whatever they answer the second login.
    pub second_login_kick_reason: &'static str,
}

/// What a grid answers a login of an avatar that is already in world
/// ([`LoginRefusals::second_login`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondLogin {
    /// The new login gets in and takes the avatar over. Second Life.
    Admitted,
    /// The new login is refused as `presence` — but the refusal clears the
    /// way, so the next attempt gets in. OpenSim.
    Refused,
}

/// The login response's fields a grid sends or omits regardless of the
/// request's `options` list ([`ImitatedGrid::login_fields`]).
///
/// Both grids send `look_at`, `max-agent-groups`, `map-server-url` and the
/// library owner; neither sends `voice-config` or a currency symbol. Those need
/// no row. What is here is where the two disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginFields {
    /// Whether the response carries `home`. OpenSim does; aditi did not, even
    /// with every option asked for.
    pub home: bool,
    /// Whether the response carries `region_size_x` / `region_size_y`. Only
    /// OpenSim, whose regions may be larger than 256 m, sends them.
    pub region_size: bool,
    /// The `max-agent-groups` figure, or `None` for the account's own package
    /// limit — which is what Second Life sends (50 for a `Base` account on
    /// aditi, its `group_membership_limit`).
    pub max_agent_groups: Option<u32>,
    /// The content sections and the one-grid scalars, with the values each
    /// grid was measured sending.
    pub sections: LoginSections,
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    /// The default is the grid the workspace targets. Every derived knob reads
    /// this, so it is the one place a stock fake grid's whole personality is
    /// decided.
    #[test]
    fn a_grid_nobody_configured_is_second_life() {
        assert_eq!(ImitatedGrid::default(), ImitatedGrid::SecondLife);
    }

    /// A region's budget is its product's on Second Life and one number on
    /// OpenSim, and its parcels share it by area — each owner's total being
    /// what they hold across the region.
    #[test]
    fn a_regions_budget_follows_its_product_and_is_shared_by_area() {
        use sl_proto::{LandArea, ProductType, RegionLocalParcelId};
        use sl_types::key::{AgentKey, OwnerKey};

        let sl = ImitatedGrid::SecondLife;
        assert_eq!(
            sl.region_capacity(ProductType::FullRegion).land_impact,
            20_000
        );
        assert_eq!(
            sl.region_capacity(ProductType::Homestead).land_impact,
            7_500
        );
        assert_eq!(sl.region_capacity(ProductType::Openspace).max_agents, 15);
        for product in [ProductType::Homestead, ProductType::Openspace] {
            assert_eq!(
                ImitatedGrid::OpenSim.region_capacity(product).land_impact,
                15_000,
                "OpenSim has no products"
            );
        }

        let owner = |id: u128| OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(id)));
        let parcel = |id: i32, area: u32, held_by: u128| {
            let mut parcel =
                crate::world::region_wide_parcel(RegionLocalParcelId(id), owner(held_by), "Land");
            parcel.area = LandArea(area);
            parcel
        };
        // A quarter and a half for one owner, the last quarter for another.
        let mut parcels = vec![
            parcel(1, 128 * 128, 1),
            parcel(2, 128 * 256, 1),
            parcel(3, 128 * 128, 2),
        ];
        sl.region_capacity(ProductType::FullRegion)
            .allot(&mut parcels);
        let allowances: Vec<(i32, i32)> = parcels
            .iter()
            .map(|parcel| (parcel.max_prims, parcel.sim_wide_max_prims))
            .collect();
        assert_eq!(
            allowances,
            vec![(5_000, 15_000), (10_000, 15_000), (5_000, 5_000)]
        );
    }

    /// The two grids take opposite sides of every knob derived here. That is
    /// not a coincidence worth asserting for its own sake — it is the check
    /// that a knob added later actually *decides* something, rather than
    /// answering both flavours the same way and only looking derived.
    #[test]
    fn every_derived_knob_separates_the_two_grids() {
        let sl = ImitatedGrid::SecondLife;
        let opensim = ImitatedGrid::OpenSim;
        assert_ne!(sl.object_assets(), opensim.object_assets());
        assert_ne!(sl.honors_login_options(), opensim.honors_login_options());
        assert_ne!(
            sl.advertises_open_sim_extras(),
            opensim.advertises_open_sim_extras()
        );
        assert_ne!(sl.voice_backend(), opensim.voice_backend());
        assert_ne!(sl.legacy_udp_inventory(), opensim.legacy_udp_inventory());
        assert_ne!(
            sl.inventory_announcement(),
            opensim.inventory_announcement()
        );
        assert_ne!(sl.upload_announcements(), opensim.upload_announcements());
        assert_ne!(sl.bakes(), opensim.bakes());
        assert_ne!(sl.region_protocol_bits(), opensim.region_protocol_bits());
        assert_ne!(sl.prices(), opensim.prices());
        assert_ne!(
            sl.describes_account_entitlements(),
            opensim.describes_account_entitlements()
        );
        assert_ne!(sl.sit_policy(), opensim.sit_policy());
        let (sl_parcels, opensim_parcels) = (sl.parcel_policy(), opensim.parcel_policy());
        assert_ne!(
            sl_parcels.answers_request_by_id,
            opensim_parcels.answers_request_by_id
        );
        assert_ne!(sl_parcels.edit_echo, opensim_parcels.edit_echo);
        assert_ne!(
            sl_parcels.udp_edit_nulls_media_type,
            opensim_parcels.udp_edit_nulls_media_type
        );
        assert_ne!(
            sl_parcels.unset_media_loops,
            opensim_parcels.unset_media_loops
        );
        assert_ne!(
            sl_parcels.sends_extended_blocks,
            opensim_parcels.sends_extended_blocks
        );
        assert_ne!(
            sl_parcels.parcel_return_reads_task_ids,
            opensim_parcels.parcel_return_reads_task_ids
        );
        assert_ne!(
            sl_parcels.owners_reply_over_event_queue,
            opensim_parcels.owners_reply_over_event_queue
        );
        assert_ne!(sl_parcels.wire_types, opensim_parcels.wire_types);
        assert_ne!(
            sl.environment_change_reply(),
            opensim.environment_change_reply()
        );
        assert_ne!(sl.stock_day(), opensim.stock_day());
        assert_ne!(sl.logout_reply(), opensim.logout_reply());
        assert_ne!(sl.circuit_policy(), opensim.circuit_policy());
        assert_ne!(sl.arrival_policy(), opensim.arrival_policy());
        assert_ne!(sl.terrain_policy(), opensim.terrain_policy());
        assert_ne!(sl.teleport_policy(), opensim.teleport_policy());
        let (sl_view, opensim_view) = (sl.neighbour_policy(), opensim.neighbour_policy());
        assert_ne!(sl_view.reach, opensim_view.reach);
        assert_ne!(sl_view.retire_delay, opensim_view.retire_delay);
        assert_ne!(sl_view.states_region_size, opensim_view.states_region_size);
        assert_ne!(sl_view.crossing_look_at, opensim_view.crossing_look_at);
        assert_ne!(
            sl_view.handshake_on_crossing,
            opensim_view.handshake_on_crossing
        );
        assert_ne!(
            sl.stock_simulator_features(),
            opensim.stock_simulator_features()
        );
        assert_ne!(
            sl.region_capacity(sl_proto::ProductType::FullRegion),
            opensim.region_capacity(sl_proto::ProductType::FullRegion)
        );
    }

    /// The draw distances each grid was measured holding a neighbour at
    /// (2026-10-07): on aditi 126 m did not reach a region sharing an edge
    /// and 128 m did, 180 m did not reach the one touching at a corner and
    /// 184 m did; on OpenSim, from the middle of a region, 32 m reached
    /// nothing and 100 m everything.
    #[test]
    fn each_grid_reaches_the_neighbours_it_was_measured_reaching() {
        let sl = ImitatedGrid::SecondLife.neighbour_policy();
        // Where the aditi avatar stood: eight metres from two borders, so no
        // figure below is a distance to anything.
        let corner = (7.7, 10.1);
        let (west, south, south_west) = ((-1, 0), (0, -1), (-1, -1));
        for edge in [west, south] {
            assert!(!sl.reaches(126.0, corner, edge));
            assert!(sl.reaches(128.0, corner, edge));
        }
        assert!(!sl.reaches(180.0, corner, south_west));
        assert!(sl.reaches(184.0, corner, south_west));
        // ... and the same from anywhere else.
        assert!(!sl.reaches(126.0, (128.0, 128.0), west));

        let opensim = ImitatedGrid::OpenSim.neighbour_policy();
        let middle = (128.0, 128.0);
        for neighbour in [(1, 0), (0, 1), (1, 1)] {
            assert!(!opensim.reaches(32.0, middle, neighbour));
            assert!(opensim.reaches(100.0, middle, neighbour));
            assert!(opensim.reaches(512.0, middle, neighbour));
        }
        // Twelve metres from the eastern border even the smallest view
        // reaches east, and still not north.
        assert!(opensim.reaches(32.0, (244.0, 128.0), (1, 0)));
        assert!(!opensim.reaches(32.0, (244.0, 128.0), (0, 1)));
        // And from the western edge the largest does not reach east at all.
        assert!(!opensim.reaches(512.0, (0.0, 128.0), (1, 0)));
    }

    /// The bake policy is four coupled advertisements, and the flavour has to
    /// carry all four: a grid that says it is OpenSim while still naming an
    /// appearance service is the shape of divergence this whole module exists
    /// to make impossible, and one that drops the service while still stamping
    /// an `AppearanceData` block is the *worse* shape — every avatar a cloud,
    /// silently.
    #[test]
    fn an_open_sim_flavoured_grid_bakes_nothing_of_its_own() {
        let opensim = ImitatedGrid::OpenSim.bakes();
        assert_eq!(opensim, BakePolicy::ClientSide);
        assert!(!opensim.advertises_appearance_service());
        assert!(!opensim.grants_bake_capability());
        assert_eq!(opensim.region_protocol_bits(), 0);

        let sl = ImitatedGrid::SecondLife.bakes();
        assert_eq!(sl, BakePolicy::ServerSide);
        assert!(sl.advertises_appearance_service());
        assert!(sl.grants_bake_capability());
        assert_eq!(
            sl.region_protocol_bits(),
            crate::bakes::REGION_PROTOCOL_SERVER_BAKES
        );
    }

    /// The two halves of `RegionProtocols` are independent, and the whole field
    /// is what a region handshake carries: OpenSim's bit 63 is measured from
    /// `LLClientView.SendRegionHandshake`, and it survives a grid that has been
    /// told to bake server-side anyway.
    #[test]
    fn the_region_protocol_halves_compose() {
        assert_eq!(
            ImitatedGrid::OpenSim.region_protocol_bits()
                | ImitatedGrid::OpenSim.bakes().region_protocol_bits(),
            REGION_PROTOCOL_BAKES_ON_MESH
        );
        assert_eq!(
            ImitatedGrid::SecondLife.region_protocol_bits()
                | ImitatedGrid::SecondLife.bakes().region_protocol_bits(),
            crate::bakes::REGION_PROTOCOL_SERVER_BAKES
        );
        // An explicit override moves only its own half.
        assert_eq!(
            ImitatedGrid::OpenSim.region_protocol_bits()
                | BakePolicy::ServerSide.region_protocol_bits(),
            REGION_PROTOCOL_BAKES_ON_MESH | crate::bakes::REGION_PROTOCOL_SERVER_BAKES
        );
    }

    /// The inventory pair is the one place the flavour decides both ends of the
    /// same divergence, and getting either backwards would let a viewer that
    /// depends on the legacy path pass against a grid claiming to be Second
    /// Life — the exact failure this task existed to make impossible.
    #[test]
    fn only_the_open_sim_flavour_speaks_legacy_inventory() {
        assert_eq!(
            ImitatedGrid::OpenSim.legacy_udp_inventory(),
            LegacyUdpInventory::Served
        );
        assert_eq!(
            ImitatedGrid::OpenSim.inventory_announcement(),
            InventoryAnnouncement::Legacy
        );
        assert_eq!(
            ImitatedGrid::SecondLife.legacy_udp_inventory(),
            LegacyUdpInventory::Refused
        );
        assert_eq!(
            ImitatedGrid::SecondLife.inventory_announcement(),
            InventoryAnnouncement::BulkUpdate
        );
    }

    /// A take and a *save* are announced by **opposite** rules, and each
    /// flavour is on the other side of the two. Written as one test because
    /// the mistake it guards against is reusing one answer for both questions:
    /// every part of it is a measurement (`notecard-create-update`'s
    /// `save_announcement`, `asset-upload`'s `upload_announcement`,
    /// `object-asset-format`'s take leg), so a "tidying" that collapsed the two
    /// enums would be wrong about a live grid in both directions at once.
    #[test]
    fn a_take_and_a_save_are_announced_by_opposite_rules() {
        assert_eq!(
            ImitatedGrid::SecondLife.inventory_announcement(),
            InventoryAnnouncement::BulkUpdate
        );
        assert_eq!(
            ImitatedGrid::SecondLife.upload_announcements().saved,
            UploadAnnouncement::Legacy
        );
        assert_eq!(
            ImitatedGrid::OpenSim.inventory_announcement(),
            InventoryAnnouncement::Legacy
        );
        assert_eq!(
            ImitatedGrid::OpenSim.upload_announcements().saved,
            UploadAnnouncement::Silent
        );
    }

    /// The half of the upload table the two grids **agree** on, which is the
    /// half that was got wrong by reasoning from the other half.
    ///
    /// It is asserted rather than left implicit because the wrong answer is the
    /// tempting one: Second Life sends the legacy push after an in-place save,
    /// and "so it must send it after a creation too" is what the fake grid
    /// served until the aditi run (2026-09-08, `asset-upload`) recorded `none`
    /// on both grids. A future flavour that starts announcing a creation should
    /// have to delete this test on purpose.
    #[test]
    fn no_live_grid_announces_an_item_a_capability_upload_created() {
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            assert_eq!(
                grid.upload_announcements().created,
                UploadAnnouncement::Silent,
                "{grid:?} announced an item a NewFileAgentInventory upload created"
            );
        }
    }

    /// The fake grid's own default has to be the one Second Life actually is,
    /// and the other side has to be silence rather than a Vivox fixture: this
    /// workspace implements no Vivox-shaped voice, so a grid defaulting to it
    /// would serve a path nothing here speaks.
    #[test]
    fn the_stock_grid_speaks_webrtc_and_the_other_one_speaks_nothing() {
        assert_eq!(
            ImitatedGrid::SecondLife.voice_backend(),
            VoiceBackend::WebRtc
        );
        assert_eq!(ImitatedGrid::OpenSim.voice_backend(), VoiceBackend::Silent);
        assert_eq!(VoiceBackend::default(), VoiceBackend::WebRtc);
    }

    /// **The two flavours answer an update's completion differently**, and the
    /// stock grid is the strict one.
    ///
    /// This is the divergence a client is most likely to depend on without
    /// noticing, because the lenient answer makes a broken correlation work: a
    /// viewer that matches "my save landed" on the echoed item passes against
    /// OpenSim and hangs on "Saving…" against Second Life. Modelling only the
    /// lenient side is what let exactly that ship — so the default has to be the
    /// side that fails it, and the other side has to remain reachable, or a run
    /// imitating OpenSim is not imitating OpenSim.
    #[test]
    fn only_the_open_sim_flavour_echoes_a_rewritten_item() {
        assert_eq!(
            ImitatedGrid::SecondLife.update_completion_item(),
            UpdateCompletionItem::Omitted
        );
        assert_eq!(
            ImitatedGrid::OpenSim.update_completion_item(),
            UpdateCompletionItem::Echoed
        );
        assert_eq!(
            UpdateCompletionItem::default(),
            UpdateCompletionItem::Omitted
        );
        assert!(!UpdateCompletionItem::Omitted.names_item());
        assert!(UpdateCompletionItem::Echoed.names_item());
    }
}
