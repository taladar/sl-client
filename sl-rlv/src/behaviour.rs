//! The RLV / RLVa behaviour vocabulary — the `behaviour` keyword of an
//! `@behaviour[:option]=param` command, and the dictionary that maps a keyword
//! onto it.
//!
//! Two tables live here, mirroring the two maps the reference builds in its
//! `RlvBehaviourDictionary` constructor (`rlvhelper.cpp:80`):
//!
//! - [`RlvBehaviour`] is the set of **canonical behaviours** — `ERlvBehaviour`.
//!   This is the identity a restriction is reference-counted under, so two
//!   keywords that name the same restriction must map to the same variant.
//! - [`RlvEntry`] is one **dictionary row**: a wire keyword, the param kind it
//!   is declared for, the canonical behaviour it names, and its flags. The
//!   reference keys this map on the pair `(keyword, param type)`
//!   (`m_String2InfoMap`, `rlvhelper.cpp:340`), so `@tpto=force` is the
//!   teleport action while `@tpto=n` is nothing at all — there is no such
//!   restriction. [`RlvBehaviour::resolve`] is what a lookup has to pass.
//!
//! The split is what makes **synonyms** expressible: `@touchfar=n` and
//! `@fartouch=n` are two rows naming one [`RlvBehaviour::Fartouch`], and an
//! object holding both holds one restriction, not two.

use crate::command::RlvParamKind;

/// Declarative table of the canonical RLV behaviours (`ERlvBehaviour`).
///
/// A variant here is a **reference-counting slot**, not a keyword: the wire
/// keywords that reach it are the [`RlvEntry`] rows below, and several of them
/// may name the same variant.
macro_rules! rlv_behaviours {
    ( $( $variant:ident $( = $doc:literal )? ; )* ) => {
        /// A canonical RLV / RLVa behaviour — one entry of `ERlvBehaviour`.
        ///
        /// This is the identity under which a restriction is reference-counted
        /// and asked about, *not* the spelling that arrived on the wire. A
        /// command keeps its raw keyword on
        /// [`RlvCommand::keyword`](crate::RlvCommand::keyword), so a synonym or
        /// an unrecognised keyword never loses its spelling.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum RlvBehaviour {
            $(
                $( #[doc = $doc] )?
                $variant,
            )*
            /// A behaviour keyword this decoder does not recognise (a
            /// newer-than-us or malformed behaviour), or a known keyword used
            /// with a param kind it is not declared for. The keyword text is
            /// kept on [`RlvCommand::keyword`](crate::RlvCommand::keyword).
            Unknown,
        }

        impl RlvBehaviour {
            /// Every canonical behaviour, in table order.
            ///
            /// [`RlvBehaviour::Unknown`] is not a behaviour and is not in this
            /// slice.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];
        }
    };
}

