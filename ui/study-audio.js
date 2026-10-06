/* Audio overviews: two hosts talking a notebook's sources through, spoken by
 * Fish Audio — the player, chapters to jump to, and the transcript, which
 * follows the voice and is followed when clicked.
 *
 * Before anything is spoken the person agrees to Fish's terms and chooses
 * the voices, here; the core keeps both, reads the key from the shell, and
 * speaks the script on a worker. Its sound is served as study://localhost/
 * — kept out of Git, in the notebook's .notebook/audio. */

import { escapeHtml as esc, icon } from "./html.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudySegment").StudySegment} Segment */
/** @typedef {import("./bindings/StudyChapter").StudyChapter} Chapter */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {import("./bindings/StudyVoiceState").StudyVoiceState} VoiceState */
/** @typedef {import("./bindings/StudyVoice").StudyVoice} Voice */
/** @typedef {import("./bindings/StudyHost").StudyHost} Host */
/** @typedef {{made: Made, format: string, segments: Segment[], chapters: Chapter[], timings: [number, number][], duration: number,
 *   state: string, why: string | null, done: number, of: number, url: string | null, citations: Citation[], voice: VoiceState}} AudioEvent */

/** What each format is called. */
export const FORMATS = /** @type {[string, string, string][]} */ ([
  ["deepDive", "Deep dive", "Two hosts unpacking the sources"],
  ["brief", "Brief", "The essentials, in a few minutes"],
  ["critique", "Critique", "Two experts weighing what the sources argue"],
  ["debate", "Debate", "Two hosts on opposite sides"],
  ["lecture", "Lecture", "One teacher, from the ground up"],
]);
const FORMAT = Object.fromEntries(FORMATS.map(([k, name]) => [k, name]));

/** Where `study://` media is, as this platform's webview names it. @param {string} path */
export function studyUrl(path) {
  // The preview hands its own sound over as it is.
  if (/^(data|blob):/.test(path)) return path;
  return `${/Win/.test(navigator.platform) ? "http://study.localhost/" : "study://localhost/"}${path}`;
}

