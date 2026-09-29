//! Expectations: `viewer.expect(&locator).to_be_disabled()`,
//! `viewer.expect_chat().to_contain("hello")`. Each is a wait **in the
//! viewer**, evaluated every frame until it holds or its timeout (the
//! viewer's default unless set with `timeout`) runs out — never a read and a
//! sleep. One that does not come to hold is a [`DriverError::Failed`] with
//! the viewer's last observation, its report and the saved artifacts.

use std::time::Duration;

use serde_json::{Value as JsonValue, json};
use sl_automation_proto::{
    Probe, RequestBody, ResponseBody, StateCondition, StateObservation, UiNode, ValueTest,
    WaitCondition, WorldNode, WorldWaitCondition,
};

use crate::artifacts::Subject;
use crate::error::DriverError;
use crate::ui::UiLocator;
use crate::viewer::Viewer;
use crate::world::WorldHandle;

/// An expectation on the nodes a UI locator names.
#[derive(Debug, Clone)]
pub struct UiExpect {
    /// The nodes.
    locator: UiLocator,
    /// How long it may take to hold; the locator's wait when `None`.
    timeout: Option<Duration>,
}

impl UiExpect {
    /// An expectation on `locator`'s nodes.
    pub(crate) const fn new(locator: UiLocator) -> Self {
        Self {
            locator,
            timeout: None,
        }
    }

    /// Give it `timeout` to hold.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Wait for the matches of `locator` to satisfy `condition`; `wanted`
    /// says what in words.
    async fn hold(
        &self,
        locator: sl_automation_proto::Locator,
        condition: WaitCondition,
        wanted: &str,
    ) -> Result<Vec<UiNode>, DriverError> {
        let viewer = self.locator.viewer();
        let timeout = self.timeout.unwrap_or_else(|| self.locator.wait());
        let action = format!("expect {} to {wanted}", self.locator.as_locator());
        match viewer
            .ask(
                RequestBody::WaitFor {
                    locator: locator.clone(),
                    condition,
                    deadline: Viewer::deadline(timeout),
                },
                timeout,
                &action,
                Subject::Ui(&locator),
            )
            .await?
        {
            ResponseBody::Satisfied { nodes } => Ok(nodes),
            other => Err(viewer.unexpected(&action, &other)),
        }
    }

    /// The locator's own query.
    fn query(&self) -> sl_automation_proto::Locator {
        self.locator.as_locator().clone()
    }

    /// At least one node matches.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when it does not come to hold.
    pub async fn to_be_attached(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Attached, "be attached")
            .await
    }

    /// No node matches.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_detached(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Detached, "be detached")
            .await
    }

    /// At least one matching node can be seen.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_visible(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Visible, "be visible")
            .await
    }

    /// No matching node can be seen (none matching counts).
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_hidden(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Hidden, "be hidden")
            .await
    }

    /// Some node matches and none is disabled.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_enabled(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Enabled, "be enabled")
            .await
    }

    /// Some node matches and all are disabled.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_disabled(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(self.query(), WaitCondition::Disabled, "be disabled")
            .await
    }

    /// A matching node's text — a field's or a label's value, else its name
    /// — is exactly `text`.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_have_text(self, text: &str) -> Result<Vec<UiNode>, DriverError> {
        let wanted = format!("have the text {text:?}");
        self.hold(
            self.query(),
            WaitCondition::Text(UiLocator::text_matcher(text, true)),
            &wanted,
        )
        .await
    }

    /// A matching node's text contains `part`.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_contain_text(self, part: &str) -> Result<Vec<UiNode>, DriverError> {
        let wanted = format!("contain the text {part:?}");
        self.hold(
            self.query(),
            WaitCondition::Text(UiLocator::text_matcher(part, false)),
            &wanted,
        )
        .await
    }

    /// A matching node is checked.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_checked(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(
            self.query().checked(true),
            WaitCondition::Attached,
            "be checked",
        )
        .await
    }

    /// A matching node is not checked.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_unchecked(self) -> Result<Vec<UiNode>, DriverError> {
        self.hold(
            self.query().checked(false),
            WaitCondition::Attached,
            "be unchecked",
        )
        .await
    }
}

/// An expectation on the things a world locator names.
#[derive(Debug, Clone)]
pub struct WorldExpect {
    /// The things.
    handle: WorldHandle,
    /// How long it may take to hold; the handle's wait when `None`.
    timeout: Option<Duration>,
}

impl WorldExpect {
    /// An expectation on `handle`'s things.
    pub(crate) const fn new(handle: WorldHandle) -> Self {
        Self {
            handle,
            timeout: None,
        }
    }

    /// Give it `timeout` to hold.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Wait for `condition`.
    async fn hold(
        self,
        condition: WorldWaitCondition,
        wanted: &str,
    ) -> Result<Vec<WorldNode>, DriverError> {
        let handle = match self.timeout {
            Some(timeout) => self.handle.timeout(timeout),
            None => self.handle,
        };
        let verb = format!("expect {} to {wanted}", handle.as_locator());
        handle.wait_for(condition, &verb).await
    }

    /// Something matches.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when it does not come to hold.
    pub async fn to_be_attached(self) -> Result<Vec<WorldNode>, DriverError> {
        self.hold(WorldWaitCondition::Attached, "be attached").await
    }

    /// Nothing matches.
    ///
    /// # Errors
    ///
    /// As [`to_be_attached`](Self::to_be_attached).
    pub async fn to_be_detached(self) -> Result<Vec<WorldNode>, DriverError> {
        self.hold(WorldWaitCondition::Detached, "be detached").await
    }
}

