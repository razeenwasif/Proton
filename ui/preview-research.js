/* Investigations for the browser preview: one a round in, with hypotheses
 * reviewed and ranked, and one just made — enough to design the Research
 * tab against — and rounds that play themselves on a timer, so the round's
 * controls and its calls can be watched. The real ones are folders the core
 * reads and engines it drives; this must never grow into a second one. */

const now = Math.floor(Date.now() / 1000);

const PLACEHOLDER = {
  plan: "# Research plan\n\n_Written by the Principal Investigator in the first round, and rewritten after every overview._\n",
  overview: "# Overview\n\n_Where the investigation stands: written by the meta-review at the end of each round._\n",
  verdicts: "# Verdicts\n\n_The Lab Engineer's verifications, newest last._\n",
  sandbox: "# Sandbox log\n\n_Lab runs and what they showed, newest last._\n",
};

const claude = { seat: "claude", model: "claude-opus-5-5" };
const codex = { seat: "codex", model: "gpt-6.1-sol" };
const agy = { seat: "antigravity", model: "gemini-3.1-pro-high" };
const family = { claude: "anthropic", codex: "openai", antigravity: "google", gemini: "google", scientist: "scientist" };

/** @param {string} title @param {string} discipline @param {string} goal */
function brief(title, discipline, goal) {
  return `# ${title}\n\n**Discipline:** ${discipline}\n\n## Goal\n\n${goal}\n\n## Constraints\n\n_Anything the team must keep to: assumptions, methods, what is out of scope._\n`;
}

