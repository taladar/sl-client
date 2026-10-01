//! UI locator handles: [`Ui`] to start from, and [`UiLocator`] — a
//! [`Locator`] bound to its viewer, built fluently and acted on.
//!
//! Every action waits in the viewer for its one node to be actionable —
//! attached, visible, in the viewport, stable, enabled, receiving events —
//! and plays real input; a locator matching several nodes is refused with
//! the candidates, never applied to the first. Every read waits for the node
//! to be attached, then reads it strictly the same way.

use std::time::Duration;

use sl_automation_proto::{
    Locator, NameMatcher, NodeState, NodeValue, NodeVisibility, PointerButton, RequestBody,
    ResponseBody, Role, UiNode, WaitCondition,
};

use crate::artifacts::Subject;
use crate::error::DriverError;
use crate::viewer::Viewer;

/// The UI of one viewer, to find nodes in: the start of every UI locator.
#[derive(Debug, Clone)]
pub struct Ui {
    /// The viewer.
    viewer: Viewer,
}

impl Ui {
    /// The UI of `viewer`.
    pub(crate) const fn new(viewer: Viewer) -> Self {
        Self { viewer }
    }

    /// The node `locator` names, anywhere.
    #[must_use]
    pub fn locator(&self, locator: Locator) -> UiLocator {
        UiLocator::new(self.viewer.clone(), locator)
    }

    /// The window of the floater whose stable id this is (`build`,
    /// `inventory`, `preferences`): its test id is `floater:<id>`.
    #[must_use]
    pub fn window(&self, floater: &str) -> UiLocator {
        self.locator(window(floater))
    }

    /// The node whose test id — the entity's `Name` — is `test_id`.
    #[must_use]
    pub fn test_id(&self, test_id: &str) -> UiLocator {
        self.locator(Locator::test_id(test_id))
    }

    /// The nodes of `role`.
    #[must_use]
    pub fn role(&self, role: Role) -> UiLocator {
        self.locator(Locator::role(role))
    }

    /// The button whose name was translated from the Fluent key `key`.
    #[must_use]
    pub fn button_key(&self, key: &str) -> UiLocator {
        self.locator(Locator::role(Role::Button).name_key(key))
    }

    /// The button named `name` exactly.
    #[must_use]
    pub fn button(&self, name: &str) -> UiLocator {
        self.locator(Locator::role(Role::Button).named(name))
    }

    /// The nodes whose name was translated from the Fluent key `key`.
    #[must_use]
    pub fn key(&self, key: &str) -> UiLocator {
        self.locator(Locator::default().name_key(key))
    }
}

/// The locator of the window of the floater `floater`.
fn window(floater: &str) -> Locator {
    Locator {
        role: Some(Role::Window),
        ..Locator::test_id(format!("floater:{floater}"))
    }
}

/// A UI locator bound to its viewer: refine it, act on its one node, read
/// it, or expect something of it with [`Viewer::expect`].
#[derive(Debug, Clone)]
pub struct UiLocator {
    /// The viewer.
    viewer: Viewer,
    /// The query.
    locator: Locator,
    /// How long its actions and reads wait; the viewer's default when
    /// `None`.
    timeout: Option<Duration>,
}

impl UiLocator {
    /// `locator` on `viewer`.
    pub(crate) const fn new(viewer: Viewer, locator: Locator) -> Self {
        Self {
            viewer,
            locator,
            timeout: None,
        }
    }

    /// The query.
    #[must_use]
    pub const fn as_locator(&self) -> &Locator {
        &self.locator
    }

    /// The viewer.
    #[must_use]
    pub const fn viewer(&self) -> &Viewer {
        &self.viewer
    }

    /// How long its actions and reads wait.
    #[must_use]
    pub fn wait(&self) -> Duration {
        self.timeout
            .unwrap_or_else(|| self.viewer.options().timeout)
    }

