/* Investigations for the browser preview: one a round in, with hypotheses
 * reviewed and ranked, and one just made — enough to design the Research
 * tab against. The real ones are folders the core reads; this must never
 * grow into a second one. */

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

export function createResearchPreview() {
  const investigations = [cubes(), sky()];
  let open = investigations[0].slug;
  /** @type {string | null} */
  let showing = null;

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
        overview: i.overview, verdicts: i.verdicts, sandbox: i.sandbox, hypotheses: rows(i) },
      { type: "researchHypothesis", slug: i.slug, detail: showing ? detail(i, showing) : null },
    ];
  }

  function sync() {
    return [{ type: "researchList", investigations: investigations.map(summary), open }, ...board()];
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
        return sync();
      }
      case "researchOpen":
        if (!find(command.slug)) return [{ type: "notice", level: "error", text: `No investigation ${command.slug}.` }];
        open = command.slug;
        showing = null;
        return sync();
      case "researchShow":
        showing = command.hypothesis ?? null;
        return board();
      default:
        return [];
    }
  }

  return { sync, command };
}
