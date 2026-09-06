//! What a restriction does with its option, and whether it counts.
//!
//! Every `@behaviour[:option]=n` reaches the reference through a handler that
//! decides two things before any viewer state is touched: what the option
//! *means*, and whether the command reference-counts the behaviour. Those two
//! decisions are the whole of the state machine's contract with the command
//! stream, and they are what this module tabulates.
//!
//! The distinction that matters most is the one between a restriction and an
//! **exception**. `@sendim=n` blocks every IM; `@sendim:<uuid>=add` does not
//! block anything — it lets one avatar through the block someone else put up.
//! The reference expresses this by having the exception branch leave
//! `fRefCount` false (`RlvBehaviourGenericHandler<RLV_OPTION_NONE_OR_EXCEPTION>`,
//! `rlvhandler.cpp:1816`), which is why
//! [`RlvRestrictionRule::refcount_with_option`] exists. Get it wrong and
//! `@sendim:<uuid>=add` reads back as "IMs are blocked".
//!
//! Reference (Firestorm, read-only): `rlvhandler.cpp:1591-2560` (the
//! `RLV_TYPE_ADDREM` handlers) and `rlvdefines.h`
//! (`ERlvBehaviourOptionType`).

use crate::behaviour::RlvBehaviour;
use crate::modifier::RlvModifier;

/// Whether a restriction's option may or must appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvOptionArity {
    /// The command takes no option; one that carries an option is malformed
    /// (`RLV_OPTION_NONE`).
    Forbidden,
    /// The command works with or without an option
    /// (`RLV_OPTION_NONE_OR_EXCEPTION`, `RLV_OPTION_NONE_OR_MODIFIER`).
    Optional,
    /// The command is malformed without an option (`RLV_OPTION_EXCEPTION`,
    /// `RLV_OPTION_MODIFIER`).
    Required,
}

/// What a restriction's option means when it is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvOptionMeaning {
    /// Something this crate does not interpret: a wearable type, an attachment
    /// point, a shared-folder path. The held command keeps the raw text and the
    /// consumer reads it — and may still reject the command on a closer look,
    /// since this layer only validates what it stores.
    Opaque,
    /// A UUID that is let through the restriction.
    Exception,
    /// A chat channel, which is also an exception — `@redirchat:<channel>=n`
    /// names where the redirected chat goes, `@sendchannel:<channel>=n` names
    /// a channel that stays open.
    Channel,
    /// A `@notify` spec: `<channel>[;<filter>]`. The channel is checked here
    /// because it decides whether the command is well formed at all; the filter
    /// is the consumer's, and the channel is not an exception — nothing is let
    /// through, the object is merely told what happens.
    NotifyChannel,
    /// A typed value for a global modifier slot.
    Modifier(RlvModifier),
    /// A UUID exception, or failing that `<min>[;<max>]` metres, which land in
    /// two modifier slots as **squared** distances
    /// (`RlvBehaviourRecvSendStartIMHandler`, `rlvhandler.cpp:2226`). Neither
    /// branch reference-counts.
    ExceptionOrDistance {
        /// The slot the minimum distance goes to.
        min: RlvModifier,
        /// The slot the maximum distance goes to.
        max: RlvModifier,
    },
    /// A UUID exception, or failing that a modifier value
    /// (`@shownametags:<uuid>=n` exempts one avatar, `@shownametags:<dist>=n`
    /// sets the distance). The exception branch does not reference-count; the
    /// modifier branch does.
    ExceptionOrModifier(RlvModifier),
    /// A zoom multiplier that lands in a modifier slot as
    /// `DEFAULT_FIELD_OF_VIEW / multiplier`, defaulting to `1.0` when the
    /// option is absent (`RlvBehaviourCamZoomMinMaxHandler`,
    /// `rlvhandler.cpp:3721`).
    FovMultiplier(RlvModifier),
}