rlv_behaviours! {
    Detach = "`@detach` — lock an attachment on (restriction) or take it off (action).";
    Addattach = "`@addattach` — block attaching to an attachment point.";
    Remattach = "`@remattach` — block detaching from an attachment point.";
    Addoutfit = "`@addoutfit` — block wearing a wearable layer.";
    Remoutfit = "`@remoutfit` — block removing a wearable layer.";
    Sharedwear = "`@sharedwear` — block wearing from the `#RLV` shared folder.";
    Sharedunwear = "`@sharedunwear` — block unwearing from `#RLV`.";
    Unsharedwear = "`@unsharedwear` — block wearing from outside `#RLV`.";
    Unsharedunwear = "`@unsharedunwear` — block unwearing from outside `#RLV`.";
    Emote = "`@emote` — force emotes through the chat filter.";
    Sendchat = "`@sendchat` — block public chat.";
    Recvchat = "`@recvchat` — block incoming public chat.";
    Recvchatfrom = "`@recvchatfrom` — block incoming chat from one avatar.";
    Recvemote = "`@recvemote` — block incoming emotes.";
    Recvemotefrom = "`@recvemotefrom` — block incoming emotes from one avatar.";
    Redirchat = "`@redirchat` — redirect public chat to a channel.";
    Rediremote = "`@rediremote` — redirect emotes to a channel.";
    Chatwhisper = "`@chatwhisper` — force whispered chat up to normal.";
    Chatnormal = "`@chatnormal` — force chat to normal volume.";
    Chatshout = "`@chatshout` — force shouted chat down to normal.";
    Sendchannel = "`@sendchannel` — block chat on scripted channels.";
    SendchannelExcept = "`@sendchannel_except` — block all channels but the listed ones.";
    Sendim = "`@sendim` — block sending instant messages.";
    Sendimto = "`@sendimto` — block sending IMs to one avatar.";
    Recvim = "`@recvim` — block receiving instant messages.";
    Recvimfrom = "`@recvimfrom` — block receiving IMs from one avatar.";
    Startim = "`@startim` — block starting an IM session.";
    Startimto = "`@startimto` — block starting an IM session with one avatar.";
    Sendgesture = "`@sendgesture` — block playing gestures.";
    Permissive = "`@permissive` — force every exception-carrying restriction into strict mode.";
    Notify = "`@notify` — report restriction changes on a channel.";
    Share = "`@share` — block giving inventory to other avatars.";
    Showinv = "`@showinv` — hide the inventory.";
    Showminimap = "`@showminimap` — hide the minimap.";
    Showworldmap = "`@showworldmap` — hide the world map.";
    Showloc = "`@showloc` — hide the agent's location.";
    Shownames = "`@shownames` — hide avatar names in lists and chat.";
    Shownametags = "`@shownametags` — hide avatar name tags in world.";
    Shownearby = "`@shownearby` — hide the nearby-avatar list.";
    Showhovertext = "`@showhovertext` — hide one object's hover text.";
    Showhovertexthud = "`@showhovertexthud` — hide HUD hover text.";
    Showhovertextworld = "`@showhovertextworld` — hide in-world hover text.";
    Showhovertextall = "`@showhovertextall` — hide all hover text.";
    Showself = "`@showself` — hide the agent's own avatar.";
    Showselfhead = "`@showselfhead` — hide the agent's own head.";
    Tplm = "`@tplm` — block teleporting by landmark.";
    Tploc = "`@tploc` — block teleporting to a location.";
    Tplocal = "`@tplocal` — block short-range teleports.";
    Tplure = "`@tplure` — block accepting a teleport offer.";
    Tprequest = "`@tprequest` — block requesting a teleport.";
    Viewnote = "`@viewnote` — block opening notecards.";
    Viewscript = "`@viewscript` — block opening scripts.";
    Viewtexture = "`@viewtexture` — block opening textures.";
    Acceptpermission = "`@acceptpermission` — auto-accept script permission requests.";
    Accepttp = "`@accepttp` — auto-accept teleport offers.";
    Accepttprequest = "`@accepttprequest` — auto-accept teleport requests.";
    Allowidle = "`@allowidle` — allow the away/idle animation.";
    Buy = "`@buy` — block buying objects.";
    Edit = "`@edit` — block the build/edit tools.";
    Editattach = "`@editattach` — block editing attachments.";
    Editobj = "`@editobj` — block editing one object.";
    Editworld = "`@editworld` — block editing in-world objects.";
    Viewtransparent = "`@viewtransparent` — block highlighting transparent faces.";
    Viewwireframe = "`@viewwireframe` — block wireframe rendering.";
    Pay = "`@pay` — block paying objects and avatars.";
    Rez = "`@rez` — block rezzing objects.";
    Fartouch = "`@fartouch` — limit touch range (also spelled `@touchfar`).";
    Interact = "`@interact` — block world interaction entirely.";
    Touchthis = "`@touchthis` — block touching one object.";
    Touchattach = "`@touchattach` — block touching attachments.";
    Touchattachself = "`@touchattachself` — block touching own attachments.";
    Touchattachother = "`@touchattachother` — block touching others' attachments.";
    Touchhud = "`@touchhud` — block touching HUDs.";
    Touchworld = "`@touchworld` — block touching in-world objects.";
    Touchall = "`@touchall` — block touching anything.";
    Touchme = "`@touchme` — allow touching the restricting object.";
    Fly = "`@fly` — block flying (restriction) or start/stop flying (action).";
    Jump = "`@jump` — block jumping.";
    Setgroup = "`@setgroup` — block changing the active group (or set it).";
    Unsit = "`@unsit` — block standing up (restriction) or stand up (action).";
    Sit = "`@sit` — block sitting down (restriction) or sit on a target (action).";
    Sitground = "`@sitground` — sit on the ground.";
    Sittp = "`@sittp` — limit the range of a sit teleport.";
    Standtp = "`@standtp` — teleport back to the sit source on standing.";
    Setdebug = "`@setdebug` — give one object control of the debug settings.";
    Setenv = "`@setenv` — give one object control of the environment.";
    Alwaysrun = "`@alwaysrun` — block toggling always-run.";
    Temprun = "`@temprun` — block temporary running.";
    Detachme = "`@detachme` — detach the issuing object.";
    Attachthis = "`@attachthis` / `@attachallthis` — lock a folder against wearing.";
    AttachthisExcept = "`@attachthis_except` — exempt a folder from an attach lock.";
    Detachthis = "`@detachthis` / `@detachallthis` — lock a folder against removal.";
    DetachthisExcept = "`@detachthis_except` — exempt a folder from a detach lock.";
    Adjustheight = "`@adjustheight` — change the avatar's hover height.";
    Getheightoffset = "`@getheightoffset` — report the avatar's hover height.";
    Tpto = "`@tpto` — teleport to a location.";
    Version = "`@version` — report the RLV specification version.";
    Versionnew = "`@versionnew` — report the version, new-style.";
    Versionnum = "`@versionnum` — report the version as a packed number.";
    Getattach = "`@getattach` — report which attachment points are used.";
    Getattachnames = "`@getattachnames` — report the names of used attachment points.";
    Getaddattachnames = "`@getaddattachnames` — report attachable points.";
    Getremattachnames = "`@getremattachnames` — report detachable points.";
    Getoutfit = "`@getoutfit` — report which wearable layers are worn.";
    Getoutfitnames = "`@getoutfitnames` — report the names of worn layers.";
    Getaddoutfitnames = "`@getaddoutfitnames` — report wearable layers.";
    Getremoutfitnames = "`@getremoutfitnames` — report removable layers.";
    Findfolder = "`@findfolder` — find one shared folder by name.";
    Findfolders = "`@findfolders` — find every matching shared folder.";
    Getpath = "`@getpath` — report the shared path of a worn item.";
    Getpathnew = "`@getpathnew` — report the shared path, new-style.";
    Getinv = "`@getinv` — list a shared folder's subfolders.";
    Getinvworn = "`@getinvworn` — list a shared folder with worn markers.";
    Getgroup = "`@getgroup` — report the active group.";
    Getsitid = "`@getsitid` — report the object the avatar is sitting on.";
    Getcommand = "`@getcommand` — report which commands this viewer knows.";
    Getstatus = "`@getstatus` — report the issuing object's restrictions.";
    Getstatusall = "`@getstatusall` — report every object's restrictions.";
    ForceWear = "The internal behaviour every force-wear command shares (`RLV_CMD_FORCEWEAR`).";
    Setcam = "`@setcam` — give one object exclusive control of the camera.";
    SetcamAvdist = "`@setcam_avdist` — distance at which avatars become silhouettes.";
    SetcamAvdistmin = "`@setcam_avdistmin` — minimum camera distance from the avatar.";
    SetcamAvdistmax = "`@setcam_avdistmax` — maximum camera distance from the avatar.";
    SetcamOrigindistmin = "`@setcam_origindistmin` — minimum distance from the focus origin.";
    SetcamOrigindistmax = "`@setcam_origindistmax` — maximum distance from the focus origin.";
    SetcamEyeoffset = "`@setcam_eyeoffset` — override the default camera offset.";
    SetcamEyeoffsetscale = "`@setcam_eyeoffsetscale` — override the camera offset scale.";
    SetcamFocusoffset = "`@setcam_focusoffset` — override the default focus offset.";
    SetcamFocus = "`@setcam_focus` — force the camera focus to a target.";
    SetcamFov = "`@setcam_fov` — set the current field of view.";
    SetcamFovmin = "`@setcam_fovmin` — minimum field of view.";
    SetcamFovmax = "`@setcam_fovmax` — maximum field of view.";
    SetcamMouselook = "`@setcam_mouselook` — block mouselook.";
    SetcamTextures = "`@setcam_textures` — replace every world texture with one texture.";
    SetcamUnlock = "`@setcam_unlock` — force the camera focus back to the avatar.";
    Camzoommin = "`@camzoommin` — deprecated minimum-zoom multiplier.";
    Camzoommax = "`@camzoommax` — deprecated maximum-zoom multiplier.";
    GetcamAvdist = "`@getcam_avdist` — report the silhouette distance.";
    GetcamAvdistmin = "`@getcam_avdistmin` — report the minimum camera distance.";
    GetcamAvdistmax = "`@getcam_avdistmax` — report the maximum camera distance.";
    GetcamFov = "`@getcam_fov` — report the current field of view.";
    GetcamFovmin = "`@getcam_fovmin` — report the minimum field of view.";
    GetcamFovmax = "`@getcam_fovmax` — report the maximum field of view.";
    GetcamTextures = "`@getcam_textures` — report the forced world texture.";
    SetcamMode = "`@setcam_mode` — switch the camera into a named mode.";
    Setsphere = "`@setsphere` — the vision-sphere effect.";
    Setoverlay = "`@setoverlay` — the screen-overlay effect.";
    SetoverlayTouch = "`@setoverlay_touch` — let the overlay's alpha block interaction.";
    SetoverlayTween = "`@setoverlay_tween` — animate the overlay to new values.";
    Clear = "`@clear` — drop the issuing object's restrictions.";
}

