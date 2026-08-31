/* Proton front end.
 *
 * Two kinds of state live here, and keeping them apart is the whole point:
 *
 *   - Core state — the vim mode, the active workspace mode, pending commands.
 *     The page never decides these. It sends a Command and renders the Events
 *     that come back.
 *   - View state — which dock tab is open, which tree row is selected, which
 *     theme is applied. Purely local; the core neither knows nor cares.
 *
 * Anything that becomes a document (a real selection, a real buffer) graduates
 * from the second list to the first.
 */

import { HAS_SHELL, dispatch, onEvent, sendKey, setMode as coreSetMode, sync, wireWindowChrome }
  from "./bridge.js";

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/* ── theme ────────────────────────────────────────────────────────────
   A theme sets sixteen custom properties on <html> and nothing else. No
   rule below the palette block knows which one is active, which is why
   switching is instant and why adding a fifth is a paste, not a port. */
const THEMES = ["tokyo-night", "one-dark", "catppuccin", "nord"];

function setTheme(name) {
  if (!THEMES.includes(name)) return;
  document.documentElement.dataset.theme = name;
  $$("[data-theme-set]").forEach((c) => c.classList.toggle("on", c.dataset.themeSet === name));
  try {
    localStorage.setItem("proton.theme", name);
  } catch {
    /* private window, blocked storage — the theme just doesn't persist */
  }
  requestAnimationFrame(drawPointerEdges); // edges are painted, not tokenised
}

$$("[data-theme-set]").forEach((c) =>
  c.addEventListener("click", () => setTheme(c.dataset.themeSet)),
);

/* ── workspace modes ──────────────────────────────────────────────────
   The mode bar asks the core to switch; the core's ModeChanged event is
   what actually moves the UI. That indirection looks pointless with one
   front end and is the reason a second one is possible. */
const MODES = ["chat", "codex", "design", "modeling", "game", "video", "data"];

function renderMode(mode) {
  $$(".mode").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mode === mode)));
  $$(".workspace").forEach((w) => w.classList.toggle("on", w.dataset.ws === mode));
  closeAll();
}

$$(".mode").forEach((b) => b.addEventListener("click", () => coreSetMode(b.dataset.mode)));
$$("[data-mode-go]").forEach((el) =>
  el.addEventListener("click", () => coreSetMode(el.dataset.modeGo)),
);

/* ── right dock ───────────────────────────────────────────────────────── */
function setDock(panel) {
  const dock = $("#codex-dock");
  if (!dock) return;
  $$(".dock-tab", dock).forEach((t) => t.classList.toggle("on", t.dataset.dockpanel === panel));
  $$(".dock-panel", dock).forEach((p) => {
    const on = p.dataset.panel === panel;
    p.classList.toggle("on", on);
    p.style.display = on ? "flex" : "none";
  });
  requestAnimationFrame(drawPointerEdges);
}

$$("#codex-dock .dock-tab").forEach((t) =>
  t.addEventListener("click", () => setDock(t.dataset.dockpanel)),
);

// Rail and tree shortcuts that jump straight to a dock panel.
$$("[data-dock]").forEach((el) =>
  el.addEventListener("click", () => {
    coreSetMode("codex");
    setDock(el.dataset.dock);
  }),
);

// Dock tab groups elsewhere are view state only.
$$(".dock-tabs").forEach((group) => {
  if (group.closest("#codex-dock")) return;
  $$(".dock-tab", group).forEach((t) =>
    t.addEventListener("click", () => {
      $$(".dock-tab", group).forEach((o) => o.classList.remove("on"));
      t.classList.add("on");
    }),
  );
});

$$(".seg, .builder-rail").forEach((group) => {
  $$("button", group).forEach((b) =>
    b.addEventListener("click", () => {
      $$("button", group).forEach((o) => o.classList.remove("on"));
      b.classList.add("on");
    }),
  );
});

$$(".tree").forEach((tree) => {
  $$(".row", tree).forEach((r) =>
    r.addEventListener("click", () => {
      $$(".row", tree).forEach((o) => o.classList.remove("sel"));
      r.classList.add("sel");
    }),
  );
});

$$(".buffers").forEach((bar) => {
  $$(".buf", bar).forEach((b) =>
    b.addEventListener("click", () => {
      $$(".buf", bar).forEach((o) => o.classList.remove("on"));
      b.classList.add("on");
    }),
  );
});

/* ── overlays ─────────────────────────────────────────────────────────── */
const OVERLAYS = ["telescope", "whichkey", "cmdline", "settings"];

function closeAll() {
  OVERLAYS.forEach((id) => $("#" + id).classList.remove("on"));
  $("#scrim").classList.remove("on");
}

function open(id) {
  const wasOpen = $("#" + id).classList.contains("on");
  closeAll();
  if (wasOpen) return;
  $("#" + id).classList.add("on");
  $("#scrim").classList.add("on");
}

$$("[data-open]").forEach((el) => el.addEventListener("click", () => open(el.dataset.open)));
$$("[data-close]").forEach((el) => el.addEventListener("click", closeAll));
$("#scrim").addEventListener("click", closeAll);

/* ── rendering core events ────────────────────────────────────────────── */

function renderVim(label) {
  const el = $("#sl-mode");
  el.className = "sl sl-mode " + (label === "NORMAL" ? "" : label.toLowerCase().replace("-", ""));
  el.textContent = label;
}

function renderStatus(text) {
  const el = $("#sl-status");
  if (el) el.textContent = text ?? "";
}

