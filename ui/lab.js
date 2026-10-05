/* The Lab, in Research's right pane: the Python an investigation's code
 * runs in, and — next — the runs.
 *
 * The Lab's Python is Proton's own conda environment, made with the
 * scientist's say-so twice: once to plan it (conda asks conda-forge what it
 * would be, fetching its package index if conda's copy is old), and once to
 * make exactly that plan, with every package and what downloading them
 * costs in front of them. Each step is the core's; this draws where it
 * stands, and asks for the next only when a button says so. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/LabEnv").LabEnv} Env */
/** @typedef {import("./bindings/LabPlan").LabPlan} Plan */

/** Bytes, as a size people read. @param {number} n */
export function bytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** A path, with the home folder as `~`. @param {string} path */
const home = (path) => String(path ?? "").replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");

/** A day, from seconds since the epoch. @param {number} s */
const day = (s) => new Date(s * 1000).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createLab(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("[data-lab-env]"));
  if (!found) return null;
  const box = found;
  /** @type {{env: Env | null, confirming: boolean}} */
  const state = { env: null, confirming: false };

  /** @param {string[]} log */
  const tail = (log) => (log.length ? `<pre class="lab-log">${esc(log.slice(-8).join("\n"))}</pre>` : "");

  /** @param {Plan} plan */
  function planHtml(plan) {
    const fetched = plan.packages.length - plan.cached;
    const rows = plan.packages.map((p) =>
      `<tr><td>${esc(p.name)}</td><td>${esc(p.version)}</td><td>${p.size == null ? `<span class="lab-faint">cached</span>` : esc(bytes(p.size))}</td></tr>`).join("");
    return `<div class="lab-step">
        <b>Then, if you agree.</b> ${plan.packages.length} packages from conda-forge:
        ${fetched ? `<b>${esc(bytes(plan.download))}</b> to download for ${fetched}` : "nothing to download"}${plan.cached ? `, ${plan.cached} already in conda's cache` : ""},
        into <code>${esc(home(plan.prefix))}</code>.
      </div>
      <details class="lab-packages"><summary>Every package</summary><table>${rows}</table></details>
      <div class="lab-acts">
        <button class="cc-quiet" data-lab="cancel">Not now</button>
        <button class="cc-primary" data-lab="create">Make it</button>
      </div>`;
  }

  function html() {
    const env = state.env;
    if (!env) return "";
    switch (env.state) {
      case "unknown":
        return `<p class="lab-faint">Looking for the Lab's Python…</p>`;
      case "noConda":
        return `<p>The Lab runs code in a Python of its own, made with conda — and there is no conda here.
          Install Miniforge, conda-forge's own conda, then come back to Research.</p>`;
      case "missing": {
        const cached = env.index
          ? `conda's copy of it is from ${esc(day(env.index))}, and it is refreshed if it is out of date`
          : "conda has no copy of it yet, so it is downloaded — tens of megabytes";
        const python = env.specs.find((s) => s.startsWith("python"))?.replace(/^python=?/, "") ?? "";
        const stack = env.specs.filter((s) => !s.startsWith("python")).map((s) => s.replace(/=.*$/, "").replace(/-base$/, ""));
        const listed = stack.length > 1 ? `${stack.slice(0, -1).join(", ")} and ${stack[stack.length - 1]}` : stack.join("");
        return `<p>The Lab runs an investigation's code in a Python of its own — Python ${esc(python)} with
          ${esc(listed)} — in <code>${esc(home(env.prefix))}</code>, apart from your own environments.</p>
          <div class="lab-step"><b>First, a plan.</b> conda asks conda-forge what that environment would be, which needs
          conda-forge's package index for this machine: ${cached}. Nothing is installed yet.</div>
          <div class="lab-acts"><button class="cc-primary" data-lab="plan">Plan the Lab's Python</button></div>`;
      }
      case "planning":
        return `<p class="lab-busy"><span class="lab-spin"></span> conda is working out the environment. This can take a minute.</p>
          ${tail(env.log)}
          <div class="lab-acts"><button class="cc-quiet" data-lab="cancel">Stop</button></div>`;
      case "planned":
        return planHtml(env.plan);
      case "creating": {
        const pct = env.total ? Math.round((env.fetched / env.total) * 100) : 0;
        return `<p class="lab-busy"><span class="lab-spin"></span> Making the Lab's Python: ${env.fetched} of ${env.total} downloaded.</p>
          <div class="lab-bar"><i style="width:${pct}%"></i></div>
          ${tail(env.log)}
          <div class="lab-acts"><button class="cc-quiet" data-lab="cancel">Stop</button></div>`;
      }
      case "ready": {
        const packages = env.packages.map((p) => `${esc(p.name)} <span class="lab-faint">${esc(p.version)}</span>`).join(" · ");
        const acts = state.confirming
          ? `<div class="lab-confirm">Take the Lab's Python away? Runs keep what they wrote.
              <div class="lab-acts"><button class="cc-quiet" data-lab="keep">Keep it</button><button class="cc-danger" data-lab="remove">Remove</button></div></div>`
          : `<div class="lab-acts"><button class="cc-quiet" data-lab="confirm">Remove…</button></div>`;
        return `<p><b>Python ${esc(env.python)}</b> ${icon("check", "lab-ok")}</p>
          <p class="lab-packs">${packages}</p>
          <p class="lab-faint"><code>${esc(home(env.prefix))}</code></p>${acts}`;
      }
      case "removing":
        return `<p class="lab-busy"><span class="lab-spin"></span> Removing the Lab's Python…</p>`;
      case "failed": {
        const what = { plan: "Planning", create: "Making it", remove: "Removing it", inspect: "Reading it" }[env.step] ?? "It";
        return `<p class="lab-err">${esc(what)} failed: ${esc(env.why)}</p>
          ${env.log.length ? `<details class="lab-packages"><summary>What conda said</summary>${tail(env.log)}</details>` : ""}
          <div class="lab-acts"><button class="cc-quiet" data-lab="cancel">Start over</button>
          <button class="cc-primary" data-lab="plan">Try again</button></div>`;
      }
      default:
        return "";
    }
  }

  function render() {
    box.innerHTML = `<header>Python</header>${html()}`;
  }

  box.addEventListener("click", (e) => {
    const button = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-lab]"));
    if (!button) return;
    const what = button.dataset.lab;
    if (what === "confirm" || what === "keep") {
      state.confirming = what === "confirm";
      render();
      return;
    }
    state.confirming = false;
    const command = { plan: "labPlan", create: "labCreate", cancel: "labCancel", remove: "labRemove" }[/** @type {string} */ (what)];
    if (command) host.dispatch({ type: command });
  });

  /** @param {{env: Env}} event */
  function env(event) {
    if (event.env.state !== "ready") state.confirming = false;
    state.env = event.env;
    render();
  }

  render();
  return { env };
}