/// The flag bits a dictionary row carries (`RlvBehaviourInfo::EBehaviourFlags`,
/// `rlvhelper.h:45`).
///
/// `@getcommand` filters on these, and the strict bit decides whether a `_sec`
/// suffix is a keyword at all, so the decoder has to keep them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RlvBehaviourFlags(u8);

impl RlvBehaviourFlags {
    /// No flags — an ordinary, current, non-strict behaviour.
    pub const NONE: Self = Self(0);
    /// `BHVR_STRICT`: the behaviour has a `_sec` variant.
    pub const STRICT: Self = Self(0x01);
    /// `BHVR_SYNONYM`: this keyword is another spelling of a behaviour that has
    /// its own row.
    pub const SYNONYM: Self = Self(0x02);
    /// `BHVR_EXTENDED`: part of the RLVa extended command set.
    pub const EXTENDED: Self = Self(0x04);
    /// `BHVR_EXPERIMENTAL`: part of the RLVa experimental command set.
    pub const EXPERIMENTAL: Self = Self(0x08);
    /// `BHVR_DEPRECATED`: still accepted, but scripts should stop using it.
    pub const DEPRECATED: Self = Self(0x20);

    /// Every flag, paired with its name, for [`core::fmt::Debug`] and tests.
    const NAMED: &'static [(Self, &'static str)] = &[
        (Self::STRICT, "STRICT"),
        (Self::SYNONYM, "SYNONYM"),
        (Self::EXTENDED, "EXTENDED"),
        (Self::EXPERIMENTAL, "EXPERIMENTAL"),
        (Self::DEPRECATED, "DEPRECATED"),
    ];

    /// Both flag sets together.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether the behaviour accepts the strict `_sec` suffix.
    #[must_use]
    pub const fn is_strict(self) -> bool {
        self.contains(Self::STRICT)
    }

    /// Whether this keyword is a synonym of a behaviour spelled another way.
    #[must_use]
    pub const fn is_synonym(self) -> bool {
        self.contains(Self::SYNONYM)
    }

    /// Whether the behaviour is part of the RLVa extended command set.
    #[must_use]
    pub const fn is_extended(self) -> bool {
        self.contains(Self::EXTENDED)
    }

    /// Whether the behaviour is part of the RLVa experimental command set.
    #[must_use]
    pub const fn is_experimental(self) -> bool {
        self.contains(Self::EXPERIMENTAL)
    }

    /// Whether the behaviour is deprecated.
    #[must_use]
    pub const fn is_deprecated(self) -> bool {
        self.contains(Self::DEPRECATED)
    }
}

