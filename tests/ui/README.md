# UI regression checks

Run `node ui/serve.mjs`, then open `http://localhost:5178/tests/ui/index.html`.
The page loads the production UI in an iframe with an in-memory native
transport. It tests command wiring, modal focus/input isolation, hidden-pane
measurement, resize handling, metadata, and corner styling without reading or
writing a real workspace. Results appear at the top of the page.

The second batch adds panel collapse/restoration, narrow and wide layouts,
all eight modes, dock shortcuts, keyboard resizing, display-setting persistence,
font-size viewport measurement, and default restoration on workspace changes.
The harness temporarily resets browser panel preferences and restores them
after its run. Settings writes use an in-memory transport.

The third batch adds tree roles and hierarchy, roving focus, arrow navigation,
focus recovery after collapsing a parent, type-to-find, and opening a file
without leaking keys into Vim. `npm ci && npm run check` separately checks the
file-tree and display-settings modules against the Rust-generated protocol.
The negative assertions in `protocol.test.ts` ensure invalid commands fail the
check. This is an incremental boundary; see the eighth batch below for its
current coverage.

The fourth batch adds editor Undo/Redo shortcuts, pane-targeted toolbar actions,
native input/picker history isolation, Codex-only controls, and Windows/Linux
shortcut mapping. The history-controls module is also type-checked. Browser
checks verify transport wiring; Rust editing/text tests verify actual undo
grouping, cursor/save boundaries, replay, and saved-revision integrity.

The fifth batch uses the native sequence-envelope format in the fixture. It
checks delayed and duplicate events, rapid file/pane changes, grouped focus/edit
gestures, resizing both panes, unchanged DOM preservation, and read-only startup
appearance caching. `npm run test:transport` runs the same standalone ordering
checks in Node, including startup and missing-push recovery. The transport and
editor-state modules are included in the strict type-check boundary.

The browser page reports a repeatable 60-line, 40-edit rendering comparison:
separate viewport/cursor delivery versus one transaction. This measures browser
render work, not native typing latency. Paint/row counts are asserted; elapsed
time is reported without a brittle machine-specific threshold.

For native timing, call `protonPerformance.start()` in the web inspector, type
in a disposable buffer, then read `protonPerformance.stop()`. It retains at most
200 samples with command kind, queue time, IPC reply time, ordered delivery and
render time, and time until the next animation-frame callback. The last measure
is a paint-opportunity estimate, not physical display latency; hidden windows
may defer it. No keys, source text, or paths are recorded. Sampling is off by
default, and nothing is persisted or sent anywhere.

Native history check: type several characters in the disposable workspace,
undo and redo the whole run using Command+Z / Shift+Command+Z on macOS, then
the pane buttons and Normal-mode `u` / Ctrl+R. Undo the fixture edits afterward.

The sixth batch checks buffer-tab semantics and manual activation, safe close
and focus recovery, keyboard editor-split resizing, last-pane close protection,
delayed file-loading feedback, failed-open preservation, and empty-buffer guidance.
The tab, split-resize, and editor-feedback modules are strictly type-checked.
Native checks should include arrows/Enter/Delete/Escape on tabs, refusal to close
an unsaved buffer, divider arrows/Shift/Home/Escape, and focus after pane closure.

The seventh batch adds a 100,000-line browser document with bounded DOM rows
and rapid project switches with independent scroll positions. Ordering tests
also cover cancellation of queued typing after a failed project switch and a
reply that never arrives. Run the real-file checks with
`cargo test -p proton-core --test reliability --test layout --test workspaces`.
The reliability test reports opening, distant scrolling, and 20 edits in a
50,000-line Rust file; compare optimized builds with `--release -- --nocapture`.
These are synthetic core timings without a language server or Git repository,
not physical display latency. Syntax-budget behavior is separately checked by
`cargo test -p proton-syntax --test highlight`.

