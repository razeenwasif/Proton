/* TeX for the LaTeX mode: Proton's own TinyTeX — a TeX Live made small —
 * with latexmk, and texlab for completion, in ~/.proton/tex.
 *
 * Installed with the person's say-so twice, as Lean is: once to plan it
 * (GitHub is asked for TinyTeX's and texlab's newest releases and what they
 * cost — exact sizes said as such, estimates as estimates), and once to
 * install exactly that. Each step is the core's; this draws where it stands
 * in the PDF tab, and asks for the next only when a button says so. */

import { escapeHtml as esc, icon } from "./html.js";
import { bytes } from "./lab.js";

/** @typedef {import("./bindings/TexEnv").TexEnv} Env */
/** @typedef {import("./bindings/TexPlan").TexPlan} Plan */
/** @typedef {import("./bindings/TexBundle").TexBundle} Bundle */

/** An install's stages, in order, as the core names them. */
export const STAGES = /** @type {[string, string][]} */ ([
  ["tinytex", "TinyTeX"],
  ["latexmk", "latexmk"],
  ["texlab", "texlab, for completion"],
  ["checking", "A first page"],
  ["finishing", "Tidying up"],
]);

/** What failed, in words. */
const FAILED = /** @type {Record<string, string>} */ ({
  plan: "Planning", tinytex: "Installing TinyTeX", latexmk: "Installing latexmk", texlab: "Installing texlab", checking: "The first page", finishing: "Finishing",
});

/** The bundles offered, as the card says them. */
export const BUNDLES = /** @type {[Bundle, string, string][]} */ ([
  ["full", "TinyTeX — about 210 MB", "Most articles, theses and Beamer decks compile without asking for more."],
  ["small", "TinyTeX-1 — about 67 MB", "The common core: more documents stop to ask for a package the first time."],
]);

/** A path, with the home folder as `~`. @param {string} path */
const home = (path) => String(path ?? "").replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");

/** A size, said as exact or as about. @param {number} n @param {boolean} exact */
const size = (n, exact) => (exact ? bytes(n) : `about ${bytes(n)}`);

/** @param {string[]} log */
const tail = (log) => (log.length ? `<pre class="lab-log">${esc(log.slice(-8).join("\n"))}</pre>` : "");

/** @param {Plan} plan */
function planHtml(plan) {
  const rows = plan.downloads.map((d) => `<tr><td>${esc(d.what)}<br><span class="lab-faint">${esc(d.host)}</span></td>` +
    `<td>${esc(size(d.bytes, d.exact))}</td><td>${d.verified ? icon("check", "lab-ok") : ""}</td></tr>`).join("");
  const estimated = plan.downloads.some((d) => !d.exact);
  const room = plan.fits ? `${esc(bytes(plan.free))} is free.` : `<span class="lab-err">There is not room for it: ${esc(bytes(plan.free))} is free.</span>`;
  return `<div class="lab-step"><b>Then, if you agree.</b> ${esc(plan.bundle === "small" ? "TinyTeX-1" : "TinyTeX")} ${esc(plan.tinytex)}, with latexmk,
      and texlab ${esc(plan.texlab)}: <b>${esc(size(plan.download, !estimated))}</b> to download, taking up to ${esc(bytes(plan.disk))} while it is
      installed and keeping about ${esc(bytes(plan.kept))}, in <code>${esc(home(plan.home))}</code>. ${room}</div>
    <details class="lab-packages" open><summary>What it downloads</summary><table>${rows}</table>
      <p class="lab-faint">${icon("check", "lab-ok")} checked against the SHA-256 GitHub publishes.${estimated ? " latexmk's size is an estimate: TeX's package manager fetches it." : ""}</p></details>
    <div class="lab-acts"><button class="cc-quiet" data-tex-env="cancel">Not now</button>
      <button class="cc-primary" data-tex-env="create"${plan.fits ? "" : " disabled"}>Install</button></div>`;
}

/** The stages of an install, those done ticked. @param {string} stage */
function stagesHtml(stage) {
  const at = STAGES.findIndex(([id]) => id === stage);
  return `<ol class="lean-stages">${STAGES.map(([id, name], n) =>
    `<li data-stage="${id}" class="${n < at ? "done" : n === at ? "on" : ""}">${n < at ? icon("check", "lab-ok") : ""}${esc(name)}</li>`).join("")}</ol>`;
}