function sky() {
  const hypotheses = [
    { id: "H001", title: "Rayleigh's λ⁻⁴ law, weighed by the solar spectrum", author: claude, round: 1, status: "active", parents: [],
      statement: "The sky's colour is the scattered spectrum $I_s(\\lambda) \\propto \\lambda^{-4}\\,B(\\lambda, T_\\odot)$ seen through the eye's cone responses; it peaks in the violet, but the cones and the solar spectrum's fall below 450 nm move the perceived hue to blue.",
      rationale: "Molecular scattering by $\\mathrm{N_2}$ and $\\mathrm{O_2}$ is Rayleigh: the scatterers are $\\sim 10^{-3}$ of the wavelength.",
      predictions: ["Measured skylight peaks near 400–420 nm in power per wavelength.", "Projected onto CIE 1931 colour matching functions it lies near 475 nm dominant wavelength."],
      assumptions: ["Single scattering.", "Clean air: no aerosols."] },
    { id: "H002", title: "Ozone's Chappuis band deepens the zenith blue at twilight", author: agy, round: 1, status: "active", parents: [],
      statement: "At solar elevations below 5° the Chappuis absorption of ozone (500–700 nm) removes the yellow–red from the long slant path, so the zenith stays blue when Rayleigh alone would make it grey.",
      rationale: "Hulburt (1953) showed Rayleigh alone predicts a yellowish zenith at sunset.", predictions: ["Zenith colour at sunset is bluer than a Rayleigh-only model by a measurable $\\Delta u'v'$."], assumptions: [] },
    { id: "H003", title: "Violet is missing because the atmosphere absorbs it", author: codex, round: 1, status: "retired", parents: [],
      statement: "Ultraviolet and violet light is absorbed high in the atmosphere, so the scattered light that reaches the ground is blue.",
      rationale: "", predictions: [], assumptions: [] },
    { id: "H004", title: "Cone responses and the solar spectrum together explain the hue", author: codex, round: 1, status: "active", parents: ["H001"],
      statement: "Combining $\\lambda^{-4}$ scattering, the measured solar irradiance and the CIE 1931 2° observer gives a dominant wavelength of 475 ± 5 nm for clear-sky zenith light.",
      rationale: "Evolved from H001: makes its perceptual step quantitative.", predictions: ["Dominant wavelength 470–480 nm."], assumptions: ["CIE 1931 2° observer."] },
  ];
  const reviews = [
    { id: "H001.1", hypothesis: "H001", reviewer: codex, kind: "review", verdict: "strong", correctness: 5, novelty: 2, testability: 5, round: 1,
      summary: "Standard and correct; the perceptual step is asserted rather than computed.", critique: "Quantify the cone weighting — a dominant wavelength would make this testable against a spectroradiometer.", citations: ["Bohren & Fraser, *Colors of the sky*, Phys. Teach. 23 (1985)"] },
    { id: "H001.2", hypothesis: "H001", reviewer: agy, kind: "verification", verdict: "promising", correctness: 4, novelty: 2, testability: 4, round: 1,
      summary: "The scattered spectrum's peak checks out numerically: $\\lambda_{max} \\approx 405\\,\\mathrm{nm}$.", critique: "", citations: [] },
    { id: "H003.1", hypothesis: "H003", reviewer: claude, kind: "review", verdict: "flawed", correctness: 1, novelty: 1, testability: 3, round: 1,
      summary: "Ozone absorbs below ~320 nm; violet at 400–450 nm reaches the ground, so absorption cannot be the reason.", critique: "", citations: [] },
    { id: "H002.1", hypothesis: "H002", reviewer: claude, kind: "review", verdict: "promising", correctness: 4, novelty: 4, testability: 3, round: 1,
      summary: "Right effect, but the claim should be limited to twilight; at noon the Chappuis band is a small correction.", critique: "", citations: ["Hulburt, *J. Opt. Soc. Am.* 43 (1953)"] },
  ];
  const matches = [
    { round: 1, a: "H001", b: "H003", judge: agy, winner: "H001", rationale: "A explains the observation; B contradicts the measured transmission." },
    { round: 1, a: "H004", b: "H002", judge: agy, winner: "H004", rationale: "Both are sound; A answers the question asked." },
    { round: 1, a: "H002", b: "H003", judge: codex, winner: "H002", rationale: "" },
  ];
  return {
    slug: "why-is-the-daytime-sky-blue", title: "Why is the daytime sky blue rather than violet?", discipline: "physics", created: now - 86400, rounds: 1,
    brief: brief("Why is the daytime sky blue rather than violet?", "Physics",
      "Rayleigh scattering favours the shortest wavelengths, so why is the clear daytime sky seen as blue rather than violet? A good answer is quantitative: it predicts the colour a spectroradiometer and a colour-matching observer would report."),
    plan: "# Research plan\n\n1. Establish the scattered spectrum: $I_s(\\lambda) \\propto \\lambda^{-4} B(\\lambda)$.\n2. Weigh it by the CIE 1931 observer and find the dominant wavelength.\n3. Check the twilight case, where ozone matters.\n",
    overview: "# Overview\n\nAfter one round the leading account (**H004**) is quantitative: Rayleigh scattering peaks in the violet, and the eye's cone responses move the perceived hue to $\\approx 475\\,\\mathrm{nm}$. **H003** was retired: ozone does not absorb violet.\n",
    verdicts: "# Verdicts\n\n_The Lab Engineer's verifications, newest last._\n\n## H001 — round 1\n\nThe peak of $\\lambda^{-4}B(\\lambda, 5772\\,\\mathrm{K})$ is at 405 nm. **Promising.**\n",
    sandbox: PLACEHOLDER.sandbox,
    hypotheses, reviews, matches,
  };
}

function cubes() {
  return {
    slug: "sums-of-three-cubes-below-1000", title: "Sums of three cubes below 1000", discipline: "mathematics", created: now - 3600, rounds: 0,
    brief: brief("Sums of three cubes below 1000", "Mathematics",
      "Which $n < 1000$ with $n \\not\\equiv \\pm 4 \\pmod 9$ still lack a known representation $n = x^3 + y^3 + z^3$, and what search would find one?"),
    plan: PLACEHOLDER.plan, overview: PLACEHOLDER.overview, verdicts: PLACEHOLDER.verdicts, sandbox: PLACEHOLDER.sandbox,
    hypotheses: [], reviews: [], matches: [],
  };
}

const SEAT_MODELS = { claude: "claude-opus-5-5", codex: "gpt-6.1-sol", antigravity: "gemini-3.1-pro-high" };
const ROLES = { claude: "investigator", codex: "engineer", antigravity: "sandbox" };

/** A light round, as the supervisor would plan it: each step, its seat,
 *  and what it is about — filled in as the steps before it land. */
