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
import { highlight } from "./highlight.js";

/** @typedef {import("./bindings/LabEnv").LabEnv} Env */
/** @typedef {import("./bindings/LabPlan").LabPlan} Plan */
/** @typedef {import("./bindings/ExperimentRow").ExperimentRow} Experiment */
/** @typedef {import("./bindings/LabRun").LabRun} RunRow */
/** @typedef {import("./bindings/LabRunDetail").LabRunDetail} RunDetail */
/** @typedef {import("./bindings/LabSandbox").LabSandbox} Sandbox */
/** @typedef {{slug: string, experiments: Experiment[], runs: RunRow[], sandbox: Sandbox}} LabEvent */
/** @typedef {{slug: string, detail: RunDetail | null}} RunEvent */

const SEATS = /** @type {Record<string, string>} */ ({
  claude: "Claude Code", codex: "Codex", antigravity: "Antigravity", gemini: "Gemini", scientist: "You",
});

/** How a run ended, in words. */
const ENDED = /** @type {Record<string, string>} */ ({
  running: "Running", done: "Done", failed: "Failed", timedOut: "Out of time", stopped: "Stopped",
});

/** @param {number} s */
const seconds = (s) => (s < 60 ? `${s.toFixed(1)} s` : `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`);

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
  const foundWork = /** @type {HTMLElement | null} */ (host.root.querySelector("[data-lab-work]"));
  if (!found || !foundWork) return null;
  const box = found;
  const work = foundWork;
  /** @type {{env: Env | null, confirming: boolean, lab: LabEvent | null, run: RunDetail | null, open: Set<string>}} */
  const state = { env: null, confirming: false, lab: null, run: null, open: new Set() };

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

  /* ── what waits, and what has run ──────────────────────────────── */

  /** Python, coloured, the first lines shown until it is opened.
   *  @param {string} code @param {string} id */
  function codeHtml(code, id) {
    const lines = code.replace(/\s+$/, "").split("\n");
    const open = state.open.has(id) || lines.length <= 14;
    const shown = open ? lines : lines.slice(0, 12);
    return `<div class="md-code lab-code"><div class="md-code-head"><span>python · ${lines.length} lines</span>` +
      `<button class="md-copy" data-copy title="Copy">Copy</button></div>` +
      `<pre><code data-lang="python">${highlight(shown.join("\n"), "python")}</code></pre>` +
      (open ? "" : `<button class="lab-more" data-lab-open="${esc(id)}">${lines.length - 12} more lines</button>`) + `</div>`;
  }

  /** @param {Experiment} e @param {Sandbox} sandbox */
  function proposalHtml(e, sandbox) {
    const ready = state.env?.state === "ready";
    const by = SEATS[e.proposer.seat] ?? e.proposer.seat;
    const run = sandbox === "none"
      ? `<button class="cc-danger" data-lab-approve="${esc(e.id)}" data-unsandboxed="1"${ready ? "" : " disabled"}
           title="This machine has no sandbox the Lab trusts: it could do anything you can">Run without a sandbox</button>`
      : `<button class="cc-primary" data-lab-approve="${esc(e.id)}"${ready ? "" : " disabled"}
           title="${ready ? "No network, writing only its own folder" : "Make the Lab's Python first"}">Run in the sandbox</button>`;
    return `<article class="lab-proposal" data-experiment="${esc(e.id)}">
      <header><b>${esc(e.id)}</b>${e.hypothesis ? ` <span class="lab-faint">for</span> ${esc(e.hypothesis)}` : ""}
        <span class="lab-faint">· ${esc(by)}${e.proposer.model ? ` · ${esc(e.proposer.model)}` : ""}</span></header>
      ${e.purpose.trim() ? `<p>${esc(e.purpose)}</p>` : ""}
      ${e.expectation.trim() ? `<p class="lab-faint">Expects: ${esc(e.expectation)}</p>` : ""}
      ${codeHtml(e.code, e.id)}
      <div class="lab-acts"><button class="cc-quiet" data-lab-reject="${esc(e.id)}">Turn down</button>${run}</div>
    </article>`;
  }

  /** @param {RunRow} r */
  function runRow(r) {
    const time = r.seconds != null ? seconds(r.seconds) : "";
    const exit = r.state === "failed" && r.exit != null ? ` · exit ${r.exit}` : "";
    const on = state.run?.row.id === r.id;
    return `<button class="lab-run${on ? " on" : ""}" data-lab-run="${esc(r.id)}" data-state="${esc(r.state)}">` +
      `<i class="rs-dot"></i><b>${esc(r.id)}</b><span>${esc(r.experiment ?? "")}${r.hypothesis ? ` · ${esc(r.hypothesis)}` : ""}</span>` +
      `<span class="lab-faint">${esc(ENDED[r.state] ?? r.state)}${exit}${time ? ` · ${esc(time)}` : ""}</span></button>`;
  }

  /** @param {RunDetail} d */
  function detailHtml(d) {
    const r = d.row;
    const record = /** @type {any} */ (d.record);
    const fence = r.sandbox === "none"
      ? `<span class="lab-err">without a sandbox</span>`
      : `in ${r.sandbox === "seatbelt" ? "Seatbelt" : "bubblewrap"}: no network, writing only its own folder`;
    const metrics = Object.entries(d.metrics ?? {}).map(([k, v]) =>
      `<tr><td>${esc(k)}</td><td>${esc(String(/** @type {any} */ (v)?.value ?? ""))}</td><td class="lab-faint">${esc(/** @type {any} */ (v)?.unit ?? "")}</td></tr>`).join("");
    const saved = [...d.figures, ...d.scenes].map((f) => `<li><code>${esc(f)}</code></li>`).join("");
    const short = (/** @type {string | undefined} */ h) => (h ? `<code title="${esc(h)}">${esc(h.slice(0, 12))}</code>` : "—");
    const approved = record?.about?.approved;
    return `<div class="lab-detail">
      <header><button class="rs-back" data-lab-run="">${icon("chev")}Runs</button><span class="grow"></span>
        ${r.state === "running" ? `<button class="cc-quiet" data-lab-stop="${esc(r.id)}">Stop</button>` : ""}</header>
      <h3>${esc(r.id)} <span class="lab-faint">— ${esc(r.experiment ?? "")}${r.hypothesis ? `, testing ${esc(r.hypothesis)}` : ""}</span></h3>
      <p>${esc(ENDED[r.state] ?? r.state)}${r.exit != null && r.state !== "done" ? `, exit ${r.exit}` : ""}${r.seconds != null ? ` after ${esc(seconds(r.seconds))}` : ""}, ${fence}.</p>
      ${metrics ? `<table class="lab-metrics">${metrics}</table>` : ""}
      ${saved ? `<div><span class="lab-faint">Saved</span><ul class="lab-saved">${saved}</ul></div>` : ""}
      ${d.stdout.trim() ? `<div><span class="lab-faint">Output${d.stdoutCut ? " (its end)" : ""}</span><pre class="lab-log">${esc(d.stdout)}</pre></div>` : ""}
      ${d.stderr.trim() ? `<div><span class="lab-faint">Errors${d.stderrCut ? " (their end)" : ""}</span><pre class="lab-log err">${esc(d.stderr)}</pre></div>` : ""}
      <details class="lab-packages"><summary>Provenance</summary><table>
        <tr><td>Code</td><td>main.py ${short(record?.code?.sha256)}</td></tr>
        <tr><td>Python</td><td><code>${esc(home(String(record?.python ?? "")))}</code></td></tr>
        <tr><td>Environment</td><td>${record?.environment?.lock ? `lock ${short(record.environment.lock.sha256)}` : "—"}</td></tr>
        <tr><td>Approved</td><td>${approved ? `by ${approved.by === "scientist" ? "you" : esc(approved.by)}, ${esc(new Date(approved.at * 1000).toLocaleString())}` : "—"}</td></tr>
        <tr><td>Limits</td><td>${esc(String(record?.limits?.seconds ?? ""))} s</td></tr>
      </table></details>
    </div>`;
  }

  function renderWork() {
    const lab = state.lab;
    if (!lab) {
      work.innerHTML = "";
      return;
    }
    if (state.run) {
      work.innerHTML = detailHtml(state.run);
      return;
    }
    const waiting = lab.experiments.filter((e) => e.status === "proposed");
    const turnedDown = lab.experiments.filter((e) => e.status === "rejected").length;
    const fence = lab.sandbox === "none"
      ? `<p class="lab-err">This machine has no sandbox the Lab trusts. Code runs only if you say so, and can do anything you can.</p>`
      : "";
    const proposals = waiting.length
      ? `<header>Waiting for you <span class="rs-count">${waiting.length}</span></header>${fence}${waiting.map((e) => proposalHtml(e, lab.sandbox)).join("")}`
      : "";
    const runs = lab.runs.length
      ? `<header>Runs</header><div class="lab-runs">${lab.runs.map(runRow).join("")}</div>`
      : "";
    const empty = !waiting.length && !lab.runs.length
      ? `<p class="lab-faint">Nothing has run yet. Code an investigation proposes waits here for you to read it, and runs with no network and nowhere to write but its own folder.</p>`
      : "";
    work.innerHTML = proposals + runs + empty + (turnedDown ? `<p class="lab-faint">${turnedDown} turned down.</p>` : "");
  }

  work.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const more = /** @type {HTMLElement | null} */ (target.closest("[data-lab-open]"));
    if (more) {
      state.open.add(more.dataset.labOpen ?? "");
      return renderWork();
    }
    const approve = /** @type {HTMLButtonElement | null} */ (target.closest("[data-lab-approve]"));
    if (approve && !approve.disabled) {
      host.dispatch({ type: "researchApprove", experiment: approve.dataset.labApprove, unsandboxed: approve.dataset.unsandboxed === "1" });
      return;
    }
    const reject = /** @type {HTMLElement | null} */ (target.closest("[data-lab-reject]"));
    if (reject) return void host.dispatch({ type: "researchReject", experiment: reject.dataset.labReject });
    const stop = /** @type {HTMLElement | null} */ (target.closest("[data-lab-stop]"));
    if (stop) return void host.dispatch({ type: "researchStopRun", run: stop.dataset.labStop });
    const run = /** @type {HTMLElement | null} */ (target.closest("[data-lab-run]"));
    if (run) {
      const id = run.dataset.labRun || null;
      if (!id) state.run = null;
      host.dispatch({ type: "researchShowRun", run: id });
      if (!id) renderWork();
      return;
    }
    const copy = /** @type {HTMLElement | null} */ (target.closest("[data-copy]"));
    if (copy) {
      const code = copy.closest(".md-code")?.querySelector("code")?.textContent ?? "";
      navigator.clipboard?.writeText(code).catch(() => {});
    }
  });

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
    // Whether a proposal can be run depends on it.
    renderWork();
  }

  /** @param {LabEvent} event */
  function experiments(event) {
    if (state.lab && state.lab.slug !== event.slug) state.run = null;
    state.lab = event;
    renderWork();
  }

  /** @param {RunEvent} event */
  function run(event) {
    if (state.lab && event.slug !== state.lab.slug) return;
    state.run = event.detail;
    renderWork();
  }

  render();
  return { env, experiments, run };
}
