/* The tutor, in a notebook: a chat with Claude Code, Codex, Gemini or
 * Antigravity, answering from the notebook's ticked sources and citing them.
 *
 * An answer's citations — `[[S07 p. 12]]` as the tutor writes them — are
 * drawn as numbered chips: over one, what the source says there; clicked, the
 * source opens in the reader at that place. A citation of nothing in the
 * notebook is drawn struck through. What the tutor does on its way to an
 * answer — its searches, what it read — is drawn as it does it. The core
 * resolves every citation and keeps every finished turn; this draws them. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";

/** @typedef {import("./bindings/StudyTurn").StudyTurn} Turn */
/** @typedef {import("./bindings/StudyLive").StudyLive} Live */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {import("./bindings/StudyTutorSettings").StudyTutorSettings} Settings */
/** @typedef {import("./bindings/StudyChatSummary").StudyChatSummary} Summary */
/** @typedef {import("./bindings/ClaudeCapabilities").ClaudeCapabilities} Capabilities */
/** @typedef {import("./bindings/StudyObjective").StudyObjective} Objective */
/** @typedef {{id: string, title: string, settings: Settings, turns: Turn[], live: Live | null, busy: boolean,
 *   error: string | null, grounding: string | null, chats: Summary[], lesson: Objective | null}} ChatEvent */

/** The engines a tutor can be, in the order offered. */
export const ENGINES = /** @type {[string, string][]} */ ([
  ["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini"], ["antigravity", "Antigravity"],
]);
const ENGINE = Object.fromEntries(ENGINES);

const STYLES = /** @type {[string, string, string][]} */ ([
  ["default", "Default", "Answers, each claim cited"],
  ["guide", "Learning guide", "Asks before it tells, and leads in steps"],
  ["custom", "Your own", "A role you write"],
]);
const LENGTHS = /** @type {[string, string][]} */ ([["shorter", "Shorter"], ["default", "Default"], ["longer", "Longer"]]);

const STEP_ICON = /** @type {Record<string, string>} */ ({ list: "book", search: "search", read: "file", view: "image", said: "chat", other: "bolt" });