/// How one restriction handles its option and its reference count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvRestrictionRule {
    /// Whether an option may or must appear.
    pub arity: RlvOptionArity,
    /// What the option means when present.
    pub meaning: RlvOptionMeaning,
    /// Whether a bare `@bhvr=n` reference-counts the behaviour.
    ///
    /// Only `@detach` and `@setoverlay_touch` say no: both are held by the
    /// object and asked about per object, never counted globally.
    pub refcount_bare: bool,
    /// Whether `@bhvr:<option>=n` reference-counts the behaviour.
    ///
    /// The exception branch of an
    /// [`RlvOptionMeaning::ExceptionOrDistance`] or
    /// [`RlvOptionMeaning::ExceptionOrModifier`] never does, whatever this
    /// says.
    pub refcount_with_option: bool,
    /// The behaviour whose count this one bumps, when it is not its own.
    ///
    /// Only the deprecated `@camzoommin` / `@camzoommax` do this: they are
    /// counted as `@setcam_fovmin` / `@setcam_fovmax` while keeping their own
    /// identity in the object's command list.
    pub refcount_as: Option<RlvBehaviour>,
    /// The highest count at which a further add is still allowed, if any.
    ///
    /// `@setcam`, `@setdebug` and `@setenv` may be held by one object at a time
    /// so two objects cannot deadlock over the same subsystem
    /// (`rlvhandler.cpp:498`); `@setsphere` tops out at six effects
    /// (`rlvhandler.cpp:2161`).
    ///
    /// The limit is checked before the duplicate check, which is where the
    /// reference puts it for the three exclusive restrictions. `@setsphere`
    /// checks it a step later, so an object re-sending `@setsphere=n` when six
    /// are already up is told "duplicate" there and "locked" here; either way
    /// nothing changes and nothing is added.
    pub holder_limit: Option<u32>,
}