const PLAN = [
  ["frame", "claude"],
  ["generate", "claude"], ["generate", "codex"], ["generate", "antigravity"],
  ["reflect", "codex"], ["reflect", "antigravity"], ["reflect", "claude"],
  ["verify", "codex"], ["rank", "antigravity"], ["rank", "claude"],
  ["evolve", "claude"], ["evolve", "codex"],
  ["meta", "claude"],
];
const ORDER = ["frame", "generate", "reflect", "verify", "rank", "evolve", "meta"];
const SEATS_NAME = { claude: "Claude Code", codex: "Codex", antigravity: "Antigravity" };

/** Elo from the matches, as the core works it out: K = 32 from 1200, in order. */
function ratings(matches) {
  /** @type {Map<string, {elo: number, wins: number, losses: number}>} */
  const r = new Map();
  const get = (id) => r.get(id) ?? (r.set(id, { elo: 1200, wins: 0, losses: 0 }), r.get(id));
  for (const m of matches) {
    const a = get(m.a), b = get(m.b);
    const expected = 1 / (1 + 10 ** ((b.elo - a.elo) / 400));
    const score = m.winner === m.a ? 1 : 0;
    a.elo += 32 * (score - expected);
    b.elo -= 32 * (score - expected);
    (score ? a : b).wins++;
    (score ? b : a).losses++;
  }
  return r;
}

