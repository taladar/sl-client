//! The restriction state machine — what the viewer actually asks.
//!
//! Everything upstream of here is classification: a chat line becomes a typed
//! [`RlvCommand`]. This is where a command becomes *state*, and where every
//! enforcement family gets its one answer to "is behaviour X in force?" instead
//! of re-deriving it at each choke point.
//!
//! The model is the reference's `RlvHandler`, minus the viewer
//! (`rlvhandler.cpp`):
//!
//! - restrictions are held **per issuing object** and **reference-counted**
//!   across objects. A behaviour stays in force while any object holds it, so
//!   one collar lifting `@fly=y` does not hand back flight while another collar
//!   still says no;
//! - the bookkeeping is bidirectional — object to behaviours
//!   ([`RlvState::restrictions_of`]) and behaviour to objects
//!   ([`RlvState::objects_holding`]) — because both questions get asked;
//! - **exceptions** ride alongside: `@sendim:<uuid>=add` does not restrict
//!   anything, it lets one avatar through a block. Whether an exception issued
//!   by *another* object counts is the `@permissive` / `_sec` question, which
//!   [`RlvState::is_exception`] answers;
//! - an object's restrictions all drop when it detaches
//!   ([`RlvState::clear_object`]). That transition is what makes the whole
//!   enforcement layer correct — a collar you took off must stop restricting
//!   you — so it is modelled here rather than in each consumer.
//!
//! Nothing here does I/O, and nothing here obeys anything: the answers are data
//! the Bevy viewer, a headless bot, or a test can act on.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::behaviour::{RlvBehaviour, RlvEntry, RlvLocalModifier};
use crate::command::{RlvCommand, RlvParam, RlvParamKind};
use crate::modifier::{DEFAULT_FIELD_OF_VIEW, RlvModifier, RlvModifierState, RlvModifierValue};
use crate::notify::{RlvNotification, RlvNotifyRegistry};
use crate::restriction::{RlvOptionArity, RlvOptionMeaning, RlvRestrictionRule};

/// The camera modifier slots that `@setcam` takes exclusive control of.
///
/// When one object holds `@setcam` it becomes the primary object of each of
/// these, so its values win outright rather than competing with everyone
/// else's (`RlvBehaviourToggleHandler<RLV_BHVR_SETCAM>::onCommandToggle`,
/// `rlvhandler.cpp:2441`).
const SETCAM_EXCLUSIVE_MODIFIERS: &[RlvModifier] = &[
    RlvModifier::SetcamAvdist,
    RlvModifier::SetcamAvdistmin,
    RlvModifier::SetcamAvdistmax,
    RlvModifier::SetcamOrigindistmin,
    RlvModifier::SetcamOrigindistmax,
    RlvModifier::SetcamEyeoffset,
    RlvModifier::SetcamEyeoffsetscale,
    RlvModifier::SetcamFocusoffset,
    RlvModifier::SetcamFovmin,
    RlvModifier::SetcamFovmax,
    RlvModifier::SetcamTexture,
];

/// What applying a command did (`ERlvCmdRet`, `rlvdefines.h:328`).
///
/// The reference reports this back to the issuing object through `@notify`, and
/// its debug console shows it; the same distinctions are what a consumer needs
/// to know whether anything changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvOutcome {
    /// The command was applied.
    Success,
    /// An add for something this object already holds
    /// (`RLV_RET_SUCCESS_DUPLICATE`). Nothing changed, which is the point: two
    /// `@detach=n` from one object are one restriction, so one `@detach=y`
    /// lifts it.
    SuccessDuplicate,
    /// A remove for something this object never held
    /// (`RLV_RET_SUCCESS_UNSET`). Not an error — objects lift restrictions
    /// defensively all the time.
    SuccessUnset,
    /// Applied, but the spelling the object used is deprecated
    /// (`RLV_RET_SUCCESS_DEPRECATED`) — `@camzoommin` for `@setcam_fovmin`,
    /// `@camtextures` for `@setcam_textures`. Worth surfacing to the user,
    /// because the object will stop working against a viewer that finally
    /// drops the shim.
    SuccessDeprecated,
    /// The behaviour keyword is not one this decoder knows, or is not a
    /// restriction (`RLV_RET_FAILED_PARAM`).
    FailedParam,
    /// The option was missing, present where it may not be, or unparsable
    /// (`RLV_RET_FAILED_OPTION`).
    FailedOption,
    /// The behaviour is already held by as many objects as it allows
    /// (`RLV_RET_FAILED_LOCK`) — `@setcam`, `@setdebug` and `@setenv` allow
    /// one, `@setsphere` allows six.
    FailedLock,
    /// A local modifier was addressed on an object that does not hold the
    /// restriction it belongs to (`RLV_RET_FAILED_UNHELDBEHAVIOUR`).
    FailedUnheldBehaviour,
    /// A command this state machine does not own: an action to perform
    /// (`=force`) or a query to answer (`=<channel>`). The consumer dispatches
    /// it.
    NotAStateChange,
}

impl RlvOutcome {
    /// Whether the command was applied — the reference's `RLV_RET_SUCCEEDED`.
    #[must_use]
    pub const fn succeeded(self) -> bool {
        matches!(
            self,
            Self::Success | Self::SuccessDuplicate | Self::SuccessUnset | Self::SuccessDeprecated
        )
    }
}

/// What an exception is an exception *for*.
///
/// The reference stores this as a `boost::variant`
/// (`RlvExceptionOption`); the three arms are the three things an option can
/// name once parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvExceptionOption {
    /// An avatar or object let through the restriction.
    Avatar(Uuid),
    /// A chat channel — where `@redirchat` sends chat, or which channel
    /// `@sendchannel` leaves open.
    Channel(i32),
    /// A behaviour, which is how a `_sec` restriction records against
    /// [`RlvBehaviour::Permissive`] that *this* behaviour is now strict.
    Behaviour(RlvBehaviour),
}

/// One exception: an object said this option is allowed through this
/// behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvException {
    /// The object that added the exception.
    pub object: Uuid,
    /// The behaviour it applies to.
    pub behaviour: RlvBehaviour,
    /// What is let through.
    pub option: RlvExceptionOption,
}

/// How strictly an exception is checked (`ERlvExceptionCheck`,
/// `rlvdefines.h:363`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RlvExceptionCheck {
    /// Decide from the restrictions actually in force: strict when the
    /// behaviour is held and not permissive, permissive otherwise. This is what
    /// a consumer wants.
    #[default]
    Automatic,
    /// One object's exception is enough.
    Permissive,
    /// *Every* object holding the restriction must also have granted the
    /// exception — the point of `_sec`, so a second collar cannot poke a hole
    /// in the first one's block.
    Strict,
}

/// One restriction an object is holding.
///
/// The raw [`keyword`](RlvHeldCommand::keyword) is kept because that is what
/// identity means here: the reference compares held commands by keyword text,
/// so an object that sent both `@fartouch=n` and its synonym `@touchfar=n`
/// holds two, and has to lift both.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvHeldCommand {
    /// The keyword as it arrived, with any `_sec` suffix still attached.
    pub keyword: String,
    /// The canonical behaviour it names.
    pub behaviour: RlvBehaviour,
    /// The option it carried, if any.
    pub option: Option<String>,
    /// Whether it was issued strict (`_sec`).
    pub strict: bool,
    /// Whether it bumped a reference count. A command that only granted an
    /// exception did not, and so does not answer "is this behaviour in force".
    pub ref_counted: bool,
}

impl RlvHeldCommand {
    /// The `keyword[:option]` text `@getstatus` reports and `@clear`'s filter
    /// matches against (`RlvCommand::asString`, `rlvhelper.h:728`).
    #[must_use]
    pub fn as_string(&self) -> String {
        match self.option {
            Some(ref option) => format!("{}:{option}", self.keyword),
            None => self.keyword.clone(),
        }
    }

    /// The option text, treating an absent option as empty — which is how the
    /// reference compares them.
    fn option_text(&self) -> &str {
        self.option.as_deref().unwrap_or("")
    }
}

/// Everything one object is holding.
#[derive(Debug, Clone, Default)]
struct RlvObject {
    /// The restrictions it holds, in the order they arrived.
    commands: Vec<RlvHeldCommand>,
    /// The per-object values it set on its own effect
    /// (`@setsphere_mode:1=force`).
    modifiers: BTreeMap<RlvLocalModifier, RlvModifierValue>,
}

impl RlvObject {
    /// Whether this object holds `behaviour` with no option
    /// (`RlvObject::hasBehaviour(eBhvr, fStrictOnly)`, `rlvhelper.cpp:1181`).
    fn holds_bare(&self, behaviour: RlvBehaviour, strict_only: bool) -> bool {
        self.commands.iter().any(|command| {
            command.behaviour == behaviour
                && command.option.is_none()
                && (!strict_only || command.strict)
        })
    }

    /// Whether this object holds `behaviour` with exactly `option`
    /// (`RlvObject::hasBehaviour(eBhvr, strOption, fStrictOnly)`,
    /// `rlvhelper.cpp:1189`).
    ///
    /// An empty `option` also matches any reference-counted command, whatever
    /// its own option: that command is what put the behaviour in force, so it
    /// answers the unqualified question.
    fn holds(&self, behaviour: RlvBehaviour, option: &str, strict_only: bool) -> bool {
        self.commands.iter().any(|command| {
            command.behaviour == behaviour
                && (command.option_text() == option || (option.is_empty() && command.ref_counted))
                && (!strict_only || command.strict)
        })
    }

    /// The index of the command matching `keyword`, `option` and `strict`, if
    /// this object holds it.
    fn position_of(&self, keyword: &str, option: Option<&str>, strict: bool) -> Option<usize> {
        self.commands.iter().position(|command| {
            command.keyword == keyword
                && command.option.as_deref() == option
                && command.strict == strict
        })
    }
}

/// The restriction state machine.
///
/// Feed it decoded commands with [`RlvState::apply`], tell it when an object
/// goes away with [`RlvState::clear_object`], and ask it questions. It is
/// deliberately inert: it never performs an action and never answers a query,
/// it only knows what is in force.
#[derive(Debug, Clone)]
pub struct RlvState {
    /// The objects holding restrictions, ordered by id so that "the first
    /// object holding X" is a stable answer.
    objects: BTreeMap<Uuid, RlvObject>,
    /// How many held commands reference-count each behaviour.
    counts: BTreeMap<RlvBehaviour, u32>,
    /// The exceptions in force, in the order they were granted.
    exceptions: Vec<RlvException>,
    /// The global modifier slots.
    modifiers: RlvModifierState,
    /// Whether the RLVa experimental command set is enabled.
    experimental: bool,
    /// Who asked to be told about every change (`@notify`).
    notify: RlvNotifyRegistry,
    /// The notifications produced but not yet taken by the consumer.
    pending: Vec<RlvNotification>,
}

