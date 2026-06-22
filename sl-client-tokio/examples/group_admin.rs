//! Logs in to a Second Life / OpenSim grid and exercises the **group
//! management edits** surface (ROADMAP #31): it creates a throwaway group, then
//! runs a role create → list (find the new role) → update → delete cycle
//! (`GroupRoleUpdate`), posts a group notice (`IM_GROUP_NOTICE`), and — when a
//! second member is supplied — assigns that member to the new role
//! (`GroupRoleChanges`) and ejects them (`EjectGroupMemberRequest`).
//!
//! Needs the grid's Groups V2 module enabled (on OpenSim: `Module = "Groups
//! Module V2"` with a MySQL/MariaDB backend); otherwise no group replies arrive.
//!
//! Configure via environment variables:
//!   `SL_LOGIN_URI`  (default `http://127.0.0.1:9000/`)
//!   `SL_FIRST`, `SL_LAST`, `SL_PASSWORD`  (required)
//!   `SL_START`      (default `last`)
//!   `SL_CHANNEL`    (default `sl-client-tokio-group-admin`)
//!   `SL_VERSION`    (default this crate's version)
//!   `SL_HOLD_SECS`  (default `30`)
//!   `SL_MEMBER`     (optional agent UUID already in the group to assign a role
//!                    to and then eject; skipped if unset)

use std::time::Duration;

use sl_client_tokio::{
    AgentKey, Client, Command, CreateGroupParams, DisconnectReason, Error, Event, GroupRoleChange,
    GroupRoleEdit, GroupRoleMemberChange, GroupRoleUpdateType, LoginParams, LoginRequest, Throttle,
    Uuid, group_powers,
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

/// Reads an environment variable or returns the given default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_ignored| default.to_owned())
}

