/* A scripted Claude Code session for the browser preview: enough to design
 * the Chat tab against — every kind of row, a question waiting, a task list,
 * an artifact, a Codex conversation beside it, the desktop app's sessions —
 * and a reply that streams when something is sent. The real conversations
 * are the processes proton-claude drives; this must never grow into a
 * second one. */

const ARTIFACT = "4a1f9d2e-77b3-4c41-9e0a-5c2d8f1b6a30";
const SESSION = "0f8fad5b-d9cb-469f-a165-70867728950e";
const CODEX_SESSION = "7d3e2f10-5b6a-4c8d-9e0f-1a2b3c4d5e6f";
const GLOSSARY = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";
const GLOSSARY_ARTIFACT = "9c3b7e21-4d8a-4f6b-a1e2-7b5c0d9e8f14";
const ROOT = "/Users/you/Projects/Proton";

const now = Date.now();

function transcript() {
  return [
    { kind: "user", id: "u1", text: "The fleet grid janks when the telemetry updates. Find out why and fix it, then make me a small chart of before/after frame times.", images: 0 },
    { kind: "thinking", id: "m1:0", text: "The grid probably re-renders every row on each tick. I should look at how `ranked` is built and whether the rows are memoised.", tokens: 412, done: true, parent: null },
    { kind: "text", id: "m1:1", text: "I'll start by looking at how the grid builds its rows.", done: true, parent: null },
    { kind: "tool", id: "t1", name: "Grep", input: { pattern: "ranked", path: "src" }, state: "done", streamed: 0,
      result: "src/fleet/Grid.tsx\nsrc/fleet/useRanked.ts", detail: { numFiles: 2, filenames: ["src/fleet/Grid.tsx", "src/fleet/useRanked.ts"] },
      parent: null, ask: null, artifact: null },
    { kind: "tool", id: "t2", name: "Read", input: { file_path: `${ROOT}/src/fleet/useRanked.ts` }, state: "done", streamed: 0,
      result: "", detail: { type: "text", file: { filePath: `${ROOT}/src/fleet/useRanked.ts`, numLines: 24, startLine: 1, totalLines: 24,
        content: "export function useRanked(sats: Sat[], health: Health) {\n  // Rebuilt on every render.\n  return sats.map((sat) => ({ ...sat, score: score(sat, health) }))\n    .sort((a, b) => b.score - a.score);\n}\n" } },
      parent: null, ask: null, artifact: null },
    { kind: "text", id: "m2:0", text: "Found it: `useRanked` returns a **new array on every render**, so all 40 `Sparkline`s remount each tick. Memoising it on `[sats, health]` keeps the rows stable:", done: true, parent: null },
    { kind: "tool", id: "t3", name: "Edit", input: { file_path: `${ROOT}/src/fleet/useRanked.ts`, old_string: "  return sats", new_string: "  return useMemo(() => sats" }, state: "done", streamed: 0,
      result: "The file has been updated.", detail: { filePath: `${ROOT}/src/fleet/useRanked.ts`, structuredPatch: [{ oldStart: 1, oldLines: 5, newStart: 1, newLines: 5, lines: [
        " export function useRanked(sats: Sat[], health: Health) {",
        "-  // Rebuilt on every render.",
        "-  return sats.map((sat) => ({ ...sat, score: score(sat, health) }))",
        "-    .sort((a, b) => b.score - a.score);",
        "+  // Rebuilt only when the fleet or its health changes.",
        "+  return useMemo(() => sats.map((sat) => ({ ...sat, score: score(sat, health) }))",
        "+    .sort((a, b) => b.score - a.score), [sats, health]);",
        " }",
      ] }] }, parent: null, ask: null, artifact: null },
    { kind: "tool", id: "t4", name: "Bash", input: { command: "npm test -- fleet", description: "Run the fleet tests" }, state: "done", streamed: 0,
      result: "", detail: { stdout: "PASS src/fleet/Grid.test.tsx\n  ✓ keeps rows stable across ticks (14 ms)\n  ✓ re-ranks when health changes (3 ms)\n\nTests: 2 passed, 2 total", stderr: "", interrupted: false },
      parent: null, ask: null, artifact: null },
    { kind: "tool", id: "t5", name: "TaskCreate", input: { subject: "Memoise the ranking" }, state: "done", streamed: 0, result: "", detail: null, parent: null, ask: null, artifact: null },
    { kind: "tool", id: "t6", name: "Artifact", input: { file_path: `${ROOT}/frame-times.html`, icon: "chart", description: "Frame times before and after the fix" }, state: "done", streamed: 0,
      result: "Published", detail: { url: "https://claude.ai/artifact/preview", artifact_id: ARTIFACT, title: "Fleet grid frame times", seq: 1 },
      parent: null, ask: null, artifact: { id: ARTIFACT, seq: 1, title: "Fleet grid frame times", url: "https://claude.ai/artifact/preview", icon: "chart", file: "index.html" } },
    { kind: "text", id: "m3:0", text: "The grid now holds **60 fps** through telemetry bursts. The chart beside this compares the two.\n\nOne more thing: the ranking is also used by the export. Shall I write the same fix there?", done: true, parent: null },
    { kind: "turn", id: "turn-1", ms: 48200, turns: 9, cost: 0.21, error: null },
    { kind: "user", id: "u2", text: "Yes, and write the export test too.", images: 0 },
    { kind: "tool", id: "t7", name: "Write", input: { file_path: `${ROOT}/src/export/rank.test.ts`, content: "import { rankForExport } from './rank';\n\ntest('ranks once per export', () => {\n  expect(rankForExport(fleet)).toHaveLength(40);\n});\n" },
      state: "waiting", streamed: 0, result: null, detail: null, parent: null,
      ask: { request: "req-1", kind: "permission", description: "rank.test.ts", suggestions: [{ type: "setMode", mode: "acceptEdits", destination: "session" }], blocked: null },
      artifact: null },
  ];
}