/// An expectation on a probe's readout, as JSON.
#[derive(Debug, Clone)]
pub struct StateExpect {
    /// The viewer.
    viewer: Viewer,
    /// The readout.
    probe: Probe,
    /// Where in it, as a JSON Pointer; the whole readout when empty.
    pointer: String,
    /// How long it may take to hold; the viewer's default when `None`.
    timeout: Option<Duration>,
}

impl StateExpect {
    /// An expectation on `probe`'s readout.
    pub(crate) const fn new(viewer: Viewer, probe: Probe) -> Self {
        Self {
            viewer,
            probe,
            pointer: String::new(),
            timeout: None,
        }
    }

    /// Look at the value at `pointer` (a JSON Pointer: `/region/name`)
    /// rather than the whole readout.
    #[must_use]
    pub fn at(mut self, pointer: &str) -> Self {
        pointer.clone_into(&mut self.pointer);
        self
    }

    /// Give it `timeout` to hold.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Wait for the value to pass `test`.
    async fn hold(self, test: ValueTest) -> Result<StateObservation, DriverError> {
        let timeout = self
            .timeout
            .unwrap_or_else(|| self.viewer.options().timeout);
        self.viewer
            .wait_for_state(
                StateCondition::Probe {
                    probe: self.probe,
                    pointer: self.pointer,
                    test,
                },
                timeout,
            )
            .await
    }

    /// The value is exactly `value`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when it does not come to hold.
    pub async fn to_equal(self, value: JsonValue) -> Result<StateObservation, DriverError> {
        self.hold(ValueTest::Equals(value)).await
    }

    /// The value includes `value`: a structural subset, a string's
    /// substring ([`sl_automation_proto::includes`]).
    ///
    /// # Errors
    ///
    /// As [`to_equal`](Self::to_equal).
    pub async fn to_include(self, value: JsonValue) -> Result<StateObservation, DriverError> {
        self.hold(ValueTest::Includes(value)).await
    }

    /// There is a value, not `null`.
    ///
    /// # Errors
    ///
    /// As [`to_equal`](Self::to_equal).
    pub async fn to_be_present(self) -> Result<StateObservation, DriverError> {
        self.hold(ValueTest::Present).await
    }

    /// There is no value, or `null`.
    ///
    /// # Errors
    ///
    /// As [`to_equal`](Self::to_equal).
    pub async fn to_be_absent(self) -> Result<StateObservation, DriverError> {
        self.hold(ValueTest::Absent).await
    }
}

/// An expectation on the chat and instant-message transcripts: a line in any
/// conversation.
#[derive(Debug, Clone)]
pub struct ChatExpect {
    /// The viewer.
    viewer: Viewer,
    /// Only lines whose speaker's name contains this.
    speaker: Option<String>,
    /// How long it may take to hold; the viewer's default when `None`.
    timeout: Option<Duration>,
}

impl ChatExpect {
    /// An expectation on `viewer`'s transcripts.
    pub(crate) const fn new(viewer: Viewer) -> Self {
        Self {
            viewer,
            speaker: None,
            timeout: None,
        }
    }

    /// Only lines spoken by someone whose name contains `speaker`.
    #[must_use]
    pub fn from(mut self, speaker: &str) -> Self {
        self.speaker = Some(speaker.to_owned());
        self
    }

    /// Give it `timeout` to hold.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Some conversation has a line containing `text`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when none comes.
    pub async fn to_contain(self, text: &str) -> Result<StateObservation, DriverError> {
        let mut line = json!({ "text": text });
        if let (Some(speaker), Some(fields)) = (&self.speaker, line.as_object_mut()) {
            let _previous = fields.insert("speaker".to_owned(), json!(speaker));
        }
        let expect = StateExpect::new(self.viewer, Probe::Conversations);
        let expect = match self.timeout {
            Some(timeout) => expect.timeout(timeout),
            None => expect,
        };
        expect.to_include(json!([{ "lines": [line] }])).await
    }
}

/// An expectation on the notifications the viewer raised.
#[derive(Debug, Clone)]
pub struct NotificationExpect {
    /// The viewer.
    viewer: Viewer,
    /// How long it may take to hold; the viewer's default when `None`.
    timeout: Option<Duration>,
}

impl NotificationExpect {
    /// An expectation on `viewer`'s notifications.
    pub(crate) const fn new(viewer: Viewer) -> Self {
        Self {
            viewer,
            timeout: None,
        }
    }

    /// Give it `timeout` to hold.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Some notification's text contains `text`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when none comes.
    pub async fn to_contain(self, text: &str) -> Result<StateObservation, DriverError> {
        let expect = StateExpect::new(self.viewer, Probe::Notifications);
        let expect = match self.timeout {
            Some(timeout) => expect.timeout(timeout),
            None => expect,
        };
        expect.to_include(json!([{ "text": text }])).await
    }

    /// Some notification raised from the catalogue template `template` is
    /// on screen.
    ///
    /// # Errors
    ///
    /// As [`to_contain`](Self::to_contain).
    pub async fn to_show(self, template: &str) -> Result<StateObservation, DriverError> {
        let expect = StateExpect::new(self.viewer, Probe::Notifications);
        let expect = match self.timeout {
            Some(timeout) => expect.timeout(timeout),
            None => expect,
        };
        expect
            .to_include(json!([{ "template": template, "live": true }]))
            .await
    }
}