impl core::fmt::Debug for RlvBehaviourFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for &(flag, name) in Self::NAMED {
            if self.contains(flag) {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        if first {
            f.write_str("NONE")?;
        }
        Ok(())
    }
}

/// One row of the behaviour dictionary: a wire keyword declared for one param
/// kind.
///
/// The reference's `RlvBehaviourInfo` (`rlvhelper.h:39`), minus the command
/// handler it carries — obeying a command is the consumer's job, classifying it
/// is this crate's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvEntry {
    /// The wire keyword, lower-cased and without any `_sec` suffix.
    pub keyword: &'static str,
    /// The param kind this row is declared for.
    pub kind: RlvParamKind,
    /// The canonical behaviour the keyword names.
    pub behaviour: RlvBehaviour,
    /// The row's flags.
    pub flags: RlvBehaviourFlags,
}

/// Declarative table of every dictionary row.
///
/// Each row is `"keyword" <Kind> => <Behaviour> [<flags>]`. A keyword the
/// reference registers twice — `@sit=n` the restriction and `@sit=force` the
/// action — gets one row per kind, because that is exactly what the reference's
/// `(keyword, param type)` key means.
macro_rules! rlv_dictionary {
    ( $( $kw:literal $kind:ident => $bhvr:ident [ $( $flag:ident )* ] ; )* ) => {
        impl RlvEntry {
            /// Every dictionary row, in table order.
            ///
            /// This is the enumeration the reference's `getCommands` walks to
            /// answer `@getcommand` (`rlvhelper.cpp:468`).
            pub const ALL: &'static [Self] = &[ $(
                Self {
                    keyword: $kw,
                    kind: RlvParamKind::$kind,
                    behaviour: RlvBehaviour::$bhvr,
                    flags: RlvBehaviourFlags::NONE
                        $( .union(RlvBehaviourFlags::$flag) )*,
                },
            )* ];
        }
    };
}

impl RlvEntry {
    /// The row for an exact, already lower-cased keyword used with a param of
    /// `kind`, or `None` when the dictionary has no such row.
    ///
    /// The `_sec` strict suffix is *not* handled here — strip it first (the
    /// decoder does this in [`RlvBehaviour::resolve`]).
    ///
    /// The scan is linear over a couple of hundred rows, which is the right
    /// trade here: the input is chat, arriving a handful of lines at a time,
    /// and one table with no index cannot drift out of step with itself.
    #[must_use]
    pub fn lookup(keyword: &str, kind: RlvParamKind) -> Option<&'static Self> {
        Self::ALL
            .iter()
            .find(|entry| entry.kind == kind && entry.keyword == keyword)
    }

    /// Every keyword whose text contains `filter`, for `@getcommand`
    /// (`RlvBehaviourDictionary::getCommands`, `rlvhelper.cpp:468`).
    ///
    /// `kind` of `None` means any kind. A keyword with a strict form is listed
    /// twice, plain and `_sec`, and each spelling is filtered on its own — so
    /// `@getcommand:_sec` lists only the strict spellings. An empty `filter`
    /// lists everything.
    ///
    /// `experimental` says whether the RLVa experimental command set is
    /// enabled. The reference gates this at registration
    /// (`RlvBehaviourDictionary::addEntry`, `rlvhelper.cpp:376`), so with it
    /// off those keywords are not commands at all — see
    /// [`RlvState::set_experimental_commands`](crate::RlvState::set_experimental_commands).
    #[must_use]
    pub fn commands_matching(
        filter: &str,
        kind: Option<RlvParamKind>,
        experimental: bool,
    ) -> Vec<String> {
        let mut commands = Vec::new();
        for entry in Self::ALL {
            if kind.is_some_and(|kind| kind != entry.kind) {
                continue;
            }
            if !experimental && entry.flags.is_experimental() {
                continue;
            }
            if filter.is_empty() || entry.keyword.contains(filter) {
                commands.push(entry.keyword.to_owned());
            }
            if entry.flags.is_strict() {
                let strict = format!("{}_sec", entry.keyword);
                if filter.is_empty() || strict.contains(filter) {
                    commands.push(strict);
                }
            }
        }
        commands
    }

    /// The non-synonym row for `behaviour` and `kind`, or `None` when the
    /// behaviour is not declared for that kind.
    ///
    /// This is the reference's `getBehaviourInfo(eBhvr, eParamType)`
    /// (`rlvhelper.cpp:422`) — the row that owns the behaviour, as opposed to
    /// the alternative spellings that merely reach it. Where the reference
    /// declares *two* owning rows for one behaviour and kind — the folder-lock
    /// pairs, `@attachthis` beside `@attachallthis`, which differ only in
    /// whether the lock covers the subtree — it gives up and answers nothing;
    /// this answers with the first row, which is the one whose keyword is the
    /// behaviour's plain spelling.
    #[must_use]
    pub fn canonical(behaviour: RlvBehaviour, kind: RlvParamKind) -> Option<&'static Self> {
        Self::ALL.iter().find(|entry| {
            entry.behaviour == behaviour && entry.kind == kind && !entry.flags.is_synonym()
        })
    }
}

