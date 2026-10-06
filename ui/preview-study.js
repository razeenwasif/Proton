/* Notebooks for the browser preview: one with sources, a knowledge base of
 * three roots — one all in iCloud — and sources that read themselves on a
 * timer, so the picker and the sources panel can be designed against. The
 * real ones are folders the core reads; this must never grow into a second. */

const now = Math.floor(Date.now() / 1000);

/** @param {string} root @param {string} rel @param {string} kind @param {number} size @param {boolean} [inCloud] */
const file = (root, rel, kind, size, inCloud = false) => ({
  name: rel.split("/").pop(), path: `${root}/${rel}`, kind, size, mtime: now - 86400 * 30, inCloud,
  readable: !["image", "video", "audio", "slides", "document", "other"].includes(kind), source: null,
});

const FILES = [
  file("OnyxVault", "00 - Machine Learning/Attention.md", "markdown", 6200),
  file("OnyxVault", "00 - Machine Learning/Gradient descent.md", "markdown", 4100),
  file("OnyxVault", "03 - Algorithms/Search.md", "markdown", 8800),
  file("OnyxVault", "03 - Algorithms/Heaps and priority queues.md", "markdown", 3900),
  file("Courses", "COMP3670/Exam_2020.pdf", "pdf", 412_000),
  file("Courses", "COMP3670/Exam_2020_solutions.pdf", "pdf", 512_000),
  file("Courses", "COMP3670/Week 3 - Informed search.pdf", "pdf", 2_400_000),
  file("Courses", "COMP3670/lab2.ipynb", "notebook", 96_000),
  file("Courses", "COMP3670/Lecture 4.vtt", "captions", 71_000),
  file("Courses", "COMP3670/screenshot.png", "image", 340_000),
  file("ANU", "2023 S2/COMP2310/Final exam 2022.pdf", "pdf", 1_800_000, true),
  file("ANU", "2023 S2/COMP2310/Lecture 12 - Virtual memory.pdf", "pdf", 5_100_000, true),
];

const LEFT = [
  { root: "OnyxVault", dir: "", name: "07 - Business", why: "left out by your rule OnyxVault/07 - Business/05 - Finance/", rule: null },
  { root: "Courses", dir: "COMP3670", name: "node_modules", why: "left out by default (node_modules/)", rule: null },
];

