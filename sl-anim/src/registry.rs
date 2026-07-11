//! The **built-in agent-animation library**: the fixed asset UUIDs Second Life
//! and OpenSim reserve for the standard avatar animations (walk, stand, wave,
//! the dances and emotes, …), paired with how a viewer produces each one.
//!
//! Every avatar animation the simulator signals in an `AvatarAnimation` update
//! is one of these fixed UUIDs or an *uploaded* animation asset. Resolving one
//! to its playable [`Motion`](crate::Motion) (P18.2) differs by
//! [`BuiltinKind`]:
//!
//! - A [`BuiltinKind::Keyframe`] built-in is an ordinary keyframe-motion
//!   (`.anim`) asset stored on the grid's asset server under its fixed UUID; a
//!   viewer fetches and decodes it exactly like an uploaded animation. The
//!   reference viewer registers these with `LLKeyframeMotion` **or one of its
//!   subclasses** — `LLKeyframeWalkMotion` (the walks/runs/turns),
//!   `LLKeyframeStandMotion` (the stands/crouch), and `LLKeyframeFallMotion`
//!   (`standup`) all extend `LLKeyframeMotion`, which fetches the asset by UUID
//!   (`gAssetStorage->getAssetData`); the subclass merely layers a *procedural
//!   adjustment* (foot IK / torso facing / fall recovery) on top of the
//!   downloaded keyframe motion. So the whole locomotion set is downloadable, not
//!   synthesised — the earlier classification of walk/run/stand/turn/crouch as
//!   [`BuiltinKind::Procedural`] was a bug that stopped a viewer from ever
//!   fetching them (P31.6).
//! - A [`BuiltinKind::Procedural`] built-in has **no** downloadable asset: the
//!   reference viewer synthesises it from a dedicated C++ motion class with no
//!   `LLKeyframeMotion` base — `LLEmote` for the facial expressions, `LLNullMotion`
//!   for `do_not_disturb`, and the always-on adjusters
//!   (head/eye/hand/breathe/physics/pelvis/walk-adjust, which the simulator never
//!   signals in an `AvatarAnimation` and so are absent from this table). Fetching
//!   such a UUID over the asset capability would 404, so a resolver skips the fetch.
//!
//! The table is pure data ported from the reference viewer
//! (`indra/llcharacter/llanimationstates.cpp` for the UUIDs;
//! `indra/newview/llvoavatar.cpp`'s `registerMotion` block for which motion class
//! backs each — a `LLKeyframeMotion` subclass ⟹ [`BuiltinKind::Keyframe`],
//! anything else ⟹ [`BuiltinKind::Procedural`]), keeping this crate Bevy-free and
//! I/O-free — the fetch/decode and the procedural synthesis live in the runtime /
//! viewer layers.

use uuid::{Uuid, uuid};

/// How a viewer produces a built-in agent animation, deciding whether its fixed
/// UUID resolves to a downloadable asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    /// A downloadable keyframe-motion (`.anim`) asset, fetched and decoded from
    /// the grid's asset server under the fixed UUID (reference viewer:
    /// `LLKeyframeMotion`).
    Keyframe,
    /// A procedurally generated motion with no downloadable asset — the
    /// reference viewer synthesises it from a bespoke C++ motion class that does
    /// **not** extend `LLKeyframeMotion` (the `LLEmote` facial expressions, the
    /// `LLNullMotion` do-not-disturb, and the always-on head/eye/hand/breathe/
    /// physics adjusters). The walks/stands/turns are **not** here: their
    /// `LLKeyframe*Motion` classes download a keyframe asset, so they are
    /// [`Keyframe`](Self::Keyframe).
    Procedural,
}

/// One built-in agent animation: a short human-readable name (the reference
/// viewer's `ANIM_AGENT_*` constant, lowercased with the prefix stripped), its
/// fixed asset UUID, and how a viewer produces it.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinAnimation {
    /// A short lowercase label (e.g. `"walk"`, `"hello"`, `"dance1"`), for
    /// logging; it is *not* the on-wire identifier — the [`id`](Self::id) is.
    pub name: &'static str,
    /// The animation's fixed asset UUID, as signalled in an `AvatarAnimation`.
    pub id: Uuid,
    /// How a viewer produces this animation.
    pub kind: BuiltinKind,
}