/** Where a citation points, in words. @param {Citation} c */
export function place(c) {
  if (!c.place) return "the whole source";
  const { from, to } = c.place;
  if (c.units === "pages") return from === to ? `page ${from}` : `pages ${from}–${to}`;
  if (c.units === "seconds") {
    const clock = (/** @type {number} */ s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
    return `${clock(from)}–${clock(to)}`;
  }
  return from === to ? `line ${from}` : `lines ${from}–${to}`;
}

/** An answer's Markdown, its citations drawn as numbered chips.
 *  @param {string} text @param {Citation[]} citations */
export function answerHtml(text, citations) {
  // Each citation becomes a mark the renderer passes through untouched,
  // then a chip: the renderer never sees the brackets.
  const marked = String(text).replace(/\[\[([^[\]\n]+)\]\]/g, (whole, inner) => {
    const ns = String(inner).split(";").map((p) => citations.find((c) => c.raw === p.trim())?.n).filter((n) => n !== undefined);
    return ns.length ? ns.map((n) => `${n}`).join("") : whole;
  });
  return markdown(marked).replace(/(\d+)/g, (_, n) => {
    const c = citations.find((x) => x.n === Number(n));
    if (!c) return "";
    const tip = c.resolved ? `${c.source} · ${c.title} — ${place(c)}` : `${c.raw}: not in this notebook`;
    return `<button class="st-cite${c.resolved ? "" : " bad"}" data-study-cite="${c.n}" aria-label="${esc(tip)}">${c.n}</button>`;
  });
}

/**
 * @param {{dispatch: (command: any) => unknown, ws: HTMLElement, readOnly: () => boolean,
 *   show: (id: string, at: {from: number, to: number} | null) => void, lookup?: (key: string) => Citation[]}} host
 */
export function createChat(host) {
  /** @type {{chat: ChatEvent | null, live: Live | null, caps: Map<string, Capabilities>, menu: string | null, draft: Settings | null}} */
  const state = { chat: null, live: null, caps: new Map(), menu: null, draft: null };

  // One popover for citations, kept in the tab.
  const tip = document.createElement("div");
  tip.className = "st-tip";
  tip.hidden = true;
  host.ws.append(tip);

  /** @type {HTMLElement | null} */
  let root = null;

  const q = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (root?.querySelector(sel) ?? null);

  /** Every citation shown, by number — finished turns' and the live one's, keyed by turn. */
  function citationsOf(/** @type {string} */ key) {
    if (key === "live") return state.live?.citations ?? [];
    if (/^\d+$/.test(key)) return state.chat?.turns[Number(key)]?.citations ?? [];
    return host.lookup?.(key) ?? [];
  }

  /* ── drawing ───────────────────────────────────────────────────── */

  function engineName(/** @type {string} */ e) {
    return ENGINE[e || "claude"] ?? e;
  }

  function headHtml() {
    const chat = state.chat;
    if (!chat) return "";
    const s = chat.settings;
    const who = `${engineName(s.engine)}${s.model ? ` · ${s.model}` : ""}`;
    const how = STYLES.find(([v]) => v === s.style)?.[1] ?? "";
    const passages = chat.grounding === "passages"
      ? `<span class="st-faint" title="It is given the passages found for each question, rather than searching the sources itself">· passages</span>` : "";
    return `<div class="st-chat-head">
        <button class="st-chip-btn" data-study-menu="chats" title="This notebook's chats">${icon("chat")}<span>${esc(chat.turns.length ? chat.title : "New chat")}</span>${icon("chev")}</button>
        <span class="grow"></span>
        <button class="st-chip-btn" data-study-menu="tutor" title="Who answers, and how">${icon("spark")}<span>${esc(who)}${style(how)}</span></button>${passages}
        <button class="st-chip-btn" data-study-chat-new title="A new chat">${icon("plus")}</button>
      </div>${lessonHtml()}`;
  }

  /** What a lesson is on, and the check that ends it. */
  function lessonHtml() {
    const lesson = state.chat?.lesson;
    if (!lesson) return "";
    const text = markdown(lesson.text).replace(/^<p>|<\/p>\s*$/g, "");
    return `<div class="st-lesson">${icon("target")}<span>A lesson on <b>${esc(lesson.id)}</b>: ${text}</span>` +
      (host.readOnly() ? "" : `<button class="cc-quiet" data-study-lesson-check="${esc(lesson.id)}" title="Three questions on it, made from the sources">Check me</button>`) + `</div>`;
  }

  /** @param {string} how */
  function style(how) {
    return how && how !== "Default" ? ` · ${esc(how)}` : "";
  }

  /** @param {Turn} turn @param {number} i */
  function turnHtml(turn, i) {
    const body = turn.error && !turn.answer
      ? `<div class="st-turn-error">${icon("d-error")} ${esc(turn.error)}</div>`
      : `<div class="st-answer md" data-turn="${i}">${answerHtml(turn.answer, turn.citations)}</div>`;
    const keep = host.readOnly() || !turn.answer ? "" : `<button class="st-link" data-study-keep="${i}" title="Keep this answer as a note">Keep as a note</button>`;
    const by = [engineName(turn.engine), turn.model].filter(Boolean).join(" · ");
    // The notes it wrote, kept in the notebook: each opens where notes do.
    const notes = (turn.notes ?? []).map((n) => `<button class="st-made-note" data-study-show-note="${esc(n.id)}" title="Open the note">` +
      `${icon("file-doc")}<span class="st-made-note-title">${esc(n.title)}</span>` +
      `<span class="st-faint">${n.updated ? "written again" : "kept as a note"} · ${esc(n.id)}</span></button>`).join("");
    return `<div class="st-turn"><div class="st-asked">${esc(turn.asked)}</div>${turn.answer || !notes ? body : ""}${notes}` +
      `<div class="st-turn-foot"><span class="st-faint">${esc(by)}</span>${keep}</div></div>`;
  }

  function liveHtml() {
    const live = state.live;
    const chat = state.chat;
    if (!chat?.busy && !live) return "";
    const asked = live?.asked ?? "";
    const steps = (live?.steps ?? []).map((s) =>
      `<li class="${s.done ? "done" : ""}">${s.done ? icon(STEP_ICON[s.kind] ?? "bolt") : '<span class="lab-spin"></span>'}<span>${esc(s.text)}</span></li>`).join("");
    const waiting = !live?.answer
      ? `<div class="st-thinking"><span class="lab-spin"></span>${live?.thinking ? "Thinking" : live?.steps.length ? "Reading the sources" : "Starting"}…</div>` : "";
    return `${asked ? `<div class="st-asked">${esc(asked)}</div>` : ""}${steps ? `<ul class="st-steps">${steps}</ul>` : ""}${waiting}` +
      (live?.answer ? `<div class="st-answer md" data-turn="live">${answerHtml(live.answer, live.citations)}</div>` : "");
  }

  function logHtml() {
    const chat = state.chat;
    if (!chat) return "";
    const turns = chat.turns.map(turnHtml).join("");
    const empty = !chat.turns.length && !chat.busy
      ? `<div class="st-chat-empty"><h2>Ask about your sources</h2><p>Every answer is drawn from the sources you tick, and cites them: hover a number to see the passage, click it to open the source there.</p></div>` : "";
    const error = chat.error ? `<div class="st-turn-error">${icon("d-error")} ${esc(chat.error)}</div>` : "";
    return `${empty}${turns}<div class="st-turn st-live" data-study-live>${liveHtml()}</div>${error}`;
  }

  function composerHtml() {
    const ro = host.readOnly();
    return `<form class="st-composer cc-composer" data-study-ask-form novalidate>
        <textarea class="cc-input" name="text" rows="1" spellcheck="true" aria-label="Ask the tutor"
          placeholder="${ro ? "This notebook is read only here" : "Ask about your sources — ⏎ to send, ⇧⏎ for a new line"}"${ro ? " disabled" : ""}></textarea>
        <div class="cc-foot"><span class="st-faint" data-study-busy></span><span class="grow"></span>
          <button type="button" class="cc-quiet" data-study-stop hidden>Stop</button>
          <button type="submit" class="cc-send" aria-label="Send"${ro ? " disabled" : ""}>${icon("arrow-up")}</button></div>
      </form>`;
  }

  function menuHtml() {
    const chat = state.chat;
    if (!chat || !state.menu) return "";
    if (state.menu === "chats") {
      const rows = chat.chats.map((c) => `<button class="cc-pop-row${c.id === chat.id ? " on" : ""}" data-study-chat="${esc(c.id)}">` +
        `<b>${esc(c.title)}</b><span>${c.turns} ${c.turns === 1 ? "question" : "questions"} · ${new Date(c.updated * 1000).toLocaleDateString()}</span></button>`).join("");
      return `<div class="st-menu" data-study-menu-box><div class="cc-pop-head">Chats</div>${rows || '<p class="st-faint st-pad">None yet.</p>'}` +
        `<button class="cc-pop-row" data-study-chat-new>${icon("plus")}<b>A new chat</b></button></div>`;
    }
    const d = state.draft ?? chat.settings;
    const engine = d.engine || "claude";
    const engines = ENGINES.map(([value, name]) => {
      const caps = state.caps.get(value);
      const missing = caps?.missing;
      return `<label class="st-opt${missing ? " off" : ""}" title="${esc(missing ?? "")}"><input type="radio" name="engine" value="${value}"${value === engine ? " checked" : ""}${missing ? " disabled" : ""}>` +
        `<span>${esc(name)}${missing ? ` <i>— ${esc(missing.split(".")[0])}</i>` : ""}</span></label>`;
    }).join("");
    const models = state.caps.get(engine)?.models ?? [];
    const modelField = models.length
      ? `<select name="model"><option value="">Its default</option>${models.filter((m) => m.value !== "default").map((m) =>
        `<option value="${esc(m.value)}"${m.value === d.model ? " selected" : ""}>${esc(m.name)}</option>`).join("")}</select>`
      : `<input name="model" value="${esc(d.model)}" placeholder="Its default" autocomplete="off" spellcheck="false">`;
    const styles = STYLES.map(([value, name, note]) => `<label class="st-opt"><input type="radio" name="style" value="${value}"${value === d.style ? " checked" : ""}>` +
      `<span>${esc(name)} <i>— ${esc(note)}</i></span></label>`).join("");
    const lengths = LENGTHS.map(([value, name]) => `<label class="st-seg"><input type="radio" name="length" value="${value}"${value === d.length ? " checked" : ""}><span>${esc(name)}</span></label>`).join("");
    return `<form class="st-menu st-tutor-form" data-study-menu-box data-study-tutor-form novalidate>
        <div class="cc-pop-head">Who answers</div>${engines}
        <label class="rs-field"><span>Model</span>${modelField}</label>
        <div class="cc-pop-head">How</div>${styles}
        <textarea name="custom" rows="4" maxlength="10000" placeholder="Your role for the tutor: “Quiz me like a strict examiner, one question at a time.”"${d.style === "custom" ? "" : " hidden"}>${esc(d.custom)}</textarea>
        <div class="st-segs">${lengths}</div>
        <label class="rs-field"><span>Language</span><input name="language" value="${esc(d.language)}" placeholder="The question's own" autocomplete="off"></label>
        <p class="st-faint">A change starts the chat's next answer afresh, told what was said.</p>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-menu-close>Cancel</button><button type="submit" class="cc-primary">Save</button></div>
      </form>`;
  }

  /** What scrolls: the stage the chat is drawn in. @type {HTMLElement | null} */
  let scroller = null;

  /** Draw the chat into a box: whole the first time, and its parts after,
   *  so what is being typed is never lost. @param {HTMLElement} stage @param {HTMLElement} scrolls */
  function render(stage, scrolls = stage) {
    scroller = scrolls;
    if (!state.chat) {
      stage.innerHTML = `<p class="st-faint st-pad">Opening the chat…</p>`;
      root = null;
      return;
    }
    root = /** @type {HTMLElement | null} */ (stage.querySelector(".st-chat"));
    if (!root) {
      stage.innerHTML = `<div class="st-chat"><div data-study-chat-head></div><div class="st-chat-log" data-study-log></div><div data-study-menu-host></div>${composerHtml()}</div>`;
      root = /** @type {HTMLElement} */ (stage.querySelector(".st-chat"));
    }
    const near = scrolls.scrollHeight - scrolls.scrollTop - scrolls.clientHeight < 80;
    /** @type {HTMLElement} */ (q("[data-study-chat-head]")).innerHTML = headHtml();
    /** @type {HTMLElement} */ (q("[data-study-log]")).innerHTML = logHtml();
    /** @type {HTMLElement} */ (q("[data-study-menu-host]")).innerHTML = menuHtml();
    renderBusy();
    if (near) scrolls.scrollTop = scrolls.scrollHeight;
  }

  function renderBusy() {
    const busy = Boolean(state.chat?.busy);
    const stop = q("[data-study-stop]");
    if (stop) stop.hidden = !busy;
    const send = /** @type {HTMLButtonElement | null} */ (q(".cc-send"));
    if (send) send.disabled = busy || host.readOnly();
  }

  /** Only the answer coming is drawn again, as it comes. */
  function renderLive() {
    const box = q("[data-study-live]");
    const stage = scroller;
    if (!box || !stage) return;
    const near = stage.scrollHeight - stage.scrollTop - stage.clientHeight < 80;
    box.innerHTML = liveHtml();
    renderBusy();
    if (near) stage.scrollTop = stage.scrollHeight;
  }

  /* ── the citations' popover ────────────────────────────────────── */

  /** @param {HTMLElement} chip */
  function showTip(chip) {
    const n = Number(chip.dataset.studyCite);
    const key = /** @type {HTMLElement | null} */ (chip.closest("[data-turn]"))?.dataset.turn ?? "";
    const c = citationsOf(key).find((x) => x.n === n);
    if (!c) return;
    tip.innerHTML = c.resolved
      // A heading in a quote is a line like the rest.
      ? `<header><b>${esc(c.source)}</b> ${esc(c.title)} <span class="st-faint">${esc(place(c))}</span></header><div class="md">${markdown(c.quote.replace(/^#{1,6}\s+/gm, ""))}</div>`
      : `<header><b>${esc(c.raw)}</b></header><p>This citation names nothing in the notebook${c.title ? `: ${esc(c.title)} has no ${esc(place(c))}` : ""}.</p>`;
    tip.hidden = false;
    const box = chip.getBoundingClientRect();
    const frame = host.ws.getBoundingClientRect();
    const left = Math.max(8, Math.min(box.left - frame.left - 20, frame.width - tip.offsetWidth - 8));
    const above = box.top - frame.top - tip.offsetHeight - 8;
    tip.style.left = `${left}px`;
    tip.style.top = `${above > 8 ? above : box.bottom - frame.top + 8}px`;
  }

  host.ws.addEventListener("mouseover", (e) => {
    const chip = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-study-cite]"));
    if (chip) showTip(chip);
    else if (!/** @type {HTMLElement} */ (e.target).closest?.(".st-tip")) tip.hidden = true;
  });
  host.ws.addEventListener("focusin", (e) => {
    const chip = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-study-cite]"));
    if (chip) showTip(chip);
  });

  /* ── what is done here ─────────────────────────────────────────── */

  /** @param {HTMLFormElement} form */
  function ask(form) {
    const input = /** @type {HTMLTextAreaElement} */ (form.querySelector("textarea"));
    const text = input.value.trim();
    if (!text || state.chat?.busy || host.readOnly()) return;
    host.dispatch({ type: "studyAsk", text });
    input.value = "";
    input.style.height = "";
  }

  /** Handle a click in the chat; `true` if it was the chat's. @param {HTMLElement} t */
  function click(t) {
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const chip = at("[data-study-cite]");
    if (chip) {
      const key = /** @type {HTMLElement | null} */ (chip.closest("[data-turn]"))?.dataset.turn ?? "";
      const c = citationsOf(key).find((x) => x.n === Number(chip.dataset.studyCite));
      if (c?.resolved) host.show(c.source, c.place);
      tip.hidden = true;
      return true;
    }
    if (!root?.contains(t)) return false;
    // A note an answer kept, opened where notes are.
    const note = at("[data-study-show-note]");
    if (note) {
      host.dispatch({ type: "studyShowNote", id: note.dataset.studyShowNote });
      return true;
    }
    const menu = at("[data-study-menu]");
    if (menu) {
      const which = menu.dataset.studyMenu ?? null;
      state.menu = state.menu === which ? null : which;
      state.draft = state.chat ? { ...state.chat.settings } : null;
      /** @type {HTMLElement} */ (q("[data-study-menu-host]")).innerHTML = menuHtml();
      return true;
    }
    if (at("[data-study-menu-close]")) {
      state.menu = null;
      /** @type {HTMLElement} */ (q("[data-study-menu-host]")).innerHTML = "";
      return true;
    }
    if (at("[data-study-chat-new]")) {
      state.menu = null;
      host.dispatch({ type: "studyChatNew" });
      return true;
    }
    const check = at("[data-study-lesson-check]");
    if (check) {
      host.dispatch({ type: "studyMake", kind: "quiz", options: { focus: "", ask: "", columns: [], amount: "fewer", level: "medium", deck: null,
        objectives: [check.dataset.studyLessonCheck], source: null, count: 3 } });
      return true;
    }
    const pick = at("[data-study-chat]");
    if (pick) {
      state.menu = null;
      host.dispatch({ type: "studyChatOpen", id: pick.dataset.studyChat });
      return true;
    }
    const keep = at("[data-study-keep]");
    if (keep) {
      host.dispatch({ type: "studySaveAnswer", turn: Number(keep.dataset.studyKeep) });
      return true;
    }
    if (at("[data-study-stop]")) {
      host.dispatch({ type: "studyStopAnswer" });
      return true;
    }
    return true;
  }

  /** A change in the tutor's form: the custom role shown, the models of the engine chosen. @param {HTMLElement} t */
  function change(t) {
    const form = /** @type {HTMLFormElement | null} */ (t.closest("[data-study-tutor-form]"));
    if (!form || !state.draft) return false;
    const data = new FormData(form);
    state.draft = { ...state.draft, engine: String(data.get("engine") ?? state.draft.engine), style: /** @type {any} */ (String(data.get("style") ?? state.draft.style)),
      custom: String(data.get("custom") ?? ""), length: /** @type {any} */ (String(data.get("length") ?? state.draft.length)),
      language: String(data.get("language") ?? ""), model: t.matches('[name="engine"]') ? "" : String(data.get("model") ?? "") };
    if (t.matches('[name="engine"]') || t.matches('[name="style"]')) {
      /** @type {HTMLElement} */ (q("[data-study-menu-host]")).innerHTML = menuHtml();
    }
    return true;
  }

  /** @param {SubmitEvent} e */
  function submit(e) {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (form.matches("[data-study-ask-form]")) {
      e.preventDefault();
      ask(form);
      return true;
    }
    if (form.matches("[data-study-tutor-form]")) {
      e.preventDefault();
      const data = new FormData(form);
      const settings = { engine: String(data.get("engine") ?? "claude"), model: String(data.get("model") ?? ""), style: String(data.get("style") ?? "default"),
        custom: String(data.get("custom") ?? ""), length: String(data.get("length") ?? "default"), language: String(data.get("language") ?? "").trim() };
      state.menu = null;
      state.draft = null;
      host.dispatch({ type: "studyTutor", settings });
      return true;
    }
    return false;
  }

  /** ⏎ sends; ⇧⏎ is a new line. @param {KeyboardEvent} e */
  function key(e) {
    const t = /** @type {HTMLElement} */ (e.target);
    if (!t.matches?.("[data-study-ask-form] textarea")) return false;
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      const form = /** @type {HTMLFormElement | null} */ (t.closest("form"));
      if (form) ask(form);
    }
    return true;
  }

  host.ws.addEventListener("input", (e) => {
    const t = /** @type {HTMLTextAreaElement} */ (e.target);
    if (!t.matches?.("[data-study-ask-form] textarea")) return;
    t.style.height = "";
    t.style.height = `${Math.min(t.scrollHeight, 220)}px`;
  });

  /* ── the core's answers ────────────────────────────────────────── */

  /** @param {ChatEvent} event */
  function chat(event) {
    if (state.chat?.id !== event.id) state.menu = null;
    state.chat = event;
    // A finished turn is the whole answer; the live one goes.
    state.live = event.busy ? state.live : null;
  }

  /** @param {{id: string, live: Live}} event */
  function live(event) {
    if (state.chat?.id !== event.id) return false;
    state.live = event.live;
    return true;
  }

  /** @param {Capabilities} caps */
  function capabilities(caps) {
    state.caps.set(caps.engine, caps);
  }

  return { chat, live, capabilities, render, renderLive, click, change, submit, key, has: () => state.chat !== null };
}
