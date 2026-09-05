//! The RLV / RLVa behaviour vocabulary — the `behaviour` keyword of an
//! `@behaviour[:option]=param` command.
//!
//! `RlvBehaviour` is a fieldless classification of the ~175 behaviour keywords
//! the reference viewer knows (`ERlvBehaviour` plus the wire synonyms and
//! deprecated aliases in `RlvBehaviourDictionary`). It is deliberately *only*
//! the classification: [`RlvCommand`](crate::RlvCommand) always keeps the raw
//! keyword text, so an unrecognised or future keyword round-trips as
//! [`RlvBehaviour::Unknown`] without losing its spelling.
//!
//! A keyword on its own does not identify a behaviour. The reference keys its
//! dictionary on the pair `(keyword, param type)` (`m_String2InfoMap`,
//! `rlvhelper.cpp:340`), so `@tpto=force` is the teleport action while
//! `@tpto=n` is nothing at all — there is no such restriction. Every row here
//! therefore carries the set of [`RlvParamKind`]s it is declared for, and
//! [`RlvBehaviour::accepts`] is what a lookup has to pass.

use crate::command::RlvParamKind;

/// Declarative table of every known RLV behaviour keyword.
///
/// Each row is `Variant = "keyword" strict <bool> params [<kinds>]`:
///
/// - the boolean records whether the behaviour accepts the strict `_sec`
///   suffix (`BHVR_STRICT` in the reference dictionary);
/// - the bracketed list is the set of param kinds the reference declares an
///   entry for. A keyword the reference registers twice — `@sit=n` the
///   restriction and `@sit=force` the action — lists both.
///
/// The macro expands the table into the enum plus the lookups, so the keyword
/// list has a single source of truth.
macro_rules! rlv_behaviours {
    ( $( $variant:ident = $kw:literal strict $strict:literal params [ $( $kind:ident ),+ ] ; )* ) => {
        /// A classified RLV / RLVa behaviour keyword.
        ///
        /// This is the `behaviour` part of an `@behaviour[:option]=param`
        /// command, mapped to a typed variant. Keywords the decoder does not
        /// know map to [`RlvBehaviour::Unknown`]; the raw text is kept on
        /// [`RlvCommand::keyword`](crate::RlvCommand::keyword).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum RlvBehaviour {
            $(
                #[doc = concat!("The `@", $kw, "` behaviour.")]
                $variant,
            )*
            /// A behaviour keyword this decoder does not recognise (a
            /// newer-than-us or malformed behaviour), or a known keyword used
            /// with a param kind it is not declared for. The keyword text is
            /// kept on [`RlvCommand::keyword`](crate::RlvCommand::keyword).
            Unknown,
        }

        impl RlvBehaviour {
            /// Every declared behaviour, in table order.
            ///
            /// [`RlvBehaviour::Unknown`] is not a declared behaviour and is not
            /// in this slice. This is the enumeration the reference's
            /// `getCommands` walks to answer `@getcommand`.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The classified behaviour for an exact, already lower-cased
            /// keyword, or `None` if the keyword is not one this decoder knows.
            ///
            /// This is the keyword axis alone. A decoder wants
            /// [`RlvBehaviour::resolve`], which also applies the param-kind
            /// axis the reference dictionary is keyed on; a bare keyword lookup
            /// answers "is this a keyword at all", not "is this command real".
            ///
            /// The `_sec` strict suffix is *not* handled here — strip it first
            /// (the decoder does this in
            /// [`RlvCommand::parse_field`](crate::RlvCommand::parse_field)).
            #[must_use]
            pub fn from_keyword(keyword: &str) -> Option<Self> {
                match keyword {
                    $( $kw => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// The canonical wire keyword for this behaviour, or `None` for
            /// [`RlvBehaviour::Unknown`].
            #[must_use]
            pub const fn keyword(self) -> Option<&'static str> {
                match self {
                    $( Self::$variant => Some($kw), )*
                    Self::Unknown => None,
                }
            }

            /// Whether this behaviour accepts the strict `_sec` suffix
            /// (`@recvim_sec=n` and friends). `false` for
            /// [`RlvBehaviour::Unknown`].
            #[must_use]
            pub const fn has_strict(self) -> bool {
                match self {
                    $( Self::$variant => $strict, )*
                    Self::Unknown => false,
                }
            }

            /// The param kinds this behaviour is declared for — the second half
            /// of the reference's `(keyword, param type)` dictionary key.
            ///
            /// Empty for [`RlvBehaviour::Unknown`].
            #[must_use]
            pub const fn param_kinds(self) -> &'static [RlvParamKind] {
                match self {
                    $( Self::$variant => &[ $( RlvParamKind::$kind, )+ ], )*
                    Self::Unknown => &[],
                }
            }
        }
    };
}

