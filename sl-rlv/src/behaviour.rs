//! The RLV / RLVa behaviour vocabulary — the `behaviour` keyword of an
//! `@behaviour[:option]=param` command.
//!
//! `RlvBehaviour` is a fieldless classification of the ~175 behaviour keywords
//! the reference viewer knows (`ERlvBehaviour` plus the wire synonyms and
//! deprecated aliases in `RlvBehaviourDictionary`). It is deliberately *only*
//! the classification: [`RlvCommand`](crate::RlvCommand) always keeps the raw
//! keyword text, so an unrecognised or future keyword round-trips as
//! [`RlvBehaviour::Unknown`] without losing its spelling.

/// Declarative table of every known RLV behaviour keyword.
///
/// Each row is `Variant = "keyword" strict <bool>` where the boolean records
/// whether the behaviour accepts the strict `_sec` suffix (`BHVR_STRICT` in the
/// reference dictionary). The macro expands the table into the enum plus the
/// keyword to variant lookups so the keyword list has a single source of truth.
macro_rules! rlv_behaviours {
    ( $( $variant:ident = $kw:literal strict $strict:literal ; )* ) => {
        /// A classified RLV / RLVa behaviour keyword.
        ///
        /// This is the `behaviour` part of an `@behaviour[:option]=param`
        /// command, mapped to a typed variant. Keywords the decoder does not
        /// know map to [`RlvBehaviour::Unknown`]; the raw text is preserved on
        /// [`RlvCommand::keyword`](crate::RlvCommand::keyword).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum RlvBehaviour {
            $(
                #[doc = concat!("The `@", $kw, "` behaviour.")]
                $variant,
            )*
            /// A behaviour keyword this decoder does not recognise (a
            /// newer-than-us or malformed behaviour). The keyword text is kept
            /// on [`RlvCommand::keyword`](crate::RlvCommand::keyword).
            Unknown,
        }

        impl RlvBehaviour {
            /// The classified behaviour for an exact, already lower-cased
            /// keyword, or `None` if the keyword is not one this decoder knows.
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
        }
    };
}