/** Seconds as a player's clock reads them. @param {number} s */
const clock = (s) => {
  const t = Math.max(0, Math.floor(s));
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, "0")}`;
};

/** @param {{dispatch: (command: any) => unknown, readOnly: () => boolean}} host */
export function createAudio(host) {
  /** @type {{audio: AudioEvent | null, voice: VoiceState | null, voices: Voice[], query: string, mine: boolean, finding: boolean, error: string | null,
   *   choosing: boolean, samples: Map<string, string>, picked: Host[]}} */
  const state = {
    audio: null, voice: null,
    // The voice browser: what was found, for what — and whether only the
    // account's own; a sample's sound by voice; the voices being chosen.
    voices: [], query: "", mine: false, finding: false, error: null, choosing: false, samples: new Map(), picked: [],
  };

  /** Who a speaker is: their voice's name, or Host A, Host B. @param {number} speaker */
  const speakerName = (speaker) => state.voice?.hosts[speaker]?.title || (state.audio?.format === "lecture" ? "Teacher" : `Host ${"AB"[speaker] ?? speaker + 1}`);

  /** Agreeing to Fish's terms. */
  function consentHtml() {
    return `<div class="st-card st-voice-consent"><h2>${icon("audio")} Spoken by Fish Audio</h2>
        <p>Audio overviews are spoken by Fish Audio (fish.audio) with the key in <code>FISH_API_KEY</code>, using its <b>s2.1-pro-free</b> model, free until
          30 November 2026. On its free tier <b>Fish may use what it is sent to improve its models</b>: that is each script, written from your sources
          and notes. Nothing is sent until you agree.</p>
        ${host.readOnly() ? "" : `<div class="rs-actions"><button class="cc-primary" data-study-voice-agree>Agree, and go on</button></div>`}</div>`;
  }

  /** The voices found, each to listen to and to choose. @param {number} speakers */
  function resultsHtml(speakers) {
    const rows = state.voices.map((v) => {
      const sample = state.samples.get(v.id);
      const chosen = state.picked.some((h) => h.id === v.id);
      return `<div class="st-voice"><div><b>${esc(v.title)}</b><span class="st-faint">${esc([v.languages.join(", "), v.author, `${v.uses.toLocaleString()} uses`].filter(Boolean).join(" · "))}</span>` +
        `${v.description ? `<span class="st-faint">${esc(v.description)}</span>` : ""}</div>` +
        (v.sample ? (sample ? `<audio class="st-sample" controls preload="none" src="${esc(studyUrl(sample))}"></audio>` : `<button class="st-link" data-study-voice-listen="${esc(v.id)}">Listen</button>`) : "") +
        (chosen || state.picked.length >= speakers || host.readOnly() ? "" : `<button class="cc-quiet" data-study-voice-pick="${esc(v.id)}">Use as voice ${state.picked.length + 1}</button>`) + `</div>`;
    }).join("");
    const said = state.finding ? `<p class="st-faint">${icon("search")} Finding…</p>` : state.error ? `<p class="st-faint">${esc(state.error)}</p>` : "";
    return `${said}${rows}`;
  }

  /** Choosing the voices: searched, listened to, picked. @param {number} speakers */
  function chooserHtml(speakers) {
    const picked = state.picked.map((h, i) => `<span class="st-chip-btn">${i + 1}. ${esc(h.title)}<button class="st-x" data-study-voice-unpick="${i}" aria-label="Not this one">${icon("close")}</button></span>`).join("");
    return `<div class="st-card st-voice-chooser"><h2>${icon("mic")} ${speakers === 1 ? "A voice" : "Two voices"}</h2>
        <p>${speakers === 1 ? "The voice that teaches." : "The two hosts' voices, the first speaking first."} Search Fish's library, listen, and choose.</p>
        ${picked ? `<div class="st-picked">${picked}</div>` : ""}
        <form class="st-voice-find" data-study-voice-find novalidate><input name="query" value="${esc(state.query)}" placeholder="A voice — “narrator”, “British” — or its id or fish.audio link" autocomplete="off">
          <input name="language" value="en" aria-label="Language" size="4">
          <label class="st-mine" title="Only the voices your Fish account made: your clones"><input type="checkbox" name="mine"${state.mine ? " checked" : ""}> Mine</label>
          <button type="submit" class="cc-quiet">Find</button></form>
        <div class="st-voices" data-study-voice-results>${resultsHtml(speakers)}</div>
        ${state.picked.length >= speakers && !host.readOnly() ? `<div class="rs-actions"><button class="cc-primary" data-study-voice-use>Use ${speakers === 1 ? "this voice" : "these voices"}</button></div>` : ""}</div>`;
  }

  /** The key, Fish's terms and the voices, on their own: what audio
   *  overviews are spoken with, set before any is made. */
  function settingsHtml() {
    const voice = state.voice;
    const ro = host.readOnly();
    const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>Voices</h1><span class="st-faint">What audio overviews are spoken with: Fish Audio's s2.1-pro-free model, in voices you choose</span></div></div>`;
    if (!voice) return `<div class="st-reader st-voices-view">${head}<p class="st-faint"><span class="lab-spin"></span> Looking for Fish's key…</p></div>`;
    const key = voice.key
      ? `<div class="st-card"><h2>${icon("check")} Fish Audio's key is set</h2><p>Proton found <code>FISH_API_KEY</code> in your shell. It holds it in memory only, and sends it to Fish alone.</p></div>`
      : `<div class="st-card"><h2>${icon("d-warning")} Fish Audio's key is not set</h2><p>Make a key at fish.audio/app/api-keys, add <code>export FISH_API_KEY=…</code> to your shell's
          profile (<code>~/.zshrc</code>), and look again — Proton reads it from your shell each time, and never stores it.</p>
          ${ro ? "" : `<div class="rs-actions"><button class="cc-primary" data-study-voice-key>Look again</button></div>`}</div>`;
    const hosts = voice.hosts.length
      ? `<p>Spoken by ${voice.hosts.map((h, i) => `<b>${esc(h.title)}</b>${i === 0 ? " (who also teaches a lecture)" : ""}`).join(" and ")}.</p>`
      : `<p class="st-faint">No voices chosen yet.</p>`;
    const chosen = `<div class="st-card"><h2>${icon("mic")} The voices</h2>${hosts}` +
      (ro || !voice.key || !voice.consented || state.choosing ? "" : `<div class="rs-actions"><button class="cc-quiet" data-study-voice-change>${voice.hosts.length ? "Change the voices" : "Choose the voices"}</button></div>`) + `</div>`;
    const body = !voice.consented ? consentHtml() : voice.key && (state.choosing || !voice.hosts.length) ? chooserHtml(2) : "";
    return `<div class="st-reader st-voices-view">${head}${key}${chosen}${body}</div>`;
  }

  /** The voices, on their own, drawn; a search being typed is kept. @param {HTMLElement} stage */
  function renderSettings(stage) {
    const results = stage.querySelector(".st-voices-view [data-study-voice-results]");
    if (results && stage.querySelector("[data-study-voice-find] input:focus")) {
      results.innerHTML = resultsHtml(2);
      return;
    }
    stage.innerHTML = settingsHtml();
  }

  /** What stands between the script and its sound. @param {AudioEvent} a */
  function statusHtml(a) {
    const voice = a.voice;
    const speakers = a.format === "lecture" ? 1 : 2;
    if (a.state === "voicing") {
      return `<div class="st-card st-voicing"><span class="lab-spin"></span><div><b>Being spoken</b><span class="st-faint">${a.done} of ${a.of} pieces · each kept as it comes, so a stop goes on from there</span></div>` +
        (host.readOnly() ? "" : `<button class="cc-quiet" data-study-voice-stop>Stop</button>`) + `</div>`;
    }
    if (!voice.consented) return consentHtml();
    if (!voice.key) {
      return `<div class="st-card"><h2>${icon("d-warning")} Fish Audio's key is not set</h2><p>Make a key at fish.audio/app/api-keys, put <code>export FISH_API_KEY=…</code> in your shell's
          profile, and look again. Proton reads it from your shell, holds it in memory only, and sends it to Fish alone.</p>
          ${host.readOnly() ? "" : `<div class="rs-actions"><button class="cc-primary" data-study-voice-key>Look again</button></div>`}</div>`;
    }
    if (state.choosing || voice.hosts.length < speakers) return chooserHtml(speakers);
    const said = a.why ? `<p>${esc(a.why)}</p>` : "";
    return `<div class="st-card"><h2>${a.state === "failed" ? `${icon("d-error")} Not spoken` : "Not spoken yet"}</h2>${said}` +
      (host.readOnly() ? "" : `<div class="rs-actions"><button class="cc-quiet" data-study-voice-change>Change the voices</button><button class="cc-primary" data-study-voice-go>Speak it</button></div>`) + `</div>`;
  }

  /** The transcript: every turn, its speaker, its citations. @param {AudioEvent} a */
  function transcriptHtml(a) {
    const chapters = new Map(a.chapters.map((c) => [c.segment, c.title]));
    return `<div class="st-transcript" data-turn="made">${a.segments.map((s, i) => {
      const chapter = chapters.has(i) ? `<h3 data-study-chapter-at="${i}">${esc(chapters.get(i) ?? "")}</h3>` : "";
      const cites = s.cites.length ? ` ${s.cites.map((raw) => `[[${raw}]]`).join("")}` : "";
      return `${chapter}<div class="st-line-said speaker${s.speaker}" data-study-segment="${i}"><b>${esc(speakerName(s.speaker))}</b>` +
        `<div class="md">${answerHtml(`${s.display}${cites}`, a.citations)}</div></div>`;
    }).join("")}</div>`;
  }

  /** The player: its sound, chapters, speed. @param {AudioEvent} a */
  function playerHtml(a) {
    if (a.state !== "ready" || !a.url) return "";
    const chapters = a.chapters.length ? `<div class="st-chapters">${a.chapters.map((c) => `<button class="st-chip-btn" data-study-chapter="${c.segment}">` +
      `${esc(c.title)}<span class="st-faint">${clock(a.timings[c.segment]?.[0] ?? 0)}</span></button>`).join("")}</div>` : "";
    return `<div class="st-player"><audio data-study-sound preload="metadata" src="${esc(studyUrl(a.url))}"></audio>
        <button class="st-play" data-study-play aria-label="Play">${icon("play")}</button>
        <span class="st-time" data-study-time>0:00</span>
        <input class="st-seek" data-study-seek type="range" min="0" max="${Math.max(1, Math.round(a.duration))}" step="1" value="0" aria-label="Where in it">
        <span class="st-time">${clock(a.duration)}</span>
        <select data-study-speed aria-label="Speed">${[0.75, 1, 1.25, 1.5, 2].map((r) => `<option value="${r}"${r === 1 ? " selected" : ""}>${r}×</option>`).join("")}</select>
      </div>${chapters}`;
  }

  /** The overview, whole, for the middle. @param {AudioEvent} a */
  function html(a) {
    const ro = host.readOnly();
    const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>${esc(a.made.title)}</h1><span class="st-faint">Audio overview · ${esc(FORMAT[a.format] ?? a.format)}${a.duration ? ` · ${clock(a.duration)}` : ""} · ${esc(a.made.by)}</span></div>` +
      `<button class="cc-quiet" data-study-task="${esc(a.made.task)}" title="What the call was told and asked">Prompt</button>` +
      (ro ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(a.made.id)}" title="Put its script in the notebook's bin">Bin</button>`) + `</div>`;
    const status = a.state === "ready" ? "" : statusHtml(a);
    const voices = a.state === "ready" && a.voice.hosts.length ? `<p class="st-faint st-voices-said">Spoken by ${a.voice.hosts.map((h) => esc(h.title)).join(" and ")} · Fish Audio</p>` : "";
    return `<div class="st-reader st-audio-view">${head}${status}${playerHtml(a)}${voices}<h3>Transcript</h3>${transcriptHtml(a)}</div>`;
  }

  /** The turn being said, lit; the clock and the seek bar moved. @param {HTMLElement} stage */
  function follow(stage) {
    const sound = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
    const a = state.audio;
    if (!sound || !a) return;
    const t = sound.currentTime;
    const time = stage.querySelector("[data-study-time]");
    if (time) time.textContent = clock(t);
    const seek = /** @type {HTMLInputElement | null} */ (stage.querySelector("[data-study-seek]"));
    if (seek && document.activeElement !== seek) seek.value = String(Math.round(t));
    const at = a.timings.findIndex(([start, end]) => t >= start && t < end);
    stage.querySelectorAll(".st-line-said.now").forEach((el) => { if (el.getAttribute("data-study-segment") !== String(at)) el.classList.remove("now"); });
    const line = at >= 0 ? stage.querySelector(`[data-study-segment="${at}"]`) : null;
    if (line && !line.classList.contains("now")) {
      line.classList.add("now");
      if (!sound.paused) line.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
    const play = stage.querySelector("[data-study-play]");
    if (play) play.innerHTML = icon(sound.paused ? "play" : "pause");
  }

  /** Draw the overview; its sound is kept playing if it is the same sound. @param {HTMLElement} stage */
  function render(stage) {
    const a = state.audio;
    if (!a) return;
    const sound = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
    const src = a.url ? studyUrl(a.url) : "";
    // The same sound already playing: the rest is drawn around it.
    if (sound && sound.getAttribute("src") === src && a.state === "ready") return follow(stage);
    // A search being typed is kept: only what was found is drawn again.
    const results = stage.querySelector("[data-study-voice-results]");
    if (results && stage.querySelector("[data-study-voice-find] input:focus")) {
      results.innerHTML = resultsHtml(a.format === "lecture" ? 1 : 2);
      return;
    }
    stage.innerHTML = html(a);
    const fresh = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
    if (fresh) {
      for (const kind of ["timeupdate", "play", "pause", "seeked", "ended"]) fresh.addEventListener(kind, () => follow(stage));
    }
  }

  /** Jump to a time, playing on if it was playing. @param {HTMLElement} stage @param {number} t */
  function seek(stage, t) {
    const sound = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
    if (!sound) return;
    sound.currentTime = Math.max(0, t);
    follow(stage);
  }

  /** A click in the overview; `true` if it was the overview's. @param {HTMLElement} t @param {HTMLElement} stage @param {() => void} redraw */
  function click(t, stage, redraw) {
    const a = state.audio;
    const settings = Boolean(t.closest(".st-voices-view"));
    if (!settings && (!a || !t.closest(".st-audio-view"))) return false;
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    // The voices, on their own: only what sets them.
    if (settings || !a) return voiceClick(at, redraw);
    if (at("[data-study-play]")) {
      const sound = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
      if (sound) {
        if (sound.paused) sound.play().catch(() => {});
        else sound.pause();
        follow(stage);
      }
      return true;
    }
    const chapter = at("[data-study-chapter]");
    if (chapter) {
      seek(stage, a.timings[Number(chapter.dataset.studyChapter)]?.[0] ?? 0);
      return true;
    }
    const line = at("[data-study-segment]");
    if (line && !at("[data-study-cite]") && a.state === "ready") {
      seek(stage, a.timings[Number(line.dataset.studySegment)]?.[0] ?? 0);
      return true;
    }
    if (at("[data-study-voice-go]")) {
      host.dispatch({ type: "studyAudioVoice", id: a.made.id });
      return true;
    }
    if (at("[data-study-voice-stop]")) {
      host.dispatch({ type: "studyCancelJob", id: a.made.id });
      return true;
    }
    return voiceClick(at, redraw);
  }

  /** A click on what sets the voices: Fish's terms, its key, the voices
   *  found and chosen. @param {(sel: string) => HTMLElement | null} at @param {() => void} redraw */
  function voiceClick(at, redraw) {
    if (at("[data-study-voice-agree]")) {
      host.dispatch({ type: "studyVoiceSetup", consent: true, hosts: state.voice?.hosts ?? [] });
      return true;
    }
    if (at("[data-study-voice-key]")) {
      host.dispatch({ type: "studyVoiceKey" });
      return true;
    }
    if (at("[data-study-voice-change]")) {
      state.choosing = true;
      state.picked = [];
      redraw();
      return true;
    }
    const listen = at("[data-study-voice-listen]");
    if (listen) {
      listen.textContent = "Fetching…";
      host.dispatch({ type: "studyVoiceSample", id: listen.dataset.studyVoiceListen });
      return true;
    }
    const pick = at("[data-study-voice-pick]");
    if (pick) {
      const v = state.voices.find((x) => x.id === pick.dataset.studyVoicePick);
      if (v) state.picked.push({ id: v.id, title: v.title });
      redraw();
      return true;
    }
    const unpick = at("[data-study-voice-unpick]");
    if (unpick) {
      state.picked.splice(Number(unpick.dataset.studyVoiceUnpick), 1);
      redraw();
      return true;
    }
    if (at("[data-study-voice-use]")) {
      host.dispatch({ type: "studyVoiceSetup", consent: true, hosts: state.picked });
      state.choosing = false;
      return true;
    }
    return false;
  }

  /** @param {HTMLElement} t @param {HTMLElement} stage */
  function change(t, stage) {
    if (!state.audio || !t.closest(".st-audio-view")) return false;
    const sound = /** @type {HTMLAudioElement | null} */ (stage.querySelector("[data-study-sound]"));
    if (t.matches("[data-study-seek]") && sound) {
      seek(stage, Number(/** @type {HTMLInputElement} */ (t).value));
      return true;
    }
    if (t.matches("[data-study-speed]") && sound) {
      sound.playbackRate = Number(/** @type {HTMLSelectElement} */ (t).value) || 1;
      return true;
    }
    return false;
  }

  /** @param {HTMLFormElement} form */
  function submit(form) {
    if (!form.matches("[data-study-voice-find]")) return false;
    const data = new FormData(form);
    state.query = String(data.get("query") ?? "").trim();
    state.mine = data.get("mine") === "on";
    state.finding = true;
    state.error = null;
    host.dispatch({ type: "studyVoiceFind", query: state.query, language: String(data.get("language") ?? "").trim(), mine: state.mine });
    return true;
  }

  return {
    render, renderSettings, click, change, submit,
    /** @param {AudioEvent} event */
    audio(event) {
      state.audio = event;
      state.voice = event.voice;
      if (event.voice.hosts.length && !state.choosing) state.picked = [...event.voice.hosts];
    },
    /** @param {VoiceState} voice */
    voiceState(voice) {
      state.voice = voice;
      if (state.audio) state.audio = { ...state.audio, voice };
      if (voice.hosts.length && !state.choosing) state.picked = [...voice.hosts];
    },
    /** The voices are being looked at on their own, from the start. */
    settings() {
      state.choosing = false;
      state.error = null;
    },
    /** @param {{query: string, voices: Voice[], total: number, error: string | null}} event */
    voices(event) {
      state.finding = false;
      state.voices = event.voices;
      state.error = event.error ?? (event.voices.length ? null : "No voice by that name.");
    },
    /** @param {{id: string, url: string | null, error: string | null}} event */
    sample(event) {
      if (event.url) state.samples.set(event.id, event.url);
      else state.error = event.error;
    },
    open: () => state.audio?.made.id ?? null,
    citations: () => state.audio?.citations ?? [],
    clear: () => { state.audio = null; },
  };
}
