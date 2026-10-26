/* Diffuse's stand-in for the core, in the browser preview.
 *
 * The preview has no core and no forge, so this is both: it holds flows in
 * memory, applies the window's ops much as proton-diffuse does (without its
 * checks beyond the plainest), and says forge serves the v1 catalogue —
 * the same JSON as proton-diffuse's test fixture, which a node test keeps
 * in step. A run plays out over a couple of seconds, step by step, and its
 * image is a picture drawn here. Nothing is written to disk. */

/** The v1 node catalogue: `crates/proton-diffuse/tests/fixtures/catalogue-v1.json`. */
export const CATALOGUE = {"version":1,"types":["MODEL","CLIP","VAE","CONDITIONING","LATENT","IMAGE","MASK","UPSCALER"],"nodes":[{"type":"LoadCheckpoint","title":"Load Checkpoint","group":"Load","inputs":[{"name":"checkpoint","kind":"choice","from":"checkpoints"}],"outputs":[{"name":"model","type":"MODEL"},{"name":"clip","type":"CLIP"},{"name":"vae","type":"VAE"}]},{"type":"LoadLoRA","title":"Load LoRA","group":"Load","inputs":[{"name":"model","type":"MODEL"},{"name":"clip","type":"CLIP"},{"name":"lora","kind":"choice","from":"loras"},{"name":"strength","kind":"float","min":-2,"max":2,"step":0.05,"default":1}],"outputs":[{"name":"model","type":"MODEL"},{"name":"clip","type":"CLIP"}]},{"type":"LoadVAE","title":"Load VAE","group":"Load","inputs":[{"name":"vae","kind":"choice","from":"vaes"}],"outputs":[{"name":"vae","type":"VAE"}]},{"type":"LoadImage","title":"Load Image","group":"Load","inputs":[{"name":"image","kind":"upload"}],"outputs":[{"name":"image","type":"IMAGE"},{"name":"mask","type":"MASK"}]},{"type":"TextEncode","title":"Text Encode","group":"Prompt","inputs":[{"name":"clip","type":"CLIP"},{"name":"text","kind":"text","multiline":true,"default":""}],"outputs":[{"name":"conditioning","type":"CONDITIONING"}]},{"type":"EmptyLatent","title":"Empty Latent","group":"Latent","inputs":[{"name":"width","kind":"int","min":64,"max":4096,"step":8,"default":1024},{"name":"height","kind":"int","min":64,"max":4096,"step":8,"default":1024},{"name":"batch","kind":"int","min":1,"max":8,"step":1,"default":1}],"outputs":[{"name":"latent","type":"LATENT"}]},{"type":"KSampler","title":"KSampler","group":"Sample","inputs":[{"name":"model","type":"MODEL"},{"name":"positive","type":"CONDITIONING"},{"name":"negative","type":"CONDITIONING","optional":true},{"name":"latent","type":"LATENT"},{"name":"seed","kind":"seed"},{"name":"steps","kind":"int","min":1,"max":150,"step":1,"default":30},{"name":"cfg","kind":"float","min":0,"max":30,"step":0.1,"default":6.5},{"name":"sampler","kind":"choice","from":"samplers"},{"name":"scheduler","kind":"choice","from":"schedulers"},{"name":"denoise","kind":"float","min":0,"max":1,"step":0.01,"default":1}],"outputs":[{"name":"latent","type":"LATENT"}]},{"type":"VAEDecode","title":"VAE Decode","group":"Latent","inputs":[{"name":"vae","type":"VAE"},{"name":"latent","type":"LATENT"}],"outputs":[{"name":"image","type":"IMAGE"}]},{"type":"VAEEncode","title":"VAE Encode","group":"Latent","inputs":[{"name":"vae","type":"VAE"},{"name":"image","type":"IMAGE"}],"outputs":[{"name":"latent","type":"LATENT"}]},{"type":"SetLatentMask","title":"Set Latent Mask","group":"Latent","inputs":[{"name":"latent","type":"LATENT"},{"name":"mask","type":"MASK"}],"outputs":[{"name":"latent","type":"LATENT"}]},{"type":"UpscaleLatent","title":"Upscale Latent","group":"Latent","inputs":[{"name":"latent","type":"LATENT"},{"name":"scale","kind":"float","min":1,"max":4,"step":0.05,"default":1.5}],"outputs":[{"name":"latent","type":"LATENT"}]},{"type":"LoadUpscaler","title":"Load Upscaler","group":"Load","inputs":[{"name":"upscaler","kind":"choice","from":"upscalers"}],"outputs":[{"name":"upscaler","type":"UPSCALER"}]},{"type":"UpscaleImage","title":"Upscale Image","group":"Image","inputs":[{"name":"image","type":"IMAGE"},{"name":"upscaler","type":"UPSCALER","optional":true},{"name":"scale","kind":"float","min":1,"max":4,"step":0.05,"default":2}],"outputs":[{"name":"image","type":"IMAGE"}]},{"type":"InvertMask","title":"Invert Mask","group":"Image","inputs":[{"name":"mask","type":"MASK"}],"outputs":[{"name":"mask","type":"MASK"}]},{"type":"PreviewImage","title":"Preview Image","group":"Output","inputs":[{"name":"images","type":"IMAGE"}],"outputs":[]},{"type":"SaveImage","title":"Save Image","group":"Output","inputs":[{"name":"images","type":"IMAGE"},{"name":"prefix","kind":"text","default":"image"}],"outputs":[]}],"lists":{"samplers":["euler","euler_a","heun","dpmpp_2m","dpmpp_2m_sde","dpmpp_sde","ddim","unipc","lcm"],"schedulers":["normal","karras","exponential","sgm_uniform","simple"]}};

