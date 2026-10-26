/* Diffuse: image generation by node graph, a small ComfyUI.
 *
 * Laid out as Game and Canvas are, on Codex's ground: the project's flows,
 * the catalogue's nodes and forge's models on the left; the graph in the
 * middle; the node selected — or, with none, the flow — beside the server
 * and the agent on the right. The screens are the "Diffuse UI" mockups.
 *
 * The core holds each flow and answers every edit with the flow as it now
 * is, so this draws what it is told: an edit is sent as a batch of ops
 * (`DiffuseOp`) and the graph redrawn from the answer. Only a drag is shown
 * here as it happens — a node moving, a wire being pulled — and sent when
 * it is let go. A batch that adds a node names it by the id the core will
 * give it (`freshId`), so one gesture — a wire let go on empty space and a
 * node picked for it — is one undo step.
 *
 * What forge has comes from the core too (`diffuseKnown`): its catalogue,
 * its models and how it is. The window asks the core to look again when
 * the mode comes on screen and every half minute while it stays, and on
 * Try again. */

import { copyText, putDiffuseFile, readDiffuse } from "./bridge.js";
import { openMask } from "./diffuse-mask.js";
import { escapeHtml as esc } from "./html.js";
import { colourOf, fieldsOf, freshId, grouped, HEAD, NODE_W, pngText, portAt, portOf, rowsOf, search, shortValue, socketFor, wire } from "./diffuse-graph.js";

/** @typedef {import('./bindings/DiffuseView').DiffuseView} View */
/** @typedef {import('./bindings/DiffuseKnown').DiffuseKnown} Known */
/** @typedef {import('./bindings/DiffuseFlow').DiffuseFlow} Flow */
/** @typedef {import('./bindings/DiffuseNode').DiffuseNode} Node */
/** @typedef {import('./bindings/DiffuseNodeType').DiffuseNodeType} NodeType */
/** @typedef {import('./bindings/DiffuseInput').DiffuseInput} Input */
/** @typedef {import('./bindings/DiffuseOp').DiffuseOp} Op */
/** @typedef {import('./bindings/DiffuseProblem').DiffuseProblem} Problem */
/** @typedef {import('./bindings/DiffuseRun').DiffuseRun} Run */
/** @typedef {import('./bindings/DiffuseImage').DiffuseImage} Image */
/** @typedef {{x: number, y: number, z: number}} Camera */
/** @typedef {{port: string, side: "in" | "out", type: string, detach: string | null, at: [number, number]}} Pulling */

/** How often forge is looked at again while the mode is on screen. */
const LOOK_EVERY = 30_000;
/** The keys, for `?`. */
const KEYS = /** @type {[string, string][]} */ ([
  ["double-click", "add a node there"], ["drag from a socket", "a wire; let go on space to add a node for it"],
  ["drag a node's head", "move it — ⇧ adds it to those chosen"], ["drag the ground", "move over the graph"],
  ["⌘ wheel  pinch", "zoom"], ["⌫", "remove what is chosen"], ["⌘D", "a copy of what is chosen"], ["⌘A", "choose every node"],
  ["⌘↩", "run the flow on forge — again to queue another"], ["⌘.", "stop its run"],
  ["⌫ in the Gallery", "move the images chosen to the Trash — ⌘-click chooses more"],
  ["drop a PNG", "open the graph that made it, as a new flow"],
  ["drop a picture", "on a Load Image: its picture; elsewhere: a Load Image of it"],
  ["⌘Z ⇧⌘Z", "undo, redo"], ["⌘S", "save the flow"], ["f", "the whole graph"], ["⎋", "close the search, choose nothing"], ["?", "these keys"],
]);