rlv_behaviours! {
    Acceptpermission = "acceptpermission" strict false params [AddRem];
    Accepttp = "accepttp" strict true params [AddRem];
    Accepttprequest = "accepttprequest" strict true params [AddRem];
    Addattach = "addattach" strict false params [AddRem];
    Addoutfit = "addoutfit" strict false params [AddRem, Force];
    Addoutfitall = "addoutfitall" strict false params [Force];
    Addoutfitallover = "addoutfitallover" strict false params [Force];
    Addoutfitallthis = "addoutfitallthis" strict false params [Force];
    Addoutfitallthisover = "addoutfitallthisover" strict false params [Force];
    Addoutfitover = "addoutfitover" strict false params [Force];
    Addoutfitthis = "addoutfitthis" strict false params [Force];
    Addoutfitthisover = "addoutfitthisover" strict false params [Force];
    Adjustheight = "adjustheight" strict false params [Force];
    Allowidle = "allowidle" strict false params [AddRem];
    Alwaysrun = "alwaysrun" strict false params [AddRem];
    Attach = "attach" strict false params [Force];
    Attachall = "attachall" strict false params [Force];
    Attachallover = "attachallover" strict false params [Force];
    Attachalloverorreplace = "attachalloverorreplace" strict false params [Force];
    Attachallthis = "attachallthis" strict false params [AddRem, Force];
    AttachallthisExcept = "attachallthis_except" strict false params [AddRem];
    Attachallthisover = "attachallthisover" strict false params [Force];
    Attachallthisoverorreplace = "attachallthisoverorreplace" strict false params [Force];
    Attachover = "attachover" strict false params [Force];
    Attachoverorreplace = "attachoverorreplace" strict false params [Force];
    Attachthis = "attachthis" strict false params [AddRem, Force];
    AttachthisExcept = "attachthis_except" strict false params [AddRem];
    Attachthisover = "attachthisover" strict false params [Force];
    Attachthisoverorreplace = "attachthisoverorreplace" strict false params [Force];
    Buy = "buy" strict false params [AddRem];
    Camavdist = "camavdist" strict false params [AddRem];
    Camdistmax = "camdistmax" strict false params [AddRem];
    Camdistmin = "camdistmin" strict false params [AddRem];
    Camtextures = "camtextures" strict false params [AddRem];
    Camunlock = "camunlock" strict false params [AddRem];
    Camzoommax = "camzoommax" strict false params [AddRem];
    Camzoommin = "camzoommin" strict false params [AddRem];
    Chatnormal = "chatnormal" strict false params [AddRem];
    Chatshout = "chatshout" strict false params [AddRem];
    Chatwhisper = "chatwhisper" strict false params [AddRem];
    Detach = "detach" strict false params [AddRem, Force];
    Detachall = "detachall" strict false params [Force];
    Detachallthis = "detachallthis" strict false params [AddRem, Force];
    DetachallthisExcept = "detachallthis_except" strict false params [AddRem];
    Detachme = "detachme" strict false params [Force];
    Detachthis = "detachthis" strict false params [AddRem, Force];
    DetachthisExcept = "detachthis_except" strict false params [AddRem];
    Edit = "edit" strict false params [AddRem];
    Editattach = "editattach" strict false params [AddRem];
    Editobj = "editobj" strict false params [AddRem];
    Editworld = "editworld" strict false params [AddRem];
    Emote = "emote" strict false params [AddRem];
    Fartouch = "fartouch" strict false params [AddRem];
    Findfolder = "findfolder" strict false params [Reply];
    Findfolders = "findfolders" strict false params [Reply];
    Fly = "fly" strict false params [AddRem, Force];
    Getaddattachnames = "getaddattachnames" strict false params [Reply];
    Getaddoutfitnames = "getaddoutfitnames" strict false params [Reply];
    Getattach = "getattach" strict false params [Reply];
    Getattachnames = "getattachnames" strict false params [Reply];
    GetcamAvdist = "getcam_avdist" strict false params [Reply];
    GetcamAvdistmax = "getcam_avdistmax" strict false params [Reply];
    GetcamAvdistmin = "getcam_avdistmin" strict false params [Reply];
    GetcamFov = "getcam_fov" strict false params [Reply];
    GetcamFovmax = "getcam_fovmax" strict false params [Reply];
    GetcamFovmin = "getcam_fovmin" strict false params [Reply];
    GetcamTextures = "getcam_textures" strict false params [Reply];
    Getcommand = "getcommand" strict false params [Reply];
    Getgroup = "getgroup" strict false params [Reply];
    Getheightoffset = "getheightoffset" strict false params [Reply];
    Getinv = "getinv" strict false params [Reply];
    Getinvworn = "getinvworn" strict false params [Reply];
    Getoutfit = "getoutfit" strict false params [Reply];
    Getoutfitnames = "getoutfitnames" strict false params [Reply];
    Getpath = "getpath" strict false params [Reply];
    Getpathnew = "getpathnew" strict false params [Reply];
    Getremattachnames = "getremattachnames" strict false params [Reply];
    Getremoutfitnames = "getremoutfitnames" strict false params [Reply];
    Getsitid = "getsitid" strict false params [Reply];
    Getstatus = "getstatus" strict false params [Reply];
    Getstatusall = "getstatusall" strict false params [Reply];
    Interact = "interact" strict false params [AddRem];
    Jump = "jump" strict false params [AddRem];
    Notify = "notify" strict false params [AddRem];
    Pay = "pay" strict false params [AddRem];
    Permissive = "permissive" strict false params [AddRem];
    Recvchat = "recvchat" strict true params [AddRem];
    Recvchatfrom = "recvchatfrom" strict true params [AddRem];
    Recvemote = "recvemote" strict true params [AddRem];
    Recvemotefrom = "recvemotefrom" strict true params [AddRem];
    Recvim = "recvim" strict true params [AddRem];
    Recvimfrom = "recvimfrom" strict true params [AddRem];
    Redirchat = "redirchat" strict false params [AddRem];
    Rediremote = "rediremote" strict false params [AddRem];
    Remattach = "remattach" strict false params [AddRem, Force];
    Remoutfit = "remoutfit" strict false params [AddRem, Force];
    Rez = "rez" strict false params [AddRem];
    Sendchannel = "sendchannel" strict true params [AddRem];
    SendchannelExcept = "sendchannel_except" strict true params [AddRem];
    Sendchat = "sendchat" strict false params [AddRem];
    Sendgesture = "sendgesture" strict false params [AddRem];
    Sendim = "sendim" strict true params [AddRem];
    Sendimto = "sendimto" strict true params [AddRem];
    Setcam = "setcam" strict false params [AddRem];
    SetcamAvdist = "setcam_avdist" strict false params [AddRem];
    SetcamAvdistmax = "setcam_avdistmax" strict false params [AddRem];
    SetcamAvdistmin = "setcam_avdistmin" strict false params [AddRem];
    SetcamEyeoffset = "setcam_eyeoffset" strict false params [AddRem, Force];
    SetcamEyeoffsetscale = "setcam_eyeoffsetscale" strict false params [AddRem, Force];
    SetcamFocus = "setcam_focus" strict false params [Force];
    SetcamFocusoffset = "setcam_focusoffset" strict false params [AddRem, Force];
    SetcamFov = "setcam_fov" strict false params [Force];
    SetcamFovmax = "setcam_fovmax" strict false params [AddRem];
    SetcamFovmin = "setcam_fovmin" strict false params [AddRem];
    SetcamMode = "setcam_mode" strict false params [Force];
    SetcamMouselook = "setcam_mouselook" strict false params [AddRem];
    SetcamOrigindistmax = "setcam_origindistmax" strict false params [AddRem];
    SetcamOrigindistmin = "setcam_origindistmin" strict false params [AddRem];
    SetcamTextures = "setcam_textures" strict false params [AddRem];
    SetcamUnlock = "setcam_unlock" strict false params [AddRem];
    Setdebug = "setdebug" strict false params [AddRem];
    Setenv = "setenv" strict false params [AddRem];
    Setgroup = "setgroup" strict false params [AddRem, Force];
    Setoverlay = "setoverlay" strict false params [AddRem];
    SetoverlayTouch = "setoverlay_touch" strict false params [AddRem];
    SetoverlayTween = "setoverlay_tween" strict false params [Force];
    Setsphere = "setsphere" strict false params [AddRem];
    Share = "share" strict true params [AddRem];
    Sharedunwear = "sharedunwear" strict false params [AddRem];
    Sharedwear = "sharedwear" strict false params [AddRem];
    Showhovertext = "showhovertext" strict false params [AddRem];
    Showhovertextall = "showhovertextall" strict false params [AddRem];
    Showhovertexthud = "showhovertexthud" strict false params [AddRem];
    Showhovertextworld = "showhovertextworld" strict false params [AddRem];
    Showinv = "showinv" strict false params [AddRem];
    Showloc = "showloc" strict false params [AddRem];
    Showminimap = "showminimap" strict false params [AddRem];
    Shownames = "shownames" strict true params [AddRem];
    Shownametags = "shownametags" strict true params [AddRem];
    Shownearby = "shownearby" strict false params [AddRem];
    Showself = "showself" strict false params [AddRem];
    Showselfhead = "showselfhead" strict false params [AddRem];
    Showworldmap = "showworldmap" strict false params [AddRem];
    Sit = "sit" strict false params [AddRem, Force];
    Sitground = "sitground" strict false params [Force];
    Sittp = "sittp" strict false params [AddRem];
    Standtp = "standtp" strict false params [AddRem];
    Startim = "startim" strict true params [AddRem];
    Startimto = "startimto" strict true params [AddRem];
    Temprun = "temprun" strict false params [AddRem];
    Touchall = "touchall" strict false params [AddRem];
    Touchattach = "touchattach" strict false params [AddRem];
    Touchattachother = "touchattachother" strict false params [AddRem];
    Touchattachself = "touchattachself" strict false params [AddRem];
    Touchfar = "touchfar" strict false params [AddRem];
    Touchhud = "touchhud" strict false params [AddRem];
    Touchme = "touchme" strict false params [AddRem];
    Touchthis = "touchthis" strict false params [AddRem];
    Touchworld = "touchworld" strict false params [AddRem];
    Tplm = "tplm" strict false params [AddRem];
    Tploc = "tploc" strict false params [AddRem];
    Tplocal = "tplocal" strict false params [AddRem];
    Tplure = "tplure" strict true params [AddRem];
    Tprequest = "tprequest" strict true params [AddRem];
    Tpto = "tpto" strict false params [Force];
    Unsharedunwear = "unsharedunwear" strict false params [AddRem];
    Unsharedwear = "unsharedwear" strict false params [AddRem];
    Unsit = "unsit" strict false params [AddRem, Force];
    Version = "version" strict false params [Reply];
    Versionnew = "versionnew" strict false params [Reply];
    Versionnum = "versionnum" strict false params [Reply];
    Viewnote = "viewnote" strict false params [AddRem];
    Viewscript = "viewscript" strict false params [AddRem];
    Viewtexture = "viewtexture" strict false params [AddRem];
    Viewtransparent = "viewtransparent" strict false params [AddRem];
    Viewwireframe = "viewwireframe" strict false params [AddRem];
    Clear = "clear" strict false params [Clear];
}

