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

/** A small protein made up for the preview, as a PDB file: a helix, two
 *  strands side by side, another helix, with its HELIX and SHEET records, a
 *  sulfate bound beside it and three waters. */
function previewFold() {
  /** @type {{p: number[], ss: string, axis?: number[]}[]} */
  const cas = [];
  const helix = (/** @type {number[]} */ at, /** @type {number} */ dir, /** @type {number} */ n) => {
    for (let k = 0; k < n; k++) {
      const t = (k * 100 * Math.PI) / 180;
      cas.push({ p: [at[0] + 2.3 * Math.cos(t), at[1] + 2.3 * Math.sin(t), at[2] + dir * 1.5 * k], ss: "H", axis: [0, 0, dir] });
    }
  };
  const strand = (/** @type {number} */ x, /** @type {number} */ z, /** @type {number} */ dir, /** @type {number} */ n) => {
    for (let k = 0; k < n; k++) cas.push({ p: [x + (k % 2 ? 0.9 : -0.9), 0, z + dir * 3.3 * k], ss: "E" });
  };
  /** A loop from the last α-carbon to `to`, bowed outward. @param {number[]} to */
  const loop = (to) => {
    const from = cas[cas.length - 1].p;
    const n = Math.max(1, Math.ceil(Math.hypot(to[0] - from[0], to[1] - from[1], to[2] - from[2]) / 3.75));
    for (let k = 1; k < n; k++) {
      const f = k / n;
      cas.push({ p: from.map((v, i) => v + (to[i] - v) * f + (i === 1 ? Math.sin(f * Math.PI) * 2 : 0)), ss: "C" });
    }
  };
  helix([0, 0, 0], 1, 16);
  loop([9.1, 0, 26]);
  const s1 = cas.length;
  strand(10, 26, -1, 7);
  loop([15.1, 0, 6.2]);
  const s2 = cas.length;
  strand(16, 6.2, 1, 7);
  // Over the second helix's top, so the loop does not run into its side.
  loop([23, 0, 31.5]);
  cas.push({ p: [23, 0, 31.5], ss: "C" });
  loop([24.3, 0, 28]);
  const h2 = cas.length;
  helix([22, 0, 28], -1, 14);
  const NAMES = ["ALA", "LEU", "GLU", "LYS", "VAL", "SER", "GLY", "THR", "ILE", "ASP"];
  const lines = [];
  let serial = 1;
  const atom = (/** @type {string} */ name, /** @type {string} */ res, /** @type {number} */ seq, /** @type {number[]} */ p, /** @type {string} */ el, het = false) =>
    lines.push(`${(het ? "HETATM" : "ATOM").padEnd(6)}${String(serial++).padStart(5)} ${(name.length < 4 ? ` ${name}` : name).padEnd(4)} ${res.padStart(3)} ${het ? "B" : "A"}${String(seq).padStart(4)}    ` +
      `${p.map((v) => v.toFixed(3).padStart(8)).join("")}  1.00 20.00          ${el.padStart(2)}`);
  const sub = (/** @type {number[]} */ a, /** @type {number[]} */ b) => a.map((v, i) => v - b[i]);
  const unit = (/** @type {number[]} */ v) => { const l = Math.hypot(...v) || 1; return v.map((x) => x / l); };
  const along = (/** @type {number[]} */ p, /** @type {number[]} */ v, /** @type {number} */ f) => p.map((x, i) => x + v[i] * f);
  cas.forEach((r, i) => {
    const next = cas[i + 1]?.p ?? along(r.p, sub(r.p, cas[i - 1].p), 1);
    const d = sub(next, r.p);
    const dh = unit(d);
    // Its C=O: along a helix's axis, across a strand by turns, else upward.
    const want = r.axis ?? (r.ss === "E" ? [i % 2 ? -1 : 1, 0, 0] : [0, 1, 0]);
    const dot = want.reduce((s, v, k) => s + v * dh[k], 0);
    const o = unit(want.map((v, k) => v - dot * dh[k]));
    const res = NAMES[i % NAMES.length];
    const seq = i + 1;
    if (i === 0) atom("N", res, seq, along(r.p, dh, -1.45), "N");
    // Its N on the side away from its own O.
    else atom("N", res, seq, along(along(cas[i - 1].p, sub(r.p, cas[i - 1].p), 0.67), o, -0.3), "N");
    atom("CA", res, seq, r.p, "C");
    const c = along(along(r.p, d, 0.33), o, 0.45);
    atom("C", res, seq, c, "C");
    atom("O", res, seq, along(c, o, 1.23), "O");
  });
  // A sulfate: sulfur at the middle of a tetrahedron of oxygens, 1.49 Å out.
  const S = [6, 7, 12];
  atom("S", "SO4", 101, S, "S", true);
  [[1, 1, 1], [1, -1, -1], [-1, 1, -1], [-1, -1, 1]].forEach((v, k) => atom(`O${k + 1}`, "SO4", 101, along(S, unit(v), 1.49), "O", true));
  [[-6, 2, 4], [-5, -4, 16], [30, 3, 10]].forEach((p, k) => atom("O", "HOH", 201 + k, p, "O", true));
  const pad = (/** @type {number} */ n, /** @type {number} */ w) => String(n).padStart(w);
  const resAt = (/** @type {number} */ i) => NAMES[i % NAMES.length];
  const helixLine = (/** @type {number} */ n, /** @type {number} */ from, /** @type {number} */ to) =>
    `HELIX  ${pad(n, 3)} ${pad(n, 3)} ${resAt(from)} A ${pad(from + 1, 4)}  ${resAt(to)} A ${pad(to + 1, 4)}  1`;
  const sheetLine = (/** @type {number} */ n, /** @type {number} */ from, /** @type {number} */ to) =>
    `SHEET  ${pad(n, 3)}   A 2 ${resAt(from)} A${pad(from + 1, 4)}  ${resAt(to)} A${pad(to + 1, 4)}  ${n === 1 ? " 0" : "-1"}`;
  return ["HEADER    MADE UP FOR THE PREVIEW", "TITLE     A SMALL ALPHA/BETA FOLD",
    helixLine(1, 0, 15), helixLine(2, h2, h2 + 13), sheetLine(1, s1, s1 + 6), sheetLine(2, s2, s2 + 6),
    ...lines, "END", ""].join("\n");
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

/** Three cubes: one hypothesis proved in Lean, one stated with its proof open. */
function cubes() {
  const hypotheses = [
    { id: "H001", title: "No sum of three cubes is 4 or 5 modulo 9", author: claude, round: 1, status: "active", parents: [],
      statement: "For all integers $x, y, z$, $x^3 + y^3 + z^3 \\not\\equiv 4, 5 \\pmod 9$: a cube is $0$ or $\\pm 1 \\pmod 9$, and no three of those sum to $\\pm 4$.",
      rationale: "The classical obstruction: it settles which $n$ cannot be a sum of three cubes at all.", predictions: ["No representation exists for $n = 4, 5, 13, 14, \\ldots$"], assumptions: [] },
    { id: "H002", title: "Every other n below 1000 is a sum of three cubes", author: codex, round: 1, status: "active", parents: [],
      statement: "Every $0 < n < 1000$ with $n \\not\\equiv \\pm 4 \\pmod 9$ is $x^3 + y^3 + z^3$ for some integers $x, y, z$.",
      rationale: "Booker (2019) found 33 and Booker and Sutherland (2021) found 42; the rest below 1000 have smaller solutions.",
      predictions: ["A search to $|x|, |y|, |z| \\le 10^{16}$ finds each."], assumptions: [] },
  ];
  const reviews = [
    { id: "H001.1", hypothesis: "H001", reviewer: agy, kind: "verification", verdict: "strong", correctness: 5, novelty: 1, testability: 5, round: 1,
      summary: "Cubes modulo 9 are 0, 1 and 8, and no three of them sum to 4 or 5.", critique: "", citations: [] },
    { id: "H001.2", hypothesis: "H001", reviewer: codex, kind: "formal", verdict: "strong", correctness: 5, novelty: 3, testability: 3, round: 1,
      summary: "Lean proves it, as `∀ (x y z : ℤ), (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 4 ∧ (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 5`.",
      critique: "The statement is the hypothesis's, residue by residue.\n\nChecked by Lean v4.34.1 with Mathlib v4.34.1, on propext, Classical.choice, Quot.sound. The file is `proofs/H001.lean`.",
      citations: [] },
  ];
  const proofs = {
    H001: {
      record: { hypothesis: "H001", outcome: "proved", kind: "proof",
        statement: "∀ (x y z : ℤ), (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 4 ∧ (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 5",
        axioms: ["propext", "Classical.choice", "Quot.sound"],
        notes: "The statement is the hypothesis's: for every $x, y, z$ the sum of their cubes leaves neither 4 nor 5 modulo 9.",
        prover: codex, round: 1, task: "T0011", attempts: 2, attempt: 2, lean: "v4.34.1", mathlib: "v4.34.1", messages: [], at: now - 1800 },
      lean: "import Mathlib\n\n/-- A cube is 0, 1 or 8 modulo 9. -/\nlemma cube_mod_nine (t : ℤ) : t ^ 3 % 9 = 0 ∨ t ^ 3 % 9 = 1 ∨ t ^ 3 % 9 = 8 := by\n" +
        "  have h : t % 9 = 0 ∨ t % 9 = 1 ∨ t % 9 = 2 ∨ t % 9 = 3 ∨ t % 9 = 4 ∨ t % 9 = 5 ∨ t % 9 = 6 ∨ t % 9 = 7 ∨ t % 9 = 8 := by omega\n" +
        "  rcases h with h | h | h | h | h | h | h | h | h <;> simp [pow_succ, Int.mul_emod, h]\n\n" +
        "theorem H001 (x y z : ℤ) :\n    (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 4 ∧ (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 5 := by\n" +
        "  rcases cube_mod_nine x with hx | hx | hx <;> rcases cube_mod_nine y with hy | hy | hy <;>\n    rcases cube_mod_nine z with hz | hz | hz <;> omega\n",
    },
    H002: {
      record: { hypothesis: "H002", outcome: "open", kind: "proof",
        statement: "∀ (n : ℤ), 0 < n → n < 1000 → n % 9 ≠ 4 → n % 9 ≠ 5 → ∃ x y z, x ^ 3 + y ^ 3 + z ^ 3 = n",
        axioms: ["propext", "sorryAx", "Classical.choice", "Quot.sound"],
        notes: "Stated as the hypothesis is. The witnesses for 33 and 42 are too large to check here, so the cases are left open.",
        prover: claude, round: 1, task: "T0013", attempts: 3, attempt: 5, lean: "v4.34.1", mathlib: "v4.34.1",
        messages: [{ severity: "warning", line: 3, column: 8, text: "declaration uses 'sorry'" }], at: now - 900 },
      lean: "import Mathlib\n\ntheorem H002 : ∀ n : ℤ, 0 < n → n < 1000 → n % 9 ≠ 4 → n % 9 ≠ 5 →\n    ∃ x y z : ℤ, x ^ 3 + y ^ 3 + z ^ 3 = n := by\n  sorry\n",
    },
  };
  return {
    slug: "sums-of-three-cubes-below-1000", title: "Sums of three cubes below 1000", discipline: "mathematics", created: now - 3600, rounds: 1,
    brief: brief("Sums of three cubes below 1000", "Mathematics",
      "Which $n < 1000$ with $n \\not\\equiv \\pm 4 \\pmod 9$ still lack a known representation $n = x^3 + y^3 + z^3$, and what search would find one?"),
    plan: PLACEHOLDER.plan, overview: PLACEHOLDER.overview, sandbox: PLACEHOLDER.sandbox,
    verdicts: "# Verdicts\n\n_The Lab Engineer's verifications, newest last._\n\n## H001, round 1 — Lean: proved\n\n_Codex (gpt-6.1-sol), in 2 checks; Lean v4.34.1, Mathlib v4.34.1._\n\n" +
      "**What Lean proved.**\n\n```lean\n∀ (x y z : ℤ), (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 4 ∧ (x ^ 3 + y ^ 3 + z ^ 3) % 9 ≠ 5\n```\n\nThe file is `proofs/H001.lean`.\n",
    hypotheses, reviews, matches: [], proofs,
  };
}

const SEAT_MODELS = { claude: "claude-opus-5-5", codex: "gpt-6.1-sol", antigravity: "gemini-3.1-pro-high" };

/** The sky's literature: two papers a round found, paywalled, and one the
 *  scientist added with a PDF of their own. */
function skyPapers() {
  return [
    { id: "P001", title: "On the light from the sky, its polarization and colour", authors: ["Lord Rayleigh"], authorsMore: 0, year: 1871,
      venue: "The London, Edinburgh, and Dublin Philosophical Magazine and Journal of Science", doi: "10.1080/14786447108640452", arxiv: null,
      abstract: "", tldr: "", access: "closed", hasPdf: false, pdfFrom: null, pdfUrl: null, by: "round", round: 1, citations: 1830,
      queries: ["rayleigh scattering sky colour"], sources: ["openAlex", "semanticScholar"], citedBy: [],
      pdfNote: "It is behind a paywall, as far as the sources know." },
    { id: "P002", title: "Explanation of the Brightness and Color of the Sky, Particularly the Twilight Sky", authors: ["E. O. Hulburt"], authorsMore: 0, year: 1953,
      venue: "Journal of the Optical Society of America", doi: "10.1364/JOSA.43.000113", arxiv: null,
      abstract: "Calculations of the brightness and color of the sky by Rayleigh scattering alone give a yellowish zenith at sunset; with the absorption of **ozone** in the Chappuis band the zenith stays blue, as it is observed.",
      tldr: "Ozone's Chappuis absorption, not Rayleigh scattering alone, keeps the twilight zenith blue.", access: "closed", hasPdf: false, pdfFrom: null, pdfUrl: null,
      by: "round", round: 1, citations: 162, queries: ["ozone chappuis twilight zenith"], sources: ["semanticScholar", "openAlex"],
      citedBy: [{ by: "H002.1", hypothesis: "H002", kind: "review" }], pdfNote: "It is behind a paywall, as far as the sources know." },
    { id: "P003", title: "Human color vision and the unsaturated blue color of the daytime sky", authors: ["Glenn S. Smith"], authorsMore: 0, year: 2005,
      venue: "American Journal of Physics", doi: null, arxiv: null,
      abstract: "The scattered spectrum peaks in the violet, yet the sky is seen as an unsaturated blue: the eye's cone responses and the solar spectrum together set the hue.",
      tldr: "", access: "unknown", hasPdf: true, pdfFrom: null, pdfUrl: null, by: "scientist", round: null, citations: 41,
      queries: [], sources: ["openAlex"], citedBy: [], pdfNote: null },
  ];
}

/** What the preview's sources find, whatever is asked: enough to draw. */
const FOUND = [
  { title: "Colors of the sky", authors: "Bohren and Fraser", year: 1985, venue: "The Physics Teacher", citations: 74, access: "unknown", pdf: true,
    abstract: "Why the sky is blue, why sunsets are red, and why neither answer is as simple as it is usually made: multiple scattering, the observer, and the solar spectrum." },
  { title: "Rayleigh scattering", authors: "Miles et al.", year: 2001, venue: "Measurement Science and Technology", citations: 590, access: "open", pdf: true,
    abstract: "A review of laser Rayleigh scattering: its cross-sections, depolarisation, and its use to measure density and temperature in gases." },
  { title: "On the molecular scattering of light in water and the colour of the sea", authors: "Raman", year: 1922, venue: "Proceedings of the Royal Society A", citations: 51, access: "open", pdf: false,
    abstract: "" },
  { title: "Explanation of the Brightness and Color of the Sky, Particularly the Twilight Sky", authors: "Hulburt", year: 1953, venue: "Journal of the Optical Society of America",
    citations: 162, access: "closed", pdf: false, have: "P002", abstract: "Calculations of the brightness and color of the sky by Rayleigh scattering alone give a yellowish zenith at sunset." },
];
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

  /* The Lab's Python: missing until planned and made, each on a timer. */
  const PREFIX = "/Users/you/.proton/lab/envs/lab";
  const SPECS = ["python=3.12", "numpy", "scipy", "sympy", "mpmath", "matplotlib-base", "networkx", "pandas"];
  /** @type {any} */
  const MAKER = { kind: "conda", conda: "/Users/you/miniconda3/bin/conda" };
  let env = { state: "missing", maker: MAKER, index: now - 86400 * 9, specs: SPECS, prefix: PREFIX };
  /** @type {any} */
  let labTimer = null;
  const labEvent = () => ({ type: "labEnv", env });

  /* Lean and Mathlib: missing until planned and made, each on a timer. */
  const LEAN_HOME = "/Users/you/.proton/lean";
  const LEAN_MISSING = { state: "missing", needs: [], free: 88_000_000_000, home: LEAN_HOME };
  /** @type {any} */
  let lean = LEAN_MISSING;
  /** @type {any} */
  let leanTimer = null;
  const leanEvent = () => ({ type: "leanEnv", env: lean });
  const leanPlan = () => {
    const downloads = [
      { what: "elan v4.2.4", host: "github.com", bytes: 2_190_845, exact: true, verified: true },
      { what: "Lean v4.34.1 toolchain", host: "github.com", bytes: 562_038_070, exact: true, verified: false },
      { what: "Mathlib v4.34.1 sources", host: "github.com", bytes: 700_000_000, exact: false, verified: false },
      { what: "Mathlib's prebuilt library", host: "cache.mathlib.org", bytes: 2_400_000_000, exact: false, verified: false },
    ];
    return { lean: "v4.34.1", mathlib: "v4.34.1", elan: "v4.2.4", toolchain: "leanprover/lean4:v4.34.1", downloads,
      download: downloads.reduce((n, d) => n + d.bytes, 0), disk: 17_586_826_879, kept: 14_622_597_964, free: 88_000_000_000, fits: true,
      home: LEAN_HOME, git: "/opt/homebrew/bin/git", elanUrl: "https://github.com/leanprover/elan/releases/download/v4.2.4/elan-aarch64-apple-darwin.tar.gz",
      elanSha256: null };
  };

  /* Experiments and runs, by investigation. */
  const SPECTRUM = `import numpy as np
from proton_lab import metric, figure
import matplotlib.pyplot as plt

h, c, k = 6.62607015e-34, 2.99792458e8, 1.380649e-23
lam = np.linspace(300e-9, 800e-9, 2001)
planck = 2 * h * c**2 / lam**5 / (np.exp(h * c / (lam * k * 5772)) - 1)
sky = planck * lam**-4
peak = lam[np.argmax(sky)] * 1e9
metric("peak_nm", round(float(peak), 1), "nm")
plt.plot(lam * 1e9, sky / sky.max())
plt.xlabel("wavelength (nm)")
figure("spectrum")
print(f"scattered spectrum peaks at {peak:.1f} nm")`;
  /** @type {Record<string, any[]>} */
  const experiments = {
    "why-is-the-daytime-sky-blue": [
      { id: "E002", hypothesis: "H004", proposer: { seat: "codex", model: "gpt-6.1-sol" }, round: 1, status: "proposed", run: null,
        purpose: "Weigh the scattered spectrum by the CIE 1931 observer and find its dominant wavelength.", expectation: "Between 470 and 480 nm.",
        code: SPECTRUM.replace("peak_nm", "dominant_nm") },
      { id: "E001", hypothesis: "H001", proposer: { seat: "claude", model: "claude-opus-5-5" }, round: 1, status: "done", run: "R001",
        purpose: "Integrate the scattered spectrum and find its peak.", expectation: "A peak below 450 nm.", code: SPECTRUM },
    ],
  };
  /** @type {Record<string, any[]>} */
  const runs = {
    "why-is-the-daytime-sky-blue": [
      { row: { id: "R001", experiment: "E001", hypothesis: "H001", state: "done", exit: 0, seconds: 2.4, started: now - 3600, sandbox: "seatbelt" },
        stdout: "scattered spectrum peaks at 405.2 nm", stdoutCut: false, stderr: "", stderrCut: false,
        metrics: { peak_nm: { value: 405.2, unit: "nm" } }, figures: ["figures/spectrum.png"], scenes: ["orbit.scene.json"],
        molecules: ["caffeine.smi", "fold.pdb", "ozone.xyz"],
        record: { code: { sha256: "9f2c1e7a0b4d5e6f" }, python: "/Users/you/.proton/lab/envs/lab/bin/python",
          environment: { lock: { sha256: "4b1d3c2a9e8f7d6c" } }, approved: undefined, about: { approved: { by: "scientist", at: now - 3600 } }, limits: { seconds: 600 } } },
    ],
  };
  /** Where the investigations are kept, as the window says it. */
  let kept = "research/";
  /** @type {string | null} */
  let showingRun = null;
  /** Investigations that run proposals unasked. @type {Set<string>} */
  const autoRun = new Set();

  /* The literature, by investigation: whether it is searched, in which
     sources, and its papers; the paper shown, the last search, and what is
     being downloaded and looked up. */
  const SOURCES = [["arxiv", "arXiv"], ["semanticScholar", "Semantic Scholar"], ["openAlex", "OpenAlex"]];
  /** @type {Record<string, {enabled: boolean, sources: Set<string>, papers: any[]}>} */
  const lit = {};
  const litOf = (/** @type {string} */ slug) => (lit[slug] ??= {
    enabled: true, sources: new Set(SOURCES.map(([s]) => s)), papers: slug === "why-is-the-daytime-sky-blue" ? skyPapers() : [],
  });
  /** @type {string | null} */
  let showingPaper = null;
  /** @type {any} */
  let results = null;
  /** @type {Set<string>} */
  const fetching = new Set();
  /** @type {string[]} */
  const adding = [];
  const KEYS = { arxiv: "notNeeded", semanticScholar: "found", openAlex: "missing" };
  const short = (/** @type {string[]} */ a) => a.length === 1 ? a[0].split(" ").pop() : a.length === 2 ? `${a[0].split(" ").pop()} and ${a[1].split(" ").pop()}` : `${a[0].split(" ").pop()} et al.`;
  const paperRow = (/** @type {any} */ p) => ({ id: p.id, title: p.title, authors: short(p.authors), year: p.year, venue: p.venue, access: p.access,
    hasPdf: p.hasPdf, by: p.by, round: p.round, cited: p.citedBy.length, sources: p.sources });
  const paperDetail = (/** @type {any} */ p) => ({ row: paperRow(p), authors: p.authors, authorsMore: p.authorsMore, doi: p.doi, arxiv: p.arxiv,
    abstract: p.abstract, tldr: p.tldr, url: p.doi ? `https://doi.org/${p.doi}` : null, pdfUrl: p.pdfUrl, pdfFrom: p.hasPdf ? p.pdfFrom : null,
    pdfNote: p.hasPdf ? null : p.pdfNote, queries: p.queries, citations: p.citations, citedBy: p.citedBy, extra: [] });
  function literatureEvents() {
    const l = litOf(open);
    const sources = SOURCES.map(([source, name]) => ({ source, name, chosen: l.sources.has(source), key: KEYS[source], resting: null }));
    const shown = l.papers.find((p) => p.id === showingPaper);
    return [
      { type: "researchLiterature", slug: open, enabled: l.enabled, check: true, live: true, sources, papers: l.papers.map(paperRow), fetching: [...fetching], adding: [...adding] },
      { type: "researchPaper", slug: open, detail: shown ? paperDetail(shown) : null },
    ];
  }
  const resultsEvents = () => (results && results.slug === open ? [{ type: "researchResults", ...results }] : []);
  const say = (/** @type {any[]} */ events) => events.forEach((e) => deliver(e));
  /** A paper added: its PDF downloaded, a moment later, when one is to be had. @param {any} paper @param {boolean} pdf */
  function addPaper(paper, pdf) {
    const l = litOf(open);
    paper.id = `P${String(l.papers.length + 1).padStart(3, "0")}`;
    l.papers.push(paper);
    if (pdf) {
      const slug = open;
      fetching.add(paper.id);
      setTimeout(() => {
        fetching.delete(paper.id);
        paper.hasPdf = true;
        paper.pdfFrom = "https://example.org/paper.pdf";
        if (open === slug) say(literatureEvents());
      }, 1400);
    }
    return paper.id;
  }
  const labRunsEvent = () => {
    const out = [{ type: "researchLab", slug: open, experiments: experiments[open] ?? [], runs: (runs[open] ?? []).map((r) => r.row), sandbox: "seatbelt", autoRun: autoRun.has(open) }];
    if (showingRun) out.push({ type: "researchRun", slug: open, detail: (runs[open] ?? []).find((r) => r.row.id === showingRun) ?? null });
    return out;
  };
  const plan = () => {
    const pkgs = [["python", "3.12.7", 13_412_345], ["numpy", "2.1.3", 5_301_200], ["scipy", "1.14.1", 14_900_000], ["sympy", "1.13.3", 4_620_000],
      ["mpmath", "1.3.0", null], ["matplotlib-base", "3.9.2", 7_830_000], ["networkx", "3.4.2", 1_210_000], ["pandas", "2.2.3", 11_400_000],
      ["libopenblas", "0.3.28", 4_210_000], ["openssl", "3.3.2", null], ["tk", "8.6.13", 3_120_000], ["ca-certificates", "2024.8.30", null]];
    const packages = pkgs.map(([name, version, size]) => ({ name, version, build: "h0", size })).sort((a, b) => String(a.name).localeCompare(String(b.name)));
    return { packages, download: packages.reduce((n, p) => n + Number(p.size ?? 0), 0), cached: packages.filter((p) => p.size == null).length, prefix: PREFIX };
  };

  const summary = (i) => ({ slug: i.slug, title: i.title, discipline: i.discipline, created: i.created, hypotheses: i.hypotheses.length, rounds: i.rounds,
    forkedFrom: i.forked?.title ?? null });
  const find = (slug) => investigations.find((i) => i.slug === slug);

  function rows(i) {
    const r = ratings(i.matches);
    return i.hypotheses.map((h) => {
      const rating = r.get(h.id) ?? { elo: 1200, wins: 0, losses: 0 };
      return {
        id: h.id, title: h.title, author: h.author, family: family[h.author.seat], round: h.round, status: h.status,
        elo: Math.round(rating.elo), wins: rating.wins, losses: rating.losses,
        verdicts: i.reviews.filter((v) => v.hypothesis === h.id).map((v) => v.verdict),
        parents: h.parents, duplicateOf: null, experiments: 0, formal: i.proofs?.[h.id]?.record.outcome ?? null,
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
      experiments: [], task: null, proof: i.proofs?.[id] ?? null,
    };
  }

  function board() {
    const i = find(open);
    if (!i) return [];
    return [
      { type: "researchBoard", slug: i.slug, title: i.title, discipline: i.discipline, brief: i.brief, plan: i.plan,
        overview: i.overview, verdicts: i.verdicts, sandbox: i.sandbox, feedback: i.feedback ?? "", hypotheses: rows(i),
        // Every round here ended since forking was possible, so each can be forked from.
        forked: i.forked ?? null, roundEnds: Array.from({ length: i.rounds }, (_, k) => k + 1) },
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
      tasks: r.tasks.map(({ prompt, answer, ...t }) => t), literature: r.literature ?? null, held: null }];
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
    // The literature, searched once the plan is framed: a beat to answer, a paper found.
    const lit = r.literature;
    if (lit && r.status === "running") {
      const now = Math.floor(Date.now() / 1000);
      if (lit.state === "searching") {
        const l = litOf(i.slug);
        const id = `P${String(l.papers.length + 1).padStart(3, "0")}`;
        l.papers.push({ id, title: "On the representation of integers as sums of three cubes", authors: ["A. Author", "B. Author"], authorsMore: 0, year: 2021,
          venue: "A journal of number theory", doi: null, arxiv: null, abstract: "Which integers are sums of three cubes, and how a search finds the representations.",
          tldr: "", access: "open", hasPdf: true, pdfFrom: "https://example.org/paper.pdf", pdfUrl: null, by: "round", round: r.n, citations: 12,
          queries: lit.queries, sources: ["arxiv"], citedBy: [], pdfNote: null });
        Object.assign(lit, { state: "done", ended: now, added: [id] });
        if (i.slug === open) say(literatureEvents());
      } else if (lit.state === "due" && r.tasks.filter((/** @type {any} */ t) => t.phase === "frame").every((/** @type {any} */ t) => t.state === "done")) {
        Object.assign(lit, { state: "searching", started: now, queries: [i.title.toLowerCase().replace(/[^a-z0-9 ]/g, "")] });
      }
    }
    if (r.status === "running") {
      const now = Math.floor(Date.now() / 1000);
      // A step starts once every step before it has landed; two to an engine.
      for (const t of r.tasks.filter((/** @type {any} */ t) => t.state === "ready")) {
        const before = r.tasks.filter((/** @type {any} */ x) => ORDER.indexOf(x.phase) < ORDER.indexOf(t.phase));
        if (before.some((/** @type {any} */ x) => x.state !== "done")) break;
        // The hypotheses wait for the literature.
        if (t.phase === "generate" && lit && (lit.state === "due" || lit.state === "searching")) break;
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
      for (const event of [...roundEvent(), ...board(), ...transcriptEvent(), { type: "researchList", investigations: investigations.map(summary), open, folder: kept }]) deliver(event);
    }
  }

  function run() {
    if (!timer) timer = setInterval(beat, 700);
  }

  /** A file a run left, as the shell would read it: a picture drawn here,
   *  or a scene made here. @param {string} key */
  async function file(key) {
    const path = key.split("/").slice(2).join("/");
    if (path.endsWith(".png")) {
      const canvas = document.createElement("canvas");
      canvas.width = 640;
      canvas.height = 360;
      const g = /** @type {CanvasRenderingContext2D} */ (canvas.getContext("2d"));
      g.fillStyle = "#16161e";
      g.fillRect(0, 0, 640, 360);
      g.strokeStyle = "#7aa2f7";
      g.lineWidth = 2.5;
      const ys = Array.from({ length: 601 }, (_, x) => {
        const lam = 300 + (x / 600) * 500;
        return lam ** -9 / (Math.exp(14388e3 / (lam * 5772)) - 1);
      });
      const top = Math.max(...ys);
      g.beginPath();
      ys.forEach((y, x) => g[x ? "lineTo" : "moveTo"](20 + x, 330 - (y / top) * 280));
      g.stroke();
      g.fillStyle = "#a9b1d6";
      g.font = "14px sans-serif";
      g.fillText("scattered spectrum, normalised — wavelength 300–800 nm", 24, 28);
      const blob = await new Promise((done) => canvas.toBlob(done, "image/png"));
      return /** @type {Blob} */ (blob).arrayBuffer();
    }
    if (path.endsWith(".scene.json")) {
      // A planet on an ellipse round its star, a frame every five degrees.
      const frames = Array.from({ length: 72 }, (_, k) => {
        const a = (k / 72) * Math.PI * 2;
        const r = 1 / (1 + 0.3 * Math.cos(a));
        const at = [r * Math.cos(a), 0, r * Math.sin(a)];
        const path = Array.from({ length: k + 1 }, (_, j) => {
          const b = (j / 72) * Math.PI * 2, s = 1 / (1 + 0.3 * Math.cos(b));
          return [s * Math.cos(b), 0, s * Math.sin(b)];
        });
        return [
          { kind: "sphere", center: [0, 0, 0], radius: 0.16, color: "#e0af68" },
          { kind: "sphere", center: at, radius: 0.06, color: "#7aa2f7" },
          { kind: "line", xyz: path, color: "#565f89" },
          { kind: "arrow", origin: at, vector: [-Math.sin(a) * 0.35, 0, Math.cos(a) * 0.35], color: "#9ece6a" },
        ];
      });
      return new TextEncoder().encode(JSON.stringify({ format: "proton.scene/1", title: "orbit", units: "AU", frames })).buffer;
    }
    const text = (/** @type {string} */ t) => new TextEncoder().encode(t).buffer;
    if (path === "caffeine.smi") return text("Cn1cnc2c1c(=O)n(C)c(=O)n2C caffeine\n");
    if (path === "ozone.xyz") {
      // Ozone bending, its angle 117° ± 8°, twenty-four frames a cycle.
      return text(Array.from({ length: 24 }, (_, k) => {
        const half = ((117 + 8 * Math.sin((k / 24) * Math.PI * 2)) / 2) * (Math.PI / 180);
        const r = 1.278;
        return `3\nozone bend ${k}\nO 0 0 0\nO ${(r * Math.sin(half)).toFixed(4)} ${(-r * Math.cos(half)).toFixed(4)} 0\nO ${(-r * Math.sin(half)).toFixed(4)} ${(-r * Math.cos(half)).toFixed(4)} 0\n`;
      }).join(""));
    }
    if (path === "fold.pdb") return text(previewFold());
    throw new Error(`${key}: not a file a run left`);
  }

  function sync() {
    return [{ type: "researchList", investigations: investigations.map(summary), open, folder: kept }, ...board(), ...literatureEvents(), ...resultsEvents(),
      ...roundEvent(), labEvent(), leanEvent(), ...labRunsEvent()];
  }

  /** @param {any} command */
  function command(command) {
    switch (command.type) {
      case "researchFolder": {
        // Where they are kept, as the window would say it; nothing moves in a preview.
        const typed = String(command.folder ?? "").trim().replace(/[\\/]+$/, "");
        kept = typed ? `${typed}/` : "research/";
        return sync();
      }
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
        showingRun = null;
        showingPaper = null;
        litOf(slug).enabled = Boolean(command.literature);
        return sync();
      }
      case "researchFork": {
        const from = find(command.slug);
        const title = String(command.title ?? "").trim();
        if (!from) return [{ type: "notice", level: "error", text: `No investigation ${command.slug}.` }];
        if (!title) return [{ type: "notice", level: "error", text: "A fork needs a title." }];
        const r = command.round ?? null;
        if (r == null && roundOf(from.slug).status !== "idle") {
          return [{ type: "notice", level: "warn", text: "A round is under way: fork from a round that has ended, or once this one has." }];
        }
        let slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "investigation";
        for (let n = 2; find(slug); n++) slug = `${slug.replace(/-\d+$/, "")}-${n}`;
        const within = (/** @type {number} */ n) => r == null || n <= r;
        const after = r == null ? "as it stood" : `after round ${r}`;
        const said = String(command.direction ?? "").trim();
        const fork = {
          ...structuredClone({ ...from, forked: undefined }), slug, title, created: Math.floor(Date.now() / 1000), rounds: r ?? from.rounds,
          brief: `${from.brief.replace(`# ${from.title}`, `# ${title}`).trimEnd()}\n\n## Forked\n\nForked from *${from.title}* (\`${from.slug}\`) ${after}, to take another direction.\n`,
          hypotheses: structuredClone(from.hypotheses.filter((h) => within(h.round))),
          reviews: structuredClone(from.reviews.filter((v) => within(v.round ?? 1))),
          matches: structuredClone(from.matches.filter((m) => within(m.round))),
          feedback: `${from.feedback ?? "# Feedback\n"}${said ? `\n## On forking${r == null ? "" : `, after round ${r}`}\n\n${said}\n` : ""}`,
          forked: { from: from.slug, title: from.title, round: r, at: Math.floor(Date.now() / 1000) },
        };
        investigations.unshift(fork);
        open = slug;
        showing = null;
        watching = null;
        showingRun = null;
        showingPaper = null;
        return [...sync(), { type: "notice", level: "info", text: `Forked “${from.title}”${r == null ? "" : ` after round ${r}`} as “${title}”.` }];
      }
      case "researchOpen":
        if (!find(command.slug)) return [{ type: "notice", level: "error", text: `No investigation ${command.slug}.` }];
        open = command.slug;
        showing = null;
        watching = null;
        showingRun = null;
        showingPaper = null;
        return sync();
      case "researchSetLiterature": {
        const l = litOf(open);
        if (command.enabled != null) l.enabled = command.enabled;
        if (command.sources) l.sources = new Set(command.sources);
        return literatureEvents();
      }
      case "researchSearch": {
        const l = litOf(open);
        if (!l.enabled) return [{ type: "notice", level: "warn", text: "This investigation does not use the literature. Turn it on in the Literature tab first." }];
        const query = String(command.query ?? "").trim();
        const slug = open;
        results = { slug, query, searching: true, results: [], failures: [] };
        setTimeout(() => {
          const have = (/** @type {string} */ title) => l.papers.find((p) => p.title === title)?.id ?? null;
          results = { slug, query, searching: false, failures: l.sources.has("openAlex") ? [] : [],
            results: FOUND.map((r, i) => ({ key: `title:${r.title.toLowerCase()}|${r.year}`, title: r.title, authors: r.authors, year: r.year, venue: r.venue,
              doi: null, arxiv: null, abstract: r.abstract, citations: r.citations, access: r.access, pdf: r.pdf,
              sources: i % 2 ? ["semanticScholar", "openAlex"] : ["arxiv"], queries: [query], have: have(r.title) })) };
          if (open === slug) say(resultsEvents());
        }, 900);
        return resultsEvents();
      }
      case "researchAddPaper": {
        const l = litOf(open);
        if (!l.enabled) return [{ type: "notice", level: "warn", text: "This investigation does not use the literature. Turn it on in the Literature tab first." }];
        const key = String(command.paper ?? "").trim();
        const found = results?.results.find((/** @type {any} */ r) => r.key === key);
        if (found) {
          const id = addPaper({ title: found.title, authors: found.authors.replace(" et al.", "").split(" and "), authorsMore: 0, year: found.year, venue: found.venue,
            doi: null, arxiv: null, abstract: found.abstract, tldr: "", access: found.access, hasPdf: false, pdfFrom: null, pdfUrl: null, by: "scientist", round: null,
            citations: found.citations, queries: found.queries, sources: found.sources, citedBy: [],
            pdfNote: found.pdf ? null : found.access === "closed" ? "It is behind a paywall, as far as the sources know." : "No open copy of it is known." }, found.pdf);
          found.have = id;
          showingPaper = id;
          return [{ type: "notice", level: "info", text: `${id} is added: ${found.title}` }, ...literatureEvents(), ...resultsEvents()];
        }
        if (!/^(10\.\d{4,}\/\S+|(arxiv:)?\d{4}\.\d{4,5}|https?:\/\/\S+)$/i.test(key.replace(/^doi:/i, ""))) {
          return [{ type: "notice", level: "warn", text: "Give a DOI or an arXiv id, or a link to either, to add a paper by." }];
        }
        adding.push(key);
        const slug = open;
        setTimeout(() => {
          adding.splice(adding.indexOf(key), 1);
          const id = addPaper({ title: `A paper looked up by ${key}`, authors: ["A. Author"], authorsMore: 0, year: 2024, venue: "A journal",
            doi: key.replace(/^doi:/i, ""), arxiv: null, abstract: "", tldr: "", access: "unknown", hasPdf: false, pdfFrom: null, pdfUrl: null,
            by: "scientist", round: null, citations: 0, queries: [], sources: ["openAlex"], citedBy: [], pdfNote: "No open copy of it is known." }, false);
          if (open === slug) say([{ type: "notice", level: "info", text: `${id} is added: A paper looked up by ${key}` }, ...literatureEvents()]);
        }, 700);
        return literatureEvents();
      }
      case "researchShowPaper":
        showingPaper = command.paper ?? null;
        return literatureEvents();
      case "researchOpenPaper":
        return [{ type: "notice", level: "info", text: `In the app, ${command.paper}.pdf opens here in the document viewer.` }];
      case "researchFetchPdf": {
        const paper = litOf(open).papers.find((p) => p.id === command.paper);
        if (!paper) return [];
        fetching.add(paper.id);
        const slug = open;
        setTimeout(() => {
          fetching.delete(paper.id);
          paper.pdfNote = "Its PDF could not be kept: the link gives a web page, not a PDF.";
          if (open === slug) say(literatureEvents());
        }, 1200);
        return literatureEvents();
      }
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
        r.literature = litOf(open).enabled ? { state: "due", why: null, queries: [], added: [], notes: [], started: null, ended: null } : null;
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
        return [{ type: "researchList", investigations: investigations.map(summary), open, folder: kept }, ...board(),
          { type: "notice", level: "info", text: `${id} is yours: it is reviewed and played with the others.` }];
      }
      case "researchSetStatus": {
        const h = find(open)?.hypotheses.find((/** @type {any} */ x) => x.id === command.hypothesis);
        if (h) h.status = command.status;
        return board();
      }
      case "researchApprove": {
        const e = (experiments[open] ?? []).find((x) => x.id === command.experiment);
        if (!e) return [];
        if (env.state !== "ready") return [{ type: "notice", level: "warn", text: "Make the Lab's Python first: the Lab pane says how." }];
        const list = (runs[open] ??= []);
        const id = `R${String(list.length + 1).padStart(3, "0")}`;
        const run = { row: { id, experiment: e.id, hypothesis: e.hypothesis, state: "running", exit: null, seconds: null, started: Math.floor(Date.now() / 1000), sandbox: "seatbelt" },
          stdout: "", stdoutCut: false, stderr: "", stderrCut: false, metrics: {}, figures: [], scenes: [], molecules: [],
          record: { code: { sha256: "1a2b3c4d5e6f7a8b" }, python: "/Users/you/.proton/lab/envs/lab/bin/python", about: { approved: { by: "scientist", at: now } }, limits: { seconds: 600 } } };
        list.unshift(run);
        e.status = "running";
        e.run = id;
        showingRun = id;
        setTimeout(() => {
          if (run.row.state !== "running") return;
          Object.assign(run.row, { state: "done", exit: 0, seconds: 3.1 });
          Object.assign(run, { stdout: "dominant wavelength 474.8 nm", metrics: { dominant_nm: { value: 474.8, unit: "nm" } }, figures: ["figures/spectrum.png"] });
          e.status = "done";
          for (const event of labRunsEvent()) deliver(event);
        }, 2500);
        return labRunsEvent();
      }
      case "researchAutoRun":
        if (command.enabled) autoRun.add(open);
        else autoRun.delete(open);
        return labRunsEvent();
      case "researchReject": {
        const e = (experiments[open] ?? []).find((x) => x.id === command.experiment);
        if (e) e.status = "rejected";
        return labRunsEvent();
      }
      case "researchStopRun": {
        const run = (runs[open] ?? []).find((r) => r.row.id === command.run);
        if (run && run.row.state === "running") {
          Object.assign(run.row, { state: "stopped", seconds: 1.2 });
          const e = (experiments[open] ?? []).find((x) => x.id === run.row.experiment);
          if (e) e.status = "failed";
        }
        return labRunsEvent();
      }
      case "researchShowRun":
        showingRun = command.run ?? null;
        return showingRun ? labRunsEvent() : [{ type: "researchRun", slug: open, detail: null }];
      case "labPlan":
        clearInterval(labTimer);
        env = { state: "planning", log: ["Collecting package metadata (repodata.json): ...working..."] };
        labTimer = setTimeout(() => { env = { state: "planned", plan: plan() }; deliver(labEvent()); }, 1500);
        return [labEvent()];
      case "labCreate": {
        if (env.state !== "planned") return [];
        const total = env.plan.packages.filter((/** @type {any} */ p) => p.size != null).length;
        env = { state: "creating", fetched: 0, total, log: [] };
        clearInterval(labTimer);
        labTimer = setInterval(() => {
          if (env.state !== "creating") return clearInterval(labTimer);
          env = { ...env, fetched: env.fetched + 1 };
          if (env.fetched >= env.total) {
            clearInterval(labTimer);
            env = { state: "ready", python: "3.12.7", prefix: PREFIX, packages: [["numpy", "2.1.3"], ["scipy", "1.14.1"], ["sympy", "1.13.3"], ["mpmath", "1.3.0"],
              ["matplotlib", "3.9.2"], ["networkx", "3.4.2"], ["pandas", "2.2.3"]].map(([name, version]) => ({ name, version, build: "", size: null })) };
          }
          deliver(labEvent());
        }, 350);
        return [labEvent()];
      }
      case "labCancel":
        clearInterval(labTimer);
        clearTimeout(labTimer);
        env = { state: "missing", maker: MAKER, index: now - 86400 * 9, specs: SPECS, prefix: PREFIX };
        return [labEvent()];
      case "labRemove":
        env = { state: "removing" };
        setTimeout(() => {
          env = { state: "missing", maker: MAKER, index: now - 86400 * 9, specs: SPECS, prefix: PREFIX };
          deliver(labEvent());
        }, 800);
        return [labEvent()];
      case "leanPlan":
        clearTimeout(leanTimer);
        lean = { state: "planning" };
        leanTimer = setTimeout(() => { lean = { state: "planned", plan: leanPlan() }; deliver(leanEvent()); }, 1200);
        return [leanEvent()];
      case "leanCreate": {
        if (lean.state !== "planned") return [];
        // Each stage in a few ticks: what it downloads, and what it ran.
        const stages = [["elan", 2_190_845, "curl -q -fsSL --proto =https … -o elan.tar.gz"],
          ["toolchain", 562_038_070, "elan toolchain install leanprover/lean4:v4.34.1"], ["mathlib", 700_000_000, "lake update"],
          ["library", 2_400_000_000, "lake exe cache get"], ["finding", 0, "lake env printenv LEAN_PATH"], ["checking", 0, "lean --json --threads=4 Proof.lean"],
          ["finishing", 0, ""]];
        let at = 0;
        let step = 0;
        const show = () => {
          const [stage, total, ran] = stages[at];
          lean = { state: "making", stage, done: Math.round((Number(total) * step) / 3), total, log: ran ? [`$ ${ran}`] : [] };
        };
        show();
        clearInterval(leanTimer);
        leanTimer = setInterval(() => {
          step++;
          if (step > 3 || !stages[at][1]) {
            at++;
            step = 0;
          }
          if (at >= stages.length) {
            clearInterval(leanTimer);
            lean = { state: "ready", lean: "v4.34.1", mathlib: "v4.34.1", commit: "6f3a1c2e9b5d47a0", size: 14_100_000_000, home: LEAN_HOME };
          } else {
            show();
          }
          deliver(leanEvent());
        }, 400);
        return [leanEvent()];
      }
      case "leanCancel":
        clearInterval(leanTimer);
        clearTimeout(leanTimer);
        lean = LEAN_MISSING;
        return [leanEvent()];
      case "leanRemove":
        lean = { state: "removing" };
        setTimeout(() => { lean = LEAN_MISSING; deliver(leanEvent()); }, 800);
        return [leanEvent()];
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

  return { sync, command, file };
}