    /// Wait `timeout` instead of the viewer's default.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The same query with its own criteria changed by `change`.
    fn refined(&self, change: impl FnOnce(Locator) -> Locator) -> Self {
        Self {
            viewer: self.viewer.clone(),
            locator: change(self.locator.clone()),
            timeout: self.timeout,
        }
    }

    /// `inner`, looked for inside the one node this names.
    #[must_use]
    pub fn get(&self, inner: Locator) -> Self {
        self.refined(|scope| inner.within(scope))
    }

    /// The node inside this one whose test id is `test_id`.
    #[must_use]
    pub fn test_id(&self, test_id: &str) -> Self {
        self.get(Locator::test_id(test_id))
    }

    /// The nodes of `role` inside this one.
    #[must_use]
    pub fn role(&self, role: Role) -> Self {
        self.get(Locator::role(role))
    }

    /// The button inside this one whose name was translated from `key`.
    #[must_use]
    pub fn button_key(&self, key: &str) -> Self {
        self.get(Locator::role(Role::Button).name_key(key))
    }

    /// The button inside this one named `name` exactly.
    #[must_use]
    pub fn button(&self, name: &str) -> Self {
        self.get(Locator::role(Role::Button).named(name))
    }

    /// The nodes inside this one whose name was translated from `key`.
    #[must_use]
    pub fn key(&self, key: &str) -> Self {
        self.get(Locator::default().name_key(key))
    }

    /// Only the nodes named `name` exactly.
    #[must_use]
    pub fn named(&self, name: &str) -> Self {
        self.refined(|locator| locator.named(name))
    }

    /// Only the nodes whose name contains `part`.
    #[must_use]
    pub fn name_containing(&self, part: &str) -> Self {
        self.refined(|locator| locator.name_containing(part))
    }

    /// Only the match at zero-based `index`, in reading order.
    #[must_use]
    pub fn nth(&self, index: u32) -> Self {
        self.refined(|locator| locator.nth(index))
    }

    /// A short description of an action on this locator, for errors.
    fn describe(&self, verb: &str) -> String {
        format!("{verb} {}", self.locator)
    }