/// Declarative table of the **local behaviour modifiers** — the named knobs a
/// restriction exposes as `@<behaviour>_<modifier>=force`.
///
/// Each row is `Variant = Behaviour "name"`: the restriction the modifier hangs
/// off, and the suffix that addresses it. The reference registers these on the
/// behaviour's *restriction* entry (`RlvBehaviourInfo::addModifier`,
/// `rlvhelper.cpp:152-171`), which is why the lookup fallback in
/// [`RlvBehaviour::resolve`] only consults [`RlvParamKind::AddRem`] rows.
macro_rules! rlv_local_modifiers {
    ( $( $variant:ident = $base:ident $kw:literal ; )* ) => {
        /// A named modifier of a restriction, addressed by a `=force` command
        /// whose keyword is `<behaviour>_<modifier>` (`ERlvLocalBhvrModifier`).
        ///
        /// `@setsphere_mode=force` is not a behaviour of its own: it sets the
        /// `mode` modifier of the `@setsphere` restriction the same object
        /// already holds. The decoder reports it as the base behaviour plus
        /// this, on [`RlvCommand::modifier`](crate::RlvCommand::modifier).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        }
    };
}

rlv_local_modifiers! {
    OverlayAlpha = Setoverlay "alpha";
    OverlayTexture = Setoverlay "texture";
    OverlayTint = Setoverlay "tint";
    SphereMode = Setsphere "mode";
    SphereOrigin = Setsphere "origin";
    SphereColor = Setsphere "color";
    SphereDistmin = Setsphere "distmin";
    SphereDistmax = Setsphere "distmax";
    SphereDistextend = Setsphere "distextend";
    SphereParams = Setsphere "param";
    SphereTween = Setsphere "tween";
    SphereValuemin = Setsphere "valuemin";
    SphereValuemax = Setsphere "valuemax";
}

