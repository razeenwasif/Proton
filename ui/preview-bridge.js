/* Legacy browser-only fixtures. The native bridge never loads this module.
 * Its public boundary is described in preview-bridge.d.ts; the fixture
 * implementation remains outside the incremental strict-check boundary. */
import {encodeMesh} from './meshbytes.js';
import {createClaudePreview} from './preview-claude.js';

export function createPreview(deliver, mockLatency) {
  let mockMesh = () => null;
  let send;
  const claude = createClaudePreview(deliver);
  // Browser preview. Enough of the modal engine to keep the design prototype
  // honest — it must never grow past that: the real state machine is in
  // crates/proton-core/src/vim.rs and this is a demo stand-in, not a fork.
  const state = { mode: "codex", vim: "Normal", cmdline: "", prompt: ":", pattern: "", leader: null };
  const LABEL = {
    Normal: "NORMAL", Insert: "INSERT", Visual: "VISUAL",
    VisualLine: "V-LINE", Replace: "REPLACE", Command: "COMMAND",
  };
  const vimEvent = () => ({ type: "vimMode", mode: state.vim, label: LABEL[state.vim] });

  // Read-only file viewing, so the design preview shows real code instead of
  // an empty pane. It cannot edit — the rope lives in Rust — and it must never
  // learn how to.
  const doc = { lines: [""], first: 0, height: 60, name: "[No Name]", path: null };
  // One split in the preview. The real layout — how many, which buffer each
  // shows, whose cursor is where — lives in crates/proton-core/src/split.rs;
  // this only has to give the chrome something to draw.
  const layout = { count: 1, focused: 0, vertical: true, scroll: [0] };
  // Staged changes for the preview. Filled on first use, so the panel is
  // empty until something asks for it — the way it is in the real thing.
  const staged = {};

  // A canned queue, so the Tasks panel can be designed against. The real one
  // is in crates/proton-core/src/tasks.rs and runs the turns for real.
  let queue = [
    { id: 0, prompt: "Memoise the fleet grid", state: "running",
      note: "Looking at how ranked is rebuilt", tools: 3, files: [] },
    { id: 1, prompt: "Port trajectory.c to the Android target", state: "queued",
      note: "", tools: 0, files: [] },
    { id: 2, prompt: "Ingest six months of telemetry", state: "ready",
      note: "Parquet written, schema inferred, 3 nulls flagged", tools: 6,
      files: ["src/lib/telemetry.ts", "src/schema.json"] },
    { id: 3, prompt: "Explain the pointer bug in trajectory.c", state: "failed",
      note: "no ANTHROPIC_API_KEY", tools: 0, files: [] },
  ];
  const tasksEvent = () => ({ type: "tasks", tasks: queue });

  /* A notebook for the design preview, in the shape the core sends one: a
     dataframe as rows rather than HTML, a stream, and a traceback. The
     fleet numbers are the ones the prototype used, so "open sat 17 in
     Codex" still means something to look at. */
  const notebook = {
    path: "C:/fleet/fleet_health.ipynb",
    cells: [
      {
        kind: "markdown",
        source: "# Fleet health\n\nBattery mean, by satellite.",
        outputs: [],
        executionCount: null,
        running: false,
        selected: false,
      },
      {
        kind: "code",
        source:
          'import pandas as pd\ndf = pd.read_parquet("telemetry.parquet")\ndf.groupby("sat_id").battery.mean().head()',
        outputs: [
          {
            type: "table",
            table: {
              columns: ["sat_id", "mean_v", "n"],
              rows: [
                ["17", "3.41", "128400"],
                ["31", "3.58", "128112"],
                ["4", "3.74", "129001"],
                ["22", "3.88", "127980"],
              ],
              total: 4213887,
            },
          },
        ],
        executionCount: 1,
        running: false,
        selected: true,
      },
      {
        kind: "code",
        source: 'print("checked", len(df), "rows")\ndf.battery.min()',
        outputs: [
          { type: "stream", name: "stdout", text: "checked 4213887 rows\n" },
          { type: "text", text: "2.94" },
        ],
        executionCount: 2,
        running: false,
        selected: false,
      },
      {
        kind: "code",
        source: 'df["missing"].mean()',
        outputs: [
          {
            type: "error",
            ename: "KeyError",
            evalue: "'missing'",
            traceback: [
              "Traceback (most recent call last):",
              '  File "<stdin>", line 1, in <module>',
              "KeyError: 'missing'",
            ],
          },
        ],
        executionCount: 3,
        running: false,
        selected: false,
      },
    ],
    problem: "",
  };
  /* A theme, so Design mode has a palette to show without a core behind it.
     These are the real tokens from `styles.css`, in the scopes they are
     actually declared in — a mock palette of invented names would be the same
     mistake Data mode's sidebar used to make. */
  const theme = {
    path: "C:/proton/ui/styles.css",
    selected: 0,
    problem: "",
    tokens: [
      { name: "--bg", value: "#16161e", scope: ":root", line: 20 },
      { name: "--bg-deep", value: "#0f0f16", scope: ":root", line: 21 },
      { name: "--accent", value: "#7aa2f7", scope: ":root", line: 26 },
      { name: "--accent-2", value: "#bb9af7", scope: ":root", line: 27 },
      { name: "--ok", value: "#9ece6a", scope: ":root", line: 28 },
      { name: "--warn", value: "#e0af68", scope: ":root", line: 29 },
      { name: "--err", value: "#f7768e", scope: ":root", line: 30 },
      {
        name: "--shadow",
        value: "0 1px 2px rgba(0, 0, 0, .4)",
        scope: ":root",
        line: 31,
      },
      { name: "--accent", value: "#89b4fa", scope: ':root[data-theme="catppuccin"]', line: 62 },
      { name: "--accent", value: "#83a598", scope: ':root[data-theme="gruvbox-dark"]', line: 104 },
    ],
  };

  /* Settings, so the page has something to read and write without a core.
     Starts empty on purpose: the panels have to render correctly when nothing
     has been chosen, which is every first run. */
  const settings = {};
  /* Conversations, so Chat has a list to click without a core behind it.
     Two of them, one from each provider, because the row shows which. */
  const chats = {
    current: ".proton/chats/why-the-grid-janks.json",
    conversations: [
      {
        path: ".proton/chats/why-the-grid-janks.json",
        title: "Why the fleet grid janks",
        turns: 6,
        provider: "claude",
      },
      {
        path: ".proton/chats/telemetry-glossary.json",
        title: "Fleet telemetry glossary",
        turns: 2,
        provider: "gemini",
      },
    ],
  };
  const chatsEvent = () => ({
    type: "chats",
    conversations: chats.conversations,
    current: chats.current,
  });

  const settingsEvent = () => ({
    type: "settings",
    values: settings,
    path: "C:/proton/.proton/settings.json",
  });

  const themeEvent = () => ({
    type: "theme",
    path: theme.path,
    tokens: theme.tokens,
    selected: theme.selected,
    problem: theme.problem,
  });

  // A component for the artboard, so Design mode can be designed against
  // something with real markup in it rather than an empty frame. It names
  // tokens through `var()` with fallbacks, the way somebody else's component
  // would — the artboard has to look right against a palette it did not
  // choose.
  const component = {
    path: "C:/proton/ui/components/card.html",
    document: false,
    problem: "",
    source: [
      '<article class="card">',
      '  <h3>Pricing</h3>',
      '  <p>A component file, drawn with the project\u2019s own palette.</p>',
      '  <button>Choose plan</button>',
      '</article>',
      '<style>',
      '  .card { max-width: 340px; padding: 20px; border-radius: 12px;',
      '          border: 1px solid var(--accent, #7aa2f7);',
      '          background: var(--bg-deep, #0f0f16); }',
      '  .card h3 { margin: 0 0 6px; color: var(--accent, #7aa2f7); }',
      '  .card p { margin: 0 0 14px; opacity: .8; }',
      '  .card button { border: 0; border-radius: 7px; padding: 9px 16px;',
      '                 font: 600 13px system-ui;',
      '                 background: var(--accent, #7aa2f7); color: var(--bg, #16161e); }',
      '</style>',
    ].join("\n"),
  };

  // Written out rather than parsed. Everywhere else in this file a mock is a
  // fixture, and a second markup parser living here to serve one fixed
  // component would be a second thing to keep correct — the real one is
  // proton-design::markup and it has its own tests. The lines match the
  // source above; change one and change the other.
  const componentLayers = [
    { tag: "article", id: "", classes: ["card"], text: "", line: 1, depth: 0,
      leaf: false, path: "article:nth-child(1)" },
    { tag: "h3", id: "", classes: [], text: "Pricing", line: 2, depth: 1, leaf: true,
      path: "article:nth-child(1) > h3:nth-child(1)" },
    { tag: "p", id: "", classes: [], text: "A component file, drawn with the project\u2019s own p\u2026",
      line: 3, depth: 1, leaf: true, path: "article:nth-child(1) > p:nth-child(2)" },
    { tag: "button", id: "", classes: [], text: "Choose plan", line: 4, depth: 1, leaf: true,
      path: "article:nth-child(1) > button:nth-child(3)" },
    { tag: "style", id: "", classes: [], text: "", line: 6, depth: 0, leaf: true,
      path: "style:nth-child(2)" },
  ];

  // A scene for the outliner, so Modeling can be designed against a real
  // tree. Written out rather than parsed, like every other fixture here; the
  // real one is read from `scene.proton.json` by proton-scene.
  const scene = {
    path: "C:/proton/scene.proton.json",
    problem: "",
    selected: -1,
    nodes: [
      { id: "rig-1", name: "Satellite", kind: "empty", parent: "", depth: 0,
        children: 3, position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
      { id: "body-1", name: "Body", kind: "mesh", parent: "rig-1", depth: 1, material: "alu",
        mesh: "meshes/body.obj",
        children: 0, position: [0, 1.4, 0], rotation: [0, 32, 0], scale: [1, 1, 1] },
      { id: "array-1", name: "SolarArray", kind: "mesh", parent: "rig-1", depth: 1,
        mesh: "meshes/panel.obj",
        children: 0, position: [1.8, 1.4, 0], rotation: [0, 0, 0], scale: [4, 1, 1] },
      // A mesh naming a file nobody copied — the ordinary failure, and the one
      // the viewport has to keep drawing something for.
      { id: "dish-1", name: "Dish", kind: "mesh", parent: "rig-1", depth: 1,
        mesh: "meshes/dish.obj",
        children: 0, position: [-2, 1.4, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
      { id: "key-1", name: "Key Light", kind: "light", parent: "rig-1", depth: 1,
        children: 0, position: [3, 4, 2], rotation: [-40, 25, 0], scale: [1, 1, 1] },
      { id: "cam-1", name: "Camera", kind: "camera", parent: "", depth: 0,
        children: 0, position: [6, 3, 6], rotation: [-12, 45, 0], scale: [1, 1, 1] },
    ],
  };

  // One material, wired, so the graph panel can be designed against a real
  // shape. `body-1` wears it; every other object wears none, which is the
  // ordinary case and the one the panel has to handle too.
  const materials = [
    {
      id: "alu",
      name: "Brushed aluminium",
      nodes: [
        { id: "noise-1", kind: "noise", at: [14, 26],
          params: [{ name: "detail", value: { number: 2 } },
                   { name: "scale", value: { number: 6.4 } }] },
        { id: "ramp-1", kind: "ramp", at: [14, 104],
          params: [{ name: "fac", value: { number: 0.31 } }] },
        { id: "bsdf-1", kind: "principled", at: [244, 34],
          params: [{ name: "baseColor", value: { color: [0.28, 0.29, 0.31] } },
                   { name: "metallic", value: { number: 0.9 } },
                   { name: "roughness", value: { number: 0.24 } }] },
        { id: "out-1", kind: "output", at: [470, 62], params: [] },
      ],
      links: [
        { from: "noise-1:fac", to: "ramp-1:fac" },
        { from: "ramp-1:color", to: "bsdf-1:roughness" },
        { from: "bsdf-1:bsdf", to: "out-1:surface" },
      ],
    },
  ];

  // The same material, compiled. Written out by hand rather than compiled
  // here: `Material::compile` is Rust, and a second compiler in the preview
  // would be a thing that can disagree with the real one about what a graph
  // means. This is a fixture of its output — the noise into a ramp into
  // roughness, exactly as the graph above says.
  const shaders = [
    {
      id: "alu",
      ops: [
        { op: "position" },
        { op: "noise", at: 0, scale: 6.4, detail: 2 },
        { op: "constant", value: [0, 0, 0] },
        { op: "constant", value: [1, 1, 1] },
        { op: "mix", a: 2, b: 3, fac: 1 },
        { op: "constant", value: [0.28, 0.29, 0.31] },
        { op: "constant", value: [0.9, 0.9, 0.9] },
      ],
      base: 5,
      metallic: 6,
      roughness: 4,
      problem: "",
    },
  ];

  /* Keep the hand-written program above in step with the graph beside it.
   *
   * Not a compiler, and deliberately not: `Material::compile` is Rust and a
   * second one here would be a thing that can disagree with it about what a
   * graph means. This only knows where *this fixture* put the constants that
   * this fixture's three settings became, which is a fact about the fixture. */
  const WHERE = { baseColor: 5, metallic: 6, "noise.scale": 1, "noise.detail": 1 };
  const keepShaderInStep = (node, param, value) => {
    const shader = shaders[0];
    if (node === "bsdf-1") {
      const at = WHERE[param];
      if (at === undefined) return;
      const n = value.number ?? null;
      shader.ops[at] = {
        op: "constant",
        value: n === null ? [...value.color] : [n, n, n],
      };
    } else if (node === "noise-1") {
      const n = value.number;
      if (typeof n !== "number") return;
      shader.ops[1] = { ...shader.ops[1], [param]: n };
    }
  };

  // A Godot scene for the hierarchy, so Game mode can be designed against a
  // real tree. Written out like every other fixture here; the real one is read
  // from a `.tscn` by proton-game.
  const levelSource = `[gd_scene load_steps=2 format=3 uid="uid://bq2x8k1n0v"]

[ext_resource type="Texture2D" path="res://art/tiles.png" id="1_x8k"]

[node name="World" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(196, 286)
speed = 400.0

[node name="Sprite" type="Sprite2D" parent="Player"]
texture = ExtResource("1_x8k")

[node name="Camera" type="Camera2D" parent="."]
`;

  const level = {
    path: "C:/game/scenes/world.tscn",
    problem: "",
    selected: -1,
    source: levelSource,
    nodes: [
      { name: "World", kind: "Node2D", path: "", parent: "", depth: 0, children: 2, line: 5,
        properties: [] },
      { name: "Player", kind: "CharacterBody2D", path: "Player", parent: ".", depth: 1,
        children: 1, line: 7,
        properties: [{ name: "position", value: "Vector2(196, 286)", line: 8 },
                     { name: "speed", value: "400.0", line: 9 }] },
      { name: "Sprite", kind: "Sprite2D", path: "Player/Sprite", parent: "Player", depth: 2,
        children: 0, line: 11,
        properties: [{ name: "texture", value: 'ExtResource("1_x8k")', line: 12 }] },
      { name: "Camera", kind: "Camera2D", path: "Camera", parent: ".", depth: 1,
        children: 0, line: 14, properties: [] },
    ],
  };

  // A cut and its captions, so Video mode can be designed against real
  // documents. Written out like every other fixture here; the real ones are
  // read from an `.edl` and an `.srt` by proton-video.
  const cutSource = `TITLE: LAUNCH SEQUENCE
FCM: NON-DROP FRAME

001  AX       V     C        00:00:00:00 00:00:12:10 00:00:00:00 00:00:12:10
* FROM CLIP NAME: launch_a.mov

002  BX       V     C        00:00:04:00 00:00:09:12 00:00:12:10 00:00:17:22
* FROM CLIP NAME: ground_cam.mp4

003  AX       AA    C        00:00:00:00 00:00:17:22 00:00:00:00 00:00:17:22
* FROM CLIP NAME: vo_take3.wav
`;

  // The rate is what turns a timecode into a frame number, so it lives in a
  // variable the conversion reads rather than being baked into it. The real
  // one re-parses the whole list when the rate changes; a mock that kept its
  // frame counts would draw a timeline the core would never produce.
  let fps = 24;
  const at = (tc) => {
    const [h, m, sec, f] = tc.split(":").map(Number);
    return ((h * 60 + m) * 60 + sec) * fps + f;
  };
  const clip = (event, reel, channel, tracks, name, sIn, sOut, rIn, rOut) => ({
    event, reel, channel, tracks, name, transition: "C", duration: "", source: "",
    sourceIn: sIn, sourceOut: sOut, recordIn: rIn, recordOut: rOut,
    start: at(rIn), end: at(rOut), line: 0,
  });

  const cut = {
    path: "C:/launch/edit/launch.edl",
    captionsPath: "C:/launch/edit/launch.srt",
    title: "LAUNCH SEQUENCE",
    problem: "",
    selected: -1,
    source: cutSource,
    rate: { fps, drop: false },
    clips: [
      clip("001", "AX", "V", ["V1"], "launch_a.mov",
           "00:00:00:00", "00:00:12:10", "00:00:00:00", "00:00:12:10"),
      clip("002", "BX", "V", ["V1"], "ground_cam.mp4",
           "00:00:04:00", "00:00:09:12", "00:00:12:10", "00:00:17:22"),
      clip("003", "AX", "AA", ["A1", "A2"], "vo_take3.wav",
           "00:00:00:00", "00:00:17:22", "00:00:00:00", "00:00:17:22"),
    ],
    tracks: ["V1", "A1", "A2"],
    captions: [
      { index: "1", start: "00:00:02,000", end: "00:00:05,500", startMs: 2000,
        endMs: 5500, text: "...and that is the burn complete.", line: 2 },
      { index: "2", start: "00:00:06,120", end: "00:00:09,000", startMs: 6120,
        endMs: 9000, text: "Orbital insertion confirmed.", line: 6 },
    ],
    // One source really on disk and two not, which is the state a cut handed
    // over on its own always arrives in.
    media: [
      { name: "launch_a.mov", path: "C:/launch/edit/media/launch_a.mov", reel: "AX",
        uses: 1, found: true, audio: false },
      { name: "ground_cam.mp4", path: "", reel: "BX", uses: 1, found: false, audio: false },
      { name: "vo_take3.wav", path: "", reel: "AX", uses: 1, found: false, audio: true },
    ],
  };
  cut.length = Math.max(...cut.clips.map((one) => one.end));

  /** The event line one clip is on, for splicing it the way the real one does. */
  const cutSourceLine = (one) =>
    cut.source
      .split("\n")
      .find((line) => line.startsWith(one.event) && line.includes(one.recordIn)) ?? "";

  /** Frame numbers, from the timecodes, at whatever rate is current. */
  const recount = () => {
    for (const one of cut.clips) {
      one.start = at(one.recordIn);
      one.end = at(one.recordOut);
    }
    cut.length = Math.max(...cut.clips.map((one) => one.end));
  };

  const cutEvent = () => ({ type: "cut", ...cut });

  const levelEvent = () => ({
    type: "level",
    path: level.path,
    nodes: level.nodes,
    source: level.source,
    selected: level.selected,
    problem: level.problem,
  });

  /* ── geometry ─────────────────────────────────────
   *
   * Built here rather than read from a file, because the preview has no disk.
   * The shapes are the two the mock's objects wear: a solid, so culling is
   * exercised, and a sheet, so the case that must *not* be culled is too. */

  /** A box, as eight corners and twelve triangles. Closed. */
  const boxMesh = (w, h, d) => {
    const positions = [];
    for (const x of [-w / 2, w / 2]) {
      for (const y of [-h / 2, h / 2]) {
        for (const z of [-d / 2, d / 2]) positions.push(x, y, z);
      }
    }
    // Corner n is bit 2 = x, bit 1 = y, bit 0 = z.
    const quad = (a, b, c, e) => [a, b, c, a, c, e];
    const triangles = [
      ...quad(0, 1, 3, 2), ...quad(4, 6, 7, 5),
      ...quad(0, 4, 5, 1), ...quad(2, 3, 7, 6),
      ...quad(0, 2, 6, 4), ...quad(1, 5, 7, 3),
    ];
    return {
      positions, triangles, skipped: 0, closed: true,
      bounds: [-w / 2, -h / 2, -d / 2, w / 2, h / 2, d / 2],
    };
  };

  /** A flat panel: two triangles and a rim, so it is *not* closed. */
  const panelMesh = (w, d) => ({
    positions: [-w / 2, 0, -d / 2, w / 2, 0, -d / 2, w / 2, 0, d / 2, -w / 2, 0, d / 2],
    triangles: [0, 1, 2, 0, 2, 3],
    skipped: 0,
    closed: false,
    bounds: [-w / 2, 0, -d / 2, w / 2, 0, d / 2],
  });

  const meshes = {
    "meshes/body.obj": boxMesh(1.2, 1.6, 1.2),
    "meshes/panel.obj": panelMesh(1, 0.5),
  };

  /* ── the modifier stack ──────────────────────────────
   *
   * Enough of an evaluator for the panel to be designed against geometry that
   * really changes. Mirror and array are exact and are three lines each.
   *
   * Subdivide here splits every triangle at its edge midpoints and stops: the
   * count goes up four times a level, as it should, and the shape does *not*
   * smooth. The real one is Loop subdivision in `proton-mesh`, and this is a
   * stand-in for a preview with no Rust behind it — said out loud so that a
   * picture from this file is never mistaken for a picture from the core. */

  const stackOf = (node) =>
    (node?.modifiers ?? []).filter((one) => one.known && !one.off);

  const runStack = (mesh, stack) => {
    let out = { ...mesh, positions: [...mesh.positions], triangles: [...mesh.triangles] };
    for (const one of stack) {
      if (one.kind === "mirror") {
        const axis = { x: 0, y: 1, z: 2 }[one.axis] ?? 0;
        const was = out.positions.length / 3;
        const twin = [];
        for (let v = 0; v < was; v++) {
          const p = out.positions.slice(v * 3, v * 3 + 3);
          if (one.weld && Math.abs(p[axis]) < 1e-6) {
            twin.push(v);
            continue;
          }
          twin.push(out.positions.length / 3);
          p[axis] = -p[axis];
          out.positions.push(...p);
        }
        const faces = [...out.triangles];
        for (let t = 0; t < faces.length; t += 3) {
          // Two corners swapped: a reflection reverses orientation.
          out.triangles.push(twin[faces[t]], twin[faces[t + 2]], twin[faces[t + 1]]);
        }
      } else if (one.kind === "array") {
        const count = Math.max(1, one.count | 0);
        const was = out.positions.length / 3;
        const pos = [...out.positions];
        const tri = [...out.triangles];
        out.positions = [];
        out.triangles = [];
        for (let copy = 0; copy < count; copy++) {
          for (let i = 0; i < pos.length; i++) {
            out.positions.push(pos[i] + one.offset[i % 3] * copy);
          }
          out.triangles.push(...tri.map((corner) => corner + was * copy));
        }
      } else if (one.kind === "subdivide") {
        for (let level = 0; level < Math.min(4, one.levels | 0); level++) {
          const mid = new Map();
          const tri = [...out.triangles];
          out.triangles = [];
          const between = (a, b) => {
            const at = `${Math.min(a, b)}:${Math.max(a, b)}`;
            if (mid.has(at)) return mid.get(at);
            const made = out.positions.length / 3;
            for (let k = 0; k < 3; k++) {
              out.positions.push((out.positions[a * 3 + k] + out.positions[b * 3 + k]) / 2);
            }
            mid.set(at, made);
            return made;
          };
          for (let t = 0; t < tri.length; t += 3) {
            const [a, b, c] = [tri[t], tri[t + 1], tri[t + 2]];
            const [ab, bc, ca] = [between(a, b), between(b, c), between(c, a)];
            out.triangles.push(a, ab, ca, b, bc, ab, c, ca, bc, ab, bc, ca);
          }
        }
      }
    }
    // The box moved, and framing reads it.
    const lo = [Infinity, Infinity, Infinity];
    const hi = [-Infinity, -Infinity, -Infinity];
    for (const v of out.triangles) {
      for (let k = 0; k < 3; k++) {
        lo[k] = Math.min(lo[k], out.positions[v * 3 + k]);
        hi[k] = Math.max(hi[k], out.positions[v * 3 + k]);
      }
    }
    out.bounds = [...lo, ...hi];
    return out;
  };

  /** The key an object asks for its geometry by — the same rule the core uses. */
  const keyOf = (node) =>
    !node.mesh || !stackOf(node).length ? node.mesh ?? "" : `${node.mesh}@${node.id}`;

  const geometryEvent = (path) => {
    // `chair.obj@chair-3` is that object's copy of it.
    const [file, id] = path.includes("@") ? path.split("@") : [path, null];
    const base = meshes[file];
    const node = id ? scene.nodes.find((one) => one.id === id) : null;
    const mesh = base ? runStack(base, stackOf(node)) : null;
    if (!mesh) {
      // Exactly what the core sends for a mesh nobody copied.
      return {
        type: "geometry",
        path,
        ready: false,
        bounds: [0, 0, 0, 0, 0, 0],
        triangles: 0,
        problem: `${file}: not there`,
      };
    }
    // A description, as the core sends it. The mesh itself follows through
    // `readGeometry`, as bytes, the same as it does from the shell.
    return {
      type: "geometry",
      path,
      ready: true,
      bounds: mesh.bounds,
      triangles: mesh.triangles.length / 3,
      problem: "",
    };
  };
  mockMesh = (path) => {
    const [file, id] = path.includes("@") ? path.split("@") : [path, null];
    const base = meshes[file];
    const node = id ? scene.nodes.find((one) => one.id === id) : null;
    return base ? runStack(base, stackOf(node)) : null;
  };

  const sceneEvent = () => ({
    type: "scene",
    materials,
    shaders,
    path: scene.path,
    // `geometry` is derived, not stored — the same rule the core follows, so
    // the viewport looks a result up by the same key either way.
    nodes: scene.nodes.map((one) => ({ ...one, geometry: keyOf(one) })),
    selected: scene.selected,
    problem: scene.problem,
    meshes: [...new Set(scene.nodes.map(keyOf).filter(Boolean))],
  });

  const artboardEvent = () => ({
    type: "artboard",
    path: component.path,
    source: component.source,
    document: component.document,
    problem: component.problem,
    layers: componentLayers,
    selected: component.selected ?? -1,
  });

  const notebookEvent = () => ({
    type: "notebook",
    path: notebook.path,
    cells: notebook.cells,
    problem: notebook.problem,
  });

  // A canned pair of plugins, so the settings panel and the theme strip can
  // be designed against. The real ones are wasm components loaded by
  // crates/proton-plugin; this only has to show the chrome.
  const pluginsEvent = () => ({
    type: "plugins",
    items: [
      { id: "dusk", name: "Dusk", description: "A theme pack: two palettes, no code.",
        version: "0.1.0", provides: "2 themes", problem: "" },
      { id: "hello", name: "Hello", description: "The example plugin.",
        version: "0.1.0", provides: "1 theme · 3 commands", problem: "" },
      { id: "broken", name: "broken", description: "", version: "",
        provides: "", problem: "broken: expected a component, found a module" },
    ],
    themes: [
      { id: "dusk-rose", name: "Dusk Rose", dark: true, from: "dusk",
        colours: [["bg", "#1f1a1d"], ["bg-deep", "#151113"], ["text", "#e0d4d9"],
                  ["text-hi", "#f6ecf0"], ["accent", "#d98a9b"]] },
      { id: "dusk-moss", name: "Dusk Moss", dark: true, from: "dusk",
        colours: [["bg", "#1a1f1b"], ["bg-deep", "#121613"], ["text", "#d2ded4"],
                  ["text-hi", "#eef5ef"], ["accent", "#8fbf9f"]] },
    ],
  });

  const splitsEvent = () => ({
    type: "splits",
    views: Array.from({ length: layout.count }, () => ({ buffer: 0, name: doc.name })),
    focused: layout.focused,
    vertical: layout.vertical,
  });

  /// Every pane's content. The preview shows one document in all of them —
  /// the real thing gives each view its own buffer — but the scroll is per
  /// pane, which is the part the chrome has to get right.
  const everyViewport = () =>
    Array.from({ length: layout.count }, (_, i) => ({
      ...viewportEvent(),
      view: i,
      firstLine: layout.scroll[i] ?? 0,
      lines: doc.lines.slice(layout.scroll[i] ?? 0, (layout.scroll[i] ?? 0) + doc.height),
    }));

  const viewportEvent = () => ({
    type: "viewport",
    view: 0,
    firstLine: doc.first,
    lines: doc.lines.slice(doc.first, doc.first + doc.height),
    totalLines: doc.lines.length,
    version: 0,
    highlights: [],
    // A couple of breakpoints, so the sign column has something to draw.
    stops: [{ line: 3, verified: true, here: true }, { line: 9, verified: false, here: false }],
    // A fixed gutter, so the strip has something to draw. The real marks are
    // a line diff against HEAD in crates/proton-vcs.
    git: doc.lines.slice(doc.first, doc.first + doc.height).map((_, i) =>
      i === 2 ? "added" : i === 3 ? "modified" : i === 7 ? "removed" : null),
    // Literal, folded like the real one in crates/proton-text/src/search.rs.
    // Enough to draw the layer; the rule that matters lives in Rust.
    matches: doc.lines.slice(doc.first, doc.first + doc.height).map((line) => {
      if (!state.pattern) return [];
      const sensitive = /[A-Z]/.test(state.pattern);
      const hay = sensitive ? line : line.toLowerCase();
      const needle = sensitive ? state.pattern : state.pattern.toLowerCase();
      const hits = [];
      for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
        hits.push({ start: at, end: at + needle.length });
      }
      return hits;
    }),
  });

  // A static set of projects, so the workspace rail has something to draw in
  // the preview. Fixed data on purpose: the session — what is open, what is
  // remembered, what a folder is called when two share a name — lives in
  // crates/proton-core/src/session.rs, and this only has to show the chrome.
  const projects = {
    active: 0,
    items: [
      { name: "Proton", path: "C:\\Users\\Razeen\\Projects\\Proton", modified: false },
      { name: "orbit/web", path: "C:\\Users\\Razeen\\Projects\\orbit\\web", modified: true },
      { name: "scratch", path: "C:\\Users\\Razeen\\scratch", modified: false },
    ],
  };
  const workspacesEvent = () => ({ type: "workspaces", ...projects });

  // A fixed tree, so the sidebar has rows to draw and folders to open. The
  // real one is computed from the disk in crates/proton-core/src/tree.rs and
  // honours the project's ignore rules; this only has to show the chrome.
  const FILES = {
    "": ["crates/", "ui/", "docs/", "Cargo.toml", "README.md"],
    "crates": ["proton-core/", "proton-text/"],
    "crates/proton-core": ["src/"],
    "crates/proton-core/src": ["vim.rs", "workspace.rs", "tree.rs", "ex.rs"],
    "crates/proton-text": ["src/"],
    "crates/proton-text/src": ["buffer.rs", "edit.rs"],
    "ui": ["app.js", "bridge.js", "styles.css"],
    "docs": ["ARCHITECTURE.md", "STACK.md"],
  };
  const open = new Set(["crates", "crates/proton-core", "crates/proton-core/src"]);

  const treeRows = (dir = "", depth = 0, rows = []) => {
    for (const entry of FILES[dir] ?? []) {
      const isDir = entry.endsWith("/");
      const name = isDir ? entry.slice(0, -1) : entry;
      const path = dir ? `${dir}/${name}` : name;
      const expanded = isDir && open.has(path);
      rows.push({ depth, name, path, dir: isDir, expanded, active: path === doc.path, truncated: 0 });
      if (expanded) treeRows(path, depth + 1, rows);
    }
    return rows;
  };
  const treeEvent = () => ({ type: "tree", rows: treeRows() });

  // A canned terminal screen, so the panel can be designed against. The real
  // one is a pseudo-terminal in crates/proton-term; this only has to show the
  // chrome and the colours.
  const run = (text, extra = {}) => ({ text, fg: null, bg: null, rgb: null, bold: false, italic: false, underline: false, inverse: false, ...extra });
  const terminalEvent = () => ({
    type: "terminal",
    rows: [
      [run("C:\Users\Razeen\Projects\Proton> "), run("cargo test", { bold: true })],
      [run("   Compiling proton-term v0.0.1", { fg: 8 })],
      [run("    Finished ", { fg: 2, bold: true }), run("test profile in 1.2s")],
      [run("running 7 tests")],
      [run("test result: "), run("ok", { fg: 2, bold: true }), run(". 7 passed; 0 failed")],
      [run("C:\Users\Razeen\Projects\Proton> ")],
    ],
    cursor: [5, 34],
    cursorVisible: true,
    running: true,
  });

  // A canned debug session, so the panel can be designed against. The real
  // one drives a debug adapter from crates/proton-core/src/debug.rs.
  const debugEvent = () => ({
    type: "debug",
    phase: "stopped",
    frames: [
      { name: "add", path: "src/demo.c", line: 4 },
      { name: "main", path: "src/demo.c", line: 11 },
      { name: "ntdll!RtlUserThreadStart", path: null, line: 0 },
    ],
    variables: [
      { name: "a", value: "0", expandable: false },
      { name: "b", value: "1", expandable: false },
      { name: "point", value: "{x = 3, y = 7}", expandable: true },
    ],
    output: ["total=1", "total=3"],
  });

  // A small file fixture, not a second filesystem index. Search interactions
  // use the same protocol as the shell so the preview exercises real controls.
  let filePicker = null;
  const previewFiles = ["README.md", "ui/styles.css", "ui/index.html", "crates/proton-core/src/vim.rs"];
  const filePickerEvent = () => {
    const items = previewFiles.filter((path) => path.toLowerCase().includes(filePicker.query.toLowerCase()))
      .map((path) => ({ label: path.split('/').pop(), detail: path, path, positions: [] }));
    filePicker.selected = Math.min(filePicker.selected, Math.max(0, items.length - 1));
    return { type: "picker", kind: "files", title: "Preview files", query: filePicker.query,
      items, selected: filePicker.selected, total: previewFiles.length, elapsedMs: 0, scanning: false };
  };
  send = async (command) => {
    if (command.type === "openPicker" && command.kind === "files") {
      filePicker = { query: "", selected: 0 };
      return [filePickerEvent()];
    }
    if (filePicker) {
      if (command.type === "pickerQuery") {
        filePicker.query = command.text;
        filePicker.selected = 0;
        return [filePickerEvent()];
      }
      if (command.type === "selectPick") {
        filePicker.selected = command.index;
        return [filePickerEvent()];
      }
      if (command.type === "key") {
        const key = command.key.key;
        if (key === "Escape") {
          filePicker = null;
          return [{ type: "pickerClosed" }];
        }
        if (key === "Enter") {
          const item = filePickerEvent().items[filePicker.selected];
          filePicker = null;
          return [{ type: "pickerClosed" }, ...(item ? await send({ type: "openBuffer", path: item.path }) : [])];
        }
        if (key === "ArrowDown") filePicker.selected++;
        if (key === "ArrowUp") filePicker.selected = Math.max(0, filePicker.selected - 1);
        return [filePickerEvent()];
      }
    }
    if (command.type === "sync") {
      return [
        { type: "state", mode: state.mode, vimMode: state.vim, vimLabel: LABEL[state.vim] },
        splitsEvent(),
        workspacesEvent(),
        treeEvent(),
        viewportEvent(),
        debugEvent(),
        terminalEvent(),
        tasksEvent(),
        pluginsEvent(),
        notebookEvent(),
        themeEvent(),
        artboardEvent(),
        sceneEvent(),
        levelEvent(),
        cutEvent(),
        settingsEvent(),
        chatsEvent(),
        ...claude.sync(),
      ];
    }
    if (command.type.startsWith("claude") || command.type === "forgetArtifact" || command.type === "openLink") {
      return claude.command(command);
    }
    if (command.type === "runCell") {
      // The shape of a real run: the cell goes busy, and the answer arrives
      // later on its own — which is what the panel has to survive.
      const cell = notebook.cells[command.index];
      if (cell) {
        cell.running = true;
        setTimeout(() => {
          cell.running = false;
          cell.executionCount = (cell.executionCount ?? 0) + 1;
          deliver(notebookEvent());
        }, 900);
      }
      return [notebookEvent()];
    }
    if (command.type === "openChat") {
      chats.current = command.path;
      return [chatsEvent()];
    }
    if (command.type === "newChat") {
      chats.current = "";
      return [chatsEvent()];
    }
    if (command.type === "deleteChat") {
      chats.conversations = chats.conversations.filter((c) => c.path !== command.path);
      if (chats.current === command.path) chats.current = "";
      return [chatsEvent()];
    }
    if (command.type === "renameChat") {
      const hit = chats.conversations.find((c) => c.path === command.path);
      if (hit) hit.title = command.title;
      return [chatsEvent()];
    }
    if (command.type === "selectClip") {
      cut.selected = command.index;
      return [cutEvent()];
    }
    if (command.type === "setClipField") {
      const one = cut.clips[command.index];
      if (one) {
        // Spliced, like the real one: the bytes that field occupies in the
        // line are replaced and no column is re-aligned.
        const was = one[command.field];
        if (was && was !== command.value) {
          const line = cutSourceLine(one);
          cut.source = cut.source.replace(line, line.replace(was, command.value));
          one[command.field] = command.value;
          if (command.field === "recordIn") one.start = at(command.value);
          if (command.field === "recordOut") one.end = at(command.value);
          cut.length = Math.max(...cut.clips.map((c) => c.end));
        }
      }
      return [cutEvent()];
    }
    if (command.type === "setCaptionField") {
      const cue = cut.captions[command.index];
      if (cue) cue[command.field] = command.value;
      return [cutEvent()];
    }
    if (command.type === "setTimelineRate") {
      fps = command.fps;
      cut.rate = { fps, drop: cut.rate.drop };
      recount();
      return [cutEvent()];
    }
    if (command.type === "selectGameNode") {
      level.selected = command.index;
      return [levelEvent()];
    }
    if (command.type === "setGameProperty") {
      const node = level.nodes[command.index];
      const prop = node?.properties.find((one) => one.name === command.name);
      if (prop) {
        level.source = level.source.replace(
          `${prop.name} = ${prop.value}`,
          `${prop.name} = ${command.value}`,
        );
        prop.value = command.value;
      }
      return [levelEvent()];
    }
    if (command.type === "loadMesh") {
      return [geometryEvent(command.path)];
    }
    if (command.type === "setNodeModifier") {
      const node = scene.nodes[command.index];
      const fresh = {
        mirror: { kind: "mirror", axis: "x", weld: true, known: true, off: false,
                  count: 2, offset: [1, 0, 0], levels: 1 },
        array: { kind: "array", axis: "x", weld: true, known: true, off: false,
                 count: 2, offset: [1, 0, 0], levels: 1 },
        subdivide: { kind: "subdivide", axis: "x", weld: true, known: true, off: false,
                     count: 2, offset: [1, 0, 0], levels: 1 },
      }[command.kind];
      if (!node || !fresh) {
        return [{ type: "notice", text: `${command.kind} is not a modifier this build knows`,
                  level: "error" }];
      }
      node.modifiers = [...(node.modifiers ?? []), { ...fresh }];
      return [sceneEvent(), geometryEvent(keyOf(node))];
    }
    if (command.type === "removeNodeModifier") {
      const node = scene.nodes[command.index];
      if (node?.modifiers) node.modifiers.splice(command.row, 1);
      return [sceneEvent(), geometryEvent(keyOf(node))];
    }
    if (command.type === "moveNodeModifier") {
      const node = scene.nodes[command.index];
      const stack = node?.modifiers;
      if (stack && command.row < stack.length) {
        const to = Math.min(Math.max(0, command.to), stack.length - 1);
        stack.splice(to, 0, ...stack.splice(command.row, 1));
      }
      return [sceneEvent(), geometryEvent(keyOf(node))];
    }
    if (command.type === "setModifierField") {
      const one = scene.nodes[command.index]?.modifiers?.[command.row];
      if (one) {
        const number = Number(command.value);
        if (command.field === "off" || command.field === "weld") {
          one[command.field] = command.value === "true";
        } else if (command.field === "axis") {
          one.axis = command.value;
        } else if (command.field === "count") {
          one.count = Math.max(1, Math.min(4096, number | 0));
        } else if (command.field === "levels") {
          one.levels = Math.max(0, Math.min(6, number | 0));
        } else if (command.field.startsWith("offset")) {
          const axis = { offsetX: 0, offsetY: 1, offsetZ: 2 }[command.field];
          if (Number.isFinite(number)) one.offset[axis] = number;
        }
      }
      const node = scene.nodes[command.index];
      return [sceneEvent(), geometryEvent(keyOf(node))];
    }
    if (command.type === "setNodeMesh") {
      const node = scene.nodes[command.index];
      if (node) node.mesh = command.path;
      return [sceneEvent()];
    }
    if (command.type === "selectNode") {
      scene.selected = command.index;
      return [sceneEvent()];
    }
    if (command.type === "renameNode") {
      const node = scene.nodes[command.index];
      if (node) node.name = command.name;
      return [sceneEvent()];
    }
    if (command.type === "setMaterialParam") {
      const m = materials.find((one) => one.id === command.material);
      const node = m?.nodes.find((one) => one.id === command.node);
      const param = node?.params.find((one) => one.name === command.param);
      if (param) param.value = command.value;
      keepShaderInStep(command.node, command.param, command.value);
      return [sceneEvent()];
    }
    if (command.type === "moveGraphNode") {
      const m = materials.find((one) => one.id === command.material);
      const node = m?.nodes.find((one) => one.id === command.node);
      if (node) node.at = command.at;
      return [sceneEvent()];
    }
    if (command.type === "connectPorts") {
      const m = materials.find((one) => one.id === command.material);
      if (m) {
        // The same two rules the real one enforces, so the preview refuses
        // what the core would refuse rather than showing a wire that will not
        // survive a reload.
        const node = (port) => port.split(":")[0];
        const reaches = (from, to) => {
          const seen = new Set();
          const stack = [from];
          while (stack.length) {
            const at = stack.pop();
            if (at === to) return true;
            if (seen.has(at)) continue;
            seen.add(at);
            for (const l of m.links) {
              if (node(l.from) === at) stack.push(node(l.to));
            }
          }
          return false;
        };
        if (node(command.from) === node(command.to)) return [];
        if (reaches(node(command.to), node(command.from))) return [];
        const existing = m.links.find((l) => l.to === command.to);
        if (existing) existing.from = command.from;
        else m.links.push({ from: command.from, to: command.to });
      }
      return [sceneEvent()];
    }
    if (command.type === "rewirePort") {
      const m = materials.find((one) => one.id === command.material);
      const moving = m?.links.find((l) => l.to === command.fromInput);
      if (m && moving) {
        m.links = m.links.filter((l) => l.to !== command.toInput);
        moving.to = command.toInput;
      }
      return [sceneEvent()];
    }
    if (command.type === "disconnectPort") {
      const m = materials.find((one) => one.id === command.material);
      if (m) m.links = m.links.filter((l) => l.to !== command.to);
      return [sceneEvent()];
    }
    if (command.type === "setNodeTransform") {
      const node = scene.nodes[command.index];
      if (node) node[command.part] = command.to;
      return [sceneEvent()];
    }
    if (command.type === "selectLayer") {
      component.selected = command.index;
      return [artboardEvent()];
    }
    // The reverse binding, in miniature: the mock's layer list is the fixture,
    // so an edit changes that and the source it came from. The real one
    // splices bytes; this only has to let the panel be designed against
    // something that responds.
    if (command.type === "setLayerText") {
      const layer = componentLayers[command.index];
      if (layer && layer.text) {
        component.source = component.source.replace(layer.text, command.text);
        layer.text = command.text;
      }
      return [artboardEvent()];
    }
    if (command.type === "setLayerAttribute") {
      const layer = componentLayers[command.index];
      if (layer) {
        if (command.name === "id") layer.id = command.value;
        else layer.classes = command.value.split(/\s+/).filter(Boolean);
      }
      return [artboardEvent()];
    }
    if (command.type === "setSetting") {
      const parts = command.key.split(".");
      const last = parts.pop();
      let node = settings;
      for (const part of parts) node = node[part] ??= {};
      if (command.value === null) delete node[last];
      else node[last] = command.value;
      return [settingsEvent()];
    }
    if (command.type === "setToken") {
      const hit = theme.tokens.find(
        (t) => t.name === command.name && t.scope === command.scope,
      );
      if (hit) hit.value = command.value;
      return [themeEvent()];
    }
    if (command.type === "selectToken") {
      theme.selected = Math.min(command.index, theme.tokens.length - 1);
      return [themeEvent()];
    }
    if (command.type === "selectCell") {
      notebook.cells.forEach((cell, i) => (cell.selected = i === command.index));
      return [notebookEvent()];
    }
    if (command.type === "runAll") {
      notebook.cells
        .filter((cell) => cell.kind === "code")
        .forEach((cell) => {
          cell.running = true;
          setTimeout(() => {
            cell.running = false;
            cell.executionCount = (cell.executionCount ?? 0) + 1;
            deliver(notebookEvent());
          }, 700 + Math.random() * 600);
        });
      return [notebookEvent()];
    }
    if (command.type === "setCell") {
      const cell = notebook.cells[command.index];
      if (cell) cell.source = command.source;
      return [notebookEvent()];
    }
    if (command.type === "addCell") {
      notebook.cells.push({
        kind: command.code ? "code" : "markdown",
        source: "",
        outputs: [],
        executionCount: null,
        running: false,
      });
      return [notebookEvent()];
    }
    if (command.type === "removeCell") {
      notebook.cells.splice(command.index, 1);
      return [notebookEvent()];
    }
    if (command.type === "dropTask") {
      queue = queue.filter((t) => t.id !== command.id || t.state === "running");
      return [tasksEvent()];
    }
    if (command.type === "clearTasks") {
      queue = queue.filter((t) => t.state === "running" || t.state === "queued");
      return [tasksEvent()];
    }
    // Staged changes. The stand-in keeps a list so Apply and Refer visibly
    // take a hunk off the panel, which is the interaction worth previewing.
    if (command.type === "review" || command.type === "applyHunk" ||
        command.type === "rejectHunk" || command.type === "applyStaged" ||
        command.type === "rejectStaged") {
      const line = (mark, text) => ({ mark, text });
      staged.files ??= [
        {
          path: "src/routes/dashboard.tsx",
          creates: false,
          hunks: [
            { line: 9, lines: [
              line("context", "const health = useTelemetry(sats, …);"),
              line("removed", "const ranked = [...sats].sort(byHealth);"),
              line("added", "const ranked = useMemo("),
              line("added", "  () => [...sats].sort(byHealth),"),
              line("added", "  [sats, health],"),
              line("added", ");"),
              line("context", "return <Grid rows={ranked} />;"),
            ]},
          ],
        },
        {
          path: "src/lib/orbit.ts",
          creates: false,
          hunks: [
            { line: 54, lines: [
              line("removed", "return { sats: [...store.sats], tick };"),
              line("added", "return { sats: store.sats, tick };  // stable identity"),
            ]},
          ],
        },
      ];
      if (command.type === "applyHunk" || command.type === "rejectHunk") {
        const file = staged.files.find((f) => f.path === command.path);
        if (file) {
          file.hunks.splice(command.hunk, 1);
          if (!file.hunks.length) staged.files = staged.files.filter((f) => f !== file);
        }
      }
      if (command.type === "applyStaged" || command.type === "rejectStaged") staged.files = [];
      return [{ type: "review", files: staged.files }];
    }

    if (command.type === "hover") {
      return [
        {
          type: "hover",
          text:
            "fn magnitude(&self) -> i32\n\n" +
            "How far the point is from the origin, as the crow flies.\n" +
            "Rounded down, because the return type says so.",
          line: doc.first + 4,
          column: 8,
        },
      ];
    }
    if (command.type === "closeHover") return [{ type: "hoverClosed" }];

    if (command.type === "codeActions") {
      const row = (label, detail) => ({
        label, detail, positions: [], path: null, line: 0, column: null, kind: null,
      });
      const items = [
        row("Extract into variable", "refactor.extract"),
        row("Fill match arms", "quickfix"),
        row("Convert to guarded return", "refactor.rewrite"),
        row("Run test", "needs a server command"),
      ];
      return [
        {
          type: "picker",
          kind: "actions",
          title: "code_actions",
          query: "",
          items,
          selected: 0,
          total: items.length,
          elapsedMs: 0.6,
          scanning: false,
        },
      ];
    }

    if (
      command.type === "findReferences" ||
      (command.type === "openPicker" && command.kind === "symbols")
    ) {
      const symbols = command.type !== "findReferences";
      const row = (label, detail, kind) => ({
        label,
        detail,
        positions: [],
        path: "src/lib.rs",
        line: 6,
        column: 4,
        kind: kind ?? null,
      });
      const items = symbols
        ? [
            row("Point", "", "type"),
            row("x", "Point  i32", "field"),
            row("magnitude", "Point  fn(&self) -> i32", "method"),
            row("describe", "fn(Point) -> i32", "function"),
          ]
        : [
            row("pub fn magnitude(&self) -> i32 {", "src/lib.rs:7"),
            row("p.magnitude()", "src/lib.rs:12"),
            row("assert_eq!(p.magnitude(), 7);", "tests/point.rs:9"),
          ];
      return [
        {
          type: "picker",
          kind: symbols ? "symbols" : "references",
          title: symbols ? "document_symbols" : "lsp_references",
          query: "",
          items,
          selected: 0,
          total: items.length,
          elapsedMs: 0.4,
          scanning: false,
        },
      ];
    }

    if (command.type === "split") {
      if (layout.count >= 4) {
        return [{ type: "notice", text: "four splits is as many as are worth having", level: "warn" }];
      }
      layout.vertical = command.vertical;
      layout.scroll.splice(layout.focused + 1, 0, layout.scroll[layout.focused] ?? 0);
      layout.count += 1;
      layout.focused += 1;
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "closeSplit") {
      if (layout.count < 2) {
        return [{ type: "notice", text: "that is the last split", level: "warn" }];
      }
      const index = command.index ?? layout.focused;
      layout.scroll.splice(index, 1);
      layout.count -= 1;
      if (index <= layout.focused) layout.focused = Math.max(0, layout.focused - 1);
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "onlySplit") {
      layout.scroll = [layout.scroll[layout.focused] ?? 0];
      layout.count = 1;
      layout.focused = 0;
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "focusSplit") {
      layout.focused = command.index;
      return [splitsEvent()];
    }
    if (command.type === "stepSplit") {
      const back = ["left", "up", "previous"].includes(command.direction);
      const next = back ? layout.focused - 1 : layout.focused + 1;
      if (next < 0 || next >= layout.count) return [];
      layout.focused = next;
      return [splitsEvent()];
    }
    if (command.type === "openTerminal") return [terminalEvent()];
    if (command.type === "closeTerminal") {
      return [{ type: "terminal", rows: [], cursor: [0, 0], cursorVisible: false, running: false }];
    }
    if (command.type === "terminalInput" || command.type === "terminalSize") return [];
    if (command.type === "focusWorkspace") {
      projects.active = command.index;
      return [workspacesEvent()];
    }
    if (command.type === "toggleTreeNode") {
      if (!open.delete(command.path)) open.add(command.path);
      return [treeEvent()];
    }
    if (command.type === "revealInTree") {
      if (!doc.path) return [{ type: "notice", text: "this buffer is not a file yet", level: "warn" }];
      // Every folder above it, so the row exists to be scrolled to.
      const parts = doc.path.split("/").slice(0, -1);
      parts.reduce((so_far, part) => {
        const path = so_far ? `${so_far}/${part}` : part;
        open.add(path);
        return path;
      }, "");
      return [treeEvent()];
    }
    if (command.type === "viewport") {
      doc.height = command.lines;
      layout.scroll[command.view] = command.firstLine;
      if (command.view === layout.focused) doc.first = command.firstLine;
      return [everyViewport()[command.view]].filter(Boolean);
    }
    if (command.type === "openBuffer") {
      try {
        const res = await fetch("/" + command.path);
        if (!res.ok) throw new Error(res.status);
        doc.lines = (await res.text()).split("\n");
        doc.name = command.path.split("/").pop();
        doc.path = command.path;
      } catch (err) {
        return [{ type: "notice", text: `${command.path}: ${err}`, level: "error" }];
      }
      return [
        { type: "buffers", names: [{ name: doc.name, path: command.path, modified: false }], active: 0 },
        viewportEvent(),
        splitsEvent(),
        { type: "cursor", view: 0, line: 0, column: 0, offset: 0, selection: null },
        // The tree marks where you are, so opening a file redraws it.
        treeEvent(),
      ];
    }
    if (command.type === "setMode") {
      state.mode = command.mode;
      return [{ type: "modeChanged", mode: state.mode }];
    }
    // A real traced step, dumped from the core by
    // `crates/proton-core/tests/dump_step.rs`. Real rather than invented,
    // because invented data is exactly what hides a rendering bug.
    if (command.type === "trace" || command.type === "traceSeek" || command.type === "traceStep") {
      try {
        const step = await fetch("/fixtures/trace-step.json").then((r) => r.json());
        return [step];
      } catch {
        return [{ type: "notice", text: "no trace fixture — run the dump test", level: "warn" }];
      }
    }

    // A canned agent turn, so the design preview can be designed against.
    // Deliberately scripted: the real conversation lives in proton-core and
    // this must never grow into a second one.
    if (command.type === "ask") {
      const you = { speaker: "you", text: command.text, thinking: "", calls: [], streaming: false };
      const reply = {
        speaker: "proton", thinking: "", streaming: true, text: "",
        calls: [{ id: "a", name: "grep", detail: `query=${command.text.split(" ")[0]}`, result: "", state: "running" }],
      };
      const agent = (activity, extra) => ({
        type: "agent", activity, inputTokens: 4210, outputTokens: 183, cachedTokens: 3904,
        provider: settings.agent?.provider ?? "claude",
        // Follows the provider when no model is named, as the core does — a
        // mock that answered "claude-opus-5" while the chip said Gemini would
        // be showing the exact bug this pane is meant to make visible.
        model:
          settings.agent?.model ??
          (settings.agent?.provider === "gemini" ? "gemini-pro-latest" : "claude-opus-5"),
        credential: "ANTHROPIC_API_KEY is set",
        transcript: [you, { ...reply, ...extra }],
      });
      // Stagger it so the streaming states are visible while designing them.
      setTimeout(() => deliver(agent("working", {})), 60);
      setTimeout(() => deliver(agent("thinking", {
        thinking: "The grid rebuilds its rows on every tick.",
        calls: [{ ...reply.calls[0], result: "6 hits (+5 more)", state: "done" }],
      })), 700);
      setTimeout(() => deliver(agent("idle", {
        streaming: false,
        thinking: "The grid rebuilds its rows on every tick.",
        calls: [{ ...reply.calls[0], result: "6 hits (+5 more)", state: "done" }],
        text:
          "`ranked` is rebuilt on every tick, so all 40 `Sparkline`s remount.\n\n" +
          "Memoise on `[sats, health]` and the grid stops re-laying out.",
      })), 1400);
      return [];
    }
    if (command.type === "cancelTurn") return [{ type: "agent", activity: "idle", inputTokens: 0, outputTokens: 0, cachedTokens: 0, transcript: [] }];

    if (command.type !== "key") return [];

    const k = command.key.key;
    const before = state.vim;

    if (k === "Escape") {
      state.cmdline = "";
      state.pattern = "";
      state.leader = null;
      state.vim = "Normal";
      // Abandoning a search stops the painting, as the real one does.
      return [{ type: "cmdline", text: null, prompt: state.prompt }, viewportEvent(), vimEvent()];
    }

    if (state.vim === "Command") {
      if (k === "Enter") {
        const text = state.cmdline;
        state.cmdline = "";
        state.vim = "Normal";
        return [{ type: "cmdlineRun", text }, vimEvent()];
      }
      if (k === "Backspace") state.cmdline = state.cmdline.slice(0, -1);
      else if (k.length === 1) state.cmdline += k;
      // Incremental: the pattern being typed is what gets painted.
      if (state.prompt !== ":") state.pattern = state.cmdline;
      return [{ type: "cmdline", text: state.cmdline, prompt: state.prompt }, viewportEvent()];
    }

    if (state.vim === "Insert" || state.vim === "Replace") {
      return k.length === 1 ? [{ type: "input", text: k }] : [];
    }

    if (state.leader !== null) {
      if (k.length === 1) state.leader += k;
      const prefix = state.leader;
      if (prefix.length >= 2) state.leader = null;
      return [{ type: "leader", prefix }];
    }

    if (k === "i" || k === "a" || k === "o") state.vim = "Insert";
    else if (k === "v") state.vim = state.vim === "Visual" ? "Normal" : "Visual";
    else if (k === "V") state.vim = state.vim === "VisualLine" ? "Normal" : "VisualLine";
    else if (k === ":" || k === "/" || k === "?") { state.vim = "Command"; state.cmdline = ""; state.prompt = k; }
    else if (k === " ") { state.leader = ""; return [{ type: "leader", prefix: "" }]; }

    if (state.vim === "Command") return [{ type: "cmdline", text: "", prompt: state.prompt }, vimEvent()];
    return state.vim === before ? [] : [vimEvent()];
  };
  return {send, async readGeometry(key) {
    const mesh = mockMesh(key);
    const wait = mockLatency.get(key) ?? 0;
    if (wait) await new Promise(done => setTimeout(done, wait));
    if (!mesh) throw new Error(`${key}: no geometry to send`);
    return encodeMesh(mesh);
  }};
}