/** @param {(event: any) => void} deliver */
export function createStudyPreview(deliver) {
  const state = {
    folder: /** @type {string | null} */ ("~/Github/Notebooks"),
    open: /** @type {string | null} */ ("comp3670-exam"),
    notebooks: [
      { slug: "comp3670-exam", title: "COMP3670 — the final exam", course: "COMP3670", exam: "2026-11-12", created: now - 86400 * 3, archived: false },
      { slug: "machine-learning", title: "Machine learning, from my notes", course: "", exam: null, created: now - 86400 * 20, archived: false },
    ],
    /** @type {Record<string, any[]>} */
    sources: {
      "comp3670-exam": [
        { id: "S01", title: "Exam_2020.pdf", kind: "pdf", role: "pastPaper", origin: { from: "file", root: "Courses", rel: "COMP3670/Exam_2020.pdf" }, status: { state: "ready" }, units: "pages", count: 9, size: 412000, thin: 0, selected: true, labels: [] },
        { id: "S02", title: "Search.md", kind: "markdown", role: "notes", origin: { from: "file", root: "OnyxVault", rel: "03 - Algorithms/Search.md" }, status: { state: "ready" }, units: "lines", count: 212, size: 8800, thin: 0, selected: true, labels: [] },
        { id: "S03", title: "Lecture 12 - Virtual memory.pdf", kind: "pdf", role: "slides", origin: { from: "file", root: "ANU", rel: "2023 S2/COMP2310/Lecture 12 - Virtual memory.pdf" }, status: { state: "inCloud" }, units: "pages", count: 0, size: 5100000, thin: 0, selected: false, labels: [] },
      ],
      "machine-learning": [],
    },
    next: /** @type {Record<string, number>} */ ({ "comp3670-exam": 4, "machine-learning": 1 }),
    roots: ["~/OnyxVault", "~/Courses", "~/Library/Mobile Documents/com~apple~CloudDocs/ANU"],
    /** @type {Record<string, {id: string, title: string, body: string, updated: number, source: string | null}[]>} */
    notes: { "comp3670-exam": [{ id: "N001", title: "What the exam asks", body: "Two questions on search every year: **optimality** and **heuristics**.\n\n- $h(n) \\le h^*(n)$", updated: now - 3600, source: null }] },
    paused: false,
  };

  const added = () => Object.fromEntries((state.sources[state.open ?? ""] ?? []).filter((s) => s.origin.from === "file").map((s) => [`${s.origin.root}/${s.origin.rel}`, s.id]));
  const withSource = (/** @type {any} */ f) => ({ ...f, source: added()[f.path] ?? null });

  function listEvent() {
    return {
      type: "studyList", folder: state.folder, open: state.open,
      notebooks: state.notebooks.map((n) => ({ slug: n.slug, title: n.title, course: n.course, exam: n.exam, sources: (state.sources[n.slug] ?? []).length, opened: n.created, archived: n.archived })),
    };
  }

  function notebookEvent() {
    const n = state.notebooks.find((b) => b.slug === state.open);
    if (!n) return { type: "studyClosed" };
    const notes = (state.notes[n.slug] ?? []).map((x) => ({ id: x.id, title: x.title, updated: x.updated, preview: x.body.replace(/\s+/g, " ").slice(0, 140) }));
    return { type: "studyNotebook", slug: n.slug, title: n.title, course: n.course, exam: n.exam, readOnly: null, sources: state.sources[n.slug], notes };
  }

  function kbEvent() {
    const roots = ["OnyxVault", "Courses", "ANU"].map((label, i) => {
      const of = FILES.filter((f) => f.path.startsWith(`${label}/`));
      return { label, path: state.roots[i], files: of.length * 100, readable: of.filter((f) => f.readable).length * 100, inCloud: of.filter((f) => f.inCloud).length * 100, missing: false };
    });
    return { type: "studyKb", roots, rules: ["OnyxVault/07 - Business/05 - Finance/"], started: true, ready: true, walking: true, done: 1340, total: 1622, paused: state.paused, documents: 1262, passages: 11494 };
  }

  /** @param {string} root @param {string} dir */
  function folderEvent(root, dir) {
    const prefix = dir ? `${root}/${dir}/` : `${root}/`;
    const under = FILES.filter((f) => f.path.startsWith(prefix));
    /** @type {Map<string, any>} */
    const folders = new Map();
    const files = [];
    for (const f of under) {
      const rest = f.path.slice(prefix.length);
      const slash = rest.indexOf("/");
      if (slash < 0) files.push(withSource(f));
      else {
        const name = rest.slice(0, slash);
        const folder = folders.get(name) ?? { name, path: prefix + name, files: 0, readable: 0, inCloud: 0 };
        folder.files++;
        folder.readable += f.readable ? 1 : 0;
        folder.inCloud += f.inCloud ? 1 : 0;
        folders.set(name, folder);
      }
    }
    const left = LEFT.filter((l) => l.root === root && l.dir === dir).map((l) => ({ name: l.name, path: prefix + l.name, dir: true, why: l.why, rule: l.rule }));
    return { type: "studyKbFolder", root, dir, folders: [...folders.values()], files, left };
  }

  /** @param {string} query */
  function resultsEvent(query) {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const files = FILES.filter((f) => words.every((w) => f.path.toLowerCase().includes(w))).map(withSource);
    const passages = words.includes("search")
      ? [{ path: "OnyxVault/03 - Algorithms/Search.md", cite: "OnyxVault/03 - Algorithms/Search.md § Search › Uniform cost L40-58", title: "Search › Uniform cost", text: "Uniform-cost search expands the node with the lowest path cost g(n) first; with positive step costs it is complete and optimal." }]
      : [];
    return { type: "studyKbResults", query, files, passages };
  }

  /** Read sources on a timer, as a worker would. @param {string} slug @param {string[]} ids */
  function read(slug, ids) {
    setTimeout(() => {
      for (const source of state.sources[slug] ?? []) {
        if (!ids.includes(source.id)) continue;
        source.status = { state: "ready" };
        source.count = source.units === "pages" ? 14 : source.units === "seconds" ? 3120 : 180;
      }
      if (state.open === slug) [notebookEvent(), listEvent()].forEach((event) => deliver(event));
    }, 1200);
  }

  /** @param {string[]} paths @param {boolean} download */
  function add(paths, download) {
    const slug = state.open;
    if (!slug) return [];
    const list = state.sources[slug];
    const reading = [];
    for (const path of paths) {
      const already = list.find((s) => s.origin.from === "file" && `${s.origin.root}/${s.origin.rel}` === path);
      if (already) {
        if (download && already.status.state === "inCloud") {
          already.status = { state: "downloading" };
          reading.push(already.id);
        }
        continue;
      }
      const f = FILES.find((x) => x.path === path);
      if (!f) continue;
      const id = `S${String(state.next[slug]++).padStart(2, "0")}`;
      const [root, ...rest] = path.split("/");
      const status = f.inCloud && !download ? "inCloud" : f.inCloud ? "downloading" : "reading";
      list.push({ id, title: f.name, kind: f.kind, role: /exam/i.test(f.name) ? (/sol/i.test(f.name) ? "solution" : "pastPaper") : f.kind === "markdown" ? "notes" : "reading",
        origin: { from: "file", root, rel: rest.join("/") }, status: { state: status }, units: f.kind === "pdf" ? "pages" : f.kind === "captions" ? "seconds" : "lines",
        count: 0, size: f.size, thin: 0, selected: true, labels: [] });
      if (status !== "inCloud") reading.push(id);
    }
    read(slug, reading);
    return [notebookEvent(), listEvent()];
  }

  return {
    sync: () => [listEvent(), kbEvent(), notebookEvent()],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "studyFolder":
          state.folder = command.folder;
          return [listEvent()];
        case "studyRoots":
          state.roots = command.roots;
          return [kbEvent()];
        case "studyKbPause":
          state.paused = command.paused;
          return [kbEvent()];
        case "studyKbRefresh":
          return [kbEvent()];
        case "studyKbBrowse":
          return [folderEvent(command.root, command.dir)];
        case "studyKbSearch":
          return [resultsEvent(command.query)];
        case "studyNew": {
          const slug = command.title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "notebook";
          state.notebooks.unshift({ slug, title: command.title, course: "", exam: null, created: Math.floor(Date.now() / 1000), archived: false });
          state.sources[slug] = [];
          state.next[slug] = 1;
          state.open = slug;
          return [listEvent(), notebookEvent()];
        }
        case "studyOpen":
          state.open = command.slug;
          return [listEvent(), notebookEvent()];
        case "studyClose":
          state.open = null;
          return [listEvent(), { type: "studyClosed" }];
        case "studyDetails": {
          const n = state.notebooks.find((b) => b.slug === state.open);
          if (n) Object.assign(n, { title: command.title || n.title, course: command.course, exam: command.exam });
          return [listEvent(), notebookEvent()];
        }
        case "studyAdd":
          return add(command.paths, false);
        case "studyDownload":
          return add(command.paths, true);
        case "studyPaste": {
          const slug = state.open ?? "";
          const id = `S${String(state.next[slug]++).padStart(2, "0")}`;
          state.sources[slug].push({ id, title: command.title || command.text.split("\n")[0].slice(0, 60), kind: "markdown", role: "notes", origin: { from: "pasted" },
            status: { state: "ready" }, units: "lines", count: command.text.split("\n").length, size: command.text.length, thin: 0, selected: true, labels: [] });
          return [notebookEvent(), listEvent()];
        }
        case "studyRemove":
          state.sources[state.open ?? ""] = (state.sources[state.open ?? ""] ?? []).filter((s) => s.id !== command.id);
          return [notebookEvent(), listEvent()];
        case "studySelect":
          for (const s of state.sources[state.open ?? ""] ?? []) s.selected = command.ids.includes(s.id);
          return [notebookEvent()];
        case "studyShow": {
          const source = (state.sources[state.open ?? ""] ?? []).find((s) => s.id === command.id);
          if (!source) return [];
          const view = source.units === "pages"
            ? { shape: "pages", pages: ["Question 1\nExplain why uniform-cost search is optimal when every step costs at least ε > 0.", "", "Question 2\nGive an admissible heuristic for the 8-puzzle, and say why it is."] }
            : source.units === "seconds"
              ? { shape: "cues", cues: [{ start: 192, end: 200, text: "So the frontier is a priority queue," }, { start: 200, end: 207, text: "ordered by g of n, the cost so far." }] }
              : { shape: "sections", sections: [{ from: 1, to: 6, text: "# Search\n\nBreadth-first search is complete, and optimal when every step costs the same: $O(b^d)$ time and space." },
                { from: 7, to: 14, text: "## Uniform cost\n\nExpands the node with the lowest $g(n)$ first.\n\n```python\nfrontier = [(0, start)]\n```" }] };
          return [{ type: "studySource", id: source.id, title: source.title, kind: source.kind, at: command.at ?? { from: 7, to: 9 },
            said: source.origin.root === "ANU" ? "Its file is in iCloud: these are its words as they were read." : null, view }];
        }
        case "studyShowNote": {
          const note = (state.notes[state.open ?? ""] ?? []).find((x) => x.id === command.id);
          return note ? [{ type: "studyNote", ...note }] : [];
        }
        case "studyNote": {
          const list = (state.notes[state.open ?? ""] ??= []);
          let note = list.find((x) => x.id === command.id);
          const title = command.title || command.body.split("\n")[0].replace(/^#+\s*/, "").slice(0, 80) || "Untitled note";
          if (note) Object.assign(note, { title, body: command.body, updated: Math.floor(Date.now() / 1000) });
          else {
            note = { id: `N${String(list.length + 1).padStart(3, "0")}`, title, body: command.body, updated: Math.floor(Date.now() / 1000), source: null };
            list.push(note);
          }
          return [notebookEvent(), { type: "studyNote", ...note }];
        }
        case "studyNoteDelete":
          state.notes[state.open ?? ""] = (state.notes[state.open ?? ""] ?? []).filter((x) => x.id !== command.id);
          return [notebookEvent()];
        case "studyNoteSource": {
          const note = (state.notes[state.open ?? ""] ?? []).find((x) => x.id === command.id);
          if (!note) return [];
          const slug = state.open ?? "";
          const id = `S${String(state.next[slug]++).padStart(2, "0")}`;
          state.sources[slug].push({ id, title: note.title, kind: "markdown", role: "notes", origin: { from: "note", note: note.id }, status: { state: "ready" },
            units: "lines", count: note.body.split("\n").length + 2, size: note.body.length, thin: 0, selected: true, labels: [] });
          note.source = id;
          return [notebookEvent(), listEvent(), { type: "studyNote", ...note }];
        }
        case "studyRole":
          for (const s of state.sources[state.open ?? ""] ?? []) if (s.id === command.id) s.role = command.role;
          return [notebookEvent()];
        default:
          return [];
      }
    },
  };
}