impl BuiltinAnimation {
    /// Whether this built-in resolves to a downloadable `.anim` asset (a
    /// [`BuiltinKind::Keyframe`]) rather than a procedural motion — i.e. whether
    /// a resolver should fetch its UUID over the asset capability.
    #[must_use]
    pub const fn is_downloadable(&self) -> bool {
        matches!(self.kind, BuiltinKind::Keyframe)
    }
}

/// Construct one [`BuiltinAnimation`] table entry (a `const` helper that keeps
/// the large table below readable).
const fn entry(name: &'static str, id: Uuid, kind: BuiltinKind) -> BuiltinAnimation {
    BuiltinAnimation { name, id, kind }
}

/// The complete built-in agent-animation table, ported verbatim from the
/// reference viewer's `ANIM_AGENT_*` definitions. Order follows the source; look
/// an entry up by UUID with [`builtin_animation`].
pub const BUILTIN_ANIMATIONS: &[BuiltinAnimation] = &[
    entry(
        "afraid",
        uuid!("6b61c8e8-4747-0d75-12d7-e49ff207a4ca"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "aim_bazooka_r",
        uuid!("b5b4a67d-0aee-30d2-72cd-77b333e932ef"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "aim_bow_l",
        uuid!("46bb4359-de38-4ed8-6a22-f1f52fe8f506"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "aim_handgun_r",
        uuid!("3147d815-6338-b932-f011-16b56d9ac18b"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "aim_rifle_r",
        uuid!("ea633413-8006-180a-c3ba-96dd1d756720"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "angry",
        uuid!("5747a48e-073e-c331-f6f3-7c2149613d3e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "away",
        uuid!("fd037134-85d4-f241-72c6-4f42164fedee"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "backflip",
        uuid!("c4ca6188-9127-4f31-0158-23c4e2f93304"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "belly_laugh",
        uuid!("18b3a4b5-b463-bd48-e4b6-71eaac76c515"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "blow_kiss",
        uuid!("db84829b-462c-ee83-1e27-9bbee66bd624"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "bored",
        uuid!("b906c4ba-703b-1940-32a3-0c7f7d791510"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "bow",
        uuid!("82e99230-c906-1403-4d9c-3889dd98daba"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "brush",
        uuid!("349a3801-54f9-bf2c-3bd0-1ac89772af01"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "do_not_disturb",
        uuid!("efcf670c-2d18-8128-973a-034ebc806b67"),
        BuiltinKind::Procedural,
    ),
    entry(
        "clap",
        uuid!("9b0c1c4e-8ac7-7969-1494-28c874c4f668"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "courtbow",
        uuid!("9ba1c942-08be-e43a-fb29-16ad440efc50"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "crouch",
        uuid!("201f3fdf-cb1f-dbec-201f-7333e328ae7c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "crouchwalk",
        uuid!("47f5f6fb-22e5-ae44-f871-73aaaf4a6022"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "cry",
        uuid!("92624d3e-1068-f1aa-a5ec-8244585193ed"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "customize",
        uuid!("038fcec9-5ebd-8a8e-0e2e-6e71a0a1ac53"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "customize_done",
        uuid!("6883a61a-b27b-5914-a61e-dda118a9ee2c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance1",
        uuid!("b68a3d7c-de9e-fc87-eec8-543d787e5b0d"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance2",
        uuid!("928cae18-e31d-76fd-9cc9-2f55160ff818"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance3",
        uuid!("30047778-10ea-1af7-6881-4db7a3a5a114"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance4",
        uuid!("951469f4-c7b2-c818-9dee-ad7eea8c30b7"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance5",
        uuid!("4bd69a1d-1114-a0b4-625f-84e0a5237155"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance6",
        uuid!("cd28b69b-9c95-bb78-3f94-8d605ff1bb12"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance7",
        uuid!("a54d8ee2-28bb-80a9-7f0c-7afbbe24a5d6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dance8",
        uuid!("b0dc417c-1f11-af36-2e80-7e7489fa7cdc"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "dead",
        uuid!("57abaae6-1d17-7b1b-5f98-6d11a6411276"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "drink",
        uuid!("0f86e355-dd31-a61c-fdb0-3a96b9aad05f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "embarrassed",
        uuid!("514af488-9051-044a-b3fc-d4dbf76377c6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "express_afraid",
        uuid!("aa2df84d-cf8f-7218-527b-424a52de766e"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_anger",
        uuid!("1a03b575-9634-b62a-5767-3a679e81f4de"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_bored",
        uuid!("214aa6c1-ba6a-4578-f27c-ce7688f61d0d"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_cry",
        uuid!("d535471b-85bf-3b4d-a542-93bea4f59d33"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_disdain",
        uuid!("d4416ff1-09d3-300f-4183-1b68a19b9fc1"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_embarrassed",
        uuid!("0b8c8211-d78c-33e8-fa28-c51a9594e424"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_frown",
        uuid!("fee3df48-fa3d-1015-1e26-a205810e3001"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_kiss",
        uuid!("1e8d90cc-a84e-e135-884c-7c82c8b03a14"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_laugh",
        uuid!("62570842-0950-96f8-341c-809e65110823"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_open_mouth",
        uuid!("d63bc1f9-fc81-9625-a0c6-007176d82eb7"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_repulsed",
        uuid!("f76cda94-41d4-a229-2872-e0296e58afe1"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_sad",
        uuid!("eb6ebfb2-a4b3-a19c-d388-4dd5c03823f7"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_shrug",
        uuid!("a351b1bc-cc94-aac2-7bea-a7e6ebad15ef"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_smile",
        uuid!("b7c7c833-e3d3-c4e3-9fc0-131237446312"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_surprise",
        uuid!("728646d9-cc79-08b2-32d6-937f0a835c24"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_tongue_out",
        uuid!("835965c6-7f2f-bda2-5deb-2478737f91bf"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_toothsmile",
        uuid!("b92ec1a5-e7ce-a76b-2b05-bcdb9311417e"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_wink",
        uuid!("da020525-4d94-59d6-23d7-81fdebf33148"),
        BuiltinKind::Procedural,
    ),
    entry(
        "express_worry",
        uuid!("9c05e5c7-6f07-6ca4-ed5a-b230390c3950"),
        BuiltinKind::Procedural,
    ),
    entry(
        "falldown",
        uuid!("666307d9-a860-572d-6fd4-c3ab8865c094"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "female_run_new",
        uuid!("85995026-eade-5d78-d364-94a64512cb66"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "female_walk",
        uuid!("f5fc7433-043d-e819-8298-f519a119b688"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "female_walk_new",
        uuid!("d60c41d2-7c24-7074-d3fa-6101cea22a51"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "finger_wag",
        uuid!("c1bc7f36-3ba0-d844-f93c-93be945d644f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "fist_pump",
        uuid!("7db00ccd-f380-f3ee-439d-61968ec69c8a"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "fly",
        uuid!("aec4610c-757f-bc4e-c092-c6e9caf18daf"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "flyslow",
        uuid!("2b5a38b2-5e00-3a97-a495-4c826bc443e6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hello",
        uuid!("9b29cd61-c45b-5689-ded2-91756b8d76a9"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hold_bazooka_r",
        uuid!("ef62d355-c815-4816-2474-b1acc21094a6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hold_bow_l",
        uuid!("8b102617-bcba-037b-86c1-b76219f90c88"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hold_handgun_r",
        uuid!("efdc1727-8b8a-c800-4077-975fc27ee2f2"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hold_rifle_r",
        uuid!("3d94bad0-c55b-7dcc-8763-033c59405d33"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hold_throw_r",
        uuid!("7570c7b5-1f22-56dd-56ef-a9168241bbb6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hover",
        uuid!("4ae8016b-31b9-03bb-c401-b1ea941db41d"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hover_down",
        uuid!("20f063ea-8306-2562-0b07-5c853b37b31e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "hover_up",
        uuid!("62c5de58-cb33-5743-3d07-9e4cd4352864"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "impatient",
        uuid!("5ea3991f-c293-392e-6860-91dfa01278a3"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "jump",
        uuid!("2305bd75-1ca9-b03b-1faa-b176b8a8c49e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "jump_for_joy",
        uuid!("709ea28e-1573-c023-8bf8-520c8bc637fa"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "kiss_my_butt",
        uuid!("19999406-3a3a-d58c-a2ac-d72e555dcf51"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "land",
        uuid!("7a17b059-12b2-41b1-570a-186368b6aa6f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "laugh_short",
        uuid!("ca5b3f14-3194-7a2b-c894-aa699b718d1f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "medium_land",
        uuid!("f4f00d6e-b9fe-9292-f4cb-0ae06ea58d57"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "motorcycle_sit",
        uuid!("08464f78-3a8e-2944-cba5-0c94aff3af29"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "muscle_beach",
        uuid!("315c3a41-a5f3-0ba4-27da-f893f769e69b"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "no",
        uuid!("5a977ed9-7f72-44e9-4c4c-6e913df8ae74"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "no_unhappy",
        uuid!("d83fa0e5-97ed-7eb2-e798-7bd006215cb4"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "nyah_nyah",
        uuid!("f061723d-0a18-754f-66ee-29a44795a32f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "onetwo_punch",
        uuid!("eefc79be-daae-a239-8c04-890f5d23654a"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "peace",
        uuid!("b312b10e-65ab-a0a4-8b3c-1326ea8e3ed9"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "point_me",
        uuid!("17c024cc-eef2-f6a0-3527-9869876d7752"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "point_you",
        uuid!("ec952cca-61ef-aa3b-2789-4d1344f016de"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "pre_jump",
        uuid!("7a4e87fe-de39-6fcb-6223-024b00893244"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "punch_left",
        uuid!("f3300ad9-3462-1d07-2044-0fef80062da0"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "punch_right",
        uuid!("c8e42d32-7310-6906-c903-cab5d4a34656"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "repulsed",
        uuid!("36f81a92-f076-5893-dc4b-7c3795e487cf"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "roundhouse_kick",
        uuid!("49aea43b-5ac3-8a44-b595-96100af0beda"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "rps_countdown",
        uuid!("35db4f7e-28c2-6679-cea9-3ee108f7fc7f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "rps_paper",
        uuid!("0836b67f-7f7b-f37b-c00a-460dc1521f5a"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "rps_rock",
        uuid!("42dd95d5-0bc6-6392-f650-777304946c0f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "rps_scissors",
        uuid!("16803a9f-5140-e042-4d7b-d28ba247c325"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "run",
        uuid!("05ddbff8-aaa9-92a1-2b74-8fe77a29b445"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "run_new",
        uuid!("1ab1b236-cd08-21e6-0cbc-0d923fc6eca2"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sad",
        uuid!("0eb702e2-cc5a-9a88-56a5-661a55c0676a"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "salute",
        uuid!("cd7668a6-7011-d7e2-ead8-fc69eff1a104"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "shoot_bow_l",
        uuid!("e04d450d-fdb5-0432-fd68-818aaf5935f8"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "shout",
        uuid!("6bd01860-4ebd-127a-bb3d-d1427e8e0c42"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "shrug",
        uuid!("70ea714f-3a97-d742-1b01-590a8fcd1db5"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit",
        uuid!("1a5fe8ac-a804-8a5d-7cbd-56bd83184568"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit_female",
        uuid!("b1709c8d-ecd3-54a1-4f28-d55ac0840782"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit_generic",
        uuid!("245f3c54-f1c0-bf2e-811f-46d8eeb386e7"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit_ground",
        uuid!("1c7600d6-661f-b87b-efe2-d7421eb93c86"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit_ground_constrained",
        uuid!("1a2bd58e-87ff-0df8-0b4c-53e047b0bb6e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sit_to_stand",
        uuid!("a8dee56f-2eae-9e7a-05a2-6fb92b97e21e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sleep",
        uuid!("f2bed5f9-9d44-39af-b0cd-257b2a17fe40"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "smoke_idle",
        uuid!("d2f2ee58-8ad1-06c9-d8d3-3827ba31567a"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "smoke_inhale",
        uuid!("6802d553-49da-0778-9f85-1599a2266526"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "smoke_throw_down",
        uuid!("0a9fb970-8b44-9114-d3a9-bf69cfe804d6"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "snapshot",
        uuid!("eae8905b-271a-99e2-4c0e-31106afd100c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stand",
        uuid!("2408fe9e-df1d-1d7d-f4ff-1384fa7b350f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "standup",
        uuid!("3da1d753-028a-5446-24f3-9c9b856d9422"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stand_1",
        uuid!("15468e00-3400-bb66-cecc-646d7c14458e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stand_2",
        uuid!("370f3a20-6ca6-9971-848c-9a01bc42ae3c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stand_3",
        uuid!("42b46214-4b44-79ae-deb8-0df61424ff4b"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stand_4",
        uuid!("f22fed8b-a5ed-2c93-64d5-bdd8b93c889f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stretch",
        uuid!("80700431-74ec-a008-14f8-77575e73693f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "stride",
        uuid!("1cb562b0-ba21-2202-efb3-30f82cdf9595"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "surf",
        uuid!("41426836-7437-7e89-025d-0aa4d10f1d69"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "surprise",
        uuid!("313b9881-4302-73c0-c7d0-0e7a36b6c224"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "sword_strike",
        uuid!("85428680-6bf9-3e64-b489-6f81087c24bd"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "talk",
        uuid!("5c682a95-6da4-a463-0bf6-0f5b7be129d1"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "tantrum",
        uuid!("11000694-3f41-adc2-606b-eee1d66f3724"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "throw_r",
        uuid!("aa134404-7dac-7aca-2cba-435f9db875ca"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "tryon_shirt",
        uuid!("83ff59fe-2346-f236-9009-4e3608af64c1"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "turnleft",
        uuid!("56e0ba0d-4a9f-7f27-6117-32f2ebbf6135"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "turnright",
        uuid!("2d6daa51-3192-6794-8e2e-a15f8338ec30"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "type",
        uuid!("c541c47f-e0c0-058b-ad1a-d6ae3a4584d9"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "walk",
        uuid!("6ed24bd8-91aa-4b12-ccc7-c97c857ab4e0"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "walk_new",
        uuid!("33339176-7ddc-9397-94a4-bf3403cbc8f5"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "whisper",
        uuid!("7693f268-06c7-ea71-fa21-2b30d6533f8f"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "whistle",
        uuid!("b1ed7982-c68e-a982-7561-52a88a5298c0"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "wink",
        uuid!("869ecdad-a44b-671e-3266-56aef2e3ac2e"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "wink_hollywood",
        uuid!("c0c4030f-c02b-49de-24ba-2331f43fe41c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "worry",
        uuid!("9f496bd2-589a-709f-16cc-69bf7df1d36c"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "yes",
        uuid!("15dd911d-be82-2856-26db-27659b142875"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "yes_happy",
        uuid!("b8c8b2a3-9008-1771-3bfc-90924955ab2d"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "yoga_float",
        uuid!("42ecd00b-9947-a97c-400a-bbc9174c7aeb"),
        BuiltinKind::Keyframe,
    ),
    entry(
        "bento_idle",
        uuid!("0e720142-0d4b-cf1d-cae5-277f58299604"),
        BuiltinKind::Keyframe,
    ),
];

/// Look a built-in agent animation up by its fixed asset UUID, or `None` if the
/// UUID is not a reserved built-in (i.e. it is an uploaded animation asset).
#[must_use]
pub fn builtin_animation(id: Uuid) -> Option<&'static BuiltinAnimation> {
    BUILTIN_ANIMATIONS
        .iter()
        .find(|animation| animation.id == id)
}

/// Look a built-in agent animation up by its short lowercase [`name`], or `None`
/// if no built-in carries that name. This is the reverse of [`builtin_animation`]:
/// a caller that knows the *state* it wants (e.g. `"walk"`, `"stand"`, `"fly"`)
/// resolves it to the fixed asset UUID to fetch/play — used by the viewer's
/// client-side locomotion-animation fallback (P31.6). Names are unique in the
/// table (`uuids_are_unique` guards the ids; the names come straight from the
/// distinct `ANIM_AGENT_*` constants), so the first match is the only one.
///
/// [`name`]: BuiltinAnimation::name
#[must_use]
pub fn builtin_animation_by_name(name: &str) -> Option<&'static BuiltinAnimation> {
    BUILTIN_ANIMATIONS
        .iter()
        .find(|animation| animation.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn table_has_every_builtin() {
        // The count ported from `llanimationstates.cpp` (140 `ANIM_AGENT_*`).
        assert_eq!(BUILTIN_ANIMATIONS.len(), 140);
    }

    #[test]
    fn uuids_are_unique() {
        let mut ids: Vec<Uuid> = BUILTIN_ANIMATIONS
            .iter()
            .map(|animation| animation.id)
            .collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate built-in animation UUID");
    }

    #[test]
    fn walks_and_stands_are_downloadable_keyframes() -> Result<(), TestError> {
        // The locomotion motions download a keyframe asset: the reference viewer's
        // `LLKeyframeWalkMotion` / `LLKeyframeStandMotion` / `LLKeyframeFallMotion`
        // all extend `LLKeyframeMotion` (which fetches the asset by UUID) and only
        // layer a procedural *adjustment* on top. They are NOT procedural (P31.6).
        for name in [
            "walk",
            "walk_new",
            "run",
            "run_new",
            "stand",
            "stand_1",
            "standup",
            "crouch",
            "crouchwalk",
            "turnleft",
            "turnright",
            "female_walk",
            "female_run_new",
        ] {
            let animation = builtin_animation_by_name(name).ok_or("built-in present")?;
            assert_eq!(
                animation.kind,
                BuiltinKind::Keyframe,
                "{name} downloads a keyframe asset"
            );
            assert!(animation.is_downloadable());
        }
        Ok(())
    }

    #[test]
    fn emotes_and_null_stay_procedural() -> Result<(), TestError> {
        // The genuinely procedural built-ins — `LLEmote` expressions and the
        // `LLNullMotion` do-not-disturb — have no downloadable asset.
        for name in ["express_smile", "express_afraid", "do_not_disturb"] {
            let animation = builtin_animation_by_name(name).ok_or("built-in present")?;
            assert_eq!(
                animation.kind,
                BuiltinKind::Procedural,
                "{name} is procedural"
            );
            assert!(!animation.is_downloadable());
        }
        Ok(())
    }

    #[test]
    fn lookup_by_name_resolves_and_rejects() -> Result<(), TestError> {
        let walk = builtin_animation_by_name("walk").ok_or("walk is a built-in")?;
        assert_eq!(walk.name, "walk");
        assert_eq!(walk.kind, BuiltinKind::Keyframe);
        assert!(builtin_animation_by_name("not_an_animation").is_none());
        Ok(())
    }

    #[test]
    fn gestures_are_downloadable_keyframes() -> Result<(), TestError> {
        // The waves / bows / dances are ordinary `.anim` assets.
        for name in ["hello", "bow", "dance1", "clap", "blow_kiss"] {
            let animation = BUILTIN_ANIMATIONS
                .iter()
                .find(|animation| animation.name == name)
                .ok_or("built-in present")?;
            assert_eq!(
                animation.kind,
                BuiltinKind::Keyframe,
                "{name} is a keyframe asset"
            );
            assert!(animation.is_downloadable());
        }
        Ok(())
    }

    #[test]
    fn lookup_resolves_a_known_uuid() -> Result<(), TestError> {
        // `ANIM_AGENT_HELLO`.
        let hello = uuid!("9b29cd61-c45b-5689-ded2-91756b8d76a9");
        let found = builtin_animation(hello).ok_or("hello is a built-in")?;
        assert_eq!(found.name, "hello");
        assert_eq!(found.kind, BuiltinKind::Keyframe);
        Ok(())
    }

    #[test]
    fn lookup_rejects_an_unknown_uuid() {
        assert!(builtin_animation(uuid!("00000000-0000-0000-0000-000000000000")).is_none());
    }
}
