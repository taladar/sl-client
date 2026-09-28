//! Requests to a viewer and the responses it sends back.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::failure::AutomationError;
use crate::locator::Locator;
use crate::snapshot::UiNode;

/// Pairs a [`Response`] with the [`Request`] it answers, so several requests
/// may be in flight on one channel. Chosen by the requester; the viewer only
/// echoes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

/// A request to a viewer.
///
/// In JSON the body's fields sit beside the id, tagged by `method`:
/// `{"id":1,"method":"click","locator":{"role":"button","name":{"exact":"OK"}}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Echoed in the response.
    pub id: RequestId,
    /// What to do.
    #[serde(flatten)]
    pub body: RequestBody,
}

/// What a [`Request`] asks the viewer to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum RequestBody {
    /// The semantic UI tree, whole or under one node. Answered with
    /// [`ResponseBody::Snapshot`].
    Snapshot {
        /// Only the subtree of the one node this resolves to; the whole UI
        /// when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<Locator>,
    },
    /// Every node the locator matches, without waiting and without
    /// strictness. Answered with [`ResponseBody::Found`], possibly empty.
    Find {
        /// The nodes to find.
        locator: Locator,
    },
    /// Wait until exactly one node matches and is actionable, then click it
    /// through the viewer's real input path. Answered with
    /// [`ResponseBody::Done`].
    Click {
        /// The node to click.
        locator: Locator,
    },
    /// Wait until exactly one text field matches and is actionable and
    /// editable, then replace its text by typing. Answered with
    /// [`ResponseBody::Done`].
    Fill {
        /// The field to fill.
        locator: Locator,
        /// The text it should hold afterwards.
        text: String,
    },
    /// Wait until the locator's matches satisfy a condition. Answered with
    /// [`ResponseBody::Satisfied`].
    WaitFor {
        /// The nodes to watch.
        locator: Locator,
        /// What they must come to satisfy.
        condition: WaitCondition,
        /// When to give up.
        #[serde(default)]
        deadline: Deadline,
    },
}

/// What a [`RequestBody::WaitFor`] waits for, evaluated in the viewer each
/// frame over the nodes the locator matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    /// At least one node matches.
    Attached,
    /// No node matches.
    Detached,
    /// At least one matching node is visible.
    Visible,
    /// No matching node is visible (including when none matches).
    Hidden,
    /// At least one node matches and none of them is disabled.
    Enabled,
    /// At least one node matches and all of them are disabled.
    Disabled,
}

impl WaitCondition {
    /// The condition's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Detached => "detached",
            Self::Visible => "visible",
            Self::Hidden => "hidden",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

impl fmt::Display for WaitCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When a wait gives up: after a number of frames or of wall-clock
/// milliseconds, whichever comes first. An unset limit is the viewer's
/// default, so [`Deadline::default`] asks for the default wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deadline {
    /// The most frames to wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// The most wall-clock milliseconds to wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub millis: Option<u64>,
}

/// A viewer's answer to one [`Request`].
///
/// In JSON the outcome is one of two keys, `ok` or `error`:
/// `{"id":1,"ok":{"kind":"found","nodes":[]}}` or
/// `{"id":1,"error":{"kind":"not_found","locator":{}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The id of the request this answers.
    pub id: RequestId,
    /// What came of it.
    #[serde(flatten, with = "outcome")]
    pub result: Result<ResponseBody, AutomationError>,
}

/// What a request that succeeded produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseBody {
    /// The answer to [`RequestBody::Snapshot`]: the top-level nodes of the
    /// requested tree, each with its children.
    Snapshot {
        /// The tree's roots in reading order.
        roots: Vec<UiNode>,
    },
    /// The answer to [`RequestBody::Find`].
    Found {
        /// Every match in reading order, each without its children.
        nodes: Vec<UiNode>,
    },
    /// The answer to an action ([`RequestBody::Click`],
    /// [`RequestBody::Fill`]).
    Done {
        /// The node acted on, as it was when the action was applied.
        node: UiNode,
    },
    /// The answer to [`RequestBody::WaitFor`].
    Satisfied {
        /// The matches in the frame the condition held, without children;
        /// empty for a condition that holds with no match.
        nodes: Vec<UiNode>,
    },
}

/// The JSON shape of [`Response::result`]: `{"ok": …}` or `{"error": …}`
/// rather than serde's default `{"Ok": …}` / `{"Err": …}`.
mod outcome {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::failure::AutomationError;
    use crate::message::ResponseBody;