export function createResearchPreview(deliver = (/** @type {any} */ _event) => {}) {
  const investigations = [cubes(), sky()];
  let open = investigations[0].slug;
  /** @type {string | null} */
  let showing = null;
  /** The task whose transcript is shown. @type {string | null} */
  let watching = null;
  /** Each investigation's round. @type {Record<string, any>} */
  const rounds = {};
  /** @type {any} */
  let timer = null;
  let nextTask = 1;

  const summary = (i) => ({ slug: i.slug, title: i.title, discipline: i.discipline, created: i.created, hypotheses: i.hypotheses.length, rounds: i.rounds });
  const find = (slug) => investigations.find((i) => i.slug === slug);

  function rows(i) {
    const r = ratings(i.matches);
    return i.hypotheses.map((h) => {
      const rating = r.get(h.id) ?? { elo: 1200, wins: 0, losses: 0 };
      return {
        id: h.id, title: h.title, author: h.author, family: family[h.author.seat], round: h.round, status: h.status,
        elo: Math.round(rating.elo), wins: rating.wins, losses: rating.losses,
        verdicts: i.reviews.filter((v) => v.hypothesis === h.id).map((v) => v.verdict),
        parents: h.parents, duplicateOf: null, experiments: 0,
      };
    }).sort((a, b) => Number(a.status === "retired") - Number(b.status === "retired") || b.elo - a.elo || a.id.localeCompare(b.id));
  }

  function detail(i, id) {
    const h = i.hypotheses.find((x) => x.id === id);
    if (!h) return null;
    const title = (x) => i.hypotheses.find((o) => o.id === x)?.title ?? "";
    return {
      row: rows(i).find((r) => r.id === id), statement: h.statement, rationale: h.rationale, predictions: h.predictions, assumptions: h.assumptions,
      children: i.hypotheses.filter((c) => c.parents.includes(id)).map((c) => c.id),
      reviews: i.reviews.filter((v) => v.hypothesis === id).map(({ hypothesis, ...v }) => v),
      matches: i.matches.filter((m) => m.a === id || m.b === id).map((m) => {
        const opponent = m.a === id ? m.b : m.a;
        return { round: m.round, opponent, opponentTitle: title(opponent), judge: m.judge, won: m.winner === id, rationale: m.rationale };
      }),
      experiments: [], task: null,
    };
  }

  function board() {
    const i = find(open);
    if (!i) return [];
    return [
      { type: "researchBoard", slug: i.slug, title: i.title, discipline: i.discipline, brief: i.brief, plan: i.plan,
        overview: i.overview, verdicts: i.verdicts, sandbox: i.sandbox, feedback: i.feedback ?? "", hypotheses: rows(i) },
      { type: "researchHypothesis", slug: i.slug, detail: showing ? detail(i, showing) : null },
    ];
  }

  /** @param {string} slug */
  function roundOf(slug) {
    return (rounds[slug] ??= { n: find(slug)?.rounds ?? 0, status: "idle", interrupted: false, used: 0, tasks: [], off: new Set() });
  }

  function roundEvent() {
    const i = find(open);
    if (!i) return [];
    const r = roundOf(i.slug);
    const seats = ["claude", "codex", "antigravity"].map((seat) => ({
      seat, role: ROLES[seat], enabled: !r.off.has(seat), why: null, cooling: null,
      running: r.tasks.filter((t) => t.seat === seat && t.state === "running").length,
      model: SEAT_MODELS[seat],
    }));
    const status = r.status === "paused" ? { status: "paused", interrupted: r.interrupted } : { status: r.status };
    return [{ type: "researchRound", slug: i.slug, round: r.n, status, used: r.used, cap: 20, seats,
      tasks: r.tasks.map(({ prompt, answer, ...t }) => t), held: null }];
  }

  /** What a task said, as the transcript draws it. @param {any} t */
  function transcriptOf(t) {
    if (!t) return [];
    const items = [{ kind: "user", id: `${t.id}:you`, text: t.prompt, images: 0 }];
    if (t.state === "ready") return [];
    items.push({ kind: "thinking", id: `${t.id}:think`, text: "", tokens: 412, done: t.state !== "running", parent: null });
    if (["frame", "generate", "reflect"].includes(t.phase)) {
      items.push({ kind: "tool", id: `${t.id}:search`, name: "WebSearch", input: { query: "Rayleigh scattering solar spectrum perceived hue" },
        state: t.state === "running" ? "running" : "done", streamed: 0, result: null, detail: null, parent: null, ask: null, artifact: null });
    }
    if (t.state === "done") {
      items.push({ kind: "text", id: `${t.id}:answer`, text: t.answer, done: true, parent: null });
      items.push({ kind: "turn", id: `${t.id}:turn`, ms: (t.ended - t.started) * 1000, turns: 1, cost: 0, error: null });
    }
    return items;
  }

  function transcriptEvent() {
    const r = rounds[open];
    const t = r?.tasks.find((/** @type {any} */ x) => x.id === watching);
    return watching && t ? [{ type: "researchTranscript", slug: open, task: watching, items: transcriptOf(t) }] : [];
  }

  /** A step lands: what it wrote. @param {any} i @param {any} r @param {any} t */
  function land(i, r, t) {
    const now = Math.floor(Date.now() / 1000);
    const next = () => `H${String(i.hypotheses.length + 1).padStart(3, "0")}`;
    const by = { seat: t.seat, model: SEAT_MODELS[t.seat] };
    const mine = (/** @type {string} */ seat) => i.hypotheses.filter((/** @type {any} */ h) => h.round === r.n && h.author.seat === seat && !h.parents.length);
    switch (t.phase) {
      case "frame":
        i.plan = "# Research plan\n\n1. Decide which representations a search could still find.\n2. Bound the search by $|x|, |y|, |z| < 10^{16}$.\n";
        t.answer = "The plan: find the open cases, then bound the search.";
        break;
      case "generate":
        for (const k of [0, 1]) {
          const id = next();
          i.hypotheses.push({ id, title: `${SEATS_NAME[t.seat]}'s idea ${k + 1}`, author: by, round: r.n, status: "active", parents: [],
            statement: k ? "A sieve modulo $9 \\cdot 7 \\cdot 13$ rules out most triples before any cube is computed." :
              "Elliptic-curve parametrisations give families of solutions for $n = 2 m^3$.",
            rationale: "", predictions: ["The search finishes in under a CPU-week."], assumptions: [] });
          t.targets.push(id);
        }
        t.answer = `Two hypotheses: ${t.targets.join(" and ")}.`;
        break;
      case "reflect": {
        const others = { codex: "claude", antigravity: "codex", claude: "antigravity" };
        const targets = mine(others[t.seat]).map((/** @type {any} */ h) => h.id);
        t.targets = targets;
        for (const id of targets) {
          i.reviews.push({ id: `${id}.${i.reviews.filter((/** @type {any} */ v) => v.hypothesis === id).length + 1}`, hypothesis: id, reviewer: by, kind: "review",
            verdict: id.endsWith("1") ? "strong" : "promising", correctness: 4, novelty: 3, testability: 4, round: r.n, summary: "Sound, and testable.", critique: "", citations: [] });
        }
        t.answer = `Reviewed ${targets.join(", ")}.`;
        break;
      }
      case "verify":
        t.targets = [i.hypotheses[0]?.id].filter(Boolean);
        i.verdicts += `\n## ${t.targets[0]}, round ${r.n} — Strong\n\nThe congruence argument holds step by step.\n`;
        t.answer = "Verified.";
        break;
      case "rank": {
        const ids = i.hypotheses.filter((/** @type {any} */ h) => h.status !== "retired").map((/** @type {any} */ h) => h.id);
        const half = t.seat === "antigravity" ? 0 : 2;
        const pairs = [[ids[half], ids[half + 1]], [ids[half + 2], ids[half + 3]]].filter((p) => p[0] && p[1]);
        t.targets = pairs.map((p) => `${p[0]} v ${p[1]}`);
        for (const [a, b] of pairs) i.matches.push({ round: r.n, a, b, judge: by, winner: a, rationale: "Sharper, and checkable." });
        t.answer = `Judged ${pairs.length} matches.`;
        break;
      }
      case "evolve": {
        const top = rows(i).filter((h) => h.status !== "retired").slice(0, t.seat === "claude" ? 1 : 2).map((h) => h.id);
        const id = next();
        i.hypotheses.push({ id, title: t.seat === "claude" ? "The leader, sharpened" : "The top two, combined", author: by, round: r.n, status: "active",
          parents: top, statement: "Sieve first, then lift the survivors along the curve's rational points.", rationale: "", predictions: [], assumptions: [] });
        t.targets = top;
        t.answer = `Evolved ${top.join(" and ")} into ${id}.`;
        break;
      }
      case "meta":
        i.overview = `# Overview — round ${r.n}\n\nThe sieve leads. Next: price the lift on a real machine.\n`;
        i.plan = `# Research plan\n\n_Rewritten after round ${r.n}._\n\nPrice the lift; then run it.\n`;
        t.answer = "The overview is written and the plan rewritten.";
        break;
    }
    t.state = "done";
    t.ended = now;
  }

  /** One beat of a running round: one call lands, and what can start starts. */
  function beat() {
    const i = find(Object.keys(rounds).find((slug) => rounds[slug].status === "running") ?? "");
    if (!i) {
      clearInterval(timer);
      timer = null;
      return;
    }
    const r = rounds[i.slug];
    const running = r.tasks.find((/** @type {any} */ t) => t.state === "running");
    if (running) land(i, r, running);
    if (r.status === "running") {
      const now = Math.floor(Date.now() / 1000);
      // A step starts once every step before it has landed; two to an engine.
      for (const t of r.tasks.filter((/** @type {any} */ t) => t.state === "ready")) {
        const before = r.tasks.filter((/** @type {any} */ x) => ORDER.indexOf(x.phase) < ORDER.indexOf(t.phase));
        if (before.some((/** @type {any} */ x) => x.state !== "done")) break;
        if (r.tasks.filter((/** @type {any} */ x) => x.seat === t.seat && x.state === "running").length >= 2) continue;
        t.state = "running";
        t.started = now;
        r.used += 1;
      }
    }
    if (r.tasks.every((/** @type {any} */ t) => t.state === "done")) {
      r.status = "idle";
      i.rounds = r.n;
    }
    if (i.slug === open) {
      for (const event of [...roundEvent(), ...board(), ...transcriptEvent(), { type: "researchList", investigations: investigations.map(summary), open }]) deliver(event);
    }
  }

  function run() {
    if (!timer) timer = setInterval(beat, 700);
  }

  function sync() {
    return [{ type: "researchList", investigations: investigations.map(summary), open }, ...board(), ...roundEvent()];
  }

  /** @param {any} command */
  function command(command) {
    switch (command.type) {
      case "researchNew": {
        const title = String(command.title ?? "").trim();
        if (!title) return [{ type: "notice", level: "error", text: "An investigation needs a title." }];
        let slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "investigation";
        for (let n = 2; find(slug); n++) slug = `${slug.replace(/-\d+$/, "")}-${n}`;
        const name = { physics: "Physics", mathematics: "Mathematics", chemistry: "Chemistry", biology: "Biology", computerScience: "Computer Science" }[command.discipline] ?? "Other";
        investigations.unshift({ slug, title, discipline: command.discipline, created: Math.floor(Date.now() / 1000), rounds: 0,
          brief: brief(title, name, String(command.goal ?? "").trim() || "_What the team should find out._"),
          ...PLACEHOLDER, hypotheses: [], reviews: [], matches: [] });
        open = slug;
        showing = null;
        watching = null;
        return sync();
      }
      case "researchOpen":
        if (!find(command.slug)) return [{ type: "notice", level: "error", text: `No investigation ${command.slug}.` }];
        open = command.slug;
        showing = null;
        watching = null;
        return sync();
      case "researchShow":
        showing = command.hypothesis ?? null;
        return board();
      case "researchStart": {
        const r = roundOf(open);
        if (Object.values(rounds).some((x) => x.status === "running")) {
          return [{ type: "notice", level: "warn", text: "A round is running. Pause or stop it first: one round at a time." }];
        }
        r.n += 1;
        r.status = "running";
        r.used = 0;
        r.tasks = PLAN.filter(([, seat]) => !r.off.has(seat)).map(([phase, seat]) => ({
          id: `T${String(nextTask++).padStart(4, "0")}`, round: r.n, phase, seat, role: ROLES[seat], targets: [], state: "ready",
          why: null, model: SEAT_MODELS[seat], attempts: 0, started: null, ended: null,
          prompt: `## ${phase}\n\nThe investigation: ${find(open)?.title}.`, answer: "",
        }));
        run();
        beat();
        return [];
      }
      case "researchPause": {
        const r = roundOf(open);
        if (r.status === "running") r.status = "paused";
        return roundEvent();
      }
      case "researchResume": {
        const r = roundOf(open);
        if (r.status !== "paused") return [];
        r.status = "running";
        r.interrupted = false;
        run();
        return roundEvent();
      }
      case "researchStop": {
        const r = roundOf(open);
        for (const t of r.tasks) if (t.state === "ready" || t.state === "running") t.state = "cancelled";
        r.status = "idle";
        return roundEvent();
      }
      case "researchTask":
        watching = command.task ?? null;
        return transcriptEvent();
      case "researchFeedback": {
        const i = find(open);
        if (!i) return [];
        const r = roundOf(open);
        const when = r.status === "idle" ? `Before round ${r.n + 1}` : `During round ${r.n}`;
        i.feedback = `${i.feedback ?? "# Feedback\n\n_What you told the team, newest last._\n"}\n## ${when}\n\n${command.text}\n`;
        return [...board(), { type: "notice", level: "info", text: "Told the team. Every task from now on is shown it." }];
      }
      case "researchAddHypothesis": {
        const i = find(open);
        if (!i) return [];
        const r = roundOf(open);
        const id = `H${String(i.hypotheses.length + 1).padStart(3, "0")}`;
        i.hypotheses.push({ id, title: command.title, author: { seat: "scientist", model: "" }, round: r.status === "idle" ? r.n + 1 : r.n,
          status: "active", parents: [], statement: command.statement, rationale: "", predictions: [], assumptions: [] });
        return [{ type: "researchList", investigations: investigations.map(summary), open }, ...board(),
          { type: "notice", level: "info", text: `${id} is yours: it is reviewed and played with the others.` }];
      }
      case "researchSetStatus": {
        const h = find(open)?.hypotheses.find((/** @type {any} */ x) => x.id === command.hypothesis);
        if (h) h.status = command.status;
        return board();
      }
      case "researchSeat": {
        const r = roundOf(open);
        if (command.enabled) r.off.delete(command.seat);
        else r.off.add(command.seat);
        return roundEvent();
      }
      default:
        return [];
    }
  }

  return { sync, command };
}