rlv_dictionary! {
    // Restrictions.
    "acceptpermission" AddRem => Acceptpermission [];
    "accepttp" AddRem => Accepttp [STRICT];
    "accepttprequest" AddRem => Accepttprequest [STRICT EXTENDED];
    "addattach" AddRem => Addattach [];
    "addoutfit" AddRem => Addoutfit [];
    "allowidle" AddRem => Allowidle [EXPERIMENTAL];
    "alwaysrun" AddRem => Alwaysrun [];
    "attachthis" AddRem => Attachthis [];
    "attachallthis" AddRem => Attachthis [];
    "attachthis_except" AddRem => AttachthisExcept [];
    "attachallthis_except" AddRem => AttachthisExcept [];
    "buy" AddRem => Buy [];
    "chatwhisper" AddRem => Chatwhisper [];
    "chatnormal" AddRem => Chatnormal [];
    "chatshout" AddRem => Chatshout [];
    "detach" AddRem => Detach [];
    "detachthis" AddRem => Detachthis [];
    "detachallthis" AddRem => Detachthis [];
    "detachthis_except" AddRem => DetachthisExcept [];
    "detachallthis_except" AddRem => DetachthisExcept [];
    "edit" AddRem => Edit [];
    "editattach" AddRem => Editattach [];
    "editobj" AddRem => Editobj [];
    "editworld" AddRem => Editworld [];
    "viewtransparent" AddRem => Viewtransparent [EXPERIMENTAL];
    "viewwireframe" AddRem => Viewwireframe [EXPERIMENTAL];
    "emote" AddRem => Emote [];
    "fartouch" AddRem => Fartouch [];
    "fly" AddRem => Fly [];
    "interact" AddRem => Interact [EXTENDED];
    "jump" AddRem => Jump [];
    "notify" AddRem => Notify [];
    "pay" AddRem => Pay [];
    "permissive" AddRem => Permissive [];
    "recvchat" AddRem => Recvchat [STRICT];
    "recvchatfrom" AddRem => Recvchatfrom [STRICT];
    "recvemote" AddRem => Recvemote [STRICT];
    "recvemotefrom" AddRem => Recvemotefrom [STRICT];
    "recvim" AddRem => Recvim [STRICT];
    "recvimfrom" AddRem => Recvimfrom [STRICT];
    "redirchat" AddRem => Redirchat [];
    "rediremote" AddRem => Rediremote [];
    "remattach" AddRem => Remattach [];
    "remoutfit" AddRem => Remoutfit [];
    "rez" AddRem => Rez [];
    "sendchannel" AddRem => Sendchannel [STRICT];
    "sendchannel_except" AddRem => SendchannelExcept [STRICT EXPERIMENTAL];
    "sendchat" AddRem => Sendchat [];
    "sendim" AddRem => Sendim [STRICT];
    "sendimto" AddRem => Sendimto [STRICT];
    "sendgesture" AddRem => Sendgesture [EXPERIMENTAL];
    "setdebug" AddRem => Setdebug [];
    "setenv" AddRem => Setenv [];
    "setgroup" AddRem => Setgroup [];
    "share" AddRem => Share [STRICT];
    "sharedunwear" AddRem => Sharedunwear [EXTENDED];
    "sharedwear" AddRem => Sharedwear [EXTENDED];
    "showhovertext" AddRem => Showhovertext [];
    "showhovertextall" AddRem => Showhovertextall [];
    "showhovertexthud" AddRem => Showhovertexthud [];
    "showhovertextworld" AddRem => Showhovertextworld [];
    "showinv" AddRem => Showinv [];
    "showloc" AddRem => Showloc [];
    "showminimap" AddRem => Showminimap [];
    "shownames" AddRem => Shownames [STRICT];
    "shownametags" AddRem => Shownametags [STRICT];
    "shownearby" AddRem => Shownearby [EXPERIMENTAL];
    "showself" AddRem => Showself [EXPERIMENTAL];
    "showselfhead" AddRem => Showselfhead [EXPERIMENTAL];
    "showworldmap" AddRem => Showworldmap [];
    "sit" AddRem => Sit [];
    "sittp" AddRem => Sittp [];
    "standtp" AddRem => Standtp [];
    "startim" AddRem => Startim [STRICT];
    "startimto" AddRem => Startimto [STRICT];
    "temprun" AddRem => Temprun [];
    "touchall" AddRem => Touchall [];
    "touchattach" AddRem => Touchattach [];
    "touchattachother" AddRem => Touchattachother [];
    "touchattachself" AddRem => Touchattachself [];
    "touchfar" AddRem => Fartouch [SYNONYM];
    "touchhud" AddRem => Touchhud [EXTENDED];
    "touchme" AddRem => Touchme [];
    "touchthis" AddRem => Touchthis [];
    "touchworld" AddRem => Touchworld [];
    "tplm" AddRem => Tplm [];
    "tploc" AddRem => Tploc [];
    "tplocal" AddRem => Tplocal [EXPERIMENTAL];
    "tplure" AddRem => Tplure [STRICT];
    "tprequest" AddRem => Tprequest [STRICT EXTENDED];
    "unsharedunwear" AddRem => Unsharedunwear [];
    "unsharedwear" AddRem => Unsharedwear [];
    "unsit" AddRem => Unsit [];
    "viewnote" AddRem => Viewnote [];
    "viewscript" AddRem => Viewscript [];
    "viewtexture" AddRem => Viewtexture [];

    // Camera restrictions.
    "setcam" AddRem => Setcam [];
    "setcam_avdist" AddRem => SetcamAvdist [];
    "setcam_avdistmin" AddRem => SetcamAvdistmin [EXPERIMENTAL];
    "setcam_avdistmax" AddRem => SetcamAvdistmax [EXPERIMENTAL];
    "setcam_origindistmin" AddRem => SetcamOrigindistmin [EXPERIMENTAL];
    "setcam_origindistmax" AddRem => SetcamOrigindistmax [EXPERIMENTAL];
    "setcam_eyeoffset" AddRem => SetcamEyeoffset [];
    "setcam_eyeoffsetscale" AddRem => SetcamEyeoffsetscale [];
    "setcam_focusoffset" AddRem => SetcamFocusoffset [];
    "setcam_fovmin" AddRem => SetcamFovmin [];
    "setcam_fovmax" AddRem => SetcamFovmax [];
    "setcam_mouselook" AddRem => SetcamMouselook [];
    "setcam_textures" AddRem => SetcamTextures [];
    "setcam_unlock" AddRem => SetcamUnlock [];
    // Camera restrictions (compatibility shims).
    "camavdist" AddRem => SetcamAvdist [SYNONYM DEPRECATED];
    "camdistmin" AddRem => SetcamAvdistmin [SYNONYM DEPRECATED];
    "camdistmax" AddRem => SetcamAvdistmax [SYNONYM DEPRECATED];
    "camtextures" AddRem => SetcamTextures [SYNONYM DEPRECATED];
    "camzoommin" AddRem => Camzoommin [DEPRECATED];
    "camzoommax" AddRem => Camzoommax [DEPRECATED];
    "camunlock" AddRem => SetcamUnlock [SYNONYM DEPRECATED];

    // Effect restrictions.
    "setoverlay" AddRem => Setoverlay [];
    "setoverlay_touch" AddRem => SetoverlayTouch [];
    "setsphere" AddRem => Setsphere [];

    // Force-wear.
    "attach" Force => ForceWear [];
    "attachall" Force => ForceWear [];
    "attachover" Force => ForceWear [];
    "attachallover" Force => ForceWear [];
    "attachthis" Force => ForceWear [];
    "attachallthis" Force => ForceWear [];
    "attachthisover" Force => ForceWear [];
    "attachallthisover" Force => ForceWear [];
    "detach" Force => Detach [];
    "detachall" Force => ForceWear [];
    "detachthis" Force => ForceWear [];
    "detachallthis" Force => ForceWear [];
    "remattach" Force => Remattach [];
    "remoutfit" Force => Remoutfit [];
    // Force-wear synonyms (`addoutfit*` -> `attach*`).
    "addoutfit" Force => ForceWear [SYNONYM];
    "addoutfitall" Force => ForceWear [SYNONYM];
    "addoutfitover" Force => ForceWear [SYNONYM];
    "addoutfitallover" Force => ForceWear [SYNONYM];
    "addoutfitthis" Force => ForceWear [SYNONYM];
    "addoutfitallthis" Force => ForceWear [SYNONYM];
    "addoutfitthisover" Force => ForceWear [SYNONYM];
    "addoutfitallthisover" Force => ForceWear [SYNONYM];
    // Force-wear synonyms (`attach*overorreplace` -> `attach*`).
    "attachoverorreplace" Force => ForceWear [SYNONYM];
    "attachalloverorreplace" Force => ForceWear [SYNONYM];
    "attachthisoverorreplace" Force => ForceWear [SYNONYM];
    "attachallthisoverorreplace" Force => ForceWear [SYNONYM];

    // Force-only.
    "adjustheight" Force => Adjustheight [];
    "detachme" Force => Detachme [];
    "fly" Force => Fly [];
    "setcam_focus" Force => SetcamFocus [EXPERIMENTAL];
    "setcam_eyeoffset" Force => SetcamEyeoffset [];
    "setcam_eyeoffsetscale" Force => SetcamEyeoffsetscale [];
    "setcam_focusoffset" Force => SetcamFocusoffset [];
    "setcam_fov" Force => SetcamFov [EXPERIMENTAL];
    "setcam_mode" Force => SetcamMode [EXPERIMENTAL];
    "setgroup" Force => Setgroup [];
    "sit" Force => Sit [];
    "sitground" Force => Sitground [];
    "tpto" Force => Tpto [];
    "unsit" Force => Unsit [];
    "setoverlay_tween" Force => SetoverlayTween [];

    // Reply-only.
    "findfolder" Reply => Findfolder [];
    "findfolders" Reply => Findfolders [EXTENDED];
    "getaddattachnames" Reply => Getaddattachnames [EXPERIMENTAL];
    "getaddoutfitnames" Reply => Getaddoutfitnames [EXPERIMENTAL];
    "getattach" Reply => Getattach [];
    "getattachnames" Reply => Getattachnames [EXPERIMENTAL];
    "getcam_avdist" Reply => GetcamAvdist [EXPERIMENTAL];
    "getcam_avdistmin" Reply => GetcamAvdistmin [EXPERIMENTAL];
    "getcam_avdistmax" Reply => GetcamAvdistmax [EXPERIMENTAL];
    "getcam_fov" Reply => GetcamFov [EXPERIMENTAL];
    "getcam_fovmin" Reply => GetcamFovmin [EXPERIMENTAL];
    "getcam_fovmax" Reply => GetcamFovmax [EXPERIMENTAL];
    "getcam_textures" Reply => GetcamTextures [EXPERIMENTAL];
    "getcommand" Reply => Getcommand [EXTENDED];
    "getgroup" Reply => Getgroup [];
    "getheightoffset" Reply => Getheightoffset [EXTENDED];
    "getinv" Reply => Getinv [];
    "getinvworn" Reply => Getinvworn [];
    "getoutfit" Reply => Getoutfit [];
    "getoutfitnames" Reply => Getoutfitnames [EXPERIMENTAL];
    "getpath" Reply => Getpath [];
    "getpathnew" Reply => Getpathnew [];
    "getremattachnames" Reply => Getremattachnames [EXPERIMENTAL];
    "getremoutfitnames" Reply => Getremoutfitnames [EXPERIMENTAL];
    "getsitid" Reply => Getsitid [];
    "getstatus" Reply => Getstatus [];
    "getstatusall" Reply => Getstatusall [];
    "version" Reply => Version [];
    "versionnew" Reply => Versionnew [];
    "versionnum" Reply => Versionnum [];

    // `@clear` is a param type of its own in the reference; the decoder gives it
    // a behaviour so every command has one.
    "clear" Clear => Clear [];
}

