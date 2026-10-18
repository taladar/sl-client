//! A message box from the grid: an `ImprovedInstantMessage` of dialog
//! `IM_MESSAGEBOX`.
//!
//! The reference's `LLIMProcessing::processNewMessage` shows one as the
//! `SystemMessageTip` with the message as its `MESSAGE`, whoever it is from.
//! OpenSim sends its refusals this way ("blue box" messages, from nobody, with
//! no name): a purchase of an object for any price but L$ 0 is answered
//! "Buying anything for a price other than zero is not implemented"
//! (`book/src/gridspec/economy.md`). Second Life was not measured to send one.

use bevy::prelude::*;

use sl_client_bevy::{ImDialog, InstantMessage, SlEvent, SlSessionEvent};

use crate::notifications::ShowNotification;

/// The tip a message box raises, or `None` for any other instant message.
#[must_use]
pub fn message_box_notice(im: &InstantMessage) -> Option<ShowNotification> {
    (im.dialog == ImDialog::MessageBox)
        .then(|| ShowNotification::new("SystemMessageTip").arg("MESSAGE", im.message.clone()))
}

/// Raises the reference's tip for every message box the grid sends.
pub fn ingest_message_boxes(
    mut events: MessageReader<SlEvent>,
    mut show: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        if let SlSessionEvent::InstantMessageReceived(im) = &event.0
            && let Some(notice) = message_box_notice(im)
        {
            show.write(notice);
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, ImDialog, InstantMessage, RegionCoordinates, Uuid};

    use super::message_box_notice;

    /// OpenSim's refusal of a paid purchase, as it arrives.
    fn blue_box(dialog: ImDialog) -> InstantMessage {
        InstantMessage {
            from_agent_id: AgentKey::from(Uuid::nil()),
            from_agent_name: String::new(),
            to_agent_id: AgentKey::from(Uuid::from_u128(0xb)),
            dialog,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: Uuid::nil(),
            parent_estate_id: 0,
            message: "Buying anything for a price other than zero is not implemented".to_owned(),
            binary_bucket: Vec::new(),
        }
    }

    /// A message box raises the system tip with its message.
    #[test]
    fn a_message_box_is_a_system_tip() {
        let notice = message_box_notice(&blue_box(ImDialog::MessageBox));
        assert_eq!(
            notice.map(|notice| (notice.template, notice.args.pairs().to_vec())),
            Some((
                "SystemMessageTip",
                vec![(
                    "MESSAGE".to_owned(),
                    "Buying anything for a price other than zero is not implemented".to_owned()
                )]
            ))
        );
    }

    /// Any other instant message is not one.
    #[test]
    fn an_ordinary_message_is_no_tip() {
        assert!(message_box_notice(&blue_box(ImDialog::Message)).is_none());
    }
}