/** A Codex conversation, as Proton keeps one: its commands and a patch. */
function codexTranscript() {
  return [
    { kind: "user", id: "you-1", text: "Bump the telemetry poll to 2 s and make the interval configurable.", images: 0 },
    { kind: "text", id: "msg_1", text: "I'll find where the poll interval is set.", done: true, parent: null },
    { kind: "tool", id: "exec-1", name: "Bash", input: { command: "rg -n 'POLL_MS' src", cwd: ROOT }, state: "done", streamed: 0,
      result: "src/fleet/poll.ts:3:export const POLL_MS = 1000;", detail: { stdout: "src/fleet/poll.ts:3:export const POLL_MS = 1000;", exitCode: 0 },
      parent: null, ask: null, artifact: null },
    { kind: "tool", id: "patch-1", name: "Patch", input: { files: [{ path: `${ROOT}/src/fleet/poll.ts`, kind: "update" }] }, state: "done", streamed: 0,
      result: null, detail: { files: [{ path: `${ROOT}/src/fleet/poll.ts`, kind: "update", movePath: null, structuredPatch: [{ oldStart: 1, oldLines: 4, newStart: 1, newLines: 4, lines: [
        " import { settings } from '../settings';",
        " ",
        "-export const POLL_MS = 1000;",
        "+export const POLL_MS = settings.number('fleet.pollMs', 2000);",
        " ",
      ] }] }] }, parent: null, ask: null, artifact: null },
    { kind: "text", id: "msg_2", text: "The poll now defaults to **2 s** and reads `fleet.pollMs` from the settings.", done: true, parent: null },
    { kind: "turn", id: "turn-1", ms: 21400, turns: 1, cost: 0, error: null },
  ];
}

