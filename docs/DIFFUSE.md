# Diffuse: image generation by node graph (design)

A small ComfyUI in Proton. You build a graph of nodes (load a checkpoint,
encode a prompt, sample, decode, save), run it, and watch it work; the
models run on forge, the RTX 4090 desktop, behind Nexus. Proton is the
editor and the client. Status: design and screens agreed 2026-10-26; D0 to D5
built: the format, the client, the mode, runs, graphs in PNGs, pictures and masks. Shared as the doc
[Diffuse Design](https://claude.ai/artifact/DuvNm2Xh1dG8bMsRDD7dBa); the
screens are the mockups in
[Diffuse UI](https://claude.ai/artifact/QcvnmxeJPeNrnFNJ8gs9hG).

## Decided

- **The models run on forge**, served by **Nexus's own diffusers
  server**, not ComfyUI and not the MacBook.
- **Nexus's side is built separately** (another session or the owner).
  This document is the contract it builds to.
- **Proton gets its own mode**, the thirteenth: **Diffuse**, internal id
  `diffuse`, on `⌃\`.

## The split

```
Proton (MacBook)                     atlas (coordinator)            forge (RTX 4090)
  Diffuse mode: canvas, queue,  ──►  gateway :7420/img/v1  ──►  image server (diffusers, CUDA)
  gallery; core client               same key as /llm/v1          executes whole graphs,
  (HTTP + one event stream)          streams events through       keeps models and results on GPU
```

- **Proton holds the document.** A workflow is a file in the project.
  Proton draws it, validates it against the server's node catalogue,
  sends it whole, shows progress, and saves what comes back.
- **The server executes the graph.** Models, embeddings and latents stay
  in the 4090's memory between nodes and between runs, so only what
  changed runs again (a new seed reruns the sampler and the decode, not
  the model load or the prompt).
- **The node catalogue comes from the server.** Proton draws whatever
  nodes the server describes, so a node added in Nexus appears in Proton
  without a Proton release. The v1 catalogue below is the contract both
  sides test against.

## Nexus's side (N1)

### Where it runs

- An **image server** on forge: Python, diffusers on CUDA, a Nexus serve
  like `llama-server` is (started and stopped from the Inference tab,
  restarted with the agent). Listens on the LAN, port 7430.
- The **gateway** on atlas forwards `/img/v1/*` to it, with the same
  gateway key as `/llm/v1` and the same LAN and Tailscale addresses. The
  event stream is passed through unbuffered.
- The image server accepts only the gateway: a Windows firewall rule
  admitting atlas, plus a shared secret header the gateway adds
  (`X-Nexus-Gateway: <secret>`), since diffusers has no auth of its own.
- **Models** live on forge in `models/{checkpoints,loras,vae,upscalers}`.
  Nexus's existing `{kind: 'download', node, url}` request fetches them
  there; the Inference tab lists them with sizes.

### Auth

Every request carries `Authorization: Bearer <gateway key>`. A missing or
wrong key is `401 {"error": "key"}`. When the image server on forge is not
running, the gateway itself answers every `/img/v1` request with
`503 {"error": "offline"}` (Nexus says `not_serving`, and `502` when the
server does not answer; Proton takes all three), so Proton can tell "atlas is unreachable",
"the key is wrong" and "forge is not serving" apart. Proton reads the key from the
`NEXUS_KEY` environment variable of the login shell and never stores it.

### Endpoints

All JSON unless said. Errors are `{"error": <code>, "message": <text>}`.

| Method and path | Does | Answers |
| --- | --- | --- |
| `GET /nodes` | The node catalogue | The catalogue (below) |
| `GET /models` | What can be loaded | `{checkpoints, loras, vaes, upscalers}`, each a list of `{name, family, bytes}`; `family` is `sd15`, `sdxl`, `sd3` or `flux` (none for upscalers) |
| `GET /status` | The GPU and the queue | `{gpu: {name, total, used}, loaded: [{name, bytes}], queue: [run ids], running: run id or null}` |
| `POST /uploads` | An input image (raw PNG or JPEG body) | `{id, width, height}`; kept 24 hours |
| `POST /runs` | Queue a graph | `{run, position}`, or `400 {error: "invalid", nodes: {<node id>: message}}` |
| `GET /runs/{run}/events` | The run as it happens (`text/event-stream`) | Events (below); ends after `done`, `error` or `cancelled` |
| `POST /runs/{run}/cancel` | Stop it, queued or running | `{ok: true}` |
| `GET /images/{image}` | An output (`image/png`) | The PNG, with the graph in its `proton-flow` text chunk; kept 24 hours |

### A run

```json
POST /runs
{
  "graph": {
    "nodes": [
      {"id": "1", "type": "LoadCheckpoint", "values": {"checkpoint": "sd_xl_base_1.0.safetensors"}},
      {"id": "2", "type": "TextEncode", "values": {"text": "a lighthouse at dusk, oil painting"}},
      {"id": "3", "type": "TextEncode", "values": {"text": "blurry"}},
      {"id": "4", "type": "EmptyLatent", "values": {"width": 1024, "height": 1024, "batch": 1}},
      {"id": "5", "type": "KSampler", "values": {"seed": 42, "steps": 30, "cfg": 6.5, "sampler": "dpmpp_2m", "scheduler": "karras", "denoise": 1.0}},
      {"id": "6", "type": "VAEDecode", "values": {}},
      {"id": "7", "type": "SaveImage", "values": {"prefix": "lighthouse"}}
    ],
    "links": [
      {"from": "1:clip", "to": "2:clip"}, {"from": "1:clip", "to": "3:clip"},
      {"from": "1:model", "to": "5:model"}, {"from": "2:conditioning", "to": "5:positive"},
      {"from": "3:conditioning", "to": "5:negative"}, {"from": "4:latent", "to": "5:latent"},
      {"from": "5:latent", "to": "6:latent"}, {"from": "1:vae", "to": "6:vae"},
      {"from": "6:image", "to": "7:images"}
    ]
  }
}
```

- A port is `node:name`, as in Model mode's shader graphs. One link into
  any input; no cycles. The server checks both, and every link's types
  against the catalogue, before queueing.
- The server computes what the output nodes (`SaveImage`,
  `PreviewImage`) need and nothing else.
- **Caching:** each node's result is keyed by its type, its values and
  its inputs' keys. A result still in memory is reused across runs and
  reported as `cached`. Models are kept in GPU memory, least recently
  used out first.
- **One run at a time**, first in first out.
- **Seeds:** the same graph and seed on the same server gives the same
  image (a CUDA generator seeded per sampler).

### Events

Server-sent events, one JSON object per `data:` line, its kind in `event:`.

| Event | Data |
| --- | --- |
| `queued` | `{position}`, again whenever it moves |
| `started` | `{}` |
| `node` | `{id, state: "running" \| "cached" \| "done"}` |
| `progress` | `{id, step, steps}` |
| `preview` | `{id, step, jpeg}`: base64 JPEG at most 512 px on its long side, every few steps |
| `image` | `{id, image, index, width, height}`: an output, fetched from `/images/{image}` |
| `error` | `{id?, error, message}`; `error` is `invalid`, `missing_model`, `oom` or `internal` |
| `done` | `{seconds}` |
| `cancelled` | `{}` |

### The v1 catalogue

```json
{
  "version": 1,
  "types": ["MODEL", "CLIP", "VAE", "CONDITIONING", "LATENT", "IMAGE", "MASK", "UPSCALER"],
  "nodes": [
    {"type": "LoadCheckpoint", "title": "Load Checkpoint", "group": "Load",
     "inputs": [{"name": "checkpoint", "kind": "choice", "from": "checkpoints"}],
     "outputs": [{"name": "model", "type": "MODEL"}, {"name": "clip", "type": "CLIP"}, {"name": "vae", "type": "VAE"}]},
    {"type": "LoadLoRA", "title": "Load LoRA", "group": "Load",
     "inputs": [{"name": "model", "type": "MODEL"}, {"name": "clip", "type": "CLIP"},
                {"name": "lora", "kind": "choice", "from": "loras"},
                {"name": "strength", "kind": "float", "min": -2, "max": 2, "step": 0.05, "default": 1}],
     "outputs": [{"name": "model", "type": "MODEL"}, {"name": "clip", "type": "CLIP"}]},
    {"type": "LoadVAE", "title": "Load VAE", "group": "Load",
     "inputs": [{"name": "vae", "kind": "choice", "from": "vaes"}],
     "outputs": [{"name": "vae", "type": "VAE"}]},
    {"type": "LoadImage", "title": "Load Image", "group": "Load",
     "inputs": [{"name": "image", "kind": "upload"}],
     "outputs": [{"name": "image", "type": "IMAGE"}, {"name": "mask", "type": "MASK"}]},
    {"type": "TextEncode", "title": "Text Encode", "group": "Prompt",
     "inputs": [{"name": "clip", "type": "CLIP"}, {"name": "text", "kind": "text", "multiline": true, "default": ""}],
     "outputs": [{"name": "conditioning", "type": "CONDITIONING"}]},
    {"type": "EmptyLatent", "title": "Empty Latent", "group": "Latent",
     "inputs": [{"name": "width", "kind": "int", "min": 64, "max": 4096, "step": 8, "default": 1024},
                {"name": "height", "kind": "int", "min": 64, "max": 4096, "step": 8, "default": 1024},
                {"name": "batch", "kind": "int", "min": 1, "max": 8, "step": 1, "default": 1}],
     "outputs": [{"name": "latent", "type": "LATENT"}]},
    {"type": "KSampler", "title": "KSampler", "group": "Sample",
     "inputs": [{"name": "model", "type": "MODEL"}, {"name": "positive", "type": "CONDITIONING"},
                {"name": "negative", "type": "CONDITIONING", "optional": true}, {"name": "latent", "type": "LATENT"},
                {"name": "seed", "kind": "seed"},
                {"name": "steps", "kind": "int", "min": 1, "max": 150, "step": 1, "default": 30},
                {"name": "cfg", "kind": "float", "min": 0, "max": 30, "step": 0.1, "default": 6.5},
                {"name": "sampler", "kind": "choice", "from": "samplers"},
                {"name": "scheduler", "kind": "choice", "from": "schedulers"},
                {"name": "denoise", "kind": "float", "min": 0, "max": 1, "step": 0.01, "default": 1}],
     "outputs": [{"name": "latent", "type": "LATENT"}]},
    {"type": "VAEDecode", "title": "VAE Decode", "group": "Latent",
     "inputs": [{"name": "vae", "type": "VAE"}, {"name": "latent", "type": "LATENT"}],
     "outputs": [{"name": "image", "type": "IMAGE"}]},
    {"type": "VAEEncode", "title": "VAE Encode", "group": "Latent",
     "inputs": [{"name": "vae", "type": "VAE"}, {"name": "image", "type": "IMAGE"}],
     "outputs": [{"name": "latent", "type": "LATENT"}]},
    {"type": "SetLatentMask", "title": "Set Latent Mask", "group": "Latent",
     "inputs": [{"name": "latent", "type": "LATENT"}, {"name": "mask", "type": "MASK"}],
     "outputs": [{"name": "latent", "type": "LATENT"}]},
    {"type": "UpscaleLatent", "title": "Upscale Latent", "group": "Latent",
     "inputs": [{"name": "latent", "type": "LATENT"},
                {"name": "scale", "kind": "float", "min": 1, "max": 4, "step": 0.05, "default": 1.5}],
     "outputs": [{"name": "latent", "type": "LATENT"}]},
    {"type": "LoadUpscaler", "title": "Load Upscaler", "group": "Load",
     "inputs": [{"name": "upscaler", "kind": "choice", "from": "upscalers"}],
     "outputs": [{"name": "upscaler", "type": "UPSCALER"}]},
    {"type": "UpscaleImage", "title": "Upscale Image", "group": "Image",
     "inputs": [{"name": "image", "type": "IMAGE"}, {"name": "upscaler", "type": "UPSCALER", "optional": true},
                {"name": "scale", "kind": "float", "min": 1, "max": 4, "step": 0.05, "default": 2}],
     "outputs": [{"name": "image", "type": "IMAGE"}]},
    {"type": "InvertMask", "title": "Invert Mask", "group": "Image",
     "inputs": [{"name": "mask", "type": "MASK"}],
     "outputs": [{"name": "mask", "type": "MASK"}]},
    {"type": "PreviewImage", "title": "Preview Image", "group": "Output",
     "inputs": [{"name": "images", "type": "IMAGE"}], "outputs": []},
    {"type": "SaveImage", "title": "Save Image", "group": "Output",
     "inputs": [{"name": "images", "type": "IMAGE"}, {"name": "prefix", "kind": "text", "default": "image"}],
     "outputs": []}
  ],
  "lists": {
    "samplers": ["euler", "euler_a", "heun", "dpmpp_2m", "dpmpp_2m_sde", "dpmpp_sde", "ddim", "unipc", "lcm"],
    "schedulers": ["normal", "karras", "exponential", "sgm_uniform", "simple"]
  }
}
```

- **Input kinds:** an input with a `type` takes a link. The others are
  set on the node: `int`, `float`, `text`, `choice` (from `lists` or from
  `/models`), `seed` (an integer Proton fills from the node's own fixed,
  increment or random setting before each run) and `upload` (an id from
  `/uploads`).
- **Families:** the server handles each model family inside the nodes.
  `TextEncode` is SDXL's two encoders and pooled embedding, or FLUX's T5
  and CLIP. `EmptyLatent` takes its channel count from the model the
  sampler is given. `cfg` is FLUX's guidance. `VAEDecode` uses the
  family's scaling. A link between families (an SDXL LoRA on a FLUX
  model) is `invalid`.
- **How the nodes map onto diffusers:** `LoadCheckpoint` is the family's
  pipeline `from_single_file`. `TextEncode` is `pipe.encode_prompt`.
  `KSampler` calls the pipeline with `prompt_embeds`, `latents`,
  `output_type="latent"` and the scheduler named, and image-to-image's
  `strength` for `denoise` below 1. `VAEDecode` and `VAEEncode` are the
  VAE.

### Proving N1

- A unit test per node on a tiny model.
- `GET /nodes` matches the catalogue above exactly. Proton keeps the same
  JSON as its fixture, so a change on one side breaks the other's test.
- A curl round trip through the gateway: queue the SDXL graph above, read
  its events, fetch the image, then run it again with only the seed
  changed and see the loader and encoders `cached`.
- A request without the key gets `401`, and forge refuses a direct
  connection from anywhere but atlas.

## Proton's side

### The mode

The screens are the [Diffuse UI](https://claude.ai/artifact/QcvnmxeJPeNrnFNJ8gs9hG)
mockups. Laid out as Game mode is: lists on the left, the stage in the
middle, tabs on the right.

- **Left:** tabs Flows (the project's `.flow.json` files, new ones from a
  starter graph, the last outputs), Nodes (the catalogue by group, dragged
  onto the graph) and Models (what `/models` lists). Its foot shows
  whether forge is serving.
- **The stage** switches between **Graph** and **Gallery**.
  - **Graph:** nodes as cards with typed, coloured sockets and wires
    between them, as Model mode's shader graph draws them. Double-click
    empty space to search the catalogue; letting go of a wire on empty
    space opens the same search, listing only nodes that take the wire's
    type. A node's fields are built from its inputs: number fields with
    their ranges, dropdowns filled from `/models` and `lists`, prompt
    boxes, an image for Load Image, a seed that stays fixed, goes up by
    one or is random after each run.
  - **Gallery:** the project's outputs, newest first, filtered by flow.
    The chosen one shows its seed, settings and prompt, and offers Open
    its graph, Send to Canvas, Send to Game, Use as input and Show in
    Finder.
- **The header:** the open flows as tabs, the Graph and Gallery switch,
  forge's chip (GPU and memory in use), undo, redo, Save and **Run**
  (`⌘↩`). While a run goes, Run queues another and Stop (`⌘.`) cancels.
- **Right:** Inspector, Queue and Agent.
  - **Inspector:** the selected node's fields and what feeds its inputs,
    with **Bypass** (Proton passes the node's first input of each
    output's type straight through when it sends the graph) and **Run to
    here** (Proton sends only what the node needs, adding a decode and a
    Preview Image after it when it makes a latent). With nothing
    selected, the flow's own settings.
  - **Queue:** the running run with each node's state (cached, done with
    its time, running, waiting), the queued runs, and today's results,
    failures included.
- **A run:** the running node is outlined, the sampler shows its step,
  the time left and the latest preview, and finished outputs appear on
  their Save and Preview nodes. Edits made during a run apply to the next.
- **Masks:** Load Image's Paint the mask opens a sheet over the graph:
  brush, eraser, fill, invert, clear, brush size (`[` and `]`), soft edge,
  and the share of the image masked.
- **forge not serving:** a banner names the cause and what to do ("start
  it in Nexus › Inference"), Run is disabled, and a Server tab shows what
  was checked (the gateway, the key, forge's server) and where Proton
  looks. Proton checks again every 30 s while the mode is open and turns
  Run back on by itself. Editing and saving still work.

### Files

Diffuse keeps its flows and images in **a folder of its own** —
`~/Diffuse`, or the person's `diffuse.folder` — whichever project is open
in Codex, as Notebook keeps its notebooks; the paths below are in it. The Flows list
says which folder and moves it. Deleting a flow or an image moves it to the
Trash; a flow's images stay when it goes.


- **`Name.flow.json`** in the project: flat `nodes` and `links`, in the
  same shape as the run's `graph`, plus each node's position and the
  seed setting. Written in a fixed key order so diffs show only what
  changed, like Model's scenes.
- **Outputs** go to `outputs/<flow name>/<prefix>_<nnnn>.png`, with the
  graph in the PNG's `proton-flow` chunk. Dropping such a PNG on the
  graph opens its graph.
- **Masks** are kept beside the flow as `<flow name>.<node id>.mask.png`,
  one per Load Image node that has one.
- **The catalogue and model lists** forge served last are kept in the
  folder's `.proton/diffuse/`, so graphs open and edit while it is away.
- **Edits** are held until `⌘S`, with undo, as Game and Canvas do.

### Inside Proton

- **`proton-diffuse`** (D0, built): the catalogue and model lists as the
  server describes them, the `.flow.json` format and its writer, typed ops
  (add, remove, place, set, connect, disconnect, seed, bypass, duplicate,
  note, outputs) applied whole or not at all, undo, saving, the checks, and
  the graph a run sends (bypassed nodes passed through, defaults filled,
  only what the outputs need). Its tests hold the v1 catalogue and the run
  above, read from this document.
- **`proton-diffuse`'s client** (D1, built): every request of the contract
  on `ureq`, sent to the address that answered last and to the next only
  when one cannot be reached; the three ways of not being served told
  apart (unreachable, `401`, `503 offline`); the event stream read a line
  at a time as it arrives, ending after `done`, `error` or `cancelled`;
  ids checked before they go into a path; and what forge served last kept
  in the project's `.proton/diffuse/`. Proved against a fake gateway on a
  loopback port playing back a recorded run.
- **The mode** (D2, built): `ui/diffuse.js` draws the graph from the core's
  answer to each edit (cards of `ui/diffuse-graph.js`'s measures, so a wire
  is drawn from the flow alone), sends a gesture as one batch of ops — a
  node added for a wire let go on space names it by the id the core will
  give it — and asks the core to look at forge on coming on screen and
  every 30 s. Load Image's picture waits for D5.
- **Graphs in PNGs and starters** (D4, built): an output's graph — the one
  forge writes into each PNG as `proton-flow` — opens as a new flow from the
  Gallery's Open its graph, or from a PNG dropped on the graph (the window
  reads the chunk itself), laid out in columns by depth since a run's graph
  carries no places (`proton-diffuse`'s `layout`), named after the image
  and held until ⌘S. New flows start from Text to image, Image to image,
  Inpaint, Hires fix or FLUX; the image inputs wait for D5's picture.
  ComfyUI's own PNGs (their `workflow` chunk) are not read.
- **Pictures and masks** (D5, built): a Load Image takes a picture chosen,
  dropped or taken from the gallery, kept as a PNG in the folder's `inputs/`
  (through the shell's `put_diffuse_file`, raw bytes); its mask is painted in
  a sheet over the graph and kept beside the flow as `<flow>.<node>.mask.png`.
  A run uploads each picture first, its mask in its alpha (transparent where
  masked, as ComfyUI's Load Image reads one), and names the upload in the
  graph. An output goes to Canvas (a canvas made in Canvas's own project) or
  to Game (into the open game's `art/` and its room).
- **Failures** (2026-10-26, from a run that looked stuck): a graph forge
  refuses (`400 invalid`) fails at once with its `message`, each node in
  `nodes` marked on the graph with its words; an `error` event marks the run
  failed and its node (or the run, with none) with the message; a node still
  running when its run ended shows stopped. A stream that closes before
  `done`, `error` or `cancelled` is opened again — it replays from the start,
  so what was heard is passed over and no image saved twice — three more
  times, a second longer each, and then the run is lost; a `404` is a run
  forge no longer has. A worker that stops unexpectedly fails its run.
  Whole-number fields on a step (a latent's 8) are put on it as they are set
  and refused off it in Nexus's words; Empty Latent offers SDXL's sizes near
  a megapixel.
- **Runs** (D3, built): Run (`⌘↩`) sends the flow shown, as often as it
  is pressed; each run has a worker in the core that sends the graph,
  follows its event stream, fetches each image and writes it — a Save
  Image's as `<prefix>_<nnnn>.png` in the flow's outputs folder, numbered
  after those there; a Preview Image's under `.proton/diffuse/previews/` —
  and the core polls their news in. The graph outlines the node at work
  with its step and preview, says of the rest cached, done in so long, or
  waiting, and shows each output on its node; the Queue tab follows every
  run this session; Stop (`⌘.`) cancels one. A seed set to move after a
  run moves when the run is sent, as an edit. The Gallery reads each
  output PNG's head — its size and the graph in it, never its pixels — to
  say what made it; the window gets an image's bytes through the shell's
  `read_diffuse`, only for an image the gallery or a run listed. The
  Server tab sets the gateway's addresses in the person's own settings. `ui/preview-diffuse.js` stands in for the core in the browser
  preview and the browser suite.
- **Core** (`proton-core/src/diffuse/`): the open flows as tabs, held until
  ⌘S and named in the quit prompt; the gateway's addresses from the
  person's own settings (`diffuse.gateway`, default
  `http://atlas:7420/img/v1`, and `diffuse.gatewayAway`), the key from
  `NEXUS_KEY` (Proton's environment, then the login shell's, kept in memory
  only); looks at forge on a worker; then, from D3, the worker that runs
  flows. The client uses the
  HTTP library Proton already has (`ureq`) and reads the event stream line
  by line on a worker thread. It downloads outputs into the project and
  sends progress, previews and results to the window.
- **Window** (`ui/diffuse*.js`): the canvas, fields, queue and gallery.
  Previews arrive as data URLs, which the window's security policy allows.
- **Settings:** the gateway URL at home and away (Tailscale), the person's
  own (`diffuse.gateway`, `diffuse.gatewayAway`); the second is tried when
  the first doesn't answer. The key comes only from
  `NEXUS_KEY`, read at launch, never stored or shown.
- **Elsewhere:** an output can be sent to Canvas as a new layer or to an
  open game as art. The agent gets `flow_info`, `flow_edit` and `flow_run`
  and an `@flow` mention, so it can build a graph from a description and
  run it.
- **The agent** (D6, built; `proton-core/src/diffuse/agent.rs`): the agent
  lives in a workspace and Diffuse in the session, so a workspace defers
  `flow_*` calls and the session answers them after its poll, as it does
  the PDF tools. `flow_info` says whether forge is serving, the flows and
  those open, the flow on screen (or one named) node by node — id, type,
  place, values, the links into its inputs — and what stops it running;
  then forge's nodes with their sockets, its models, the starters and the
  latest runs, and with `nodes` those types' fields in full (kinds, ranges,
  steps, defaults, choices). `flow_edit` makes a flow (`new`, from a
  starter or empty), opens one (`flow`), and applies `ops` — the window's
  own — as one step of the person's undo, all or none, held until they
  save; an `add` may carry a `ref` that later ops in the call use in place
  of the id it cannot know yet, and one with no `at` is placed: a flow made
  whole in the call is laid out as a PNG's graph is, and a node added to one
  goes in the column its links give it, below what is there. `save` saves,
  only when the person asked. `flow_run` runs the flow as ⌘↩ does and holds
  the turn until the run ends — up to fifteen minutes — answering with the
  images saved or why it failed and at which node; `wait: false` answers at
  once, `cancel` stops a run. `@flow` sends what `flow_info` says of the flow
  on screen; a chip beside Diffuse's agent writes it.
- **Sealed on disk** (2026-10-26): in the app, Diffuse's flows and pictures
  — outputs, inputs, masks and previews — are sealed with AES-256-GCM
  (`proton-diffuse::seal`): each file is its plain name with `.sealed`
  after it, a header, a nonce made afresh for each write, and the bytes, so
  Finder, Quick Look, Spotlight and a backup see noise. What forge served
  last (the catalogue and models) stays plain: it is forge's. The key is
  32 bytes in the person's login keychain (service "Proton Diffuse"), made
  there the first time; the shell (`proton-shell/src/vault.rs`) asks for
  Touch ID or the Mac's password through LocalAuthentication before it reads
  it — the app is ad-hoc signed, so the data-protection keychain, which can
  ask by itself, is closed to it, and a new build must be allowed once at
  the keychain's prompt. Diffuse is locked until then: nothing of it is
  shown, its commands and the agent's flow tools refused. The Gallery has a
  lock of its own, asked for each time it is shown and shut when it is
  left; older outputs reach the window only while it is open, this
  session's runs' images, pictures and masks once Diffuse is. On the first
  unlock everything still plain is sealed, read back, and its plain file
  moved to the Trash. Lock (beside Change) locks Diffuse again; quitting
  does too. Sent to Canvas or Game, a picture is written plain, as those
  keep theirs. forge's own copies — kept 24 hours by Nexus — are not
  Proton's to seal.
- **The inspector** (2026-10-26): a field being typed in keeps what was
  typed, and its caret, when a look at forge draws the inspector again,
  and is set when it is left; a prompt's box starts fourteen lines tall and
  grows with it to most of the window, set out as code once it is JSON
  (Ideogram's prompts are). A press on a picture in the Gallery chooses it:
  the stage behind no longer takes it for a pan.

### Milestones

| | Builds | Proved by |
| --- | --- | --- |
| **D0** | The format, ops and undo; validation against the catalogue | Unit tests with the catalogue fixture |
| **D1** | The client: settings, the key from the shell, requests and the event stream | A fake server in the tests playing back recorded events |
| **D2** | The mode, the canvas, fields from the catalogue, search | The browser suite |
| **D3** | Run: queue, live node states, progress, previews, outputs saved, gallery | The fake server, then one real run on forge once N1 is up |
| **D4** | Graphs inside PNGs, drag a PNG in, starter graphs (text to image, image to image, inpainting, hires fix, FLUX) | Round-trip tests |
| **D5** | Image inputs and painted masks; send to Canvas and Game | The browser suite |
| **D6** | Agent tools and `@flow` | Tool tests against the fake server |

D0 to D2 need nothing from Nexus. D3's real run waits on N1.

## Open

- **Memory on forge:** FLUX dev at full precision is about 24 GB with its
  encoders, more than the 4090 holds alongside anything else; the server
  should load it in fp8 or offload the T5 encoder. Nexus's call.