/** Models forge might have: the same crate's `models.json`. */
const MODELS = {"checkpoints":[{"name":"sd_xl_base_1.0.safetensors","family":"sdxl","bytes":6938078334},{"name":"flux1-dev-fp8.safetensors","family":"flux","bytes":17246524772}],"loras":[{"name":"impasto.safetensors","family":"sdxl","bytes":228452040}],"vaes":[],"upscalers":[{"name":"4x-UltraSharp.pth","bytes":66961958}]};

const STARTER = [
  ["LoadCheckpoint", [16, 16], { checkpoint: "sd_xl_base_1.0.safetensors" }],
  ["TextEncode", [266, 16], { text: "a lighthouse at dusk, oil painting" }],
  ["TextEncode", [266, 236], { text: "blurry, low contrast, text, watermark" }],
  ["EmptyLatent", [16, 260], {}],
  ["KSampler", [516, 16], { seed: 42, sampler: "dpmpp_2m", scheduler: "karras" }],
  ["VAEDecode", [766, 16], {}],
  ["SaveImage", [766, 160], { prefix: "image" }],
];
const STARTER_LINKS = [["1:clip", "2:clip"], ["1:clip", "3:clip"], ["1:model", "5:model"], ["2:conditioning", "5:positive"], ["3:conditioning", "5:negative"], ["4:latent", "5:latent"], ["5:latent", "6:latent"], ["1:vae", "6:vae"], ["6:image", "7:images"]];

