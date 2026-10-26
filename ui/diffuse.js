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

import { escapeHtml as esc } from "./html.js";
import { colourOf, fieldsOf, freshId, grouped, HEAD, NODE_W, portAt, portOf, rowsOf, search, shortValue, socketFor, wire } from "./diffuse-graph.js";

/** @typedef {import('./bindings/DiffuseView').DiffuseView} View */
/** @typedef {import('./bindings/DiffuseKnown').DiffuseKnown} Known */
/** @typedef {import('./bindings/DiffuseFlow').DiffuseFlow} Flow */
/** @typedef {import('./bindings/DiffuseNode').DiffuseNode} Node */
/** @typedef {import('./bindings/DiffuseNodeType').DiffuseNodeType} NodeType */
/** @typedef {import('./bindings/DiffuseInput').DiffuseInput} Input */
/** @typedef {import('./bindings/DiffuseOp').DiffuseOp} Op */
/** @typedef {import('./bindings/DiffuseProblem').DiffuseProblem} Problem */
/** @typedef {{x: number, y: number, z: number}} Camera */
/** @typedef {{port: string, side: "in" | "out", type: string, detach: string | null, at: [number, number]}} Pulling */

/** How often forge is looked at again while the mode is on screen. */
const LOOK_EVERY = 30_000;
/** The keys, for `?`. */
const KEYS = /** @type {[string, string][]} */ ([
  ["double-click", "add a node there"], ["drag from a socket", "a wire; let go on space to add a node for it"],
  ["drag a node's head", "move it — ⇧ adds it to those chosen"], ["drag the ground", "move over the graph"],
  ["⌘ wheel  pinch", "zoom"], ["⌫", "remove what is chosen"], ["⌘D", "a copy of what is chosen"], ["⌘A", "choose every node"],
  ["⌘Z ⇧⌘Z", "undo, redo"], ["⌘S", "save the flow"], ["f", "the whole graph"], ["⎋", "close the search, choose nothing"], ["?", "these keys"],
]);

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
  /** The flow whose unsaved changes a close is asking about. @type {string | null} */
  let closing = null;
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
      body.innerHTML =
        `<div class="df-head">Flows here</div>` +
        (flows.length
          ? flows
              .map((f) => `<button class="df-flow${f === path() ? " on" : ""}" data-df-open="${esc(f)}"><span class="df-flow-name">${esc(f)}</span>${open.has(f) ? `<span class="df-flow-open" title="Open">●</span>` : ""}</button>`)
              .join("")
          : `<p class="df-faint">No flow in this project yet.</p>`) +
        `<form class="df-new" data-df-new><div class="df-head">New flow</div>` +
        `<label>Name <input name="name" data-df-new-name placeholder="lighthouse" autocomplete="off" spellcheck="false" required></label>` +
        `<label class="df-radio"><input type="radio" name="template" value="textToImage" checked> Text to image <span>7 nodes</span></label>` +
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
    run.disabled = true;
    run.title = !s
      ? "Open a flow to run it"
      : s.problems.length
        ? `Not ready: ${s.problems[0].message}`
        : known?.health.state !== "serving"
          ? known?.health.message || "forge is not serving"
          : "Running a flow is not built yet";
  }

  function renderAsk() {
    const box = q("[data-df-ask]");
    box.hidden = !closing;
    if (!closing) return;
    box.innerHTML =
      `<span class="df-banner-text"><b>${esc(closing)}</b> has changes not saved.</span>` +
      `<button class="cc-quiet" data-df-answer="stay">Keep it open</button>` +
      `<button class="cc-quiet" data-df-answer="discard">Close without saving</button>` +
      `<button class="cc-primary" data-df-answer="save">Save and close</button>`;
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
    q("[data-df-foot]").innerHTML =
      `<span>${Math.round(camera().z * 100)}%</span><span>${f.nodes.length} nodes · ${f.links.length} wires</span>` +
      (problems.length ? `<span class="df-foot-problem">${problems.length === 1 ? "1 thing" : `${problems.length} things`} to fix before it runs</span>` : `<span class="df-foot-ok">ready to run</span>`) +
      `<span class="grow"></span><span>double-click: add a node · drag from a socket: wire · ? keys</span>`;
  }

  /** One node's card. @param {Node} node @param {boolean} troubled */
  function card(node, troubled) {
    const kind = kindOf(node.type);
    const classes = ["df-node", chosen.has(node.id) ? "on" : "", node.bypass ? "bypassed" : "", troubled ? "troubled" : "", kind ? "" : "unknown"].filter(Boolean).join(" ");
    const head =
      `<div class="df-node-head" data-df-grab="${esc(node.id)}"><b>${esc(kind?.title ?? node.type)}</b><span class="df-node-id">${esc(node.id)}</span>` +
      (node.bypass ? `<span class="df-badge">bypassed</span>` : "") +
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
    return `<div class="${classes}" data-df-node="${esc(node.id)}" style="left:${node.at[0]}px;top:${node.at[1]}px;width:${NODE_W}px">${head}<div class="df-rows">${rows}</div>${fields ? `<div class="df-fields">${fields}</div>` : ""}</div>`;
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
    if (panel === "inspector") renderInspector();
    if (panel === "server") renderServer();
  }

  function renderInspector() {
    const box = q("[data-df-inspector]");
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
      `<div class="df-fieldset">${fieldsOf(kind).map((field) => fieldHtml(node, field)).join("")}</div>` +
      inputsHtml(node, kind) +
      actions(node);
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
          ? `<div class="df-field">${label}<textarea id="${id}" data-df-set="${esc(field.name)}" data-kind="text" rows="4">${esc(String(shownValue))}</textarea></div>`
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
      case "upload":
        return `<div class="df-field">${label}<span class="df-faint">${value ? esc(String(value)) : "no image yet"} — picking an image comes with masks</span></div>`;
      default:
        return `<div class="df-field">${label}<code class="df-faint">${esc(JSON.stringify(shownValue))}</code></div>`;
    }
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
      (gateways.length ? gateways.map((g, i) => `<div class="df-address"><span class="df-faint">${i === 0 ? "at home" : "away"}</span><code>${esc(g)}</code></div>`).join("") : `<p class="df-pad df-faint">No address set.</p>`) +
      `<p class="df-pad df-faint">Your own settings set these: <code>diffuse.gateway</code> and <code>diffuse.gatewayAway</code> in <code>~/.config/proton/settings.json</code>. The key is <code>NEXUS_KEY</code> from your shell, never stored.</p>` +
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
    if (target.closest("[data-df-search]")) return;
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
    if (!flow() || target.closest("[data-df-node], [data-df-search]")) return;
    openSearch(toGraph(e));
  });

  stage.addEventListener(
    "wheel",
    (e) => {
      if (!flow()) return;
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
  stage.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types.includes("application/x-diffuse-node")) e.preventDefault();
  });
  stage.addEventListener("drop", (e) => {
    const type = e.dataTransfer?.getData("application/x-diffuse-node");
    const kind = type ? kindOf(type) : null;
    if (!type || !kind || !flow()) return;
    e.preventDefault();
    const [x, y] = toGraph(e);
    edit([{ op: "add", type, at: [Math.round(x - NODE_W / 2), Math.round(y - HEAD / 2)], values: seeded(kind) }]);
  });
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
        closing = p;
        renderAsk();
      } else send({ type: "diffuseClose", path: p });
      return;
    }
    const answer = at("[data-df-answer]");
    if (answer && closing) {
      const p = closing;
      closing = null;
      renderAsk();
      if (answer.dataset.dfAnswer === "discard") send({ type: "diffuseClose", path: p, discard: true });
      if (answer.dataset.dfAnswer === "save") {
        send({ type: "diffuseOpen", path: p });
        send({ type: "diffuseSave" });
        send({ type: "diffuseClose", path: p });
      }
      return;
    }
    if (at("[data-df-look]")) return send({ type: "diffuseLook", again: true });
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
    const one = [...chosen][0];
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
    if (el.dataset.dfSet && one) {
      const kind = el.dataset.kind;
      const raw = el.value;
      /** @type {unknown} */
      let value = raw;
      if (kind === "int" || kind === "float" || kind === "seed") value = raw.trim() === "" ? null : Number(raw);
      if (typeof value === "number" && !Number.isFinite(value)) return;
      edit([{ op: "set", id: one, name: el.dataset.dfSet, value }]);
      return;
    }
    if (el.dataset.dfFlow === "note") edit([{ op: "note", text: el.value }]);
    if (el.dataset.dfFlow === "outputs") edit([{ op: "outputs", path: el.value }]);
  });

  ws.addEventListener("input", (e) => {
    const el = /** @type {HTMLInputElement} */ (e.target);
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
    const typing = target.closest?.("input, textarea, select, [contenteditable]");
    const mod = e.metaKey || e.ctrlKey;
    if (mod && !e.altKey && e.key.toLowerCase() === "s") {
      if (view?.shown?.dirty) send({ type: "diffuseSave" });
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