/// The name of the throwaway role the example creates, used to find its
/// server-assigned id in the `GroupRoleData` reply.
const ROLE_NAME: &str = "sl-client #31 role";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let login_uri = env_or("SL_LOGIN_URI", "http://127.0.0.1:9000/");
    let first = std::env::var("SL_FIRST")?;
    let last = std::env::var("SL_LAST")?;
    let password = std::env::var("SL_PASSWORD")?;
    let start = env_or("SL_START", "last").parse::<sl_client_tokio::StartLocation>()?;
    let channel = env_or("SL_CHANNEL", "sl-client-tokio-group-admin");
    let version = env_or("SL_VERSION", env!("CARGO_PKG_VERSION"));
    let hold_secs: u64 = env_or("SL_HOLD_SECS", "30").parse()?;
    let member: Option<Uuid> = match std::env::var("SL_MEMBER") {
        Ok(value) => Some(
            value
                .parse()
                .map_err(|_ignored| "SL_MEMBER is not a valid UUID")?,
        ),
        Err(_unset) => None,
    };

    info!("logging in...");
    let request = LoginRequest::new(first, last, password, start, channel, version);
    let params = LoginParams { login_uri, request };
    let client = match Client::connect(params).await {
        Ok(client) => client,
        Err(Error::MfaChallenge(_)) => {
            return Err("this probe does not support interactive MFA".into());
        }
        Err(other) => return Err(other.into()),
    };
    let agent_id = client.agent_id().ok_or("no agent id after login")?;
    info!("login succeeded; agent {agent_id}");

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, _diag_rx) = mpsc::channel(16);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    // A tiny state machine across the role CRUD cycle. We learn the group id from
    // CreateGroupResult and the server-assigned role id from GroupRoleData.
    let mut started = false;
    let mut group_id = None;
    let mut role_id = None;
    let mut role_updated = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::RegionHandshakeComplete | Event::RegionChanged { .. } if !started => {
                started = true;
                info!("region active; creating a throwaway group");
                command_tx
                    .send(Command::SetThrottle(Throttle::preset_1000()))
                    .await
                    .ok();
                command_tx
                    .send(Command::CreateGroup(CreateGroupParams {
                        name: format!("sl-client #31 {agent_id}"),
                        charter: "throwaway group for #31 testing".to_owned(),
                        show_in_list: false,
                        insignia_id: Uuid::nil(),
                        membership_fee: 0,
                        open_enrollment: true,
                        allow_publish: false,
                        mature_publish: false,
                    }))
                    .await
                    .ok();

                // Log out after the hold window regardless of progress.
                let command_tx = command_tx.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_secs(hold_secs)).await;
                    command_tx.send(Command::Logout).await.ok();
                });
            }
            Event::CreateGroupResult {
                group_id: created,
                success,
                message,
            } => {
                if !success {
                    warn!("group creation failed: {message}");
                    command_tx.send(Command::Logout).await.ok();
                    continue;
                }
                info!("group {created} created; creating role \"{ROLE_NAME}\"");
                group_id = Some(created);
                // Create a new role. OpenSim assigns its own role id, so we learn
                // the real id from the GroupRoleData reply, then update/delete it.
                command_tx
                    .send(Command::UpdateGroupRoles {
                        group_id: created,
                        roles: vec![GroupRoleEdit {
                            role_id: Uuid::new_v4().into(),
                            name: ROLE_NAME.to_owned(),
                            description: "created by #31".to_owned(),
                            title: "Tester".to_owned(),
                            powers: group_powers::MEMBER_INVITE | group_powers::NOTICES_SEND,
                            update_type: GroupRoleUpdateType::Create,
                        }],
                    })
                    .await
                    .ok();
                // Post a group notice (relayed back to us as a member).
                command_tx
                    .send(Command::SendGroupNotice {
                        group_id: created,
                        subject: "sl-client #31".to_owned(),
                        message: "group management edits work".to_owned(),
                        attachment: None,
                    })
                    .await
                    .ok();
                command_tx
                    .send(Command::RequestGroupRoles(created))
                    .await
                    .ok();
            }
            Event::GroupRoleData {
                role_count, roles, ..
            } => {
                info!(
                    "group has {} of {role_count} role(s) in this packet:",
                    roles.len()
                );
                for role in &roles {
                    info!(
                        "  role {} \"{}\" powers {:#x}",
                        role.role_id, role.name, role.powers
                    );
                }
                let Some(group) = group_id else { continue };
                // Fetch the role↔member pairings so we can observe TotalPairs (#42).
                command_tx
                    .send(Command::RequestGroupRoleMembers(group))
                    .await
                    .ok();
                if let Some(found) = roles.iter().find(|role| role.name == ROLE_NAME) {
                    if role_id.is_none() {
                        // First sighting: update the role's title/powers.
                        role_id = Some(found.role_id);
                        info!("found new role {}; updating it", found.role_id);
                        command_tx
                            .send(Command::UpdateGroupRoles {
                                group_id: group,
                                roles: vec![GroupRoleEdit {
                                    role_id: found.role_id,
                                    name: ROLE_NAME.to_owned(),
                                    description: "updated by #31".to_owned(),
                                    title: "Senior Tester".to_owned(),
                                    powers: group_powers::NOTICES_SEND,
                                    update_type: GroupRoleUpdateType::UpdateAll,
                                }],
                            })
                            .await
                            .ok();
                        // If a member id was supplied, assign them to this role.
                        if let Some(member_id) = member {
                            info!("assigning member {member_id} to the role");
                            command_tx
                                .send(Command::ChangeGroupRoleMembers {
                                    group_id: group,
                                    changes: vec![GroupRoleMemberChange {
                                        role_id: found.role_id,
                                        member_id: AgentKey::from(member_id),
                                        change: GroupRoleChange::Add,
                                    }],
                                })
                                .await
                                .ok();
                            info!("ejecting member {member_id} from the group");
                            command_tx
                                .send(Command::EjectGroupMembers {
                                    group_id: group,
                                    member_ids: vec![AgentKey::from(member_id)],
                                })
                                .await
                                .ok();
                        }
                        command_tx
                            .send(Command::RequestGroupRoles(group))
                            .await
                            .ok();
                    } else if !role_updated {
                        // Second sighting (post-update): delete the role.
                        role_updated = true;
                        info!("role now titled \"{}\"; deleting it", found.title);
                        command_tx
                            .send(Command::UpdateGroupRoles {
                                group_id: group,
                                roles: vec![GroupRoleEdit {
                                    role_id: found.role_id,
                                    name: String::new(),
                                    description: String::new(),
                                    title: String::new(),
                                    powers: 0,
                                    update_type: GroupRoleUpdateType::Delete,
                                }],
                            })
                            .await
                            .ok();
                        command_tx
                            .send(Command::RequestGroupRoles(group))
                            .await
                            .ok();
                    } else {
                        info!("role still present after delete (re-fetch may lag)");
                    }
                } else if role_updated {
                    info!("role \"{ROLE_NAME}\" is gone — delete confirmed");
                }
            }
            Event::GroupRoleMembers {
                total_pairs, pairs, ..
            } => {
                info!(
                    "group has {} of {total_pairs} role/member pairing(s) in this packet",
                    pairs.len()
                );
            }
            Event::EjectGroupMemberResult {
                group_id: g,
                success,
            } => {
                info!("eject from {g}: success={success}");
            }
            Event::InstantMessageReceived(im)
                if im.dialog == sl_client_tokio::ImDialog::GroupNotice =>
            {
                info!("received our group notice: \"{}\"", im.message);
            }
            Event::LoggedOut => {
                info!("logged out cleanly");
                break;
            }
            Event::Disconnected(reason) => {
                match reason {
                    DisconnectReason::Timeout => warn!("disconnected: inactivity timeout"),
                    other => warn!("disconnected: {other:?}"),
                }
                break;
            }
            _ignored => {}
        }
    }

    run.await??;
    Ok(())
}
