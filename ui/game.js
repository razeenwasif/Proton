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
 * is always what would be saved.
 *
 * An animation (`.anim.json`, made from an Aseprite file or a sprite sheet)
 * is loaded as its sheets; what stands in a room is drawn as the frame it
 * rests on, and Play draws the frame the run is at. Its inspector plays a
 * clip small, and says its boxes and cues. */

import { escapeHtml as esc } from "./html.js";
import { draw, handle, pick, toRoom, toStage } from "./game-overlay.js";
import { corner, fitCam, layout, maskOf, pickItem, restingFrame, rgb, toPlane, viewOf } from "./game-scene.js";
import { createGameRender } from "./game-render.js";
import { createPlayer } from "./game-play.js";
import { readGame } from "./bridge.js";

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */
/** @typedef {import('./bindings/GameProjectView').GameProjectView} Project */
/** @typedef {import('./bindings/GameFound').GameFound} Found */
/** @typedef {import('./bindings/GameTemplate').GameTemplate} Template */
/** @typedef {import('./game-overlay.js').Selected} Selected */
/** @typedef {import('./bindings/GameAnim').GameAnim} Anim */
/** @typedef {import('./bindings/GamePrefab').GamePrefab} Prefab */

/** The Game mode's keys, as ? lists them. */
const KEYS = /** @type {[string, string][]} */ ([
  ["⌘S", "save every changed room and file"], ["⌘Z ⇧⌘Z", "undo, redo — wherever the change was"],
  ["⎋", "select nothing; with nothing selected, clear the agent's run"], ["⌫", "remove what is selected"], ["[ ]", "the room tab before, after"],
  ["1 2 3 4", "World, Layers, Entities, Assets"], ["v p", "select; draw collision"], ["↩ ⎋", "end the line; drop it"],
  ["⌥ click", "a point on the selected line"], ["double-click", "take a point away"], ["f", "the whole room"], ["two fingers", "move over the room"],
  ["pinch ⌘ wheel", "zoom"], ["␣ drag", "move over the room"], ["▶", "play the room — the game has the keys; ⎋ stops"], ["?", "these keys"],
]);

/** What rooms place without a prefab of the game's own. */
const BUILT_IN = ["transition", "save_point", "ability_gate", "ability_pickup"];

const SIDES = ["world", "layers", "entities", "assets"];

/** Whether an asset is an image to place: a picture, or one cut into pieces. @param {string} a */
const placeable = (a) => /\.(png|jpe?g|webp)$/i.test(a) || a.endsWith(".img.json");
/** Whether an asset is art to bring in as planes. @param {string} a */
const importable = (a) => /\.(psd|png)$/i.test(a) || animatable(a);
/** Whether an asset is made into an animation: an Aseprite file, or a sprite sheet's JSON. @param {string} a */
const animatable = (a) => /\.(aseprite|ase)$/i.test(a) || (/\.json$/i.test(a) && !a.endsWith(".anim.json") && !a.endsWith(".img.json"));
/** Whether an asset is an animation a prefab can play. @param {string} a */
const animation = (a) => a.endsWith(".anim.json");

/** The states an animated thing plays a clip in (proton-game's `STATES`), and what each is. */
const STATES = /** @type {[string, string][]} */ ([
  ["idle", "Standing"], ["run", "Running"], ["jump", "Rising"], ["fall", "Falling"], ["dash", "Dashing"], ["wall", "On a wall"], ["hurt", "Hurt"],
]);

