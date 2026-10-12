/* The LaTeX mode's stand-ins for the browser preview: TeX not installed,
 * planned, installed a stage at a time, as the core would say. Scripted —
 * the real install is in proton-lab, and this must never grow into one. */

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
    entered: () => [event()],
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
        default:
          return [];
      }
    },
  };
}