impl Default for RlvState {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            counts: BTreeMap::new(),
            exceptions: Vec::new(),
            modifiers: RlvModifierState::new(),
            // The reference ships `RLVaExperimentalCommands` on.
            experimental: true,
            notify: RlvNotifyRegistry::default(),
            pending: Vec::new(),
        }
    }
}

impl RlvState {
    /// A state machine with nothing in force.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the RLVa experimental command set is enabled.
    #[must_use]
    pub const fn experimental_commands(&self) -> bool {
        self.experimental
    }

    /// Enable or disable the RLVa experimental command set
    /// (`RLVaExperimentalCommands`).
    ///
    /// The reference implements this by not registering the experimental rows
    /// at all (`rlvhelper.cpp:376`), so with the set off those keywords are not
    /// commands: [`RlvState::apply`] answers [`RlvOutcome::FailedParam`] and
    /// [`RlvState::known_commands`] does not list them. Turning it off while
    /// experimental restrictions are held does *not* lift them, exactly as
    /// flipping the reference's setting mid-session does not.
    pub const fn set_experimental_commands(&mut self, enabled: bool) {
        self.experimental = enabled;
    }

    /// The `@getcommand` answer: every keyword containing `filter`, of `kind`
    /// (or of any kind for `None`), that this state machine would accept.
    #[must_use]
    pub fn known_commands(&self, filter: &str, kind: Option<RlvParamKind>) -> Vec<String> {
        RlvEntry::commands_matching(filter, kind, self.experimental)
    }

    // ----------------------------------------------------------------- apply

    /// Apply one decoded command issued by `object`.
    ///
    /// Restrictions (`=n` / `=y`), `@clear`, and the local-modifier `=force`
    /// commands change state here. Every other `=force` action and every
    /// `=<channel>` query is [`RlvOutcome::NotAStateChange`] — real work for
    /// the consumer, which this crate deliberately does not do.
    ///
    /// An `=n` / `=y` also feeds `@notify`
    /// ([`RlvState::take_notifications`]) — whatever it answers, because the
    /// reference reports a command that failed just as it reports one that
    /// took (`rlvhandler.cpp:585`).
    pub fn apply(&mut self, object: Uuid, command: &RlvCommand) -> RlvOutcome {
        let outcome = self.apply_silently(object, command);
        // `@clear` announces itself from inside `clear`, which is also where a
        // detach reaches, so only the add/remove pair is announced from here.
        if matches!(command.param, RlvParam::Add | RlvParam::Remove) {
            self.announce_command(command);
        }
        outcome
    }

    /// [`RlvState::apply`] without the `@notify` broadcast.
    fn apply_silently(&mut self, object: Uuid, command: &RlvCommand) -> RlvOutcome {
        if !self.experimental
            && command
                .entry
                .is_some_and(|entry| entry.flags.is_experimental())
        {
            return RlvOutcome::FailedParam;
        }
        let outcome = self.apply_known(object, command);
        // A deprecated spelling still works; the object is just told so.
        if outcome == RlvOutcome::Success
            && command
                .entry
                .is_some_and(|entry| entry.flags.is_deprecated())
        {
            return RlvOutcome::SuccessDeprecated;
        }
        outcome
    }

    /// [`RlvState::apply`] once the command's dialect has been vetted.
    fn apply_known(&mut self, object: Uuid, command: &RlvCommand) -> RlvOutcome {
        match command.param {
            RlvParam::Add => self.add(object, command),
            RlvParam::Remove => self.remove(object, command),
            RlvParam::Clear { ref filter } => {
                self.clear(object, filter.as_deref());
                RlvOutcome::Success
            }
            RlvParam::Force => match command.modifier {
                Some(modifier) => self.set_local_modifier(object, modifier, command),
                None => RlvOutcome::NotAStateChange,
            },
            RlvParam::Reply { .. } => RlvOutcome::NotAStateChange,
        }
    }

    /// Add a restriction (`RLV_TYPE_ADD`, `rlvhandler.cpp:488`).
    fn add(&mut self, object: Uuid, command: &RlvCommand) -> RlvOutcome {
        let Some(rule) = command.behaviour.restriction_rule() else {
            return RlvOutcome::FailedParam;
        };
        let counted = rule.refcount_as.unwrap_or(command.behaviour);
        if rule
            .holder_limit
            .is_some_and(|limit| self.count(counted) >= limit)
        {
            return RlvOutcome::FailedLock;
        }

        let held = RlvHeldCommand {
            keyword: command.keyword.clone(),
            behaviour: command.behaviour,
            option: command.option.clone(),
            strict: command.strict,
            ref_counted: false,
        };
        let entry = self.objects.entry(object).or_default();
        if entry
            .position_of(&held.keyword, held.option.as_deref(), held.strict)
            .is_some()
        {
            return RlvOutcome::SuccessDuplicate;
        }
        entry.commands.push(held);

        let outcome = self.apply_rule(object, command, rule, true);
        if !outcome.succeeded() {
            // The reference unwinds a failed add so a rejected command leaves
            // no trace (`rlvhandler.cpp:525`).
            self.forget(
                object,
                &command.keyword,
                command.option.as_deref(),
                command.strict,
            );
            return outcome;
        }
        outcome
    }

    /// Remove a restriction (`RLV_TYPE_REMOVE`, `rlvhandler.cpp:543`).
    fn remove(&mut self, object: Uuid, command: &RlvCommand) -> RlvOutcome {
        let Some(rule) = command.behaviour.restriction_rule() else {
            return RlvOutcome::SuccessUnset;
        };
        let Some(entry) = self.objects.get(&object) else {
            return RlvOutcome::SuccessUnset;
        };
        let Some(index) =
            entry.position_of(&command.keyword, command.option.as_deref(), command.strict)
        else {
            return RlvOutcome::SuccessUnset;
        };
        // Whether to give the count back is read off the held command, not
        // re-derived from the rule: the add already decided, and reading its
        // answer is the one way the two can never disagree.
        let ref_counted = entry
            .commands
            .get(index)
            .is_some_and(|held| held.ref_counted);

        if let Some(entry) = self.objects.get_mut(&object) {
            entry.commands.remove(index);
        }

        let outcome = self.apply_rule(object, command, rule, false);
        if ref_counted {
            self.unreference(object, command, rule);
        }
        self.drop_if_empty(object);
        outcome
    }

    /// `@clear[=<filter>]` — lift every restriction this object holds whose
    /// `keyword[:option]` text contains `filter` (`rlvhandler.cpp:623`).
    ///
    /// A bare `@clear` lifts all of them. Clearing an object that holds nothing
    /// is not an error; the reference is explicit that failing it only confuses
    /// people.
    ///
    /// `@notify` hears the `@clear` itself and not the restrictions it lifted:
    /// the reference lifts them by feeding itself internal commands, which its
    /// notify hook skips (`rlvhandler.cpp:640`).
    pub fn clear(&mut self, object: Uuid, filter: Option<&str>) {
        if let Some(entry) = self.objects.get(&object) {
            let doomed: Vec<RlvHeldCommand> = entry
                .commands
                .iter()
                .filter(|held| filter.is_none_or(|filter| held.as_string().contains(filter)))
                .cloned()
                .collect();
            for held in doomed {
                let resolved = RlvBehaviour::resolve(&held.keyword, RlvParamKind::AddRem);
                let command = RlvCommand {
                    keyword: held.keyword.clone(),
                    behaviour: held.behaviour,
                    strict: held.strict,
                    modifier: None,
                    option: held.option.clone(),
                    param: RlvParam::Remove,
                    param_text: "y".to_owned(),
                    entry: resolved.entry,
                };
                self.remove(object, &command);
            }
        }
        // Announced even when the object held nothing, and after the lifting so
        // that a `@notify` this very `@clear` just dropped no longer hears it.
        match filter {
            Some(filter) => self.announce(&format!("clear:{filter}"), ""),
            None => self.announce("clear", ""),
        }
    }

    /// Drop everything `object` holds, because it detached or was garbage
    /// collected.
    ///
    /// This is the transition the whole enforcement layer rests on: the
    /// reference reaches it by feeding the object a synthetic `@clear`
    /// (`RlvHandler::onDetach`, `rlvhandler.cpp:1042`), and so does this — the
    /// reference counts come down exactly as if the object had lifted each
    /// restriction itself.
    pub fn clear_object(&mut self, object: Uuid) {
        self.clear(object, None);
        self.objects.remove(&object);
        self.modifiers.clear_object(object);
        self.exceptions.retain(|entry| entry.object != object);
    }

    // ---------------------------------------------------------------- notify

    /// Take the notifications produced since the last call, in the order they
    /// were produced.
    ///
    /// Every [`RlvState::apply`], [`RlvState::clear`] and
    /// [`RlvState::clear_object`] can leave lines here for the objects that
    /// asked for them with `@notify`, and a consumer that speaks `@notify` at
    /// all has to drain them after each: an untaken queue only grows, and the
    /// lines in it describe a state that has since moved on. A consumer that
    /// does not speak `@notify` pays nothing for it — with no subscription in
    /// force nothing is ever queued.
    #[must_use]
    pub fn take_notifications(&mut self) -> Vec<RlvNotification> {
        core::mem::take(&mut self.pending)
    }

    /// Report an added or lifted restriction to the `@notify` subscribers.
    ///
    /// Nobody listening is the common case and this sits on the path of every
    /// command, so the line is not built until there is somewhere to send it.
    fn announce_command(&mut self, command: &RlvCommand) {
        if self.notify.is_empty() {
            return;
        }
        self.announce(&command_text(command), &format!("={}", command.param_text));
    }

    /// Queue what one change says to the objects listening for it
    /// (`RlvBehaviourNotifyHandler::sendNotification`, `rlvhelper.cpp:1904`).
    ///
    /// `text` is the `behaviour[:option]` half the filters are matched
    /// against; `suffix` is the `=<param>` glued to it, which they are not.
    fn announce(&mut self, text: &str, suffix: &str) {
        if self.notify.is_empty() {
            return;
        }
        self.pending.extend(self.notify.notifications(text, suffix));
    }

