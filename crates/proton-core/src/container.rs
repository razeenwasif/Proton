//! Wire types and projection from `proton_container::Manager` into [`Event::Container`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ContainerPhase {
    Absent,
    Detected,
    Building,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfigView {
    pub path: String,
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ForwardedPortView {
    pub container_port: u16,
    pub host_port: u16,
    pub label: Option<String>,
    pub protocol: String,
    pub url: String,
    pub auto_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ContainerTemplateView {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ContainerView {
    pub phase: ContainerPhase,
    pub name: Option<String>,
    pub config_path: Option<String>,
    pub configs: Vec<ContainerConfigView>,
    pub image: Option<String>,
    pub workspace_folder: Option<String>,
    pub remote_user: Option<String>,
    pub runtime: Option<String>,
    pub container_id: Option<String>,
    pub step_note: String,
    pub ports: Vec<ForwardedPortView>,
    pub error: Option<String>,
    pub logs: Vec<String>,
    pub suggested_template: String,
    pub templates: Vec<ContainerTemplateView>,
}

/// Convert the core's `Manager` state into the wire view emitted on the bus.
pub fn view(mgr: &proton_container::Manager) -> ContainerView {
    let phase = match mgr.phase() {
        proton_container::ContainerPhase::Absent => ContainerPhase::Absent,
        proton_container::ContainerPhase::Detected => ContainerPhase::Detected,
        proton_container::ContainerPhase::Building => ContainerPhase::Building,
        proton_container::ContainerPhase::Starting => ContainerPhase::Starting,
        proton_container::ContainerPhase::Running => ContainerPhase::Running,
        proton_container::ContainerPhase::Stopping => ContainerPhase::Stopping,
        proton_container::ContainerPhase::Error => ContainerPhase::Error,
    };

    let selected = mgr.selected_config();
    let running = mgr.running();

    let configs = mgr
        .configs()
        .iter()
        .map(|c| ContainerConfigView {
            path: c.rel_path.clone(),
            name: c.name.clone(),
            source: c.source_label.clone(),
        })
        .collect();

    let ports = running
        .map(|r| {
            r.ports
                .iter()
                .map(|p| {
                    let scheme = if p.protocol.eq_ignore_ascii_case("https") {
                        "https"
                    } else {
                        "http"
                    };
                    ForwardedPortView {
                        container_port: p.container_port,
                        host_port: p.host_port,
                        label: p.label.clone(),
                        protocol: p.protocol.clone(),
                        url: format!("{scheme}://localhost:{}", p.host_port),
                        auto_detected: p.auto_detected,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let templates = proton_container::TEMPLATES
        .iter()
        .map(|t| ContainerTemplateView {
            id: t.id.to_string(),
            label: t.label.to_string(),
            description: t.description.to_string(),
        })
        .collect();

    ContainerView {
        phase,
        name: selected.map(|c| c.name.clone()),
        config_path: selected.map(|c| c.rel_path.clone()),
        configs,
        image: running
            .map(|r| r.image.clone())
            .or_else(|| selected.map(|c| c.source_label.clone())),
        workspace_folder: running.map(|r| r.exec_target.workdir.clone()),
        remote_user: running.and_then(|r| r.exec_target.user.clone()),
        runtime: running.map(|r| r.runtime_kind.clone()),
        container_id: running.map(|r| r.container_id.chars().take(12).collect()),
        step_note: mgr.step_note().to_string(),
        ports,
        error: mgr.error().map(str::to_string),
        logs: mgr.logs(),
        suggested_template: proton_container::suggest_template(mgr.root()).to_string(),
        templates,
    }
}
