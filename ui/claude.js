/* Claude Code, in the Chat tab.
 *
 * Drawn the way Claude Code's own app draws a session: the sessions down the
 * left, the conversation in a column with your messages as bubbles and
 * Claude's as prose, every tool it uses as one quiet line that opens into its
 * diff or its output, the questions it stops to ask as cards you answer in
 * place, its task list above the composer, and what it publishes in a pane
 * beside it all.
 *
 * Nothing here decides anything. The core runs `claude` and folds what it
 * says into rows; this draws the rows it is sent — only the ones that
 * changed, each into its own element, so a reply streaming in redraws one
 * paragraph and leaves an open diff above it open. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { highlight, highlightLines } from "./highlight.js";

/** @typedef {import("./bindings/ClaudeItem").ClaudeItem} Item */
/** @typedef {import("./bindings/ClaudeTool").ClaudeTool} Tool */
/** @typedef {import("./bindings/ClaudeStatus").ClaudeStatus} Status */
/** @typedef {import("./bindings/ClaudeCapabilities").ClaudeCapabilities} Capabilities */
/** @typedef {import("./bindings/ClaudeSession").ClaudeSession} Session */
/** @typedef {import("./bindings/Artifact").Artifact} Artifact */
/** @typedef {import("./bindings/ChatEngine").ChatEngine} Engine */
/** @typedef {import("./bindings/ChatFolder").ChatFolder} Folder */

/** Claude Code's permission modes, in the order Shift+Tab steps through
 *  them — until an engine has said what its own are. */
const MODES = [
  { value: "default", name: "Ask permissions", note: "Ask before edits and commands", icon: "eye" },
  { value: "acceptEdits", name: "Accept edits", note: "Edit files without asking", icon: "pen" },
  { value: "plan", name: "Plan mode", note: "Read and plan; change nothing", icon: "bulb" },
  { value: "auto", name: "Auto", note: "Claude decides what needs asking", icon: "bolt" },
];

/** The agents the tab can talk to: what each is called, and what it runs on. */
const ENGINES = /** @type {Record<Engine, {name: string, short: string, note: string, icon: string}>} */ ({
  claude: { name: "Claude Code", short: "Claude", note: "Anthropic's agent, on your Claude plan", icon: "spark" },
  codex: { name: "Codex", short: "Codex", note: "OpenAI's agent, on your ChatGPT plan", icon: "code" },
  gemini: { name: "Gemini", short: "Gemini", note: "Google's Gemini CLI, on your Google sign-in", icon: "globe" },
  antigravity: { name: "Antigravity", short: "Antigravity", note: "Google's Antigravity agent, on its own sign-in", icon: "orbit" },
});
const ENGINE_ORDER = /** @type {Engine[]} */ (["claude", "codex", "gemini", "antigravity"]);

/** What Claude is doing, when it has not said — as Claude Code does. */
const VERBS = ["Thinking", "Pondering", "Working", "Considering", "Brewing", "Crafting", "Mulling", "Reasoning"];