/** The card, for every state but installed. @param {Env} env @param {Bundle} bundle the one chosen */
export function envHtml(env, bundle) {
  switch (env.state) {
    case "unknown":
      return `<p class="lab-faint">Looking for TeX…</p>`;
    case "missing": {
      const about = `<p>To typeset, Proton needs TeX. It installs its own — TinyTeX, a TeX Live made small, with pdfLaTeX, XeLaTeX, LuaLaTeX,
        BibTeX, biber and latexmk, and texlab for completion — in <code>${esc(home(env.home))}</code>, apart from any TeX of yours.
        Documents are typeset in the sandbox: no network, and nothing written outside their build folder.</p>`;
      if (env.needs.length) {
        return `${about}<p>It cannot be installed here yet. It needs:</p><ul class="lean-needs">${env.needs.map((n) => `<li>${esc(n)}</li>`).join("")}</ul>`;
      }
      const choices = BUNDLES.map(([value, name, note]) => `<label class="tex-bundle"><input type="radio" name="tex-bundle" value="${value}"${value === bundle ? " checked" : ""} data-tex-bundle>` +
        `<span><b>${esc(name)}</b>${value === "full" ? " <span class=\"lab-faint\">(recommended)</span>" : ""}<br><span class="lab-faint">${esc(note)}</span></span></label>`).join("");
      return `${about}<div class="tex-bundles">${choices}</div>
        <div class="lab-step"><b>First, a plan.</b> Proton asks GitHub for TinyTeX's and texlab's newest releases and what they cost — a few
          kilobytes. Nothing is downloaded yet. ${esc(bytes(env.free))} is free here.</div>
        <div class="lab-acts"><button class="cc-primary" data-tex-env="plan">Plan TeX</button></div>`;
    }
    case "planning":
      return `<p class="lab-busy"><span class="lab-spin"></span> Asking GitHub for TinyTeX's and texlab's newest releases…</p>
        <div class="lab-acts"><button class="cc-quiet" data-tex-env="cancel">Stop</button></div>`;
    case "planned":
      return planHtml(env.plan);
    case "making": {
      const name = STAGES.find(([id]) => id === env.stage)?.[1] ?? env.stage;
      const pct = env.total ? Math.min(100, Math.round((env.done / env.total) * 100)) : 0;
      const amount = env.total ? ` — ${esc(bytes(env.done))} of ${esc(bytes(env.total))}` : "";
      return `<p class="lab-busy"><span class="lab-spin"></span> Installing TeX: ${esc(name)}${amount}.</p>
        ${env.total ? `<div class="lab-bar"><i style="width:${pct}%"></i></div>` : ""}
        ${stagesHtml(env.stage)}${tail(env.log)}
        <div class="lab-acts"><button class="cc-quiet" data-tex-env="cancel" title="Nothing it installed is kept">Stop</button></div>`;
    }
    case "removing":
      return `<p class="lab-busy"><span class="lab-spin"></span> Removing TeX…</p>`;
    case "failed":
      return `<p class="lab-err">${esc(FAILED[env.step] ?? "It")} failed: ${esc(env.why)}</p>
        ${env.log.length ? `<details class="lab-packages"><summary>What it said</summary>${tail(env.log)}</details>` : ""}
        <p class="lab-faint">Nothing it installed is kept.</p>
        <div class="lab-acts"><button class="cc-quiet" data-tex-env="cancel">Put it away</button>
        <button class="cc-primary" data-tex-env="plan">Try again</button></div>`;
    default:
      return "";
  }
}

/** What an installed TeX is, in a line, and how to take it away. @param {Extract<Env, {state: "ready"}>} env @param {boolean} confirming */
export function readyHtml(env, confirming) {
  const acts = confirming
    ? `<span class="tex-confirm">Remove TeX, and every project's builds? <button class="cc-quiet" data-tex-env="keep">Keep it</button>` +
      `<button class="cc-danger" data-tex-env="remove">Remove</button></span>`
    : `<button class="st-link" data-tex-env="confirm">Remove…</button>`;
  return `<p class="tex-ready lab-faint">${icon("check", "lab-ok")} ${esc(env.bundle === "small" ? "TinyTeX-1" : "TinyTeX")} ${esc(env.tinytex)} · texlab ${esc(env.texlab)} · ` +
    `${esc(bytes(env.size))} in <code>${esc(home(env.home))}</code> ${acts}</p>`;
}

/** The command a card's button asks for, if it asks the core for one.
 *  @param {string} what @param {Bundle} bundle */
export function commandOf(what, bundle) {
  switch (what) {
    case "plan": return { type: "texPlan", bundle };
    case "create": return { type: "texCreate" };
    case "cancel": return { type: "texCancel" };
    case "remove": return { type: "texRemove" };
    default: return null;
  }
}
