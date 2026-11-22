# Remote-SSH in Proton

Proton lets you open and work directly on repositories stored on another machine — such as `mini` (`Kagutsuchi-2.local`) — from Proton on your Mac, without syncing files back and forth or mounting slow network filesystems.

## Architecture

Instead of rebuilding a partial remote file provider, Proton runs its own headless core server (`proton-shell --remote-server [folder]`) over `ssh -T` NDJSON stdio directly on the target host:

- **Native UI, remote core:** The Tauri WebView stays local at full GPU frame rate on your Mac, while `proton_core::Session` runs natively on `mini` against `mini`'s local APFS disk.
- **Everything works remotely:** File tree, fuzzy Telescope picker, ripgrep search, Git gutter & status, multi-pane splits, interactive PTY terminals (`zsh` on `mini`), language servers, debuggers, formatters, Data mode Jupyter kernels, Claude/Codex/Gemini agents, Research, Excel, Circuit, Logic, and CAD all execute on `mini`.
- **Automatic port forwarding:** SSH sessions use a short ControlMaster socket (`/tmp/pssh-<pid>-<seq>.sock`). When an attached Dev Container on `mini` exposes or auto-detects listening ports, Proton automatically runs `ssh -O forward -L 127.0.0.1:<port>:127.0.0.1:<port> <host>` and opens clicked links locally in your Mac's browser.

## Connecting to `mini`

### From the UI

- Press **`⌘⇧O`** (or choose **File ▸ Connect to Host (SSH)…**), click the **Remote SSH** icon in the titlebar, or click **`Local`** / **`SSH: ...`** in the bottom statusline.
- Pick **`mini`** (discovered automatically from `~/.ssh/config`), click **Browse repos** to see repositories under `~/Github` and `~/Github/Applications`, and click **Open in New Window** (or **Switch This Window**).

### From Ex Commands (`:` in Normal mode)

- `:ssh` / `:remote` — open the Connect to Host (SSH) modal
- `:ssh mini` — connect this window to `mini` (`~/Github`)
- `:ssh mini ~/Github/Applications/Proton` or `:ssh mini:~/Github/Applications/Proton` — open a specific repository on `mini`
- `:ssh -w mini ~/Github/Applications/Graviton` — open in a new window
- `:ssh forward 8080[:3000]` — forward an extra local port over the active SSH tunnel
- `:ssh disconnect` / `:ssh local` — disconnect back to a local window

### From the CLI

```sh
/Applications/Proton.app/Contents/MacOS/proton-shell --ssh mini:~/Github/Applications/Proton
```

## Using Dev Containers on `mini`

Because the core runs directly on `mini`, opening a repo on `mini` that contains `.devcontainer/devcontainer.json` discovers and launches Dev Containers using `mini`'s own `docker` CLI and Graviton daemon (`~/Library/Application Support/Graviton/docker.sock`). Container ports forwarded on `mini` are automatically tunneled back to `127.0.0.1:<port>` on your Mac over the SSH ControlMaster connection.

## Verification

- `crates/proton-shell/src/remote_protocol.rs`: NDJSON `ClientMsg` / `ServerMsg` serialization round-trip tests
- `crates/proton-shell/src/remote_client.rs`: `~/.ssh/config` parser (`parse_ssh_config`) and host validation tests
- `crates/proton-shell/tests/remote_loopback.rs`: full process spawn of `proton-shell --remote-server` over stdio NDJSON (`Ready`, `Sync`, `Tree`, `OpenBuffer`, `SaveUnsaved`, `Shutdown`)
- `tests/ui/remote.test.mjs`: Ex command parsing, status summary, and modal HTML rendering unit tests