/// Declarative table of the **local behaviour modifiers** — the named knobs a
/// restriction exposes as `@<behaviour>_<modifier>=force`
/// (`ERlvLocalBhvrModifier`).
///
/// Each row is `Variant = Behaviour "name" <type>`: the restriction the modifier
/// hangs off, the suffix that addresses it, and the type its option is parsed
/// as. The reference registers these on the behaviour's *restriction* entry
/// (`RlvBehaviourInfo::addModifier`, `rlvhelper.cpp:152-171`), which is why the
/// lookup fallback in [`RlvBehaviour::resolve`] only consults
/// [`RlvParamKind::AddRem`] rows.
macro_rules! rlv_local_modifiers {
    ( $( $variant:ident = $base:ident $kw:literal $ty:ident ; )* ) => {
        /// A named modifier of a restriction, addressed by a `=force` command
        /// whose keyword is `<behaviour>_<modifier>` (`ERlvLocalBhvrModifier`).
        ///
        /// `@setsphere_mode=force` is not a behaviour of its own: it sets the
        /// `mode` modifier of the `@setsphere` restriction the same object
        /// already holds. The decoder reports it as the base behaviour plus
        /// this, on [`RlvCommand::modifier`](crate::RlvCommand::modifier).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum RlvLocalModifier {
            $(
                #[doc = concat!("The `", $kw, "` modifier of `@", stringify!($base), "`.")]
                $variant,
            )*
        }

        impl RlvLocalModifier {
            /// Every declared local modifier, in table order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The modifier `name` addresses on `behaviour`, or `None` if that
            /// behaviour has no such modifier.
            #[must_use]
            pub fn lookup(behaviour: RlvBehaviour, name: &str) -> Option<Self> {
                match (behaviour, name) {
                    $( (RlvBehaviour::$base, $kw) => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// The restriction this modifier belongs to.
            #[must_use]
            pub const fn behaviour(self) -> RlvBehaviour {
                match self {
                    $( Self::$variant => RlvBehaviour::$base, )*
                }
            }

            /// The suffix that addresses this modifier.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $( Self::$variant => $kw, )*
                }
            }

            /// The type this modifier's option is parsed as.
            #[must_use]
            pub const fn value_type(self) -> RlvValueType {
                match self {
                    $( Self::$variant => RlvValueType::$ty, )*
                }
            }
        }
    };
}