    /// The borrowed form written out.
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    enum Borrowed<'a> {
        /// A success.
        Ok(&'a ResponseBody),
        /// A failure.
        Error(&'a AutomationError),
    }

    /// The owned form read back.
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Owned {
        /// A success.
        Ok(ResponseBody),
        /// A failure.
        Error(AutomationError),
    }

    /// Writes a result as `{"ok": …}` or `{"error": …}`.
    pub(super) fn serialize<S: Serializer>(
        result: &Result<ResponseBody, AutomationError>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match result {
            Ok(body) => Borrowed::Ok(body),
            Err(error) => Borrowed::Error(error),
        }
        .serialize(serializer)
    }

    /// Reads a result written by [`serialize`].
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Result<ResponseBody, AutomationError>, D::Error> {
        Ok(match Owned::deserialize(deserializer)? {
            Owned::Ok(body) => Ok(body),
            Owned::Error(error) => Err(error),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::{Deadline, Request, RequestBody, RequestId, Response, ResponseBody, WaitCondition};
    use crate::failure::{ActionabilityCheck, AutomationError};
    use crate::locator::Locator;
    use crate::snapshot::{Bounds, NodeId, NodeState, NodeValue, NodeVisibility, Role, UiNode};

    /// Serializes `value`, reads it back and checks nothing was lost.
    fn round_trip<T>(value: &T) -> Result<String, serde_json::Error>
    where
        T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let json = serde_json::to_string(value)?;
        let back: T = serde_json::from_str(&json)?;
        assert_eq!(&back, value, "{json}");
        Ok(json)
    }

    /// A text field with every optional part filled, and one child.
    fn full_node() -> UiNode {
        UiNode {
            id: NodeId(42),
            role: Role::Textbox,
            name: Some("Display name".to_owned()),
            name_key: Some("profile-display-name".to_owned()),
            test_id: Some("profile.display_name".to_owned()),
            states: [NodeState::Focused, NodeState::ReadOnly]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            value: Some(NodeValue::Text("Avatar".to_owned())),
            bounds: Bounds {
                x: 10.5,
                y: 20.25,
                width: 200.0,
                height: 18.0,
            },
            visibility: NodeVisibility::Covered,
            children: vec![bare_node()],
        }
    }

    /// A slider with nothing optional set.
    fn bare_node() -> UiNode {
        UiNode {
            id: NodeId(43),
            role: Role::Slider,
            name: None,
            name_key: None,
            test_id: None,
            states: BTreeSet::new(),
            value: Some(NodeValue::Number(0.3)),
            bounds: Bounds::default(),
            visibility: NodeVisibility::Visible,
            children: Vec::new(),
        }
    }

    /// A locator using every field.
    fn full_locator() -> Locator {
        Locator::role(Role::Checkbox)
            .named("Always run")
            .name_key("pref-always-run")
            .within(Locator::test_id("floater.preferences").name_containing("Pref"))
            .nth(0)
            .enabled(true)
            .checked(false)
            .selected(false)
            .expanded(false)
            .focused(true)
    }

    #[test]
    fn every_request_round_trips() -> Result<(), serde_json::Error> {
        let bodies = [
            RequestBody::Snapshot { within: None },
            RequestBody::Snapshot {
                within: Some(full_locator()),
            },
            RequestBody::Find {
                locator: full_locator(),
            },
            RequestBody::Click {
                locator: Locator::role(Role::Button).named("OK"),
            },
            RequestBody::Fill {
                locator: Locator::role(Role::Textbox),
                text: "hello \"world\"".to_owned(),
            },
            RequestBody::WaitFor {
                locator: Locator::test_id("floater.inventory"),
                condition: WaitCondition::Detached,
                deadline: Deadline::default(),
            },
            RequestBody::WaitFor {
                locator: Locator::test_id("floater.inventory"),
                condition: WaitCondition::Enabled,
                deadline: Deadline {
                    frames: Some(600),
                    millis: Some(10_000),
                },
            },
        ];
        for (index, body) in bodies.into_iter().enumerate() {
            round_trip(&Request {
                id: RequestId(u64::try_from(index).unwrap_or(u64::MAX)),
                body,
            })?;
        }
        Ok(())
    }

    #[test]
    fn every_response_round_trips() -> Result<(), serde_json::Error> {
        let results = [
            Ok(ResponseBody::Snapshot {
                roots: vec![full_node()],
            }),
            Ok(ResponseBody::Found {
                nodes: vec![bare_node(), full_node()],
            }),
            Ok(ResponseBody::Done { node: full_node() }),
            Ok(ResponseBody::Satisfied { nodes: Vec::new() }),
            Err(AutomationError::NotFound {
                locator: full_locator(),
            }),
            Err(AutomationError::Ambiguous {
                locator: Locator::role(Role::Button),
                candidates: vec![bare_node(), full_node()],
            }),
            Err(AutomationError::NotActionable {
                locator: full_locator(),
                check: ActionabilityCheck::Editable,
                node: full_node(),
            }),
            Err(AutomationError::TimedOut {
                locator: full_locator(),
                condition: Some(WaitCondition::Hidden),
                failed_check: None,
                last_observed: vec![full_node()],
                frames: 600,
                millis: 10_000,
            }),
            Err(AutomationError::TimedOut {
                locator: full_locator(),
                condition: None,
                failed_check: Some(ActionabilityCheck::Stable),
                last_observed: Vec::new(),
                frames: 1,
                millis: 16,
            }),
        ];
        for (index, result) in results.into_iter().enumerate() {
            round_trip(&Response {
                id: RequestId(u64::try_from(index).unwrap_or(u64::MAX)),
                result,
            })?;
        }
        Ok(())
    }

    #[test]
    fn every_enum_spelling_round_trips() -> Result<(), serde_json::Error> {
        for role in [
            Role::Button,
            Role::Checkbox,
            Role::Radio,
            Role::Textbox,
            Role::Combobox,
            Role::Slider,
            Role::Tab,
            Role::MenuItem,
            Role::ListItem,
            Role::Window,
            Role::Text,
            Role::Image,
            Role::Group,
        ] {
            let json = round_trip(&role)?;
            assert_eq!(json, format!("\"{role}\""), "display matches serde");
        }
        for check in [
            ActionabilityCheck::Attached,
            ActionabilityCheck::Visible,
            ActionabilityCheck::InViewport,
            ActionabilityCheck::Stable,
            ActionabilityCheck::Enabled,
            ActionabilityCheck::Editable,
            ActionabilityCheck::ReceivesEvents,
        ] {
            let json = round_trip(&check)?;
            assert_eq!(json, format!("\"{check}\""), "display matches serde");
        }
        for condition in [
            WaitCondition::Attached,
            WaitCondition::Detached,
            WaitCondition::Visible,
            WaitCondition::Hidden,
            WaitCondition::Enabled,
            WaitCondition::Disabled,
        ] {
            let json = round_trip(&condition)?;
            assert_eq!(json, format!("\"{condition}\""), "display matches serde");
        }
        for state in [
            NodeState::Disabled,
            NodeState::ReadOnly,
            NodeState::Checked,
            NodeState::Selected,
            NodeState::Expanded,
            NodeState::Focused,
            NodeState::Hovered,
        ] {
            round_trip(&state)?;
        }
        for visibility in [
            NodeVisibility::Visible,
            NodeVisibility::Hidden,
            NodeVisibility::Clipped,
            NodeVisibility::OffScreen,
            NodeVisibility::Covered,
        ] {
            round_trip(&visibility)?;
        }
        Ok(())
    }

    #[test]
    fn wire_shape_is_flat_and_terse() -> Result<(), serde_json::Error> {
        let request = Request {
            id: RequestId(1),
            body: RequestBody::Click {
                locator: Locator::role(Role::Button).named("OK"),
            },
        };
        assert_eq!(
            serde_json::to_string(&request)?,
            r#"{"id":1,"method":"click","locator":{"role":"button","name":{"exact":"OK"}}}"#
        );
        let found = Response {
            id: RequestId(2),
            result: Ok(ResponseBody::Found { nodes: Vec::new() }),
        };
        assert_eq!(
            serde_json::to_string(&found)?,
            r#"{"id":2,"ok":{"kind":"found","nodes":[]}}"#
        );
        let missing = Response {
            id: RequestId(3),
            result: Err(AutomationError::NotFound {
                locator: Locator::default(),
            }),
        };
        assert_eq!(
            serde_json::to_string(&missing)?,
            r#"{"id":3,"error":{"kind":"not_found","locator":{}}}"#
        );
        Ok(())
    }

    #[test]
    fn a_wait_without_deadline_asks_for_the_default() -> Result<(), serde_json::Error> {
        let request: Request = serde_json::from_str(
            r#"{"id":9,"method":"wait_for","locator":{"test_id":"x"},"condition":"visible"}"#,
        )?;
        assert_eq!(
            request.body,
            RequestBody::WaitFor {
                locator: Locator::test_id("x"),
                condition: WaitCondition::Visible,
                deadline: Deadline::default(),
            }
        );
        Ok(())
    }

    #[test]
    fn misspelt_fields_are_refused() {
        for json in [
            r#"{"id":1,"method":"find","locator":{"role":"button","nme":"OK"}}"#,
            r#"{"id":1,"method":"wait_for","locator":{},"condition":"visible","deadline":{"frame":3}}"#,
            r#"{"id":1,"method":"press","locator":{}}"#,
        ] {
            assert!(
                serde_json::from_str::<Request>(json).is_err(),
                "accepted {json}"
            );
        }
    }
}