function status(extra = {}) {
  return {
    session: SESSION, engine: "claude", title: "Why the fleet grid janks", activity: "waiting", doing: "", model: "claude-opus-5-5",
    mode: "default", effort: null, since: null, cost: 0.21, context: 41230, output: 1820,
    tasks: [
      { id: "1", subject: "Memoise the ranking", status: "completed", active: null },
      { id: "2", subject: "Fix the export's ranking", status: "in_progress", active: "Fixing the export's ranking" },
      { id: "3", subject: "Write the export test", status: "pending", active: null },
    ],
    limit: { window: "five_hour", used: 0.24, resets: Math.round(now / 1000) + 9000, status: "allowed" },
    error: null, version: "2.1.288", ...extra,
  };
}

const capabilities = {
  engine: "claude",
  modes: [
    { value: "default", name: "Ask permissions", note: "Ask before edits and commands", icon: "eye" },
    { value: "acceptEdits", name: "Accept edits", note: "Edit files without asking", icon: "pen" },
    { value: "plan", name: "Plan mode", note: "Read and plan; change nothing", icon: "bulb" },
    { value: "auto", name: "Auto", note: "Claude decides what needs asking", icon: "bolt" },
  ],
  missing: null,
  models: [
    { value: "default", resolved: "claude-opus-5-5", name: "Default (recommended)", description: "Opus 5.5 · Best for everyday, complex tasks", efforts: ["low", "medium", "high", "xhigh", "max"] },
    { value: "opus", resolved: "claude-opus-5-5", name: "Opus 5.5", description: "Best for everyday, complex tasks", efforts: ["low", "medium", "high", "xhigh", "max"] },
    { value: "fable", resolved: "claude-fable-5-1", name: "Fable 5.1", description: "Most capable for your hardest tasks", efforts: ["low", "medium", "high", "xhigh", "max"] },
    { value: "sonnet", resolved: "claude-sonnet-5-5", name: "Sonnet 5.5", description: "Fast and capable", efforts: ["low", "medium", "high"] },
    { value: "haiku", resolved: "claude-haiku-4-5-20251001", name: "Haiku 4.5", description: "Fastest for quick answers", efforts: [] },
  ],
  commands: [
    { name: "compact", description: "Clear conversation history but keep a summary in context", hint: "<instructions>" },
    { name: "init", description: "Initialize a CLAUDE.md file with codebase documentation", hint: "" },
    { name: "review", description: "Review a pull request", hint: "<pr>" },
    { name: "code-review", description: "Review the current branch for bugs", hint: "" },
    { name: "simplify", description: "Review changed code for reuse, quality and efficiency", hint: "" },
  ],
  plan: "Claude Max",
  email: "you@example.com",
};

/** The other engines, as their probes would describe them here. */
const others = [
  {
    engine: "codex", plan: "ChatGPT Plus", email: "you@example.com", missing: null, commands: [],
    models: [
      { value: "gpt-6.1-sol", resolved: "gpt-6.1-sol", name: "GPT-6.1-Sol", description: "Latest workhorse model for coding and everyday work.", efforts: ["low", "medium", "high", "xhigh"] },
      { value: "gpt-6-astra", resolved: "gpt-6-astra", name: "GPT-6-Astra", description: "For the hardest problems.", efforts: ["medium", "high", "xhigh"] },
    ],
    modes: [
      { value: "default", name: "Ask permissions", note: "Ask before commands and edits", icon: "eye" },
      { value: "acceptEdits", name: "Auto", note: "Work in the project; ask to go beyond it", icon: "pen" },
      { value: "plan", name: "Read only", note: "Read and answer; change nothing", icon: "bulb" },
      { value: "bypassPermissions", name: "Full access", note: "No sandbox, and never ask", icon: "bolt" },
    ],
  },
  {
    engine: "gemini", plan: "", email: "", commands: [],
    missing: "The Gemini CLI is not installed. Install it (npm install -g @google/gemini-cli), run `gemini` once in a terminal to sign in, then try again.",
    models: [{ value: "default", resolved: "", name: "Default (recommended)", description: "The Gemini CLI's choice", efforts: [] }],
    modes: [
      { value: "default", name: "Ask permissions", note: "Ask before edits and commands", icon: "eye" },
      { value: "acceptEdits", name: "Accept edits", note: "Edit files without asking", icon: "pen" },
      { value: "bypassPermissions", name: "Allow everything", note: "Run every tool without asking", icon: "bolt" },
    ],
  },
  {
    engine: "antigravity", plan: "", email: "", missing: null, commands: [],
    models: [
      { value: "default", resolved: "", name: "Default (recommended)", description: "Antigravity's choice", efforts: [] },
      { value: "gemini-3.1-pro-high", resolved: "gemini-3.1-pro-high", name: "Gemini 3.1 Pro (High)", description: "", efforts: [] },
    ],
    modes: [
      { value: "default", name: "Review", note: "Read freely; refuse edits and commands", icon: "eye" },
      { value: "acceptEdits", name: "Accept edits", note: "Edit files; refuse commands", icon: "pen" },
      { value: "plan", name: "Plan mode", note: "Read and plan; change nothing", icon: "bulb" },
      { value: "bypassPermissions", name: "Allow everything", note: "Run every tool without asking", icon: "bolt" },
    ],
  },
];