/// What a keyword resolved to, once both the keyword and the param kind of the
/// command have been taken into account.
///
/// This is the return of [`RlvBehaviour::resolve`] and the source of the
/// [`behaviour`](crate::RlvCommand::behaviour), [`strict`](crate::RlvCommand::strict)
/// and [`modifier`](crate::RlvCommand::modifier) fields of a decoded command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RlvResolvedBehaviour {
    /// The classified behaviour, or [`RlvBehaviour::Unknown`] if the keyword is
    /// not declared for this param kind. For a local modifier command this is
    /// the **base** restriction, matching `RlvCommand::getBehaviourType`.
    pub behaviour: RlvBehaviour,
    /// Whether the keyword carried the strict `_sec` suffix *and* the behaviour
    /// supports it.
    pub strict: bool,
    /// The local modifier the keyword addressed, if it resolved through the
    /// modifier fallback rather than as a behaviour of its own.
    pub modifier: Option<RlvLocalModifier>,
}

impl RlvBehaviour {
    /// Whether this behaviour is declared for `kind` — the reference's
    /// `(keyword, param type)` dictionary key, asked one axis at a time.
    ///
    /// `@tpto` is a force-only action, so `Tpto.accepts(RlvParamKind::AddRem)`
    /// is `false` and `@tpto=n` is not a restriction but a nonsense command.
    #[must_use]
    pub fn accepts(self, kind: RlvParamKind) -> bool {
        self.param_kinds().contains(&kind)
    }

    /// Resolve a raw keyword against the table for a command of `kind`.
    ///
    /// This is the reference's `getBehaviourInfo(strBhvr, eParamType, ...)`
    /// (`rlvhelper.cpp:434-456`), in three steps:
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
        if let Some(behaviour) = Self::from_keyword(base)
            .filter(|behaviour| behaviour.accepts(kind))
            .filter(|behaviour| !strict || behaviour.has_strict())
        {
            return RlvResolvedBehaviour {
                behaviour,
                strict,
                modifier: None,
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
                && let Some((behaviour, modifier)) = Self::from_keyword(modifier_base)
                    .filter(|behaviour| behaviour.accepts(RlvParamKind::AddRem))
                    .and_then(|behaviour| {
                        RlvLocalModifier::lookup(behaviour, name)
                            .map(|modifier| (behaviour, modifier))
                    })
            {
                return RlvResolvedBehaviour {
                    behaviour,
                    strict: false,
                    modifier: Some(modifier),
                };
            }
        }

        RlvResolvedBehaviour {
            behaviour: Self::Unknown,
            strict: false,
            modifier: None,
        }
    }
}