impl RlvRestrictionRule {
    /// The rule for a restriction that takes no option.
    const fn none() -> Self {
        Self {
            arity: RlvOptionArity::Forbidden,
            meaning: RlvOptionMeaning::Opaque,
            refcount_bare: true,
            refcount_with_option: false,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// This rule with a holder limit.
    const fn limit(self, limit: u32) -> Self {
        Self {
            holder_limit: Some(limit),
            ..self
        }
    }

    /// The rule for `@bhvr:<uuid>=n`, which both restricts and names its target
    /// (`RLV_OPTION_EXCEPTION`).
    const fn exception() -> Self {
        Self {
            arity: RlvOptionArity::Required,
            meaning: RlvOptionMeaning::Exception,
            refcount_bare: false,
            refcount_with_option: true,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for `@bhvr[:<uuid>]=n`, where the bare form restricts and the
    /// option form lets one avatar through
    /// (`RLV_OPTION_NONE_OR_EXCEPTION`).
    const fn none_or_exception() -> Self {
        Self {
            arity: RlvOptionArity::Optional,
            meaning: RlvOptionMeaning::Exception,
            refcount_bare: true,
            refcount_with_option: false,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for `@bhvr:<value>=n` (`RLV_OPTION_MODIFIER`).
    const fn modifier(slot: RlvModifier) -> Self {
        Self {
            arity: RlvOptionArity::Required,
            meaning: RlvOptionMeaning::Modifier(slot),
            refcount_bare: false,
            refcount_with_option: true,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for `@bhvr[:<value>]=n` (`RLV_OPTION_NONE_OR_MODIFIER`).
    const fn none_or_modifier(slot: RlvModifier) -> Self {
        Self {
            arity: RlvOptionArity::Optional,
            meaning: RlvOptionMeaning::Modifier(slot),
            refcount_bare: true,
            refcount_with_option: true,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for a restriction whose optional option this crate does not
    /// interpret, and which counts either way.
    const fn opaque_optional() -> Self {
        Self {
            arity: RlvOptionArity::Optional,
            meaning: RlvOptionMeaning::Opaque,
            refcount_bare: true,
            refcount_with_option: true,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for a restriction whose required option this crate does not
    /// interpret.
    const fn opaque_required() -> Self {
        Self {
            arity: RlvOptionArity::Required,
            meaning: RlvOptionMeaning::Opaque,
            refcount_bare: false,
            refcount_with_option: true,
            refcount_as: None,
            holder_limit: None,
        }
    }

    /// The rule for `@addattach` / `@remattach`: the bare form locks every
    /// attachment point and counts, the per-point form locks one and does not
    /// (`rlvhandler.cpp:1923`).
    const fn attach_point() -> Self {
        Self {
            arity: RlvOptionArity::Optional,
            meaning: RlvOptionMeaning::Opaque,
            refcount_bare: true,
            refcount_with_option: false,
            refcount_as: None,
            holder_limit: None,
        }
    }
}

/// The restriction rules, one row per canonical behaviour that can be held.
///
/// A behaviour missing from this table is not a restriction — it is an action
/// or a query — and [`RlvBehaviour::restriction_rule`] answers `None` for it.
macro_rules! rlv_restriction_rules {
    ( $( $bhvr:ident => $rule:expr ; )* ) => {
        impl RlvBehaviour {
            /// How this restriction handles its option and its reference count,
            /// or `None` when the behaviour is not a restriction at all.
            #[must_use]
            pub const fn restriction_rule(self) -> Option<RlvRestrictionRule> {
                match self {
                    $( Self::$bhvr => Some($rule), )*
                    _ => None,
                }
            }
        }
    };
}

rlv_restriction_rules! {
    Acceptpermission => RlvRestrictionRule::none();
    Accepttp => RlvRestrictionRule::none_or_exception();
    Accepttprequest => RlvRestrictionRule::none_or_exception();
    Addattach => RlvRestrictionRule::attach_point();
    Addoutfit => RlvRestrictionRule::opaque_optional();
    Allowidle => RlvRestrictionRule::none();
    Alwaysrun => RlvRestrictionRule::none();
    Attachthis => RlvRestrictionRule::opaque_optional();
    AttachthisExcept => RlvRestrictionRule::opaque_required();
    Buy => RlvRestrictionRule::none();
    Camzoommax => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::FovMultiplier(RlvModifier::SetcamFovmax),
        refcount_bare: true,
        refcount_with_option: true,
        refcount_as: Some(RlvBehaviour::SetcamFovmax),
        holder_limit: None,
    };
    Camzoommin => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::FovMultiplier(RlvModifier::SetcamFovmin),
        refcount_bare: true,
        refcount_with_option: true,
        refcount_as: Some(RlvBehaviour::SetcamFovmin),
        holder_limit: None,
    };
    Chatnormal => RlvRestrictionRule::none();
    Chatshout => RlvRestrictionRule::none();
    Chatwhisper => RlvRestrictionRule::none();
    Detach => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::Opaque,
        refcount_bare: false,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Detachthis => RlvRestrictionRule::opaque_optional();
    DetachthisExcept => RlvRestrictionRule::opaque_required();
    Edit => RlvRestrictionRule::none_or_exception();
    Editattach => RlvRestrictionRule::none();
    Editobj => RlvRestrictionRule::exception();
    Editworld => RlvRestrictionRule::none();
    Emote => RlvRestrictionRule::none();
    Fartouch => RlvRestrictionRule::none_or_modifier(RlvModifier::FartouchDist);
    Fly => RlvRestrictionRule::none();
    Interact => RlvRestrictionRule::none();
    Jump => RlvRestrictionRule::none();
    Notify => RlvRestrictionRule {
        arity: RlvOptionArity::Required,
        meaning: RlvOptionMeaning::NotifyChannel,
        refcount_bare: false,
        refcount_with_option: true,
        refcount_as: None,
        holder_limit: None,
    };
    Pay => RlvRestrictionRule::none();
    Permissive => RlvRestrictionRule::none();
    Recvchat => RlvRestrictionRule::none_or_exception();
    Recvchatfrom => RlvRestrictionRule::exception();
    Recvemote => RlvRestrictionRule::none_or_exception();
    Recvemotefrom => RlvRestrictionRule::exception();
    Recvim => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::ExceptionOrDistance {
            min: RlvModifier::RecvImDistMin,
            max: RlvModifier::RecvImDistMax,
        },
        refcount_bare: true,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Recvimfrom => RlvRestrictionRule::exception();
    Redirchat => RlvRestrictionRule {
        arity: RlvOptionArity::Required,
        meaning: RlvOptionMeaning::Channel,
        refcount_bare: false,
        refcount_with_option: true,
        refcount_as: None,
        holder_limit: None,
    };
    Rediremote => RlvRestrictionRule {
        arity: RlvOptionArity::Required,
        meaning: RlvOptionMeaning::Channel,
        refcount_bare: false,
        refcount_with_option: true,
        refcount_as: None,
        holder_limit: None,
    };
    Remattach => RlvRestrictionRule::attach_point();
    Remoutfit => RlvRestrictionRule::opaque_optional();
    Rez => RlvRestrictionRule::none();
    Sendchannel => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::Channel,
        refcount_bare: true,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    SendchannelExcept => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::Channel,
        refcount_bare: true,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Sendchat => RlvRestrictionRule::none();
    Sendgesture => RlvRestrictionRule::none();
    Sendim => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::ExceptionOrDistance {
            min: RlvModifier::SendImDistMin,
            max: RlvModifier::SendImDistMax,
        },
        refcount_bare: true,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Sendimto => RlvRestrictionRule::exception();
    Setcam => RlvRestrictionRule::none().limit(1);
    SetcamAvdist => RlvRestrictionRule::modifier(RlvModifier::SetcamAvdist);
    SetcamAvdistmax => RlvRestrictionRule::modifier(RlvModifier::SetcamAvdistmax);
    SetcamAvdistmin => RlvRestrictionRule::modifier(RlvModifier::SetcamAvdistmin);
    SetcamEyeoffset => RlvRestrictionRule::modifier(RlvModifier::SetcamEyeoffset);
    SetcamEyeoffsetscale => RlvRestrictionRule::modifier(RlvModifier::SetcamEyeoffsetscale);
    SetcamFocusoffset => RlvRestrictionRule::modifier(RlvModifier::SetcamFocusoffset);
    SetcamFovmax => RlvRestrictionRule::modifier(RlvModifier::SetcamFovmax);
    SetcamFovmin => RlvRestrictionRule::modifier(RlvModifier::SetcamFovmin);
    SetcamMouselook => RlvRestrictionRule::none();
    SetcamOrigindistmax => RlvRestrictionRule::modifier(RlvModifier::SetcamOrigindistmax);
    SetcamOrigindistmin => RlvRestrictionRule::modifier(RlvModifier::SetcamOrigindistmin);
    SetcamTextures => RlvRestrictionRule::none_or_modifier(RlvModifier::SetcamTexture);
    SetcamUnlock => RlvRestrictionRule::none();
    Setdebug => RlvRestrictionRule::none().limit(1);
    Setenv => RlvRestrictionRule::none().limit(1);
    Setgroup => RlvRestrictionRule::none();
    Setoverlay => RlvRestrictionRule::none();
    SetoverlayTouch => RlvRestrictionRule {
        arity: RlvOptionArity::Forbidden,
        meaning: RlvOptionMeaning::Opaque,
        refcount_bare: false,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Setsphere => RlvRestrictionRule::none().limit(6);
    Share => RlvRestrictionRule::none_or_exception();
    Sharedunwear => RlvRestrictionRule::none();
    Sharedwear => RlvRestrictionRule::none();
    Showhovertext => RlvRestrictionRule::exception();
    Showhovertextall => RlvRestrictionRule::none();
    Showhovertexthud => RlvRestrictionRule::none();
    Showhovertextworld => RlvRestrictionRule::none();
    Showinv => RlvRestrictionRule::none();
    Showloc => RlvRestrictionRule::none();
    Showminimap => RlvRestrictionRule::none();
    Shownames => RlvRestrictionRule::none_or_exception();
    Shownametags => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::ExceptionOrModifier(RlvModifier::ShownametagsDist),
        refcount_bare: true,
        refcount_with_option: true,
        refcount_as: None,
        holder_limit: None,
    };
    Shownearby => RlvRestrictionRule::none();
    Showself => RlvRestrictionRule::none();
    Showselfhead => RlvRestrictionRule::none();
    Showworldmap => RlvRestrictionRule::none();
    Sit => RlvRestrictionRule::none();
    Sittp => RlvRestrictionRule::none_or_modifier(RlvModifier::SittpDist);
    Standtp => RlvRestrictionRule::none();
    Startim => RlvRestrictionRule {
        arity: RlvOptionArity::Optional,
        meaning: RlvOptionMeaning::ExceptionOrDistance {
            min: RlvModifier::StartImDistMin,
            max: RlvModifier::StartImDistMax,
        },
        refcount_bare: true,
        refcount_with_option: false,
        refcount_as: None,
        holder_limit: None,
    };
    Startimto => RlvRestrictionRule::exception();
    Temprun => RlvRestrictionRule::none();
    Touchall => RlvRestrictionRule::none();
    Touchattach => RlvRestrictionRule::none_or_exception();
    Touchattachother => RlvRestrictionRule::none_or_exception();
    Touchattachself => RlvRestrictionRule::none();
    Touchhud => RlvRestrictionRule::none_or_exception();
    Touchme => RlvRestrictionRule::none();
    Touchthis => RlvRestrictionRule::exception();
    Touchworld => RlvRestrictionRule::none_or_exception();
    Tplm => RlvRestrictionRule::none();
    Tploc => RlvRestrictionRule::none();
    Tplocal => RlvRestrictionRule::none_or_modifier(RlvModifier::TplocalDist);
    Tplure => RlvRestrictionRule::none_or_exception();
    Tprequest => RlvRestrictionRule::none_or_exception();
    Unsharedunwear => RlvRestrictionRule::none();
    Unsharedwear => RlvRestrictionRule::none();
    Unsit => RlvRestrictionRule::none();
    Viewnote => RlvRestrictionRule::none();
    Viewscript => RlvRestrictionRule::none();
    Viewtexture => RlvRestrictionRule::none();
    Viewtransparent => RlvRestrictionRule::none();
    Viewwireframe => RlvRestrictionRule::none();
}