    /// Run a rule's option handling for one add or remove, and reference-count
    /// the result.
    ///
    /// This is the shared body of the reference's `RLV_TYPE_ADDREM` handlers
    /// plus `RlvCommandHandlerBaseImpl<RLV_TYPE_ADDREM>::processCommand`
    /// (`rlvhandler.cpp:1744`), which is where the reference does its
    /// reference counting and its `@permissive` bookkeeping.
    fn apply_rule(
        &mut self,
        object: Uuid,
        command: &RlvCommand,
        rule: RlvRestrictionRule,
        adding: bool,
    ) -> RlvOutcome {
        let option = command.option.as_deref();
        match (option, rule.arity) {
            (Some(_), RlvOptionArity::Forbidden) | (None, RlvOptionArity::Required) => {
                return RlvOutcome::FailedOption;
            }
            _ => {}
        }

        let counts = match option {
            None => self.apply_bare(object, command, rule, adding),
            Some(option) => self.apply_option(object, command, rule, option, adding),
        };
        let Some(counts) = counts else {
            return RlvOutcome::FailedOption;
        };

        if adding && counts {
            self.reference(object, command, rule);
        }
        RlvOutcome::Success
    }

    /// Handle a bare `@bhvr=n|y`, returning whether it reference-counts, or
    /// `None` when the option was refused.
    fn apply_bare(
        &mut self,
        object: Uuid,
        command: &RlvCommand,
        rule: RlvRestrictionRule,
        adding: bool,
    ) -> Option<bool> {
        // A bare command on a modifier-bearing behaviour contributes the slot's
        // default, if the slot asks for one
        // (`RlvBehaviourGenericHandler<RLV_OPTION_NONE_OR_MODIFIER>`,
        // `rlvhandler.cpp:1858`).
        let default_slot = match rule.meaning {
            RlvOptionMeaning::Modifier(slot) | RlvOptionMeaning::ExceptionOrModifier(slot) => {
                Some(slot)
            }
            RlvOptionMeaning::FovMultiplier(slot) => {
                // No option means a multiplier of 1.
                self.write_modifier(
                    slot,
                    RlvModifierValue::Float(DEFAULT_FIELD_OF_VIEW),
                    object,
                    command.behaviour,
                    adding,
                );
                None
            }
            _ => None,
        };
        if let Some(slot) = default_slot.filter(|slot| slot.add_default_on_empty()) {
            self.write_modifier(
                slot,
                slot.default_value(),
                object,
                command.behaviour,
                adding,
            );
        }
        Some(rule.refcount_bare)
    }

    /// Handle `@bhvr:<option>=n|y`, returning whether it reference-counts, or
    /// `None` when the option was refused.
    fn apply_option(
        &mut self,
        object: Uuid,
        command: &RlvCommand,
        rule: RlvRestrictionRule,
        option: &str,
        adding: bool,
    ) -> Option<bool> {
        match rule.meaning {
            RlvOptionMeaning::Opaque => Some(rule.refcount_with_option),
            RlvOptionMeaning::NotifyChannel => {
                let (channel, filter) = parse_notify_option(option)?;
                if adding {
                    self.notify.add(object, channel, filter);
                } else {
                    self.notify.remove(object, channel, filter);
                }
                Some(rule.refcount_with_option)
            }
            RlvOptionMeaning::Exception => {
                let avatar = parse_uuid(option)?;
                self.write_exception(
                    object,
                    command.behaviour,
                    RlvExceptionOption::Avatar(avatar),
                    adding,
                );
                Some(rule.refcount_with_option)
            }
            RlvOptionMeaning::Channel => {
                let channel = parse_channel(option, command.behaviour)?;
                self.write_exception(
                    object,
                    command.behaviour,
                    RlvExceptionOption::Channel(channel),
                    adding,
                );
                Some(rule.refcount_with_option)
            }
            RlvOptionMeaning::Modifier(slot) => {
                let value = RlvModifierValue::parse(option, slot.value_type())?;
                self.write_modifier(slot, value, object, command.behaviour, adding);
                Some(rule.refcount_with_option)
            }
            RlvOptionMeaning::ExceptionOrModifier(slot) => match parse_uuid(option) {
                Some(avatar) => {
                    self.write_exception(
                        object,
                        command.behaviour,
                        RlvExceptionOption::Avatar(avatar),
                        adding,
                    );
                    Some(false)
                }
                None => {
                    let value = RlvModifierValue::parse(option, slot.value_type())?;
                    self.write_modifier(slot, value, object, command.behaviour, adding);
                    Some(rule.refcount_with_option)
                }
            },
            RlvOptionMeaning::ExceptionOrDistance { min, max } => {
                match parse_uuid(option) {
                    Some(avatar) => {
                        self.write_exception(
                            object,
                            command.behaviour,
                            RlvExceptionOption::Avatar(avatar),
                            adding,
                        );
                    }
                    None => {
                        // `<min>[;<max>]` in metres, stored squared so a
                        // consumer can compare against a squared distance and
                        // skip the square root.
                        //
                        // Both halves are parsed before either is written: a
                        // range whose second half is malformed must leave no
                        // trace, and writing as we went would strand the first
                        // value in the slot with no held command to take it
                        // away again.
                        let (min_text, max_text) = match option.split_once(';') {
                            Some((min_text, max_text)) => (min_text, Some(max_text)),
                            None => (option, None),
                        };
                        let min_value = parse_distance(min_text)?;
                        let max_value = match max_text {
                            Some(max_text) => Some(parse_distance(max_text)?),
                            None => None,
                        };
                        self.write_modifier(
                            min,
                            RlvModifierValue::Float(min_value * min_value),
                            object,
                            command.behaviour,
                            adding,
                        );
                        if let Some(max_value) = max_value {
                            self.write_modifier(
                                max,
                                RlvModifierValue::Float(max_value * max_value),
                                object,
                                command.behaviour,
                                adding,
                            );
                        }
                    }
                }
                Some(false)
            }
            RlvOptionMeaning::FovMultiplier(slot) => {
                let multiplier = option.parse::<f32>().ok()?;
                self.write_modifier(
                    slot,
                    RlvModifierValue::Float(DEFAULT_FIELD_OF_VIEW / multiplier),
                    object,
                    command.behaviour,
                    adding,
                );
                Some(rule.refcount_with_option)
            }
        }
    }

    /// Add or take away one exception.
    fn write_exception(
        &mut self,
        object: Uuid,
        behaviour: RlvBehaviour,
        option: RlvExceptionOption,
        adding: bool,
    ) {
        if adding {
            self.exceptions.push(RlvException {
                object,
                behaviour,
                option,
            });
        } else if let Some(index) = self.exceptions.iter().position(|entry| {
            entry.behaviour == behaviour && entry.object == object && entry.option == option
        }) {
            self.exceptions.remove(index);
        }
    }

    /// Contribute or take back one modifier value.
    fn write_modifier(
        &mut self,
        slot: RlvModifier,
        value: RlvModifierValue,
        object: Uuid,
        behaviour: RlvBehaviour,
        adding: bool,
    ) {
        if adding {
            self.modifiers
                .add_value(slot, value, object, Some(behaviour));
        } else {
            self.modifiers
                .remove_value(slot, value, object, Some(behaviour));
        }
    }

    /// Mark the just-added command as counted and bump its behaviour.
    fn reference(&mut self, object: Uuid, command: &RlvCommand, rule: RlvRestrictionRule) {
        if command.strict {
            self.exceptions.push(RlvException {
                object,
                behaviour: RlvBehaviour::Permissive,
                option: RlvExceptionOption::Behaviour(command.behaviour),
            });
        }
        if let Some(entry) = self.objects.get_mut(&object)
            && let Some(index) =
                entry.position_of(&command.keyword, command.option.as_deref(), command.strict)
            && let Some(held) = entry.commands.get_mut(index)
        {
            held.ref_counted = true;
        }
        let counted = rule.refcount_as.unwrap_or(command.behaviour);
        let count = self.counts.entry(counted).or_insert(0);
        *count = count.saturating_add(1);
        self.on_count_changed(counted);
    }

    /// Undo what [`RlvState::reference`] did.
    fn unreference(&mut self, object: Uuid, command: &RlvCommand, rule: RlvRestrictionRule) {
        if command.strict {
            self.write_exception(
                object,
                RlvBehaviour::Permissive,
                RlvExceptionOption::Behaviour(command.behaviour),
                false,
            );
        }
        // Lifting a restriction drops the local modifiers that hung off it —
        // the effect it configured is gone, so its knobs are meaningless
        // (`rlvhandler.cpp:1768`).
        if let Some(entry) = self.objects.get_mut(&object) {
            entry
                .modifiers
                .retain(|modifier, _| modifier.behaviour() != command.behaviour);
        }
        let counted = rule.refcount_as.unwrap_or(command.behaviour);
        if let Some(count) = self.counts.get_mut(&counted) {
            *count = count.saturating_sub(1);
        }
        self.on_count_changed(counted);
    }

    /// React to a behaviour's count crossing zero.
    ///
    /// Only `@setcam` needs this: taking exclusive control of the camera
    /// rewrites `@setcam_unlock`'s count and makes the holder the primary
    /// object of every camera modifier slot, so that a second object's camera
    /// values stop competing (`rlvhandler.cpp:2441`).
    fn on_count_changed(&mut self, behaviour: RlvBehaviour) {
        if behaviour != RlvBehaviour::Setcam {
            return;
        }
        let holder = self
            .objects
            .iter()
            .find(|&(_, object)| object.holds_bare(RlvBehaviour::Setcam, false))
            .map(|(&id, _)| id);
        let unlock = match holder {
            Some(holder) => u32::from(
                self.objects
                    .get(&holder)
                    .is_some_and(|object| object.holds_bare(RlvBehaviour::SetcamUnlock, false)),
            ),
            None => {
                let holders = self
                    .objects
                    .values()
                    .filter(|object| object.holds_bare(RlvBehaviour::SetcamUnlock, false))
                    .count();
                u32::try_from(holders).unwrap_or(u32::MAX)
            }
        };
        self.counts.insert(RlvBehaviour::SetcamUnlock, unlock);
        for &slot in SETCAM_EXCLUSIVE_MODIFIERS {
            self.modifiers.set_primary_object(slot, holder);
        }
    }

