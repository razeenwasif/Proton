//! Wire types shared between Proton's SSH remote server and local desktop shell.

use proton_core::Command;
use serde::{Deserialize, Serialize};

use crate::transport::EventBatch;

/// Requests sent from the local desktop shell to `proton-shell --remote-server`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClientMsg {
    Dispatch { id: u64, command: Command },
    ReadDocument { id: u64, path: String },
    ReadLab { id: u64, key: String },
    SaveUnsaved { id: u64 },
    Shutdown { id: u64 },
}

/// Responses and asynchronous event pushes sent from the remote server to the
/// local desktop shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerMsg {
    Ready {
        host: String,
        home: String,
        root: String,
    },
    Reply {
        id: u64,
        batch: EventBatch,
    },
    Push {
        batch: EventBatch,
    },
    Bytes {
        id: u64,
        ok: bool,
        data: Option<String>,
        error: Option<String>,
    },
    Unsaved {
        id: u64,
        failed: Vec<String>,
    },
    Ack {
        id: u64,
    },
}

/// Active SSH remote connection status shown in the titlebar, statusline, and
/// Connect-to-Host dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub connected: bool,
    pub host: Option<String>,
    pub folder: Option<String>,
    pub remote_hostname: Option<String>,
    pub remote_home: Option<String>,
    pub error: Option<String>,
    pub forwarded_ports: Vec<ForwardedTunnel>,
}

/// Host <-> remote TCP port tunnel managed by the SSH ControlMaster socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedTunnel {
    pub local_port: u16,
    pub remote_port: u16,
    pub auto: bool,
}

/// Host entry parsed from `~/.ssh/config`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostEntry {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
}

/// Recently opened `(host, folder)` pair persisted under Proton's config dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRemote {
    pub host: String,
    pub folder: String,
}

/// Combined SSH host discovery + recent targets returned to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCatalog {
    pub hosts: Vec<SshHostEntry>,
    pub recents: Vec<RecentRemote>,
}

/// Directory entry discovered on a remote host when browsing repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirEntry {
    pub name: String,
    pub path: String,
    pub is_repo: bool,
    pub has_devcontainer: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_protocol_messages_round_trip() {
        let req = ClientMsg::Dispatch {
            id: 42,
            command: Command::Sync,
        };
        let line = serde_json::to_string(&req).unwrap();
        let decoded: ClientMsg = serde_json::from_str(&line).unwrap();
        match decoded {
            ClientMsg::Dispatch { id, command } => {
                assert_eq!(id, 42);
                assert!(matches!(command, Command::Sync));
            }
            other => panic!("unexpected {other:?}"),
        }

        let ready = ServerMsg::Ready {
            host: "Kagutsuchi-2.local".into(),
            home: "/Users/razeenwasif".into(),
            root: "/Users/razeenwasif/Github/Applications/Proton".into(),
        };
        let rline = serde_json::to_string(&ready).unwrap();
        let rdecoded: ServerMsg = serde_json::from_str(&rline).unwrap();
        assert!(matches!(rdecoded, ServerMsg::Ready { .. }));
    }
}