/** Tools whose rows say nothing the task list or the next row does not. */
const QUIET = new Set(["TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "TaskStop", "TaskOutput", "ToolSearch", "TodoWrite"]);

/** An artifact's icon word, as one of ours. */
const ARTIFACT_ICONS = {
  chart: "chart", graph: "chart", dashboard: "chart", keyboard: "keyboard", code: "code", map: "globe",
  globe: "globe", calendar: "clock", clock: "clock", game: "game", music: "audio", image: "image",
  table: "table", recipe: "book", book: "book", doc: "file-doc", document: "file-doc", design: "palette",
  slides: "layers", deck: "layers", list: "check", check: "check", video: "film", data: "db",
};

/** @param {number} ms */
function duration(ms) {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

/** @param {number} n */
function compact(n) {
  return n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}k` : String(n);
}

/** The language a file is written in, for a code block's label.
 *  @param {string} path */
function language(path) {
  const ext = (path.match(/\.([\w]+)$/)?.[1] ?? "").toLowerCase();
  const names = /** @type {Record<string, string>} */ ({
    rs: "rust", js: "javascript", mjs: "javascript", ts: "typescript", tsx: "tsx", jsx: "jsx", py: "python",
    rb: "ruby", go: "go", c: "c", h: "c", cpp: "cpp", hpp: "cpp", cc: "cpp", java: "java", kt: "kotlin",
    swift: "swift", md: "markdown", json: "json", toml: "toml", yaml: "yaml", yml: "yaml", html: "html",
    css: "css", sh: "bash", zsh: "bash", sql: "sql", lua: "lua", zig: "zig",
  });
  return names[ext] ?? ext;
}

/** What a conversation's artifacts pane was showing: whether it was open, the
 *  artifact and version in it, and whether the list was over it.
 *  @typedef {{open: boolean, shown: {id: string, seq: number} | null, gallery: boolean}} Pane */

/** Where each conversation's pane is remembered between runs, in this
 *  window's own storage: it is how the window was left, not the project. */
const PANES = "proton.chat.panes";
/** How many conversations' panes are remembered, the latest kept. */
const PANES_KEPT = 300;

/** @returns {Map<string, Pane>} */
function loadPanes() {
  try {
    const saved = JSON.parse(localStorage.getItem(PANES) ?? "[]");
    return new Map(Array.isArray(saved) ? saved : []);
  } catch {
    return new Map();
  }
}

/** @param {Map<string, Pane>} panes */
function savePanes(panes) {
  try {
    localStorage.setItem(PANES, JSON.stringify([...panes].slice(-PANES_KEPT)));
  } catch {
    // A window that cannot keep it forgets which artifact was open; nothing more.
  }
}

/**
 * @param {{
 *   root: Document,
 *   dispatch: (command: any) => unknown,
 *   panels: {show: (side: string, section?: Element) => void, hide?: (side: string, section?: Element) => void},
 *   shell: boolean,
 *   convene: (question: string) => void,
 * }} host
 */
export function createClaude(host) {
  const doc = host.root;
  const ws = /** @type {HTMLElement} */ (doc.querySelector('.workspace[data-ws="chat"]'));
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws?.querySelector(sel));
  const els = {
    sessions: q("[data-claude-sessions]"),
    account: q("[data-claude-account]"),
    title: q("[data-claude-title]"),
    folder: q("[data-claude-folder]"),
    thread: q("[data-claude-thread]"),
    items: q("[data-claude-items]"),
    working: q("[data-claude-working]"),
    earlier: q("[data-claude-earlier]"),
    welcome: q("[data-claude-welcome]"),
    tasks: q("[data-claude-tasks]"),
    input: /** @type {HTMLTextAreaElement} */ (q("[data-claude-input]")),
    attachments: q("[data-claude-attachments]"),
    menu: q("[data-claude-menu]"),
    engine: q("[data-claude-engine]"),
    mode: q("[data-claude-mode]"),
    model: q("[data-claude-model]"),
    welcomeTitle: q("[data-claude-welcome-title]"),
    welcomeText: q("[data-claude-welcome-text]"),
    meter: q("[data-claude-meter]"),
    send: q("[data-claude-send]"),
    pop: q("[data-claude-pop]"),
    file: /** @type {HTMLInputElement} */ (q("[data-claude-file]")),
    pane: q("[data-claude-artifacts]"),
    artTitle: q("[data-artifact-title]"),
    artIcon: q("[data-artifact-icon]"),
    artVersion: /** @type {HTMLSelectElement} */ (q("[data-artifact-version]")),
    artFrame: /** @type {HTMLIFrameElement} */ (q("[data-artifact-frame]")),
    artDoc: q("[data-artifact-doc]"),
    artEmpty: q("[data-artifact-empty]"),
    gallery: q("[data-artifact-gallery-list]"),
    artCount: q("[data-artifacts-count]"),
  };
  if (!ws || !els.items) return null;

  /** @type {{session: string, items: Map<string, Item>, order: string[], status: Status | null,
   *   engines: Map<string, Capabilities>, newEngine: Engine, sessions: Session[], active: string, busy: Set<string>, waiting: Set<string>,
   *   folders: Folder[], renaming: string, draft: string, fresh: boolean, expectFolder: boolean, knownFolders: Set<string>,
   *   artifacts: Artifact[], open: Set<string>, shown: {id: string, seq: number} | null, root: string,
   *   panes: Map<string, Pane>, latest: Map<string, number> | null,
   *   attachments: {media: string, data: string, name: string}[], live: boolean, menuIndex: number,
   *   limit: any, verb: string, importing: boolean}} */
  const state = {
    session: "", items: new Map(), order: [], status: null,
    engines: new Map(), newEngine: "claude",
    sessions: [], active: "", busy: new Set(), waiting: new Set(),
    folders: [], renaming: "", draft: "", fresh: false, expectFolder: false, knownFolders: new Set(),
    artifacts: [], open: new Set(), shown: null, root: "",
    panes: loadPanes(), latest: null,
    attachments: [], live: false, menuIndex: 0, limit: null, verb: VERBS[0], importing: false,
  };

  /** A path as the project sees it. @param {string} path */
  const rel = (path) => {
    const p = String(path ?? "");
    if (state.root && p.startsWith(state.root + "/")) return p.slice(state.root.length + 1);
    return p.replace(/^\/Users\/[^/]+/, "~");
  };

  /** The engine of the conversation on screen, or the one a new one starts with. */
  const engineNow = () => /** @type {Engine} */ (state.status?.engine ?? state.newEngine ?? "claude");
  /** What that engine offers, as far as it is known. @returns {Capabilities} */
  const caps = () => state.engines.get(engineNow()) ??
    /** @type {Capabilities} */ ({ engine: engineNow(), models: [], modes: [], commands: [], plan: "", email: "", missing: null });
  /** Its permission modes. @returns {{value: string, name: string, note: string, icon: string}[]} */
  const modes = () => (caps().modes?.length ? caps().modes : MODES);
  /** What it is called in a sentence: "Allow Codex to…". */
  const agent = () => ENGINES[engineNow()]?.short ?? "Claude";

  /* ── the session list ─────────────────────────────────────────── */

  /** @param {number} updated */
  function day(updated) {
    if (updated >= Number.MAX_SAFE_INTEGER) return "Today";
    const then = new Date(updated), now = new Date();
    const days = Math.floor((new Date(now.toDateString()).getTime() - new Date(then.toDateString()).getTime()) / 864e5);
    return days <= 0 ? "Today" : days === 1 ? "Yesterday" : days < 7 ? "This week" : days < 31 ? "This month" : "Earlier";
  }

  /** One row of the list, which can be dragged into a folder or moved with
   *  its menu. @param {Session} s */
  function sessionRow(s) {
    const busy = state.busy.has(s.id), waiting = state.waiting.has(s.id);
    const when = s.updated >= Number.MAX_SAFE_INTEGER ? "now" : new Date(s.updated).toLocaleString();
    const where = s.cwd ? s.cwd.split("/").filter(Boolean).pop() ?? s.cwd
      : s.engine && s.engine !== "claude" ? ENGINES[s.engine]?.short ?? "" : "";
    const tip = [s.title, s.engine && s.engine !== "claude" ? ENGINES[s.engine]?.name : "", s.cwd ? rel(s.cwd) : "", when,
      s.elsewhere ? "open in another Claude Code" : ""].filter(Boolean).join(" · ");
    return (
      `<div class="cc-session-wrap">` +
      `<button class="cc-session${s.id === state.active ? " on" : ""}" data-claude-session="${esc(s.id)}" draggable="true" title="${esc(tip)}">` +
      `<span class="cc-session-name">${esc(s.title || "Untitled")}</span>` +
      (where ? `<span class="cc-session-where"${s.cwd ? "" : ` data-engine="${esc(s.engine)}"`}>${esc(where)}</span>` : "") +
      (busy ? `<span class="cc-spin" aria-label="Working"></span>`
        : waiting ? `<span class="cc-wait" aria-label="Waiting for you"></span>`
        : s.elsewhere ? `<span class="cc-elsewhere" aria-label="Open in another Claude Code"></span>` : "") +
      `</button>` +
      `<button class="cc-session-more" data-session-menu="${esc(s.id)}" title="Move to a folder" aria-label="Move ${esc(s.title || "this chat")} to a folder">${icon("dots")}</button>` +
      `</div>`
    );
  }

  /** A folder: its head, and what it holds unless it is folded. @param {Folder} f */
  function folderBlock(f) {
    const renaming = state.renaming === f.id;
    // Folded, it still says when something in it is working or waiting.
    const marker = !f.collapsed ? ""
      : f.sessions.some((s) => state.busy.has(s.id)) ? `<span class="cc-spin" aria-label="Working"></span>`
      : f.sessions.some((s) => state.waiting.has(s.id)) ? `<span class="cc-wait" aria-label="Waiting for you"></span>` : "";
    const holdsActive = f.collapsed && f.sessions.some((s) => s.id === state.active);
    return `<div class="cc-folder${f.collapsed ? " folded" : ""}${holdsActive ? " has-on" : ""}${renaming ? " renaming" : ""}" data-folder="${esc(f.id)}">` +
      `<div class="cc-folder-head">` +
      `<button class="cc-folder-toggle" data-folder-toggle="${esc(f.id)}" aria-expanded="${String(!f.collapsed)}" title="${esc(f.name)}">` +
      `${icon("chev")}${icon(f.collapsed ? "folder" : "folder-open")}` +
      (renaming ? "" : `<span class="cc-folder-name" data-folder-name="${esc(f.id)}">${esc(f.name)}</span><span class="cc-folder-count">${f.sessions.length || ""}</span>${marker}`) +
      `</button>` +
      (renaming ? `<input class="cc-folder-input" data-folder-input="${esc(f.id)}" value="${esc(state.draft)}" aria-label="Folder name" spellcheck="false" autocomplete="off">` : "") +
      `<button class="cc-folder-more" data-folder-menu="${esc(f.id)}" title="Rename or delete" aria-label="Rename or delete ${esc(f.name)}">${icon("dots")}</button>` +
      `</div>` +
      (f.collapsed ? "" : `<div class="cc-folder-body">${f.sessions.length
        ? f.sessions.map(sessionRow).join("")
        : `<div class="cc-folder-empty">Drag chats here, or move one with its ⋯ menu</div>`}</div>`) +
      `</div>`;
  }

  function renderSessions() {
    if (!els.sessions) return;
    // Your folders first; then this project's sessions by day, then the
    // desktop app's from its other folders, newest first.
    const here = state.sessions.filter((s) => !s.cwd);
    const away = state.sessions.filter((s) => s.cwd);
    let html = `<div class="cc-folders">`;
    if (state.folders.length) html += `<div class="cc-group">Folders</div>${state.folders.map(folderBlock).join("")}`;
    html += `</div><div class="cc-loose" data-loose>`;
    if (!here.length && !state.folders.length) html += `<div class="cc-none">No sessions in this project yet.</div>`;
    let group = "";
    for (const s of here) {
      const label = day(s.updated);
      if (label !== group) {
        group = label;
        html += `<div class="cc-group">${esc(label)}</div>`;
      }
      html += sessionRow(s);
    }
    if (away.length) {
      html += `<div class="cc-group">Claude desktop</div>`;
      for (const s of away) html += sessionRow(s);
    }
    html += `</div>`;
    // A name being typed survives the list being redrawn under it.
    const typing = /** @type {HTMLInputElement | null} */ (els.sessions.querySelector("[data-folder-input]"));
    const caret = typing && doc.activeElement === typing ? [typing.selectionStart, typing.selectionEnd] : null;
    els.sessions.innerHTML = html;
    const input = /** @type {HTMLInputElement | null} */ (els.sessions.querySelector("[data-folder-input]"));
    if (input) {
      input.focus();
      if (state.fresh) {
        input.select();
        state.fresh = false;
      } else if (caret) {
        input.setSelectionRange(caret[0], caret[1]);
      }
    }
  }

  /** Name a folder in place. @param {string} id */
  function startRename(id) {
    const folder = state.folders.find((f) => f.id === id);
    if (!folder) return;
    state.renaming = id;
    state.draft = folder.name;
    state.fresh = true;
    renderSessions();
  }

  /** Keep the name typed, or with `cancel`, the one it had. @param {boolean} [cancel] */
  function finishRename(cancel = false) {
    const id = state.renaming;
    if (!id) return;
    state.renaming = "";
    const folder = state.folders.find((f) => f.id === id);
    const name = state.draft.trim();
    if (!cancel && folder && name && name !== folder.name) {
      folder.name = name;
      host.dispatch({ type: "chatFolderRename", folder: id, name });
    }
    renderSessions();
  }

  /* ── the list's menus ─────────────────────────────────────────── */

  const sideMenu = doc.createElement("div");
  sideMenu.className = "cc-pop cc-side-menu";
  sideMenu.setAttribute("role", "menu");
  sideMenu.hidden = true;
  ws.append(sideMenu);

  /** Open a menu by a point on screen. @param {string} html @param {number} x @param {number} y */
  function openSideMenu(html, x, y) {
    sideMenu.innerHTML = html;
    sideMenu.hidden = false;
    const width = sideMenu.offsetWidth, height = sideMenu.offsetHeight;
    sideMenu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - width - 8))}px`;
    sideMenu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - height - 8))}px`;
    /** @type {HTMLElement | null} */ (sideMenu.querySelector("button"))?.focus();
  }

  const closeSideMenu = () => { sideMenu.hidden = true; };

  /** Which folder a conversation is in. @param {string} id */
  const folderOf = (id) => state.folders.find((f) => f.sessions.some((s) => s.id === id));
  /** A conversation as the list has it, wherever it is drawn. @param {string} id */
  const sessionById = (id) => state.sessions.find((s) => s.id === id) ?? state.folders.flatMap((f) => f.sessions).find((s) => s.id === id);

  /** Where a conversation can go. @param {string} id */
  function sessionMenu(id) {
    const now = folderOf(id);
    return `<div class="cc-pop-head">Move to folder</div>` +
      state.folders.map((f) =>
        `<button class="cc-pop-row${f.id === now?.id ? " on" : ""}" role="menuitem" data-move-to="${esc(f.id)}" data-session="${esc(id)}">` +
        `${icon("folder")}<b>${esc(f.name)}</b>${f.id === now?.id ? icon("check") : ""}</button>`).join("") +
      `<button class="cc-pop-row" role="menuitem" data-new-folder-with="${esc(id)}">${icon("plus")}<b>New folder…</b></button>` +
      (now ? `<button class="cc-pop-row" role="menuitem" data-move-out="${esc(id)}">${icon("close")}<b>Remove from ${esc(now.name)}</b></button>` : "") +
      `<div class="cc-pop-sep"></div>` +
      // One open in another Claude Code would only write itself back.
      (sessionById(id)?.elsewhere
        ? `<button class="cc-pop-row off" role="menuitem" disabled>${icon("close")}<b>Delete chat</b><span>Close it in the other Claude Code first</span></button>`
        : `<button class="cc-pop-row danger" role="menuitem" data-delete-ask="${esc(id)}">${icon("close")}<b>Delete chat…</b></button>`);
  }

  /** Ask before a chat goes. @param {string} id */
  function deleteConfirm(id) {
    const s = sessionById(id);
    const note = s?.cwd
      ? "Its transcript goes to the Trash, where you can restore it. Claude's desktop app may still list it."
      : "Its transcript goes to the Trash, where you can restore it.";
    return `<div class="cc-pop-ask">Delete “${esc(s?.title || "this chat")}”?</div>` +
      `<p class="cc-pop-note">${esc(note)}</p>` +
      `<div class="cc-pop-acts"><button class="cc-quiet" data-delete-cancel>Cancel</button>` +
      `<button class="cc-danger" data-delete-chat="${esc(id)}">Delete</button></div>`;
  }

  /** @param {string} id */
  function folderMenu(id) {
    return `<button class="cc-pop-row" role="menuitem" data-folder-rename="${esc(id)}">${icon("pen")}<b>Rename</b></button>` +
      `<button class="cc-pop-row" role="menuitem" data-folder-delete="${esc(id)}">${icon("close")}<b>Delete folder</b><span>Its chats go back to the list</span></button>`;
  }

  /** A new folder, named in place once it exists. @param {string[]} sessions */
  function newFolder(sessions) {
    state.expectFolder = true;
    host.dispatch({ type: "chatFolderNew", name: "", sessions });
  }

  function renderAccount() {
    if (!els.account) return;
    const known = caps();
    const limit = state.limit;
    const parts = [];
    if (known.plan || known.email) {
      parts.push(`<div class="cc-acct"><span class="cc-avatar">${esc((known.email || known.plan || "?")[0].toUpperCase())}</span>` +
        `<span class="cc-acct-text"><b>${esc(known.plan || ENGINES[engineNow()]?.name || "Claude")}</b><span>${esc(known.email)}</span></span></div>`);
    }
    if (limit && limit.used > 0) {
      const pct = Math.round(limit.used * 100);
      const window = limit.window === "five_hour" ? "5-hour limit" : limit.window === "seven_day" ? "Weekly limit" : "Usage limit";
      const resets = limit.resets ? new Date(limit.resets * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "";
      parts.push(`<div class="cc-limit${limit.status !== "allowed" ? " warn" : ""}" title="${esc(window)} · ${pct}% used${resets ? ` · resets ${esc(resets)}` : ""}">` +
        `<div class="cc-limit-bar"><i style="width:${Math.min(100, pct)}%"></i></div>` +
        `<span>${esc(window)} · ${pct}%${resets ? ` · resets ${esc(resets)}` : ""}</span></div>`);
    }
    els.account.innerHTML = parts.join("");
  }

  /* ── the conversation ─────────────────────────────────────────── */

  /** Lines of a structured patch as a diff, coloured for the file they are in.
   *  @param {any[]} hunks @param {string} [lang] */
  function diffHtml(hunks, lang = "") {
    if (!Array.isArray(hunks) || !hunks.length) return "";
    let rows = "";
    for (const hunk of hunks) {
      let oldLine = hunk.oldStart ?? 1, newLine = hunk.newStart ?? 1;
      rows += `<tr class="hunk"><td colspan="3">@@ −${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@</td></tr>`;
      /** @type {string[]} */
      const lines = hunk.lines ?? [];
      // Each side coloured whole, the file's language, then taken a row at a
      // time: a comment that spans lines is still one.
      const before = highlightLines(lines.filter((l) => l[0] !== "+").map((l) => l.slice(1)).join("\n"), lang);
      const after = highlightLines(lines.filter((l) => l[0] !== "-").map((l) => l.slice(1)).join("\n"), lang);
      let b = 0, a = 0;
      for (const line of lines) {
        const mark = line[0];
        if (mark === "-") {
          rows += `<tr class="del"><td class="n">${oldLine++}</td><td class="m">−</td><td>${before[b++] ?? ""}</td></tr>`;
        } else if (mark === "+") {
          rows += `<tr class="add"><td class="n">${newLine++}</td><td class="m">+</td><td>${after[a++] ?? ""}</td></tr>`;
        } else {
          // "\ No newline at end of file" is a note about the line above, not code.
          const text = mark === "\\" ? esc(line) : after[a] ?? "";
          rows += `<tr><td class="n">${newLine++}</td><td class="m"></td><td>${text}</td></tr>`;
          a++, b++;
          oldLine++;
        }
      }
    }
    return `<div class="cc-diff"><table>${rows}</table></div>`;
  }

  /** A before/after for an edit that has not happened yet.
   *  @param {string} before @param {string} after @param {string} [lang] */
  function proposedDiff(before, after, lang = "") {
    const lines = [];
    for (const l of String(before ?? "").split("\n")) lines.push(`-${l}`);
    for (const l of String(after ?? "").split("\n")) lines.push(`+${l}`);
    return diffHtml([{ oldStart: 1, oldLines: 0, newStart: 1, newLines: 0, lines }], lang).replace(/<tr class="hunk">.*?<\/tr>/, "");
  }

  /** @param {any[]} hunks */
  function counts(hunks) {
    let add = 0, del = 0;
    for (const hunk of hunks ?? []) for (const line of hunk.lines ?? []) {
      if (line[0] === "+") add++;
      else if (line[0] === "-") del++;
    }
    return { add, del };
  }

  /** The files a patch changes, each with its hunks. @param {any[]} files */
  function patchHtml(files) {
    return (files ?? []).map((f) => {
      const verb = f.kind === "add" ? "Create" : f.kind === "delete" ? "Delete" : f.movePath ? `Move to ${rel(f.movePath)} and edit` : "Edit";
      const c = counts(f.structuredPatch);
      return `<div class="cc-patch-file"><span>${esc(verb)}</span> <code>${esc(rel(f.path))}</code>` +
        `<span class="cc-patch-n">+${c.add} −${c.del}</span></div>` + diffHtml(f.structuredPatch ?? [], language(f.path ?? ""));
    }).join("");
  }

  /** @param {string} text @param {string} lang @param {number} [max] */
  function code(text, lang, max = 60) {
    const lines = String(text ?? "").split("\n");
    const shown = lines.slice(0, max).join("\n");
    const more = lines.length > max ? `<div class="cc-more">… ${lines.length - max} more lines</div>` : "";
    return `<div class="md-code"><div class="md-code-head"><span>${esc(lang || "text")}</span><button class="md-copy" data-copy title="Copy">Copy</button></div>` +
      `<pre><code${lang ? ` data-lang="${esc(lang)}"` : ""}>${highlight(shown, lang)}</code></pre>${more}</div>`;
  }

  /** What one tool row says: a verb, what it acted on, and a note. @param {Tool} t */
  function summary(t) {
    const input = /** @type {any} */ (t.input ?? {});
    const detail = /** @type {any} */ (t.detail ?? {});
    const name = t.name;
    if (name.startsWith("mcp__")) {
      const [, server, tool] = name.split("__");
      const arg = Object.values(input).find((v) => typeof v === "string") ?? "";
      return { verb: `${server.replace(/^claude_ai_/, "").replace(/_/g, " ")} › ${tool?.replace(/_/g, " ")}`, target: String(arg).slice(0, 120), note: "" };
    }
    switch (name) {
      case "Read": {
        const range = input.offset ? `lines ${input.offset}–${input.offset + (input.limit ?? 0)}` : detail.file?.numLines ? `${detail.file.numLines} lines` : "";
        return { verb: "Read", target: rel(input.file_path), note: range, mono: true };
      }
      case "Write": {
        const hunks = detail.structuredPatch;
        const c = counts(hunks);
        const note = detail.type === "create" ? `${String(input.content ?? "").split("\n").length} lines` : hunks?.length ? `+${c.add} −${c.del}` : "";
        return { verb: detail.type === "update" ? "Update" : "Write", target: rel(input.file_path), note, mono: true };
      }
      case "Edit": case "MultiEdit": {
        const c = counts(detail.structuredPatch);
        return { verb: "Update", target: rel(input.file_path), note: detail.structuredPatch ? `+${c.add} −${c.del}` : "", mono: true };
      }
      case "NotebookEdit": return { verb: "Edit notebook", target: rel(input.notebook_path), note: input.edit_mode ?? "", mono: true };
      case "Patch": {
        const files = Array.isArray(detail.files) ? detail.files : Array.isArray(input.files) ? input.files : [];
        let add = 0, del = 0;
        for (const f of files) { const c = counts(f.structuredPatch); add += c.add; del += c.del; }
        const one = files.length === 1 ? files[0] : null;
        return {
          verb: one?.kind === "add" ? "Write" : one?.kind === "delete" ? "Delete" : "Update",
          target: one ? rel(one.path) : `${files.length} files`,
          note: add || del ? `+${add} −${del}` : "",
          mono: Boolean(one),
        };
      }
      case "Bash": return { verb: "Bash", target: String(input.command ?? "").split("\n")[0], note: input.run_in_background ? "in the background" : "", mono: true };
      case "BashOutput": case "KillShell": case "KillBash": return { verb: name === "BashOutput" ? "Shell output" : "Stop shell", target: String(input.bash_id ?? input.shell_id ?? ""), note: "", mono: true };
      case "Glob": return { verb: "Glob", target: String(input.pattern ?? ""), note: detail.numFiles != null ? `${detail.numFiles} files` : "", mono: true };
      case "Grep": return { verb: "Search", target: String(input.pattern ?? ""), note: detail.numFiles != null ? `${detail.numFiles} files` : detail.numLines != null ? `${detail.numLines} lines` : "", mono: true };
      case "WebFetch": return { verb: "Fetch", target: String(input.url ?? ""), note: "" };
      case "WebSearch": return { verb: "Web search", target: String(input.query ?? ""), note: "" };
      case "Task": case "Agent": return { verb: input.subagent_type && input.subagent_type !== "general-purpose" ? `${input.subagent_type}` : "Task", target: String(input.description ?? ""), note: "" };
      case "Skill": return { verb: "Skill", target: String(input.skill ?? input.name ?? input.command ?? ""), note: "" };
      case "Artifact": return { verb: "Artifact", target: String(input.title ?? input.action ?? (input.file_path ? rel(input.file_path) : "")), note: "" };
      case "ArtifactComments": return { verb: "Artifact comments", target: String(input.action ?? ""), note: "" };
      case "ArtifactData": return { verb: "Artifact data", target: String(input.action ?? ""), note: "" };
      case "LSP": return { verb: "LSP", target: `${input.operation ?? ""} ${rel(input.filePath ?? "")}`.trim(), note: "" };
      case "EnterPlanMode": return { verb: "Plan mode", target: "", note: "" };
      default: {
        const arg = Object.values(input).find((v) => typeof v === "string") ?? "";
        return { verb: name || "Tool", target: String(arg).split("\n")[0].slice(0, 120), note: "" };
      }
    }
  }

  /** What a tool row opens into. @param {Tool} t */
  function detailHtml(t) {
    const input = /** @type {any} */ (t.input ?? {});
    const detail = /** @type {any} */ (t.detail ?? {});
    const result = t.result ?? "";
    switch (t.name) {
      case "Edit": case "MultiEdit": {
        const lang = language(input.file_path ?? "");
        return diffHtml(detail.structuredPatch, lang) || (input.old_string != null ? proposedDiff(input.old_string, input.new_string, lang) : "");
      }
      case "Write":
        return detail.structuredPatch?.length ? diffHtml(detail.structuredPatch, language(input.file_path ?? "")) : code(input.content ?? "", language(input.file_path ?? ""));
      case "Read":
        return detail.file?.content ? code(detail.file.content, language(input.file_path ?? ""), 40) : result ? code(result, "") : "";
      case "Bash": {
        const out = [detail.stdout, detail.stderr].filter((s) => s && String(s).trim()).join("\n");
        const shown = out || result;
        return (input.description ? `<div class="cc-note">${esc(input.description)}</div>` : "") +
          code(input.command ?? "", "bash", 12) + (shown ? `<pre class="cc-output${t.state === "failed" ? " err" : ""}">${esc(shown)}</pre>` : "");
      }
      case "Task": case "Agent":
        return input.prompt ? `<div class="cc-prompt">${markdown(String(input.prompt))}</div>` : "";
      case "Patch":
        return patchHtml(detail.files ?? []);
      default: {
        const json = Object.keys(input).length ? code(JSON.stringify(input, null, 2), "json", 30) : "";
        return json + (result ? `<pre class="cc-output${t.state === "failed" ? " err" : ""}">${esc(result)}</pre>` : "");
      }
    }
  }

  /** The question a tool is waiting on, as a card to answer. @param {Tool} t */
  function askHtml(t) {
    const ask = t.ask;
    if (!ask) return "";
    const input = /** @type {any} */ (t.input ?? {});
    const req = esc(ask.request);
    if (ask.kind === "question") {
      const questions = Array.isArray(input.questions) ? input.questions : [];
      return `<form class="cc-ask cc-question" data-claude-question="${req}">` +
        questions.map((/** @type {any} */ qn, /** @type {number} */ qi) => {
          const type = qn.multiSelect ? "checkbox" : "radio";
          const name = `q${qi}`;
          return `<fieldset data-question="${esc(qn.question)}">` +
            (qn.header ? `<span class="cc-chip">${esc(qn.header)}</span>` : "") +
            `<legend>${esc(qn.question)}</legend>` +
            (qn.options ?? []).map((/** @type {any} */ o) =>
              `<label class="cc-option"><input type="${type}" name="${name}" value="${esc(o.label)}">` +
              `<span><b>${esc(o.label)}</b>${o.description ? `<small>${esc(o.description)}</small>` : ""}</span></label>`).join("") +
            `<label class="cc-option other"><input type="${type}" name="${name}" value="\u0000other"><span><input class="cc-other" placeholder="Something else…" data-other></span></label>` +
            `</fieldset>`;
        }).join("") +
        `<div class="cc-ask-acts"><button type="submit" class="cc-primary">Submit</button>` +
        `<button type="button" class="cc-quiet" data-claude-deny="${req}">Skip</button></div></form>`;
    }
    if (ask.kind === "plan") {
      return `<div class="cc-ask cc-plan"><div class="cc-ask-q">${icon("bulb")} Here is ${esc(agent())}'s plan</div>` +
        `<div class="cc-plan-body">${markdown(String(input.plan ?? ""))}</div>` +
        `<div class="cc-ask-acts">` +
        `<button class="cc-primary" data-claude-allow="${req}" data-remember="1">Yes, and accept edits</button>` +
        `<button class="cc-secondary" data-claude-allow="${req}">Yes, ask before edits</button>` +
        `<button class="cc-quiet" data-claude-deny="${req}">Keep planning</button></div>` +
        `<input class="cc-deny-why" data-claude-why placeholder="Tell ${esc(agent())} what to change (optional)"></div>`;
    }
    const who = esc(agent());
    const detail = /** @type {any} */ (t.detail ?? {});
    let question = `Allow ${who} to use <b>${esc(t.name)}</b>?`;
    let preview = "";
    switch (t.name) {
      case "Bash":
        question = `Allow ${who} to run this command?`;
        preview = code(input.command ?? "", "bash", 20) + (input.description ? `<div class="cc-note">${esc(input.description)}</div>` : "");
        break;
      case "Edit": case "MultiEdit":
        question = `Allow ${who} to edit <code>${esc(rel(input.file_path))}</code>?`;
        preview = Array.isArray(input.edits)
          ? input.edits.map((/** @type {any} */ e) => proposedDiff(e.old_string, e.new_string, language(input.file_path ?? ""))).join("")
          : proposedDiff(input.old_string, input.new_string, language(input.file_path ?? ""));
        break;
      case "Write":
        question = `Allow ${who} to write <code>${esc(rel(input.file_path))}</code>?`;
        preview = code(input.content ?? "", language(input.file_path ?? ""), 24);
        break;
      case "NotebookEdit":
        question = `Allow ${who} to edit <code>${esc(rel(input.notebook_path))}</code>?`;
        preview = code(input.new_source ?? "", input.cell_type === "markdown" ? "" : "python", 24);
        break;
      case "WebFetch":
        question = `Allow ${who} to fetch <code>${esc(input.url ?? "")}</code>?`;
        break;
      case "Patch": {
        const files = Array.isArray(detail.files) ? detail.files : [];
        question = files.length === 1
          ? `Allow ${who} to ${files[0].kind === "add" ? "create" : files[0].kind === "delete" ? "delete" : "edit"} <code>${esc(rel(files[0].path))}</code>?`
          : `Allow ${who} to change ${files.length || "these"} files?`;
        preview = patchHtml(files);
        break;
      }
      default:
        preview = Object.keys(input).length ? code(JSON.stringify(input, null, 2), "json", 20) : "";
    }
    const suggestion = (ask.suggestions ?? [])[0];
    let always = "";
    if (suggestion) {
      const s = /** @type {any} */ (suggestion);
      const label = s.type === "setMode" && s.mode === "acceptEdits" ? "Yes, and accept edits this session"
        : s.type === "addRules" && Array.isArray(s.rules) && s.rules[0]
          ? `Yes, and don't ask again for <code>${esc(s.rules[0].ruleContent ? `${s.rules[0].toolName}(${s.rules[0].ruleContent})` : s.rules[0].toolName)}</code>`
          : "Yes, and don't ask again this session";
      always = `<button class="cc-secondary" data-claude-allow="${req}" data-remember="1">${label}</button>`;
    }
    return `<div class="cc-ask"><div class="cc-ask-q">${question}</div>${preview}` +
      (ask.blocked ? `<div class="cc-note">${esc(ask.blocked)}</div>` : "") +
      `<div class="cc-ask-acts"><button class="cc-primary" data-claude-allow="${req}">Yes</button>${always}` +
      `<button class="cc-quiet" data-claude-deny="${req}">No</button></div>` +
      `<input class="cc-deny-why" data-claude-why placeholder="Or tell ${esc(agent())} what to do instead"></div>`;
  }

  /** @param {any} artifact */
  function artifactIcon(artifact) {
    const word = String(artifact?.icon ?? "").toLowerCase();
    return ARTIFACT_ICONS[/** @type {keyof typeof ARTIFACT_ICONS} */ (word)] ?? "layers";
  }

  /** @param {Tool} t */
  function artifactCard(t) {
    const a = t.artifact;
    if (!a) return "";
    return `<button class="cc-artifact" data-claude-artifact="${esc(a.id)}" data-seq="${a.seq}">` +
      `<span class="cc-artifact-icon">${icon(artifactIcon(a))}</span>` +
      `<span class="cc-artifact-text"><b>${esc(a.title)}</b><small>Artifact · version ${a.seq}</small></span>` +
      `<span class="cc-artifact-open">Open</span></button>`;
  }

  /** @param {Tool} t */
  function toolHtml(t) {
    if (t.ask && (t.ask.kind === "question" || t.ask.kind === "plan")) {
      return askHtml(t);
    }
    if (t.name === "AskUserQuestion" && t.state !== "waiting") {
      const answers = /** @type {any} */ (t.detail)?.answers ?? {};
      const lines = Object.entries(answers).map(([qn, a]) => `<div><span>${esc(qn)}</span> <b>${esc(String(a))}</b></div>`).join("");
      return `<div class="cc-answered">${icon("check")}<div>${lines || "No answer"}</div></div>`;
    }
    if (t.name === "ExitPlanMode" && t.state !== "waiting") {
      const approved = t.state === "done";
      return `<details class="cc-plan-done"><summary>${icon("bulb")} ${approved ? "Plan approved" : "Plan not approved"}</summary>` +
        `<div class="cc-plan-body">${markdown(String(/** @type {any} */ (t.input)?.plan ?? ""))}</div></details>`;
    }
    const s = summary(t);
    const open = state.open.has(t.id);
    const stateLabel = t.state === "denied" ? `<span class="cc-flag">Denied</span>` : t.state === "stopped" ? `<span class="cc-flag">Stopped</span>` : "";
    const streaming = t.state === "streaming" && t.streamed ? `<span class="cc-note-inline">${compact(t.streamed)} chars</span>` : "";
    const children = t.name === "Task" || t.name === "Agent" ? `<div class="cc-children" data-children="${esc(t.id)}"></div>` : "";
    return `<div class="cc-tool-row" data-toggle="${esc(t.id)}" role="button" tabindex="0" aria-expanded="${open}">` +
      `<span class="cc-dot ${t.state}"></span>` +
      `<span class="cc-verb">${esc(s.verb)}</span>` +
      (s.target ? `<span class="cc-target${s.mono ? " mono" : ""}">${esc(s.target)}</span>` : "") +
      (s.note ? `<span class="cc-note-inline">${esc(s.note)}</span>` : "") + streaming + stateLabel +
      `<span class="cc-chev">${icon("chev")}</span></div>` +
      (open ? `<div class="cc-tool-detail">${detailHtml(t)}</div>` : "") +
      askHtml(t) + artifactCard(t) + children;
  }

  /** @param {Item} item */
  function itemHtml(item) {
    switch (item.kind) {
      case "user":
        return `<div class="cc-bubble">${esc(item.text).replace(/\n/g, "<br>")}` +
          (item.images ? `<span class="cc-images">${icon("image")} ${item.images} image${item.images === 1 ? "" : "s"}</span>` : "") + `</div>`;
      case "text":
        return `<div class="cc-prose">${markdown(item.text, { streaming: !item.done })}${item.done ? "" : `<span class="cc-caret"></span>`}</div>`;
      case "thinking": {
        const label = item.done ? "Thought" : "Thinking";
        const tokens = item.tokens ? `<span class="cc-note-inline">${compact(item.tokens)} tokens</span>` : "";
        const body = item.text.trim() ? markdown(item.text) : `<p class="cc-faint">This model does not show its thinking.</p>`;
        return `<details class="cc-think${item.done ? "" : " live"}"${state.open.has(item.id) ? " open" : ""} data-think="${esc(item.id)}">` +
          `<summary><span class="cc-star">✻</span> ${label}${tokens}</summary><div class="cc-think-body">${body}</div></details>`;
      }
      case "tool":
        return toolHtml(item);
      case "notice":
        return `<div class="cc-notice ${item.level}">${esc(item.text)}</div>`;
      case "turn": {
        if (item.error) return `<div class="cc-notice error">${esc(item.error)}</div>`;
        const cost = !caps().plan && item.cost ? ` · $${item.cost.toFixed(3)}` : "";
        return `<div class="cc-turn"><span class="cc-star">✻</span> Worked for ${duration(item.ms)}${cost}</div>`;
      }
      default:
        return "";
    }
  }

  /** @param {Item} item */
  function itemClass(item) {
    if (item.kind === "tool") {
      const quiet = QUIET.has(item.name) && !item.ask;
      return `cc-item cc-tool ${item.state}${quiet ? " quiet" : ""}${item.name === "Task" || item.name === "Agent" ? " task" : ""}`;
    }
    return `cc-item cc-${item.kind}`;
  }

  /** The element an item is drawn into, made if it is new. @param {Item} item */
  function slot(item) {
    let el = /** @type {HTMLElement | null} */ (els.items.querySelector(`[data-item="${CSS.escape(item.id)}"]`));
    if (el) return el;
    el = doc.createElement("div");
    el.dataset.item = item.id;
    const parent = "parent" in item ? item.parent : null;
    const box = parent ? els.items.querySelector(`[data-children="${CSS.escape(parent)}"]`) : null;
    (box ?? els.items).append(el);
    return el;
  }

  /** Where streaming prose's settled part ends, for each item still
   * streaming: what comes before its last paragraph break outside a code
   * fence cannot change any more, so it is drawn once and kept, and only the
   * paragraph still arriving is drawn again each frame — a long answer cost
   * its whole length to draw on every frame of it. Drawn whole once it is
   * done, so what stays on screen is exactly what a finished answer draws.
   * @type {Map<string, number>} */
  const settledTo = new Map();

  /** Where the settled part of `text` ends, scanning on from `from`, which
   * is outside any fence. @param {string} text @param {number} from */
  function settleAt(text, from) {
    let fence = false;
    let cut = from;
    let at = from;
    for (;;) {
      const end = text.indexOf("\n", at);
      // The last line is still arriving.
      if (end < 0) break;
      const line = text.slice(at, end);
      if (/^\s*(```|~~~)/.test(line)) fence = !fence;
      else if (!fence && !line.trim()) cut = end + 1;
      at = end + 1;
    }
    return cut;
  }

  /** Streaming prose, a settled block at a time. @param {HTMLElement} el @param {Item & {kind: "text"}} item */
  function drawStreaming(el, item) {
    let prose = el.querySelector(":scope > .cc-prose.live");
    const from = prose ? settledTo.get(item.id) ?? 0 : 0;
    if (!prose || from > item.text.length) {
      el.innerHTML = `<div class="cc-prose live"><div data-settled></div><div data-tail></div><span class="cc-caret"></span></div>`;
      prose = el.querySelector(":scope > .cc-prose.live");
      settledTo.delete(item.id);
      return drawStreaming(el, item);
    }
    const cut = settleAt(item.text, from);
    if (cut > from) prose.querySelector("[data-settled]")?.insertAdjacentHTML("beforeend", markdown(item.text.slice(from, cut)));
    settledTo.set(item.id, cut);
    const tail = prose.querySelector("[data-tail]");
    if (tail) tail.innerHTML = markdown(item.text.slice(cut), { streaming: true });
  }

  /** @param {Item} item */
  function draw(item) {
    const el = slot(item);
    el.className = itemClass(item);
    if (item.kind === "text" && !item.done) return drawStreaming(el, item);
    settledTo.delete(item.id);
    // A form being filled in is not redrawn under the person filling it.
    if (el.querySelector("form.cc-question") && item.kind === "tool" && item.ask) return;
    // Keep what is focused or typed into an answer card across a redraw.
    const why = /** @type {HTMLInputElement | null} */ (el.querySelector("[data-claude-why]"));
    const kept = why?.value ?? "";
    const children = el.querySelector(":scope > [data-children]");
    el.innerHTML = itemHtml(item);
    if (children) el.querySelector(":scope > [data-children]")?.replaceWith(children);
    if (kept) {
      const fresh = /** @type {HTMLInputElement | null} */ (el.querySelector("[data-claude-why]"));
      if (fresh) fresh.value = kept;
    }
  }

  const nearBottom = () => !els.thread || els.thread.scrollHeight - els.thread.scrollTop - els.thread.clientHeight < 120;
  const toBottom = () => { if (els.thread) els.thread.scrollTop = els.thread.scrollHeight; };

  /** What the empty conversation says, in the words of the engine it is with. */
  const WELCOME = /** @type {Record<Engine, string>} */ ({
    claude: "Claude Code, in this project. It reads, edits and runs things here, asks before anything you have not allowed, and what it publishes opens beside the chat.",
    codex: "Codex, in this project, on your ChatGPT plan. It works in a sandbox, asks before anything its mode does not allow, and keeps its own thread to pick up again.",
    gemini: "The Gemini CLI, in this project, on your Google sign-in. It asks before edits and commands unless its mode allows them.",
    antigravity: "Antigravity, in this project. It cannot stop to ask, so its mode decides what it may do, and what it was refused is said when it finishes.",
  });

  function renderWelcome() {
    if (!els.welcome) return;
    els.welcome.hidden = state.order.length > 0;
    const engine = engineNow();
    if (els.welcomeTitle) els.welcomeTitle.textContent = `What should ${ENGINES[engine]?.short ?? "Claude"} work on?`;
    if (els.welcomeText) {
      const missing = state.engines.get(engine)?.missing;
      els.welcomeText.textContent = missing || WELCOME[engine] || WELCOME.claude;
      els.welcomeText.classList.toggle("cc-missing", Boolean(missing));
    }
  }

  /** @param {{session: string, items: Item[], reset: boolean, earlier: boolean}} event */
  function items(event) {
    const stick = nearBottom();
    if (event.reset || event.session !== state.session) {
      const switched = event.session !== state.session;
      state.session = event.session;
      // Each conversation has its own pane: what it had open comes back with
      // it, and one that never opened anything has it closed.
      if (switched) restorePane();
      state.items.clear();
      state.order = [];
      els.items.innerHTML = "";
      state.live = false;
      if (els.earlier) els.earlier.hidden = !event.earlier;
    }
    for (const item of event.items) {
      const fresh = !state.items.has(item.id);
      const before = state.items.get(item.id);
      state.items.set(item.id, item);
      if (fresh) state.order.push(item.id);
      draw(item);
      // An artifact published while you watch opens beside the chat, as it
      // does in Claude Code; one read back from history waits to be asked.
      if (!event.reset && item.kind === "tool" && item.artifact && !(before?.kind === "tool" && before.artifact?.seq === item.artifact.seq)) {
        openArtifact(item.artifact.id, item.artifact.seq, item.artifact);
      }
    }
    if (!event.reset) state.live = true;
    renderWelcome();
    renderWorking();
    if (stick || event.reset) requestAnimationFrame(toBottom);
  }

  /* ── where it stands ──────────────────────────────────────────── */

  let ticking = 0;
  function renderWorking() {
    const s = state.status;
    const el = els.working;
    if (!el) return;
    const activity = s?.activity ?? "idle";
    const working = activity === "working" || activity === "starting";
    const waiting = activity === "waiting";
    el.hidden = !(working || waiting);
    if (working) {
      const elapsed = s?.since ? duration(Date.now() - s.since) : "";
      const what = activity === "starting" ? "Starting Claude Code" : s?.doing || state.verb;
      el.innerHTML = `<span class="cc-star spin">✻</span><span class="cc-shimmer">${esc(what)}…</span>` +
        `<span class="cc-note-inline">${elapsed ? `${elapsed} · ` : ""}esc to interrupt</span>`;
    } else if (waiting) {
      el.innerHTML = `<span class="cc-star">✻</span><span>Waiting for you</span>`;
    }
    clearInterval(ticking);
    if (working) ticking = window.setInterval(renderWorking, 1000);
  }

  function renderTasks() {
    const el = els.tasks;
    if (!el) return;
    const tasks = state.status?.tasks ?? [];
    const open = tasks.filter((t) => t.status !== "completed").length;
    el.hidden = !tasks.length || (!open && state.status?.activity === "idle");
    if (el.hidden) return;
    const done = tasks.length - open;
    el.innerHTML = `<div class="cc-tasks-head">${icon("check")} Tasks <span>${done}/${tasks.length}</span></div>` +
      `<ul>${tasks.map((t) => {
        const mark = t.status === "completed" ? "✓" : t.status === "in_progress" ? "" : "○";
        const text = t.status === "in_progress" && t.active ? t.active : t.subject;
        return `<li class="${esc(t.status)}"><span class="cc-check">${t.status === "in_progress" ? `<span class="cc-spin"></span>` : mark}</span>${esc(text)}</li>`;
      }).join("")}</ul>`;
  }

  function modelName() {
    const s = state.status;
    const models = caps().models ?? [];
    const current = s?.model ?? "";
    const found = models.find((m) => m.resolved && m.resolved === current && m.value !== "default") ?? models.find((m) => m.value === current);
    if (found) return found.name.replace(/\s*\(recommended\)/i, "");
    if (current) return current.replace(/^claude-/, "").replace(/-(\d+)-(\d+)$/, " $1.$2").replace(/-\d{8}$/, "").replace(/^\w/, (c) => c.toUpperCase());
    return models[0]?.name?.replace(/\s*\(recommended\)/i, "") ?? "Default";
  }

  function renderComposerState() {
    const s = state.status;
    const engine = engineNow();
    const all = modes();
    const mode = all.find((m) => m.value === (s?.mode === "manual" ? "default" : s?.mode)) ?? all[0];
    if (els.engine) {
      els.engine.innerHTML = `${icon(ENGINES[engine]?.icon ?? "spark")}<span>${esc(ENGINES[engine]?.short ?? engine)}</span>${icon("chev")}`;
      els.engine.title = `${ENGINES[engine]?.name ?? engine} answers here. Choose another agent`;
      els.engine.dataset.engine = engine;
    }
    if (els.mode) {
      els.mode.innerHTML = `${icon(mode.icon)}<span>${esc(mode.name)}</span>`;
      els.mode.dataset.mode = mode.value;
    }
    if (els.model) {
      const effort = s?.effort ? ` · ${s.effort}` : "";
      els.model.innerHTML = `<span>${esc(modelName())}${esc(effort)}</span>${icon("chev")}`;
    }
    const busy = s && s.activity !== "idle";
    const empty = !els.input?.value.trim() && !state.attachments.length;
    if (els.send) {
      const stop = busy && empty;
      els.send.classList.toggle("stop", Boolean(stop));
      els.send.innerHTML = stop ? icon("stop") : icon("arrow-up");
      els.send.title = stop ? "Stop  esc" : "Send  ⏎";
      els.send.setAttribute("aria-label", stop ? "Stop" : "Send");
      els.send.toggleAttribute("disabled", !stop && empty);
    }
    if (els.meter) {
      const context = s?.context ?? 0;
      els.meter.textContent = context ? `${compact(context)} context` : "";
      els.meter.title = context ? `The last request carried ${context.toLocaleString()} tokens of context.` : "";
    }
    if (els.input) {
      const who = agent();
      els.input.placeholder = state.order.length ? `Reply to ${who}…` : `Ask ${who} to build, fix or explain something…`;
      els.input.setAttribute("aria-label", `Message ${who}`);
    }
  }

  /** @param {Status} status */
  function status(status) {
    if (status.session !== state.session && state.session) {
      // A status for a session not on screen is the list's business only.
      return;
    }
    const was = state.status?.activity;
    const engineWas = engineNow();
    state.status = status;
    // A limit is the engine's: another engine's conversation drops the last
    // one's until it says its own.
    const switched = engineNow() !== engineWas;
    if (status.limit || switched) {
      state.limit = status.limit ?? null;
      renderAccount();
    }
    if (switched) renderWelcome();
    if (was !== "working" && status.activity === "working") state.verb = VERBS[Math.floor(Math.random() * VERBS.length)];
    if (els.title) els.title.textContent = status.title || "New session";
    renderWorking();
    renderTasks();
    renderComposerState();
  }

  /** @param {{sessions: Session[], folders?: Folder[], active: string, busy: string[], waiting: string[], engine?: Engine}} event */
  function sessions(event) {
    state.folders = event.folders ?? [];
    // A folder just asked for is named in place as soon as it exists.
    const made = state.expectFolder ? state.folders.find((f) => !state.knownFolders.has(f.id)) : undefined;
    state.knownFolders = new Set(state.folders.map((f) => f.id));
    if (made) {
      state.expectFolder = false;
      state.renaming = made.id;
      state.draft = made.name;
      state.fresh = true;
    } else if (state.renaming && !state.knownFolders.has(state.renaming)) {
      state.renaming = "";
    }
    if (event.engine && event.engine !== state.newEngine) {
      state.newEngine = event.engine;
      if (!state.status) {
        renderComposerState();
        renderWelcome();
        renderAccount();
      }
    }
    state.sessions = event.sessions;
    state.active = event.active;
    state.busy = new Set(event.busy);
    state.waiting = new Set(event.waiting);
    renderSessions();
  }

  /** What one engine offers. @param {Capabilities} offered */
  function capabilities(offered) {
    state.engines.set(offered.engine ?? "claude", offered);
    if ((offered.engine ?? "claude") !== engineNow()) return;
    renderAccount();
    renderComposerState();
    renderWelcome();
  }

  /* ── artifacts ────────────────────────────────────────────────── */

  const base = () => {
    if (!host.shell) return "/fixtures/artifacts/";
    return /Win/.test(navigator.platform) ? "http://artifact.localhost/" : "artifact://localhost/";
  };

  /** @param {{artifacts: Artifact[], importing: boolean}} event */
  function artifacts(event) {
    const list = event.artifacts;
    const wasImporting = state.importing;
    state.artifacts = list;
    state.importing = Boolean(event.importing);
    // What a conversation off screen publishes is waiting open in its pane
    // when it is next looked at. An import brings in the past, and opens
    // nothing.
    const latest = new Map(list.map((a) => [a.id, a.versions[a.versions.length - 1]?.seq ?? 0]));
    if (state.latest && !wasImporting && !state.importing) {
      for (const a of list) {
        const seq = latest.get(a.id) ?? 0;
        if (a.session && a.session !== state.session && seq > (state.latest.get(a.id) ?? 0)) {
          recordPane({ open: true, shown: { id: a.id, seq }, gallery: false }, a.session);
        }
      }
    }
    state.latest = latest;
    renderCount();
    // A pane put back before the list arrived — on connect, the conversation
    // comes first — is drawn again now its artifact is known.
    if (state.shown && unknown && list.some((a) => a.id === state.shown?.id)) present(state.shown.id, state.shown.seq);
    if (state.shown) {
      const a = list.find((x) => x.id === state.shown?.id);
      if (a) renderVersions(a);
    }
    renderGallery();
  }

  /** @param {Artifact} a */
  function renderVersions(a) {
    if (!els.artVersion) return;
    els.artVersion.innerHTML = [...a.versions].reverse()
      .map((v) => `<option value="${v.seq}"${v.seq === state.shown?.seq ? " selected" : ""}>Version ${v.seq}</option>`).join("");
    els.artVersion.hidden = a.versions.length < 2;
  }

  /** This conversation's artifacts, on the pane's toggle. */
  function renderCount() {
    if (!els.artCount) return;
    const mine = state.artifacts.filter((a) => a.session && a.session === state.session).length;
    els.artCount.textContent = mine ? String(mine) : "";
    els.artCount.hidden = !mine;
  }

  /** Remember what a conversation's pane shows. @param {Partial<Pane>} patch @param {string} [session] */
  function recordPane(patch, session = state.session) {
    if (!session) return;
    const was = state.panes.get(session) ?? { open: false, shown: null, gallery: false };
    state.panes.delete(session);
    state.panes.set(session, { ...was, ...patch });
    savePanes(state.panes);
  }

  const emptyHtml = els.artEmpty?.innerHTML ?? "";

  /** The pane with nothing in it. */
  function clearPane() {
    state.shown = null;
    delete ws.dataset.artifact;
    if (els.artTitle) els.artTitle.textContent = "Artifacts";
    if (els.artIcon) els.artIcon.innerHTML = icon("layers");
    if (els.artVersion) els.artVersion.hidden = true;
    if (els.artFrame) {
      els.artFrame.hidden = true;
      if (els.artFrame.getAttribute("src")) els.artFrame.setAttribute("src", "about:blank");
    }
    if (els.artDoc) els.artDoc.hidden = true;
    if (els.artEmpty) {
      els.artEmpty.hidden = false;
      els.artEmpty.innerHTML = emptyHtml;
    }
    if (els.gallery) els.gallery.hidden = true;
  }

  /** Put the pane back as the conversation now on screen left it. */
  function restorePane() {
    const pane = state.panes.get(state.session);
    clearPane();
    if (pane?.shown) present(pane.shown.id, pane.shown.seq);
    if (pane?.gallery && els.gallery) els.gallery.hidden = false;
    renderGallery();
    renderCount();
    restoring = true;
    if (pane?.open) host.panels.show("right", ws);
    else host.panels.hide?.("right", ws);
    restoring = false;
  }

  /** Whether the pane is being put back rather than opened or closed by hand. */
  let restoring = false;

  // However the pane is opened or closed — its own buttons, the title bar,
  // a shortcut — this conversation remembers it so.
  new MutationObserver(() => {
    if (!restoring) recordPane({ open: ws.dataset.rightOpen === "true" });
  }).observe(ws, { attributes: true, attributeFilter: ["data-right-open"] });

  /** @param {Artifact} a */
  function galleryRow(a) {
    const latest = a.versions[a.versions.length - 1];
    const when = latest ? new Date(latest.at).toLocaleDateString([], { month: "short", day: "numeric" }) : "";
    return `<div class="cc-gallery-row${a.id === state.shown?.id ? " on" : ""}">` +
      `<button class="cc-gallery-open" data-claude-artifact="${esc(a.id)}" data-seq="${latest?.seq ?? 1}">` +
      `<span class="cc-artifact-icon">${icon(artifactIcon(a))}</span>` +
      `<span class="cc-artifact-text"><b>${esc(a.title)}</b><small>${esc(a.description || `${a.versions.length} version${a.versions.length === 1 ? "" : "s"}`)} · ${esc(when)}</small></span></button>` +
      `<button class="hbtn" data-artifact-forget="${esc(a.id)}" title="Forget this artifact here (it stays on claude.ai)">${icon("close")}</button></div>`;
  }

  function renderGallery() {
    if (!els.gallery) return;
    const list = state.artifacts;
    // What was published before Proton kept it is in the desktop app's
    // records and the sessions' files; one click brings it in.
    const head = `<div class="cc-gallery-head"><span>${list.length ? `${list.length} artifact${list.length === 1 ? "" : "s"}` : "No artifacts yet"}</span>` +
      (state.importing
        ? `<span class="cc-importing"><span class="cc-spin"></span>Reading your sessions…</span>`
        : `<button class="cc-secondary" data-artifact-import title="Bring in what Claude Code published from this machine before Proton kept it">Import from Claude</button>`) +
      `</div>`;
    // This conversation's first, then everything else kept here.
    const mine = list.filter((a) => a.session && a.session === state.session);
    const rest = list.filter((a) => !mine.includes(a));
    els.gallery.innerHTML = head + (list.length
      ? (mine.length
        ? `<div class="cc-gallery-group">In this chat</div>${mine.map(galleryRow).join("")}` +
          (rest.length ? `<div class="cc-gallery-group">Other chats</div>${rest.map(galleryRow).join("")}` : "")
        : rest.map(galleryRow).join(""))
      : `<div class="cc-none">Nothing published yet. Ask Claude to make something — a page, a chart, a small app — and it opens here, or import what you have already published.</div>`);
  }

  /** Show an artifact in the pane, and open the pane on it. @param {string} id @param {number} seq @param {any} [ref] */
  function openArtifact(id, seq, ref) {
    present(id, seq, ref);
    if (els.gallery) els.gallery.hidden = true;
    renderGallery();
    host.panels.show("right", ws);
    recordPane({ open: true, shown: state.shown, gallery: false });
  }

  /** Whether the artifact in the pane was not yet in the list when it was put there. */
  let unknown = false;

  /** Load an artifact into the pane, open or not. @param {string} id @param {number} seq @param {any} [ref] */
  function present(id, seq, ref) {
    const a = state.artifacts.find((x) => x.id === id);
    unknown = !a && !ref;
    const version = a?.versions.find((v) => v.seq === seq) ?? a?.versions[a.versions.length - 1];
    const file = version?.file ?? ref?.file ?? "";
    const title = a?.title ?? ref?.title ?? "Artifact";
    const url = a?.url ?? ref?.url ?? "";
    state.shown = { id, seq: version?.seq ?? seq };
    ws.dataset.artifact = id;
    if (els.artTitle) els.artTitle.textContent = title;
    if (els.artIcon) els.artIcon.innerHTML = icon(artifactIcon(a ?? ref));
    ws.querySelector("[data-artifact-open]")?.setAttribute("data-url", url);
    if (a) renderVersions(a);
    else if (els.artVersion) els.artVersion.hidden = true;
    const src = file ? `${base()}${encodeURIComponent(id)}/${state.shown.seq}/${file.split("/").map(encodeURIComponent).join("/")}` : "";
    const isMarkdown = /\.(md|markdown)$/i.test(file);
    if (els.artEmpty) {
      els.artEmpty.hidden = Boolean(file);
      // Kept as an address alone: its page was gone from this machine by the
      // time it was imported, so claude.ai is where it is.
      if (!file) {
        els.artEmpty.innerHTML = `<p>This artifact's page is no longer on this machine, so it can only be opened where it was published.</p>` +
          (url ? `<button class="cc-primary" data-artifact-open data-url="${esc(url)}">Open on claude.ai</button>` : "");
      }
    }
    if (els.artFrame) {
      els.artFrame.hidden = !file || isMarkdown;
      if (file && !isMarkdown && els.artFrame.getAttribute("src") !== src) els.artFrame.setAttribute("src", src);
    }
    if (els.artDoc) {
      els.artDoc.hidden = !isMarkdown;
      if (isMarkdown) {
        els.artDoc.innerHTML = `<div class="cc-faint">Loading…</div>`;
        fetch(src).then((r) => r.text()).then((text) => { els.artDoc.innerHTML = markdown(text); })
          .catch(() => { els.artDoc.innerHTML = `<div class="cc-none">Could not read this document.</div>`; });
      }
    }
  }

  function showGallery() {
    if (!els.gallery) return;
    els.gallery.hidden = !els.gallery.hidden;
    renderGallery();
    if (!els.gallery.hidden) host.panels.show("right", ws);
    recordPane({ gallery: !els.gallery.hidden, ...(els.gallery.hidden ? {} : { open: true }) });
  }

  /* ── the composer ─────────────────────────────────────────────── */

  function autosize() {
    const box = els.input;
    if (!box) return;
    box.style.height = "auto";
    box.style.height = `${Math.min(box.scrollHeight, Math.round(window.innerHeight * 0.4))}px`;
  }

  /** The commands the slash menu offers: Claude Code's, then Proton's own. */
  function commands() {
    const own = [{ name: "council", description: "Put a question to every model you have a subscription to, and get a brief", hint: "<question>" }];
    return [...(caps().commands ?? []), ...own];
  }

  function renderMenu() {
    const menu = els.menu;
    if (!menu || !els.input) return;
    const match = els.input.value.match(/^\/([\w:-]*)$/);
    if (!match) {
      menu.hidden = true;
      return;
    }
    const term = match[1].toLowerCase();
    const found = commands().filter((c) => c.name.toLowerCase().includes(term))
      .sort((a, b) => Number(!a.name.toLowerCase().startsWith(term)) - Number(!b.name.toLowerCase().startsWith(term)))
      .slice(0, 8);
    if (!found.length) {
      menu.hidden = true;
      return;
    }
    state.menuIndex = Math.min(state.menuIndex, found.length - 1);
    menu.hidden = false;
    menu.innerHTML = found.map((c, i) =>
      `<button class="cc-menu-row${i === state.menuIndex ? " on" : ""}" data-claude-command="${esc(c.name)}">` +
      `<b>/${esc(c.name)}</b>${c.hint ? `<i>${esc(c.hint)}</i>` : ""}<span>${esc(c.description.split("\n")[0].slice(0, 120))}</span></button>`).join("");
  }

  /** @param {string} name */
  function takeCommand(name) {
    if (!els.input) return;
    els.input.value = `/${name} `;
    els.menu.hidden = true;
    els.input.focus();
    autosize();
    renderComposerState();
  }

  function renderAttachments() {
    const box = els.attachments;
    if (!box) return;
    box.hidden = !state.attachments.length;
    box.innerHTML = state.attachments.map((a, i) =>
      `<span class="cc-attachment"><img src="data:${esc(a.media)};base64,${a.data}" alt=""><span>${esc(a.name)}</span>` +
      `<button class="hbtn" data-claude-unattach="${i}" title="Remove">${icon("close")}</button></span>`).join("");
  }

  /** @param {File} file */
  function attach(file) {
    if (!/^image\/(png|jpeg|gif|webp)$/.test(file.type) || file.size > 5 * 1024 * 1024) return;
    const reader = new FileReader();
    reader.onload = () => {
      const data = String(reader.result).split(",")[1] ?? "";
      state.attachments.push({ media: file.type, data, name: file.name || "pasted image" });
      renderAttachments();
      renderComposerState();
    };
    reader.readAsDataURL(file);
  }

  function send() {
    const box = els.input;
    if (!box) return;
    const text = box.value.trim();
    const busy = state.status && state.status.activity !== "idle";
    if (!text && !state.attachments.length) {
      if (busy) host.dispatch({ type: "claudeStop" });
      return;
    }
    const council = text.match(/^\/council\s+([\s\S]+)/);
    if (council) {
      host.convene(council[1].trim());
    } else {
      host.dispatch({ type: "claudeSend", text, images: state.attachments.map(({ media, data }) => ({ media, data })) });
    }
    box.value = "";
    state.attachments = [];
    renderAttachments();
    autosize();
    renderMenu();
    renderComposerState();
    showChat();
    requestAnimationFrame(toBottom);
  }

  /** The conversation, rather than a debate, on the stage. */
  function showChat() {
    const view = /** @type {HTMLElement | null} */ (ws.querySelector("[data-council-view]"));
    if (view && !view.hidden) {
      /** @type {HTMLElement} */ (view).hidden = true;
      els.thread.hidden = false;
    }
  }

  /** A small menu above a pill. @param {HTMLElement} anchor @param {string} html */
  function popover(anchor, html) {
    const pop = els.pop;
    if (!pop) return;
    if (!pop.hidden && pop.dataset.for === anchor.dataset.pop) {
      pop.hidden = true;
      return;
    }
    pop.innerHTML = html;
    pop.dataset.for = anchor.dataset.pop ?? "";
    pop.hidden = false;
    // Placed in the box it is positioned in — the stage, not the whole tab,
    // which starts a sidebar further left and would put it off the edge.
    const box = anchor.getBoundingClientRect();
    const frame = (pop.offsetParent ?? ws).getBoundingClientRect();
    pop.style.left = `${Math.max(8, Math.min(box.left - frame.left, frame.width - pop.offsetWidth - 8))}px`;
    pop.style.bottom = `${frame.bottom - box.top + 6}px`;
  }

  function modelMenu() {
    const s = state.status;
    const models = caps().models ?? [];
    const current = s?.model ?? "";
    const chosen = models.find((m) => (m.resolved === current || m.value === current) && m.value !== "default")?.value;
    const rows = models.map((m) =>
      `<button class="cc-pop-row${m.value === chosen ? " on" : ""}" data-claude-set-model="${esc(m.value)}">` +
      `<b>${esc(m.name)}</b><span>${esc(m.description)}</span>${m.value === chosen ? icon("check") : ""}</button>`).join("");
    const efforts = (models.find((m) => m.value === chosen) ?? models[0])?.efforts ?? [];
    const effortRow = efforts.length
      ? `<div class="cc-pop-sub">Effort</div><div class="cc-seg">` +
        [["", "Auto"], ...efforts.map((e) => [e, e === "xhigh" ? "X-High" : e[0].toUpperCase() + e.slice(1)])]
          .map(([v, label]) => `<button data-claude-set-effort="${esc(v)}" class="${(s?.effort ?? "") === v ? "on" : ""}">${esc(label)}</button>`).join("") +
        `</div>`
      : "";
    return `<div class="cc-pop-head">Model</div>${rows}${effortRow}`;
  }

  function engineMenu() {
    const now = engineNow();
    const started = state.order.length > 0;
    return `<div class="cc-pop-head">Agent${started ? " <span>starts a new conversation</span>" : ""}</div>` + ENGINE_ORDER.map((e) => {
      const known = state.engines.get(e);
      const missing = known?.missing;
      const plan = known?.plan ? ` · ${known.plan}` : "";
      return `<button class="cc-pop-row${e === now ? " on" : ""}${missing ? " off" : ""}" data-claude-set-engine="${e}"${missing ? ` title="${esc(missing)}"` : ""}>` +
        `${icon(ENGINES[e].icon)}<b>${esc(ENGINES[e].name)}</b><span>${esc(missing ? "Not installed here" : ENGINES[e].note + plan)}</span>${e === now ? icon("check") : ""}</button>`;
    }).join("");
  }

  function modeMenu() {
    const current = state.status?.mode === "manual" ? "default" : state.status?.mode ?? "default";
    return `<div class="cc-pop-head">Permissions <span>⇧⇥ to switch</span></div>` + modes().map((m) =>
      `<button class="cc-pop-row${m.value === current ? " on" : ""}" data-claude-set-mode="${m.value}">` +
      `${icon(m.icon)}<b>${esc(m.name)}</b><span>${esc(m.note)}</span>${m.value === current ? icon("check") : ""}</button>`).join("");
  }

  /* ── what you do here ─────────────────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (!(target instanceof Element)) return;
    const pop = els.pop;
    if (pop && !pop.hidden && !target.closest("[data-claude-pop]") && !target.closest("[data-pop]")) pop.hidden = true;

    if (!sideMenu.hidden && !target.closest(".cc-side-menu") && !target.closest("[data-session-menu]") && !target.closest("[data-folder-menu]")) closeSideMenu();
    const more = target.closest("[data-session-menu]") ?? target.closest("[data-folder-menu]");
    if (more) {
      const el = /** @type {HTMLElement} */ (more);
      const box = el.getBoundingClientRect();
      const html = el.dataset.sessionMenu ? sessionMenu(el.dataset.sessionMenu) : folderMenu(el.dataset.folderMenu ?? "");
      if (!sideMenu.hidden && sideMenu.dataset.for === (el.dataset.sessionMenu ?? el.dataset.folderMenu)) closeSideMenu();
      else {
        openSideMenu(html, box.left, box.bottom + 4);
        sideMenu.dataset.for = el.dataset.sessionMenu ?? el.dataset.folderMenu ?? "";
      }
      return;
    }
    const moveTo = target.closest("[data-move-to]");
    if (moveTo) {
      const el = /** @type {HTMLElement} */ (moveTo);
      closeSideMenu();
      if (folderOf(el.dataset.session ?? "")?.id !== el.dataset.moveTo) {
        host.dispatch({ type: "chatFolderMove", session: el.dataset.session, folder: el.dataset.moveTo });
      }
      return;
    }
    const moveOut = target.closest("[data-move-out]");
    if (moveOut) {
      closeSideMenu();
      host.dispatch({ type: "chatFolderMove", session: /** @type {HTMLElement} */ (moveOut).dataset.moveOut, folder: null });
      return;
    }
    const ask = target.closest("[data-delete-ask]");
    if (ask) {
      // The same menu turns into the question, where it was.
      sideMenu.innerHTML = deleteConfirm(/** @type {HTMLElement} */ (ask).dataset.deleteAsk ?? "");
      const box = sideMenu.getBoundingClientRect();
      openSideMenu(sideMenu.innerHTML, box.left, box.top);
      /** @type {HTMLElement | null} */ (sideMenu.querySelector("[data-delete-cancel]"))?.focus();
      return;
    }
    if (target.closest("[data-delete-cancel]")) {
      closeSideMenu();
      return;
    }
    const doomed = target.closest("[data-delete-chat]");
    if (doomed) {
      const id = /** @type {HTMLElement} */ (doomed).dataset.deleteChat ?? "";
      closeSideMenu();
      state.panes.delete(id);
      savePanes(state.panes);
      host.dispatch({ type: "claudeDelete", session: id });
      return;
    }
    const withSession = target.closest("[data-new-folder-with]");
    if (withSession) {
      closeSideMenu();
      newFolder([/** @type {HTMLElement} */ (withSession).dataset.newFolderWith ?? ""]);
      return;
    }
    if (target.closest("[data-claude-new-folder]")) {
      newFolder([]);
      return;
    }
    const rename = target.closest("[data-folder-rename]");
    if (rename) {
      closeSideMenu();
      startRename(/** @type {HTMLElement} */ (rename).dataset.folderRename ?? "");
      return;
    }
    const drop = target.closest("[data-folder-delete]");
    if (drop) {
      closeSideMenu();
      host.dispatch({ type: "chatFolderDelete", folder: /** @type {HTMLElement} */ (drop).dataset.folderDelete });
      return;
    }
    const toggleFolder = target.closest("[data-folder-toggle]");
    if (toggleFolder) {
      const id = /** @type {HTMLElement} */ (toggleFolder).dataset.folderToggle ?? "";
      const folder = state.folders.find((f) => f.id === id);
      // A double click names it: the first click of the two folded it, so it
      // is unfolded again rather than left the other way.
      if (folder && e.detail >= 2) {
        folder.collapsed = !folder.collapsed;
        host.dispatch({ type: "chatFolderCollapse", folder: id, collapsed: folder.collapsed });
        startRename(id);
        return;
      }
      if (folder) {
        // Folded at once; the core keeps it so for every project.
        folder.collapsed = !folder.collapsed;
        renderSessions();
        host.dispatch({ type: "chatFolderCollapse", folder: id, collapsed: folder.collapsed });
      }
      return;
    }

    const link = target.closest("a[data-link]");
    if (link) {
      e.preventDefault();
      host.dispatch({ type: "openLink", url: link.getAttribute("href") });
      return;
    }
    const copy = target.closest("[data-copy]");
    if (copy) {
      const text = copy.closest(".md-code")?.querySelector("code")?.textContent ?? "";
      navigator.clipboard?.writeText(text).then(() => {
        copy.textContent = "Copied";
        setTimeout(() => { copy.textContent = "Copy"; }, 1200);
      }).catch(() => {});
      return;
    }
    if (target.closest("[data-claude-new]")) {
      host.dispatch({ type: "claudeNew" });
      showChat();
      els.input?.focus();
      return;
    }
    const session = target.closest("[data-claude-session]");
    if (session) {
      host.dispatch({ type: "claudeOpen", session: /** @type {HTMLElement} */ (session).dataset.claudeSession });
      showChat();
      return;
    }
    const toggle = target.closest("[data-toggle]");
    if (toggle && !target.closest(".cc-tool-detail")) {
      const id = /** @type {HTMLElement} */ (toggle).dataset.toggle ?? "";
      if (state.open.has(id)) state.open.delete(id);
      else state.open.add(id);
      const item = state.items.get(id);
      if (item) draw(item);
      return;
    }
    const allow = target.closest("[data-claude-allow]");
    if (allow) {
      const request = /** @type {HTMLElement} */ (allow).dataset.claudeAllow;
      host.dispatch({ type: "claudeAnswer", request, answer: { kind: "allow", remember: /** @type {HTMLElement} */ (allow).dataset.remember === "1" } });
      return;
    }
    const deny = target.closest("[data-claude-deny]");
    if (deny) {
      const request = /** @type {HTMLElement} */ (deny).dataset.claudeDeny;
      const why = /** @type {HTMLInputElement | null} */ (deny.closest(".cc-item")?.querySelector("[data-claude-why]"));
      const message = why?.value.trim() || (deny.closest(".cc-plan") ? "Keep planning." : "");
      host.dispatch({ type: "claudeAnswer", request, answer: { kind: "deny", message } });
      return;
    }
    const artifact = target.closest("[data-claude-artifact]");
    if (artifact) {
      const el = /** @type {HTMLElement} */ (artifact);
      const tool = [...state.items.values()].find((i) => i.kind === "tool" && i.artifact?.id === el.dataset.claudeArtifact);
      openArtifact(el.dataset.claudeArtifact ?? "", Number(el.dataset.seq), tool?.kind === "tool" ? tool.artifact : undefined);
      return;
    }
    if (target.closest("[data-artifact-import]")) {
      state.importing = true;
      renderGallery();
      host.dispatch({ type: "importArtifacts" });
      return;
    }
    const forget = target.closest("[data-artifact-forget]");
    if (forget) {
      host.dispatch({ type: "forgetArtifact", id: /** @type {HTMLElement} */ (forget).dataset.artifactForget });
      return;
    }
    if (target.closest("[data-artifact-open]")) {
      const url = target.closest("[data-artifact-open]")?.getAttribute("data-url");
      if (url) host.dispatch({ type: "openLink", url });
      return;
    }
    if (target.closest("[data-artifact-reload]")) {
      if (els.artFrame?.src) els.artFrame.src = els.artFrame.src;
      return;
    }
    if (target.closest("[data-artifact-gallery]") || target.closest("[data-artifacts-toggle]")) {
      if (target.closest("[data-artifacts-toggle]") && ws.dataset.rightOpen === "true") {
        host.panels.hide?.("right", ws);
        return;
      }
      if (target.closest("[data-artifacts-toggle]") && state.shown) {
        host.panels.show("right", ws);
        return;
      }
      showGallery();
      return;
    }
    if (target.closest("[data-artifact-close]")) {
      host.panels.hide?.("right", ws);
      return;
    }
    const command = target.closest("[data-claude-command]");
    if (command) {
      takeCommand(/** @type {HTMLElement} */ (command).dataset.claudeCommand ?? "");
      return;
    }
    const unattach = target.closest("[data-claude-unattach]");
    if (unattach) {
      state.attachments.splice(Number(/** @type {HTMLElement} */ (unattach).dataset.claudeUnattach), 1);
      renderAttachments();
      renderComposerState();
      return;
    }
    if (target.closest("[data-claude-attach]")) {
      els.file?.click();
      return;
    }
    if (target.closest("[data-claude-send]")) {
      send();
      return;
    }
    const pill = target.closest("[data-pop]");
    if (pill) {
      const which = /** @type {HTMLElement} */ (pill).dataset.pop;
      popover(/** @type {HTMLElement} */ (pill), which === "model" ? modelMenu() : which === "engine" ? engineMenu() : modeMenu());
      return;
    }
    const setEngine = target.closest("[data-claude-set-engine]");
    if (setEngine) {
      if (els.pop) els.pop.hidden = true;
      const engine = /** @type {HTMLElement} */ (setEngine).dataset.claudeSetEngine;
      if (engine === engineNow() && !state.order.length) return;
      host.dispatch({ type: "claudeEngine", engine });
      showChat();
      els.input?.focus();
      return;
    }
    const setModel = target.closest("[data-claude-set-model]");
    if (setModel) {
      host.dispatch({ type: "claudeModel", model: /** @type {HTMLElement} */ (setModel).dataset.claudeSetModel });
      if (els.pop) els.pop.hidden = true;
      return;
    }
    const setEffort = target.closest("[data-claude-set-effort]");
    if (setEffort) {
      const effort = /** @type {HTMLElement} */ (setEffort).dataset.claudeSetEffort || null;
      host.dispatch({ type: "claudeEffort", effort });
      if (els.pop) els.pop.hidden = true;
      return;
    }
    const setMode = target.closest("[data-claude-set-mode]");
    if (setMode) {
      host.dispatch({ type: "claudeMode", mode: /** @type {HTMLElement} */ (setMode).dataset.claudeSetMode });
      if (els.pop) els.pop.hidden = true;
    }
  });

  // A right click on a chat or a folder offers what its ⋯ does.
  ws.addEventListener("contextmenu", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const row = /** @type {HTMLElement | null} */ (target.closest?.("[data-claude-session]"));
    const folder = /** @type {HTMLElement | null} */ (target.closest?.("[data-folder-toggle]"));
    if (!row && !folder) return;
    e.preventDefault();
    const id = row ? row.dataset.claudeSession ?? "" : folder?.dataset.folderToggle ?? "";
    openSideMenu(row ? sessionMenu(id) : folderMenu(id), e.clientX, e.clientY);
    sideMenu.dataset.for = id;
  });

  // Naming a folder in place: Enter keeps it, Escape does not, and leaving
  // the field keeps it too. Its keys are its own, not the chat's or Vim's.
  els.sessions?.addEventListener("input", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    if (field.matches?.("[data-folder-input]")) state.draft = field.value;
  });
  els.sessions?.addEventListener("keydown", (e) => {
    const field = /** @type {HTMLElement} */ (e.target);
    if (field.matches?.("[data-folder-toggle]") && e.key === "F2") {
      e.preventDefault();
      startRename(field.dataset.folderToggle ?? "");
      return;
    }
    if (!field.matches?.("[data-folder-input]")) return;
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      finishRename();
    } else if (e.key === "Escape") {
      e.preventDefault();
      finishRename(true);
    }
  });
  els.sessions?.addEventListener("focusout", (e) => {
    if (!/** @type {HTMLElement} */ (e.target).matches?.("[data-folder-input]")) return;
    // A redraw puts the focus in a new field; only leaving it for good keeps
    // the name.
    setTimeout(() => {
      if (state.renaming && !doc.activeElement?.matches?.("[data-folder-input]")) finishRename();
    }, 0);
  });
  sideMenu.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      closeSideMenu();
    }
  });

  // Dragging a chat onto a folder puts it there; onto the list, takes it out.
  const DRAGGED = "application/x-proton-chat";
  /** @param {Element | null} on */
  const markDrop = (on) => {
    els.sessions?.querySelectorAll(".drop").forEach((el) => { if (el !== on) el.classList.remove("drop"); });
    on?.classList.add("drop");
  };
  els.sessions?.addEventListener("dragstart", (e) => {
    const row = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-claude-session]"));
    if (!row || !e.dataTransfer) return;
    e.dataTransfer.setData(DRAGGED, row.dataset.claudeSession ?? "");
    e.dataTransfer.effectAllowed = "move";
    row.classList.add("dragging");
    closeSideMenu();
  });
  els.sessions?.addEventListener("dragend", () => {
    els.sessions?.querySelectorAll(".dragging").forEach((el) => el.classList.remove("dragging"));
    markDrop(null);
  });
  els.sessions?.addEventListener("dragover", (e) => {
    if (!e.dataTransfer?.types.includes(DRAGGED)) return;
    const target = /** @type {HTMLElement} */ (e.target);
    const on = target.closest?.("[data-folder]") ?? target.closest?.("[data-loose]");
    if (!on) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    markDrop(on);
  });
  els.sessions?.addEventListener("dragleave", (e) => {
    if (!els.sessions?.contains(/** @type {Node | null} */ (e.relatedTarget))) markDrop(null);
  });
  els.sessions?.addEventListener("drop", (e) => {
    const id = e.dataTransfer?.getData(DRAGGED);
    if (!id) return;
    e.preventDefault();
    markDrop(null);
    const target = /** @type {HTMLElement} */ (e.target);
    const folder = /** @type {HTMLElement | null} */ (target.closest?.("[data-folder]"))?.dataset.folder ?? null;
    if (!folder && !target.closest?.("[data-loose]")) return;
    // Dropped where it already is, nothing moves.
    if ((folderOf(id)?.id ?? null) !== folder) host.dispatch({ type: "chatFolderMove", session: id, folder });
  });

  // A question card: the answers, by question.
  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (!form.matches?.("[data-claude-question]")) return;
    e.preventDefault();
    /** @type {Record<string, string>} */
    const answers = {};
    for (const set of form.querySelectorAll("fieldset[data-question]")) {
      const chosen = [...set.querySelectorAll("input:checked")].map((input) => {
        const value = /** @type {HTMLInputElement} */ (input).value;
        return value === "\u0000other" ? /** @type {HTMLInputElement} */ (set.querySelector("[data-other]"))?.value.trim() ?? "" : value;
      }).filter(Boolean);
      answers[/** @type {HTMLElement} */ (set).dataset.question ?? ""] = chosen.join(", ");
    }
    host.dispatch({ type: "claudeAnswer", request: form.dataset.claudeQuestion, answer: { kind: "questions", answers } });
  });

  // Typing in "something else" chooses it.
  ws.addEventListener("input", (e) => {
    const other = /** @type {HTMLElement} */ (e.target);
    if (other.matches?.("[data-other]")) {
      const radio = /** @type {HTMLInputElement | null} */ (other.closest("label")?.querySelector("input[type=radio], input[type=checkbox]"));
      if (radio) radio.checked = true;
    }
  });

  ws.addEventListener("toggle", (e) => {
    const think = /** @type {HTMLDetailsElement} */ (e.target);
    if (!think.matches?.("[data-think]")) return;
    const id = think.dataset.think ?? "";
    if (think.open) state.open.add(id);
    else state.open.delete(id);
  }, true);

  els.input?.addEventListener("input", () => {
    autosize();
    state.menuIndex = 0;
    renderMenu();
    renderComposerState();
  });

  els.input?.addEventListener("keydown", (e) => {
    e.stopPropagation();
    const menuOpen = els.menu && !els.menu.hidden;
    if (menuOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const rows = els.menu.querySelectorAll("[data-claude-command]").length;
      state.menuIndex = (state.menuIndex + (e.key === "ArrowDown" ? 1 : rows - 1)) % Math.max(1, rows);
      renderMenu();
      return;
    }
    if (menuOpen && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey))) {
      const row = /** @type {HTMLElement | null} */ (els.menu.querySelectorAll("[data-claude-command]")[state.menuIndex]);
      if (row) {
        e.preventDefault();
        takeCommand(row.dataset.claudeCommand ?? "");
        return;
      }
    }
    if (e.key === "Tab" && e.shiftKey) {
      // ⇧⇥ steps through the permission modes, as it does in Claude Code.
      e.preventDefault();
      const current = state.status?.mode === "manual" ? "default" : state.status?.mode ?? "default";
      const all = modes();
      const next = all[(all.findIndex((m) => m.value === current) + 1) % all.length];
      host.dispatch({ type: "claudeMode", mode: next.value });
      return;
    }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (menuOpen) {
        els.menu.hidden = true;
      } else if (els.pop && !els.pop.hidden) {
        els.pop.hidden = true;
      } else if (state.status && state.status.activity !== "idle") {
        host.dispatch({ type: "claudeStop" });
      } else {
        els.input.blur();
      }
    }
  });

  els.input?.addEventListener("paste", (e) => {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length) return;
    e.preventDefault();
    files.forEach(attach);
  });
  els.file?.addEventListener("change", () => {
    [...(els.file.files ?? [])].forEach(attach);
    els.file.value = "";
  });

  // Escape from anywhere in the chat interrupts, as it does in Claude Code.
  ws.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.target === els.input) return;
    if (state.status && state.status.activity === "working") {
      host.dispatch({ type: "claudeStop" });
    }
  });

  els.artVersion?.addEventListener("change", () => {
    if (state.shown) openArtifact(state.shown.id, Number(els.artVersion.value));
  });

  renderWelcome();
  renderComposerState();
  renderGallery();

  return {
    sessions, items, status, capabilities, artifacts,
    /** The project, so paths can be shown from it. @param {string} root */
    setRoot(root) { state.root = String(root ?? "").replace(/\/+$/, ""); },
    focus() { els.input?.focus(); },
    showChat,
    /** The question in the composer, for the council. */
    question() { return els.input?.value.trim() ?? ""; },
    clearQuestion() {
      if (!els.input) return;
      els.input.value = "";
      autosize();
      renderComposerState();
    },
  };
}
