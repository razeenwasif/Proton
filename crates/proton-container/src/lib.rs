//! Proton Dev Containers: `devcontainer.json` discovery and parsing,
//! container engine orchestration (Docker / Podman / nerdctl), lifecycle hooks,
//! and host ↔ container path mapping.

pub mod config;
pub mod features;
pub mod jsonc;
pub mod manager;
pub mod pathmap;
pub mod ports;
pub mod runtime;
pub mod templates;
pub mod volume;

pub use config::{
    discover, BuildConfig, ContainerSource, DevContainerConfig, LifecycleCommand, LspOverride,
    MountSpec, PortAttributes, PortSpec, ResolvedConfig, ShutdownAction, SingleCommand,
};
pub use manager::{
    ConfigSummary, ContainerPhase, ForwardedPort, Manager, PollOutcome, RunningContainer,
    Transition,
};
pub use pathmap::PathMap;
pub use runtime::{graviton_socket_path, EngineKind, ExecTarget, Runtime};
pub use templates::{suggest as suggest_template, write as write_template, Template, TEMPLATES};