/** A prompt written as JSON — Ideogram's are — rather than as words. @param {string} text */
const looksJson = (text) => /^\s*[[{]/.test(text);

/** A prompt's box as tall as what is in it, from fourteen lines to most of
 *  the window, and set out as code once it is JSON: unwrapped, no spelling
 *  marks. @param {HTMLTextAreaElement} box */
function fitPrompt(box) {
  const json = looksJson(box.value);
  if (box.classList.contains("json") !== json) {
    box.classList.toggle("json", json);
    box.spellcheck = !json;
    box.wrap = json ? "off" : "soft";
  }
  box.style.height = "";
  const most = Math.round((box.ownerDocument.defaultView?.innerHeight ?? 800) * 0.7);
  box.style.height = `${Math.min(most, Math.max(box.scrollHeight + 2, box.clientHeight))}px`;
}

/** A seed nobody chose: a whole number the window holds exactly. */
const newSeed = () => Math.floor(Math.random() * 2 ** 32);

/**
 * @param {{root: Document | HTMLElement, dispatch: (command: any) => void, keys?: (title: string, keys: [string, string][]) => void}} host
 */
export function createDiffuse(host) {
  const ws = /** @type {HTMLElement} */ (host.root.querySelector('.workspace[data-ws="diffuse"]'));
  const q = (/** @type {string} */ selector) => /** @type {HTMLElement} */ (ws.querySelector(selector));
  const send = host.dispatch;
  const stage = q("[data-df-stage]");
  const world = q("[data-df-world]");

  /** @type {View | null} */
  let view = null;
  /** @type {Known | null} */
  let known = null;
  /** The gateway's addresses, in the order they are tried. @type {string[]} */
  let gateways = [];
  /** The nodes chosen, by id. */
  let chosen = new Set(/** @type {string[]} */ ([]));
  /** Where each flow is looked at from, by its path. @type {Map<string, Camera>} */
  const cameras = new Map();
  let side = "flows";
  let panel = "inspector";
  let shown = false;
  /** @type {ReturnType<typeof setInterval> | null} */
  let looking = null;
  /** What the bar above the stage is asking about: closing a flow with
   *  changes, or moving a flow or images to the Trash.
   *  @type {{kind: "close", path: string} | {kind: "deleteFlow", path: string} | {kind: "deleteImages", paths: string[]} | {kind: "note", text: string} | null} */
  let asking = null;
  /** Whether the Flows list is showing a field to move Diffuse's folder. */
  let movingFolder = false;
  /** Bin, as an icon: lines in the theme's colour. */
  const BIN = `<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13M10 11v6M14 11v6"/></svg>`;
  /** The runs this session, newest first. @type {Run[]} */
  let runs = [];
  /** The project's outputs, newest first. @type {Image[]} */
  let images = [];
  /** What the stage shows: the graph, or the outputs. */
  let stageView = "graph";
  /** The folder the gallery shows; none, all of them. */
  let folder = "";
  /** The output chosen in the gallery, whose detail the inspector shows. @type {string | null} */
  let picked = null;
  /** Every output chosen — ⌘ or ⇧ click adds one — to move to the Trash together. */
  let pickedAll = new Set(/** @type {string[]} */ ([]));
  /** Each image read, as a `data:` URL, by path. @type {Map<string, Promise<string>>} */
  const urls = new Map();

  /** A Load Image waiting for a picture from the gallery. @type {{node: string, field: string} | null} */
  let pickingInput = null;
  /** Where a Load Image's mask is kept: beside its flow. @param {string} node */
  const maskPath = (node) => {
    const p = path();
    return p ? `${p.replace(/\.flow\.json$/, "")}.${node}.mask.png` : null;
  };

  const ended = (/** @type {string} */ state) => state === "done" || state === "failed" || state === "cancelled" || state === "lost";
  /** What a run said went wrong at a node. @param {Node} node */
  const failureAt = (node) => {
    const r = lastRun();
    return r && ended(r.state) ? (r.problems ?? []).find((p) => p.node === node.id) ?? null : null;
  };
  /** The newest run of the flow shown. */
  const lastRun = () => runs.find((r) => r.flow === path()) ?? null;
  /** Its run still going, if one is. */
  const activeRun = () => runs.find((r) => r.flow === path() && !ended(r.state)) ?? null;
  /** A wire being pulled from a socket. @type {Pulling | null} */
  let pulling = null;
  /** The search open, where on the graph it adds, and what it must fit.
   *  @type {{at: [number, number], query: string, fits: {type: string, side: "in" | "out"} | null, from: Pulling | null, pick: number} | null} */
  let searching = null;

  const flow = () => view?.shown?.flow ?? null;
  const path = () => view?.shown?.path ?? null;
  const catalogue = () => known?.catalogue ?? null;
  const kindOf = (/** @type {string} */ type) => catalogue()?.nodes.find((n) => n.type === type) ?? null;
  const camera = () => {
    const p = path();
    if (!p) return { x: 40, y: 40, z: 1 };
    let cam = cameras.get(p);
    if (!cam) {
      cam = fitted();
      // Kept only once the stage has a size to fit it to.
      if (stage.clientWidth) cameras.set(p, cam);
    }
    return cam;
  };

  /** Send a batch of edits to the flow shown. @param {Op[]} ops */
  const edit = (ops) => {
    const p = path();
    if (p && ops.length) send({ type: "diffuseEdit", path: p, ops });
  };

  /* ── drawing ─────────────────────────────────────────────────────── */

  function render() {
    renderSide();
    renderBar();
    renderAsk();
    renderBanner();
    renderGraph();
    renderPanel();
  }

  function renderSide() {
    for (const tab of ws.querySelectorAll("[data-df-side]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.dfSide === side;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    const body = q("[data-df-side-body]");
    if (side === "flows") {
      const open = new Set(view?.open.map((t) => t.path) ?? []);
      const flows = view?.flows ?? [];
      const where = view?.folder ?? "~/Diffuse";
      body.innerHTML =
        (movingFolder
          ? `<form class="df-folder-form" data-df-folder-form><label class="df-field">Keep flows and images in<input name="folder" value="${esc(where)}" spellcheck="false" autocomplete="off"></label>` +
            `<div class="df-folder-buttons"><button class="cc-primary" type="submit">Keep it here</button><button class="cc-quiet" type="button" data-df-folder-default>~/Diffuse</button><button class="cc-quiet" type="button" data-df-folder-cancel>Cancel</button></div></form>`
          : `<div class="df-folder"><span class="df-faint">In</span><code title="Diffuse's own folder, whichever project is open">${esc(where)}</code><button class="df-folder-change" data-df-folder-edit>Change</button></div>`) +
        `<div class="df-head">Flows</div>` +
        (flows.length
          ? flows
              .map(
                (f) =>
                  `<div class="df-flow-row"><button class="df-flow${f === path() ? " on" : ""}" data-df-open="${esc(f)}"><span class="df-flow-name">${esc(f)}</span>${open.has(f) ? `<span class="df-flow-open" title="Open">●</span>` : ""}</button>` +
                  `<button class="df-row-trash" data-df-delete-flow="${esc(f)}" aria-label="Move ${esc(f)} to the Trash" title="Move to the Trash">${BIN}</button></div>`,
              )
              .join("")
          : `<p class="df-faint">No flow here yet.</p>`) +
        `<form class="df-new" data-df-new><div class="df-head">New flow</div>` +
        `<label>Name <input name="name" data-df-new-name placeholder="lighthouse" autocomplete="off" spellcheck="false" required></label>` +
        (view?.starters ?? [{ name: "textToImage", title: "Text to image", about: "" }])
          .map((t, i) => `<label class="df-radio" title="${esc(t.about)}"><input type="radio" name="template" value="${esc(t.name)}"${i === 0 ? " checked" : ""}> ${esc(t.title)}<span>${esc(t.about)}</span></label>`)
          .join("") +
        `<label class="df-radio"><input type="radio" name="template" value=""> Empty</label>` +
        `<button class="cc-primary" type="submit">Make it</button></form>`;
    } else if (side === "nodes") {
      const cat = catalogue();
      body.innerHTML = cat
        ? `<p class="df-faint">${cat.nodes.length} nodes, as forge serves them. Drag one onto the graph, or click it to add it in the middle.</p>` +
          grouped(cat)
            .map(
              (g) =>
                `<div class="df-head">${esc(g.group)}</div>` +
                g.nodes
                  .map((n) => {
                    const first = n.outputs[0]?.type ?? n.inputs.find((i) => i.type)?.type ?? "";
                    return `<button class="df-catalogue-node" draggable="true" data-df-add="${esc(n.type)}" title="${esc(n.type)}"><span class="df-dot" style="--c:${colourOf(first)}"></span>${esc(n.title)}</button>`;
                  })
                  .join(""),
            )
            .join("")
        : `<p class="df-faint">forge's nodes are not known yet: they show once it has been reached.</p>`;
    } else {
      const models = known?.models ?? null;
      body.innerHTML = models
        ? Object.entries(models)
            .map(
              ([list, entries]) =>
                `<div class="df-head">${esc(list)}</div>` +
                ((entries ?? []).length
                  ? (entries ?? []).map((m) => `<div class="df-model"><span>${esc(m.name)}</span><span class="df-faint">${m.family ? `${esc(m.family)} · ` : ""}${gb(m.bytes)}</span></div>`).join("")
                  : `<p class="df-faint">none</p>`),
            )
            .join("")
        : `<p class="df-faint">forge's models are not known yet.</p>`;
    }
    const forge = q("[data-df-forge]");
    const state = known?.health.state ?? "looking";
    forge.innerHTML = `<span class="df-dot" style="--c:${stateColour(state)}"></span><span>${esc(chipText())}</span>`;
  }

  function renderBar() {
    const tabs = q("[data-df-tabs]");
    tabs.innerHTML = (view?.open ?? [])
      .map(
        (t) =>
          `<span class="df-tab${t.path === path() ? " on" : ""}" role="tab" aria-selected="${t.path === path()}">` +
          `<button class="df-tab-name" data-df-open="${esc(t.path)}">${esc(t.path)}</button>` +
          (t.dirty ? `<span class="df-dirty" title="Changes not saved"></span>` : "") +
          `<button class="df-tab-close" data-df-close="${esc(t.path)}" aria-label="Close ${esc(t.path)}" title="Close">×</button></span>`,
      )
      .join("");
    const s = view?.shown;
    /** @type {HTMLButtonElement} */ (q("[data-df-undo]")).disabled = !s?.canUndo;
    /** @type {HTMLButtonElement} */ (q("[data-df-redo]")).disabled = !s?.canRedo;
    /** @type {HTMLButtonElement} */ (q("[data-df-save]")).disabled = !s?.dirty;
    const chip = q("[data-df-chip]");
    chip.innerHTML = `<span class="df-dot" style="--c:${stateColour(known?.health.state ?? "looking")}"></span>${esc(chipText())}`;
    const run = /** @type {HTMLButtonElement} */ (q("[data-df-run]"));
    const active = activeRun();
    const state = known?.health.state ?? "looking";
    const why = !s
      ? "Open a flow to run it"
      : s.problems.length
        ? `Not ready: ${s.problems[0].message}`
        : state === "serving" || state === "looking"
          ? ""
          : known?.health.message || "forge is not serving";
    run.disabled = Boolean(why);
    run.title = why || (active ? "Queue another run of it  ⌘↩" : "Run it on forge  ⌘↩");
    q("[data-df-run-label]").textContent = active ? "Queue" : "Run";
    const going = runs.filter((r) => !ended(r.state)).length;
    const count = q("[data-df-count]");
    count.hidden = !going;
    count.textContent = String(going);
    q("[data-df-stop]").hidden = !active;
    for (const b of ws.querySelectorAll("[data-df-view]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.dfView === stageView));
  }

  function renderAsk() {
    const box = q("[data-df-ask]");
    box.hidden = !asking;
    if (!asking) return;
    if (asking.kind === "close") {
      box.innerHTML =
        `<span class="df-banner-text"><b>${esc(asking.path)}</b> has changes not saved.</span>` +
        `<button class="cc-quiet" data-df-answer="stay">Keep it open</button>` +
        `<button class="cc-quiet" data-df-answer="discard">Close without saving</button>` +
        `<button class="cc-primary" data-df-answer="save">Save and close</button>`;
    } else if (asking.kind === "deleteFlow") {
      const dirty = view?.open.find((t) => t.path === /** @type {any} */ (asking).path)?.dirty;
      box.innerHTML =
        `<span class="df-banner-text">Move <b>${esc(asking.path)}</b> to the Trash?${dirty ? " Its changes not saved go with it." : ""} The images it made stay where they are.</span>` +
        `<button class="cc-quiet" data-df-answer="stay">Keep it</button>` +
        `<button class="cc-primary" data-df-answer="trash">Move to Trash</button>`;
    } else if (asking.kind === "note") {
      box.innerHTML = `<span class="df-banner-text">${esc(asking.text)}</span><button class="cc-quiet" data-df-answer="stay">OK</button>`;
    } else {
      const n = asking.paths.length;
      box.innerHTML =
        `<span class="df-banner-text">Move ${n === 1 ? `<b>${esc(asking.paths[0])}</b>` : `<b>${n} images</b>`} to the Trash? You can put ${n === 1 ? "it" : "them"} back from there.</span>` +
        `<button class="cc-quiet" data-df-answer="stay">Keep ${n === 1 ? "it" : "them"}</button>` +
        `<button class="cc-primary" data-df-answer="trash">Move to Trash</button>`;
    }
  }

  function renderBanner() {
    const banner = q("[data-df-banner]");
    const health = known?.health;
    const trouble = health && !["serving", "looking"].includes(health.state);
    banner.hidden = !trouble;
    if (!trouble || !health) return;
    banner.innerHTML =
      `<span class="df-banner-text"><b>${esc(headline(health.state))}</b> ${esc(health.message)}</span>` +
      `<button class="hbtn" data-df-look>Try again</button><button class="hbtn" data-df-panel-go="server">Server</button>`;
  }

  function renderGraph() {
    const f = flow();
    const empty = q("[data-df-empty]");
    const gallery = q("[data-df-gallery]");
    gallery.hidden = stageView !== "gallery";
    if (stageView === "gallery") {
      empty.hidden = true;
      world.hidden = true;
      renderGallery();
      return;
    }
    empty.hidden = Boolean(f);
    world.hidden = !f;
    if (!f) {
      empty.innerHTML = `<p>No flow open.</p><p class="df-faint">Open one from the Flows list, or make one there — Text to image starts you with a graph that runs.</p>`;
      q("[data-df-foot]").textContent = "";
      return;
    }
    const problems = view?.shown?.problems ?? [];
    const troubled = new Set(problems.map((p) => p.node).filter(Boolean));
    q("[data-df-nodes]").innerHTML = f.nodes.map((n) => card(n, troubled.has(n.id))).join("");
    drawWires();
    placeWorld();
    fillImages(world);
    q("[data-df-foot]").innerHTML =
      `<span>${Math.round(camera().z * 100)}%</span><span>${f.nodes.length} nodes · ${f.links.length} wires</span>` +
      (problems.length ? `<span class="df-foot-problem">${problems.length === 1 ? "1 thing" : `${problems.length} things`} to fix before it runs</span>` : `<span class="df-foot-ok">ready to run</span>`) +
      (() => {
        const r = lastRun();
        if (!r || !["failed", "lost"].includes(r.state)) return "";
        return `<span class="df-foot-failed" title="${esc(r.message ?? "")}">run ${r.id} ${r.state === "lost" ? "lost" : "failed"}: ${esc(r.message ?? "")}</span>`;
      })() +
      `<span class="grow"></span><span>double-click: add a node · drag from a socket: wire · ? keys</span>`;
  }

  /** One node's card. @param {Node} node @param {boolean} troubled */
  function card(node, troubled) {
    const kind = kindOf(node.type);
    const working = lastRun()?.nodes.some((n) => n.id === node.id && n.state === "running") && !ended(lastRun()?.state ?? "done");
    const classes = ["df-node", chosen.has(node.id) ? "on" : "", node.bypass ? "bypassed" : "", troubled ? "troubled" : "", kind ? "" : "unknown", working ? "working" : ""].filter(Boolean).join(" ");
    const r = lastRun();
    const going = r && !ended(r.state);
    const at = r?.nodes.find((n) => n.id === node.id);
    const failed = failureAt(node);
    // A run that ended leaves nothing running: where it stopped is failed
    // or stopped, never still at work.
    const state = failed ? "failed" : at ? (at.state === "running" && !going ? "stopped" : at.state) : going && !node.bypass ? "waiting" : "";
    const said = state === "done" && at?.seconds !== null && at?.seconds !== undefined ? `done ${at.seconds.toFixed(1)} s` : state;
    const head =
      `<div class="df-node-head" data-df-grab="${esc(node.id)}"><b>${esc(kind?.title ?? node.type)}</b><span class="df-node-id">${esc(node.id)}</span>` +
      (node.bypass ? `<span class="df-badge">bypassed</span>` : "") +
      (state ? `<span class="df-badge run-${esc(state)}">${esc(said)}</span>` : "") +
      (troubled ? `<span class="df-badge warn" title="Something here stops the flow running">!</span>` : "") +
      `</div>`;
    if (!kind) {
      return `<div class="${classes}" data-df-node="${esc(node.id)}" style="left:${node.at[0]}px;top:${node.at[1]}px;width:${NODE_W}px">${head}<div class="df-node-note">forge has no ${esc(node.type)} node</div></div>`;
    }
    const linked = new Set(flow()?.links.map((l) => l.to) ?? []);
    const rows = rowsOf(kind)
      .map((r) => {
        const inPort = r.input ? `${node.id}:${r.input.name}` : "";
        const outPort = r.output ? `${node.id}:${r.output.name}` : "";
        return (
          `<div class="df-row">` +
          (r.input
            ? `<span class="df-port in${linked.has(inPort) ? " linked" : ""}" data-df-in="${esc(inPort)}" data-type="${esc(r.input.type ?? "")}" style="--c:${colourOf(r.input.type ?? "")}" title="${esc(r.input.type ?? "")}"></span><span class="df-row-in">${esc(r.input.name)}${r.input.optional ? `<i>?</i>` : ""}</span>`
            : `<span class="df-row-in"></span>`) +
          (r.output
            ? `<span class="df-row-out">${esc(r.output.name)}</span><span class="df-port out" data-df-out="${esc(outPort)}" data-type="${esc(r.output.type)}" style="--c:${colourOf(r.output.type)}" title="${esc(r.output.type)}"></span>`
            : "") +
          `</div>`
        );
      })
      .join("");
    const fields = fieldsOf(kind)
      .map((field) => {
        const value = node.values[field.name] ?? field.default;
        if (field.multiline) return `<div class="df-text">${esc(String(value ?? ""))}</div>`;
        return `<div class="df-pill"><span>${esc(field.name)}</span><span>${esc(shortValue(value))}${field.kind === "seed" && node.seed && node.seed !== "fixed" ? ` · ${esc(node.seed)}` : ""}</span></div>`;
      })
      .join("");
    const failure = failureAt(node);
    // Below the fields: above them, it would move the sockets off their wires.
    const why = failure ? `<div class="df-node-failed" role="status">${esc(failure.message)}</div>` : "";
    return `<div class="${classes}${failure ? " failed" : ""}" data-df-node="${esc(node.id)}" style="left:${node.at[0]}px;top:${node.at[1]}px;width:${NODE_W}px">${head}<div class="df-rows">${rows}</div>${fields ? `<div class="df-fields">${fields}</div>` : ""}${why}${atWork(node)}</div>`;
  }

  /** What a run shows on a card: the step at work and its preview, or the
   *  image the node last made. @param {Node} node */
  function atWork(node) {
    const r = lastRun();
    if (!r) return pictureOf(node);
    const p = r.progress;
    if (p && p.node === node.id && !ended(r.state)) {
      const pct = Math.round((p.step / Math.max(1, p.steps)) * 100);
      return (
        `<div class="df-work"><div class="df-bar-track"><div class="df-bar-fill" style="width:${pct}%"></div></div>` +
        `<div class="df-work-said"><span>step ${p.step} / ${p.steps}</span><span>${pct}%</span></div>` +
        (r.preview ? `<img class="df-shot blurred" src="${esc(r.preview)}" alt="A preview at step ${p.step}">` : "") +
        `</div>`
      );
    }
    const made = r.outputs.filter((o) => o.node === node.id);
    if (!made.length) return pictureOf(node);
    const last = made[made.length - 1];
    if (!last) return "";
    return `<div class="df-work"><img class="df-shot" data-df-img="${esc(last.path)}" alt="${esc(last.path)}" draggable="false"><div class="df-work-said"><span>${esc(last.path.split("/").pop() ?? "")}</span><span>${last.width} × ${last.height}</span></div></div>`;
  }

  /** The picture a Load Image has, small on its card. @param {Node} node */
  function pictureOf(node) {
    const kind = kindOf(node.type);
    const field = kind?.inputs.find((i) => i.kind === "upload");
    const value = field ? node.values[field.name] : null;
    if (typeof value !== "string" || !value) return "";
    return `<div class="df-work"><img class="df-shot" data-df-img="${esc(value)}" alt="${esc(value)}" draggable="false"><div class="df-work-said"><span>${esc(value.split("/").pop() ?? "")}</span></div></div>`;
  }

  /** How much of a picture a kept mask covers, or `null` with none.
   *  @param {string} p @returns {Promise<number | null>} */
  async function maskShare(p) {
    try {
      const url = await urlOf(p);
      const img = new Image();
      await new Promise((done, failed) => {
        img.onload = done;
        img.onerror = failed;
        img.src = url;
      });
      const c = document.createElement("canvas");
      c.width = Math.min(256, img.naturalWidth);
      c.height = Math.max(1, Math.round((c.width / img.naturalWidth) * img.naturalHeight));
      const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
      g.drawImage(img, 0, 0, c.width, c.height);
      const data = g.getImageData(0, 0, c.width, c.height).data;
      let sum = 0;
      for (let i = 0; i < data.length; i += 4) sum += data[i];
      return sum / ((data.length / 4) * 255);
    } catch {
      return null;
    }
  }

  /** A picture file as a PNG — the folder keeps PNGs only — and kept under
   *  `inputs/`: where it was written. @param {File} file */
  async function keepPicture(file) {
    let bytes = new Uint8Array(await file.arrayBuffer());
    if (!(bytes[0] === 0x89 && bytes[1] === 0x50)) {
      const bitmap = await createImageBitmap(file);
      const c = document.createElement("canvas");
      c.width = bitmap.width;
      c.height = bitmap.height;
      /** @type {CanvasRenderingContext2D} */ (c.getContext("2d")).drawImage(bitmap, 0, 0);
      const blob = await new Promise((done) => c.toBlob(done, "image/png"));
      bytes = new Uint8Array(await /** @type {Blob} */ (blob).arrayBuffer());
    }
    const stem = file.name.replace(/\.[A-Za-z0-9]+$/, "").replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "picture";
    const rel = await putDiffuseFile(`inputs/${stem}.png`, bytes);
    urls.delete(rel);
    return rel;
  }

  /** Give a Load Image a picture — the one chosen, or the first in the flow —
   *  or, the flow having none, add one with it at `at`.
   *  @param {string} rel @param {{node: string, field: string} | null} [target] @param {[number, number]} [at] */
  function usePicture(rel, target = null, at) {
    const f = flow();
    if (!f) return;
    const loads = f.nodes.filter((n) => kindOf(n.type)?.inputs.some((i) => i.kind === "upload"));
    const node = target ? f.nodes.find((n) => n.id === target.node) : (loads.find((n) => chosen.has(n.id)) ?? loads[0]);
    if (node) {
      const field = target?.field ?? kindOf(node.type)?.inputs.find((i) => i.kind === "upload")?.name ?? "image";
      edit([{ op: "set", id: node.id, name: field, value: rel }]);
      chosen = new Set([node.id]);
      return;
    }
    const kind = kindOf("LoadImage");
    if (!kind) return;
    const place = at ?? [16, Math.max(16, ...f.nodes.map((n) => n.at[1] + 300))];
    edit([{ op: "add", type: "LoadImage", at: [Math.round(place[0]), Math.round(place[1])], values: { image: rel } }]);
  }

  /** The mask sheet, for a Load Image's picture. @param {string} field */
  async function paintMask(field) {
    const one = [...chosen][0];
    const node = flow()?.nodes.find((n) => n.id === one);
    const value = node?.values[field];
    const mask = node ? maskPath(node.id) : null;
    if (!node || typeof value !== "string" || !mask) return;
    const image = await urlOf(value).catch(() => null);
    if (!image) return;
    const kept = await urlOf(mask).catch(() => null);
    await openMask({
      host: stage,
      title: value.split("/").pop() ?? value,
      image,
      mask: kept,
      done: async (png) => {
        const rel = await putDiffuseFile(mask, png);
        urls.delete(rel);
        renderInspector();
      },
      cancel: () => {},
    });
  }

  /** An image of the project, as a `data:` URL — the window's security
   *  policy lets in no other kind. @param {string} p */
  function urlOf(p) {
    let url = urls.get(p);
    if (!url) {
      url = readDiffuse(p).then((bytes) => {
        const view = new Uint8Array(bytes);
        let text = "";
        for (let i = 0; i < view.length; i += 0x8000) text += String.fromCharCode(...view.subarray(i, i + 0x8000));
        return `data:image/png;base64,${btoa(text)}`;
      });
      url.catch(() => urls.delete(p));
      urls.set(p, url);
    }
    return url;
  }

  /** Each image waiting for its picture, given it. @param {Element} box */
  function fillImages(box) {
    for (const img of box.querySelectorAll("img[data-df-img]")) {
      const p = /** @type {HTMLElement} */ (img).dataset.dfImg ?? "";
      urlOf(p).then(
        (url) => {
          /** @type {HTMLImageElement} */ (img).src = url;
        },
        () => img.classList.add("missing"),
      );
    }
  }

  /** The outputs, as a grid. */
  function renderGallery() {
    const box = q("[data-df-gallery]");
    const folders = [...new Set(images.map((i) => i.folder))].sort();
    const shownImages = images.filter((i) => !folder || i.folder === folder);
    box.innerHTML =
      `<div class="df-gallery-head"><label class="df-field-inline">Show <select data-df-folder><option value="">every flow's</option>${folders
        .map((f) => `<option value="${esc(f)}"${f === folder ? " selected" : ""}>${esc(f)}</option>`)
        .join("")}</select></label><span class="df-faint">${shownImages.length} ${shownImages.length === 1 ? "image" : "images"}, newest first</span></div>` +
      (shownImages.length
        ? `<div class="df-gallery-grid">${shownImages
            .map(
              (i) =>
                `<button class="df-tile${pickedAll.has(i.path) ? " on" : ""}" data-df-pick-image="${esc(i.path)}" aria-pressed="${pickedAll.has(i.path)}">` +
                `<img data-df-img="${esc(i.path)}" alt="${esc(i.path)}" draggable="false"><span class="df-tile-said"><span>${esc(i.path.split("/").pop() ?? "")}</span><span class="df-faint">${i.made.seed ?? ""}</span></span></button>`,
            )
            .join("")}</div>`
        : `<div class="df-empty-note"><p>No outputs yet.</p><p class="df-faint">A flow's Save Image writes its pictures into <code>outputs/&lt;flow&gt;/</code>; they show here.</p></div>`);
    fillImages(box);
  }

  /** The wires, from the flow and where any dragged node is now.
   *  @param {Map<string, [number, number]>} [moved] */
  function drawWires(moved = new Map()) {
    const f = flow();
    if (!f) return;
    const at = (/** @type {Node} */ n) => ({ ...n, at: moved.get(n.id) ?? n.at });
    const nodes = new Map(f.nodes.map((n) => [n.id, at(n)]));
    const paths = f.links
      .map((l) => {
        const from = portOf(l.from);
        const to = portOf(l.to);
        const a = from && nodes.get(from.node);
        const b = to && nodes.get(to.node);
        const ka = a && kindOf(a.type);
        const kb = b && kindOf(b.type);
        if (!from || !to || !a || !b || !ka || !kb) return "";
        const pa = portAt(a, ka, "out", from.name);
        const pb = portAt(b, kb, "in", to.name);
        if (!pa || !pb) return "";
        const type = ka.outputs.find((o) => o.name === from.name)?.type ?? "";
        const faint = a.bypass || b.bypass ? " faint" : "";
        return `<path class="df-wire${faint}" d="${wire(pa, pb)}" stroke="${colourOf(type)}"></path>`;
      })
      .join("");
    const temp = pulling ? `<path class="df-wire pulling" data-df-temp d="" stroke="${colourOf(pulling.type)}"></path>` : "";
    q("[data-df-wires]").innerHTML = paths + temp;
  }

  function placeWorld() {
    const cam = camera();
    world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.z})`;
  }

  /** A camera that shows the whole graph, no larger than life. */
  function fitted() {
    const f = flow();
    const box = stage.getBoundingClientRect();
    if (!f || !f.nodes.length || !box.width) return { x: 40, y: 40, z: 1 };
    const xs = f.nodes.map((n) => n.at[0]);
    const ys = f.nodes.map((n) => n.at[1]);
    const left = Math.min(...xs);
    const top = Math.min(...ys);
    const width = Math.max(...xs) + NODE_W - left;
    const height = Math.max(...ys) + 260 - top;
    const z = Math.max(0.35, Math.min(1, (box.width - 80) / width, (box.height - 80) / height));
    return { x: (box.width - width * z) / 2 - left * z, y: Math.max(24, (box.height - height * z) / 2) - top * z, z };
  }

  /* ── the panel ───────────────────────────────────────────────────── */

  function renderPanel() {
    for (const tab of ws.querySelectorAll("[data-df-panel]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.dfPanel === panel;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    for (const body of ws.querySelectorAll("[data-df-body]")) /** @type {HTMLElement} */ (body).hidden = /** @type {HTMLElement} */ (body).dataset.dfBody !== panel;
    const pane = /** @type {HTMLElement} */ (ws.querySelector("#diffuse-pane"));
    if (pane) pane.dataset.showing = panel;
    if (panel === "inspector") {
      if (stageView === "gallery") renderPicked();
      else renderInspector();
    }
    if (panel === "server") renderServer();
    if (panel === "queue") renderQueue();
  }

  function renderInspector() {
    const box = q("[data-df-inspector]");
    // A field being typed in keeps what was typed, and its caret: a look at
    // forge — on coming here, then every half minute — draws the inspector
    // again, from a flow that has the field's value only once it is set.
    const active = /** @type {HTMLInputElement | HTMLTextAreaElement | null} */ (ws.ownerDocument.activeElement);
    const typing = active && box.contains(active) && active.matches("input, textarea") ? active : null;
    const where = typing && (typing.id ? `#${CSS.escape(typing.id)}` : typing.dataset.dfFlow ? `[data-df-flow="${typing.dataset.dfFlow}"]` : null);
    const kept = typing && where ? { where, value: typing.value, from: typing.selectionStart, to: typing.selectionEnd, scroll: box.scrollTop } : null;
    drawInspector(box);
    for (const prompt of box.querySelectorAll("textarea.df-prompt")) fitPrompt(/** @type {HTMLTextAreaElement} */ (prompt));
    if (!kept) return;
    const again = /** @type {HTMLInputElement | HTMLTextAreaElement | null} */ (box.querySelector(kept.where));
    if (!again) return;
    again.focus({ preventScroll: true });
    again.value = kept.value;
    // Put back, not typed: no change is said of it on leaving, so it is
    // set then by hand — unless more is typed, and the change says so.
    if (again.value !== again.defaultValue) again.dataset.dfPending = "";
    try {
      again.setSelectionRange(kept.from, kept.to);
    } catch {
      // A number field has no caret to put back.
    }
    box.scrollTop = kept.scroll;
  }

  /** The inspector: the flow, the node chosen, or several. @param {HTMLElement} box */
  function drawInspector(box) {
    const f = flow();
    if (!f) {
      box.innerHTML = `<p class="df-faint df-pad">Open a flow to see its nodes here.</p>`;
      return;
    }
    const problems = view?.shown?.problems ?? [];
    const picked = f.nodes.filter((n) => chosen.has(n.id));
    if (picked.length > 1) {
      box.innerHTML =
        `<div class="df-pad"><div class="df-title">${picked.length} nodes</div></div>` +
        `<div class="df-actions"><button class="hbtn" data-df-act="bypass">Bypass</button><button class="hbtn" data-df-act="duplicate">Duplicate</button><button class="hbtn" data-df-act="remove">Remove</button></div>`;
      return;
    }
    const node = picked[0];
    if (!node) {
      box.innerHTML =
        `<div class="df-pad"><div class="df-title">${esc(path() ?? "")}</div><div class="df-sub">no node chosen · the flow's own settings</div></div>` +
        `<div class="df-fieldset">` +
        `<label class="df-field">Outputs go to<input data-df-flow="outputs" value="${esc(f.outputs ?? "")}" placeholder="outputs/${esc((path() ?? "").replace(/\.flow\.json$/, "").split("/").pop() ?? "")}/" spellcheck="false"><span class="df-faint">a folder in this project</span></label>` +
        `<label class="df-field">Note<textarea data-df-flow="note" rows="3">${esc(f.note ?? "")}</textarea></label></div>` +
        problemList(problems);
      return;
    }
    const kind = kindOf(node.type);
    const mine = problems.filter((p) => p.node === node.id);
    if (!kind) {
      box.innerHTML = `<div class="df-pad"><div class="df-title">${esc(node.type)}</div><div class="df-sub">node ${esc(node.id)}</div><p class="df-faint">forge does not serve this kind of node, so it cannot be edited or run until it does.</p></div>${actions(node)}`;
      return;
    }
    const outs = kind.outputs.map((o) => o.name).join(", ");
    box.innerHTML =
      `<div class="df-pad"><div class="df-title">${esc(kind.title)}</div><div class="df-sub">node ${esc(node.id)} · ${esc(kind.group)}${outs ? ` · makes ${esc(outs)}` : ""}</div></div>` +
      (mine.length ? `<ul class="df-problems">${mine.map((p) => `<li>${esc(p.message)}</li>`).join("")}</ul>` : "") +
      (failureAt(node) ? `<p class="df-node-failed df-pad-x">Run ${lastRun()?.id} failed here: ${esc(failureAt(node)?.message ?? "")}</p>` : "") +
      sizePresets(node, kind) +
      `<div class="df-fieldset">${fieldsOf(kind).map((field) => fieldHtml(node, field)).join("")}</div>` +
      inputsHtml(node, kind) +
      actions(node);
    fillImages(box);
    for (const said of box.querySelectorAll("[data-df-mask-said]")) {
      const p = /** @type {HTMLElement} */ (said).dataset.dfMaskSaid ?? "";
      maskShare(p).then((share) => {
        if (share !== null) said.textContent = `${Math.round(share * 100)}% masked: only that is redrawn`;
      });
    }
  }

  /** A field's control. @param {Node} node @param {Input} field */
  function fieldHtml(node, field) {
    const id = `df-f-${node.id}-${field.name}`;
    const value = node.values[field.name];
    const shownValue = value ?? field.default ?? "";
    const label = `<label class="df-field-name" for="${id}">${esc(field.name)}</label>`;
    const range = [field.min !== null && field.min !== undefined ? ` min="${field.min}"` : "", field.max !== null && field.max !== undefined ? ` max="${field.max}"` : "", field.step !== null && field.step !== undefined ? ` step="${field.step}"` : ""].join("");
    switch (field.kind) {
      case "int":
      case "float":
        return `<div class="df-field">${label}<input id="${id}" type="number" data-df-set="${esc(field.name)}" data-kind="${field.kind}" value="${esc(String(shownValue))}"${range}></div>`;
      case "text":
        return field.multiline
          ? `<div class="df-field">${label}<textarea id="${id}" class="df-prompt${looksJson(String(shownValue)) ? " json" : ""}" data-df-set="${esc(field.name)}" data-kind="text" rows="14"${looksJson(String(shownValue)) ? ' spellcheck="false" wrap="off"' : ""}>${esc(String(shownValue))}</textarea></div>`
          : `<div class="df-field">${label}<input id="${id}" data-df-set="${esc(field.name)}" data-kind="text" value="${esc(String(shownValue))}" spellcheck="false"></div>`;
      case "choice": {
        const list = choices(field);
        if (!list) {
          return `<div class="df-field">${label}<input id="${id}" data-df-set="${esc(field.name)}" data-kind="text" value="${esc(String(shownValue))}" spellcheck="false"><span class="df-faint">forge's ${esc(field.from ?? "list")} are not known yet</span></div>`;
        }
        const current = String(shownValue);
        const missing = current && !list.includes(current) ? `<option value="${esc(current)}" selected>${esc(current)} — not on forge</option>` : "";
        const blank = current ? "" : `<option value="" selected disabled>choose…</option>`;
        return `<div class="df-field">${label}<select id="${id}" data-df-set="${esc(field.name)}" data-kind="choice">${blank}${missing}${list.map((c) => `<option value="${esc(c)}"${c === current ? " selected" : ""}>${esc(c)}</option>`).join("")}</select></div>`;
      }
      case "seed": {
        const mode = node.seed ?? "fixed";
        const seg = (/** @type {string} */ m, /** @type {string} */ text) => `<button aria-pressed="${mode === m}" data-df-seed="${m}">${text}</button>`;
        return (
          `<div class="df-field">${label}<div class="df-seed"><input id="${id}" type="number" min="0" step="1" data-df-set="${esc(field.name)}" data-kind="seed" value="${esc(String(shownValue))}">` +
          `<button class="hbtn" data-df-reseed="${esc(field.name)}" title="A new random seed" aria-label="A new random seed">⟳</button></div>` +
          `<div class="df-seg" role="group" aria-label="After each run">${seg("fixed", "fixed")}${seg("random", "random")}${seg("increment", "+1")}</div></div>`
        );
      }
      case "upload": {
        const mask = maskPath(node.id);
        return (
          `<div class="df-field"><span class="df-field-name">${esc(field.name)}</span>` +
          (value
            ? `<div class="df-upload"><img class="df-upload-shot" data-df-img="${esc(String(value))}" alt="${esc(String(value))}" draggable="false"><span class="df-upload-name">${esc(String(value))}</span></div>`
            : `<p class="df-faint">No picture yet: choose one, take one from the gallery, or drop one on the graph.</p>`) +
          `<div class="df-upload-buttons"><label class="hbtn df-file">Choose a file<input type="file" accept="image/png,image/jpeg,image/webp,image/gif" data-df-upload-file="${esc(field.name)}"></label>` +
          `<button class="hbtn" data-df-pick-input="${esc(field.name)}">From the gallery</button>` +
          (value && mask ? `<button class="hbtn" data-df-mask="${esc(field.name)}">Paint the mask</button>` : "") +
          `</div>` +
          (value && mask ? `<span class="df-faint" data-df-mask-said="${esc(mask)}">no mask: all of it is redrawn</span>` : "") +
          `</div>`
        );
      }
      default:
        return `<div class="df-field">${label}<code class="df-faint">${esc(JSON.stringify(shownValue))}</code></div>`;
    }
  }

  /** SDXL's sizes near a megapixel, which it was trained on — for a node
   *  with a width and a height on a step of 8, such as Empty Latent.
   *  @param {Node} node @param {NodeType} kind */
  function sizePresets(node, kind) {
    const w = kind.inputs.find((i) => i.name === "width" && i.kind === "int");
    const h = kind.inputs.find((i) => i.name === "height" && i.kind === "int");
    if (!w || !h) return "";
    const now = `${node.values.width ?? w.default}x${node.values.height ?? h.default}`;
    const sizes = [[1024, 1024, "square"], [1152, 896, ""], [896, 1152, ""], [1216, 832, "3:2"], [832, 1216, "2:3"], [1344, 768, "16:9"], [768, 1344, "9:16"], [1536, 640, "wide"], [640, 1536, "tall"]];
    const options = sizes.map(([a, b, said]) => `<option value="${a}x${b}"${`${a}x${b}` === now ? " selected" : ""}>${a} × ${b}${said ? ` · ${said}` : ""}</option>`).join("");
    const other = sizes.some(([a, b]) => `${a}x${b}` === now) ? "" : `<option value="" selected>${esc(now.replace("x", " × "))} · your own</option>`;
    return `<div class="df-fieldset"><label class="df-field">Size, as SDXL likes it<select data-df-size>${other}${options}</select><span class="df-faint">about a megapixel: SDXL models such as Illustrious draw best near it</span></label></div>`;
  }

  /** A number put on its field's step, counted from its least, and in its
   *  range: a latent's 1964 is 1968. @param {HTMLInputElement} el @param {number} v */
  function onStep(el, v) {
    const step = Number(el.step);
    const min = el.min === "" ? 0 : Number(el.min);
    const max = el.max === "" ? Infinity : Number(el.max);
    if (!Number.isFinite(step) || step <= 1) return v;
    return Math.min(max, Math.max(min, min + Math.round((v - min) / step) * step));
  }

  /** What a choice is chosen from, or `null` when not known yet. @param {Input} field */
  function choices(field) {
    const from = field.from ?? "";
    const cat = catalogue();
    const list = cat?.lists[from];
    if (list) return list;
    const models = known?.models?.[from];
    return models ? models.map((m) => m.name) : null;
  }

  /** What feeds each of a node's inputs. @param {Node} node @param {NodeType} kind */
  function inputsHtml(node, kind) {
    const sockets = kind.inputs.filter((i) => i.type);
    if (!sockets.length) return "";
    const f = flow();
    const rows = sockets
      .map((i) => {
        const link = f?.links.find((l) => l.to === `${node.id}:${i.name}`);
        const from = link ? portOf(link.from) : null;
        const source = from ? f?.nodes.find((n) => n.id === from.node) : null;
        const said = source
          ? `<button class="df-link-to" data-df-choose="${esc(source.id)}">${esc(kindOf(source.type)?.title ?? source.type)} ${esc(source.id)}</button>`
          : `<span class="${i.optional ? "df-faint" : "df-missing"}">${i.optional ? "optional" : "no link"}</span>`;
        return `<div class="df-input"><span class="df-dot" style="--c:${colourOf(i.type ?? "")}"></span><span class="df-input-name">${esc(i.name)}</span>${said}</div>`;
      })
      .join("");
    return `<div class="df-head df-pad-x">Inputs</div><div class="df-inputs">${rows}</div>`;
  }

  /** @param {Node} node */
  const actions = (node) =>
    `<div class="df-actions"><button class="hbtn" data-df-act="bypass" aria-pressed="${node.bypass}">${node.bypass ? "Bypassed" : "Bypass"}</button>` +
    `<button class="hbtn" data-df-act="duplicate">Duplicate</button><button class="hbtn" data-df-act="remove">Remove</button>` +
    `<button class="hbtn" disabled title="Running comes with runs">Run to here</button></div>`;

  /** @param {Problem[]} problems */
  const problemList = (problems) =>
    problems.length
      ? `<div class="df-head df-pad-x">Before it runs</div><ul class="df-problems">${problems
          .map((p) => `<li>${p.node ? `<button class="df-link-to" data-df-choose="${esc(p.node)}">${esc(kindOf(flow()?.nodes.find((n) => n.id === p.node)?.type ?? "")?.title ?? "node")} ${esc(p.node)}</button> ` : ""}${esc(p.message)}</li>`)
          .join("")}</ul>`
      : `<p class="df-ok df-pad">Ready to run.</p>`;

  /** The output chosen in the gallery, large, with what made it. */
  function renderPicked() {
    const box = q("[data-df-inspector]");
    if (pickedAll.size > 1) {
      box.innerHTML =
        `<div class="df-pad"><div class="df-title">${pickedAll.size} images</div><div class="df-sub">⌘-click adds an image or takes it away</div></div>` +
        `<div class="df-actions"><button class="hbtn df-danger" data-df-delete-images>${BIN} Move ${pickedAll.size} to the Trash</button></div>`;
      return;
    }
    const image = images.find((i) => i.path === picked);
    if (!image) {
      box.innerHTML = `<p class="df-faint df-pad">Choose an image to see what made it.</p>`;
      return;
    }
    const m = image.made;
    const row = (/** @type {string} */ label, /** @type {unknown} */ value) =>
      value === null || value === undefined || value === "" ? "" : `<span class="df-faint">${esc(label)}</span><span>${esc(String(value))}</span>`;
    const stem = image.folder.split("/").pop() ?? "";
    const flowHere = (view?.flows ?? []).find((f) => f.replace(/\.flow\.json$/, "").split("/").pop() === stem);
    box.innerHTML =
      `<div class="df-pad"><img class="df-big" data-df-img="${esc(image.path)}" alt="${esc(image.path)}" draggable="false"></div>` +
      `<div class="df-pad-x"><div class="df-title df-mono">${esc(image.path.split("/").pop() ?? "")}</div>` +
      `<div class="df-sub">${image.width} × ${image.height} · ${esc(new Date(image.madeAt * 1000).toLocaleString())}</div></div>` +
      `<div class="df-made">${row("Seed", m.seed)}${row("Steps", m.steps)}${row("CFG", m.cfg)}${row("Sampler", [m.sampler, m.scheduler].filter(Boolean).join(" · "))}${row("Denoise", m.denoise)}${row("Model", m.checkpoint?.replace(/\.safetensors$/, ""))}${row("Prompt", m.prompt)}${row("Negative", m.negative)}</div>` +
      (image.graph ? "" : `<p class="df-faint df-pad">This PNG holds no graph: it was not made by a flow, or not by forge.</p>`) +
      `<div class="df-actions">` +
      (image.graph ? `<button class="cc-primary" data-df-open-image="${esc(image.path)}" title="The graph forge wrote into it, as a new flow">Open its graph</button>` : "") +
      (flowHere ? `<button class="hbtn" data-df-open-flow="${esc(flowHere)}">Open ${esc(flowHere)}</button>` : "") +
      `<button class="hbtn" data-df-use-input="${esc(image.path)}">${pickingInput ? "Use as Load Image's picture" : "Use as input"}</button>` +
      `<button class="hbtn" data-df-send="canvas">Send to Canvas</button><button class="hbtn" data-df-send="game">Send to Game</button>` +
      `<button class="hbtn" data-df-copy-path="${esc(image.path)}">Copy its path</button>` +
      `<button class="hbtn df-danger" data-df-delete-images>${BIN} Move to Trash</button></div>`;
    fillImages(box);
  }

  /** The runs: the one at work, those waiting, and those done this session. */
  function renderQueue() {
    const box = q("[data-df-queue]");
    if (!runs.length) {
      box.innerHTML = `<p class="df-faint df-pad">No runs yet. Run a flow with ▶ Run or ⌘↩: forge works through them one at a time, and they show here.</p>`;
      return;
    }
    const going = runs.filter((r) => !ended(r.state));
    const done = runs.filter((r) => ended(r.state));
    const titleOf = (/** @type {Run} */ r, /** @type {string} */ id) => {
      const node = r.flow === path() ? flow()?.nodes.find((n) => n.id === id) : null;
      return node ? `${kindOf(node.type)?.title ?? node.type} ${id}` : `node ${id}`;
    };
    const cardOf = (/** @type {Run} */ r) => {
      const p = r.progress;
      const pct = p ? Math.round((p.step / Math.max(1, p.steps)) * 100) : 0;
      const state =
        r.state === "sending" ? "sending" : r.state === "queued" ? `queued${r.position ? ` · ${r.position} ahead` : ""}` : r.state === "running" ? "running" : r.state === "done" ? `done in ${r.seconds?.toFixed(1) ?? "?"} s` : r.state;
      return (
        `<div class="df-run-card ${esc(r.state)}"><div class="df-run-head"><b>${esc(r.flow)}</b><span class="df-faint">run ${r.id}</span></div>` +
        `<div class="df-run-state">${esc(state)}</div>` +
        (p && !ended(r.state) ? `<div class="df-bar-track"><div class="df-bar-fill" style="width:${pct}%"></div></div><div class="df-work-said"><span>${esc(titleOf(r, p.node))} · step ${p.step} / ${p.steps}</span><span>${pct}%</span></div>` : "") +
        (r.nodes.length && !ended(r.state)
          ? `<div class="df-run-nodes">${r.nodes
              .map((n) => `<div><span>${esc(titleOf(r, n.id))}</span><span class="run-${esc(n.state)}">${n.state === "done" && n.seconds !== null ? `${n.seconds.toFixed(1)} s` : esc(n.state)}</span></div>`)
              .join("")}</div>`
          : "") +
        (r.message ? `<div class="df-run-why">${esc(r.message)}</div>` : "") +
        ((r.problems ?? []).filter((p) => p.node && p.message !== r.message).length
          ? `<ul class="df-run-problems">${(r.problems ?? []).filter((p) => p.node).map((p) => `<li><b>${esc(titleOf(r, p.node ?? ""))}</b> ${esc(p.message)}</li>`).join("")}</ul>`
          : "") +
        (r.outputs.filter((o) => o.kept).length
          ? `<div class="df-run-outs">${r.outputs
              .filter((o) => o.kept)
              .map((o) => `<button class="df-run-out" data-df-show-image="${esc(o.path)}" title="${esc(o.path)}"><img data-df-img="${esc(o.path)}" alt="${esc(o.path)}" draggable="false"></button>`)
              .join("")}</div>`
          : "") +
        (!ended(r.state) ? `<div class="df-actions"><button class="hbtn" data-df-cancel="${r.id}">${r.state === "running" ? "Stop it" : "Take it out"}</button></div>` : "") +
        `</div>`
      );
    };
    box.innerHTML =
      (going.length ? `<div class="df-head df-pad-x">Now</div>${going.map(cardOf).join("")}` : "") +
      (done.length ? `<div class="df-head df-pad-x">This session</div>${done.map(cardOf).join("")}` : "");
    fillImages(box);
  }

  function renderServer() {
    const box = q("[data-df-server]");
    const h = known?.health;
    const state = h?.state ?? "looking";
    const reached = Boolean(h?.url);
    const check = (/** @type {"ok" | "bad" | "none"} */ how, /** @type {string} */ what, /** @type {string} */ note) =>
      `<div class="df-check ${how}"><span class="df-check-mark">${how === "ok" ? "✓" : how === "bad" ? "✕" : "–"}</span><span>${esc(what)}</span><span class="df-faint">${esc(note)}</span></div>`;
    const gpu = h?.status?.gpu;
    box.innerHTML =
      `<div class="df-head df-pad-x">What was checked</div>` +
      check(reached ? "ok" : state === "looking" || state === "noKey" ? "none" : "bad", "atlas's gateway answers", reached ? `${h?.ms ?? "?"} ms` : state === "looking" ? "looking" : state === "noKey" ? "not asked" : "no answer") +
      check(state === "noKey" ? "bad" : state === "badKey" ? "bad" : reached ? "ok" : "none", "The key", state === "noKey" ? "no NEXUS_KEY" : state === "badKey" ? "refused" : reached ? "accepted" : "not tried") +
      check(state === "serving" ? "ok" : state === "offline" ? "bad" : "none", "forge's image server", state === "serving" ? (gpu ? `${gpu.name} · ${gb(gpu.used)} / ${gb(gpu.total)}` : "serving") : state === "offline" ? "off" : "–") +
      check(known?.catalogue ? "ok" : "none", "Its nodes and models", known?.catalogue ? (known.fresh ? "just now" : "as kept from before") : "none kept") +
      (h?.message && state !== "serving" ? `<p class="df-pad df-say">${esc(h.message)}</p>` : "") +
      `<div class="df-head df-pad-x">Where Proton looks</div>` +
      `<form class="df-fieldset" data-df-gateways>` +
      `<label class="df-field">At home<input name="home" value="${esc(gateways[0] ?? "")}" placeholder="http://192.168.1.10:7420/img/v1" spellcheck="false" autocomplete="off"></label>` +
      `<label class="df-field">Away (Tailscale)<input name="away" value="${esc(gateways[1] ?? "")}" placeholder="http://atlas.your-tailnet.ts.net:7420/img/v1" spellcheck="false" autocomplete="off"><span class="df-faint">tried when the first doesn't answer</span></label>` +
      `<button class="hbtn" type="submit">Keep these</button></form>` +
      `<p class="df-pad df-faint">Kept in your own settings, <code>~/.config/proton/settings.json</code>. The key is <code>NEXUS_KEY</code> from your shell — Nexus › Inference › One endpoint — and is never stored.</p>` +
      `<div class="df-actions"><button class="hbtn" data-df-look>Look again</button></div>`;
  }

  /* ── forge's state, in words ─────────────────────────────────────── */

  function chipText() {
    const h = known?.health;
    switch (h?.state ?? "looking") {
      case "serving": {
        const gpu = h?.status?.gpu;
        return gpu ? `forge · ${gpu.name.replace(/^NVIDIA (GeForce )?/, "")} · ${gb(gpu.used, false)} / ${gb(gpu.total)}` : "forge · serving";
      }
      case "offline":
        return "forge · not serving";
      case "noKey":
        return "no NEXUS_KEY";
      case "badKey":
        return "the key is refused";
      case "unreachable":
        return "atlas · not answering";
      case "failing":
        return "forge · answering oddly";
      default:
        return "looking for forge…";
    }
  }

  /** @param {string} state */
  const headline = (state) =>
    ({ offline: "forge isn't serving images.", noKey: "There is no key.", badKey: "The key is refused.", unreachable: "atlas doesn't answer.", failing: "forge answered oddly." })[state] ?? "";

  /** @param {string} state */
  const stateColour = (state) => (state === "serving" ? "var(--ok)" : state === "looking" ? "var(--muted)" : state === "offline" ? "var(--warn)" : "var(--err)");

  /** Bytes as gigabytes. @param {number} bytes @param {boolean} [unit] */
  const gb = (bytes, unit = true) => {
    // As a card's memory is sold: in 2³⁰ bytes, so a 4090 is 24.
    const g = bytes / 2 ** 30;
    return `${g.toFixed(g >= 10 ? 0 : 1)}${unit ? " GB" : ""}`;
  };

  /* ── the search ──────────────────────────────────────────────────── */

  /** Open the search at a point on the graph. @param {[number, number]} at @param {Pulling | null} [from] */
  function openSearch(at, from = null) {
    if (!catalogue()) return;
    searching = { at, query: "", fits: from ? { type: from.type, side: from.side === "out" ? "in" : "out" } : null, from, pick: 0 };
    renderSearch();
    /** @type {HTMLInputElement | null} */ (q("[data-df-search] input"))?.focus();
  }

  function closeSearch() {
    searching = null;
    q("[data-df-search]").hidden = true;
  }

  function renderSearch() {
    const box = q("[data-df-search]");
    const cat = catalogue();
    if (!searching || !cat) {
      box.hidden = true;
      return;
    }
    const groups = search(cat, searching.query, searching.fits);
    const all = groups.flatMap((g) => g.nodes);
    searching.pick = Math.min(searching.pick, Math.max(0, all.length - 1));
    const cam = camera();
    const [sx, sy] = [searching.at[0] * cam.z + cam.x, searching.at[1] * cam.z + cam.y];
    const rect = stage.getBoundingClientRect();
    box.style.left = `${Math.max(8, Math.min(sx, rect.width - 330))}px`;
    box.style.top = `${Math.max(8, Math.min(sy, rect.height - 380))}px`;
    let index = 0;
    const list = groups
      .map(
        (g) =>
          `<div class="df-search-group">${esc(g.group)}</div>` +
          g.nodes
            .map((n) => {
              const i = index++;
              const ins = n.inputs.filter((x) => x.type).map((x) => `<span class="df-dot" style="--c:${colourOf(x.type ?? "")}"></span>${esc(x.name)}`).join(" ");
              const outs = n.outputs.map((o) => `<span class="df-dot" style="--c:${colourOf(o.type)}"></span>${esc(o.name)}`).join(" ");
              return `<button class="df-search-node" role="option" aria-selected="${i === searching?.pick}" data-df-pick="${esc(n.type)}"><b>${esc(n.title)}</b><span class="df-sockets">${ins}${ins || outs ? " → " : ""}${outs}</span></button>`;
            })
            .join(""),
      )
      .join("");
    const fits = searching.fits ? `<span class="df-fits">takes ${esc(searching.fits.type)}</span>` : "";
    const input = box.querySelector("input");
    if (!input) {
      box.innerHTML =
        `<label class="df-search-box"><span class="df-hidden">Search nodes</span><input data-df-query autocomplete="off" spellcheck="false" placeholder="Search nodes"></label>` +
        `<div class="df-search-fits"></div><div class="df-search-list" role="listbox" aria-label="Nodes"></div><div class="df-search-foot">↑↓ choose · ↩ add · ⎋ close</div>`;
    }
    /** @type {HTMLElement} */ (box.querySelector(".df-search-fits")).innerHTML = fits;
    /** @type {HTMLElement} */ (box.querySelector(".df-search-list")).innerHTML = list || `<p class="df-faint df-pad">Nothing called that${searching.fits ? " takes it" : ""}.</p>`;
    box.hidden = false;
  }

  /** Add a node of `type` where the search was opened, wired to the socket
   *  it was opened from. @param {string} type */
  function pick(type) {
    const f = flow();
    const kind = kindOf(type);
    if (!searching || !f || !kind) return;
    const id = freshId(f);
    /** @type {Op[]} */
    const ops = [];
    const from = searching.from;
    if (from?.detach) ops.push({ op: "disconnect", to: from.detach });
    ops.push({ op: "add", type, at: [Math.round(searching.at[0]), Math.round(searching.at[1])], values: seeded(kind) });
    if (from) {
      const theirs = socketFor(kind, from.type, from.side === "out" ? "in" : "out");
      if (theirs) ops.push(from.side === "out" ? { op: "connect", from: from.port, to: `${id}:${theirs}` } : { op: "connect", from: `${id}:${theirs}`, to: from.port });
    }
    closeSearch();
    edit(ops);
  }

  /** A new node's random seeds. @param {NodeType} kind */
  const seeded = (kind) => Object.fromEntries(kind.inputs.filter((i) => i.kind === "seed").map((i) => [i.name, newSeed()]));

  /* ── the pointer ─────────────────────────────────────────────────── */

  /** A point on the stage as a point on the graph. @param {PointerEvent | MouseEvent | DragEvent} e @returns {[number, number]} */
  const toGraph = (e) => {
    const rect = stage.getBoundingClientRect();
    const cam = camera();
    return [(e.clientX - rect.left - cam.x) / cam.z, (e.clientY - rect.top - cam.y) / cam.z];
  };

  /** Keep a pointer's moves coming here: a pen or a finger the window no
   *  longer knows of is not worth stopping for. @param {number} id */
  const capture = (id) => {
    try {
      stage.setPointerCapture(id);
    } catch {
      // A pointer already gone: its moves were not coming anyway.
    }
  };

  /** What a pointer is doing on the stage.
   *  @type {{kind: "pan", id: number, from: [number, number], cam: Camera, moved: boolean}
   *       | {kind: "move", id: number, from: [number, number], start: Map<string, [number, number]>, moved: boolean}
   *       | {kind: "wire", id: number} | null} */
  let gesture = null;

  stage.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !flow()) return;
    const target = /** @type {HTMLElement} */ (e.target);
    // The search and the mask sheet sit over the graph and are their own;
    // the gallery takes the graph's place, and a press on a picture there is
    // a click — caught for a pan, it would land on the stage, not the picture.
    if (target.closest("[data-df-search], .df-mask, [data-df-gallery]")) return;
    closeSearch();
    stage.focus({ preventScroll: true });
    const out = /** @type {HTMLElement | null} */ (target.closest("[data-df-out]"));
    const inp = /** @type {HTMLElement | null} */ (target.closest("[data-df-in]"));
    if (out || inp) {
      const f = flow();
      const port = (out ?? inp)?.dataset[out ? "dfOut" : "dfIn"] ?? "";
      const type = (out ?? inp)?.dataset.type ?? "";
      const link = inp ? f?.links.find((l) => l.to === port) : null;
      // An input already linked: its wire is picked up by its far end.
      pulling = link
        ? { port: link.from, side: "out", type, detach: port, at: toGraph(e) }
        : { port, side: out ? "out" : "in", type, detach: null, at: toGraph(e) };
      gesture = { kind: "wire", id: e.pointerId };
      capture(e.pointerId);
      drawWires();
      pullTo(e);
      e.preventDefault();
      return;
    }
    const grab = /** @type {HTMLElement | null} */ (target.closest("[data-df-grab]"));
    const nodeEl = /** @type {HTMLElement | null} */ (target.closest("[data-df-node]"));
    if (grab || nodeEl) {
      const id = (grab?.dataset.dfGrab ?? nodeEl?.dataset.dfNode) || "";
      if (e.shiftKey) {
        if (chosen.has(id)) chosen.delete(id);
        else chosen.add(id);
      } else if (!chosen.has(id)) chosen = new Set([id]);
      renderSelection();
      if (grab && chosen.has(id)) {
        const f = flow();
        const start = new Map(/** @type {[string, [number, number]][]} */ ((f?.nodes ?? []).filter((n) => chosen.has(n.id)).map((n) => [n.id, [n.at[0], n.at[1]]])));
        gesture = { kind: "move", id: e.pointerId, from: [e.clientX, e.clientY], start, moved: false };
        capture(e.pointerId);
        e.preventDefault();
      }
      return;
    }
    gesture = { kind: "pan", id: e.pointerId, from: [e.clientX, e.clientY], cam: { ...camera() }, moved: false };
    capture(e.pointerId);
  });

  stage.addEventListener("pointermove", (e) => {
    if (!gesture || gesture.id !== e.pointerId) return;
    if (gesture.kind === "pan") {
      const dx = e.clientX - gesture.from[0];
      const dy = e.clientY - gesture.from[1];
      if (Math.abs(dx) + Math.abs(dy) > 3) gesture.moved = true;
      const cam = camera();
      cam.x = gesture.cam.x + dx;
      cam.y = gesture.cam.y + dy;
      placeWorld();
    } else if (gesture.kind === "move") {
      const z = camera().z;
      const dx = (e.clientX - gesture.from[0]) / z;
      const dy = (e.clientY - gesture.from[1]) / z;
      if (Math.abs(dx) + Math.abs(dy) > 2) gesture.moved = true;
      /** @type {Map<string, [number, number]>} */
      const moved = new Map();
      for (const [id, at] of gesture.start) {
        const to = /** @type {[number, number]} */ ([Math.round(at[0] + dx), Math.round(at[1] + dy)]);
        moved.set(id, to);
        const el = /** @type {HTMLElement | null} */ (ws.querySelector(`[data-df-node="${CSS.escape(id)}"]`));
        if (el) {
          el.style.left = `${to[0]}px`;
          el.style.top = `${to[1]}px`;
        }
      }
      drawWires(moved);
    } else {
      pullTo(e);
    }
  });

  /** The wire being pulled, to the pointer. @param {PointerEvent} e */
  function pullTo(e) {
    if (!pulling) return;
    const temp = ws.querySelector("[data-df-temp]");
    const f = flow();
    const fixed = portOf(pulling.port);
    const node = fixed && f?.nodes.find((n) => n.id === fixed.node);
    const kind = node && kindOf(node.type);
    const at = node && kind && fixed ? portAt(node, kind, pulling.side, fixed.name) : null;
    if (!temp || !at) return;
    const here = toGraph(e);
    temp.setAttribute("d", pulling.side === "out" ? wire(at, here) : wire(here, at));
  }

  const endGesture = (/** @type {PointerEvent} */ e) => {
    if (!gesture || gesture.id !== e.pointerId) return;
    const was = gesture;
    gesture = null;
    if (was.kind === "pan" && !was.moved) {
      chosen.clear();
      renderSelection();
    } else if (was.kind === "move" && was.moved) {
      const z = camera().z;
      const dx = Math.round((e.clientX - was.from[0]) / z);
      const dy = Math.round((e.clientY - was.from[1]) / z);
      edit([...was.start].map(([id, at]) => ({ op: "place", id, at: [at[0] + dx, at[1] + dy] })));
    } else if (was.kind === "wire" && pulling) {
      const from = pulling;
      pulling = null;
      drawWires();
      const under = /** @type {HTMLElement | null} */ (document.elementFromPoint(e.clientX, e.clientY));
      const into = /** @type {HTMLElement | null} */ (under?.closest(from.side === "out" ? "[data-df-in]" : "[data-df-out]") ?? null);
      if (into) {
        const port = into.dataset[from.side === "out" ? "dfIn" : "dfOut"] ?? "";
        /** @type {Op[]} */
        const ops = [];
        if (from.detach && from.detach !== port) ops.push({ op: "disconnect", to: from.detach });
        if (from.detach !== port) ops.push(from.side === "out" ? { op: "connect", from: from.port, to: port } : { op: "connect", from: port, to: from.port });
        edit(ops);
      } else if (under && stage.contains(under) && !under.closest("[data-df-node]")) {
        // Let go on space: a node for it, chosen from those that fit.
        openSearch(toGraph(e), from);
      } else if (from.detach) {
        edit([{ op: "disconnect", to: from.detach }]);
      }
    }
  };
  stage.addEventListener("pointerup", endGesture);
  stage.addEventListener("pointercancel", (e) => {
    if (gesture?.kind === "wire") {
      pulling = null;
      drawWires();
    }
    gesture = null;
    render();
  });

  stage.addEventListener("dblclick", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (!flow() || target.closest("[data-df-node], [data-df-search], .df-mask")) return;
    openSearch(toGraph(e));
  });

  stage.addEventListener(
    "wheel",
    (e) => {
      if (!flow() || /** @type {HTMLElement} */ (e.target).closest?.(".df-mask")) return;
      e.preventDefault();
      const cam = camera();
      if (e.ctrlKey || e.metaKey) {
        const rect = stage.getBoundingClientRect();
        const [px, py] = [e.clientX - rect.left, e.clientY - rect.top];
        const z = Math.max(0.2, Math.min(2.5, cam.z * Math.exp(-e.deltaY * 0.01)));
        cam.x = px - ((px - cam.x) * z) / cam.z;
        cam.y = py - ((py - cam.y) * z) / cam.z;
        cam.z = z;
        renderGraph();
      } else {
        cam.x -= e.deltaX;
        cam.y -= e.deltaY;
        placeWorld();
      }
    },
    { passive: false },
  );

  // A node dragged from the catalogue.
  /** A drag that carries files. @param {DragEvent} e */
  const carriesFiles = (e) => [...(e.dataTransfer?.items ?? [])].some((i) => i.kind === "file");
  stage.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types.includes("application/x-diffuse-node") || carriesFiles(e)) e.preventDefault();
  });
  q("[data-df-gallery]").addEventListener("dragover", (e) => {
    if (carriesFiles(e)) e.preventDefault();
  });
  q("[data-df-gallery]").addEventListener("drop", (e) => void droppedPngs(e));
  stage.addEventListener("drop", (e) => {
    if (carriesFiles(e)) {
      void droppedPngs(e);
      return;
    }
    const type = e.dataTransfer?.getData("application/x-diffuse-node");
    const kind = type ? kindOf(type) : null;
    if (!type || !kind || !flow()) return;
    e.preventDefault();
    const [x, y] = toGraph(e);
    edit([{ op: "add", type, at: [Math.round(x - NODE_W / 2), Math.round(y - HEAD / 2)], values: seeded(kind) }]);
  });
  /** PNGs dropped on the graph or the gallery: each one's graph — the one
   *  forge wrote into it — opened as a new flow; a PNG with none, said so.
   *  @param {DragEvent} e */
  async function droppedPngs(e) {
    const files = [...(e.dataTransfer?.files ?? [])].filter((f) => f.type.startsWith("image/") || /\.(png|jpe?g|webp|gif)$/i.test(f.name));
    if (!files.length) return;
    e.preventDefault();
    const none = [];
    // Dropped on a Load Image: its picture. Elsewhere on the graph: a Load
    // Image of it there.
    const card = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-df-node]"));
    const onto = card ? flow()?.nodes.find((n) => n.id === card.dataset.dfNode && kindOf(n.type)?.inputs.some((i) => i.kind === "upload")) : null;
    const point = stageView === "graph" && flow() ? toGraph(e) : undefined;
    for (const file of files) {
      const text = /\.png$/i.test(file.name) || file.type === "image/png" ? pngText(new Uint8Array(await file.arrayBuffer())) : null;
      let graph = null;
      try {
        const read = text ? JSON.parse(text) : null;
        graph = read?.graph ?? read;
      } catch {
        graph = null;
      }
      if (graph && Array.isArray(graph.nodes) && Array.isArray(graph.links) && !onto) {
        send({ type: "diffuseOpenGraph", name: file.name, graph });
        setView("graph");
      } else if (flow() && stageView === "graph") {
        try {
          const rel = await keepPicture(file);
          const target = onto ? { node: onto.id, field: kindOf(onto.type)?.inputs.find((i) => i.kind === "upload")?.name ?? "image" } : null;
          usePicture(rel, target, point ? [point[0] - NODE_W / 2, point[1] - HEAD / 2] : undefined);
        } catch (error) {
          none.push(`${file.name} (${/** @type {Error} */ (error)?.message ?? error})`);
        }
      } else none.push(file.name);
    }
    if (none.length) {
      asking = { kind: "note", text: `${none.join(", ")}: no graph in ${none.length === 1 ? "it" : "them"}. Open a flow and drop a picture on the graph to use it as an input.` };
      renderAsk();
    }
  }

  ws.addEventListener("dragstart", (e) => {
    const item = /** @type {HTMLElement} */ (e.target).closest?.("[data-df-add]");
    if (item && e.dataTransfer) e.dataTransfer.setData("application/x-diffuse-node", /** @type {HTMLElement} */ (item).dataset.dfAdd ?? "");
  });

  /** The chosen nodes marked, and the inspector for them, without redrawing the graph. */
  function renderSelection() {
    for (const el of ws.querySelectorAll("[data-df-node]")) el.classList.toggle("on", chosen.has(/** @type {HTMLElement} */ (el).dataset.dfNode ?? ""));
    if (panel === "inspector") renderInspector();
  }

  /* ── clicks, fields and keys ─────────────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const at = (/** @type {string} */ s) => /** @type {HTMLElement | null} */ (t.closest(s));
    const sideTab = at("[data-df-side]");
    if (sideTab) {
      setSide(sideTab.dataset.dfSide ?? "flows");
      return;
    }
    const tab = at("[data-df-panel]");
    if (tab) return setPanel(tab.dataset.dfPanel ?? "inspector");
    const go = at("[data-df-panel-go]");
    if (go) return setPanel(go.dataset.dfPanelGo ?? "server");
    const open = at("[data-df-open]");
    if (open) return send({ type: "diffuseOpen", path: open.dataset.dfOpen });
    const close = at("[data-df-close]");
    if (close) {
      const p = close.dataset.dfClose ?? "";
      if (view?.open.find((t) => t.path === p)?.dirty) {
        asking = { kind: "close", path: p };
        renderAsk();
      } else send({ type: "diffuseClose", path: p });
      return;
    }
    const deleteFlow = at("[data-df-delete-flow]");
    if (deleteFlow) {
      asking = { kind: "deleteFlow", path: deleteFlow.dataset.dfDeleteFlow ?? "" };
      return renderAsk();
    }
    if (at("[data-df-delete-images]")) return askToDeleteImages();
    if (at("[data-df-folder-edit]")) {
      movingFolder = true;
      renderSide();
      /** @type {HTMLInputElement | null} */ (q("[data-df-folder-form] input"))?.focus();
      return;
    }
    if (at("[data-df-folder-cancel]")) {
      movingFolder = false;
      return renderSide();
    }
    if (at("[data-df-folder-default]")) {
      movingFolder = false;
      send({ type: "diffuseFolder" });
      return renderSide();
    }
    const answer = at("[data-df-answer]");
    if (answer && asking) {
      const was = asking;
      const said = answer.dataset.dfAnswer;
      asking = null;
      renderAsk();
      if (was.kind === "close") {
        if (said === "discard") send({ type: "diffuseClose", path: was.path, discard: true });
        if (said === "save") {
          send({ type: "diffuseOpen", path: was.path });
          send({ type: "diffuseSave" });
          send({ type: "diffuseClose", path: was.path });
        }
      } else if (said === "trash") {
        if (was.kind === "deleteFlow") send({ type: "diffuseDelete", path: was.path });
        else if (was.kind === "deleteImages") {
          send({ type: "diffuseDeleteImages", paths: was.paths });
          for (const p of was.paths) {
            pickedAll.delete(p);
            urls.delete(p);
          }
        }
      }
      return;
    }
    if (at("[data-df-look]")) return send({ type: "diffuseLook", again: true });
    const viewButton = at("[data-df-view]");
    if (viewButton) return setView(viewButton.dataset.dfView ?? "graph");
    if (at("[data-df-run]")) return runIt();
    if (at("[data-df-stop]")) return stopIt();
    const cancel = at("[data-df-cancel]");
    if (cancel) return send({ type: "diffuseCancel", id: Number(cancel.dataset.dfCancel) });
    const tile = at("[data-df-pick-image]");
    if (tile) {
      const p = tile.dataset.dfPickImage ?? "";
      const mouse = /** @type {MouseEvent} */ (e);
      if (mouse.metaKey || mouse.shiftKey || mouse.ctrlKey) {
        if (pickedAll.has(p)) pickedAll.delete(p);
        else pickedAll.add(p);
        picked = pickedAll.has(p) ? p : ([...pickedAll][0] ?? null);
      } else {
        picked = p;
        pickedAll = new Set([p]);
      }
      renderGallery();
      return setPanel("inspector");
    }
    const shownImage = at("[data-df-show-image]");
    if (shownImage) {
      picked = shownImage.dataset.dfShowImage ?? null;
      setView("gallery");
      return setPanel("inspector");
    }
    const pickInput = at("[data-df-pick-input]");
    const one = [...chosen][0];
    if (pickInput && one) {
      pickingInput = { node: one, field: pickInput.dataset.dfPickInput ?? "image" };
      return setView("gallery");
    }
    const maskButton = at("[data-df-mask]");
    if (maskButton) return void paintMask(maskButton.dataset.dfMask ?? "image");
    const useInput = at("[data-df-use-input]");
    if (useInput) {
      const target = pickingInput;
      pickingInput = null;
      setView("graph");
      usePicture(useInput.dataset.dfUseInput ?? "", target);
      return;
    }
    const sendTo = at("[data-df-send]");
    if (sendTo && picked) return send({ type: "diffuseSend", path: picked, to: sendTo.dataset.dfSend });
    const openImage = at("[data-df-open-image]");
    if (openImage) {
      send({ type: "diffuseOpenImage", path: openImage.dataset.dfOpenImage });
      return setView("graph");
    }
    const openFlow = at("[data-df-open-flow]");
    if (openFlow) {
      send({ type: "diffuseOpen", path: openFlow.dataset.dfOpenFlow });
      return setView("graph");
    }
    const copy = at("[data-df-copy-path]");
    if (copy) return void copyText(copy.dataset.dfCopyPath ?? "");
    if (at("[data-df-chip]")) return setPanel("server");
    if (at("[data-df-undo]")) return send({ type: "diffuseUndo" });
    if (at("[data-df-redo]")) return send({ type: "diffuseRedo" });
    if (at("[data-df-save]")) return send({ type: "diffuseSave" });
    const add = at("[data-df-add]");
    if (add) {
      const type = add.dataset.dfAdd ?? "";
      const kind = kindOf(type);
      const rect = stage.getBoundingClientRect();
      const cam = camera();
      if (kind && flow()) edit([{ op: "add", type, at: [Math.round((rect.width / 2 - cam.x) / cam.z - NODE_W / 2), Math.round((rect.height / 3 - cam.y) / cam.z)], values: seeded(kind) }]);
      return;
    }
    const pickEl = at("[data-df-pick]");
    if (pickEl) return pick(pickEl.dataset.dfPick ?? "");
    const choose = at("[data-df-choose]");
    if (choose) {
      chosen = new Set([choose.dataset.dfChoose ?? ""]);
      renderSelection();
      return;
    }
    const seed = at("[data-df-seed]");
    if (seed && one) return edit([{ op: "seed", id: one, mode: /** @type {any} */ (seed.dataset.dfSeed) }]);
    const reseed = at("[data-df-reseed]");
    if (reseed && one) return edit([{ op: "set", id: one, name: reseed.dataset.dfReseed ?? "seed", value: newSeed() }]);
    const act = at("[data-df-act]");
    if (act) return act_(act.dataset.dfAct ?? "");
  });

  /** Bypass, duplicate or remove what is chosen. @param {string} what */
  function act_(what) {
    const f = flow();
    const ids = [...chosen].filter((id) => f?.nodes.some((n) => n.id === id));
    if (!ids.length) return;
    if (what === "remove") {
      chosen.clear();
      edit([{ op: "remove", ids }]);
    } else if (what === "duplicate") {
      edit([{ op: "duplicate", ids, by: [32, 32] }]);
    } else if (what === "bypass") {
      const on = !ids.every((id) => f?.nodes.find((n) => n.id === id)?.bypass);
      edit(ids.map((id) => ({ op: "bypass", id, on })));
    }
  }

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (form.matches("[data-df-folder-form]")) {
      e.preventDefault();
      const typed = String(new FormData(form).get("folder") ?? "").trim();
      movingFolder = false;
      send(typed ? { type: "diffuseFolder", folder: typed } : { type: "diffuseFolder" });
      renderSide();
      return;
    }
    if (form.matches("[data-df-gateways]")) {
      e.preventDefault();
      const data = new FormData(form);
      send({ type: "diffuseGateways", home: String(data.get("home") ?? "").trim() || undefined, away: String(data.get("away") ?? "").trim() || undefined });
      return;
    }
    if (!form.matches("[data-df-new]")) return;
    e.preventDefault();
    const data = new FormData(form);
    const name = String(data.get("name") ?? "").trim();
    if (!name) return;
    const template = String(data.get("template") ?? "");
    send({ type: "diffuseNew", name, ...(template ? { template } : {}) });
  });

  ws.addEventListener("change", (e) => {
    const el = /** @type {HTMLInputElement} */ (e.target);
    const one = [...chosen][0];
    if (el.dataset.dfUploadFile && one && el.files?.[0]) {
      const field = el.dataset.dfUploadFile;
      const file = el.files[0];
      void keepPicture(file).then(
        (rel) => usePicture(rel, { node: one, field }),
        (error) => {
          asking = { kind: "note", text: `${file.name} could not be kept: ${error?.message ?? error}` };
          renderAsk();
        },
      );
      return;
    }
    if (el.dataset.dfSet && one) {
      setField(el, one);
      return;
    }
    if (el.matches("[data-df-size]") && one && el.value) {
      const [w, h] = el.value.split("x").map(Number);
      edit([{ op: "set", id: one, name: "width", value: w }, { op: "set", id: one, name: "height", value: h }]);
      return;
    }
    if (el.matches("[data-df-folder]")) {
      folder = el.value;
      renderGallery();
      return;
    }
    if (el.dataset.dfFlow === "note") {
      delete el.dataset.dfPending;
      edit([{ op: "note", text: el.value }]);
    }
    if (el.dataset.dfFlow === "outputs") edit([{ op: "outputs", path: el.value }]);
  });

  /** A field's value set on node `id`, as the person left it. @param {HTMLInputElement} el @param {string} id */
  function setField(el, id) {
    delete el.dataset.dfPending;
    const kind = el.dataset.kind;
    const raw = el.value;
    /** @type {unknown} */
    let value = raw;
    if (kind === "int" || kind === "float" || kind === "seed") value = raw.trim() === "" ? null : Number(raw);
    if (typeof value === "number" && !Number.isFinite(value)) return;
    if (kind === "int" && typeof value === "number") {
      value = onStep(el, value);
      el.value = String(value);
    }
    edit([{ op: "set", id, name: el.dataset.dfSet ?? "", value }]);
  }

  // A field put back after the inspector was drawn again, left with no more
  // typed: set now, since no change will be said of it.
  ws.addEventListener("focusout", (e) => {
    const el = /** @type {HTMLInputElement} */ (e.target);
    const one = [...chosen][0];
    if (el.dataset?.dfPending === undefined) return;
    if (el.dataset.dfSet && one) setField(el, one);
    else if (el.dataset.dfFlow === "note") {
      delete el.dataset.dfPending;
      edit([{ op: "note", text: el.value }]);
    }
  });

  ws.addEventListener("input", (e) => {
    const el = /** @type {HTMLInputElement} */ (e.target);
    if (el.matches("textarea.df-prompt")) {
      fitPrompt(/** @type {HTMLTextAreaElement} */ (/** @type {unknown} */ (el)));
      return;
    }
    if (el.matches("[data-df-query]") && searching) {
      searching.query = el.value;
      searching.pick = 0;
      renderSearch();
    }
  });

  ws.addEventListener("keydown", (e) => {
    const el = /** @type {HTMLElement} */ (e.target);
    if (!el.matches?.("[data-df-query]") || !searching) return;
    const options = [...ws.querySelectorAll("[data-df-pick]")];
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      searching.pick = Math.max(0, Math.min(options.length - 1, searching.pick + (e.key === "ArrowDown" ? 1 : -1)));
      renderSearch();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const chosenOption = /** @type {HTMLElement | undefined} */ (options[searching.pick]);
      if (chosenOption) pick(chosenOption.dataset.dfPick ?? "");
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeSearch();
      stage.focus({ preventScroll: true });
    }
    e.stopPropagation();
  });

  /** Ask before moving the images chosen in the gallery to the Trash. */
  function askToDeleteImages() {
    const paths = [...pickedAll].filter((p) => images.some((i) => i.path === p));
    if (!paths.length) return;
    asking = { kind: "deleteImages", paths };
    renderAsk();
  }

  /** The graph or the outputs. @param {string} to */
  function setView(to) {
    stageView = to === "gallery" ? "gallery" : "graph";
    if (stageView === "gallery") send({ type: "diffuseGallery" });
    closeSearch();
    renderBar();
    renderGraph();
    renderPanel();
  }

  /** Run the flow shown, or queue another run of it. */
  function runIt() {
    const p = path();
    const button = /** @type {HTMLButtonElement} */ (q("[data-df-run]"));
    if (!p || button.disabled) return;
    send({ type: "diffuseRun", path: p });
    if (panel === "server") setPanel("queue");
  }

  /** Stop the flow's run. */
  function stopIt() {
    const r = activeRun();
    if (r) send({ type: "diffuseCancel", id: r.id });
  }

  /** @param {string} to */
  function setSide(to) {
    side = to;
    renderSide();
  }

  /** @param {string} to */
  function setPanel(to) {
    panel = to;
    renderPanel();
  }

  /** A key, when Diffuse is on screen: whether it was taken. @param {KeyboardEvent} e */
  function key(e) {
    const target = /** @type {HTMLElement} */ (e.target);
    // The mask sheet's keys are its own: ⌫ there must not take a node away.
    if (target.closest?.(".df-mask")) return false;
    const typing = target.closest?.("input, textarea, select, [contenteditable]");
    const mod = e.metaKey || e.ctrlKey;
    if (mod && !e.altKey && e.key.toLowerCase() === "s") {
      if (view?.shown?.dirty) send({ type: "diffuseSave" });
      return true;
    }
    if (mod && e.key === "Enter") {
      // A field being typed in is set first, so the run has what was typed.
      if (typing) /** @type {HTMLElement} */ (target).blur();
      runIt();
      return true;
    }
    if (mod && e.key === ".") {
      stopIt();
      return true;
    }
    if (typing) return false;
    if (mod && e.key.toLowerCase() === "z") {
      send({ type: e.shiftKey ? "diffuseRedo" : "diffuseUndo" });
      return true;
    }
    if (mod && e.key.toLowerCase() === "d") {
      act_("duplicate");
      return true;
    }
    if (mod && e.key.toLowerCase() === "a" && flow()) {
      chosen = new Set(flow()?.nodes.map((n) => n.id));
      renderSelection();
      return true;
    }
    if (mod) return false;
    if ((e.key === "Backspace" || e.key === "Delete") && stageView === "gallery") {
      askToDeleteImages();
      return true;
    }
    if ((e.key === "Backspace" || e.key === "Delete") && chosen.size) {
      act_("remove");
      return true;
    }
    if (e.key === "Escape") {
      if (searching) closeSearch();
      else {
        chosen.clear();
        renderSelection();
      }
      return true;
    }
    if (e.key === "f" && flow()) {
      const p = path();
      if (p) cameras.set(p, fitted());
      renderGraph();
      return true;
    }
    if (e.key === "?") {
      host.keys?.("Diffuse", KEYS);
      return true;
    }
    return false;
  }

  /** Ask the core to look at forge, now and every half minute while shown. */
  function lookNow() {
    send({ type: "diffuseLook" });
  }

  return {
    /** The flows, those open and the one shown. @param {View} next */
    view(next) {
      const before = path();
      view = next;
      const f = flow();
      if (next.added.length) chosen = new Set(next.added);
      else if (path() !== before) chosen.clear();
      if (f) chosen = new Set([...chosen].filter((id) => f.nodes.some((n) => n.id === id)));
      render();
      if (searching) renderSearch();
    },
    /** The runs this session. @param {{runs: Run[]}} event */
    runs(event) {
      runs = event.runs ?? [];
      renderBar();
      if (stageView === "graph") renderGraph();
      if (panel === "queue") renderQueue();
    },
    /** The project's outputs. @param {{images: Image[]}} event */
    gallery(event) {
      images = event.images ?? [];
      pickedAll = new Set([...pickedAll].filter((p) => images.some((i) => i.path === p)));
      if (picked && !images.some((i) => i.path === picked)) picked = null;
      if (!picked) picked = [...pickedAll][0] ?? images[0]?.path ?? null;
      if (picked && !pickedAll.size) pickedAll.add(picked);
      if (stageView === "gallery") {
        renderGallery();
        if (panel === "inspector") renderPicked();
      }
    },
    /** What forge is and has. @param {{known: Known, gateways: string[]}} event */
    known(event) {
      known = event.known;
      gateways = event.gateways ?? [];
      render();
    },
    /** The mode came on screen, or went. @param {boolean} on */
    show(on) {
      if (on === shown) return;
      shown = on;
      if (looking) clearInterval(looking);
      looking = null;
      if (on) {
        lookNow();
        looking = setInterval(lookNow, LOOK_EVERY);
        render();
      } else {
        closeSearch();
      }
    },
    key,
    /** For tests: the nodes chosen. */
    chosen: () => [...chosen],
    /** For tests: where a port is on the screen. @param {string} port @param {"in" | "out"} side */
    portOnScreen(port, side) {
      const el = /** @type {HTMLElement | null} */ (ws.querySelector(side === "in" ? `[data-df-in="${CSS.escape(port)}"]` : `[data-df-out="${CSS.escape(port)}"]`));
      const r = el?.getBoundingClientRect();
      return r ? [r.left + r.width / 2, r.top + r.height / 2] : null;
    },
  };
}
