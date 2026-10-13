/* Game mode: Proton's own engine's editor.
 *
 * Laid out as PDF and LaTeX are, on Codex's ground: the game's World, a
 * room's Layers and Entities, and its Assets down the left; the rooms open
 * as tabs over the stage; what is selected, how a run starts and the agent
 * on the right. With no game open, the games the project holds and a new
 * one to make are the whole page.
 *
 * Nothing is kept here that the core does not say. An edit is sent as a
 * batch of ops on one of the game's documents (`gameEdit`); the core
 * applies it whole or not at all and sends the room back, so what is drawn
 * is always what would be saved. */

import { escapeHtml as esc } from "./html.js";
import { draw, fit, pick } from "./game-overlay.js";

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */
/** @typedef {import('./bindings/GameProjectView').GameProjectView} Project */
/** @typedef {import('./bindings/GameFound').GameFound} Found */
/** @typedef {import('./bindings/GameTemplate').GameTemplate} Template */
/** @typedef {import('./game-overlay.js').Selected} Selected */

/** The Game mode's keys, as ? lists them. */
const KEYS = /** @type {[string, string][]} */ ([
  ["⌘S", "save every changed room and file"], ["⌘Z ⇧⌘Z", "undo, redo — wherever the change was"],
  ["⎋", "select nothing"], ["⌫", "remove what is selected"], ["[ ]", "the room tab before, after"],
  ["1 2 3 4", "World, Layers, Entities, Assets"], ["?", "these keys"],
]);

/** What rooms place without a prefab of the game's own. */
const BUILT_IN = ["transition", "save_point", "ability_gate", "ability_pickup"];

const SIDES = ["world", "layers", "entities", "assets"];

/** A per-window preference, kept if it can be. @param {string} key @param {string} fallback */
function pref(key, fallback) {
  try {
    return localStorage.getItem(`proton.game.${key}`) ?? fallback;
  } catch {
    return fallback;
  }
}
/** @param {string} key @param {string} value */
function keep(key, value) {
  try {
    localStorage.setItem(`proton.game.${key}`, value);
  } catch {
    // Not kept: the next window starts from the default.
  }
}

/** A field of the inspector: where in a document it edits, and how.
 *  @typedef {{label: string, path: string[], value: any, type: "text" | "number" | "optional" | "check" | "select" | "pair" | "rect" | "color" | "abilities" | "read", options?: [string, string][], hint?: string}} Field */

