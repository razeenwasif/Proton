/* Lean, in the Lab pane: Proton's own Lean 4 and Mathlib, in which a
 * mathematics investigation's leading hypothesis is stated and its proof
 * checked, each round.
 *
 * Made with the scientist's say-so twice, as the Lab's Python is: once to
 * plan it (GitHub is asked which Lean Mathlib has tagged, and what making
 * it would download and keep — exact sizes said as such, estimates as
 * estimates), and once to make exactly that, in ~/.proton/lean. Each step
 * is the core's; this draws where it stands, and asks for the next only
 * when a button says so. It is shown for a mathematics investigation, and
 * whenever Lean is made, being made, or failed to be. */

import { escapeHtml as esc, icon } from "./html.js";
import { bytes } from "./lab.js";

/** @typedef {import("./bindings/LeanEnv").LeanEnv} Env */
/** @typedef {import("./bindings/LeanPlan").LeanPlan} Plan */

/** A making's stages, in order, as the core names them. */
export const STAGES = /** @type {[string, string][]} */ ([
  ["elan", "elan, Lean's installer"],
  ["toolchain", "Lean's toolchain"],
  ["mathlib", "Mathlib's sources"],
  ["library", "Mathlib's prebuilt library"],
  ["finding", "Finding Mathlib"],
  ["checking", "A first proof"],
  ["finishing", "Tidying up"],
]);

/** What failed, in words. */
const FAILED = /** @type {Record<string, string>} */ ({
  plan: "Planning", elan: "Installing elan", toolchain: "Installing Lean", mathlib: "Fetching Mathlib",
  library: "Downloading Mathlib's library", finding: "Finding Mathlib", checking: "The first proof", finishing: "Finishing",
});

/** A path, with the home folder as `~`. @param {string} path */
const home = (path) => String(path ?? "").replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");

