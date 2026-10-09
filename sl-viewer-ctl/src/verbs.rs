//! Running one verb against a viewer, and printing what it answered.

use std::io::Write;

use sl_automation_proto::{InventoryFolderReadout, InventoryRoot, NameMatcher, WaitCondition};
use sl_viewer_driver::Viewer;

use crate::cli::{Verb, WorldVerb};
use crate::error::CtlError;
use crate::output::{Outcome, Printer};

/// Run `verb` on `viewer`, printing its results through `printer`.
///
/// `events --follow` prints until the connection closes or `stop` resolves
/// (the command line's Ctrl-C).
///
/// # Errors
///
/// [`CtlError::Driver`] when the viewer refuses or fails the request — the
/// error names the action, the viewer's reason and any artifacts saved — and
/// [`CtlError::Output`] when printing fails.
pub(crate) async fn run<W: Write>(
    viewer: &Viewer,
    verb: &Verb,
    printer: &mut Printer<W>,
    stop: impl Future<Output = ()>,
) -> Result<(), CtlError> {
    let ui = viewer.ui();
    let outcome = match verb {
        Verb::Tree { selector, depth } => {
            let roots = match selector {
                Some(selector) => vec![ui.locator(selector.clone()).node().await?],
                None => viewer.snapshot().await?,
            };
            Outcome::Tree {
                roots,
                depth: *depth,
            }
        }
        Verb::Find { selector } => Outcome::Nodes(ui.locator(selector.clone()).nodes().await?),
        Verb::Click {
            selector,
            right,
            double,
        } => {
            let target = ui.locator(selector.clone());
            let (verb, node) = if *right {
                ("right-clicked", target.right_click().await?)
            } else if *double {
                ("double-clicked", target.double_click().await?)
            } else {
                ("clicked", target.click().await?)
            };
            Outcome::Acted { verb, node }
        }
        Verb::Drag { selector, onto, by } => {
            let source = ui.locator(selector.clone());
            match (onto, by) {
                (Some(target), None) => Outcome::Acted {
                    verb: "dropped onto",
                    node: source.drag_to(&ui.locator(target.clone())).await?,
                },
                (None, Some([x, y])) => Outcome::Acted {
                    verb: "dragged",
                    node: source.drag_by(*x, *y).await?,
                },
                _ => return Err(CtlError::Usage("drag takes exactly one of --onto and --by")),
            }
        }
        Verb::Fill { selector, text } => Outcome::Acted {
            verb: "filled",
            node: ui.locator(selector.clone()).fill(text).await?,
        },
        Verb::Press { keys, on, hold } => {
            match (on, hold) {
                (Some(field), None) => ui.locator(field.clone()).press(keys).await?,
                (None, None) => viewer.press(keys).await?,
                (on, Some(frames)) => {
                    if let Some(field) = on {
                        let _focused = ui.locator(field.clone()).click().await?;
                    }
                    viewer.hold(keys, *frames).await?;
                }
            }
            Outcome::Pressed(keys.clone())
        }
        Verb::Wait {
            selector,
            condition,
        } => {
            let expect = viewer.expect(&ui.locator(selector.clone()));
            let nodes = match condition {
                WaitCondition::Attached => expect.to_be_attached().await?,
                WaitCondition::Detached => expect.to_be_detached().await?,
                WaitCondition::Visible => expect.to_be_visible().await?,
                WaitCondition::Hidden => expect.to_be_hidden().await?,
                WaitCondition::Enabled => expect.to_be_enabled().await?,
                WaitCondition::Disabled => expect.to_be_disabled().await?,
                WaitCondition::Text(NameMatcher::Exact(text)) => expect.to_have_text(text).await?,
                WaitCondition::Text(NameMatcher::Contains(part)) => {
                    expect.to_contain_text(part).await?
                }
            };
            Outcome::Nodes(nodes)
        }
        Verb::Open { floater } => {
            let window = viewer.open_floater(floater).await?;
            Outcome::Acted {
                verb: "opened",
                node: window.node().await?,
            }
        }
        Verb::Menu { path } => {
            let keys: Vec<&str> = path.iter().map(String::as_str).collect();
            Outcome::Acted {
                verb: "clicked",
                node: viewer.menu_path(&keys).await?,
            }
        }
        Verb::World(WorldVerb::Find { selector }) => {
            Outcome::WorldNodes(viewer.world().locator(selector.clone()).nodes().await?)
        }
        Verb::World(WorldVerb::Touch {
            selector,
            no_reveal,
        }) => {
            let mut target = viewer.world().locator(selector.clone());
            if *no_reveal {
                target = target.without_reveal();
            }
            Outcome::WorldActed {
                verb: "touched",
                node: target.touch().await?,
            }
        }
        Verb::World(WorldVerb::DoubleClick {
            selector,
            no_reveal,
        }) => {
            let mut target = viewer.world().locator(selector.clone());
            if *no_reveal {
                target = target.without_reveal();
            }
            Outcome::WorldActed {
                verb: "double-clicked",
                node: target.double_click().await?,
            }
        }
        Verb::World(WorldVerb::GroundDoubleClick {
            region,
            at: [x, y],
            no_reveal,
        }) => {
            let mut target = viewer.world().ground(region, *x, *y);
            if *no_reveal {
                target = target.without_reveal();
            }
            let hit_point = target.double_click().await?;
            Outcome::GroundActed {
                verb: "double-clicked",
                ground: target.as_ground().clone(),
                hit_point,
            }
        }
        Verb::Chat => Outcome::Conversations(viewer.conversations().await?),
        Verb::Notifications => Outcome::Notifications(viewer.notifications().await?),
        Verb::Agent => Outcome::Agent(viewer.agent().await?),
        Verb::Inventory {
            library,
            wait_loaded,
        } => {
            let root = if *library {
                InventoryRoot::Library
            } else {
                InventoryRoot::Agent
            };
            Outcome::InventoryTree(settled_inventory_tree(viewer, root, *wait_loaded).await?)
        }
        Verb::Environment => Outcome::Environment(viewer.environment().await?),
        Verb::WorldMap => Outcome::WorldMap(viewer.world_map().await?),
        Verb::Setting { key, value } => {
            let value = match value {
                Some(text) => {
                    // A bare word is a string: `setting SkinName vintage`
                    // need not be quoted twice.
                    let wanted = serde_json::from_str(text)
                        .unwrap_or_else(|_not_json| serde_json::Value::String(text.clone()));
                    viewer.set_setting(key, wanted).await?
                }
                None => viewer.setting(key).await?,
            };
            Outcome::Setting {
                key: key.clone(),
                value,
            }
        }
        Verb::FileDialog { path, .. } => Outcome::FileDialog {
            answered: viewer.answer_file_dialog(path.as_deref()).await?,
            picked: path.clone(),
        },
        Verb::Screenshot { path, outline } => {
            Outcome::Screenshot(viewer.screenshot(path, outline.clone()).await?)
        }
        Verb::Events {
            follow,
            stream,
            from,
        } => {
            return if *follow {
                follow_events(viewer, stream, *from, printer, stop).await
            } else {
                let page = viewer.read_log(from.unwrap_or(0), stream).await?;
                if page.dropped > 0 {
                    printer.print(&Outcome::Dropped(page.dropped))?;
                }
                printer.print(&Outcome::Log(page.entries))?;
                Ok(())
            };
        }
    };
    printer.print(&outcome)?;
    Ok(())
}