/** How far a point is from a segment. @param {number} px @param {number} py @param {number[]} a @param {number[]} b */
function segmentDistance(px, py, a, b) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const length = dx * dx + dy * dy;
  const t = length === 0 ? 0 : Math.max(0, Math.min(1, ((px - a[0]) * dx + (py - a[1]) * dy) / length));
  return Math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy));
}

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
  /** The game being played in the stage, once the runtime is up.
   *  @type {ReturnType<typeof createPlayer> | null} */
  let player = null;
  const playing = () => Boolean(player?.playing());
  /** The abilities a run from here starts with: the Run tab's toggles. */
  const granted = new Set();
  /** What to do with the next run when it arrives: Step asks for one paused. */
  let startPaused = false;
  /** Why the last run could not start, shown in the Run tab. */
  let playError = "";
  /** Whether Play draws animations' boxes. */
  let showBoxes = pref("boxes", "") === "1";
  /** The inspector's preview of an animation: which clip, and its timer. */
  let preview = { clip: "", timer: 0, frame: 0, forward: true };
  /** Where the agent's last run went, room by room: drawn until ⎋ or a run of
   *  the person's own. @type {{room: string, points: number[][], hurt: number[][]}[]} */
  let agentPath = [];

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
  /** Where a file was imported into, or that it changed since, for its row. @param {string} a */
  const imported = (a) => {
    const done = project?.imports?.find((i) => i.source === a);
    if (!done) return "";
    return done.changed ? `<span class="game-changed" title="Saved again since it was imported">changed</span>` : `in ${esc(done.room)}`;
  };
  const row = (/** @type {string} */ kind, /** @type {string} */ id, /** @type {string} */ label, /** @type {string} */ meta = "", extra = "", after = "") => {
    const on = selected?.kind === kind && selected.id === id;
    return `<div class="game-row${on ? " on" : ""}" data-game-pick="${kind}" data-game-pick-id="${esc(id)}"${extra}>` +
      `<span class="game-row-name">${esc(label)}</span>${meta ? `<span class="game-row-meta">${meta}</span>` : ""}${after}</div>`;
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
      const prefabRows = project.prefabs
        .map((p) => row("prefab", p.id, p.name || p.id, esc(p.sprite && animation(p.sprite) ? "animated" : p.behaviour.kind)))
        .join("");
      box.innerHTML =
        row("game", "game", "Game settings", project.gameDirty ? `<span class="dot"></span>` : "") +
        (prefabRows ? `<h5 class="game-head">Prefabs</h5>${prefabRows}` : "") +
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
          const off = solo ? solo !== p.id : hidden.has(p.id);
          const toggles =
            `<button class="game-toggle${off ? " off" : ""}${solo === p.id ? " solo" : ""}" data-game-hide="${esc(p.id)}" title="Show or hide it · ⌥ shows it alone" aria-pressed="${!off}">${off ? "◌" : "●"}</button>` +
            `<button class="game-toggle${locked.has(p.id) ? " on" : ""}" data-game-lock="${esc(p.id)}" title="Lock it, so a click on the stage passes through" aria-pressed="${locked.has(p.id)}">${locked.has(p.id) ? "▣" : "□"}</button>`;
          return row("plane", p.id, p.name || p.id, esc(meta), "", toggles) + (items ? `<div class="game-sub">${items}</div>` : "");
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
      ? project.assets.map((a) => row("asset", a, a, imported(a), placeable(a) ? ` draggable="true" title="Drag it onto the stage to place it"` : "")).join("")
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
    const hide = /** @type {HTMLElement | null} */ (target.closest("[data-game-hide]"));
    if (hide) {
      const id = hide.dataset.gameHide ?? "";
      if (/** @type {MouseEvent} */ (e).altKey) solo = solo === id ? null : id;
      else if (solo) solo = null;
      else if (hidden.has(id)) hidden.delete(id);
      else hidden.add(id);
      renderSide();
      paint();
      return;
    }
    const lock = /** @type {HTMLElement | null} */ (target.closest("[data-game-lock]"));
    if (lock) {
      const id = lock.dataset.gameLock ?? "";
      if (locked.has(id)) locked.delete(id);
      else locked.add(id);
      renderSide();
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
    if (what && (what.kind !== "asset" || importable(what.id) || animation(what.id))) showPanel("inspector");
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

  /** The art, drawn by the GPU — or nothing, where WebGL 2 is not to be had,
   *  and the overlay alone shows the room. */
  const glCanvas = /** @type {HTMLCanvasElement} */ (q("[data-game-gl]"));
  let gpu = (() => {
    try {
      return createGameRender(glCanvas, { onLost: () => { gpu = null; images.clear(); paint(); } });
    } catch {
      return null;
    }
  })();
  glCanvas.hidden = !gpu;

  /** Each image's size once it is on the GPU, and the ones on their way.
   *  @type {Map<string, [number, number]>} */
  const images = new Map();
  /** Images cut into pieces (`.img.json`): where each piece goes.
   *  @type {Map<string, import('./game-scene.js').Parts>} */
  const parts = new Map();
  /** Each image's opacity, small, so a click finds what is drawn there.
   *  @type {Map<string, {w: number, h: number, alpha: Uint8Array}>} */
  const masks = new Map();
  /** Animations loaded, by their `.anim.json`. @type {Map<string, Anim>} */
  const anims = new Map();
  /** A sheet's pixels, for the inspector's preview to draw from. @type {Map<string, ImageBitmap>} */
  const bitmaps = new Map();
  /** Whether an image is drawn at u, v. @param {string} key @param {number} u @param {number} v */
  const solid = (key, u, v) => {
    const m = masks.get(key);
    if (!m) return true;
    const x = Math.min(m.w - 1, Math.max(0, Math.floor(u * m.w)));
    const y = Math.min(m.h - 1, Math.max(0, Math.floor(v * m.h)));
    return m.alpha[y * m.w + x] > 24;
  };
  /** @type {Set<string>} */
  const loading = new Set();
  /** @type {Set<string>} */
  const failed = new Set();

  /** Load what a room draws and has not been loaded. @param {Room} r */
  function load(r) {
    const wanted = new Set(r.planes.flatMap((p) => p.items.map((i) => i.image)));
    for (const s of sprites(r)) wanted.add(s.key);
    loadKeys(wanted);
  }

  /** Load these images, those not loaded yet. @param {Iterable<string>} wanted */
  function loadKeys(wanted) {
    if (!gpu) return;
    const filter = project?.game.view.filter === "nearest" ? "nearest" : "smooth";
    for (const key of wanted) {
      if (!key || images.has(key) || loading.has(key) || failed.has(key)) continue;
      // An animation: what its frames are first, then its sheets.
      if (key.endsWith(".anim.json")) {
        loading.add(key);
        readGame(key)
          .then((bytes) => JSON.parse(new TextDecoder().decode(bytes)))
          .then((anim) => {
            anims.set(key, anim);
            images.set(key, anim.size ?? [0, 0]);
            loadKeys(anim.sheets ?? []);
            if (selected?.kind === "asset" && selected.id === key) renderInspector();
          })
          .catch(() => failed.add(key))
          .finally(() => {
            loading.delete(key);
            if (!playing()) paint();
          });
        continue;
      }
      // An image cut up: what its pieces are first, then the pieces.
      if (key.endsWith(".img.json")) {
        loading.add(key);
        readGame(key)
          .then((bytes) => JSON.parse(new TextDecoder().decode(bytes)))
          .then((sheet) => {
            const chunks = (sheet.chunks ?? []).filter((/** @type {any} */ c) => c && typeof c.file === "string");
            parts.set(key, { size: sheet.size ?? [0, 0], chunks });
            images.set(key, sheet.size ?? [0, 0]);
            loadKeys(chunks.map((/** @type {any} */ c) => c.file));
          })
          .catch(() => failed.add(key))
          .finally(() => {
            loading.delete(key);
            if (!playing()) paint();
          });
        continue;
      }
      if (!/\.(png|jpe?g|webp)$/i.test(key)) continue;
      loading.add(key);
      readGame(key)
        .then((bytes) => createImageBitmap(new Blob([bytes])))
        .then((bitmap) => {
          gpu?.upload(key, bitmap, filter);
          images.set(key, [bitmap.width, bitmap.height]);
          const mask = maskOf(bitmap);
          if (mask) masks.set(key, mask);
          bitmap.close?.();
        })
        .catch(() => failed.add(key))
        .finally(() => {
          loading.delete(key);
          if (!playing()) paint();
        });
    }
  }

  /** What stands in the room drawn as its prefab's sprite: enemies, props,
   *  and the player where a run starts — an animation as the frame it rests
   *  on, on that frame's feet. @param {Room} r */
  function sprites(r) {
    if (!project) return [];
    const prefabs = new Map(project.prefabs.map((p) => [p.id, p]));
    /** @type {import('./game-scene.js').Standing[]} */
    const out = [];
    const standing = (/** @type {Prefab} */ prefab, /** @type {number[]} */ at) => {
      if (!prefab.sprite) return;
      const anim = anims.get(prefab.sprite);
      if (!anim && animation(prefab.sprite)) return void loadKeys([prefab.sprite]);
      const frame = anim ? restingFrame(anim, prefab.clips) : null;
      const pivot = (anim && frame !== null ? anim.frames[frame]?.pivot : null) ?? prefab.pivot;
      out.push({ key: prefab.sprite, at, pivot, frame });
    };
    for (const e of r.entities) {
      const prefab = prefabs.get(e.prefab);
      if (prefab && e.at) standing(prefab, e.at);
    }
    const player = project.prefabs.find((p) => project && project.game.player.endsWith(`${p.id}.prefab.json`));
    const start = project.game.start.room === r.id ? r.spawns.find((s) => s.id === project?.game.start.spawn) : null;
    if (player && start) standing(player, start.at);
    return out;
  }

  /** The editor's camera for each room, kept while the game is open — and
   *  fitted to the stage again as the stage changes size, until the person
   *  has moved it themselves. @type {Map<string, import('./game-scene.js').Cam & {moved?: boolean}>} */
  const cams = new Map();
  /** Planes hidden, locked, or shown alone — the editor's, never saved. */
  const hidden = new Set();
  const locked = new Set();
  /** @type {string | null} */
  let solo = null;
  /** A drag under way: what moves, from where, and the room as it is meanwhile.
   *  @type {{kind: "move" | "scale" | "pan" | "vertex", from: [number, number], at: [number, number], moved: boolean, base: any, picked: Selected | null, index?: number} | null} */
  let drag = null;
  /** What a click on the stage does: selects, or draws collision. */
  let tool = "select";
  /** A line of collision being drawn. @type {{points: number[][], hover: number[] | null, kind: string}} */
  const pen = { points: [], hover: null, kind: "solid" };

  /** @param {string} which */
  function useTool(which) {
    tool = which;
    ws.dataset.tool = which;
    for (const b of ws.querySelectorAll("[data-game-tool]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.gameTool === which));
    q("[data-game-pen-kind]").hidden = which !== "pen";
    pen.points = [];
    pen.hover = null;
    paint();
  }
  q("[data-game-tools]").addEventListener("click", (e) => {
    const b = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-tool]"));
    if (b) useTool(b.dataset.gameTool ?? "select");
  });
  q("[data-game-pen-kind]").addEventListener("change", (e) => {
    pen.kind = /** @type {HTMLSelectElement} */ (e.target).value;
    paint();
  });

  /** Where a point drawn with the pen goes: onto a point already there when
   *  near one, and held to eighths of a turn from the last with ⇧.
   *  @param {number} sx @param {number} sy @param {boolean} straight */
  function penPoint(sx, sy, straight) {
    const c = cam();
    if (!c || !room) return null;
    const [w, h] = stageSize();
    const view = viewOf(c, w, h);
    for (const p of [...pen.points.slice(0, 1), ...room.collision.flatMap((s) => s.points)]) {
      const [x, y] = toStage(view, p);
      if (Math.hypot(x - sx, y - sy) <= 8) return [p[0], p[1]];
    }
    let [x, y] = toRoom(view, sx, sy);
    const last = pen.points[pen.points.length - 1];
    if (straight && last) {
      const angle = Math.round(Math.atan2(y - last[1], x - last[0]) / (Math.PI / 4)) * (Math.PI / 4);
      const length = Math.hypot(x - last[0], y - last[1]);
      x = last[0] + Math.cos(angle) * length;
      y = last[1] + Math.sin(angle) * length;
    }
    return [Math.round(x), Math.round(y)];
  }

  /** The line drawn, added to the room: closed when it ended on its start. @param {boolean} closed */
  function finishPen(closed) {
    if (!room || pen.points.length < 2) {
      pen.points = [];
      paint();
      return;
    }
    const taken = new Set(room.collision.map((s) => s.id));
    let n = room.collision.length + 1;
    while (taken.has(`shape_${n}`)) n++;
    const id = `shape_${n}`;
    send({ type: "gameEdit", target: `room:${room.id}`, ops: [{ op: "insert", path: ["collision"], value: { id, kind: pen.kind, closed: closed && pen.points.length >= 3, points: pen.points } }], label: "draw" });
    selected = { kind: "shape", id };
    pen.points = [];
    pen.hover = null;
    paint();
  }
  /** The room as a drag has it, drawn in place of the room until the drag ends.
   *  @type {Room | null} */
  let draft = null;

  function stageSize() {
    return [stage.clientWidth, stage.clientHeight];
  }

  /** The editor's camera on the room on screen. */
  function cam() {
    if (!room) return null;
    const [w, h] = stageSize();
    if (!w || !h) return null;
    let held = cams.get(room.id);
    if (!held || !held.moved) {
      held = { ...fitCam(room, w, h), moved: false };
      cams.set(room.id, held);
    }
    return held;
  }

  /** How the scene is laid out for the camera. */
  function how() {
    const c = cam();
    const view = /** @type {[number, number]} */ ([project?.game.view.width ?? 1920, project?.game.view.height ?? 1080]);
    return { focal: project?.game.view.focal ?? 1000, view, center: c?.center ?? /** @type {[number, number]} */ ([0, 0]), sizes: images, parts, anims, hidden, solo, sprites: room ? sprites(draft ?? room) : [] };
  }

  /** What the scene draws now. */
  function drawn() {
    const r = draft ?? room;
    return r ? layout(r, how()) : [];
  }

  /** The selected image's box in room pixels, when an image is selected. */
  function selectedBox() {
    if (selected?.kind !== "item") return null;
    const s = drawn().find((d) => d.item === selected?.id);
    if (!s) return null;
    const box = s.whole ?? s;
    return { x: box.x, y: box.y, w: box.w, h: box.h, rot: s.rot };
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
    // A run draws itself, every frame, until it stops.
    if (playing()) {
      empty.hidden = true;
      canvas.hidden = false;
      glCanvas.hidden = !gpu;
      return;
    }
    empty.hidden = Boolean(room) || !project;
    if (!room && project) empty.innerHTML = `<p class="game-faint">Open a room from the World tab.</p>`;
    canvas.hidden = !room;
    glCanvas.hidden = !room || !gpu;
    if (!room || !shown) return;
    const [width, height] = stageSize();
    const c = cam();
    if (!width || !height || !c) return;
    const ratio = window.devicePixelRatio || 1;
    const view = viewOf(c, width, height);
    const shade = colors();
    const r = draft ?? room;
    load(r);
    gpu?.draw(drawn(), view, width, height, ratio, rgb(shade.ground));
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const g = canvas.getContext("2d");
    if (!g) return;
    g.setTransform(ratio, 0, 0, ratio, 0, 0);
    g.clearRect(0, 0, width, height);
    const frame = how();
    const at = corner(r, frame.view, frame.center);
    const line = selected?.kind === "shape" ? r.collision.find((x) => x.id === selected?.id) : null;
    draw(g, r, view, shade, selected, {
      art: Boolean(gpu), box: selectedBox(), camera: gpu ? [at[0], at[1], frame.view[0], frame.view[1]] : null,
      vertices: line?.points ?? null, pen: tool === "pen" ? pen : null,
      path: agentPath.filter((p) => p.room === r.id),
    });
  }
  new ResizeObserver(() => paint()).observe(stage);

  player = createPlayer({
    canvas,
    gpu: () => gpu,
    sizes: images,
    parts,
    anims,
    boxes: () => showBoxes,
    want: (keys) => loadKeys(keys),
    stageSize,
    focal: () => project?.game.view.focal ?? 1000,
    ground: () => rgb(colors().ground),
    dispatch: send,
    changed: (state) => renderTransport(state),
    ended: () => paint(),
  });

  /** The transport over the stage, as the run stands.
   *  @param {{playing: boolean, paused: boolean, tick: number}} [state] */
  function renderTransport(state = { playing: playing(), paused: Boolean(player?.paused()), tick: 0 }) {
    const button = (/** @type {string} */ name) => /** @type {HTMLButtonElement} */ (q(`[data-game-${name}]`));
    const can = Boolean(project?.focus && room);
    button("play").disabled = state.playing ? !state.paused : !can;
    button("play").title = state.playing ? "Go on" : "Play from here — this room, with the Run tab's abilities";
    button("pause").disabled = !state.playing || state.paused;
    button("step").disabled = state.playing ? false : !can;
    button("stop").disabled = !state.playing;
    ws.classList.toggle("game-playing", state.playing);
    const tick = ws.querySelector("[data-game-tick]");
    if (tick) tick.textContent = state.playing ? `tick ${state.tick}${state.paused ? " · paused" : ""}` : "";
  }

  /** A run from this room, with the abilities the Run tab grants. */
  function play() {
    if (!project?.focus) return;
    playError = "";
    agentPath = [];
    send({ type: "gamePlay", room: project.focus, abilities: [...granted] });
  }
  q("[data-game-play]").addEventListener("click", () => (playing() ? player?.pause(false) : play()));
  q("[data-game-pause]").addEventListener("click", () => player?.pause(true));
  q("[data-game-step]").addEventListener("click", () => {
    if (playing()) return player?.step();
    startPaused = true;
    play();
  });
  q("[data-game-stop]").addEventListener("click", () => player?.stop());

  /** A stage point, from a pointer. @param {MouseEvent} e @returns {[number, number]} */
  function stagePoint(e) {
    const box = canvas.getBoundingClientRect();
    return [e.clientX - box.left, e.clientY - box.top];
  }

  /** Fit the room in the stage again. */
  function refit() {
    if (!room) return;
    cams.delete(room.id);
    paint();
  }

  let space = false;
  stage.addEventListener("keydown", (e) => { if (e.key === " ") space = true; });
  stage.addEventListener("keyup", (e) => { if (e.key === " ") space = false; });

  // Two fingers move over the room; a pinch, or ⌘ with the wheel, zooms
  // about the pointer.
  stage.addEventListener("wheel", (e) => {
    if (playing()) return;
    const c = cam();
    if (!c || !room) return;
    e.preventDefault();
    c.moved = true;
    if (e.ctrlKey || e.metaKey) {
      const [sx, sy] = stagePoint(e);
      const [w, h] = stageSize();
      const before = toRoom(viewOf(c, w, h), sx, sy);
      c.zoom = Math.min(8, Math.max(0.02, c.zoom * Math.exp(-e.deltaY * 0.01)));
      const after = toRoom(viewOf(c, w, h), sx, sy);
      c.center = [c.center[0] + before[0] - after[0], c.center[1] + before[1] - after[1]];
    } else {
      c.center = [c.center[0] + e.deltaX / c.zoom, c.center[1] + e.deltaY / c.zoom];
    }
    paint();
  }, { passive: false });

  canvas.addEventListener("pointerdown", (e) => {
    if (playing()) return;
    const c = cam();
    if (!room || !c) return;
    stage.focus({ preventScroll: true });
    const [sx, sy] = stagePoint(e);
    const [w, h] = stageSize();
    const view = viewOf(c, w, h);
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch {
      // A pointer the browser does not know — a test's — is not held.
    }
    if (e.button === 1 || space) {
      drag = { kind: "pan", from: [sx, sy], at: [sx, sy], moved: false, base: [...c.center], picked: null };
      return;
    }
    if (tool === "pen") {
      const p = penPoint(sx, sy, e.shiftKey);
      if (!p) return;
      const start = pen.points[0];
      if (pen.points.length >= 3 && start && p[0] === start[0] && p[1] === start[1]) return finishPen(true);
      if (e.detail >= 2) return finishPen(false);
      pen.points.push(p);
      paint();
      return;
    }
    // A selected line's points move; ⌥ on the line adds one; a double click
    // on one takes it away.
    const shape = selected?.kind === "shape" ? room.collision.find((x) => x.id === selected?.id) : null;
    if (shape) {
      const index = shape.points.findIndex((p) => {
        const [x, y] = toStage(view, p);
        return Math.abs(x - sx) <= 7 && Math.abs(y - sy) <= 7;
      });
      if (index >= 0 && e.detail >= 2 && shape.points.length > 2) {
        const points = shape.points.filter((_, i) => i !== index);
        send({ type: "gameEdit", target: `room:${room.id}`, ops: [{ op: "set", path: ["collision", `#${shape.id}`, "points"], value: points }], label: "remove a point" });
        return;
      }
      if (index >= 0) {
        drag = { kind: "vertex", from: [sx, sy], at: [sx, sy], moved: false, base: structuredClone(room), picked: selected, index };
        return;
      }
      if (e.altKey) {
        const [rx, ry] = toRoom(view, sx, sy);
        const ring = shape.closed ? [...shape.points, shape.points[0]] : shape.points;
        let best = -1;
        let bestAt = 8 / view.scale;
        for (let i = 1; i < ring.length; i++) {
          const at = segmentDistance(rx, ry, ring[i - 1], ring[i]);
          if (at <= bestAt) {
            bestAt = at;
            best = i;
          }
        }
        if (best >= 0) {
          const points = [...shape.points];
          points.splice(best, 0, [Math.round(rx), Math.round(ry)]);
          send({ type: "gameEdit", target: `room:${room.id}`, ops: [{ op: "set", path: ["collision", `#${shape.id}`, "points"], value: points }], label: "add a point" });
          return;
        }
      }
    }
    // The selected image's handle scales it.
    const box = selectedBox();
    if (box) {
      const [hx, hy] = handle(view, box);
      if (Math.abs(sx - hx) <= 7 && Math.abs(sy - hy) <= 7) {
        drag = { kind: "scale", from: [sx, sy], at: [sx, sy], moved: false, base: box, picked: selected };
        return;
      }
    }
    const [rx, ry] = toRoom(view, sx, sy);
    const thing = pick(room, view, sx, sy);
    const art = thing ? null : pickItem(drawn(), rx, ry, locked, solid);
    /** @type {Selected | null} */
    const picked = thing ?? (art?.item ? { kind: "item", id: art.item, plane: art.plane } : null);
    select(picked ?? { kind: "room", id: room.id });
    if (picked) drag = { kind: "move", from: [sx, sy], at: [sx, sy], moved: false, base: structuredClone(room), picked };
  });

  canvas.addEventListener("pointermove", (e) => {
    if (playing()) return;
    const c = cam();
    if (tool === "pen" && room && c && !drag) {
      const [sx, sy] = stagePoint(e);
      pen.hover = pen.points.length ? penPoint(sx, sy, e.shiftKey) : null;
      paint();
      return;
    }
    if (!drag || !room || !c) return;
    const [sx, sy] = stagePoint(e);
    drag.at = [sx, sy];
    if (!drag.moved && Math.hypot(sx - drag.from[0], sy - drag.from[1]) < 3) return;
    drag.moved = true;
    const dx = (sx - drag.from[0]) / c.zoom;
    const dy = (sy - drag.from[1]) / c.zoom;
    if (drag.kind === "pan") {
      c.moved = true;
      c.center = [drag.base[0] - dx, drag.base[1] - dy];
    } else if (drag.kind === "vertex" && drag.picked && drag.index !== undefined) {
      const r = structuredClone(drag.base);
      const s = r.collision.find((/** @type {any} */ x) => x.id === drag?.picked?.id);
      const was = drag.base.collision.find((/** @type {any} */ x) => x.id === drag?.picked?.id)?.points[drag.index];
      if (s && was) s.points[drag.index] = [was[0] + Math.round(dx), was[1] + Math.round(dy)];
      draft = r;
    } else if (drag.kind === "move" && drag.picked) {
      draft = moved(drag.base, drag.picked, Math.round(dx), Math.round(dy));
    } else if (drag.kind === "scale" && drag.picked) {
      const box = drag.base;
      const grow = Math.max(0.05, (box.w + dx) / Math.max(1, box.w));
      draft = scaled(room, drag.picked, grow);
    }
    paint();
  });

  canvas.addEventListener("pointerup", () => {
    const done = drag;
    drag = null;
    if (!done || !room || !done.moved || done.kind === "pan" || !done.picked) {
      draft = null;
      paint();
      return;
    }
    const edit = commit(done.picked, draft);
    draft = null;
    if (edit) send({ type: "gameEdit", target: `room:${room.id}`, ops: [edit], label: done.kind });
    paint();
  });

  /** The room with `what` moved by `dx`, `dy` room pixels.
   *  @param {Room} base @param {Selected} what @param {number} dx @param {number} dy @returns {Room} */
  function moved(base, what, dx, dy) {
    const r = structuredClone(base);
    const by = (/** @type {number[]} */ p) => [p[0] + dx, p[1] + dy];
    if (what.kind === "entity") {
      const e = r.entities.find((x) => x.id === what.id);
      if (e?.at) e.at = /** @type {[number, number]} */ (by(e.at));
      if (e?.rect) e.rect = [e.rect[0] + dx, e.rect[1] + dy, e.rect[2], e.rect[3]];
    } else if (what.kind === "spawn") {
      const s = r.spawns.find((x) => x.id === what.id);
      if (s) s.at = /** @type {[number, number]} */ (by(s.at));
    } else if (what.kind === "light") {
      const l = r.lights.find((x) => x.id === what.id);
      if (l) l.at = /** @type {[number, number]} */ (by(l.at));
    } else if (what.kind === "shape") {
      const s = r.collision.find((x) => x.id === what.id);
      if (s) s.points = s.points.map((p) => /** @type {[number, number]} */ (by(p)));
    } else if (what.kind === "item") {
      for (const p of r.planes) for (const i of p.items) if (i.id === what.id) i.at = /** @type {[number, number]} */ (by(i.at));
    }
    return r;
  }

  /** The room with the image `what` scaled by `grow`. @param {Room} base @param {Selected} what @param {number} grow */
  function scaled(base, what, grow) {
    const r = structuredClone(base);
    for (const p of r.planes) for (const i of p.items) if (i.id === what.id) i.scale = Math.round(i.scale * grow * 1000) / 1000;
    return r;
  }

  /** The one op that says what a drag did. @param {Selected} what @param {Room | null} r */
  function commit(what, r) {
    if (!r) return null;
    const set = (/** @type {string[]} */ path, /** @type {any} */ value) => ({ op: "set", path, value });
    if (what.kind === "entity") {
      const e = r.entities.find((x) => x.id === what.id);
      if (!e) return null;
      return e.rect ? set(["entities", `#${e.id}`, "rect"], e.rect) : set(["entities", `#${e.id}`, "at"], e.at);
    }
    if (what.kind === "spawn") {
      const s = r.spawns.find((x) => x.id === what.id);
      return s ? set(["spawns", `#${s.id}`, "at"], s.at) : null;
    }
    if (what.kind === "light") {
      const l = r.lights.find((x) => x.id === what.id);
      return l ? set(["lights", `#${l.id}`, "at"], l.at) : null;
    }
    if (what.kind === "shape") {
      const s = r.collision.find((x) => x.id === what.id);
      return s ? set(["collision", `#${s.id}`, "points"], s.points) : null;
    }
    if (what.kind === "item") {
      for (const p of r.planes) {
        const i = p.items.find((x) => x.id === what.id);
        if (i) {
          const was = room?.planes.find((x) => x.id === p.id)?.items.find((x) => x.id === i.id);
          if (was && was.scale !== i.scale) return set(["planes", `#${p.id}`, "items", `#${i.id}`, "scale"], i.scale);
          return set(["planes", `#${p.id}`, "items", `#${i.id}`, "at"], i.at);
        }
      }
    }
    return null;
  }

  // An image dragged in from Assets is placed where it is dropped, on the
  // plane selected — or the one the game is played on.
  stage.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types.includes("text/x-proton-asset")) e.preventDefault();
  });
  stage.addEventListener("drop", (e) => {
    const key = e.dataTransfer?.getData("text/x-proton-asset");
    const c = cam();
    if (!key || !room || !c || playing()) return;
    e.preventDefault();
    const planeId = selected?.kind === "plane" ? selected.id : selected?.kind === "item" ? selected.plane : null;
    const plane = room.planes.find((p) => p.id === planeId) ?? room.planes.find((p) => p.gameplay) ?? room.planes[room.planes.length - 1];
    if (!plane) return;
    const [w, h] = stageSize();
    const [sx, sy] = stagePoint(e);
    const point = toPlane(room, plane, how(), toRoom(viewOf(c, w, h), sx, sy));
    const size = images.get(key) ?? [0, 0];
    const base = (key.split("/").pop() ?? "image").replace(/\.[^.]+$/, "").replace(/[^a-z0-9_]+/gi, "_").toLowerCase();
    const taken = new Set(room.planes.flatMap((p) => p.items.map((i) => i.id)));
    let id = base;
    for (let n = 2; taken.has(id); n++) id = `${base}_${n}`;
    const at = [Math.round(point[0] - size[0] / 2), Math.round(point[1] - size[1] / 2)];
    send({ type: "gameEdit", target: `room:${room.id}`, ops: [{ op: "insert", path: ["planes", `#${plane.id}`, "items"], value: { id, image: key, at } }], label: "place" });
    selected = { kind: "item", id, plane: plane.id };
  });
  q("[data-game-side-body]").addEventListener("dragstart", (e) => {
    const row = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-pick='asset']"));
    if (row && e.dataTransfer) {
      e.dataTransfer.setData("text/x-proton-asset", row.dataset.gamePickId ?? "");
      e.dataTransfer.effectAllowed = "copy";
    }
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
   *  @returns {{title: string, target: string, fields: Field[], remove?: string[], trace?: string} | null} */
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
        trace: plane.gameplay ? item.id : undefined,
        fields: [
          { label: "Image", path: [...at, "image"], value: item.image, type: "select", options: project.assets.filter(placeable).map((a) => [a, a]) },
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
    if (selected?.kind === "prefab") {
      const p = project.prefabs.find((x) => x.id === selected?.id);
      if (!p) return null;
      const looks = project.assets.filter((a) => placeable(a) || animation(a)).map((a) => /** @type {[string, string]} */ ([a, a]));
      /** @type {Field[]} */
      const fields = [
        { label: "Name", path: ["name"], value: p.name, type: "text" },
        { label: "Looks", path: ["sprite"], value: p.sprite ?? "", type: "select", options: looks },
        { label: "Feet", path: ["pivot"], value: p.pivot, type: "pair", hint: "from its picture's top left, where no slice says" },
        { label: "Body", path: ["size"], value: p.size, type: "pair", hint: "width × height, centred on its feet" },
        { label: "Does", path: ["behaviour", "kind"], value: p.behaviour.kind, type: "select", options: [["still", "Stands still"], ["player", "The player"], ["crawler", "Crawls"], ["flyer", "Flies"], ["charger", "Charges"], ["turret", "Shoots"]] },
      ];
      return { title: p.name || p.id, target: `prefab:${p.id}`, fields };
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
      (shape.trace ? `<p class="game-trace"><button class="cc-quiet" data-game-trace="${esc(shape.trace)}" title="Collision drawn round where it is opaque — one edit, undone whole">Trace collision</button></p>` : "") +
      `<div class="game-fields" data-game-target="${esc(shape.target)}">${shape.fields
        .map((f, i) => `<label class="game-field"><span>${esc(f.label)}</span>${input(f, i)}${f.hint ? `<small>${esc(f.hint)}</small>` : ""}</label>`)
        .join("")}</div>` +
      (selected?.kind === "asset" && importable(selected.id) ? importForm(selected.id) : "") +
      (selected?.kind === "asset" && animation(selected.id) ? animPanel(selected.id) : "") +
      (selected?.kind === "prefab" ? clipsPanel(selected.id) : "");
    startPreview();
  }

  /** A prefab's clips, state by state, when what it looks like is an animation. @param {string} id */
  function clipsPanel(id) {
    const p = project?.prefabs.find((x) => x.id === id);
    if (!p?.sprite || !animation(p.sprite)) return "";
    const anim = anims.get(p.sprite);
    if (!anim) {
      loadKeys([p.sprite]);
      return `<p class="game-faint">Loading ${esc(p.sprite)}…</p>`;
    }
    const chosen = p.clips ?? {};
    return `<section class="game-tool" data-game-clips="${esc(p.id)}"><h4>What it plays</h4>` +
      `<p class="game-faint">Each state plays the clip of its name, unless one is chosen here.</p><div class="game-fields">` +
      STATES.map(([state, label]) => {
        const own = anim.clips.some((c) => c.name.toLowerCase() === state);
        return `<label class="game-field"><span>${label}</span><select data-game-clip="${state}"><option value="">${own ? `${state} — its own` : "— the nearest"}</option>${anim.clips
          .map((c) => `<option value="${esc(c.name)}"${chosen[state] === c.name ? " selected" : ""}>${esc(c.name)}</option>`)
          .join("")}</select></label>`;
      }).join("") +
      `</div></section>`;
  }
  q("[data-game-inspector]").addEventListener("change", (e) => {
    const pick = /** @type {HTMLSelectElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-clip]"));
    const id = pick?.closest("[data-game-clips]")?.getAttribute("data-game-clips");
    const p = project?.prefabs.find((x) => x.id === id);
    if (!pick || !p) return;
    e.stopImmediatePropagation();
    /** @type {Record<string, string | undefined>} */
    const clips = { ...(p.clips ?? {}) };
    const state = pick.dataset.gameClip ?? "";
    if (pick.value) clips[state] = pick.value;
    else delete clips[state];
    send({ type: "gameEdit", target: `prefab:${p.id}`, ops: [{ op: "set", path: ["clips"], value: clips }], label: "clips" });
  }, true);

  /** An animation: its clips — one played small — its boxes and its cues,
   *  and which prefabs play it. @param {string} key */
  function animPanel(key) {
    const anim = anims.get(key);
    if (!anim) {
      loadKeys([key]);
      return `<p class="game-faint">Loading…</p>`;
    }
    if (!anim.clips.some((c) => c.name === preview.clip)) preview = { ...preview, clip: anim.clips[0]?.name ?? "", frame: 0 };
    const users = project?.prefabs.filter((p) => p.sprite === key) ?? [];
    const player = project?.prefabs.find((p) => project && project.game.player.endsWith(`${p.id}.prefab.json`));
    const ms = (/** @type {import('./bindings/GameClip').GameClip} */ c) => anim.frames.slice(c.from, c.to + 1).reduce((t, f) => t + f.ms, 0);
    const way = { forward: "on", reverse: "back", pingPong: "to the end and back", pingPongReverse: "back and on again" };
    return `<section class="game-tool game-anim" data-game-anim="${esc(key)}"><h4>Animation</h4>` +
      `<p class="game-faint">${anim.frames.length} frame${anim.frames.length === 1 ? "" : "s"} of ${anim.size[0]} × ${anim.size[1]}, from ${esc(anim.source || "—")}.</p>` +
      `<canvas class="game-anim-preview" data-game-anim-preview width="1" height="1"></canvas>` +
      `<ul class="game-clips">${anim.clips
        .map((c) => `<li><button class="game-clip${c.name === preview.clip ? " on" : ""}" data-game-preview-clip="${esc(c.name)}"><b>${esc(c.name)}</b>` +
          `<span class="game-faint">frames ${c.from + 1}–${c.to + 1}, ${way[c.direction] ?? c.direction}${c.repeat ? `, ${c.repeat}×` : ""} · ${(ms(c) / 1000).toFixed(2)} s</span></button></li>`)
        .join("")}</ul>` +
      (anim.boxes.length ? `<h5 class="game-head">Boxes</h5><p class="game-faint">${anim.boxes.map((b) => `${esc(b.name)} (${b.kind})`).join(", ")} — drawn over a run when Boxes is on in the Run tab.</p>` : "") +
      (anim.cues.length ? `<h5 class="game-head">Cues</h5><p class="game-faint">${anim.cues.map((c) => `${esc(c.name)} at frame ${c.frame + 1}`).join(", ")}</p>` : "") +
      `<p class="game-faint">${users.length ? `Played by ${users.map((p) => esc(p.name || p.id)).join(", ")}.` : "No prefab plays it yet."}</p>` +
      (player && player.sprite !== key ? `<p><button class="cc-quiet" data-game-anim-player="${esc(player.id)}">Make it the player's</button></p>` : "") +
      `</section>`;
  }
  q("[data-game-inspector]").addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const clip = /** @type {HTMLElement | null} */ (target.closest("[data-game-preview-clip]"));
    if (clip) {
      preview = { ...preview, clip: clip.dataset.gamePreviewClip ?? "", frame: -1 };
      for (const b of q("[data-game-inspector]").querySelectorAll("[data-game-preview-clip]")) b.classList.toggle("on", b === clip);
      startPreview();
    }
    const give = /** @type {HTMLElement | null} */ (target.closest("[data-game-anim-player]"));
    const key = give?.closest("[data-game-anim]")?.getAttribute("data-game-anim");
    if (give && key) send({ type: "gameEdit", target: `prefab:${give.dataset.gameAnimPlayer}`, ops: [{ op: "set", path: ["sprite"], value: key }], label: "the player's animation" });
  });

  /** The inspector's animation played: its clip's frames, each for its
   *  time, round and round while it is on screen. */
  function startPreview() {
    clearTimeout(preview.timer);
    const box = /** @type {HTMLElement | null} */ (q("[data-game-inspector]").querySelector("[data-game-anim]"));
    const c = /** @type {HTMLCanvasElement | null} */ (box?.querySelector("[data-game-anim-preview]") ?? null);
    const anim = box ? anims.get(box.dataset.gameAnim ?? "") : null;
    const clip = anim?.clips.find((x) => x.name === preview.clip);
    if (!c || !anim || !clip) return;
    const back = clip.direction === "reverse" || clip.direction === "pingPongReverse";
    if (preview.frame < clip.from || preview.frame > clip.to) preview = { ...preview, frame: back ? clip.to : clip.from, forward: !back };
    const f = anim.frames[preview.frame];
    const sheet = f ? anim.sheets[f.sheet] : null;
    const bitmap = sheet ? bitmaps.get(sheet) : null;
    if (sheet && !bitmap) {
      readGame(sheet)
        .then((bytes) => createImageBitmap(new Blob([bytes])))
        .then((b) => {
          bitmaps.set(sheet, b);
          startPreview();
        })
        .catch(() => {});
      return;
    }
    const scale = Math.max(1, Math.floor(160 / Math.max(anim.size[0], anim.size[1])));
    c.width = anim.size[0] * scale;
    c.height = anim.size[1] * scale;
    const g = c.getContext("2d");
    if (g && f && bitmap && f.rect[2] && f.rect[3]) {
      g.imageSmoothingEnabled = project?.game.view.filter !== "nearest";
      g.clearRect(0, 0, c.width, c.height);
      g.drawImage(bitmap, f.rect[0], f.rect[1], f.rect[2], f.rect[3], f.offset[0] * scale, f.offset[1] * scale, f.rect[2] * scale, f.rect[3] * scale);
    }
    c.dataset.frame = String(preview.frame);
    // The next, as the clip goes.
    const pingpong = clip.direction === "pingPong" || clip.direction === "pingPongReverse";
    let next = preview.frame + (preview.forward ? 1 : -1);
    let forward = preview.forward;
    if (next > clip.to || next < clip.from) {
      if (pingpong && clip.from < clip.to) {
        forward = !forward;
        next = preview.frame + (forward ? 1 : -1);
      } else next = forward ? clip.from : clip.to;
    }
    preview.timer = window.setTimeout(() => {
      preview = { ...preview, frame: next, forward };
      startPreview();
    }, Math.max(16, f?.ms ?? 100));
  }

  /** Bringing a .psd or .png in as a room's planes: where, how large, and
   *  whether lights get normal maps — and what it made the last time.
   *  @param {string} source */
  function importForm(source) {
    if (!project) return "";
    const done = project.imports?.find((i) => i.source === source);
    if (animatable(source)) {
      return `<section class="game-tool game-import" data-game-import-form><h4>Import as an animation</h4>` +
        `<p class="game-faint">Its frames go onto sheets beside it, with <code>${esc(source.replace(/(-sheet)?\.[^./]+$/i, ""))}.anim.json</code> to say where each is: its tags become clips, slices called <code>hit…</code> and <code>hurt…</code> boxes, one called <code>feet</code> where it stands, and a cel's text a cue. A prefab plays it once it looks like it.</p>` +
        (done?.anim ? `<p class="game-faint">Made ${esc(done.anim)} the last time. Importing again makes it again.</p>${done.changed ? `<p class="game-changed-note">The file has been saved since.</p>` : ""}` : "") +
        `<p><button class="cc-primary" data-game-import="${esc(source)}">${done?.anim ? "Import again" : "Import"}</button></p></section>`;
    }
    const into = done?.room ?? project.focus ?? project.game.start.room;
    const rooms = project.rooms.map((r) => `<option value="${esc(r.id)}"${r.id === into ? " selected" : ""}>${esc(r.id)}</option>`).join("");
    const before = done
      ? `<p class="game-faint">Imported into ${esc(done.room)}: ${done.planes} plane${done.planes === 1 ? "" : "s"}${done.items ? `, ${done.items} on the play plane` : ""}. Importing again replaces them.</p>` +
        (done.changed ? `<p class="game-changed-note">The file has been saved since.</p>` : "")
      : "";
    return `<section class="game-tool game-import" data-game-import-form><h4>Import as planes</h4>` +
      `<p class="game-faint">Each group becomes a plane at the depth in its name — <code>Far hills [800]</code> behind, <code>Leaves [-150]</code> in front, <code>[still]</code> for a sky that does not move, <code>[play]</code> for the plane the game is played on. The images are written now; the room's change waits for ⌘S.</p>` +
      before +
      `<div class="game-fields">` +
      `<label class="game-field"><span>Room</span><select data-game-import-room>${rooms}</select></label>` +
      `<label class="game-field"><span>Scale</span><input type="number" step="any" min="0.01" value="${done?.scale ?? 1}" data-game-import-scale><small>room pixels to a pixel of the file</small></label>` +
      `<label class="game-field game-check"><input type="checkbox" data-game-import-normals${done?.normals ? " checked" : ""}> <span>Normal maps, for lights</span></label>` +
      `</div><p><button class="cc-primary" data-game-import="${esc(source)}">${done ? "Import again" : "Import"}</button></p></section>`;
  }
  q("[data-game-inspector]").addEventListener("click", (e) => {
    const go = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-import]"));
    if (!go) return;
    const form = go.closest("[data-game-import-form]");
    const room = /** @type {HTMLSelectElement | null} */ (form?.querySelector("[data-game-import-room]"))?.value;
    const scale = Number(/** @type {HTMLInputElement | null} */ (form?.querySelector("[data-game-import-scale]"))?.value);
    const normals = Boolean(/** @type {HTMLInputElement | null} */ (form?.querySelector("[data-game-import-normals]"))?.checked);
    send({ type: "gameImport", source: go.dataset.gameImport, room, scale: Number.isFinite(scale) && scale > 0 ? scale : undefined, normals });
  });

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
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest("[data-game-remove]")) removeSelected();
    const trace = /** @type {HTMLElement | null} */ (target.closest("[data-game-trace]"));
    if (trace && room) send({ type: "gameTrace", room: room.id, item: trace.dataset.gameTrace });
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
      `<section class="game-tool"><h4>Abilities a run starts with</h4>${
        game.abilities.length
          ? `<div class="game-toggles game-grants">${game.abilities
              .map((a) => `<label class="game-grant"><input type="checkbox" data-game-grant="${esc(a.id)}"${granted.has(a.id) ? " checked" : ""}> ${esc(a.name || a.id)}</label>`)
              .join("")}</div>`
          : `<p class="game-faint">None yet.</p>`
      }</section>` +
      `<section class="game-tool"><h4>Play</h4>` +
      `<p class="game-faint">▶ over the stage plays the room on screen. The game has the keys while it runs; Esc stops it, and the run is kept here.</p>` +
      `<label class="game-field game-check"><input type="checkbox" data-game-boxes${showBoxes ? " checked" : ""}> <span>Boxes — an animation's hitboxes and hurtboxes, over the run</span></label>` +
      (playError ? `<p class="game-error" role="alert">${esc(playError)}</p>` : "") +
      `<p class="game-faint" data-game-tick></p></section>` +
      `<section class="game-tool"><h4>Runs</h4>${
        project.recordings.length
          ? `<ul class="game-runs">${project.recordings
              .map(
                (r) =>
                  `<li class="game-run"><span class="game-run-room">${esc(r.room)}</span>` +
                  `<span class="game-faint">${(r.ticks / Math.max(1, game.physics.tick)).toFixed(1)} s</span>` +
                  `<span class="${r.same ? "game-ok" : "game-error"}" title="${r.same ? "Run again in the core, it ended in the same place" : "Run again in the core, it ended somewhere else"}">${r.same ? "same in the core" : "differs in the core"}</span>` +
                  `<button class="cc-quiet" data-game-replay="${r.id}">Play back</button></li>`,
              )
              .join("")}</ul>`
          : `<p class="game-faint">None yet: a run played is kept here, to be played back.</p>`
      }</section>`;
    renderTransport();
  }
  q("[data-game-run]").addEventListener("click", (e) => {
    const replay = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-replay]"));
    if (replay) {
      playError = "";
      send({ type: "gamePlay", recording: Number(replay.dataset.gameReplay), abilities: [] });
    }
  });
  q("[data-game-run]").addEventListener("change", (e) => {
    const boxes = /** @type {HTMLInputElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-boxes]"));
    if (boxes) {
      showBoxes = boxes.checked;
      keep("boxes", showBoxes ? "1" : "");
      return;
    }
    const grant = /** @type {HTMLInputElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-game-grant]"));
    if (grant) {
      if (grant.checked) granted.add(grant.dataset.gameGrant ?? "");
      else granted.delete(grant.dataset.gameGrant ?? "");
      return;
    }
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
    if (tool === "pen" && e.key === "Escape") return (pen.points.length ? ((pen.points = []), (pen.hover = null), paint()) : useTool("select"), true);
    if (tool === "pen" && e.key === "Enter") return (finishPen(false), true);
    if (e.key === "Escape" && !selected && agentPath.length) return ((agentPath = []), paint(), true);
    if (e.key === "Escape") return (select(null), true);
    if (e.key === "v") return (useTool("select"), true);
    if (e.key === "p") return (useTool("pen"), true);
    if (e.key === "f") return (refit(), true);
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

  // For the window's tests: what is drawn, and what a run shows and says.
  /** @type {any} */ (ws).gameProbe = {
    drawn: () => drawn().map((d) => d.key),
    frame: () => player?.frame() ?? null,
    cue: () => player?.cue() ?? null,
  };

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
      renderTransport();
      paint();
    },
    /** The mode came on screen, or went. @param {boolean} on */
    show(on) {
      shown = on;
      if (!on && playing()) player?.stop();
      if (on) paint();
    },
    /** A run, cooked by the core: played in the stage.
     *  @param {{world: string, start: number[], rooms: Room[], keys: [string, number][], replay?: [number, number][] | null}} event */
    play(event) {
      const paused = startPaused;
      startPaused = false;
      if (!shown || !player) return;
      select(null);
      player
        .start(event)
        .then(() => {
          if (paused) player?.step();
          stage.focus({ preventScroll: true });
        })
        .catch((error) => {
          playError = `The game could not run: ${error instanceof Error ? error.message : String(error)}`;
          renderRun();
        });
    },
    /** Where the agent's run went, to draw over the room.
     *  @param {{parts: {room: string, points: number[][], hurt: number[][]}[]}} event */
    path(event) {
      agentPath = event.parts ?? [];
      paint();
    },
    /** The run's frame, for tests. */
    frame: () => player?.frame() ?? null,
    key,
  };
}