rlv_behaviours! {
    Acceptpermission = "acceptpermission" strict false;
    Accepttp = "accepttp" strict true;
    Accepttprequest = "accepttprequest" strict true;
    Addattach = "addattach" strict false;
    Addoutfit = "addoutfit" strict false;
    Addoutfitall = "addoutfitall" strict false;
    Addoutfitallover = "addoutfitallover" strict false;
    Addoutfitallthis = "addoutfitallthis" strict false;
    Addoutfitallthisover = "addoutfitallthisover" strict false;
    Addoutfitover = "addoutfitover" strict false;
    Addoutfitthis = "addoutfitthis" strict false;
    Addoutfitthisover = "addoutfitthisover" strict false;
    Adjustheight = "adjustheight" strict false;
    Allowidle = "allowidle" strict false;
    Alwaysrun = "alwaysrun" strict false;
    Attach = "attach" strict false;
    Attachall = "attachall" strict false;
    Attachallover = "attachallover" strict false;
    Attachalloverorreplace = "attachalloverorreplace" strict false;
    Attachallthis = "attachallthis" strict false;
    AttachallthisExcept = "attachallthis_except" strict false;
    Attachallthisover = "attachallthisover" strict false;
    Attachallthisoverorreplace = "attachallthisoverorreplace" strict false;
    Attachover = "attachover" strict false;
    Attachoverorreplace = "attachoverorreplace" strict false;
    Attachthis = "attachthis" strict false;
    AttachthisExcept = "attachthis_except" strict false;
    Attachthisover = "attachthisover" strict false;
    Attachthisoverorreplace = "attachthisoverorreplace" strict false;
    Buy = "buy" strict false;
    Camavdist = "camavdist" strict false;
    Camdistmax = "camdistmax" strict false;
    Camdistmin = "camdistmin" strict false;
    Camtextures = "camtextures" strict false;
    Camunlock = "camunlock" strict false;
    Camzoommax = "camzoommax" strict false;
    Camzoommin = "camzoommin" strict false;
    Chatnormal = "chatnormal" strict false;
    Chatshout = "chatshout" strict false;
    Chatwhisper = "chatwhisper" strict false;
    Detach = "detach" strict false;
    Detachall = "detachall" strict false;
    Detachallthis = "detachallthis" strict false;
    DetachallthisExcept = "detachallthis_except" strict false;
    Detachme = "detachme" strict false;
    Detachthis = "detachthis" strict false;
    DetachthisExcept = "detachthis_except" strict false;
    Edit = "edit" strict false;
    Editattach = "editattach" strict false;
    Editobj = "editobj" strict false;
    Editworld = "editworld" strict false;
    Emote = "emote" strict false;
    Fartouch = "fartouch" strict false;
    Findfolder = "findfolder" strict false;
    Findfolders = "findfolders" strict false;
    Fly = "fly" strict false;
    Getaddattachnames = "getaddattachnames" strict false;
    Getaddoutfitnames = "getaddoutfitnames" strict false;
    Getattach = "getattach" strict false;
    Getattachnames = "getattachnames" strict false;
    GetcamAvdist = "getcam_avdist" strict false;
    GetcamAvdistmax = "getcam_avdistmax" strict false;
    GetcamAvdistmin = "getcam_avdistmin" strict false;
    GetcamFov = "getcam_fov" strict false;
    GetcamFovmax = "getcam_fovmax" strict false;
    GetcamFovmin = "getcam_fovmin" strict false;
    GetcamTextures = "getcam_textures" strict false;
    Getcommand = "getcommand" strict false;
    Getgroup = "getgroup" strict false;
    Getheightoffset = "getheightoffset" strict false;
    Getinv = "getinv" strict false;
    Getinvworn = "getinvworn" strict false;
    Getoutfit = "getoutfit" strict false;
    Getoutfitnames = "getoutfitnames" strict false;
    Getpath = "getpath" strict false;
    Getpathnew = "getpathnew" strict false;
    Getremattachnames = "getremattachnames" strict false;
    Getremoutfitnames = "getremoutfitnames" strict false;
    Getsitid = "getsitid" strict false;
    Getstatus = "getstatus" strict false;
    Getstatusall = "getstatusall" strict false;
    Interact = "interact" strict false;
    Jump = "jump" strict false;
    Notify = "notify" strict false;
    Pay = "pay" strict false;
    Permissive = "permissive" strict false;
    Recvchat = "recvchat" strict true;
    Recvchatfrom = "recvchatfrom" strict true;
    Recvemote = "recvemote" strict true;
    Recvemotefrom = "recvemotefrom" strict true;
    Recvim = "recvim" strict true;
    Recvimfrom = "recvimfrom" strict true;
    Redirchat = "redirchat" strict false;
    Rediremote = "rediremote" strict false;
    Remattach = "remattach" strict false;
    Remoutfit = "remoutfit" strict false;
    Rez = "rez" strict false;
    Sendchannel = "sendchannel" strict true;
    SendchannelExcept = "sendchannel_except" strict true;
    Sendchat = "sendchat" strict false;
    Sendgesture = "sendgesture" strict false;
    Sendim = "sendim" strict true;
    Sendimto = "sendimto" strict true;
    Setcam = "setcam" strict false;
    SetcamAvdist = "setcam_avdist" strict false;
    SetcamAvdistmax = "setcam_avdistmax" strict false;
    SetcamAvdistmin = "setcam_avdistmin" strict false;
    SetcamEyeoffset = "setcam_eyeoffset" strict false;
    SetcamEyeoffsetscale = "setcam_eyeoffsetscale" strict false;
    SetcamFocus = "setcam_focus" strict false;
    SetcamFocusoffset = "setcam_focusoffset" strict false;
    SetcamFov = "setcam_fov" strict false;
    SetcamFovmax = "setcam_fovmax" strict false;
    SetcamFovmin = "setcam_fovmin" strict false;
    SetcamMode = "setcam_mode" strict false;
    SetcamMouselook = "setcam_mouselook" strict false;
    SetcamOrigindistmax = "setcam_origindistmax" strict false;
    SetcamOrigindistmin = "setcam_origindistmin" strict false;
    SetcamTextures = "setcam_textures" strict false;
    SetcamUnlock = "setcam_unlock" strict false;
    Setdebug = "setdebug" strict false;
    Setenv = "setenv" strict false;
    Setgroup = "setgroup" strict false;
    Setoverlay = "setoverlay" strict false;
    SetoverlayTouch = "setoverlay_touch" strict false;
    SetoverlayTween = "setoverlay_tween" strict false;
    Setsphere = "setsphere" strict false;
    Share = "share" strict true;
    Sharedunwear = "sharedunwear" strict false;
    Sharedwear = "sharedwear" strict false;
    Showhovertext = "showhovertext" strict false;
    Showhovertextall = "showhovertextall" strict false;
    Showhovertexthud = "showhovertexthud" strict false;
    Showhovertextworld = "showhovertextworld" strict false;
    Showinv = "showinv" strict false;
    Showloc = "showloc" strict false;
    Showminimap = "showminimap" strict false;
    Shownames = "shownames" strict true;
    Shownametags = "shownametags" strict true;
    Shownearby = "shownearby" strict false;
    Showself = "showself" strict false;
    Showselfhead = "showselfhead" strict false;
    Showworldmap = "showworldmap" strict false;
    Sit = "sit" strict false;
    Sitground = "sitground" strict false;
    Sittp = "sittp" strict false;
    Standtp = "standtp" strict false;
    Startim = "startim" strict true;
    Startimto = "startimto" strict true;
    Temprun = "temprun" strict false;
    Touchall = "touchall" strict false;
    Touchattach = "touchattach" strict false;
    Touchattachother = "touchattachother" strict false;
    Touchattachself = "touchattachself" strict false;
    Touchfar = "touchfar" strict false;
    Touchhud = "touchhud" strict false;
    Touchme = "touchme" strict false;
    Touchthis = "touchthis" strict false;
    Touchworld = "touchworld" strict false;
    Tplm = "tplm" strict false;
    Tploc = "tploc" strict false;
    Tplocal = "tplocal" strict false;
    Tplure = "tplure" strict true;
    Tprequest = "tprequest" strict true;
    Tpto = "tpto" strict false;
    Unsharedunwear = "unsharedunwear" strict false;
    Unsharedwear = "unsharedwear" strict false;
    Unsit = "unsit" strict false;
    Version = "version" strict false;
    Versionnew = "versionnew" strict false;
    Versionnum = "versionnum" strict false;
    Viewnote = "viewnote" strict false;
    Viewscript = "viewscript" strict false;
    Viewtexture = "viewtexture" strict false;
    Viewtransparent = "viewtransparent" strict false;
    Viewwireframe = "viewwireframe" strict false;
    Clear = "clear" strict false;
}