/// The type of a behaviour-modifier value — which arm of
/// [`RlvModifierValue`](crate::RlvModifierValue) a slot holds.
///
/// The reference stores the type as the `std::type_index` of the modifier's
/// default value and checks it on every write (`RlvBehaviourModifier::addValue`,
/// `rlvhelper.cpp:569`); naming the type up front says the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvValueType {
    /// A single `f32` — a distance, an angle, an alpha, a duration.
    Float,
    /// A single `i32` — a mode selector.
    Int,
    /// Three `f32`s parsed from `x/y/z` — an offset or a colour.
    Vector3,
    /// Four `f32`s parsed from `x/y/z/w` — effect parameters.
    Vector4,
    /// A UUID — a texture.
    Uuid,
}

rlv_local_modifiers! {
    OverlayAlpha = Setoverlay "alpha" Float;
    OverlayTexture = Setoverlay "texture" Uuid;
    OverlayTint = Setoverlay "tint" Vector3;
    SphereMode = Setsphere "mode" Int;
    SphereOrigin = Setsphere "origin" Int;
    SphereColor = Setsphere "color" Vector3;
    SphereDistmin = Setsphere "distmin" Float;
    SphereDistmax = Setsphere "distmax" Float;
    SphereDistextend = Setsphere "distextend" Int;
    SphereParams = Setsphere "param" Vector4;
    SphereTween = Setsphere "tween" Float;
    SphereValuemin = Setsphere "valuemin" Float;
    SphereValuemax = Setsphere "valuemax" Float;
}

/// What a keyword resolved to, once both the keyword and the param kind of the
/// command have been taken into account.
///
/// This is the return of [`RlvBehaviour::resolve`] and the source of the
/// [`behaviour`](crate::RlvCommand::behaviour), [`strict`](crate::RlvCommand::strict)
/// and [`modifier`](crate::RlvCommand::modifier) fields of a decoded command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvResolvedBehaviour {
    /// The canonical behaviour, or [`RlvBehaviour::Unknown`] if the keyword is
    /// not declared for this param kind. For a local modifier command this is
    /// the **base** restriction, matching `RlvCommand::getBehaviourType`.
    pub behaviour: RlvBehaviour,
    /// Whether the keyword carried the strict `_sec` suffix *and* the behaviour
    /// supports it.
    pub strict: bool,
    /// The local modifier the keyword addressed, if it resolved through the
    /// modifier fallback rather than as a behaviour of its own.
    pub modifier: Option<RlvLocalModifier>,
    /// The dictionary row the keyword resolved through, or `None` when it
    /// resolved to nothing. A synonym resolves through *its own* row, so the
    /// spelling that arrived is still recoverable, along with its flags.
    pub entry: Option<&'static RlvEntry>,
}