    /// Take a held command back off an object without any of the bookkeeping —
    /// the unwind path for an add that failed after it was recorded.
    fn forget(&mut self, object: Uuid, keyword: &str, option: Option<&str>, strict: bool) {
        if let Some(entry) = self.objects.get_mut(&object)
            && let Some(index) = entry.position_of(keyword, option, strict)
        {
            entry.commands.remove(index);
        }
        self.drop_if_empty(object);
    }

    /// Forget an object that holds nothing any more, and with it the standalone
    /// modifier values it wrote (`rlvhandler.cpp:556`).
    fn drop_if_empty(&mut self, object: Uuid) {
        if self
            .objects
            .get(&object)
            .is_some_and(|entry| entry.commands.is_empty())
        {
            self.objects.remove(&object);
            self.modifiers.clear_object(object);
        }
    }

    /// `@<behaviour>_<modifier>[:<value>]=force` — set or clear one of this
    /// object's own effect knobs (`RlvBehaviourInfo::processModifier`,
    /// `rlvhelper.cpp:519`).
    ///
    /// The object has to hold the restriction the modifier belongs to: there is
    /// no `@setsphere` effect to configure until it has said `@setsphere=n`.
    fn set_local_modifier(
        &mut self,
        object: Uuid,
        modifier: RlvLocalModifier,
        command: &RlvCommand,
    ) -> RlvOutcome {
        if !self
            .objects
            .get(&object)
            .is_some_and(|entry| entry.holds(modifier.behaviour(), "", false))
        {
            return RlvOutcome::FailedUnheldBehaviour;
        }
        let value = match command.option {
            Some(ref option) => match RlvModifierValue::parse(option, modifier.value_type()) {
                Some(value) => Some(value),
                None => return RlvOutcome::FailedOption,
            },
            None => None,
        };
        if let Some(entry) = self.objects.get_mut(&object) {
            match value {
                Some(value) => entry.modifiers.insert(modifier, value),
                None => entry.modifiers.remove(&modifier),
            };
        }
        RlvOutcome::Success
    }

    // --------------------------------------------------------------- queries

    /// How many held commands reference-count `behaviour`.
    #[must_use]
    pub fn count(&self, behaviour: RlvBehaviour) -> u32 {
        self.counts.get(&behaviour).copied().unwrap_or(0)
    }

    /// Whether `behaviour` is in force — held by at least one object.
    #[must_use]
    pub fn has_behaviour(&self, behaviour: RlvBehaviour) -> bool {
        self.count(behaviour) > 0
    }

    /// Whether any object holds `behaviour` with exactly `option`.
    #[must_use]
    pub fn has_behaviour_with_option(&self, behaviour: RlvBehaviour, option: &str) -> bool {
        self.objects
            .values()
            .any(|entry| entry.holds(behaviour, option, false))
    }

    /// Whether some object *other than* `object` holds `behaviour` with
    /// `option` — the question "would this still be restricted if I lifted
    /// mine?" (`hasBehaviourExcept`, `rlvhandler.cpp:234`).
    #[must_use]
    pub fn has_behaviour_except(
        &self,
        behaviour: RlvBehaviour,
        option: &str,
        object: Uuid,
    ) -> bool {
        self.objects
            .iter()
            .any(|(&id, entry)| id != object && entry.holds(behaviour, option, false))
    }

    /// Whether `object` holds `behaviour` with `option`.
    #[must_use]
    pub fn has_behaviour_from(&self, object: Uuid, behaviour: RlvBehaviour, option: &str) -> bool {
        self.objects
            .get(&object)
            .is_some_and(|entry| entry.holds(behaviour, option, false))
    }

    /// Whether `object` is the *only* object holding `behaviour`
    /// (`ownsBehaviour`, `rlvhandler.cpp:251`).
    ///
    /// `false` when nobody holds it, so this is "mine and mine alone", not
    /// "nobody else's".
    #[must_use]
    pub fn owns_behaviour(&self, object: Uuid, behaviour: RlvBehaviour) -> bool {
        let mut owned = false;
        for (&id, entry) in &self.objects {
            if entry.holds_bare(behaviour, false) {
                if id != object {
                    return false;
                }
                owned = true;
            }
        }
        owned
    }

    /// Every object holding `behaviour`, in object-id order
    /// (`findBehaviour`, `rlvhandler.cpp:217`).
    pub fn objects_holding(&self, behaviour: RlvBehaviour) -> impl Iterator<Item = Uuid> {
        self.objects
            .iter()
            .filter(move |&(_, entry)| entry.holds_bare(behaviour, false))
            .map(|(&id, _)| id)
    }

    /// Every restriction `object` is holding, in the order it issued them.
    #[must_use]
    pub fn restrictions_of(&self, object: Uuid) -> &[RlvHeldCommand] {
        self.objects
            .get(&object)
            .map_or(&[], |entry| entry.commands.as_slice())
    }

    /// Every object that holds anything, in object-id order.
    pub fn restricting_objects(&self) -> impl Iterator<Item = Uuid> {
        self.objects.keys().copied()
    }

    /// The `@getstatus` reply for `object`: each of its restrictions matching
    /// `filter`, prefixed by `separator` (`RlvObject::getStatusString`,
    /// `rlvhelper.cpp:1208`).
    ///
    /// The reply *starts* with the separator, and an object holding nothing
    /// gets an empty string rather than a bare separator — both quirks are what
    /// scripts have been parsing since RLV 1.16.
    #[must_use]
    pub fn status_string(&self, object: Uuid, filter: &str, separator: &str) -> String {
        let mut status = String::new();
        for held in self.restrictions_of(object) {
            let text = held.as_string();
            if filter.is_empty() || text.contains(filter) {
                status.push_str(separator);
                status.push_str(&text);
            }
        }
        status
    }

    /// The `@getstatusall` reply: every object's [`RlvState::status_string`],
    /// concatenated in object-id order.
    #[must_use]
    pub fn status_string_all(&self, filter: &str, separator: &str) -> String {
        let mut status = String::new();
        for &object in self.objects.keys() {
            status.push_str(&self.status_string(object, filter, separator));
        }
        status
    }

    /// Whether `behaviour` has any exception at all.
    #[must_use]
    pub fn has_exception(&self, behaviour: RlvBehaviour) -> bool {
        self.exceptions
            .iter()
            .any(|entry| entry.behaviour == behaviour)
    }

    /// Every exception in force, in the order they were granted.
    pub fn exceptions(&self) -> impl Iterator<Item = &RlvException> {
        self.exceptions.iter()
    }

    /// Whether `option` is let through `behaviour`
    /// (`RlvHandler::isException`, `rlvhandler.cpp:275`).
    ///
    /// Under [`RlvExceptionCheck::Permissive`] one object's word is enough.
    /// Under [`RlvExceptionCheck::Strict`] every object holding the restriction
    /// must also have granted the exception — otherwise a second collar could
    /// undo the first one's block by granting an exception it never agreed to.
    /// [`RlvExceptionCheck::Automatic`] picks strict exactly when the
    /// restriction is in force and not permissive.
    #[must_use]
    pub fn is_exception(
        &self,
        behaviour: RlvBehaviour,
        option: RlvExceptionOption,
        check: RlvExceptionCheck,
    ) -> bool {
        let check = match check {
            RlvExceptionCheck::Automatic => {
                if self.has_behaviour(behaviour) && !self.is_permissive(behaviour) {
                    RlvExceptionCheck::Strict
                } else {
                    RlvExceptionCheck::Permissive
                }
            }
            other => other,
        };

        let matching = || {
            self.exceptions
                .iter()
                .filter(move |entry| entry.behaviour == behaviour && entry.option == option)
        };

        if check == RlvExceptionCheck::Permissive {
            return matching().next().is_some();
        }

        // Strict: collect everyone holding the restriction and tick them off.
        // `@permissive=n` switches strict mode on for *everybody*, so once it
        // is in force a holder that never said `_sec` has to be ticked off
        // too — which makes the check harder to pass, not easier.
        let strict_only = !self.has_behaviour(RlvBehaviour::Permissive);
        let mut pending: Vec<Uuid> = self
            .objects
            .iter()
            .filter(|&(_, entry)| entry.holds_bare(behaviour, strict_only))
            .map(|(&id, _)| id)
            .collect();
        for entry in matching() {
            if let Some(index) = pending.iter().position(|&id| id == entry.object) {
                pending.remove(index);
            }
            if pending.is_empty() {
                return true;
            }
        }
        false
    }

    /// Whether `behaviour`'s exceptions are permissive — one object's exception
    /// is enough (`RlvHandler::isPermissive`, `rlvhandler.cpp:311`).
    ///
    /// Read `@permissive` the other way round to its name: it is a
    /// *restriction*, and holding it forces every exception-carrying behaviour
    /// into strict mode at once, which is why it makes this `false`. A
    /// behaviour goes strict either that way or by some object having issued
    /// it as `_sec`, which records it against
    /// [`RlvBehaviour::Permissive`] as an exception. A behaviour with no `_sec`
    /// form has no strict mode to enter and is always permissive.
    #[must_use]
    pub fn is_permissive(&self, behaviour: RlvBehaviour) -> bool {
        if !behaviour.has_strict() {
            return true;
        }
        !(self.has_behaviour(RlvBehaviour::Permissive)
            || self.is_exception(
                RlvBehaviour::Permissive,
                RlvExceptionOption::Behaviour(behaviour),
                RlvExceptionCheck::Permissive,
            ))
    }

    /// The global modifier slots.
    #[must_use]
    pub const fn modifiers(&self) -> &RlvModifierState {
        &self.modifiers
    }

    /// The value `object` set for one of its own effect knobs, if any.
    #[must_use]
    pub fn local_modifier(
        &self,
        object: Uuid,
        modifier: RlvLocalModifier,
    ) -> Option<RlvModifierValue> {
        self.objects
            .get(&object)
            .and_then(|entry| entry.modifiers.get(&modifier))
            .copied()
    }
}

/// Parse an option as the 36-character hyphenated UUID the reference insists
/// on.
fn parse_uuid(option: &str) -> Option<Uuid> {
    (option.len() == 36)
        .then(|| Uuid::parse_str(option).ok())
        .flatten()
}

/// The channel the viewer reserves for its own debug output, and so will not
/// chat a reply on (`CHAT_CHANNEL_DEBUG`, `indra_constants.h:286`).
const CHAT_CHANNEL_DEBUG: i32 = i32::MAX;