/** @param {{root: Document, dispatch: (command: any) => unknown, keys?: (title: string, rows: [string, string][]) => void}} host */
export function createGame(host) {
  const ws = /** @type {HTMLElement} */ (host.root.querySelector('.workspace[data-ws="game"]'));
  const q = (/** @type {string} */ selector) => /** @type {HTMLElement} */ (ws.querySelector(selector));
  const send = host.dispatch;

  /** @type {{games: Found[], templates: Template[]}} */
  let home = { games: [], templates: [] };
  /** @type {Project | null} */
  let project = null;
  /** @type {Room | null} */
  let room = null;
  /** @type {Selected | null} */
  let selected = null;
  let side = SIDES.includes(pref("side", "world")) ? pref("side", "world") : "world";
  /** Asking before the game is left with changes in it. */
  let asking = false;
  /** Whether the person is looking at this mode. */
  let shown = false;

  const stage = q("[data-game-stage]");
  const canvas = /** @type {HTMLCanvasElement} */ (q("[data-game-overlay]"));

  /* ── the home page ───────────────────────────────────────────── */

  function renderHome() {
    ws.classList.toggle("game-home", !project);
    q("[data-game-open]").hidden = Boolean(project);
    const box = q("[data-game-home]");
    const templates = home.templates.map((t) => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join("");
    const games = home.games.length
      ? `<div class="game-cards">${home.games
          .map(
            (g) =>
              `<button class="game-card" data-game-open-path="${esc(g.path)}"><b>${esc(g.name || g.path)}</b>` +
              `<span>${esc(g.niche)} · ${g.rooms} room${g.rooms === 1 ? "" : "s"}</span><span class="game-card-path">${esc(g.path)}</span></button>`,
          )
          .join("")}</div>`
      : `<p class="game-faint">No game in this project yet. Make one to start from: two rooms, a door between them, and an ability to find.</p>`;
    box.innerHTML =
      `<header class="game-home-head"><h3>Games</h3></header>${games}` +
      `<form class="game-new" data-game-new>` +
      `<h4>New game</h4>` +
      `<label>Name <input name="name" data-game-new-name placeholder="The name of your game" autocomplete="off" spellcheck="false" required></label>` +
      `<label>Kind <select name="template" data-game-new-template>${templates}</select></label>` +
      `<button class="cc-primary" type="submit">Make it</button></form>`;
  }

  ws.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const card = /** @type {HTMLElement | null} */ (target.closest("[data-game-open-path]"));
    if (card) send({ type: "gameOpen", path: card.dataset.gameOpenPath ?? "." });
  });
  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (!form.matches("[data-game-new]")) return;
    e.preventDefault();
    const data = new FormData(form);
    const name = String(data.get("name") ?? "").trim();
    if (!name) return;
    send({ type: "gameNew", name, template: String(data.get("template") ?? "metroidvania") });
  });

  /* ── the bar: tabs, undo, save ───────────────────────────────── */

  function renderBar() {
    q("[data-game-name]").textContent = project ? project.game.name || project.path : "";
    const tabs = q("[data-game-tabs]");
    tabs.innerHTML = (project?.open ?? [])
      .map((id) => {
        const row = project?.rooms.find((r) => r.id === id);
        const on = id === project?.focus;
        return `<div class="game-tab${on ? " on" : ""}" role="tab" aria-selected="${on}" data-game-tab="${esc(id)}" title="${esc(id)}">` +
          `<span class="game-tab-name">${esc(id)}</span>${row?.dirty ? `<span class="dot" title="Unsaved changes"></span>` : ""}` +
          `<button class="game-tab-close" data-game-tab-close="${esc(id)}" aria-label="Close ${esc(id)}">×</button></div>`;
      })
      .join("");
    /** @type {HTMLButtonElement} */ (q("[data-game-undo]")).disabled = !project?.canUndo;
    /** @type {HTMLButtonElement} */ (q("[data-game-redo]")).disabled = !project?.canRedo;
    /** @type {HTMLButtonElement} */ (q("[data-game-save]")).disabled = !project?.dirty;
  }

  q("[data-game-tabs]").addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const close = /** @type {HTMLElement | null} */ (target.closest("[data-game-tab-close]"));
    if (close) {
      e.stopPropagation();
      send({ type: "gameCloseRoom", room: close.dataset.gameTabClose });
      return;
    }
    const tab = /** @type {HTMLElement | null} */ (target.closest("[data-game-tab]"));
    if (tab && tab.dataset.gameTab !== project?.focus) send({ type: "gameFocus", room: tab.dataset.gameTab });
  });
  q("[data-game-undo]").addEventListener("click", () => send({ type: "gameUndo" }));
  q("[data-game-redo]").addEventListener("click", () => send({ type: "gameRedo" }));
  q("[data-game-save]").addEventListener("click", () => send({ type: "gameSave" }));
  q("[data-game-leave]").addEventListener("click", () => leave());

  /** Back to the home page — asking first when something is unsaved. */
  function leave() {
    if (!project) return;
    if (project.dirty) {
      asking = true;
      renderAsk();
      return;
    }
    send({ type: "gameClose", discard: false });
  }

  function renderAsk() {
    const box = q("[data-game-ask]");
    box.hidden = !asking;
    if (!asking) return;
    box.innerHTML =
      `<span>${esc(project?.game.name || "This game")} has changes that are not saved.</span><span class="grow"></span>` +
      `<button class="cc-quiet" data-game-ask-answer="stay">Stay</button>` +
      `<button class="cc-quiet" data-game-ask-answer="discard">Leave without saving</button>` +
      `<button class="cc-primary" data-game-ask-answer="save">Save and leave</button>`;
  }
  q("[data-game-ask]").addEventListener("click", (e) => {
    const answer = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-ask-answer]"))?.dataset.gameAskAnswer;
    if (!answer) return;
    asking = false;
    renderAsk();
    if (answer === "save") {
      send({ type: "gameSave" });
      send({ type: "gameClose", discard: false });
    } else if (answer === "discard") {
      send({ type: "gameClose", discard: true });
    }
  });

  /* ── the left: World, Layers, Entities, Assets ───────────────── */

  /** @param {string} which */
  function showSide(which) {
    side = which;
    keep("side", which);
    for (const tab of ws.querySelectorAll("[data-game-side]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.gameSide === which;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    renderSide();
  }
  q(".game-side-tabs").addEventListener("click", (e) => {
    const tab = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-side]"));
    if (tab) showSide(tab.dataset.gameSide ?? "world");
  });

  /** A row of a list that selects something. */
  const row = (/** @type {string} */ kind, /** @type {string} */ id, /** @type {string} */ label, /** @type {string} */ meta = "", extra = "") => {
    const on = selected?.kind === kind && selected.id === id;
    return `<div class="game-row${on ? " on" : ""}" data-game-pick="${kind}" data-game-pick-id="${esc(id)}"${extra}>` +
      `<span class="game-row-name">${esc(label)}</span>${meta ? `<span class="game-row-meta">${meta}</span>` : ""}</div>`;
  };

  function renderSide() {
    const box = q("[data-game-side-body]");
    if (!project) {
      box.innerHTML = "";
      return;
    }
    if (side === "world") {
      const areas = project.world.areas.length ? project.world.areas : [{ id: "", name: "Rooms", color: "", music: null }];
      const rooms = (/** @type {string} */ area) =>
        project?.rooms
          .filter((r) => !project?.world.areas.length || r.area === area)
          .map((r) => {
            const on = r.id === project?.focus;
            return `<div class="game-row${on ? " on" : ""}" data-game-room="${esc(r.id)}"><span class="game-row-name">${esc(r.id)}</span>` +
              `${r.dirty ? `<span class="dot" title="Unsaved changes"></span>` : ""}<span class="game-row-meta">${r.size[0]}×${r.size[1]}</span></div>`;
          })
          .join("") ?? "";
      const problems = project.problems.length
        ? `<h5 class="game-head warn">To fix</h5>${project.problems.map((p) => `<p class="game-problem">${esc(p)}</p>`).join("")}`
        : "";
      box.innerHTML =
        row("game", "game", "Game settings", project.gameDirty ? `<span class="dot"></span>` : "") +
        areas
          .map((a) => `<h5 class="game-head"><i class="game-swatch" style="background:${esc(a.color || "transparent")}"></i>${esc(a.name || a.id)}</h5>${rooms(a.id)}`)
          .join("") +
        problems;
      return;
    }
    if (!room) {
      box.innerHTML = `<p class="game-faint">Open a room from the World tab.</p>`;
      return;
    }
    if (side === "layers") {
      // Front first, as a painter's layers are listed: what is nearest the
      // camera at the top.
      box.innerHTML = [...room.planes]
        .reverse()
        .map((p) => {
          const open = selected?.kind === "plane" ? selected.id === p.id : selected?.kind === "item" && selected.plane === p.id;
          const items = open
            ? p.items.map((i) => row("item", i.id, i.image.split("/").pop() ?? i.id, "", ` data-game-pick-plane="${esc(p.id)}"`)).join("")
            : "";
          const meta = `${p.gameplay ? "play · " : ""}${p.parallax !== null && p.parallax !== undefined ? `×${p.parallax}` : depthText(p.depth)} · ${p.items.length}`;
          return row("plane", p.id, p.name || p.id, esc(meta)) + (items ? `<div class="game-sub">${items}</div>` : "");
        })
        .join("");
      return;
    }
    if (side === "entities") {
      const group = (/** @type {string} */ title, /** @type {string} */ rows) => (rows ? `<h5 class="game-head">${title}</h5>${rows}` : "");
      box.innerHTML =
        group("Entities", room.entities.map((e) => row("entity", e.id, e.id, esc(e.prefab))).join("")) +
        group("Spawns", room.spawns.map((s) => row("spawn", s.id, s.id, `${s.at[0]}, ${s.at[1]}`)).join("")) +
        group("Lights", room.lights.map((l) => row("light", l.id, l.id, esc(l.plane))).join("")) +
        group("Collision", room.collision.map((s) => row("shape", s.id, s.id, esc(s.kind))).join("")) ||
        `<p class="game-faint">Nothing in this room yet.</p>`;
      return;
    }
    box.innerHTML = project.assets.length
      ? project.assets.map((a) => row("asset", a, a)).join("")
      : `<p class="game-faint">No images or sounds in the game's folder.</p>`;
  }

  /** @param {number} depth */
  function depthText(depth) {
    return depth === 0 ? "depth 0" : depth > 0 ? `${depth} behind` : `${-depth} in front`;
  }

  q("[data-game-side-body]").addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const open = /** @type {HTMLElement | null} */ (target.closest("[data-game-room]"));
    if (open) {
      send({ type: "gameOpenRoom", room: open.dataset.gameRoom });
      return;
    }
    const picked = /** @type {HTMLElement | null} */ (target.closest("[data-game-pick]"));
    if (picked) select({ kind: picked.dataset.gamePick ?? "", id: picked.dataset.gamePickId ?? "", plane: picked.dataset.gamePickPlane });
  });

  /** @param {Selected | null} what */
  function select(what) {
    selected = what;
    renderSide();
    renderInspector();
    paint();
    if (what && what.kind !== "asset" && what.kind !== "game") showPanel("inspector");
  }

  /* ── the stage ───────────────────────────────────────────────── */

  /** The colours the overlay draws in, from the theme. */
  function colors() {
    const style = getComputedStyle(ws);
    const token = (/** @type {string} */ name, /** @type {string} */ fallback) => style.getPropertyValue(name).trim() || fallback;
    return {
      text: token("--text", "#ddd"),
      muted: token("--muted", "#999"),
      faint: token("--faint", "#666"),
      accent: token("--accent", "#7aa2f7"),
      ok: token("--ok", "#9ece6a"),
      warn: token("--warn", "#e0af68"),
      err: token("--err", "#f7768e"),
      ground: token("--bg-deep", "#111"),
    };
  }

  let painting = 0;
  function paint() {
    if (painting) return;
    painting = requestAnimationFrame(() => {
      painting = 0;
      paintNow();
    });
  }
  function paintNow() {
    const empty = q("[data-game-stage-empty]");
    empty.hidden = Boolean(room) || !project;
    if (!room && project) empty.innerHTML = `<p class="game-faint">Open a room from the World tab.</p>`;
    canvas.hidden = !room;
    if (!room || !shown) return;
    const ratio = window.devicePixelRatio || 1;
    const width = stage.clientWidth;
    const height = stage.clientHeight;
    if (!width || !height) return;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const g = canvas.getContext("2d");
    if (!g) return;
    g.setTransform(ratio, 0, 0, ratio, 0, 0);
    g.clearRect(0, 0, width, height);
    draw(g, room, fit(room, width, height), colors(), selected);
  }
  new ResizeObserver(() => paint()).observe(stage);

  canvas.addEventListener("pointerdown", (e) => {
    if (!room) return;
    const box = canvas.getBoundingClientRect();
    const view = fit(room, box.width, box.height);
    select(pick(room, view, e.clientX - box.left, e.clientY - box.top) ?? { kind: "room", id: room.id });
    stage.focus({ preventScroll: true });
  });

  /* ── the right: Inspector, Run, Agent ────────────────────────── */

  /** @param {string} which */
  function showPanel(which) {
    const pane = q("#game-pane");
    pane.dataset.showing = which;
    for (const tab of pane.querySelectorAll("[data-game-panel]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.gamePanel === which;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    for (const body of pane.querySelectorAll("[data-game-body]")) /** @type {HTMLElement} */ (body).hidden = /** @type {HTMLElement} */ (body).dataset.gameBody !== which;
    keep("panel", which);
    if (which === "agent") {
      q("[data-game-agent-dot]").hidden = true;
      const thread = q("[data-game-body=agent] [data-agent-thread]");
      thread.scrollTop = thread.scrollHeight;
      q("[data-game-body=agent] [data-agent-input]").focus();
    }
  }
  q("#game-pane").addEventListener("click", (e) => {
    const tab = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-panel]"));
    if (tab) showPanel(tab.dataset.gamePanel ?? "inspector");
  });
  const startPanel = pref("panel", "inspector");
  if (startPanel !== "inspector") showPanel(startPanel);

  /** Which document a selection is in, and the fields it has.
   *  @returns {{title: string, target: string, fields: Field[], remove?: string[]} | null} */
  function inspected() {
    if (!project) return null;
    const game = project.game;
    if (selected?.kind === "game") {
      return {
        title: "Game settings",
        target: "game",
        fields: [
          { label: "Name", path: ["name"], value: game.name, type: "text" },
          { label: "Kind", path: ["niche"], value: game.niche, type: "select", options: [["metroidvania", "Metroidvania"], ["roguelite", "Roguelite"], ["isometric", "Isometric"], ["lowpoly", "Low-poly 3D"]] },
          { label: "View", path: ["view"], value: [game.view.width, game.view.height], type: "pair", hint: "width × height the camera frames" },
          { label: "Focal length", path: ["view", "focal"], value: game.view.focal, type: "number", hint: "how far the camera stands from the play plane" },
          { label: "Art", path: ["view", "filter"], value: game.view.filter, type: "select", options: [["smooth", "Smooth — painted"], ["nearest", "Nearest — pixels"]] },
          { label: "Run speed", path: ["controller", "run"], value: game.controller.run, type: "number" },
          { label: "Jump height", path: ["controller", "jump", "height"], value: game.controller.jump.height, type: "number" },
          { label: "Seconds to the top", path: ["controller", "jump", "apex"], value: game.controller.jump.apex, type: "number" },
          { label: "Coyote frames", path: ["controller", "jump", "coyote"], value: game.controller.jump.coyote, type: "number" },
          { label: "Jump buffer frames", path: ["controller", "jump", "buffer"], value: game.controller.jump.buffer, type: "number" },
          { label: "Dash distance", path: ["controller", "dash", "distance"], value: game.controller.dash.distance, type: "number" },
          { label: "Air jumps", path: ["controller", "airJumps"], value: game.controller.airJumps, type: "number" },
        ],
      };
    }
    if (!room) return null;
    const target = `room:${room.id}`;
    const planes = /** @type {[string, string][]} */ (room.planes.map((p) => [p.id, p.name || p.id]));
    const blends = /** @type {[string, string][]} */ ([["normal", "Normal"], ["add", "Add"], ["multiply", "Multiply"], ["screen", "Screen"]]);
    if (selected?.kind === "plane") {
      const p = room.planes.find((x) => x.id === selected?.id);
      if (!p) return null;
      const at = ["planes", `#${p.id}`];
      return {
        title: p.name || p.id,
        target,
        remove: at,
        fields: [
          { label: "Name", path: [...at, "name"], value: p.name, type: "text" },
          { label: "Depth", path: [...at, "depth"], value: p.depth, type: "number", hint: "behind the play plane; negative is in front" },
          { label: "Parallax", path: [...at, "parallax"], value: p.parallax, type: "optional", hint: "blank: from its depth" },
          { label: "Haze", path: [...at, "tint"], value: p.tint, type: "color" },
          { label: "Haze amount", path: [...at, "fog"], value: p.fog, type: "number" },
          { label: "Lit", path: [...at, "lit"], value: p.lit, type: "check" },
          { label: "Blend", path: [...at, "blend"], value: p.blend, type: "select", options: blends },
          { label: "Played on", path: [...at, "gameplay"], value: p.gameplay, type: "check" },
        ],
      };
    }
    if (selected?.kind === "item") {
      const plane = room.planes.find((x) => x.id === selected?.plane) ?? room.planes.find((x) => x.items.some((i) => i.id === selected?.id));
      const item = plane?.items.find((i) => i.id === selected?.id);
      if (!plane || !item) return null;
      const at = ["planes", `#${plane.id}`, "items", `#${item.id}`];
      return {
        title: item.image.split("/").pop() ?? item.id,
        target,
        remove: at,
        fields: [
          { label: "Image", path: [...at, "image"], value: item.image, type: "select", options: project.assets.filter((a) => /\.(png|jpe?g|webp)$/i.test(a)).map((a) => [a, a]) },
          { label: "Where", path: [...at, "at"], value: item.at, type: "pair" },
          { label: "Scale", path: [...at, "scale"], value: item.scale, type: "number" },
          { label: "Turn", path: [...at, "rot"], value: item.rot, type: "number", hint: "degrees" },
          { label: "Flipped", path: [...at, "flip"], value: item.flip, type: "check" },
          { label: "Opacity", path: [...at, "opacity"], value: item.opacity, type: "number" },
          { label: "Blend", path: [...at, "blend"], value: item.blend, type: "select", options: blends },
        ],
      };
    }
    if (selected?.kind === "entity") {
      const e = room.entities.find((x) => x.id === selected?.id);
      if (!e) return null;
      const at = ["entities", `#${e.id}`];
      const kinds = /** @type {[string, string][]} */ ([...BUILT_IN.map((k) => [k, k.replace("_", " ")]), ...project.prefabs.map((p) => [p.id, p.name || p.id])]);
      /** @type {Field[]} */
      const fields = [{ label: "What", path: [...at, "prefab"], value: e.prefab, type: "select", options: kinds }];
      if (e.at) fields.push({ label: "Where", path: [...at, "at"], value: e.at, type: "pair" });
      if (e.rect) fields.push({ label: "Covers", path: [...at, "rect"], value: e.rect, type: "rect" });
      if (e.prefab === "transition") {
        fields.push({ label: "Leads to", path: [...at, "to", "room"], value: e.to?.room ?? "", type: "select", options: project.rooms.map((r) => [r.id, r.id]) });
        fields.push({ label: "Arriving at", path: [...at, "to", "entry"], value: e.to?.entry ?? "", type: "text", hint: "a spawn in that room" });
      }
      if (e.prefab === "ability_gate") fields.push({ label: "Opens for", path: [...at, "requires"], value: e.requires, type: "abilities" });
      if (e.prefab === "ability_pickup") {
        fields.push({ label: "Gives", path: [...at, "grants"], value: e.grants ?? "", type: "select", options: game.abilities.map((a) => [a.id, a.name || a.id]) });
      }
      return { title: e.id, target, remove: at, fields };
    }
    if (selected?.kind === "spawn") {
      const s = room.spawns.find((x) => x.id === selected?.id);
      if (!s) return null;
      const at = ["spawns", `#${s.id}`];
      return { title: s.id, target, remove: at, fields: [{ label: "Where", path: [...at, "at"], value: s.at, type: "pair" }] };
    }
    if (selected?.kind === "light") {
      const l = room.lights.find((x) => x.id === selected?.id);
      if (!l) return null;
      const at = ["lights", `#${l.id}`];
      return {
        title: l.id,
        target,
        remove: at,
        fields: [
          { label: "Plane", path: [...at, "plane"], value: l.plane, type: "select", options: planes },
          { label: "Where", path: [...at, "at"], value: l.at, type: "pair" },
          { label: "Reach", path: [...at, "radius"], value: l.radius, type: "number" },
          { label: "Colour", path: [...at, "color"], value: l.color, type: "color" },
          { label: "Brightness", path: [...at, "intensity"], value: l.intensity, type: "number" },
          { label: "Flicker", path: [...at, "flicker"], value: l.flicker, type: "number" },
        ],
      };
    }
    if (selected?.kind === "shape") {
      const s = room.collision.find((x) => x.id === selected?.id);
      if (!s) return null;
      const at = ["collision", `#${s.id}`];
      return {
        title: s.id,
        target,
        remove: at,
        fields: [
          { label: "Kind", path: [...at, "kind"], value: s.kind, type: "select", options: [["solid", "Solid"], ["oneway", "One-way, from above"], ["hazard", "Hazard"], ["nowall", "Solid, not a wall"]] },
          { label: "Closed", path: [...at, "closed"], value: s.closed, type: "check" },
          { label: "Points", path: [...at, "points"], value: s.points.length, type: "read" },
        ],
      };
    }
    if (selected?.kind === "asset") {
      return { title: selected.id.split("/").pop() ?? selected.id, target: "game", fields: [{ label: "File", path: [], value: selected.id, type: "read" }] };
    }
    return {
      title: room.id,
      target,
      fields: [
        { label: "Size", path: ["size"], value: room.size, type: "pair" },
        { label: "Dark", path: ["ambience", "ambient"], value: room.ambience.ambient, type: "color", hint: "the light where no light falls" },
      ],
    };
  }

  function renderInspector() {
    const box = q("[data-game-inspector]");
    const shape = inspected();
    if (!shape) {
      box.innerHTML = project ? `<p class="game-faint">Select a plane, an entity or a line.</p>` : "";
      return;
    }
    const abilities = project?.game.abilities ?? [];
    const input = (/** @type {Field} */ f, /** @type {number} */ i) => {
      const where = esc(JSON.stringify(f.path));
      switch (f.type) {
        case "read":
          return `<span class="game-read">${esc(String(f.value))}</span>`;
        case "check":
          return `<input type="checkbox" data-game-field="${where}" data-game-kind="check"${f.value ? " checked" : ""}>`;
        case "select": {
          const options = (f.options ?? []).some(([v]) => v === f.value) ? f.options ?? [] : [[String(f.value ?? ""), String(f.value || "—")], ...(f.options ?? [])];
          return `<select data-game-field="${where}" data-game-kind="select">${options
            .map(([v, label]) => `<option value="${esc(v)}"${v === f.value ? " selected" : ""}>${esc(label)}</option>`)
            .join("")}</select>`;
        }
        case "pair":
        case "rect":
          return `<span class="game-nums">${/** @type {number[]} */ (f.value)
            .map((v, n) => `<input type="number" step="1" data-game-field="${where}" data-game-kind="${f.type}" data-game-index="${n}" value="${esc(String(v))}">`)
            .join("")}</span>`;
        case "abilities":
          return `<span class="game-checks">${abilities
            .map((a) => `<label><input type="checkbox" data-game-field="${where}" data-game-kind="abilities" value="${esc(a.id)}"${/** @type {string[]} */ (f.value).includes(a.id) ? " checked" : ""}> ${esc(a.name || a.id)}</label>`)
            .join("")}</span>`;
        case "number":
          return `<input type="number" step="any" data-game-field="${where}" data-game-kind="number" value="${esc(String(f.value))}">`;
        case "optional":
          return `<input type="number" step="any" data-game-field="${where}" data-game-kind="optional" value="${f.value === null || f.value === undefined ? "" : esc(String(f.value))}" placeholder="—">`;
        case "color":
          return `<input type="text" data-game-field="${where}" data-game-kind="color" value="${esc(f.value ?? "")}" placeholder="#rrggbb" spellcheck="false">`;
        default:
          return `<input type="text" data-game-field="${where}" data-game-kind="text" value="${esc(String(f.value ?? ""))}" spellcheck="false" data-game-n="${i}">`;
      }
    };
    box.innerHTML =
      `<header class="game-insp-head"><h4>${esc(shape.title)}</h4>${shape.remove ? `<button class="cc-quiet" data-game-remove title="Remove it  ⌫">Remove</button>` : ""}</header>` +
      `<div class="game-fields" data-game-target="${esc(shape.target)}">${shape.fields
        .map((f, i) => `<label class="game-field"><span>${esc(f.label)}</span>${input(f, i)}${f.hint ? `<small>${esc(f.hint)}</small>` : ""}</label>`)
        .join("")}</div>`;
  }

  /** The value a field's inputs say now. @param {HTMLElement} field */
  function valueOf(field) {
    const kind = field.dataset.gameKind;
    const path = field.dataset.gameField ?? "[]";
    const peers = /** @type {HTMLInputElement[]} */ ([...ws.querySelectorAll(`[data-game-field='${path.replace(/'/g, "\\'")}']`)]);
    const input = /** @type {HTMLInputElement} */ (field);
    switch (kind) {
      case "check":
        return input.checked;
      case "number": {
        const n = Number(input.value);
        return Number.isFinite(n) ? n : undefined;
      }
      case "optional":
        return input.value.trim() === "" ? null : Number(input.value);
      case "pair":
      case "rect":
        return peers.map((p) => Math.round(Number(p.value) || 0));
      case "abilities":
        return peers.filter((p) => p.checked).map((p) => p.value);
      case "color": {
        const text = input.value.trim();
        return text === "" ? null : /^#[0-9a-f]{6}$/i.test(text) ? text.toLowerCase() : undefined;
      }
      default:
        return input.value;
    }
  }

  q("[data-game-inspector]").addEventListener("change", (e) => {
    const field = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-field]"));
    const target = /** @type {HTMLElement | null} */ (field?.closest("[data-game-target]"))?.dataset.gameTarget;
    if (!field || !target) return;
    const value = valueOf(field);
    if (value === undefined) {
      renderInspector();
      return;
    }
    send({ type: "gameEdit", target, ops: [{ op: "set", path: JSON.parse(field.dataset.gameField ?? "[]"), value }], label: "inspector" });
  });
  q("[data-game-inspector]").addEventListener("click", (e) => {
    if (/** @type {HTMLElement} */ (e.target).closest("[data-game-remove]")) removeSelected();
  });

  function removeSelected() {
    const shape = inspected();
    if (!shape?.remove) return;
    send({ type: "gameEdit", target: shape.target, ops: [{ op: "remove", path: shape.remove }], label: "remove" });
    selected = null;
  }

  function renderRun() {
    const box = q("[data-game-run]");
    if (!project) {
      box.innerHTML = "";
      return;
    }
    const game = project.game;
    const start = project.rooms.map((r) => `<option value="${esc(r.id)}"${r.id === game.start.room ? " selected" : ""}>${esc(r.id)}</option>`).join("");
    box.innerHTML =
      `<section class="game-tool"><h4>Where a run starts</h4><div class="game-fields" data-game-target="game">` +
      `<label class="game-field"><span>Room</span><select data-game-field='["start","room"]' data-game-kind="select">${start}</select></label>` +
      `<label class="game-field"><span>Spawn</span><input type="text" data-game-field='["start","spawn"]' data-game-kind="text" value="${esc(game.start.spawn)}" spellcheck="false"></label>` +
      `</div></section>` +
      `<section class="game-tool"><h4>Abilities</h4>${
        game.abilities.length
          ? `<p class="game-faint">${game.abilities.map((a) => esc(a.name || a.id)).join(" · ")}</p>`
          : `<p class="game-faint">None yet.</p>`
      }</section>` +
      `<section class="game-tool"><h4>Play</h4><p class="game-faint">Playing a room comes with the engine's runtime: the transport over the stage lights up then.</p></section>`;
  }
  q("[data-game-run]").addEventListener("change", (e) => {
    const field = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-field]"));
    if (!field) return;
    send({ type: "gameEdit", target: "game", ops: [{ op: "set", path: JSON.parse(field.dataset.gameField ?? "[]"), value: valueOf(field) }], label: "start" });
  });

  /* ── keys ────────────────────────────────────────────────────── */

  /** Game mode's own keys. @param {KeyboardEvent} e @returns {boolean} whether it was Game's */
  function key(e) {
    if (!shown || !project) return false;
    const target = /** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null);
    const typing = Boolean(target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
    if (e.metaKey && !e.altKey && !e.ctrlKey) {
      const k = e.key.toLowerCase();
      if (k === "s") return (send({ type: "gameSave" }), true);
      // In a field, ⌘Z is the field's.
      if (k === "z") return !typing && (send({ type: e.shiftKey ? "gameRedo" : "gameUndo" }), true);
      return false;
    }
    if (typing || e.altKey || e.ctrlKey || e.metaKey) return false;
    if (e.key === "?") return (host.keys?.("— Game's keys", KEYS), true);
    if (e.key === "Escape") return (select(null), true);
    if (e.key === "Backspace" || e.key === "Delete") return (removeSelected(), true);
    if (e.key === "[" || e.key === "]") {
      const open = project.open;
      const at = open.indexOf(project.focus ?? "");
      const next = open[(at + (e.key === "]" ? 1 : -1) + open.length) % Math.max(1, open.length)];
      if (next && next !== project.focus) send({ type: "gameFocus", room: next });
      return true;
    }
    const digit = Number(e.key);
    if (digit >= 1 && digit <= 4) return (showSide(SIDES[digit - 1]), true);
    return false;
  }

  /* ── what the core says ──────────────────────────────────────── */

  function render() {
    renderHome();
    renderBar();
    renderAsk();
    renderSide();
    renderInspector();
    renderRun();
    paint();
  }

  showSide(side);
  render();

  return {
    /** @param {{games: Found[], templates: Template[]}} event */
    home(event) {
      home = { games: event.games ?? [], templates: event.templates ?? [] };
      renderHome();
    },
    /** @param {{project: Project | null}} event */
    project(event) {
      const was = project?.path;
      project = event.project ?? null;
      if (!project || project.path !== was) {
        selected = null;
        asking = false;
      }
      render();
    },
    /** @param {{room: Room | null, revision: number}} event */
    room(event) {
      const was = room?.id;
      room = event.room ?? null;
      // What was selected may have been removed, or the room changed.
      if (room?.id !== was && selected?.kind !== "game" && selected?.kind !== "asset") selected = null;
      renderSide();
      renderInspector();
      paint();
    },
    /** The mode came on screen, or went. @param {boolean} on */
    show(on) {
      shown = on;
      if (on) paint();
    },
    key,
  };
}