/// How often [`settled_inventory_tree`] re-reads the tree while it waits.
const INVENTORY_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// The `root` inventory tree, read once — or, given `wait_loaded`, re-read
/// until every folder is loaded and two reads in a row agree (the background
/// fetch has finished), or that many seconds have passed; the last read is
/// returned either way, its unloaded folders saying how far it got.
async fn settled_inventory_tree(
    viewer: &Viewer,
    root: InventoryRoot,
    wait_loaded: Option<u64>,
) -> Result<Vec<InventoryFolderReadout>, CtlError> {
    let mut tree = viewer.inventory_tree(root).await?;
    let Some(seconds) = wait_loaded else {
        return Ok(tree);
    };
    let started = tokio::time::Instant::now();
    let budget = std::time::Duration::from_secs(seconds);
    loop {
        if started.elapsed() >= budget {
            return Ok(tree);
        }
        tokio::time::sleep(INVENTORY_POLL).await;
        let next = viewer.inventory_tree(root).await?;
        let settled = !next.is_empty() && next.iter().all(|folder| folder.loaded) && next == tree;
        tree = next;
        if settled {
            return Ok(tree);
        }
    }
}

/// Print the event log's entries as they are recorded — from `from` when
/// given, else from now — until the connection closes or `stop` resolves.
async fn follow_events<W: Write>(
    viewer: &Viewer,
    streams: &[sl_automation_proto::LogStream],
    from: Option<u64>,
    printer: &mut Printer<W>,
    stop: impl Future<Output = ()>,
) -> Result<(), CtlError> {
    let mut events = match from {
        Some(cursor) => viewer.subscribe_from(cursor, streams).await?,
        None => viewer.subscribe(streams).await?,
    };
    let mut stop = core::pin::pin!(stop);
    loop {
        tokio::select! {
            () = &mut stop => return Ok(()),
            page = events.next() => {
                let Some(page) = page else {
                    return Ok(());
                };
                if page.dropped > 0 {
                    printer.print(&Outcome::Dropped(page.dropped))?;
                }
                for entry in page.entries {
                    printer.print(&Outcome::Entry(entry))?;
                }
            }
        }
    }
}
