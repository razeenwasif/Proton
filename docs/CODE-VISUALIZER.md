# Integrating Code Visualizer

Upstream: <https://github.com/razeenwasif/Code-Visualizer>

Step through a program one line at a time and watch its memory — stack frames,
heap blocks, and a real arrow for every pointer. C, C++, Python and TypeScript
work today; Rust is planned behind the same trace format.

The good news is that this integration is mostly *already designed*. The
upstream architecture is one seam:

```
  C  ──► backends/c/    ─┐
 C++ ──► backends/cpp/   │
 Py  ──► backends/python/├──►  trace JSON  ──►  web/  (the only frontend)
 TS  ──► backends/ts/    │
Rust ──► backends/rust/ ─┘
```

Backends emit [one JSON document](https://github.com/razeenwasif/Code-Visualizer/blob/main/docs/trace-format.md);
the frontend contains no language-specific code. The README says so explicitly:
*"Swapping in a TUI later means writing one consumer of the same JSON, not
rewriting anything."* **Proton is that second consumer.** Nothing here is a
fork or a patch to upstream's internals — it uses the extension point the
project was built around.

---

## Three stages

### Stage 1 — sidecar (a day)

Vendor the repo at `vendor/code-visualizer` (git submodule or subtree), spawn
`server/app.py` on a loopback port as a Proton child process, and render
`web/index.html` in the Visualizer dock as a webview.

- Zero changes to either codebase.
- Immediately gives Proton every language upstream supports.
- Costs: two visual languages in one window, and Proton's keymap fights the
  embedded page's.

Worth doing first anyway, because it proves the process supervision, the port
handshake and the toolchain detection before any UI work.

### Stage 2 — native pane (the real integration)

Drop the embedded HTML. Proton calls the same HTTP API, takes the trace JSON,
and renders it in Proton's own design language — which is what the prototype's
Visualizer tab shows.

```
Proton editor ──► POST /api/session/start {language, source, stdin}
                      │
                      ├─ GET /api/session/poll   → steps as they arrive
                      ├─ POST /api/session/input → interactive stdin
                      └─ POST /api/session/stop
                      ▼
                 trace JSON ──► Proton Visualizer pane
```

The API surface is small and already stable:

| Endpoint | Use |
|---|---|
| `GET /api/languages` | Which languages this build implements |
| `GET /api/toolchain` | Which are *usable here*, with a reason and a hint when not — drives the **Targets** list in the explorer |
| `GET /api/examples` | Sample programs for the empty state |
| `POST /api/session/start` | Trace a program |
| `POST /api/session/poll` | Stream steps during a live run |
| `POST /api/session/input` / `/eof` | Interactive stdin |
| `POST /api/session/stop` | Cancel |

Two properties of the format make the native pane cheap to build:

- **A pointer is an edge, never an inline expansion.** Every object is drawn
  once; a pointer carries only a target address, and the renderer resolves it —
  exact match first, then the smallest containing range, so `&node->next` and
  `&arr[2]` land on the precise field or cell. This is why linked lists
  terminate instead of recursing, and it is the one rule the Proton pane must
  not break. The prototype implements it literally: pointer slots carry
  `data-ptr`, blocks carry `data-addr`, and edges are measured between the two
  live boxes.
- **Partial traces are normal.** `status` may be `timeout`, `step_limit` or
  `runtime_error` and still carry useful steps. Proton renders whatever it gets
  and surfaces `error` in the problem list rather than showing an empty pane.

Also carried through: `leaks` (measured at process exit, not at the last step),
`double_free`, and compile `diagnostics` / `warnings` — all of which map onto
Proton's existing diagnostics surface instead of a bespoke one.

### Stage 3 — wired into the workspace

This is where it stops being an embedded tool and starts being part of the
editor.

**Editor ⇄ trace.** Traced lines get a gutter mark. Clicking a line number jumps
to the next step on that line. Clicking any value in the pane tracks it through
the whole run, and `[` / `]` jump to the previous and next change *to that
value*. The current step highlights the corresponding line in the buffer, and
the output pane dims everything the program had not printed yet at that step.

**Agent ⇄ trace.** `@trace` becomes a context scope. The agent can run a trace
as a tool and read the result, which turns "why does this crash" into something
it can answer with a step number instead of a guess — the prototype's task queue
shows the shape of it: *"traced to step 212 — `node->next` reads freed memory"*,
with `trace-2181.json` attached as an artifact. A trace is a far better grounding
signal than source text alone, because it is a record of what actually happened.

**Data mode.** The Python backend traces with `sys.settrace` and no compiler, so
a notebook cell can be stepped in the same pane. Aliasing becomes visible —
`b = a` draws one list with two arrows, and the mutable-default-argument trap
draws as three names converging on one object.

**Targets.** `GET /api/toolchain` reports implemented-but-unusable as its own
state with an actionable hint (`gdb built without Python support`,
`node < 22.18`). That maps directly onto the explorer's Targets list, which is
where a user already looks to find out why something will not run.

---

## Decisions to make

**Runtime.** Upstream needs `python3`, and C/C++ need `gcc`/`g++` and a
Python-enabled `gdb`. Options: require a system Python; bundle one as upstream's
Windows desktop build already does (`tools/build_windows.py` ships a folder you
unzip and double-click); or ship the tracer in a container. The desktop build is
the closest fit — it exists, and it solves the same problem Proton has.

On Windows specifically, the gdb backends want an MSYS2 UCRT64 toolchain. That
is already how this machine is set up, so Proton should detect
`C:\msys64\ucrt64\bin` rather than asking.

**Keymap collision.** Upstream binds `Space` to play/pause and the arrow keys to
stepping. Proton's leader is `Space`. Resolution: arrow keys and `[` / `]` stay
scoped to the pane when it has focus; play/pause moves to `␣cp`; `␣ct` traces
the current buffer; `:ProtonTrace <file>` is the explicit form. Upstream's
bindings keep working inside the sidecar in Stage 1.

**Where traces live.** Upstream can already `Save` / `Open` a trace and reopen
it with no server. Proton should write them to `.proton/traces/` so `␣ft` can
find them, they survive a restart, and a trace can be attached to an agent task
or a PR review as evidence.

**Upstreamable.** If the native pane needs anything the format does not carry, it
belongs in `docs/trace-format.md` as a versioned addition — not in a Proton-only
side channel. The seam is the whole value of the design; keeping Proton on the
public side of it is what keeps both projects cheap to change.