/** @param {(event: any) => void} deliver */
export function createDiffusePreview(deliver) {
  /** @type {Map<string, {flow: any, undo: any[], redo: any[], saved: string}>} */
  const flows = new Map();
  /** @type {string[]} */
  const open = [];
  /** @type {string | null} */
  let shown = null;
  const kind = (/** @type {string} */ type) => CATALOGUE.nodes.find((n) => n.type === type);
  const copy = (/** @type {any} */ v) => JSON.parse(JSON.stringify(v));
  const fresh = (/** @type {any} */ flow) => String(Math.max(0, ...flow.nodes.map((/** @type {any} */ n) => Number(n.id)).filter(Number.isInteger)) + 1);
  const port = (/** @type {string} */ p) => p.split(":")[0];
  /** Runs this session, newest first. @type {any[]} */
  const runs = [];
  /** The outputs, newest first. @type {any[]} */
  const images = [];
  /** Each output's picture, by path: the seed it was drawn from. @type {Map<string, number>} */
  const drawn = new Map();
  /** The graph each output was made by, by path. @type {Map<string, any>} */
  const ranWith = new Map();
  let runCount = 0;
  /** Diffuse's folder, as the person would type it. */
  let folder = "~/Diffuse";
  const runsEvent = () => ({ type: "diffuseRuns", runs: copy(runs) });
  const galleryEvent = () => ({ type: "diffuseGallery", images: copy(images) });

  /** A picture from a seed: a dusk sky, a sea, a sun. @param {number} seed @param {number} size @param {string} type */
  function picture(seed, size, type) {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = size;
    const g = /** @type {CanvasRenderingContext2D} */ (canvas.getContext("2d"));
    const hue = (seed * 47) % 360;
    const sky = g.createLinearGradient(0, 0, 0, size * 0.62);
    sky.addColorStop(0, `hsl(${hue} 40% 18%)`);
    sky.addColorStop(1, `hsl(${(hue + 40) % 360} 60% 62%)`);
    g.fillStyle = sky;
    g.fillRect(0, 0, size, size * 0.62);
    g.fillStyle = `hsl(${(hue + 200) % 360} 35% 16%)`;
    g.fillRect(0, size * 0.62, size, size);
    g.fillStyle = "#f7c58a";
    g.beginPath();
    g.arc(size * (0.3 + ((seed % 7) / 14)), size * 0.5, size * 0.07, 0, Math.PI * 2);
    g.fill();
    return canvas.toDataURL(type, 0.6);
  }

  function apply(/** @type {any} */ flow, /** @type {any} */ op, /** @type {string[]} */ added) {
    switch (op.op) {
      case "add": {
        const k = kind(op.type);
        if (!k) throw new Error(`the server has no ${op.type} node`);
        const values = { ...op.values };
        for (const i of k.inputs) if (!i.type && i.default !== undefined && !(i.name in values)) values[i.name] = i.default;
        for (const i of k.inputs) if (i.kind === "seed" && !(i.name in values)) values[i.name] = 0;
        const id = fresh(flow);
        flow.nodes.push({ id, type: op.type, at: op.at, values, seed: null, bypass: null });
        added.push(id);
        break;
      }
      case "remove":
        flow.nodes = flow.nodes.filter((/** @type {any} */ n) => !op.ids.includes(n.id));
        flow.links = flow.links.filter((/** @type {any} */ l) => !op.ids.includes(port(l.from)) && !op.ids.includes(port(l.to)));
        break;
      case "place":
        flow.nodes.find((/** @type {any} */ n) => n.id === op.id).at = op.at;
        break;
      case "set": {
        const node = flow.nodes.find((/** @type {any} */ n) => n.id === op.id);
        if (op.value === null) delete node.values[op.name];
        else node.values[op.name] = op.value;
        break;
      }
      case "connect":
        flow.links = flow.links.filter((/** @type {any} */ l) => l.to !== op.to);
        flow.links.push({ from: op.from, to: op.to });
        break;
      case "disconnect":
        flow.links = flow.links.filter((/** @type {any} */ l) => l.to !== op.to);
        break;
      case "seed":
        flow.nodes.find((/** @type {any} */ n) => n.id === op.id).seed = op.mode === "fixed" ? null : op.mode;
        break;
      case "bypass":
        flow.nodes.find((/** @type {any} */ n) => n.id === op.id).bypass = op.on || null;
        break;
      case "duplicate": {
        /** @type {Record<string, string>} */
        const map = {};
        for (const id of op.ids) {
          const n = copy(flow.nodes.find((/** @type {any} */ x) => x.id === id));
          const to = fresh(flow);
          map[id] = to;
          flow.nodes.push({ ...n, id: to, at: [n.at[0] + op.by[0], n.at[1] + op.by[1]] });
          added.push(to);
        }
        for (const l of [...flow.links]) {
          const [a, an] = l.from.split(":");
          const [b, bn] = l.to.split(":");
          if (map[a] && map[b]) flow.links.push({ from: `${map[a]}:${an}`, to: `${map[b]}:${bn}` });
        }
        break;
      }
      case "note":
        flow.note = op.text?.trim() ? op.text : null;
        break;
      case "outputs":
        flow.outputs = op.path?.trim() ? op.path.trim().replace(/\/+$/, "") : null;
        break;
    }
  }

  /** What stops a flow running, the plainest of it. @param {any} flow */
  function problems(flow) {
    const out = [];
    for (const n of flow.nodes) {
      const k = kind(n.type);
      if (!k) {
        out.push({ node: n.id, input: null, message: `the server has no ${n.type} node` });
        continue;
      }
      if (n.bypass) continue;
      for (const i of k.inputs) {
        if (i.type && !i.optional && !flow.links.some((/** @type {any} */ l) => l.to === `${n.id}:${i.name}`)) out.push({ node: n.id, input: i.name, message: `${i.name} has no link` });
        if (!i.type && i.kind === "choice" && i.default === undefined && !(i.name in n.values)) out.push({ node: n.id, input: i.name, message: `choose a ${i.name}` });
      }
    }
    if (!flow.nodes.some((/** @type {any} */ n) => kind(n.type)?.outputs.length === 0 && !n.bypass)) out.push({ node: null, input: null, message: "nothing to run: no output node such as Save Image or Preview Image" });
    return out;
  }

  function view(/** @type {string[]} */ added = []) {
    const doc = shown ? flows.get(shown) : null;
    const tab = (/** @type {string} */ path) => ({ path, dirty: JSON.stringify(flows.get(path)?.flow) !== flows.get(path)?.saved });
    return {
      type: "diffuseView",
      view: {
        folder,
        flows: [...flows.keys()].sort(),
        open: open.map(tab),
        shown: doc && shown ? { path: shown, flow: copy(doc.flow), problems: problems(doc.flow), dirty: tab(shown).dirty, canUndo: doc.undo.length > 0, canRedo: doc.redo.length > 0 } : null,
        added,
        starters: [
          { name: "textToImage", title: "Text to image", about: "a prompt, SDXL, one picture" },
          { name: "imageToImage", title: "Image to image", about: "a picture of yours, redrawn from a prompt" },
          { name: "inpaint", title: "Inpaint", about: "a picture with part of it redrawn: its mask" },
          { name: "hiresFix", title: "Hires fix", about: "drawn, enlarged in the latent, drawn again sharper" },
          { name: "flux", title: "FLUX", about: "FLUX.1 dev: one prompt, no negative" },
        ],
      },
    };
  }

  const known = () => ({
    type: "diffuseKnown",
    known: {
      health: { state: "serving", url: "http://atlas:7420/img/v1", ms: 9, message: "", status: { gpu: { name: "NVIDIA GeForce RTX 4090", total: 25757220864, used: 7623000000 }, loaded: [], queue: [], running: null } },
      catalogue: CATALOGUE,
      models: MODELS,
      fresh: true,
    },
    gateways: ["http://atlas:7420/img/v1"],
  });

  // A flow to look at, from the starter.
  (() => {
    const flow = { format: 1, note: null, outputs: null, nodes: [], links: [] };
    for (const [type, at, values] of STARTER) apply(flow, { op: "add", type, at, values }, []);
    for (const [from, to] of STARTER_LINKS) apply(flow, { op: "connect", from, to }, []);
    flows.set("lighthouse.flow.json", { flow, undo: [], redo: [], saved: JSON.stringify(flow) });
  })();

  return {
    sync: () => [view(), known(), runsEvent(), galleryEvent()],
    /** An output's picture, as the shell would read it. @param {string} path */
    async file(path) {
      if (!drawn.has(path)) throw new Error(`${path}: not an image Diffuse may read`);
      const url = picture(drawn.get(path) ?? 0, 512, "image/png");
      return (await fetch(url)).arrayBuffer();
    },
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "diffuseOpen":
          if (!flows.has(command.path)) return [];
          if (!open.includes(command.path)) open.push(command.path);
          shown = command.path;
          return [view()];
        case "diffuseNew": {
          const path = `${command.name}.flow.json`;
          if (flows.has(path)) return [{ type: "notice", text: `${path} is already here`, level: "warn" }];
          const flow = { format: 1, note: null, outputs: null, nodes: [], links: [] };
          if (command.template) {
            for (const [type, at, values] of STARTER) apply(flow, { op: "add", type, at, values }, []);
            for (const [from, to] of STARTER_LINKS) apply(flow, { op: "connect", from, to }, []);
          }
          flows.set(path, { flow, undo: [], redo: [], saved: JSON.stringify(flow) });
          open.push(path);
          shown = path;
          return [view()];
        }
        case "diffuseClose": {
          const at = open.indexOf(command.path);
          if (at >= 0) open.splice(at, 1);
          if (shown === command.path) shown = open[open.length - 1] ?? null;
          return [view()];
        }
        case "diffuseEdit": {
          const doc = flows.get(command.path);
          if (!doc) return [];
          const next = copy(doc.flow);
          /** @type {string[]} */
          const added = [];
          try {
            for (const op of command.ops) apply(next, op, added);
          } catch (error) {
            return [{ type: "notice", text: String(/** @type {Error} */ (error).message), level: "warn" }, view()];
          }
          doc.undo.push(doc.flow);
          doc.redo = [];
          doc.flow = next;
          return [view(added)];
        }
        case "diffuseUndo":
        case "diffuseRedo": {
          const doc = shown ? flows.get(shown) : null;
          if (!doc) return [];
          const [from, to] = command.type === "diffuseUndo" ? [doc.undo, doc.redo] : [doc.redo, doc.undo];
          const back = from.pop();
          if (back) {
            to.push(doc.flow);
            doc.flow = back;
          }
          return [view()];
        }
        case "diffuseSave": {
          const doc = shown ? flows.get(shown) : null;
          if (doc) doc.saved = JSON.stringify(doc.flow);
          return [view()];
        }
        case "diffuseLook":
          setTimeout(() => deliver(known()), 30);
          return [];
        case "diffuseGallery":
          return [galleryEvent()];
        case "diffuseOpenImage":
        case "diffuseOpenGraph": {
          // The stand-in keeps each output's flow as it ran, for its graph.
          const graph = command.graph ?? ranWith.get(command.path);
          if (!graph) return [{ type: "notice", text: `${command.path} holds no graph`, level: "warn" }];
          const stem = String(command.name ?? command.path).split("/").pop().replace(/\.png$/, "").replace(/[^A-Za-z0-9_-]+/g, "-");
          let path = `${stem}.flow.json`;
          for (let n = 2; flows.has(path); n++) path = `${stem}-${n}.flow.json`;
          const flow = { format: 1, note: null, outputs: null, nodes: graph.nodes.map((/** @type {any} */ n, /** @type {number} */ i) => ({ ...copy(n), at: [16 + (i % 4) * 250, 16 + Math.floor(i / 4) * 260], seed: null, bypass: null })), links: copy(graph.links) };
          flows.set(path, { flow, undo: [], redo: [], saved: "" });
          open.push(path);
          shown = path;
          return [{ type: "notice", text: `Opened its graph as ${path} — ⌘S keeps it.`, level: "info" }, view()];
        }
        case "diffuseFolder":
          folder = command.folder || "~/Diffuse";
          return [view()];
        case "diffuseDelete": {
          flows.delete(command.path);
          const at = open.indexOf(command.path);
          if (at >= 0) open.splice(at, 1);
          if (shown === command.path) shown = open[open.length - 1] ?? null;
          return [{ type: "notice", text: `Moved ${command.path} to the Trash.`, level: "info" }, view()];
        }
        case "diffuseDeleteImages": {
          for (const p of command.paths) {
            const at = images.findIndex((i) => i.path === p);
            if (at >= 0) images.splice(at, 1);
            drawn.delete(p);
            for (const r of runs) r.outputs = r.outputs.filter((/** @type {any} */ o) => o.path !== p);
          }
          return [runsEvent(), galleryEvent()];
        }
        case "diffuseCancel": {
          const r = runs.find((x) => x.id === command.id);
          if (r && !["done", "failed", "cancelled"].includes(r.state)) {
            r.state = "cancelled";
            r.progress = null;
            r.preview = null;
          }
          return [runsEvent()];
        }
        case "diffuseRun": {
          const doc = flows.get(command.path);
          if (!doc) return [];
          const flow = doc.flow;
          const r = { id: ++runCount, run: `preview-${runCount}`, flow: command.path, state: "sending", position: null, nodes: [], progress: null, preview: null, outputs: [], message: null, seconds: null, started: Date.now() };
          runs.unshift(r);
          const sampler = flow.nodes.find((/** @type {any} */ n) => n.type === "KSampler");
          const seed = Number(sampler?.values.seed ?? 0);
          const steps = Number(sampler?.values.steps ?? 30);
          const saves = flow.nodes.filter((/** @type {any} */ n) => n.type === "SaveImage" || n.type === "PreviewImage");
          /** @type {(() => void)[]} */
          const beats = [() => Object.assign(r, { state: "queued", position: 0 }), () => Object.assign(r, { state: "running", position: null })];
          for (const n of flow.nodes.filter((/** @type {any} */ n) => n.type !== "KSampler" && !saves.includes(n))) {
            beats.push(() => r.nodes.push({ id: n.id, state: "running", seconds: null }));
            beats.push(() => Object.assign(r.nodes.find((/** @type {any} */ x) => x.id === n.id), { state: "done", seconds: 0.2 }));
          }
          if (sampler) {
            beats.push(() => r.nodes.push({ id: sampler.id, state: "running", seconds: null }));
            for (let step = 1; step <= steps; step += Math.max(1, Math.floor(steps / 10))) {
              beats.push(() => {
                r.progress = { node: sampler.id, step, steps };
                r.preview = picture(seed, 96, "image/jpeg");
              });
            }
            beats.push(() => {
              Object.assign(r.nodes.find((/** @type {any} */ x) => x.id === sampler.id), { state: "done", seconds: 1.6 });
              r.progress = null;
              r.preview = null;
            });
          }
          for (const n of saves) {
            beats.push(() => {
              const kept = n.type === "SaveImage";
              const stem = command.path.replace(/\.flow\.json$/, "").split("/").pop();
              const prefix = String(n.values.prefix ?? stem);
              const number = images.filter((i) => i.path.startsWith(`outputs/${stem}/${prefix}_`)).length + 1;
              const out = kept ? `outputs/${stem}/${prefix}_${String(number).padStart(4, "0")}.png` : `.proton/diffuse/previews/preview-${r.id}.png`;
              drawn.set(out, seed);
              ranWith.set(out, { nodes: flow.nodes.map((/** @type {any} */ x) => ({ id: x.id, type: x.type, values: copy(x.values) })), links: copy(flow.links) });
              r.nodes.push({ id: n.id, state: "done", seconds: 0.1 });
              r.outputs.push({ node: n.id, path: out, width: 1024, height: 1024, kept });
              if (kept) {
                const text = (/** @type {string} */ input) => {
                  const link = flow.links.find((/** @type {any} */ l) => l.to === `${sampler?.id}:${input}`);
                  return flow.nodes.find((/** @type {any} */ x) => x.id === link?.from.split(":")[0])?.values.text ?? null;
                };
                images.unshift({ path: out, folder: `outputs/${stem}`, madeAt: Math.floor(Date.now() / 1000), width: 1024, height: 1024, graph: true,
                  made: { seed, steps, cfg: sampler?.values.cfg ?? null, sampler: sampler?.values.sampler ?? null, scheduler: sampler?.values.scheduler ?? null, denoise: sampler?.values.denoise ?? null,
                    checkpoint: flow.nodes.find((/** @type {any} */ x) => x.type === "LoadCheckpoint")?.values.checkpoint ?? null, prompt: text("positive"), negative: text("negative") } });
              }
            });
          }
          beats.push(() => Object.assign(r, { state: "done", seconds: 2.4 }));
          let at = 0;
          const beat = () => {
            if (r.state === "cancelled") return;
            beats[at++]();
            deliver(runsEvent());
            if (r.state === "done") deliver(galleryEvent());
            if (at < beats.length) setTimeout(beat, 140);
          };
          setTimeout(beat, 140);
          // The seeds that move after a run, moved.
          for (const n of flow.nodes) {
            if (n.seed === "increment") n.values.seed = Number(n.values.seed ?? 0) + 1;
            if (n.seed === "random") n.values.seed = Math.floor(Math.random() * 2 ** 32);
          }
          return [runsEvent(), view()];
        }
        default:
          return [];
      }
    },
  };
}