/// Parse a chat channel, rejecting the ones the behaviour will not accept.
///
/// `@sendchannel` wants a positive channel — channel 0 is open chat, which it
/// does not govern (`rlvhandler.cpp:2209`). `@redirchat`, `@rediremote` and
/// `@notify` want a channel the viewer could actually chat *back* on, which
/// additionally rules out the debug channel (`RlvUtil::isValidReplyChannel`,
/// `rlvcommon.h:340`).
fn parse_channel(option: &str, behaviour: RlvBehaviour) -> Option<i32> {
    let channel = option.parse::<i32>().ok()?;
    let ok = match behaviour {
        RlvBehaviour::Sendchannel | RlvBehaviour::SendchannelExcept => channel > 0,
        _ => channel > 0 && channel != CHAT_CHANNEL_DEBUG,
    };
    ok.then_some(channel)
}

/// Parse a `@notify` option: `<channel>[;<filter>]`, where the channel has to
/// be one a reply could be chatted on (`rlvParseNotifyOption`,
/// `rlvhandler.cpp:97`).
///
/// An absent filter is an empty one, which is how the reference stores it and
/// what makes a bare `@notify:<channel>` hear everything.
fn parse_notify_option(option: &str) -> Option<(i32, &str)> {
    let (channel, filter) = match option.split_once(';') {
        Some((channel, filter)) => (channel, filter),
        None => (option, ""),
    };
    // At most one `;`: a second separator is a malformed option, not a filter
    // that happens to contain one.
    if filter.contains(';') {
        return None;
    }
    Some((parse_channel(channel, RlvBehaviour::Notify)?, filter))
}

/// The `behaviour[:option]` text a command reports itself as to `@notify` and
/// `@getstatus` (`RlvCommand::asString`, `rlvhelper.h:728`).
///
/// The keyword is what arrived, `_sec` and all, because that is the spelling
/// the object will use again when it lifts the restriction.
fn command_text(command: &RlvCommand) -> String {
    match command.option {
        Some(ref option) => format!("{}:{option}", command.keyword),
        None => command.keyword.clone(),
    }
}

/// Parse one metre distance from a `@recvim` / `@sendim` / `@startim` range.
///
/// Negative distances are refused, as the reference does before squaring them
/// (`rlvhandler.cpp:2234`).
fn parse_distance(text: &str) -> Option<f32> {
    let value = text.parse::<f32>().ok()?;
    (value >= 0.0).then_some(value)
}