    /// Carry out `body`, an action on this locator, and return the node it
    /// names.
    async fn act(&self, verb: &str, body: RequestBody) -> Result<UiNode, DriverError> {
        let action = self.describe(verb);
        match self
            .viewer
            .ask(body, self.wait(), &action, Subject::Ui(&self.locator))
            .await?
        {
            ResponseBody::Done { node } => Ok(node),
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// A click on the one node with `button`, twice for a double click.
    async fn click_with(
        &self,
        verb: &str,
        button: PointerButton,
        double: bool,
    ) -> Result<UiNode, DriverError> {
        self.act(
            verb,
            RequestBody::Click {
                locator: self.locator.clone(),
                button,
                double,
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Click the one node this names, once it is actionable.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when it matches several nodes, never becomes
    /// actionable, or the viewer cannot play input; the other
    /// [`DriverError`]s when the viewer does not answer.
    pub async fn click(&self) -> Result<UiNode, DriverError> {
        self.click_with("click", PointerButton::Left, false).await
    }

    /// Double-click the one node this names.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn double_click(&self) -> Result<UiNode, DriverError> {
        self.click_with("double-click", PointerButton::Left, true)
            .await
    }

    /// Right-click the one node this names.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn right_click(&self) -> Result<UiNode, DriverError> {
        self.click_with("right-click", PointerButton::Right, false)
            .await
    }

    /// Rest the pointer on the one node this names; a disabled node may be
    /// hovered.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn hover(&self) -> Result<UiNode, DriverError> {
        self.act(
            "hover",
            RequestBody::Hover {
                locator: self.locator.clone(),
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Replace the text of the one text field this names by typing: click
    /// into it, select all, delete, type. Answers once it holds `text`.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click); also when the node is not an editable
    /// field or does not hold the text afterwards.
    pub async fn fill(&self, text: &str) -> Result<UiNode, DriverError> {
        self.act(
            "fill",
            RequestBody::Fill {
                locator: self.locator.clone(),
                text: text.to_owned(),
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Click into the one node this names, then press `keys` — `Enter`,
    /// `Ctrl+A` — on it. The click is what gives it the focus, so this is for
    /// fields, not for buttons a click would press.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), and for keys the viewer cannot parse.
    pub async fn press(&self, keys: &str) -> Result<(), DriverError> {
        let _node = self.click().await?;
        self.viewer.press(keys).await
    }

    /// Check the one checkbox this names: click it unless it is checked, and
    /// wait for it to be.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), and when it does not come to be checked.
    pub async fn check(&self) -> Result<(), DriverError> {
        self.set_checked(true).await
    }

    /// Uncheck the one checkbox this names: click it if it is checked, and
    /// wait for it not to be.
    ///
    /// # Errors
    ///
    /// As [`check`](Self::check).
    pub async fn uncheck(&self) -> Result<(), DriverError> {
        self.set_checked(false).await
    }

    /// Click the one node this names unless its checked state is `wanted`,
    /// then wait for it to be.
    async fn set_checked(&self, wanted: bool) -> Result<(), DriverError> {
        if self.node().await?.has_state(NodeState::Checked) == wanted {
            return Ok(());
        }
        let _node = self.click().await?;
        let verb = if wanted { "check" } else { "uncheck" };
        self.wait_for(
            &self.locator.clone().checked(wanted),
            WaitCondition::Attached,
            verb,
        )
        .await
        .map(drop)
    }

    /// Open the one combo box this names, unless it is open, and pick
    /// `option` — a locator looked for inside the combo.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), for the combo and for the option.
    pub async fn select_option(&self, option: Locator) -> Result<UiNode, DriverError> {
        let verb = format!("select {option} in");
        self.act(
            &verb,
            RequestBody::SelectOption {
                combo: self.locator.clone(),
                option,
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Drag the one node this names onto the one `target` names, with the
    /// left button; answers with the target.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), for the source and for the target.
    pub async fn drag_to(&self, target: &Self) -> Result<UiNode, DriverError> {
        let verb = format!("drag onto {} the node", target.locator);
        self.act(
            &verb,
            RequestBody::DragTo {
                source: self.locator.clone(),
                target: target.locator.clone(),
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Drag the one node this names by `x` logical pixels rightwards and `y`
    /// downwards, with the left button — a window by its title bar, or by its
    /// resize grip. Answers with the node as it was when pressed.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn drag_by(&self, x: f32, y: f32) -> Result<UiNode, DriverError> {
        let verb = format!("drag by ({x}, {y}) the node");
        self.act(
            &verb,
            RequestBody::DragBy {
                source: self.locator.clone(),
                offset: [x, y],
                deadline: Viewer::deadline(self.wait()),
            },
        )
        .await
    }

    /// Wait for `locator`'s matches to satisfy `condition` within this
    /// handle's wait, and return them.
    pub(crate) async fn wait_for(
        &self,
        locator: &Locator,
        condition: WaitCondition,
        verb: &str,
    ) -> Result<Vec<UiNode>, DriverError> {
        let action = format!("{verb}: wait for {locator} to be {condition}");
        match self
            .viewer
            .ask(
                RequestBody::WaitFor {
                    locator: locator.clone(),
                    condition,
                    deadline: Viewer::deadline(self.wait()),
                },
                self.wait(),
                &action,
                Subject::Ui(locator),
            )
            .await?
        {
            ResponseBody::Satisfied { nodes } => Ok(nodes),
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// The one node this names, with its children, once one is attached.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when none is attached within the wait, or
    /// several are.
    pub async fn node(&self) -> Result<UiNode, DriverError> {
        let _attached = self
            .wait_for(&self.locator, WaitCondition::Attached, "read")
            .await?;
        let action = self.describe("read");
        match self
            .viewer
            .ask(
                RequestBody::Snapshot {
                    within: Some(self.locator.clone()),
                },
                Duration::ZERO,
                &action,
                Subject::Ui(&self.locator),
            )
            .await?
        {
            ResponseBody::Snapshot { roots } => {
                let mut roots = roots.into_iter();
                match (roots.next(), roots.next()) {
                    (Some(node), None) => Ok(node),
                    (first, second) => Err(self.viewer.unexpected(
                        &action,
                        &ResponseBody::Snapshot {
                            roots: first.into_iter().chain(second).collect(),
                        },
                    )),
                }
            }
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// The text of the one node this names: a field's or a label's value,
    /// else its accessible name.
    ///
    /// # Errors
    ///
    /// As [`node`](Self::node).
    pub async fn text(&self) -> Result<Option<String>, DriverError> {
        let node = self.node().await?;
        Ok(match node.value {
            Some(NodeValue::Text(text)) => Some(text),
            Some(NodeValue::Number(_)) | None => node.name,
        })
    }

    /// The value of the one node this names: a field's text, a slider's
    /// number.
    ///
    /// # Errors
    ///
    /// As [`node`](Self::node).
    pub async fn value(&self) -> Result<Option<NodeValue>, DriverError> {
        Ok(self.node().await?.value)
    }

    /// Whether the one node this names is disabled, counting a disabled
    /// ancestor.
    ///
    /// # Errors
    ///
    /// As [`node`](Self::node).
    pub async fn is_disabled(&self) -> Result<bool, DriverError> {
        Ok(self.node().await?.has_state(NodeState::Disabled))
    }

    /// Whether the one node this names is checked.
    ///
    /// # Errors
    ///
    /// As [`node`](Self::node).
    pub async fn is_checked(&self) -> Result<bool, DriverError> {
        Ok(self.node().await?.has_state(NodeState::Checked))
    }

    /// Whether the one node this names can be seen (a node something covers
    /// counts: it is drawn and on screen).
    ///
    /// # Errors
    ///
    /// As [`node`](Self::node).
    pub async fn is_visible(&self) -> Result<bool, DriverError> {
        Ok(matches!(
            self.node().await?.visibility,
            NodeVisibility::Visible | NodeVisibility::Covered
        ))
    }

    /// Every node this names now, without waiting, without children.
    ///
    /// # Errors
    ///
    /// As any request; [`DriverError::Failed`] when a scope it names matches
    /// several nodes.
    pub async fn nodes(&self) -> Result<Vec<UiNode>, DriverError> {
        let action = self.describe("find");
        match self
            .viewer
            .ask(
                RequestBody::Find {
                    locator: self.locator.clone(),
                },
                Duration::ZERO,
                &action,
                Subject::Ui(&self.locator),
            )
            .await?
        {
            ResponseBody::Found { nodes } => Ok(nodes),
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// How many nodes this names now.
    ///
    /// # Errors
    ///
    /// As [`nodes`](Self::nodes).
    pub async fn count(&self) -> Result<usize, DriverError> {
        Ok(self.nodes().await?.len())
    }

    /// A handle on each node this names now, each picked by its index.
    ///
    /// # Errors
    ///
    /// As [`nodes`](Self::nodes).
    pub async fn all(&self) -> Result<Vec<Self>, DriverError> {
        let count = self.count().await?;
        Ok((0..u32::try_from(count).unwrap_or(u32::MAX))
            .map(|index| self.nth(index))
            .collect())
    }

    /// The text matcher an expectation compares against.
    pub(crate) fn text_matcher(text: &str, exact: bool) -> NameMatcher {
        if exact {
            NameMatcher::Exact(text.to_owned())
        } else {
            NameMatcher::Contains(text.to_owned())
        }
    }
}
