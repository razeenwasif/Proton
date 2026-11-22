# Dev Containers in Proton

Proton supports VS Code–style `devcontainer.json` development containers without requiring the host editor to live inside Docker. The Proton UI, rope buffers, tree-sitter syntax engine, and host files stay native on macOS, while interactive terminals, language servers, formatters, debuggers, and agent shell commands route transparently into the attached Linux container.

## Quick start

1. Open a project folder (`⌘O` or `:cd <folder>`).
2. Open the **Dev Container** dock panel from the left activity rail or titlebar container button.
3. If no `devcontainer.json` exists yet:
   - Choose a suggested template (`rust`, `python`, `node`, `cpp`, `go`, `base`) and click **Create `.devcontainer/devcontainer.json`**, or run `␣ki` / `:container init [template]`.
4. Click **Start & Attach** (or run `␣ku` / `:container start`).
5. Once attached:
   - Status line shows `Container: <name>` and the container badge appears in the terminal header.
   - New terminals open inside the container shell (`customizations.proton.shell` or `/bin/sh`).
   - Language servers (`proton-lsp`), formatters (`formatters::start_in_container`), debug adapters (`proton-dap`), and agent `run_command` executions run inside the container with automatic host ↔ container path translation.

## `devcontainer.json` discovery & configuration

Proton discovers configuration files (`crates/proton-container/src/config.rs`) in standard precedence order:

1. `.devcontainer/devcontainer.json`
2. `.devcontainer.json`
3. `.devcontainer/<subdir>/devcontainer.json` (sorted)
4. `.proton/devcontainer.json`

Files are parsed as JSONC (comments and trailing commas allowed). Supported fields include:

- **Container source:** `image`, `build` (`dockerfile` / `dockerFile`, `context`, `args`, `target`, `cacheFrom`), `dockerComposeFile` + `service` (+ optional `runServices`)
- **Features & security:** `features` (OCI registry `<registry>/<repo>[:<tag>]` and local `./` / `../` paths), `overrideFeatureInstallOrder`, `capAdd`, `securityOpt`, `privileged`, `init`
- **Workspace & environment:** `name`, `workspaceFolder`, `workspaceMount` (bind or `type=volume`), `mounts` (named volumes created automatically), `containerEnv`, `remoteEnv`, `remoteUser`, `containerUser`, `runArgs`, `overrideCommand`, `shutdownAction`
- **Ports:** `forwardPorts` (port number or `"host:container"`), dynamic `/proc/net/tcp{,6}` listening-port auto-detection, `portsAttributes` / `otherPortsAttributes` (`label`, `protocol`, `onAutoForward`)
- **Lifecycle hooks:** `initializeCommand`, `onCreateCommand`, `updateContentCommand`, `postCreateCommand`, `postStartCommand`, `postAttachCommand` (string, argv array, or named parallel/object map)
- **Proton customizations:** `customizations.proton.shell` and per-server overrides under `customizations.proton.lsp.<lang>`

## Container runtimes

`crates/proton-container/src/runtime.rs` auto-detects reachable container engines in order:

- `docker` (default context / Docker Desktop / OrbStack)
- `graviton` (`docker` CLI targeting `~/Library/Application Support/Graviton/docker.sock` with `DOCKER_BUILDKIT=0` injected automatically)
- `podman`
- `nerdctl`

Detection searches `PATH` plus common macOS locations (`/opt/homebrew/bin`, `/usr/local/bin`, Docker Desktop, OrbStack) so Dock-launched builds find your runtime without inheriting a custom shell profile. When `Auto` sees Docker Desktop stopped and Graviton's socket running, Proton automatically switches to Graviton and propagates `DOCKER_HOST` and `DOCKER_BUILDKIT=0` to build/hook steps, terminals, LSP servers, debuggers, formatters, and agent commands. You can also set an explicit engine in **Settings ▸ Containers** (`container.runtime`: `auto`, `graviton`, `docker`, `podman`, `nerdctl`; `container.autoAttach` to auto-attach when opening a project).

For tests and custom CLI wrappers, set `PROTON_CONTAINER_BIN=/path/to/executable`.

## Host ↔ container path mapping

The native macOS workspace root and the container's `workspaceFolder` are bridged by `proton_container::PathMap` (`crates/proton-container/src/pathmap.rs`):

- **LSP (`proton-lsp`):** Outgoing URIs/paths (open buffers, diagnostics, go-to-definition, references, hover, rename, code actions) are rewritten from host paths to container `file://` URIs, and incoming container URIs/edits are mapped back to host files (`Client::spawn_with_map`). Canonicalization differences such as macOS `/var` ↔ `/private/var` are handled automatically.
- **DAP (`proton-dap`):** `Client::launch_mapped` maps breakpoint source paths on the way into the container adapter and stack frames / breakpoint events on the way back out.

## Keyboard shortcuts & Ex commands

### Leader sequences (Normal mode)

| Keys | Action |
| --- | --- |
| `␣ku` | Start & attach Dev Container |
| `␣kr` | Rebuild Dev Container |
| `␣ks` | Stop Dev Container |
| `␣kd` | Detach without stopping |
| `␣ki` | Create `.devcontainer/devcontainer.json` from suggested template |

### Ex commands

- `:container` / `:container start` / `:container up` / `:container attach`
- `:container rebuild`
- `:container stop` / `:container down`
- `:container detach`
- `:container refresh`
- `:container clone <git-url> [branch]` — clones a repository into a deterministic named volume and creates `.devcontainer/devcontainer.json` if absent
- `:container init [rust|python|node|cpp|go|base]` (`:devcontainer` is an alias)

## Built-in templates

`crates/proton-container/src/templates.rs` provides six templates and detects a suggested default from project files (`Cargo.toml`, Python manifests, `package.json`, C/C++, `go.mod`, or fallback `base`):

- `rust` — `mcr.microsoft.com/devcontainers/rust:1-bookworm`
- `python` — `mcr.microsoft.com/devcontainers/python:3.12-bookworm`
- `node` — `mcr.microsoft.com/devcontainers/javascript-node:22-bookworm`
- `cpp` — `mcr.microsoft.com/devcontainers/cpp:bookworm`
- `go` — `mcr.microsoft.com/devcontainers/go:1.23-bookworm`
- `base` — `mcr.microsoft.com/devcontainers/base:bookworm`

## Verification & test architecture

- `crates/proton-container`: Unit tests for JSONC parsing, discovery, variable expansion, `PathMap`, templates, and an end-to-end mocked runtime lifecycle test (`PROTON_CONTAINER_BIN`).
- `crates/proton-term`, `crates/proton-lsp`, `crates/proton-dap`: Command builder, mapped stdio LSP translation, and DAP mapped breakpoint/stack translation tests.
- `crates/proton-core`: Workspace integration tests covering `Sync`, `:container init`, leader key bindings, and attach/detach state transitions.
- `tests/ui/protocol.test.ts` & `tests/ui/container.test.mjs`: TypeScript IPC wire contract checks and pure UI rendering unit tests across all lifecycle phases (`absent`, `detected`, `building`, `running`, `stopping`, `error`).
