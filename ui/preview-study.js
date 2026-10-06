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
    /** The open chat, as the core would keep it. */
    chat: { id: "C001", title: "Why is uniform-cost search optimal?", settings: { engine: "claude", model: "", style: "default", custom: "", length: "default", language: "" },
      turns: [{ asked: "Why is uniform-cost search optimal?",
        answer: "Uniform-cost search expands nodes in order of their path cost $g(n)$ [[S02 § Search › Uniform cost L40-58]], so the first time it expands a goal, no cheaper path to it can remain on the frontier — provided every step costs at least some $\\varepsilon > 0$ [[S01 p. 3]]. The 2020 exam asks exactly this in Question 1 [[S01 p. 1]], and an old note of yours cites a page that is not there [[S01 p. 99]].",
        citations: [
          { n: 1, raw: "S02 § Search › Uniform cost L40-58", source: "S02", title: "Search.md", units: "lines", place: { from: 40, to: 58 }, quote: "## Uniform cost Expands the node with the lowest path cost $g(n)$ first. With positive step costs it is complete and optimal.", resolved: true },
          { n: 2, raw: "S01 p. 3", source: "S01", title: "Exam_2020.pdf", units: "pages", place: { from: 3, to: 3 }, quote: "Assume each action costs at least ε > 0. Show that UCS returns an optimal solution.", resolved: true },
          { n: 3, raw: "S01 p. 1", source: "S01", title: "Exam_2020.pdf", units: "pages", place: { from: 1, to: 1 }, quote: "Question 1 (10 marks). Explain why uniform-cost search is optimal.", resolved: true },
          { n: 4, raw: "S01 p. 99", source: "S01", title: "Exam_2020.pdf", units: "pages", place: { from: 99, to: 99 }, quote: "", resolved: false },
        ],
        at: now - 600, engine: "claude", model: "claude-sonnet-5-5", error: null }],
      busy: false, error: null, grounding: "tools",
      chats: [{ id: "C001", title: "Why is uniform-cost search optimal?", updated: now - 600, turns: 1 }] },
  };

  const added = () => Object.fromEntries((state.sources[state.open ?? ""] ?? []).filter((s) => s.origin.from === "file").map((s) => [`${s.origin.root}/${s.origin.rel}`, s.id]));
  const withSource = (/** @type {any} */ f) => ({ ...f, source: added()[f.path] ?? null });

  function listEvent() {
    return {
      type: "studyList", folder: state.folder, open: state.open,
      notebooks: state.notebooks.map((n) => ({ slug: n.slug, title: n.title, course: n.course, exam: n.exam, sources: (state.sources[n.slug] ?? []).length, opened: n.created, archived: n.archived,
        due: n.slug === "comp3670-exam" ? deck.cards.filter((r) => r.state === "new" || (r.due ?? 0) <= Math.floor(Date.now() / 1000)).length : 0 })),
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

  const chatEvent = () => ({ type: "studyChat", live: null, lesson: null, ...state.chat });

  /** The studio: one thing made, and anything being made. */
  const studio = {
    jobs: /** @type {any[]} */ ([]),
    made: [{ id: "M001", kind: "mindMap", title: "Search", made: now - 5000, by: "Claude Code · claude-opus-5-5", task: "J0001" },
      { id: "R001", kind: "studyGuide", title: "COMP3670 — Search, a study guide", made: now - 4000, by: "Codex · gpt-6.1-sol", task: "J0002" },
      { id: "D001", kind: "flashcards", title: "Flashcards: search", made: now - 3000, by: "Claude Code · claude-opus-5-5", task: "J0003" },
      { id: "Q001", kind: "quiz", title: "Quiz: search", made: now - 2000, by: "Claude Code · claude-opus-5-5", task: "J0004" },
      { id: "Q002", kind: "exam", title: "COMP3670 practice final", made: now - 1000, by: "Codex · gpt-6.1-sol", task: "J0005" },
      { id: "L001", kind: "overview", title: "Search, an overview", made: now - 500, by: "Claude Code · claude-opus-5-5", task: "J0006" }],
  };
  /** A quiz, and a stand-in for its marking: the real one is by rule, or by a model of another family, in the core. */
  const question = (/** @type {string} */ id, /** @type {string} */ kind, /** @type {Record<string, any>} */ more) => ({ id, kind, prompt: "", options: [], correct: [], rationales: [], blanks: [],
    number: null, tolerance: 0, unit: "", rubric: [], modelAnswer: "", hint: "", explanation: "", topic: "Search", objectives: ["O001"], level: "medium", cites: [], ...more });
  const quiz = {
    questions: [
      question("q01", "choice", { prompt: "Which search is complete when the branching factor is finite?", options: ["Depth-first", "Breadth-first", "Greedy best-first", "Hill climbing"], correct: [1],
        rationales: ["It can go down one branch for ever.", "It reaches every depth in turn.", "It can loop.", "It stops at a local best."], hint: "Think in layers.", explanation: "BFS expands every node at depth $d$ before any at $d + 1$ [[S02 § Search › Uniform cost L40-58]].", cites: ["S02 § Search › Uniform cost L40-58"] }),
      question("q02", "blanks", { prompt: "A heuristic is ___ when $h(n) \\le h^*(n)$ for every $n$.", blanks: [["admissible"]], explanation: "It never overestimates." }),
      question("q03", "number", { prompt: "The 2020 exam's step cost below which UCS can loop?", number: 0.25, tolerance: 0.001, explanation: "Below 0.25 the lecturer's counterexample loops [[S01 p. 3]].", cites: ["S01 p. 3"] }),
      question("q04", "short", { prompt: "Why is uniform-cost search optimal?", rubric: [{ id: "c1", text: "It expands in order of path cost $g(n)$", points: 2 }, { id: "c2", text: "Step costs are at least some $\\varepsilon > 0$", points: 1 }],
        modelAnswer: "It expands nodes in order of $g(n)$; with every step costing at least $\\varepsilon > 0$, the first goal it expands is the cheapest.", cites: ["S01 p. 3"] }),
    ],
    /** @type {any[]} */
    attempts: [],
    /** @type {string[]} */
    marking: [],
  };
  /** An exam paper, and its sittings: drafts until handed in. */
  const paper = {
    questions: [
      { ...quiz.questions[0], id: "q01", marks: 2, hint: "" },
      { ...quiz.questions[2], id: "q02", marks: 3 },
      question("q03", "long", { prompt: "Prove that A* with an admissible heuristic returns an optimal solution.", marks: 8, cites: ["S02 § Search › Uniform cost L40-58"],
        rubric: [{ id: "c1", text: "Sets up a suboptimal goal $G_2$ on the frontier", points: 3 }, { id: "c2", text: "Uses $f(n) \\le C^*$ for a node on the optimal path", points: 3 }, { id: "c3", text: "Concludes $G_2$ is never expanded first", points: 2 }],
        modelAnswer: "Suppose a suboptimal goal $G_2$ is on the frontier…" }),
    ],
    sections: [{ title: "Section A — Short answers", instructions: "Answer every question.", questions: ["q01", "q02"] }, { title: "Section B — Proof", instructions: "", questions: ["q03"] }],
    /** @type {any[]} */
    attempts: [],
    /** @type {Record<string, any>} */
    drafts: {},
    /** @type {string[]} */
    marking: [],
  };
  const paperEvent = () => {
    const open = paper.attempts.find((a) => a.finished === null);
    return { type: "studyQuiz", made: studio.made.find((m) => m.id === "Q002"), questions: paper.questions, attempts: paper.attempts, marking: paper.marking, results: [], mastery: [], citations: cites,
      drafts: open ? Object.entries(paper.drafts).map(([question, given]) => ({ question, given })) : [], sections: paper.sections, minutes: 90,
      blueprint: "The 2020 and 2021 finals: **90 minutes**, two sections — short answers on search (5 marks), then a proof (8 marks).", source: null };
  };
  const quizEvent = () => ({ type: "studyQuiz", made: studio.made.find((m) => m.id === "Q001"), questions: quiz.questions, attempts: quiz.attempts, marking: quiz.marking,
    drafts: [], sections: [], minutes: null, blueprint: "", source: null,
    results: quiz.attempts.length ? [{ objective: "O001", text: "Explain why uniform-cost search is optimal", topic: "Search",
      score: quiz.attempts.at(-1).answers.reduce((/** @type {number} */ n, /** @type {any} */ a) => { const g = a.second ?? a.grade; return n + (g ? g.score / g.of : 0); }, 0),
      of: Math.max(1, quiz.attempts.at(-1).answers.length), questions: [] }] : [],
    mastery: [{ objective: "O001", text: "Explain why uniform-cost search is optimal", topic: "Search", level: 0.55, evidence: 3, state: "focus" }], citations: cites });
  /** A mark by rule, or none for a written answer. @param {any} q @param {any} given */
  const rule = (q, given) => {
    const grade = (/** @type {number} */ score, /** @type {number} */ of, /** @type {string} */ feedback) => ({ score, of, by: "rule", criteria: [], feedback });
    if (q.kind === "choice") return grade(q.correct.includes(given.index) ? 1 : 0, 1, q.rationales[given.index] ?? "");
    if (q.kind === "blanks") return grade(given.words.filter((w, i) => (q.blanks[i] ?? []).includes(String(w).trim().toLowerCase())).length, q.blanks.length, "");
    if (q.kind === "number") return grade(Math.abs(Number(given.text) - q.number) <= q.tolerance ? 1 : 0, 1, `The answer is ${q.number}.`);
    return null;
  };
  const tally = (/** @type {any} */ attempt) => {
    const marks = attempt.answers.map((/** @type {any} */ a) => a.second ?? a.grade).filter(Boolean);
    attempt.score = marks.reduce((/** @type {number} */ n, /** @type {any} */ g) => n + g.score, 0);
    attempt.of = marks.reduce((/** @type {number} */ n, /** @type {any} */ g) => n + g.of, 0);
  };
  /** A deck, and a stand-in for its schedule: the real one is FSRS, replayed from a log in the core. */
  const card = (/** @type {string} */ id, /** @type {string} */ kind, /** @type {string} */ front, /** @type {string} */ back, /** @type {string[]} */ cites) =>
    ({ id, kind, front, back, topic: "Search", objectives: [], cites, task: "J0003", added: now - 3000, edited: null });
  const deck = {
    cards: [
      { card: card("C001", "basic", "Why is uniform-cost search optimal?", "It expands nodes in order of path cost $g(n)$, so the first goal expanded is the cheapest [[S02 § Search › Uniform cost L40-58]].", ["S02 § Search › Uniform cost L40-58"]),
        state: "review", due: now - 600, recall: 0.86, reviews: 4, missed: 1 },
      { card: card("C002", "cloze", "A heuristic is {{c1::admissible}} when it never overestimates: {{c2::$h(n) \\le h^*(n)$::an inequality}}.", "", ["S01 p. 3"]),
        state: "new", due: null, recall: null, reviews: 0, missed: 0 },
      { card: card("C003", "basic", "What does A* order its frontier by?", "$f(n) = g(n) + h(n)$ [[S01 p. 1]]", ["S01 p. 1"]), state: "new", due: null, recall: null, reviews: 0, missed: 0 },
    ],
    next: 4,
  };
  const deckEvent = () => {
    const counts = { new: deck.cards.filter((r) => r.state === "new").length, learning: 0, review: deck.cards.filter((r) => r.state === "review" && (r.due ?? 0) <= Math.floor(Date.now() / 1000)).length,
      missed: deck.cards.filter((r) => r.missed).length, total: deck.cards.length };
    return { type: "studyDeck", made: studio.made.find((m) => m.id === "D001"), cards: deck.cards, counts, citations: cites };
  };
  /** @type {{mode: string, deck: string | null, queue: string[], done: number, again: number, history: string[]} | null} */
  let session = null;
  const reviewEvent = () => {
    if (!session) return { type: "studyReviewing", review: null };
    const id = session.queue[0];
    const row = deck.cards.find((r) => r.card.id === id);
    const left = { new: deck.cards.filter((r) => r.state === "new" && session?.queue.includes(r.card.id)).length, learning: 0,
      review: deck.cards.filter((r) => r.state !== "new" && session?.queue.includes(r.card.id)).length, missed: deck.cards.filter((r) => r.missed).length, total: deck.cards.length };
    return { type: "studyReviewing", review: {
      deck: session.deck, title: session.deck ? "Flashcards: search" : "COMP3670 — the final exam", mode: session.mode, shuffle: false,
      card: row ? { deck: "D001", deckTitle: "Flashcards: search", card: row.card, state: row.state, intervals: row.state === "new" ? [60, 330, 600, 4 * 86400] : [600, 3 * 86400, 9 * 86400, 21 * 86400] } : null,
      citations: cites, left, remaining: session.mode === "due" ? 0 : session.queue.length, done: session.done, again: session.again, wait: null, undo: session.history.length > 0 } };
  };
  const studioEvent = () => ({ type: "studyStudio", ...studio });
  const cites = state.chat.turns[0].citations;
  const BODIES = /** @type {Record<string, any>} */ ({
    R001: { shape: "prose", markdown: "# COMP3670 — Search, a study guide\n\n## Uninformed search\n\nBreadth-first search is complete and, with unit costs, optimal [[S02 § Search › Uniform cost L40-58]].\n\n## Informed search\n\nA* orders the frontier by $f(n) = g(n) + h(n)$ [[S01 p. 3]].\n\n## Practice questions\n\n1. Why is uniform-cost search optimal? [[S01 p. 1]]" },
    M001: { shape: "map", nodes: [
      { id: "1", parent: null, label: "Search", cites: [] },
      { id: "2", parent: "1", label: "Uninformed", cites: [] }, { id: "3", parent: "1", label: "Informed", cites: [] },
      { id: "4", parent: "2", label: "Breadth-first", cites: ["S02 § Search › Uniform cost L40-58"] }, { id: "5", parent: "2", label: "Uniform cost: order by $g(n)$", cites: ["S01 p. 3"] },
      { id: "6", parent: "3", label: "A*: $f = g + h$", cites: ["S01 p. 1"] }, { id: "7", parent: "6", label: "Admissible $h$", cites: [] },
    ] },
  });

  /** Answer as a tutor would: steps, then the answer in pieces. @param {string} asked */
  function answer(asked) {
    const steps = [{ kind: "list", text: "Looked at the notebook's sources", done: true }, { kind: "search", text: `Searched for “${asked.slice(0, 40)}”`, done: true }];
    const text = "A heuristic is admissible when it never overestimates the cost to reach the goal [[S02 § Search › Uniform cost L40-58]]: $h(n) \\le h^*(n)$.";
    const cites = [state.chat.turns[0].citations[0]];
    const id = state.chat.id;
    const live = (/** @type {any} */ l) => deliver({ type: "studyChatLive", id, live: { asked, steps: [], answer: "", citations: [], thinking: false, ...l } });
    setTimeout(() => live({ steps: steps.slice(0, 1), thinking: true }), 300);
    setTimeout(() => live({ steps }), 800);
    setTimeout(() => live({ steps, answer: text.slice(0, 60), citations: [] }), 1300);
    setTimeout(() => {
      state.chat.turns.push({ asked, answer: text, citations: cites, at: Math.floor(Date.now() / 1000), engine: state.chat.settings.engine || "claude", model: "claude-sonnet-5-5", error: null });
      state.chat.busy = false;
      deliver(chatEvent());
    }, 1900);
  }

  return {
    sync: () => [listEvent(), kbEvent(), notebookEvent(), chatEvent(), studioEvent()],
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
        case "studyAsk":
          state.chat.busy = true;
          answer(command.text);
          return [chatEvent()];
        case "studyTutor":
          state.chat.settings = command.settings;
          return [chatEvent()];
        case "studyChatNew":
          state.chat = { ...state.chat, id: "C002", title: "New chat", turns: [], chats: [...state.chat.chats], lesson: null };
          return [chatEvent()];
        case "studySaveAnswer":
          return [notebookEvent()];
        case "studyMake": {
          const id = `J${String(3 + studio.jobs.length).padStart(4, "0")}`;
          const title = command.options.focus ? `${command.kind}: ${command.options.focus}` : command.kind;
          studio.jobs.unshift({ id, kind: command.kind, title, state: "running", why: null, made: null, engine: "claude", started: Math.floor(Date.now() / 1000) });
          setTimeout(() => {
            const job = studio.jobs.find((j) => j.id === id);
            if (job) Object.assign(job, { state: "done", made: "R001" });
            studio.jobs = studio.jobs.filter((j) => j.id !== id);
            deliver(studioEvent());
          }, 1500);
          return [studioEvent()];
        }
        case "studyShowMade": {
          if (command.id === "D001") return [deckEvent()];
          if (command.id === "Q001") return [quizEvent()];
          if (command.id === "Q002") return [paperEvent()];
          if (command.id === "L001") return [{ type: "studyOverview", made: studio.made.find((m) => m.id === "L001"), deck: "D001", quiz: "Q001", citations: cites, topics: [
            { title: "Uninformed search", summary: "Breadth-first search expands every node at depth $d$ before any at $d + 1$, so it is complete when the branching factor is finite [[S02 § Search › Uniform cost L40-58]].\n\nUniform-cost search orders the frontier by path cost $g(n)$ instead [[S01 p. 3]].",
              map: [{ id: "1", parent: null, label: "Uninformed", cites: [] }, { id: "2", parent: "1", label: "BFS", cites: [] }, { id: "3", parent: "1", label: "UCS: by $g(n)$", cites: ["S01 p. 3"] }],
              cards: deck.cards.slice(0, 2).map((r) => r.card),
              check: [question("k01", "choice", { prompt: "Which is complete?", options: ["DFS", "BFS"], correct: [1], explanation: "BFS goes layer by layer." })] },
            { title: "Informed search", summary: "A* orders the frontier by $f(n) = g(n) + h(n)$; with an admissible $h$ it returns an optimal solution [[S01 p. 1]].",
              map: [], cards: [], check: [question("k01", "blanks", { prompt: "A heuristic that never overestimates is ___.", blanks: [["admissible"]], explanation: "That is what admissible means." })] },
          ] }];
          const made = studio.made.find((m) => m.id === command.id);
          return made ? [{ type: "studyMadeOpen", made, body: BODIES[command.id], citations: cites }] : [];
        }
        case "studyReview": {
          const all = deck.cards.map((r) => r.card.id);
          const queue = command.mode === "due" ? deck.cards.filter((r) => r.state === "new" || (r.due ?? 0) <= Math.floor(Date.now() / 1000)).map((r) => r.card.id)
            : command.mode === "missed" ? deck.cards.filter((r) => r.missed).map((r) => r.card.id) : all;
          session = { mode: command.mode, deck: command.deck, queue, done: 0, again: 0, history: [] };
          return [reviewEvent()];
        }
        case "studyRate": {
          if (!session?.queue.length) return [];
          const id = /** @type {string} */ (session.queue.shift());
          const row = deck.cards.find((r) => r.card.id === id);
          session.done += 1;
          session.history.push(id);
          if (command.rating === "again") {
            session.again += 1;
            session.queue.splice(Math.min(2, session.queue.length), 0, id);
            if (row) row.missed += 1;
          }
          if (row) Object.assign(row, { state: command.rating === "again" ? "learning" : "review", reviews: row.reviews + 1, due: Math.floor(Date.now() / 1000) + 86400, recall: 0.9 });
          return [reviewEvent()];
        }
        case "studyReviewUndo": {
          const id = session?.history.pop();
          if (!session || !id) return [];
          session.done -= 1;
          session.queue.unshift(id);
          return [reviewEvent()];
        }
        case "studyReviewEnd": {
          const scoped = session?.deck;
          session = null;
          return [{ type: "studyReviewing", review: null }, listEvent(), ...(scoped ? [deckEvent()] : [])];
        }
        case "studyCard": {
          if (command.card) {
            const row = deck.cards.find((r) => r.card.id === command.card);
            if (row) Object.assign(row.card, { kind: command.kind, front: command.front, back: command.back, topic: command.topic, edited: Math.floor(Date.now() / 1000) });
          } else {
            const id = `C${String(deck.next++).padStart(3, "0")}`;
            deck.cards.push({ card: { ...card(id, command.kind, command.front, command.back, []), topic: command.topic, task: "" }, state: "new", due: null, recall: null, reviews: 0, missed: 0 });
          }
          return [deckEvent()];
        }
        case "studyCardDelete":
          deck.cards = deck.cards.filter((r) => r.card.id !== command.card);
          return [deckEvent()];
        case "studyExamSit": {
          paper.drafts = {};
          paper.attempts.push({ n: paper.attempts.length + 1, questions: paper.questions.map((q) => q.id), answers: [], started: Math.floor(Date.now() / 1000), finished: null,
            ends: Math.floor(Date.now() / 1000) + command.minutes * 60, score: 0, of: 0 });
          return [paperEvent()];
        }
        case "studyExamExport":
          return [{ type: "notice", level: "info", text: `Written to ~/Github/Notebooks/comp3670-exam/exports/Q002-comp3670-practice-final${command.scheme ? "-scheme" : ""}.${command.html ? "html. Open it in a browser to print it." : "md."}` }];
        case "studyProgress":
          return [{ type: "studyProgress", due: 3, objectives: [
            { objective: "O002", text: "Explain why uniform-cost search is optimal", topic: "Search", level: 0.35, evidence: 3, state: "focus" },
            { objective: "O003", text: "Derive the memory of iterative deepening", topic: "Search", level: 0.7, evidence: 2, state: "developing" },
            { objective: "O004", text: "Prove A* optimal with an admissible $h$", topic: "Heuristics", level: null, evidence: 0, state: "notStarted" },
            { objective: "O001", text: "Say when breadth-first search is complete", topic: "Search", level: 0.92, evidence: 4, state: "strong" },
          ], quizzes: [{ id: "Q001", title: "Quiz: search", kind: "quiz", attempts: 2, best: 0.8, last: 0.6, finished: now - 3600 }] }];
        case "studyLesson":
          state.chat = { ...state.chat, id: "C003", title: "Lesson: Explain why uniform-cost search is optimal", turns: [], busy: true,
            settings: { ...state.chat.settings, style: "guide" }, chats: [...state.chat.chats],
            lesson: { id: command.objective, text: "Explain why uniform-cost search is optimal", topic: "Search", cites: [] } };
          answer("Teach me this");
          return [chatEvent()];
        case "studyQuizStart": {
          const last = quiz.attempts.at(-1);
          const ids = command.missed && last ? last.questions.filter((/** @type {string} */ id) => { const a = last.answers.find((/** @type {any} */ x) => x.question === id); const g = a && (a.second ?? a.grade); return !g || g.score < g.of; })
            : quiz.questions.map((q) => q.id);
          quiz.attempts.push({ n: quiz.attempts.length + 1, questions: ids, answers: [], started: Math.floor(Date.now() / 1000), finished: null, score: 0, of: 0 });
          return [quizEvent()];
        }
        case "studyQuizAnswer": {
          if (command.quiz === "Q002") {
            paper.drafts[command.question] = command.given;
            return [paperEvent()];
          }
          const attempt = quiz.attempts.find((a) => a.n === command.attempt);
          const q = quiz.questions.find((x) => x.id === command.question);
          if (!attempt || !q) return [];
          const answer = { question: q.id, given: command.given, hinted: command.hinted, grade: rule(q, command.given), disputed: null, second: null, unmarked: null, at: Math.floor(Date.now() / 1000) };
          attempt.answers = attempt.answers.filter((/** @type {any} */ a) => a.question !== q.id).concat([answer]);
          tally(attempt);
          if (!answer.grade) {
            const key = `${attempt.n}/${q.id}`;
            quiz.marking.push(key);
            setTimeout(() => {
              quiz.marking = quiz.marking.filter((k) => k !== key);
              answer.grade = { score: 2, of: 3, by: "Codex · gpt-6.1-sol", feedback: "Say why the step costs must be above zero.",
                criteria: [{ id: "c1", points: 2, evidence: [String(command.given.text).split(/[.,;]/)[0]], why: "It names the order." }, { id: "c2", points: 0, evidence: [], why: "Step costs are not mentioned." }] };
              tally(attempt);
              deliver(quizEvent());
            }, 1200);
          }
          return [quizEvent()];
        }
        case "studyQuizDispute": {
          const answer = quiz.attempts.find((a) => a.n === command.attempt)?.answers.find((/** @type {any} */ a) => a.question === command.question);
          if (!answer) return [];
          answer.disputed = command.why;
          const key = `${command.attempt}/${command.question}`;
          quiz.marking.push(key);
          setTimeout(() => {
            quiz.marking = quiz.marking.filter((k) => k !== key);
            answer.second = { ...answer.grade, score: 3, by: "Antigravity · gemini-3.8-pro", feedback: "The second line does say why." };
            tally(quiz.attempts.find((a) => a.n === command.attempt));
            deliver(quizEvent());
          }, 1200);
          return [quizEvent()];
        }
        case "studyQuizMarkAgain":
          return [quizEvent()];
        case "studyQuizFinish": {
          if (command.quiz === "Q002") {
            const sat = paper.attempts.find((a) => a.n === command.attempt);
            if (!sat) return [];
            sat.finished = Math.floor(Date.now() / 1000);
            sat.answers = Object.entries(paper.drafts).map(([id, given]) => {
              const q = paper.questions.find((x) => x.id === id);
              const g = q && rule(q, given);
              const grade = g ? { ...g, score: g.score / g.of * (q.marks ?? g.of), of: q.marks ?? g.of } : null;
              return { question: id, given, hinted: false, grade, disputed: null, second: null, unmarked: null, at: sat.finished };
            });
            for (const a of sat.answers.filter((/** @type {any} */ x) => !x.grade)) {
              const key = `${sat.n}/${a.question}`;
              paper.marking.push(key);
              setTimeout(() => {
                paper.marking = paper.marking.filter((k) => k !== key);
                a.grade = { score: 5, of: 8, by: "Codex · gpt-6.1-sol", feedback: "The last step needs its inequality.", criteria: [] };
                tally(sat);
                deliver(paperEvent());
              }, 1500);
            }
            tally(sat);
            paper.drafts = {};
            return [paperEvent()];
          }
          const attempt = quiz.attempts.find((a) => a.n === command.attempt);
          if (attempt) attempt.finished = Math.floor(Date.now() / 1000);
          return [quizEvent()];
        }
        case "studyDeckExport":
          return [{ type: "notice", level: "info", text: `${deck.cards.length} cards written to ~/Github/Notebooks/comp3670-exam/exports/D001-flashcards-search.${command.anki ? "anki.txt. In Anki: File › Import, and choose it." : "csv."}` }];
        case "studyShowTask":
          return [{ type: "studyTask", id: command.id, title: "Search", engine: "claude", model: "claude-opus-5-5", state: "done", why: null,
            instructions: "The notebook's sources are given to you below, every passage beginning with its citation in double brackets…", prompt: "Map the ideas of these sources…\n\n---\n\nThe sources:\n\n[[S01 p. 1]]\nQuestion 1 …" }];
        case "studyRole":
          for (const s of state.sources[state.open ?? ""] ?? []) if (s.id === command.id) s.role = command.role;
          return [notebookEvent()];
        default:
          return [];
      }
    },
  };
}
