//! What a script may do to an avatar: the permissions it asked for, who
//! granted them, and the question still waiting for an answer
//! (`llRequestPermissions`, `run_time_permissions`).
//!
//! The grant is the script's own, as each grid keeps it on the script's
//! inventory item: a state change keeps it, a reset clears it, and the host
//! carries it across a take and a rez with the instance (book chapter
//! `gridspec/lsl.md`, § When a grant lapses). The question goes to the host,
//! which puts it to the avatar's viewer; the answer comes back through
//! [`Engine::answer`](crate::vm::Engine::answer), and a viewer's revocation
//! through [`Engine::revoke`](crate::vm::Engine::revoke).

use crate::vm::flavour::EngineFlavour;

/// `PERMISSION_DEBIT`: take the owner's money.
pub const PERMISSION_DEBIT: i32 = 0x2;
/// `PERMISSION_TAKE_CONTROLS`.
pub const PERMISSION_TAKE_CONTROLS: i32 = 0x4;
/// `PERMISSION_TRIGGER_ANIMATION`.
pub const PERMISSION_TRIGGER_ANIMATION: i32 = 0x10;
/// `PERMISSION_ATTACH`.
pub const PERMISSION_ATTACH: i32 = 0x20;
/// `PERMISSION_TRACK_CAMERA`.
pub const PERMISSION_TRACK_CAMERA: i32 = 0x400;
/// `PERMISSION_CONTROL_CAMERA`.
pub const PERMISSION_CONTROL_CAMERA: i32 = 0x800;
/// `PERMISSION_OVERRIDE_ANIMATIONS`.
pub const PERMISSION_OVERRIDE_ANIMATIONS: i32 = 0x8000;

/// What an attachment is granted without a question when it asks its wearer:
/// OpenSim's `implicitPerms` for an attachment (`LSL_Api.llRequestPermissions`),
/// which is also the set Second Life documents. `PERMISSION_DEBIT` is never
/// among them: worn, the probe's question of 1046 was still put (`money/lapse`).
pub const WORN_IMPLICIT: i32 = PERMISSION_TAKE_CONTROLS
    | PERMISSION_TRIGGER_ANIMATION
    | PERMISSION_CONTROL_CAMERA
    | PERMISSION_TRACK_CAMERA
    | PERMISSION_ATTACH
    | PERMISSION_OVERRIDE_ANIMATIONS;

/// What an object is granted without a question when it asks an avatar
/// sitting on it — the owner or another alike, on both grids (`avatar/sit`).
pub const SEATED_IMPLICIT: i32 = PERMISSION_TRIGGER_ANIMATION
    | PERMISSION_CONTROL_CAMERA
    | PERMISSION_TRACK_CAMERA
    | PERMISSION_TAKE_CONTROLS;

/// Every permission bit: a viewer's revocation of all of them
/// (`0xFFFFFFFF`) clears the granter too.
const ALL: i32 = -1;

/// A permission question the script put to an avatar, for the host to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The avatar asked.
    pub agent: String,
    /// The permissions asked for, as the script asked: no grid narrows the
    /// question.
    pub mask: i32,
}

/// A script's permissions: the bits it holds, who granted them, and the
/// question it is waiting on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Permissions {
    /// Who granted what is held — `llGetPermissionsKey`; [`None`] is
    /// `NULL_KEY`.
    granter: Option<String>,
    /// What is held — `llGetPermissions`.
    mask: i32,
    /// The question put and not yet answered.
    asked: Option<Question>,
}

impl Permissions {
    /// The avatar who granted what is held, [`None`] for nobody.
    #[must_use]
    pub fn granter(&self) -> Option<&str> {
        self.granter.as_deref()
    }

    /// The bits held.
    #[must_use]
    pub const fn mask(&self) -> i32 {
        self.mask
    }

    /// Whether every bit of `bits` is held.
    #[must_use]
    pub const fn holds(&self, bits: i32) -> bool {
        self.mask & bits == bits
    }