/** A size, said as exact or as about. @param {number} n @param {boolean} exact */
const size = (n, exact) => (exact ? bytes(n) : `about ${bytes(n)}`);

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createLean(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("[data-lean-env]"));
  if (!found) return null;
  const box = found;
  /** @type {{env: Env | null, confirming: boolean, mathematics: boolean}} */
  const state = { env: null, confirming: false, mathematics: false };

  /** @param {string[]} log */
  const tail = (log) => (log.length ? `<pre class="lab-log">${esc(log.slice(-8).join("\n"))}</pre>` : "");

  /** Whether the card is shown: for mathematics, or for a Lean that is
   *  anything but missing — made, being made, or failed. */
  function shown() {
    const s = state.env?.state;
    return state.mathematics || (s != null && s !== "unknown" && s !== "missing");
  }

  /** @param {Plan} plan */
  function planHtml(plan) {
    const rows = plan.downloads.map((d) => `<tr><td>${esc(d.what)}<br><span class="lab-faint">${esc(d.host)}</span></td>` +
      `<td>${esc(size(d.bytes, d.exact))}</td><td>${d.verified ? icon("check", "lab-ok") : ""}</td></tr>`).join("");
    const estimated = plan.downloads.some((d) => !d.exact);
    const room = plan.fits
      ? `${esc(bytes(plan.free))} is free.`
      : `<span class="lab-err">There is not room for it: ${esc(bytes(plan.free))} is free.</span>`;
    return `<div class="lab-step">
        <b>Then, if you agree.</b> Lean ${esc(plan.lean)} and the Mathlib tagged for it, installed with elan ${esc(plan.elan)}:
        <b>${esc(size(plan.download, !estimated))}</b> to download, taking up to ${esc(bytes(plan.disk))} while it is made and
        keeping about ${esc(bytes(plan.kept))}, in <code>${esc(home(plan.home))}</code>. ${room}
      </div>
      <details class="lab-packages lean-downloads" open><summary>What it downloads</summary><table>${rows}</table>
        <p class="lab-faint">${icon("check", "lab-ok")} checked against the SHA-256 its source publishes.${
          estimated ? " Mathlib's sizes are estimates: only its own tools know them exactly." : ""}</p></details>
      <div class="lab-acts">
        <button class="cc-quiet" data-lean="cancel">Not now</button>
        <button class="cc-primary" data-lean="create"${plan.fits ? "" : " disabled"}>Make it</button>
      </div>`;
  }

  /** The stages of a making, those done ticked. @param {string} stage */
  function stagesHtml(stage) {
    const at = STAGES.findIndex(([id]) => id === stage);
    return `<ol class="lean-stages">${STAGES.map(([id, name], n) =>
      `<li data-stage="${id}" class="${n < at ? "done" : n === at ? "on" : ""}">${n < at ? icon("check", "lab-ok") : ""}${esc(name)}</li>`).join("")}</ol>`;
  }

  function html() {
    const env = state.env;
    if (!env) return "";
    switch (env.state) {
      case "unknown":
        return `<p class="lab-faint">Looking for Lean…</p>`;
      case "missing": {
        const about = `<p>Each round, the team states a mathematics investigation's leading hypothesis in Lean 4 with Mathlib and
          tries to prove it — or its negation — and Lean checks every proof, in the sandbox, without asking. Lean and Mathlib
          are Proton's own, in <code>${esc(home(env.home))}</code>, apart from any Lean of yours.</p>`;
        if (env.needs.length) {
          return `${about}<p>It cannot be made here yet. It needs:</p><ul class="lean-needs">${env.needs.map((n) => `<li>${esc(n)}</li>`).join("")}</ul>`;
        }
        return `${about}
          <div class="lab-step"><b>First, a plan.</b> Proton asks GitHub which Lean Mathlib has tagged, and what making it
            costs — a few kilobytes. Nothing is downloaded yet. It needs about 17 GB free while it is made;
            ${esc(bytes(env.free))} is free here.</div>
          <div class="lab-acts"><button class="cc-primary" data-lean="plan">Plan Lean</button></div>`;
      }
      case "planning":
        return `<p class="lab-busy"><span class="lab-spin"></span> Asking GitHub which Lean Mathlib has tagged…</p>
          <div class="lab-acts"><button class="cc-quiet" data-lean="cancel">Stop</button></div>`;
      case "planned":
        return planHtml(env.plan);
      case "making": {
        const name = STAGES.find(([id]) => id === env.stage)?.[1] ?? env.stage;
        const pct = env.total ? Math.min(100, Math.round((env.done / env.total) * 100)) : 0;
        const amount = env.total ? ` — ${esc(bytes(env.done))} of about ${esc(bytes(env.total))}` : "";
        return `<p class="lab-busy"><span class="lab-spin"></span> Making Lean: ${esc(name)}${amount}.</p>
          ${env.total ? `<div class="lab-bar"><i style="width:${pct}%"></i></div>` : ""}
          ${stagesHtml(env.stage)}
          ${tail(env.log)}
          <div class="lab-acts"><button class="cc-quiet" data-lean="cancel" title="Nothing it made is kept">Stop</button></div>`;
      }
      case "ready": {
        const acts = state.confirming
          ? `<div class="lab-confirm">Take Lean and Mathlib away? Proofs already checked keep their files.
              <div class="lab-acts"><button class="cc-quiet" data-lean="keep">Keep it</button><button class="cc-danger" data-lean="remove">Remove</button></div></div>`
          : `<div class="lab-acts"><button class="cc-quiet" data-lean="confirm">Remove…</button></div>`;
        return `<p><b>Lean ${esc(env.lean)}</b> ${icon("check", "lab-ok")} with Mathlib ${esc(env.mathlib)}
            ${env.commit ? `<span class="lab-faint">(${esc(env.commit.slice(0, 8))})</span>` : ""}</p>
          <p class="lab-faint">${esc(bytes(env.size))} in <code>${esc(home(env.home))}</code></p>${acts}`;
      }
      case "removing":
        return `<p class="lab-busy"><span class="lab-spin"></span> Removing Lean…</p>`;
      case "failed":
        return `<p class="lab-err">${esc(FAILED[env.step] ?? "It")} failed: ${esc(env.why)}</p>
          ${env.log.length ? `<details class="lab-packages"><summary>What it said</summary>${tail(env.log)}</details>` : ""}
          <p class="lab-faint">Nothing it made is kept.</p>
          <div class="lab-acts"><button class="cc-quiet" data-lean="cancel">Put it away</button>
          <button class="cc-primary" data-lean="plan">Try again</button></div>`;
      default:
        return "";
    }
  }

  function render() {
    box.hidden = !shown();
    box.innerHTML = shown() ? `<header>Lean</header>${html()}` : "";
  }

  box.addEventListener("click", (e) => {
    const button = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-lean]"));
    if (!button) return;
    const what = button.dataset.lean;
    if (what === "confirm" || what === "keep") {
      state.confirming = what === "confirm";
      render();
      return;
    }
    state.confirming = false;
    const command = { plan: "leanPlan", create: "leanCreate", cancel: "leanCancel", remove: "leanRemove" }[/** @type {string} */ (what)];
    if (command) host.dispatch({ type: command });
  });

  /** @param {{env: Env}} event */
  function env(event) {
    if (event.env.state !== "ready") state.confirming = false;
    state.env = event.env;
    render();
  }

  /** The open investigation's discipline: the card is for mathematics. @param {string | null} discipline */
  function discipline(discipline) {
    state.mathematics = discipline === "mathematics";
    render();
  }

  render();
  return { env, discipline };
}
