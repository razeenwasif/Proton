/* The LaTeX mode's stand-ins for the browser preview: TeX not installed,
 * planned, installed a stage at a time, as the core would say. Scripted —
 * the real install is in proton-lab, and this must never grow into one. */

/** The gallery's templates, as the core lists them. */
const TEMPLATES = [
  { id: "article", name: "Article", about: "A short paper or essay: a title, an abstract, sections and references.", files: ["main.tex", "references.bib"], wants: [] },
  { id: "paper", name: "Research paper", about: "Theorems and proofs, a table, and references by number with natbib.", files: ["main.tex", "references.bib"], wants: [] },
  { id: "report", name: "Report", about: "Chapters, with a table of contents.", files: ["main.tex"], wants: [] },
  { id: "thesis", name: "Thesis", about: "A book of several files, a chapter in each, typeset from main.tex.", files: ["main.tex", "chapters/introduction.tex", "chapters/background.tex", "chapters/conclusion.tex", "references.bib"], wants: [] },
  { id: "slides", name: "Slides", about: "A Beamer presentation: a title page, an outline, and frames.", files: ["main.tex"], wants: [] },
  { id: "cv", name: "CV", about: "One page: education, experience and skills.", files: ["main.tex"], wants: [] },
];

/** A thesis's outline, as the core reads one. */
const OUTLINE = { words: 4182, headings: [
  { level: 1, title: "Introduction", file: "chapters/introduction.tex", line: 1, label: "ch:introduction" },
  { level: 1, title: "Background", file: "chapters/background.tex", line: 1, label: "ch:background" },
  { level: 2, title: "What is known", file: "chapters/background.tex", line: 4, label: "sec:known" },
  { level: 2, title: "What is not", file: "chapters/background.tex", line: 9, label: "sec:gap" },
  { level: 1, title: "Conclusion", file: "chapters/conclusion.tex", line: 1, label: null },
] };

/** What the cite picker finds, as the core would. */
const CITATIONS = [
  { source: "bib", id: "knuth1984", key: "knuth1984", title: "The TeXbook", authors: "Knuth", year: "1984", venue: null, from: "references.bib" },
  { source: "research", id: "why-is-the-sky-blue/P001", key: "strutt1871light", title: "On the light from the sky, its polarization and colour", authors: "Strutt", year: "1871", venue: "Philosophical Magazine", from: "P001 · Why is the sky blue?" },
  { source: "research", id: "why-is-the-sky-blue/P002", key: "lovelace2021scattering", title: "Scattering in the atmosphere", authors: "Lovelace et al.", year: "2021", venue: null, from: "P002 · Why is the sky blue?" },
];

/** @param {(event: any) => void} deliver */
export function createTexPreview(deliver) {
  const home = "~/.proton/tex";
  /** @type {any} */
  let env = { state: "missing", needs: [], free: 120_000_000_000, home };
  const plan = (/** @type {string} */ bundle) => ({
    bundle, tinytex: "v2026.10", texlab: "v5.26.0", home, free: 120_000_000_000, fits: true,
    downloads: [
      { what: `${bundle === "small" ? "TinyTeX-1" : "TinyTeX"} v2026.10`, host: "github.com", bytes: bundle === "small" ? 67_200_000 : 210_500_000, exact: true, verified: true },
      { what: "latexmk, by TeX's package manager", host: "ctan.org", bytes: 1_000_000, exact: false, verified: false },
      { what: "texlab v5.26.0", host: "github.com", bytes: 10_200_000, exact: true, verified: true },
    ],
    download: bundle === "small" ? 78_400_000 : 221_700_000, disk: 1_000_000_000, kept: 780_000_000,
    tinytexUrl: "", tinytexSha256: null, texlabUrl: "", texlabSha256: null,
  });
  const event = () => ({ type: "texEnv", env });
  /** @type {ReturnType<typeof setTimeout>[]} */
  let timers = [];
  const later = (/** @type {number} */ ms, /** @type {() => void} */ then) => timers.push(setTimeout(() => { then(); deliver(event()); }, ms));

  return {
    /** What the window sees on entering the mode. */
    entered: () => [event(), { type: "texTemplates", templates: TEMPLATES, investigations: [["why-is-the-sky-blue", "Why is the sky blue?"]] },
      { type: "texOutline", main: "main.tex", outline: OUTLINE }],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "texPlan":
          env = { state: "planning" };
          later(600, () => { env = { state: "planned", plan: plan(command.bundle ?? "full") }; });
          return [event()];
        case "texCreate": {
          const stages = ["tinytex", "latexmk", "texlab", "checking", "finishing"];
          stages.forEach((stage, i) => later(500 * (i + 1), () => {
            env = { state: "making", stage, done: stage === "tinytex" ? 120_000_000 : 0, total: stage === "tinytex" ? 210_500_000 : 0, log: [`$ ${stage}`] };
          }));
          later(500 * (stages.length + 1), () => { env = { state: "ready", tinytex: "v2026.10", texlab: "v5.26.0", bundle: "full", size: 812_000_000, home }; });
          return [];
        }
        case "texCancel":
          timers.forEach(clearTimeout);
          timers = [];
          env = { state: "missing", needs: [], free: 120_000_000_000, home };
          return [event()];
        case "texRemove":
          env = { state: "removing" };
          later(400, () => { env = { state: "missing", needs: [], free: 120_000_000_000, home }; });
          return [event()];
        case "texCiteSearch": {
          const words = String(command.query ?? "").toLowerCase().split(/\s+/).filter(Boolean);
          const citations = CITATIONS.filter((c) => words.every((w) => `${c.title} ${c.authors} ${c.year} ${c.key}`.toLowerCase().includes(w)));
          return [{ type: "texCitations", query: command.query ?? "", citations, searching: false, note: null }];
        }
        default:
          return [];
      }
    },
  };
}