    /// The question waiting for an answer, if one is.
    #[must_use]
    pub const fn asked(&self) -> Option<&Question> {
        self.asked.as_ref()
    }

    /// `agent` holds `mask` for the script now, without a question — an
    /// implicit grant, or OpenSim's release (`NULL_KEY` and 0).
    pub(crate) fn grant(&mut self, agent: Option<String>, mask: i32) {
        self.granter = agent;
        self.mask = mask;
    }

    /// `question` is put. OpenSim names the avatar asked as the granter at
    /// once, holding nothing until the answer, unless it was already waiting
    /// for one (`LSL_Api.llRequestPermissions`); Second Life keeps what is
    /// held until the answer comes (`money/debit`'s non-owner, refused).
    pub(crate) fn ask(&mut self, question: Question, flavour: EngineFlavour) {
        if flavour.is_opensim() && self.asked.is_none() {
            self.granter = Some(question.agent.clone());
            self.mask = 0;
        }
        self.asked = Some(question);
    }

    /// `agent` answered the question put to it with `granted`: it is held as
    /// it came, more than was asked included — no grid masks an answer by
    /// its question. `false`, with nothing changed, when no question is
    /// waiting for `agent`: an answer nobody asked for is nothing
    /// (`avatar/permissions`, the camera question Second Life never put).
    pub(crate) fn answer(&mut self, agent: &str, granted: i32) -> bool {
        if !self
            .asked
            .as_ref()
            .is_some_and(|question| question.agent.eq_ignore_ascii_case(agent))
        {
            return false;
        }
        self.asked = None;
        self.granter = Some(agent.to_owned());
        self.mask = granted;
        true
    }

    /// `agent`'s viewer revoked `bits`: they are dropped when `agent` is the
    /// granter, the rest and the granter kept; revoking every bit clears the
    /// granter too (`money/lapse`, on every grid). No event is raised.
    pub(crate) fn revoke(&mut self, agent: &str, bits: i32) {
        if !self
            .granter
            .as_deref()
            .is_some_and(|granter| granter.eq_ignore_ascii_case(agent))
        {
            return;
        }
        self.mask &= !bits;
        if bits == ALL {
            self.granter = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const OWNER: &str = "0a0a0a0a-0000-4000-8000-000000000001";

    #[test]
    fn an_answer_is_held_as_it_came_and_only_for_a_question() {
        let mut held = Permissions::default();
        assert!(!held.answer(OWNER, 16));
        held.ask(
            Question {
                agent: OWNER.to_owned(),
                mask: 16,
            },
            EngineFlavour::SecondLife,
        );
        assert!(held.answer(OWNER, 1044));
        assert_eq!((held.granter(), held.mask()), (Some(OWNER), 1044));
        assert_eq!(held.asked(), None);
    }

    #[test]
    fn opensim_names_the_avatar_asked_at_once_second_life_keeps_the_grant() {
        for (flavour, expected) in [
            (EngineFlavour::SecondLife, (Some(OWNER), 2)),
            (EngineFlavour::XEngine, (Some("other"), 0)),
        ] {
            let mut held = Permissions::default();
            held.grant(Some(OWNER.to_owned()), 2);
            held.ask(
                Question {
                    agent: "other".to_owned(),
                    mask: 2,
                },
                flavour,
            );
            assert_eq!((held.granter(), held.mask()), expected, "{flavour:?}");
        }
    }

    #[test]
    fn a_revocation_drops_its_bits_and_every_bit_drops_the_granter() {
        let mut held = Permissions::default();
        held.grant(Some(OWNER.to_owned()), 1046);
        held.revoke("someone else", 16);
        assert_eq!(held.mask(), 1046);
        held.revoke(
            OWNER,
            PERMISSION_TRIGGER_ANIMATION | PERMISSION_OVERRIDE_ANIMATIONS,
        );
        assert_eq!((held.granter(), held.mask()), (Some(OWNER), 1030));
        held.revoke(OWNER, ALL);
        assert_eq!((held.granter(), held.mask()), (None, 0));
    }
}