impl RlvBehaviour {
    /// The canonical wire keyword for this behaviour when used with `kind`, or
    /// `None` when the behaviour is not declared for that kind.
    ///
    /// A synonym never answers here: `Fartouch.canonical_keyword(AddRem)` is
    /// `"fartouch"`, never `"touchfar"`.
    #[must_use]
    pub fn canonical_keyword(self, kind: RlvParamKind) -> Option<&'static str> {
        RlvEntry::canonical(self, kind).map(|entry| entry.keyword)
    }

    /// The canonical behaviour for an exact, already lower-cased keyword used
    /// with a param of `kind`, or `None` if the dictionary has no such row.
    ///
    /// The `_sec` strict suffix is *not* handled here — strip it first (the
    /// decoder does this in [`RlvBehaviour::resolve`]).
    #[must_use]
    pub fn from_keyword(keyword: &str, kind: RlvParamKind) -> Option<Self> {
        RlvEntry::lookup(keyword, kind).map(|entry| entry.behaviour)
    }

    /// Whether this behaviour is declared for `kind` — the reference's
    /// `(keyword, param type)` dictionary key, asked one axis at a time.
    ///
    /// `@tpto` is a force-only action, so `Tpto.accepts(RlvParamKind::AddRem)`
    /// is `false` and `@tpto=n` is not a restriction but a nonsense command.
    #[must_use]
    pub fn accepts(self, kind: RlvParamKind) -> bool {
        RlvEntry::canonical(self, kind).is_some()
    }

    /// Whether this behaviour is a restriction — declared for
    /// [`RlvParamKind::AddRem`], and so something that can be held.
    #[must_use]
    pub fn is_restriction(self) -> bool {
        self.accepts(RlvParamKind::AddRem)
    }

    /// Whether this behaviour accepts the strict `_sec` suffix
    /// (`@recvim_sec=n` and friends).
    ///
    /// This is the reference's `getHasStrict` (`rlvhelper.cpp:485`): only a
    /// restriction can be strict, so the answer comes from the behaviour's
    /// [`RlvParamKind::AddRem`] row and is `false` for anything else.
    #[must_use]
    pub fn has_strict(self) -> bool {
        RlvEntry::canonical(self, RlvParamKind::AddRem).is_some_and(|entry| entry.flags.is_strict())
    }

    /// Resolve a raw keyword against the dictionary for a command of `kind`.
    ///
    /// This is the reference's `getBehaviourInfo(strBhvr, eParamType, ...)`
    /// (`rlvhelper.cpp:436-458`), in three steps:
    ///
    /// 1. a trailing `_sec` selects the strict variant, and is stripped before
    ///    the lookup;
    /// 2. the base keyword is looked up **for this param kind**, so a keyword
    ///    declared only for `=force` does not answer an `=n`, and vice versa;
    /// 3. failing that, a `=force` command may still be addressing a local
    ///    modifier of a restriction — `@setsphere_mode=force` — which is looked
    ///    up by splitting the keyword at its last `_`.
    ///
    /// A keyword that resolves to nothing yields [`RlvBehaviour::Unknown`]; the
    /// caller keeps the raw text.
    #[must_use]
    pub fn resolve(keyword: &str, kind: RlvParamKind) -> RlvResolvedBehaviour {
        let (base, strict) = match keyword.strip_suffix("_sec") {
            Some(base) => (base, true),
            None => (keyword, false),
        };

        // The strict gate is applied to the entry that was found, exactly as
        // the reference does: `_sec` on a behaviour that has no strict variant
        // is not a behaviour at all.
        if let Some(entry) =
            RlvEntry::lookup(base, kind).filter(|entry| !strict || entry.flags.is_strict())
        {
            return RlvResolvedBehaviour {
                behaviour: entry.behaviour,
                strict,
                modifier: None,
                entry: Some(entry),
            };
        }

        // Local behaviour-modifier fallback. Only for `=force`, only when the
        // keyword was not strict, and only against the restriction rows.
        if !strict && kind == RlvParamKind::Force {
            // `rsplit_once` stands in for the reference's `find_last_of('_')`
            // plus its `!strBhvrLastPart.empty()` guard: a keyword with no `_`,
            // or one that ends in `_`, names no modifier.
            if let Some((modifier_base, name)) =
                keyword.rsplit_once('_').filter(|it| !it.1.is_empty())
                && let Some((entry, modifier)) =
                    RlvEntry::lookup(modifier_base, RlvParamKind::AddRem).and_then(|entry| {
                        RlvLocalModifier::lookup(entry.behaviour, name)
                            .map(|modifier| (entry, modifier))
                    })
            {
                return RlvResolvedBehaviour {
                    behaviour: entry.behaviour,
                    strict: false,
                    modifier: Some(modifier),
                    entry: Some(entry),
                };
            }
        }

        RlvResolvedBehaviour {
            behaviour: Self::Unknown,
            strict: false,
            modifier: None,
            entry: None,
        }
    }
}