/// The kind a decoded command has to have for [`RlvState::apply`] to own it.
///
/// Handy for a consumer deciding what to dispatch itself: everything else comes
/// back as [`RlvOutcome::NotAStateChange`].
#[must_use]
pub const fn is_state_change(command: &RlvCommand) -> bool {
    match command.param.kind() {
        RlvParamKind::AddRem | RlvParamKind::Clear => true,
        RlvParamKind::Force => command.modifier.is_some(),
        RlvParamKind::Reply => false,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        RlvException, RlvExceptionCheck, RlvExceptionOption, RlvOutcome, RlvState, is_state_change,
    };
    use crate::behaviour::{RlvBehaviour, RlvEntry, RlvLocalModifier};
    use crate::command::{RlvCommand, RlvParamKind};
    use crate::modifier::{
        DEFAULT_FIELD_OF_VIEW, FARTOUCH_DEFAULT, IMG_DEFAULT, RlvModifier, RlvModifierValue,
    };
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// The first object in every test — think "the collar".
    const COLLAR: Uuid = Uuid::from_u128(0x0000_0001);
    /// The second object — think "the cuffs".
    const CUFFS: Uuid = Uuid::from_u128(0x0000_0002);
    /// A third object, for the rare test that needs one.
    const ANKLETS: Uuid = Uuid::from_u128(0x0000_0003);

    /// An avatar id an exception can name.
    const ALICE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    /// A second avatar id.
    const BOB: &str = "bbbbbbbb-0000-4000-8000-000000000002";

    /// `ALICE`, parsed.
    fn alice() -> Uuid {
        Uuid::from_u128(0xaaaa_aaaa_0000_4000_8000_0000_0000_0001)
    }

    /// Apply one command field on behalf of `object`, failing the test if the
    /// field does not even decode.
    fn apply(state: &mut RlvState, object: Uuid, field: &str) -> Result<RlvOutcome, TestError> {
        let command = RlvCommand::parse_field(field)?;
        Ok(state.apply(object, &command))
    }

    /// Everything `@notify` has queued, drained, as `<channel> <line>` text.
    fn notifications(state: &mut RlvState) -> Vec<String> {
        state
            .take_notifications()
            .into_iter()
            .map(|note| format!("{} {}", note.channel, note.message))
            .collect()
    }

    /// Throw away what is queued, for a test that only cares what comes after.
    fn forget_notifications(state: &mut RlvState) {
        drop(state.take_notifications());
    }

    /// Apply a command field and assert it succeeded.
    fn ok(state: &mut RlvState, object: Uuid, field: &str) -> Result<(), TestError> {
        let outcome = apply(state, object, field)?;
        assert!(
            outcome.succeeded(),
            "`{field}` from {object} came back {outcome:?}"
        );
        Ok(())
    }

    #[test]
    fn a_restriction_is_in_force_while_any_object_holds_it() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fly=n")?;
        ok(&mut state, CUFFS, "fly=n")?;
        assert_eq!(state.count(RlvBehaviour::Fly), 2);

        ok(&mut state, COLLAR, "fly=y")?;
        assert!(
            state.has_behaviour(RlvBehaviour::Fly),
            "the cuffs still say no"
        );
        assert_eq!(state.count(RlvBehaviour::Fly), 1);

        ok(&mut state, CUFFS, "fly=y")?;
        assert!(!state.has_behaviour(RlvBehaviour::Fly));
        assert_eq!(
            state.restricting_objects().count(),
            0,
            "an object holding nothing is forgotten"
        );
        Ok(())
    }

    #[test]
    fn a_duplicate_add_is_one_restriction() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fly=n")?;
        assert_eq!(
            apply(&mut state, COLLAR, "fly=n")?,
            RlvOutcome::SuccessDuplicate
        );
        assert_eq!(state.count(RlvBehaviour::Fly), 1);

        // ... so one remove is enough to lift it.
        ok(&mut state, COLLAR, "fly=y")?;
        assert!(!state.has_behaviour(RlvBehaviour::Fly));
        Ok(())
    }

    #[test]
    fn removing_something_never_held_is_not_an_error() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "fly=y")?,
            RlvOutcome::SuccessUnset
        );
        assert_eq!(state.count(RlvBehaviour::Fly), 0);
        Ok(())
    }

    #[test]
    fn detaching_drops_everything_that_object_held() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fly=n")?;
        ok(&mut state, COLLAR, "showloc=n")?;
        ok(&mut state, CUFFS, "fly=n")?;

        state.clear_object(COLLAR);

        assert!(!state.has_behaviour(RlvBehaviour::Showloc));
        assert!(
            state.has_behaviour(RlvBehaviour::Fly),
            "the cuffs' `@fly=n` outlives the collar"
        );
        assert_eq!(state.restrictions_of(COLLAR).len(), 0);
        assert_eq!(state.count(RlvBehaviour::Fly), 1);
        Ok(())
    }

    #[test]
    fn clear_takes_a_substring_filter() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "tplm=n")?;
        ok(&mut state, COLLAR, "tploc=n")?;
        ok(&mut state, COLLAR, "showloc=n")?;

        // `@clear=tp` matches `tplm` and `tploc` — and nothing else, even
        // though `showloc` also ends in "loc".
        state.clear(COLLAR, Some("tp"));
        assert!(!state.has_behaviour(RlvBehaviour::Tplm));
        assert!(!state.has_behaviour(RlvBehaviour::Tploc));
        assert!(state.has_behaviour(RlvBehaviour::Showloc));

        state.clear(COLLAR, None);
        assert!(!state.has_behaviour(RlvBehaviour::Showloc));
        Ok(())
    }

    #[test]
    fn clear_matches_the_option_too() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, &format!("editobj:{ALICE}=n"))?;
        ok(&mut state, COLLAR, &format!("editobj:{BOB}=n"))?;
        assert_eq!(state.count(RlvBehaviour::Editobj), 2);

        // The filter runs against `keyword:option`, so it can name one target.
        state.clear(COLLAR, Some(ALICE));
        assert_eq!(state.count(RlvBehaviour::Editobj), 1);
        assert!(state.has_behaviour_with_option(RlvBehaviour::Editobj, BOB));
        Ok(())
    }

    #[test]
    fn a_synonym_shares_the_reference_count() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fartouch=n")?;
        ok(&mut state, CUFFS, "touchfar=n")?;
        assert_eq!(
            state.count(RlvBehaviour::Fartouch),
            2,
            "two spellings, one restriction"
        );

        ok(&mut state, CUFFS, "touchfar=y")?;
        assert_eq!(state.count(RlvBehaviour::Fartouch), 1);
        Ok(())
    }

    #[test]
    fn an_exception_does_not_restrict() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // `@sendim:<uuid>=add` lets one avatar through; it does not block IMs.
        ok(&mut state, COLLAR, &format!("sendim:{ALICE}=add"))?;
        assert!(
            !state.has_behaviour(RlvBehaviour::Sendim),
            "an exception is not a restriction"
        );
        assert!(state.has_exception(RlvBehaviour::Sendim));
        assert!(state.is_exception(
            RlvBehaviour::Sendim,
            RlvExceptionOption::Avatar(alice()),
            RlvExceptionCheck::Automatic,
        ));

        // The bare form is the restriction.
        ok(&mut state, COLLAR, "sendim=n")?;
        assert!(state.has_behaviour(RlvBehaviour::Sendim));
        assert_eq!(state.count(RlvBehaviour::Sendim), 1);
        Ok(())
    }

    #[test]
    fn a_required_option_restricts_and_names_its_target() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // `@recvimfrom:<uuid>=n` is not an exception to anything — it is the
        // restriction, and it names who it blocks.
        ok(&mut state, COLLAR, &format!("recvimfrom:{ALICE}=n"))?;
        assert!(state.has_behaviour(RlvBehaviour::Recvimfrom));
        assert!(state.has_exception(RlvBehaviour::Recvimfrom));
        Ok(())
    }

    #[test]
    fn strict_exceptions_need_every_holder_to_agree() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "sendim_sec=n")?;
        ok(&mut state, CUFFS, "sendim_sec=n")?;
        assert!(!state.is_permissive(RlvBehaviour::Sendim));

        // The collar alone letting Alice through is not enough: the cuffs
        // never agreed, and that is exactly what `_sec` buys.
        ok(&mut state, COLLAR, &format!("sendim:{ALICE}=add"))?;
        assert!(!state.is_exception(
            RlvBehaviour::Sendim,
            RlvExceptionOption::Avatar(alice()),
            RlvExceptionCheck::Automatic,
        ));
        assert!(
            state.is_exception(
                RlvBehaviour::Sendim,
                RlvExceptionOption::Avatar(alice()),
                RlvExceptionCheck::Permissive,
            ),
            "asked permissively, one object's word is enough"
        );

        // Once the cuffs agree too, Alice is through.
        ok(&mut state, CUFFS, &format!("sendim:{ALICE}=add"))?;
        assert!(state.is_exception(
            RlvBehaviour::Sendim,
            RlvExceptionOption::Avatar(alice()),
            RlvExceptionCheck::Automatic,
        ));
        Ok(())
    }

    #[test]
    fn permissive_switches_strictness_on_for_everybody() -> Result<(), TestError> {
        // `@permissive` reads backwards: holding it forces every
        // exception-carrying restriction into strict mode at once, so a plain
        // `@sendim=n` starts behaving like `@sendim_sec=n`.
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "sendim=n")?;
        ok(&mut state, CUFFS, "sendim=n")?;
        ok(&mut state, COLLAR, &format!("sendim:{ALICE}=add"))?;
        assert!(state.is_permissive(RlvBehaviour::Sendim));
        assert!(
            state.is_exception(
                RlvBehaviour::Sendim,
                RlvExceptionOption::Avatar(alice()),
                RlvExceptionCheck::Automatic,
            ),
            "with nothing strict, one object's exception is enough"
        );

        ok(&mut state, ANKLETS, "permissive=n")?;
        assert!(!state.is_permissive(RlvBehaviour::Sendim));
        assert!(
            !state.is_exception(
                RlvBehaviour::Sendim,
                RlvExceptionOption::Avatar(alice()),
                RlvExceptionCheck::Automatic,
            ),
            "the cuffs never agreed, and now that has to matter"
        );

        // Even the object that never said `_sec` has to be ticked off now.
        ok(&mut state, CUFFS, &format!("sendim:{ALICE}=add"))?;
        assert!(state.is_exception(
            RlvBehaviour::Sendim,
            RlvExceptionOption::Avatar(alice()),
            RlvExceptionCheck::Automatic,
        ));

        ok(&mut state, ANKLETS, "permissive=y")?;
        assert!(state.is_permissive(RlvBehaviour::Sendim));
        Ok(())
    }

    #[test]
    fn a_non_strict_behaviour_is_always_permissive() -> Result<(), TestError> {
        let state = RlvState::new();
        assert!(
            state.is_permissive(RlvBehaviour::Fly),
            "@fly has no `_sec` form, so strictness never applies to it"
        );
        assert!(!RlvBehaviour::Fly.has_strict());
        assert!(RlvBehaviour::Sendim.has_strict());
        Ok(())
    }

    #[test]
    fn lifting_a_strict_restriction_takes_its_permissive_exception_with_it() -> Result<(), TestError>
    {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "sendim_sec=n")?;
        assert_eq!(
            state
                .exceptions()
                .filter(|entry| entry.behaviour == RlvBehaviour::Permissive)
                .count(),
            1,
            "a strict add records itself against @permissive"
        );

        ok(&mut state, COLLAR, "sendim_sec=y")?;
        assert_eq!(
            state
                .exceptions()
                .filter(|entry| entry.behaviour == RlvBehaviour::Permissive)
                .count(),
            0
        );
        Ok(())
    }

    #[test]
    fn detach_is_held_but_never_counted() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "detach=n")?;
        // The reference deliberately does not reference-count `@detach`: the
        // lock lives on the attachment, and the question asked is always "does
        // *this* object hold it".
        assert_eq!(state.count(RlvBehaviour::Detach), 0);
        assert!(!state.has_behaviour(RlvBehaviour::Detach));
        assert!(state.has_behaviour_from(COLLAR, RlvBehaviour::Detach, ""));
        assert_eq!(state.restrictions_of(COLLAR).len(), 1);
        Ok(())
    }

    #[test]
    fn an_attachment_point_option_does_not_count() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // Bare `@addattach=n` locks every point and counts...
        ok(&mut state, COLLAR, "addattach=n")?;
        assert_eq!(state.count(RlvBehaviour::Addattach), 1);
        // ... while the per-point form locks one point and does not.
        ok(&mut state, CUFFS, "addattach:chest=n")?;
        assert_eq!(state.count(RlvBehaviour::Addattach), 1);
        assert!(state.has_behaviour_with_option(RlvBehaviour::Addattach, "chest"));
        Ok(())
    }

    #[test]
    fn exclusive_restrictions_admit_one_object() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "setenv=n")?;
        assert_eq!(
            apply(&mut state, CUFFS, "setenv=n")?,
            RlvOutcome::FailedLock,
            "two objects fighting over the environment is the deadlock this prevents"
        );
        assert_eq!(state.count(RlvBehaviour::Setenv), 1);
        assert_eq!(
            state.restrictions_of(CUFFS).len(),
            0,
            "a rejected add leaves no trace"
        );

        ok(&mut state, COLLAR, "setenv=y")?;
        ok(&mut state, CUFFS, "setenv=n")?;
        Ok(())
    }

    #[test]
    fn setsphere_admits_six_effects() -> Result<(), TestError> {
        let mut state = RlvState::new();
        for index in 0..6_u128 {
            ok(&mut state, Uuid::from_u128(0x100 + index), "setsphere=n")?;
        }
        assert_eq!(
            apply(&mut state, Uuid::from_u128(0x200), "setsphere=n")?,
            RlvOutcome::FailedLock
        );
        Ok(())
    }

    #[test]
    fn a_forbidden_option_is_refused() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "fly:2=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(state.restricting_objects().count(), 0);
        Ok(())
    }

    #[test]
    fn a_missing_required_option_is_refused() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "editobj=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, "setcam_avdist=n")?,
            RlvOutcome::FailedOption
        );
        Ok(())
    }

    #[test]
    fn an_unparsable_option_is_refused() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "editobj:not-a-uuid=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, "setcam_avdist:far=n")?,
            RlvOutcome::FailedOption
        );
        // Channel 0 is open chat, which `@sendchannel` does not govern.
        assert_eq!(
            apply(&mut state, COLLAR, "sendchannel:0=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(state.restricting_objects().count(), 0);
        Ok(())
    }

    #[test]
    fn a_bare_modifier_behaviour_contributes_its_default() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            state.modifiers().value(RlvModifier::FartouchDist),
            RlvModifierValue::Float(FARTOUCH_DEFAULT)
        );
        ok(&mut state, COLLAR, "fartouch=n")?;
        assert!(state.modifiers().has_value(RlvModifier::FartouchDist));
        assert_eq!(
            state.modifiers().value(RlvModifier::FartouchDist),
            RlvModifierValue::Float(FARTOUCH_DEFAULT)
        );

        ok(&mut state, COLLAR, "fartouch=y")?;
        assert!(!state.modifiers().has_value(RlvModifier::FartouchDist));
        Ok(())
    }

    #[test]
    fn the_most_restrictive_modifier_value_wins() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fartouch:5.0=n")?;
        ok(&mut state, CUFFS, "fartouch:2.0=n")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::FartouchDist),
            RlvModifierValue::Float(2.0),
            "the shortest reach wins"
        );

        // Take the strict one away and the loose one is in force again.
        ok(&mut state, CUFFS, "fartouch:2.0=y")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::FartouchDist),
            RlvModifierValue::Float(5.0)
        );
        Ok(())
    }

    #[test]
    fn a_maximum_slot_keeps_the_largest_value() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // `@setcam_avdistmin` is a floor, so the highest floor is the binding
        // one.
        ok(&mut state, COLLAR, "setcam_avdistmin:1.0=n")?;
        ok(&mut state, CUFFS, "setcam_avdistmin:3.0=n")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamAvdistmin),
            RlvModifierValue::Float(3.0)
        );
        Ok(())
    }

    #[test]
    fn im_ranges_land_squared_in_two_slots() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "recvim=n")?;
        ok(&mut state, COLLAR, "recvim:10;20=n")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::RecvImDistMin),
            RlvModifierValue::Float(100.0)
        );
        assert_eq!(
            state.modifiers().value(RlvModifier::RecvImDistMax),
            RlvModifierValue::Float(400.0)
        );
        assert_eq!(
            state.count(RlvBehaviour::Recvim),
            1,
            "the range form does not count a second time"
        );
        Ok(())
    }

    #[test]
    fn shownametags_takes_either_an_exception_or_a_distance() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, &format!("shownametags:{ALICE}=n"))?;
        assert_eq!(
            state.count(RlvBehaviour::Shownametags),
            0,
            "exempting one avatar restricts nothing"
        );
        assert!(state.has_exception(RlvBehaviour::Shownametags));

        ok(&mut state, CUFFS, "shownametags:12.5=n")?;
        assert_eq!(state.count(RlvBehaviour::Shownametags), 1);
        assert_eq!(
            state.modifiers().value(RlvModifier::ShownametagsDist),
            RlvModifierValue::Float(12.5)
        );
        Ok(())
    }

    #[test]
    fn camzoommin_counts_as_setcam_fovmin() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "camzoommin:2=n")?;
        assert_eq!(
            state.count(RlvBehaviour::SetcamFovmin),
            1,
            "the deprecated spelling counts as the modern behaviour"
        );
        assert_eq!(state.count(RlvBehaviour::Camzoommin), 0);
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamFovmin),
            RlvModifierValue::Float(DEFAULT_FIELD_OF_VIEW / 2.0)
        );

        ok(&mut state, COLLAR, "camzoommin:2=y")?;
        assert_eq!(state.count(RlvBehaviour::SetcamFovmin), 0);
        Ok(())
    }

    #[test]
    fn setcam_gives_one_object_the_last_word_on_every_camera_slot() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // The cuffs pin a tight FOV first...
        ok(&mut state, CUFFS, "setcam_fovmin:1.0=n")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamFovmin),
            RlvModifierValue::Float(1.0)
        );

        // ... then the collar takes exclusive control with a looser one. Its
        // value wins outright rather than losing on most-restrictive-wins.
        ok(&mut state, COLLAR, "setcam=n")?;
        ok(&mut state, COLLAR, "setcam_fovmin:0.5=n")?;
        assert_eq!(
            state.modifiers().primary_object(RlvModifier::SetcamFovmin),
            Some(COLLAR)
        );
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamFovmin),
            RlvModifierValue::Float(0.5)
        );

        // Releasing the camera hands the slot back to the usual rule.
        ok(&mut state, COLLAR, "setcam=y")?;
        assert_eq!(
            state.modifiers().primary_object(RlvModifier::SetcamFovmin),
            None
        );
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamFovmin),
            RlvModifierValue::Float(1.0),
            "the highest floor is binding again"
        );
        Ok(())
    }

    #[test]
    fn setcam_rewrites_the_unlock_count() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, CUFFS, "setcam_unlock=n")?;
        ok(&mut state, ANKLETS, "setcam_unlock=n")?;
        assert_eq!(state.count(RlvBehaviour::SetcamUnlock), 2);

        // The collar takes the camera and does *not* ask for unlock, so nobody
        // else's unlock is in force any more.
        ok(&mut state, COLLAR, "setcam=n")?;
        assert_eq!(state.count(RlvBehaviour::SetcamUnlock), 0);

        // Handing it back restores the real count.
        ok(&mut state, COLLAR, "setcam=y")?;
        assert_eq!(state.count(RlvBehaviour::SetcamUnlock), 2);
        Ok(())
    }

    #[test]
    fn the_forced_texture_defaults_to_the_unrezzed_one() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "setcam_textures=n")?;
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamTexture),
            RlvModifierValue::Uuid(IMG_DEFAULT)
        );
        ok(&mut state, CUFFS, &format!("setcam_textures:{ALICE}=n"))?;
        assert_eq!(
            state.modifiers().value(RlvModifier::SetcamTexture),
            RlvModifierValue::Uuid(IMG_DEFAULT),
            "the first value in an insertion-ordered slot stays in front"
        );
        Ok(())
    }

    #[test]
    fn a_local_modifier_needs_the_restriction_it_belongs_to() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "setsphere_mode:1=force")?,
            RlvOutcome::FailedUnheldBehaviour,
            "there is no sphere to configure yet"
        );

        ok(&mut state, COLLAR, "setsphere=n")?;
        ok(&mut state, COLLAR, "setsphere_mode:1=force")?;
        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::SphereMode),
            Some(RlvModifierValue::Int(1))
        );

        // A bad value is refused rather than stored.
        assert_eq!(
            apply(&mut state, COLLAR, "setsphere_mode:blue=force")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::SphereMode),
            Some(RlvModifierValue::Int(1))
        );

        // An option-less modifier command clears the knob.
        ok(&mut state, COLLAR, "setsphere_mode=force")?;
        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::SphereMode),
            None
        );
        Ok(())
    }

    #[test]
    fn local_modifiers_are_per_object_and_typed() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "setsphere=n")?;
        ok(&mut state, CUFFS, "setsphere=n")?;
        ok(&mut state, COLLAR, "setsphere_color:1/0/0=force")?;
        ok(&mut state, CUFFS, "setsphere_color:0/0/1=force")?;

        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::SphereColor),
            Some(RlvModifierValue::Vector3([1.0, 0.0, 0.0]))
        );
        assert_eq!(
            state.local_modifier(CUFFS, RlvLocalModifier::SphereColor),
            Some(RlvModifierValue::Vector3([0.0, 0.0, 1.0])),
            "two spheres do not share one colour"
        );
        Ok(())
    }

    #[test]
    fn lifting_the_restriction_drops_its_local_modifiers() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "setoverlay=n")?;
        ok(&mut state, COLLAR, "setoverlay_alpha:0.5=force")?;
        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::OverlayAlpha),
            Some(RlvModifierValue::Float(0.5))
        );

        ok(&mut state, COLLAR, "setoverlay=y")?;
        assert_eq!(
            state.local_modifier(COLLAR, RlvLocalModifier::OverlayAlpha),
            None,
            "the effect is gone, so its knobs are meaningless"
        );
        Ok(())
    }

    #[test]
    fn owns_behaviour_means_mine_and_mine_alone() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert!(
            !state.owns_behaviour(COLLAR, RlvBehaviour::Fly),
            "nobody holds it, so nobody owns it"
        );
        ok(&mut state, COLLAR, "fly=n")?;
        assert!(state.owns_behaviour(COLLAR, RlvBehaviour::Fly));

        ok(&mut state, CUFFS, "fly=n")?;
        assert!(!state.owns_behaviour(COLLAR, RlvBehaviour::Fly));
        assert_eq!(
            state.objects_holding(RlvBehaviour::Fly).collect::<Vec<_>>(),
            vec![COLLAR, CUFFS]
        );
        Ok(())
    }

    #[test]
    fn has_behaviour_except_asks_what_survives_lifting_mine() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "showloc=n")?;
        assert!(!state.has_behaviour_except(RlvBehaviour::Showloc, "", COLLAR));

        ok(&mut state, CUFFS, "showloc=n")?;
        assert!(state.has_behaviour_except(RlvBehaviour::Showloc, "", COLLAR));
        Ok(())
    }

    #[test]
    fn getstatus_reports_what_the_object_holds() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "tplm=n")?;
        ok(&mut state, COLLAR, "recvim_sec=n")?;
        ok(&mut state, COLLAR, &format!("editobj:{ALICE}=n"))?;

        assert_eq!(
            state.status_string(COLLAR, "", "/"),
            format!("/tplm/recvim_sec/editobj:{ALICE}"),
            "the reply starts with the separator and keeps the `_sec` spelling"
        );
        assert_eq!(state.status_string(COLLAR, "tp", "/"), "/tplm");
        assert_eq!(
            state.status_string(COLLAR, "", ","),
            format!(",tplm,recvim_sec,editobj:{ALICE}")
        );
        assert_eq!(
            state.status_string(CUFFS, "", "/"),
            "",
            "an object holding nothing gets an empty reply, not a bare separator"
        );

        ok(&mut state, CUFFS, "fly=n")?;
        assert_eq!(
            state.status_string_all("", "/"),
            format!("/tplm/recvim_sec/editobj:{ALICE}/fly")
        );
        Ok(())
    }

    #[test]
    fn actions_and_queries_are_not_state_changes() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(
                &mut state,
                COLLAR,
                "sit:a3f2c1d4-0000-4000-8000-000000000000=force"
            )?,
            RlvOutcome::NotAStateChange
        );
        assert_eq!(
            apply(&mut state, COLLAR, "version=2222")?,
            RlvOutcome::NotAStateChange
        );
        assert_eq!(state.restricting_objects().count(), 0);

        assert!(!is_state_change(&RlvCommand::parse_field("version=2222")?));
        assert!(!is_state_change(&RlvCommand::parse_field(
            "tpto:1/2/3=force"
        )?));
        assert!(is_state_change(&RlvCommand::parse_field("fly=n")?));
        assert!(is_state_change(&RlvCommand::parse_field("clear")?));
        assert!(is_state_change(&RlvCommand::parse_field(
            "setsphere_mode:1=force"
        )?));
        Ok(())
    }

    #[test]
    fn an_unknown_behaviour_changes_nothing() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "frobnicate=n")?,
            RlvOutcome::FailedParam
        );
        // `@tpto` is an action, so `@tpto=n` is not a restriction to add.
        assert_eq!(
            apply(&mut state, COLLAR, "tpto=n")?,
            RlvOutcome::FailedParam
        );
        assert_eq!(state.restricting_objects().count(), 0);
        Ok(())
    }

    #[test]
    fn exceptions_carry_the_object_that_granted_them() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "redirchat:2222=n")?;
        let granted: Vec<&RlvException> = state.exceptions().collect();
        assert_eq!(
            granted,
            vec![&RlvException {
                object: COLLAR,
                behaviour: RlvBehaviour::Redirchat,
                option: RlvExceptionOption::Channel(2222),
            }]
        );
        assert_eq!(
            state.count(RlvBehaviour::Redirchat),
            1,
            "`@redirchat` both restricts and names the channel"
        );

        state.clear_object(COLLAR);
        assert_eq!(state.exceptions().count(), 0);
        Ok(())
    }

    #[test]
    fn a_reply_channel_has_to_be_one_a_reply_could_come_back_on() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // `@redirchat` sends chat somewhere, so its channel must be one the
        // viewer would actually chat on: positive, and not the debug channel.
        ok(&mut state, COLLAR, "redirchat:2222=n")?;
        assert_eq!(
            apply(&mut state, COLLAR, "redirchat:0=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, "redirchat:-5=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, &format!("redirchat:{}=n", i32::MAX))?,
            RlvOutcome::FailedOption,
            "the debug channel is the viewer's own"
        );
        // `@sendchannel` only blocks a channel, so the debug one is fair game.
        ok(&mut state, COLLAR, &format!("sendchannel:{}=n", i32::MAX))?;
        Ok(())
    }

    #[test]
    fn notify_checks_its_channel_and_leaves_the_filter_alone() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        ok(&mut state, COLLAR, "notify:2222;tp=n")?;
        assert_eq!(state.count(RlvBehaviour::Notify), 2);

        assert_eq!(
            apply(&mut state, COLLAR, "notify:0;tp=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, "notify:nope=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(
            apply(&mut state, COLLAR, "notify:2222;tp;extra=n")?,
            RlvOutcome::FailedOption,
            "a `@notify` spec is a channel and at most one filter"
        );
        assert_eq!(
            apply(&mut state, COLLAR, "notify=n")?,
            RlvOutcome::FailedOption
        );
        assert_eq!(state.count(RlvBehaviour::Notify), 2);
        Ok(())
    }

    #[test]
    fn a_deprecated_spelling_works_and_says_so() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert_eq!(
            apply(&mut state, COLLAR, "camtextures=n")?,
            RlvOutcome::SuccessDeprecated
        );
        assert!(state.has_behaviour(RlvBehaviour::SetcamTextures));
        // The modern spelling is not deprecated.
        assert_eq!(
            apply(&mut state, CUFFS, "setcam_textures=n")?,
            RlvOutcome::Success
        );
        Ok(())
    }

    #[test]
    fn experimental_commands_can_be_switched_off() -> Result<(), TestError> {
        let mut state = RlvState::new();
        assert!(
            state.experimental_commands(),
            "the reference ships the experimental set on"
        );
        ok(&mut state, COLLAR, "shownearby=n")?;

        let mut plain = RlvState::new();
        plain.set_experimental_commands(false);
        assert_eq!(
            apply(&mut plain, COLLAR, "shownearby=n")?,
            RlvOutcome::FailedParam,
            "with the set off the keyword is not a command at all"
        );
        assert_eq!(plain.restricting_objects().count(), 0);
        // A non-experimental command still works.
        ok(&mut plain, COLLAR, "showloc=n")?;
        Ok(())
    }

    #[test]
    fn getcommand_lists_the_keywords_this_state_machine_accepts() -> Result<(), TestError> {
        let state = RlvState::new();

        let tp = state.known_commands("tplure", None);
        assert_eq!(
            tp,
            vec!["tplure".to_owned(), "tplure_sec".to_owned()],
            "a strict behaviour is listed in both spellings"
        );

        // The filter is a plain substring, as `@getcommand` specifies — so
        // "sit" also catches `@sitground` and `@unsit`.
        assert_eq!(
            state.known_commands("sit", Some(RlvParamKind::Force)),
            vec!["sit".to_owned(), "sitground".to_owned(), "unsit".to_owned()]
        );
        // `@sit` is declared once per kind, so restricting the kind is what
        // keeps it from being listed twice.
        assert_eq!(
            state
                .known_commands("", None)
                .iter()
                .filter(|command| command.as_str() == "sit")
                .count(),
            2
        );

        // The experimental set changes the answer.
        let mut plain = RlvState::new();
        plain.set_experimental_commands(false);
        assert!(state.known_commands("shownearby", None).len() == 1);
        assert!(plain.known_commands("shownearby", None).is_empty());

        // Everything, and nothing missing.
        assert_eq!(
            state.known_commands("", None).len(),
            RlvEntry::ALL.len()
                + RlvEntry::ALL
                    .iter()
                    .filter(|entry| entry.flags.is_strict())
                    .count()
        );
        Ok(())
    }

    #[test]
    fn a_hostile_command_stream_leaves_consistent_state() -> Result<(), TestError> {
        // Whatever an in-world object throws at it, the counts must never
        // outlive the objects that caused them.
        let fields = [
            "fly=n",
            "fly=n",
            "fly=y",
            "fly=y",
            "setenv=n",
            "setenv=n",
            "detach=n",
            "detach:chest=n",
            "sendim_sec=n",
            "sendim:not-a-uuid=add",
            "clear=nope",
            "fartouch:-1=n",
            "recvim:5;=n",
            "recvim:-3=n",
            "camzoommin:0=n",
            "setsphere=n",
            "setsphere_mode:9=force",
            "shownametags:abc=n",
            "sendchannel:-5=n",
            "editobj=n",
            "notify:0=n",
            "notify:2222=n",
            "notify:2222;a;b=n",
            "redirchat:2147483647=n",
            "clear",
        ];
        let mut state = RlvState::new();
        for object in [COLLAR, CUFFS] {
            for field in fields {
                let Ok(command) = RlvCommand::parse_field(field) else {
                    continue;
                };
                state.apply(object, &command);
            }
        }
        for object in [COLLAR, CUFFS] {
            state.clear_object(object);
        }

        for &behaviour in RlvBehaviour::ALL {
            assert_eq!(
                state.count(behaviour),
                0,
                "{behaviour:?} is still counted after every object let go"
            );
        }
        assert_eq!(state.restricting_objects().count(), 0);
        assert_eq!(state.exceptions().count(), 0);
        assert_eq!(state.modifiers().active().count(), 0);

        // Nobody is listening any more either: a subscription outliving the
        // object that made it would chat at a prim that is gone.
        forget_notifications(&mut state);
        ok(&mut state, ANKLETS, "fly=n")?;
        assert_eq!(notifications(&mut state), Vec::<String>::new());
        Ok(())
    }

    // ---------------------------------------------------------------- notify

    #[test]
    fn notify_reports_every_change_on_the_channel_it_named() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        // The subscription is in force by the time its own command is
        // reported, so the collar hears itself arrive.
        assert_eq!(notifications(&mut state), ["2222 /notify:2222=n"]);

        ok(&mut state, CUFFS, "detach=n")?;
        assert_eq!(notifications(&mut state), ["2222 /detach=n"]);
        ok(&mut state, CUFFS, "detach=y")?;
        assert_eq!(notifications(&mut state), ["2222 /detach=y"]);

        // Lifting the subscription is the one change it does not hear: the
        // reference removes it before reporting.
        ok(&mut state, COLLAR, "notify:2222=y")?;
        assert_eq!(notifications(&mut state), Vec::<String>::new());
        ok(&mut state, CUFFS, "detach=n")?;
        assert_eq!(notifications(&mut state), Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_notify_filter_matches_the_behaviour_half_only() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222;detach=n")?;
        forget_notifications(&mut state);

        ok(&mut state, CUFFS, "detach=n")?;
        ok(&mut state, CUFFS, "fly=n")?;
        ok(&mut state, CUFFS, "detach=y")?;
        assert_eq!(
            notifications(&mut state),
            ["2222 /detach=n", "2222 /detach=y"],
            "one filter hears a restriction go on and off, and hears nothing else"
        );

        // The param is glued on after the filter has had its say, so a filter
        // that only the param would satisfy matches nothing.
        ok(&mut state, COLLAR, "notify:2223;rem=n")?;
        forget_notifications(&mut state);
        ok(&mut state, CUFFS, "fly=rem")?;
        assert_eq!(notifications(&mut state), Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn notify_reports_the_spelling_the_object_used() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        forget_notifications(&mut state);

        // `add` and `n` are the same command and not the same three
        // characters; a script watching for one must see the one that arrived.
        ok(&mut state, CUFFS, "detach=add")?;
        ok(&mut state, CUFFS, "detach=rem")?;
        ok(&mut state, CUFFS, "recvim_sec=n")?;
        ok(&mut state, CUFFS, &format!("sendim:{ALICE}=n"))?;
        assert_eq!(
            notifications(&mut state),
            [
                "2222 /detach=add",
                "2222 /detach=rem",
                "2222 /recvim_sec=n",
                &format!("2222 /sendim:{ALICE}=n"),
            ]
        );
        Ok(())
    }

    #[test]
    fn notify_reports_a_command_that_failed() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        forget_notifications(&mut state);

        // RLV tells scripts about the commands it refused, so a script that
        // stopped hearing them would read that as a viewer gone deaf.
        assert_eq!(
            apply(&mut state, CUFFS, "bogus=n")?,
            RlvOutcome::FailedParam
        );
        assert_eq!(
            apply(&mut state, CUFFS, "notify:0=n")?,
            RlvOutcome::FailedOption
        );
        ok(&mut state, CUFFS, "fly=n")?;
        assert_eq!(
            apply(&mut state, CUFFS, "fly=n")?,
            RlvOutcome::SuccessDuplicate
        );
        assert_eq!(
            notifications(&mut state),
            [
                "2222 /bogus=n",
                "2222 /notify:0=n",
                "2222 /fly=n",
                "2222 /fly=n",
            ]
        );
        Ok(())
    }

    #[test]
    fn clear_reports_itself_and_not_what_it_lifted() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        ok(&mut state, CUFFS, "tplm=n")?;
        ok(&mut state, CUFFS, "tploc=n")?;
        ok(&mut state, CUFFS, "fly=n")?;
        forget_notifications(&mut state);

        ok(&mut state, CUFFS, "clear=tp")?;
        assert_eq!(
            notifications(&mut state),
            ["2222 /clear:tp"],
            "the two restrictions it lifted are internal removes, which the \
             reference's notify hook skips"
        );
        assert!(state.has_behaviour(RlvBehaviour::Fly));

        // An object that held nothing still says it cleared.
        ok(&mut state, ANKLETS, "clear")?;
        assert_eq!(notifications(&mut state), ["2222 /clear"]);
        Ok(())
    }

    #[test]
    fn a_detaching_object_announces_a_clear_it_no_longer_hears() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "notify:2222=n")?;
        ok(&mut state, CUFFS, "notify:2223=n")?;
        ok(&mut state, CUFFS, "fly=n")?;
        forget_notifications(&mut state);

        state.clear_object(CUFFS);
        assert_eq!(
            notifications(&mut state),
            ["2222 /clear"],
            "the detached object's own subscription went with it"
        );
        assert!(!state.has_behaviour(RlvBehaviour::Fly));

        // And it stays gone: nothing chats at a prim that is no longer there.
        ok(&mut state, ANKLETS, "fly=n")?;
        assert_eq!(notifications(&mut state), ["2222 /fly=n"]);
        Ok(())
    }

    #[test]
    fn every_subscription_hears_it_once() -> Result<(), TestError> {
        let mut state = RlvState::new();
        // Two objects, three subscriptions, two of them on one channel: the
        // reference merges none of them, and reports in object order.
        ok(&mut state, CUFFS, "notify:2223=n")?;
        ok(&mut state, CUFFS, "notify:2223;fly=n")?;
        ok(&mut state, COLLAR, "notify:2222=n")?;
        forget_notifications(&mut state);

        ok(&mut state, ANKLETS, "fly=n")?;
        assert_eq!(
            notifications(&mut state),
            ["2222 /fly=n", "2223 /fly=n", "2223 /fly=n"]
        );

        // The unfiltered one alone hears a change its sibling filtered out.
        ok(&mut state, ANKLETS, "showloc=n")?;
        assert_eq!(
            notifications(&mut state),
            ["2222 /showloc=n", "2223 /showloc=n"]
        );
        Ok(())
    }

    #[test]
    fn nothing_is_queued_for_nobody() -> Result<(), TestError> {
        let mut state = RlvState::new();
        ok(&mut state, COLLAR, "fly=n")?;
        ok(&mut state, COLLAR, "clear")?;
        state.clear_object(COLLAR);
        assert_eq!(
            notifications(&mut state),
            Vec::<String>::new(),
            "a consumer that never sees a `@notify` never pays for one"
        );

        // A query or an action is not a state change and is not reported.
        ok(&mut state, COLLAR, "notify:2222=n")?;
        forget_notifications(&mut state);
        assert_eq!(
            apply(&mut state, CUFFS, "version=2222")?,
            RlvOutcome::NotAStateChange
        );
        assert_eq!(
            apply(
                &mut state,
                CUFFS,
                "sit:00000000-0000-4000-8000-000000000001=force"
            )?,
            RlvOutcome::NotAStateChange
        );
        assert_eq!(notifications(&mut state), Vec::<String>::new());
        Ok(())
    }
}