For native restoration checks, move the cursor without making a structural
change, quit normally, and verify the position after relaunch. Repeat with a
scratch pane next to a file, switch projects rapidly, and temporarily move a
disposable fixture file to verify a failed open preserves the current buffer.
Restore the fixture and undo any edits after testing.

The eighth batch separates native IPC from browser demo fixtures and extracts
text rows with explicit pane/diagnostic inputs. Native bridge, text rows, and
HTML helpers are strictly checked; `app.js` and the legacy demo implementation
are not yet fully checked. The browser harness verifies that native startup
does not load demo fixtures and that backward Unicode selection remains visible.
Six row tests cover selection direction/shapes, pane isolation, character
columns, diagnostic precedence, escaping, and merged long runs.

Run the automated checks with:

```sh
npm ci --ignore-scripts
npm run check
npm run test:unit
npx playwright install chromium
npm run test:browser
```

The browser runner owns a loopback server on port 5180 and uses isolated browser
contexts. It runs all 58 harness checks and a standalone-preview smoke test,
fails on incomplete runs, failed assertions, or uncaught runtime errors, and
also rejects console errors in the standalone preview. Screenshots and traces
are retained on failure in `test-results/`. CI runs Chromium on Linux, macOS,
and Windows, installs browser dependencies with `--with-deps`, and uploads
failure reports for seven days. This covers browser behavior on those operating
systems, not native Tauri webviews. `npm run test:transport` remains available
for the nine transport-only checks.

Codex coverage verifies provider/model switching and display labels. Adapter
unit tests cover cancellation, disconnects, missing login, and rejecting model
substitution. The opt-in live connection test is
`cargo test -p proton-agent --test codex_live -- --ignored --nocapture`; it uses
the signed-in Codex account, calls one synthetic hosted tool, and verifies the
streamed answer. It is excluded from ordinary CI.

For native verification, build `cargo build -p proton-shell` and run the binary
with a disposable workspace and a separate `XDG_CONFIG_HOME`. Check mouse
search, query typing/paste, Escape and scrim dismissal, Settings Tab navigation,
Codex → Data → Codex, and rounded corners in both ordinary and maximized windows.
Also check Settings → Appearance at 125% interface text and 16 px editor text,
collapse a panel, and restart to verify those choices survive. These display
settings are saved in the disposable project's `.proton/settings.json`; panel
widths and visibility are local preferences per mode.

`tests/ui/terminal-bench.html` streams a coloured build log through the
terminal a line a frame — every row changing every time — once drawn as text
and once by the GPU, and reports the main thread's time per frame and the
frames that came late. Drivers can call `window.bench` themselves: Chrome's
profiler and a WKWebView with the web and GPU processes' CPU time both agree
with what the page reports. Nothing in it is asserted; the suite's GPU check
is that the canvas draws the right colours where it should, that the rows
under it catch up once the screen is still, and that switching back leaves
nothing behind.

The Research checks push the core's research events into the page and read
what it asks back: making an investigation (a refusal keeps what was typed,
a redraw keeps the caret), the board and a hypothesis drawn whole, a round
started, followed call by call, paused, resumed, stopped and held by another
Proton, a call's transcript drawn only for that call, what the scientist tells
the team and `/h` hypotheses of their own, and the table. The Lab checks
cover its Python's two say-sos, a proposal read and run or turned down (and
said to be unsandboxed where there is no sandbox), a run opened whole, its
pictures read through `read_lab`, a scene drawn or — without WebGL 2 — said
not to be, and the switch that lets proposals run unasked offered only with a
sandbox. `npm run test:unit` checks what the composer reads as a hypothesis,
sizes, and the 3D view's geometry. The rounds, the runs and the sandbox are
exercised in Rust: `cargo test -p proton-core --lib research::` plays rounds on
scripted engines, and `cargo test -p proton-lab` drives a fake conda, real runs
and, on macOS, the Seatbelt fence.