export function createClaudePreview(deliver) {
  let items = transcript();
  let current = status();
  let counter = 0;
  let engine = "claude";
  let active = SESSION;
  const row = (/** @type {string} */ id, /** @type {string} */ title, /** @type {number} */ updated, extra = {}) =>
    ({ id, title, updated, cwd: "", elsewhere: false, engine: "claude", ...extra });
  /** Your folders, as the core keeps them. @type {{id: string, name: string, collapsed: boolean, members: string[]}[]} */
  let folders = [{ id: "folder-fleet", name: "Fleet", collapsed: false, members: [GLOSSARY, CODEX_SESSION] }];
  const sessions = () => {
    const rows = everyRow();
    const inFolders = new Set(folders.flatMap((f) => f.members));
    return {
      type: "claudeSessions", active, engine, busy: current.activity === "working" ? [active] : [],
      waiting: current.activity === "waiting" ? [active] : [],
      folders: folders.map((f) => ({ id: f.id, name: f.name, collapsed: f.collapsed,
        sessions: f.members.map((id) => rows.find((r) => r.id === id)).filter(Boolean) })),
      sessions: rows.filter((r) => !inFolders.has(r.id)),
    };
  };
  const everyRow = () => [
      row(SESSION, "Why the fleet grid janks", now - 60_000),
      row(CODEX_SESSION, "Make the telemetry poll configurable", now - 2 * 3600_000, { engine: "codex" }),
      row(GLOSSARY, "Fleet telemetry glossary", now - 3 * 3600_000),
      row("6ec0bd7f-11c0-43da-975e-2a8ad9ebae0b", "Port trajectory.c to Android", now - 26 * 3600_000),
      row("9b2c1e44-0d5f-4c2e-8a1b-3f6d7e8c9a0b", "Set up the Parquet ingest", now - 9 * 86400_000),
      // The desktop app's, from its other folders.
      row("c2a7f9e0-3b1d-4e5f-8a6b-7c9d0e1f2a3b", "Batting averages by venue", now - 5 * 3600_000, { cwd: "/Users/you/Github/Cricket-Analysis", elsewhere: true }),
      row("e4b8a1c2-5d6f-4a7b-9c8d-0e1f2a3b4c5d", "Quantisation and SNR", now - 4 * 86400_000, { cwd: "/Users/you/Research/TinyML" }),
    ];
  const artifacts = (importing = false) => ({
    type: "artifacts", importing,
    artifacts: [
      { id: ARTIFACT, url: "https://claude.ai/artifact/preview", title: "Fleet grid frame times", icon: "chart",
        description: "Frame times before and after the fix", project: ROOT, session: SESSION, versions: [{ seq: 1, file: "index.html", at: now - 60_000 }] },
      { id: GLOSSARY_ARTIFACT, url: "https://claude.ai/artifact/glossary", title: "Telemetry glossary", icon: "book",
        description: "The fields the fleet grid draws", project: ROOT, session: GLOSSARY, versions: [{ seq: 1, file: "index.html", at: now - 3 * 3600_000 }] },
    ],
  });

  const all = () => ({ type: "claudeItems", session: active, items, reset: true, earlier: false });
  const changed = (/** @type {any[]} */ rows) => ({ type: "claudeItems", session: active, items: rows, reset: false, earlier: false });

  function sync() {
    return [
      sessions(), all(), { type: "claudeStatus", status: current }, { type: "claudeCapabilities", capabilities },
      ...others.map((c) => ({ type: "claudeCapabilities", capabilities: c })), artifacts(),
    ];
  }

  /** A new, empty conversation with an engine. @param {string} chosen */
  function fresh(chosen) {
    engine = chosen;
    active = `new-${++counter}`;
    items = [];
    const model = chosen === "claude" ? "claude-opus-5-5" : "";
    current = status({ session: active, engine: chosen, activity: "idle", title: "", tasks: [], cost: 0, context: 0, model, limit: null });
    return [sessions(), all(), { type: "claudeStatus", status: current }];
  }

  /** A reply, streamed a few words at a time. @param {string} text */
  function reply(text) {
    const id = `p${++counter}`;
    current = { ...current, activity: "working", since: Date.now(), doing: "", title: current.title || "New session" };
    deliver({ type: "claudeStatus", status: current });
    deliver(sessions());
    const thinking = { kind: "thinking", id: `${id}:0`, text: "", tokens: 0, done: false, parent: null };
    setTimeout(() => {
      items.push(thinking);
      deliver(changed([thinking]));
    }, 250);
    const words = text.split(/(\s+)/);
    const row = { kind: "text", id: `${id}:1`, text: "", done: false, parent: null };
    setTimeout(() => {
      thinking.done = true;
      thinking.tokens = 186;
      items.push(row);
      deliver(changed([thinking, row]));
    }, 900);
    words.forEach((_, i) => setTimeout(() => {
      row.text = words.slice(0, i + 1).join("");
      row.done = i === words.length - 1;
      deliver(changed([row]));
      if (row.done) {
        const turn = { kind: "turn", id: `turn-${id}`, ms: 2400 + words.length * 40, turns: 1, cost: 0.01, error: null };
        items.push(turn);
        current = { ...current, activity: "idle", since: null };
        deliver(changed([turn]));
        deliver({ type: "claudeStatus", status: current });
        deliver(sessions());
      }
    }, 1000 + i * 35));
  }

  /** @param {any} command */
  function command(command) {
    switch (command.type) {
      case "claudeSend": {
        const you = { kind: "user", id: `you-${++counter}`, text: command.text, images: command.images?.length ?? 0 };
        items.push(you);
        setTimeout(() => reply(
          "Here is what I would do:\n\n1. Read the export's ranking in `src/export/rank.ts`\n2. Memoise it the way the grid's is\n3. Add a test that **ranks once per export**\n\n```ts\nexport const rankForExport = (fleet: Sat[]) =>\n  useMemo(() => rank(fleet), [fleet]);\n```\n\nShall I go ahead?",
        ), 120);
        return [changed([you])];
      }
      case "claudeAnswer": {
        const tool = items.find((i) => i.kind === "tool" && i.ask?.request === command.request);
        if (!tool) return [];
        const allowed = command.answer.kind !== "deny";
        tool.ask = null;
        tool.state = allowed ? "done" : "denied";
        tool.result = allowed ? "File created successfully." : "The user declined.";
        if (allowed) tool.detail = { type: "create", filePath: tool.input.file_path, content: tool.input.content, structuredPatch: [] };
        current = { ...current, activity: "idle" };
        return [changed([tool]), { type: "claudeStatus", status: current }, sessions()];
      }
      case "claudeStop":
        current = { ...current, activity: "idle" };
        return [{ type: "claudeStatus", status: current }];
      case "claudeNew":
        return fresh(command.engine ?? engine);
      case "claudeEngine":
        return fresh(command.engine);
      case "claudeOpen":
        if (command.session === CODEX_SESSION) {
          active = CODEX_SESSION;
          items = codexTranscript();
          current = status({ session: CODEX_SESSION, engine: "codex", title: "Make the telemetry poll configurable", activity: "idle", model: "gpt-6.1-sol",
            tasks: [], cost: 0, context: 18812, limit: { window: "seven_day", used: 0.02, resets: Math.round(now / 1000) + 400000, status: "allowed" } });
          return [sessions(), all(), { type: "claudeStatus", status: current }];
        }
        if (command.session !== SESSION) {
          // Every other conversation, briefly — its own id, so it has its
          // own pane; the glossary one published an artifact.
          const title = everyRow().find((r) => r.id === command.session)?.title ?? "A conversation";
          active = command.session;
          items = [
            { kind: "user", id: "you-1", text: `Pick up “${title}”.`, images: 0 },
            { kind: "text", id: "m1:0", text: `Here is where **${title}** stands.`, done: true, parent: null },
            ...(command.session === GLOSSARY ? [{ kind: "tool", id: "g1", name: "Artifact", input: { title: "Telemetry glossary", icon: "book" }, state: "done", streamed: 0,
              result: "Published", detail: { artifact_id: GLOSSARY_ARTIFACT, seq: 1 }, parent: null, ask: null,
              artifact: { id: GLOSSARY_ARTIFACT, seq: 1, title: "Telemetry glossary", url: "https://claude.ai/artifact/glossary", icon: "book", file: "index.html" } }] : []),
            { kind: "turn", id: "turn-1", ms: 12000, turns: 1, cost: 0.01, error: null },
          ];
          current = status({ session: active, title, activity: "idle", tasks: [], limit: null });
          return [sessions(), all(), { type: "claudeStatus", status: current }];
        }
        active = SESSION;
        items = transcript();
        current = status();
        return sync();
      case "claudeMode":
        current = { ...current, mode: command.mode };
        return [{ type: "claudeStatus", status: current }];
      case "claudeModel": {
        const known = [capabilities, ...others].find((c) => c.engine === current.engine) ?? capabilities;
        const model = known.models.find((m) => m.value === command.model);
        current = { ...current, model: model?.resolved ?? command.model };
        return [{ type: "claudeStatus", status: current }];
      }
      case "claudeEffort":
        current = { ...current, effort: command.effort };
        return [{ type: "claudeStatus", status: current }];
      case "chatFolderNew": {
        const taken = new Set(folders.map((f) => f.name));
        let name = String(command.name ?? "").trim();
        if (!name) {
          name = "New folder";
          for (let n = 2; taken.has(name); n++) name = `New folder ${n}`;
        }
        for (const f of folders) f.members = f.members.filter((id) => !command.sessions.includes(id));
        folders.push({ id: `folder-${++counter}`, name, collapsed: false, members: [...command.sessions] });
        return [sessions()];
      }
      case "chatFolderRename": {
        const f = folders.find((x) => x.id === command.folder);
        if (f && String(command.name).trim()) f.name = String(command.name).trim();
        return [sessions()];
      }
      case "chatFolderDelete":
        folders = folders.filter((f) => f.id !== command.folder);
        return [sessions()];
      case "chatFolderCollapse": {
        const f = folders.find((x) => x.id === command.folder);
        if (f) f.collapsed = command.collapsed;
        return [sessions()];
      }
      case "chatFolderMove":
        for (const f of folders) f.members = f.members.filter((id) => id !== command.session);
        if (command.folder) folders.find((f) => f.id === command.folder)?.members.push(command.session);
        return [sessions()];
      case "importArtifacts":
        setTimeout(() => {
          deliver(artifacts(false));
          deliver({ type: "notice", level: "info", text: "No artifacts to import: every one published from this machine is already here." });
        }, 600);
        return [artifacts(true)];
      default:
        return [];
    }
  }

  return { sync, command };
}