onEvent((event) => {
  switch (event.type) {
    case "state":
      renderMode(event.mode);
      renderVim(event.vimLabel);
      break;

    case "modeChanged":
      renderMode(event.mode);
      break;

    case "vimMode":
      renderVim(event.label);
      // Leaving normal mode closes which-key and the pending display.
      if (event.mode !== "Command") $("#cmdline").classList.remove("on");
      renderStatus("");
      break;

    case "pending":
      renderStatus(event.keys);
      if (!event.keys) closeAll();
      break;

    case "leader":
      if (event.prefix === null) closeAll();
      else open("whichkey");
      // ␣ff is the one leader sequence M0 resolves, so Telescope is reachable
      // the way it will be in M2.
      if (event.prefix === "ff") open("telescope");
      break;

    case "cmdline":
      if (event.text === null) {
        closeAll();
      } else {
        open("cmdline");
        const el = $("#cmdline-text");
        if (el) el.textContent = event.text;
      }
      break;

    case "action":
      renderStatus(event.description);
      break;

    case "unsupported":
      // The mode has no meaning for this verb, and says so. Silently doing
      // nothing is the failure users cannot debug.
      renderStatus(`${event.description} — not in ${event.mode}`);
      break;

    case "input":
      renderStatus(`insert ${JSON.stringify(event.text)}`);
      break;
  }
});

/* ── keys ─────────────────────────────────────────────────────────────
   Two layers, and the split matters. Shell chords (⌘K, ⌘,, ⌃1-7) are the
   window's own bindings and never reach the core. Everything else is a
   keystroke the core interprets — including the digits, which are vim
   counts, not mode switches. */
document.addEventListener("keydown", (e) => {
  const meta = e.metaKey || e.ctrlKey;

  if (meta && /^[1-7]$/.test(e.key)) {
    e.preventDefault();
    coreSetMode(MODES[Number(e.key) - 1]);
    return;
  }

  if (meta && e.key.toLowerCase() === "k") {
    e.preventDefault();
    open("telescope");
    return;
  }

  if (meta && e.key === ",") {
    e.preventDefault();
    open("settings");
    return;
  }

  if (meta) return; // leave the rest of the browser/OS chords alone

  // Overlays are view state, so Escape closes them here as well as unwinding
  // the modal state in the core.
  if (e.key === "Escape") closeAll();

  // Space and the vim keys must not scroll or type into the page.
  if (e.key === " " || e.key.length === 1) e.preventDefault();

  sendKey(e);
});

/* ── Code Visualizer: a pointer is an edge, never an inline expansion ──
   Every pointer slot carries data-ptr; every heap block carries data-addr.
   The edge is measured from the live boxes, so it survives any relayout. */
function drawPointerEdges() {
  const canvas = $("#cv-canvas");
  const svg = $("#cv-edges");
  if (!canvas || !svg) return;
  if (!canvas.offsetParent) return; // dock panel is hidden — nothing to measure

  [...svg.querySelectorAll("path")].forEach((p) => p.remove());
  const origin = canvas.getBoundingClientRect();

  $$("[data-ptr]", canvas).forEach((slot) => {
    const target = $(`[data-addr="${slot.dataset.ptr}"]`, canvas);
    if (!target) return;

    const a = slot.getBoundingClientRect();
    const b = target.getBoundingClientRect();
    const dx = canvas.scrollLeft - origin.left;
    const dy = canvas.scrollTop - origin.top;
    const y1 = a.top + a.height / 2 + dy;
    const xTo = b.left + dx - 5;
    const yTo = b.top + 11 + dy;

    // Every arrow lands on the target's left edge. A pointer that already sits
    // inside the heap column — the next link in a chain — leaves to the *left*
    // and drops down the routing lane, so it never crosses the block it points
    // at. A pointer from the stack simply reaches across.
    const sameColumn = a.left + dx > xTo;
    let d;
    if (sameColumn) {
      const lane = xTo - 15;
      const x1 = a.left + dx - 5;
      d = `M${x1} ${y1} C ${lane} ${y1}, ${lane} ${yTo}, ${xTo} ${yTo}`;
    } else {
      const x1 = a.right + dx;
      const bend = (xTo - x1) * 0.55;
      d = `M${x1} ${y1} C ${x1 + bend} ${y1}, ${xTo - bend} ${yTo}, ${xTo} ${yTo}`;
    }

    const dangling = slot.classList.contains("dangling");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke-width", "1.6");
    path.setAttribute("stroke", dangling ? token("--err") : token("--info"));
    path.setAttribute("marker-end", dangling ? "url(#ahr)" : "url(#ah)");
    if (dangling) path.setAttribute("stroke-dasharray", "4 3");
    svg.appendChild(path);
  });
}

new ResizeObserver(drawPointerEdges).observe(document.body);
window.addEventListener("resize", drawPointerEdges);
$("#cv-canvas")?.addEventListener("scroll", drawPointerEdges);

/* ── ambient life: the trace scrubber creeps forward ───────────────────
   Placeholder until proton-trace streams real steps in M4. */
const scrub = $(".cv-scrub .bar");
if (scrub) {
  let at = 38;
  setInterval(() => {
    at = at >= 92 ? 12 : at + 0.35;
    scrub.querySelector("i").style.width = at + "%";
    scrub.querySelector("u").style.left = at + "%";
  }, 220);
}

/* ── boot ─────────────────────────────────────────────────────────────── */

try {
  const saved = localStorage.getItem("proton.theme");
  if (saved) setTheme(saved);
} catch {
  /* nothing stored, or storage is blocked — the default theme stands */
}

wireWindowChrome();
document.documentElement.classList.toggle("shell", HAS_SHELL);
sync();
drawPointerEdges();

// Exposed for the browser preview's console and for driving screenshots.
Object.assign(window, { setTheme, setMode: coreSetMode, setDock, open, closeAll, dispatch, drawPointerEdges });
