/* Game mode's stand-in for the browser preview: the UI tests' fixture game
 * (tests/ui/fixtures/game — the template, as proton-game writes it), read
 * once, and edits applied to it here as the core applies them: by ops
 * addressed by id, whole or not at all, held until "saved". Scripted; the
 * real game is proton-core's `game`. */

const ROOT = "/tests/ui/fixtures/game/";

/** Where a list member is: `#id` by id, a number by place. @param {any[]} list @param {string} step */
function member(list, step) {
  const at = step.startsWith("#") ? list.findIndex((m) => m?.id === step.slice(1)) : Number(step);
  if (!Number.isInteger(at) || at < 0 || at >= list.length) throw new Error(`there is no ${step}`);
  return at;
}

/** What `path` leads to. @param {any} tree @param {string[]} path */
function at(tree, path) {
  let here = tree;
  for (const step of path) {
    if (Array.isArray(here)) here = here[member(here, step)];
    else if (here && typeof here === "object" && step in here) here = here[step];
    else throw new Error(`there is no ${path.join("/")}`);
  }
  return here;
}

/** Apply one op to a tree, as proton-game does. @param {any} tree @param {any} op */
function apply(tree, op) {
  const path = /** @type {string[]} */ (op.path);
  const last = path[path.length - 1];
  const parent = at(tree, path.slice(0, -1));
  if (op.op === "set") {
    if (Array.isArray(parent)) parent[member(parent, last)] = op.value;
    else parent[last] = op.value;
  } else if (op.op === "insert") {
    const list = at(tree, path);
    if (op.value?.id && list.some((/** @type {any} */ m) => m?.id === op.value.id)) throw new Error(`${path.join("/")} already has a ${op.value.id}`);
    list.splice(Math.min(op.at ?? list.length, list.length), 0, op.value);
  } else if (op.op === "remove") {
    if (Array.isArray(parent)) parent.splice(member(parent, last), 1);
    else delete parent[last];
  } else if (op.op === "move") {
    const moving = parent.splice(member(parent, last), 1)[0];
    parent.splice(Math.min(op.to, parent.length), 0, moving);
  }
}

/** A room as the core sends it: its files leave out what is empty, and the
 *  core's types fill it back in. @param {any} r */
export function fullRoom(r) {
  return {
    format: 1, id: "", size: [1920, 1080], ...r,
    camera: { bounds: null, zones: [], ...r.camera },
    planes: (r.planes ?? []).map((/** @type {any} */ p) => ({ id: "", name: "", depth: 0, parallax: null, tint: null, fog: 0, lit: false, blend: "normal", gameplay: false, ...p,
      items: (p.items ?? []).map((/** @type {any} */ i) => ({ id: "", image: "", at: [0, 0], scale: 1, rot: 0, flip: false, opacity: 1, blend: "normal", ...i })) })),
    collision: (r.collision ?? []).map((/** @type {any} */ c) => ({ kind: "solid", closed: false, points: [], ...c })),
    entities: (r.entities ?? []).map((/** @type {any} */ e) => ({ at: null, rect: null, to: null, requires: [], grants: null, props: {}, ...e })),
    lights: (r.lights ?? []).map((/** @type {any} */ l) => ({ radius: 400, color: "#ffd9a0", intensity: 1, flicker: 0, ...l })),
    spawns: r.spawns ?? [],
    ambience: { ambient: null, particles: [], ...r.ambience },
  };
}

/** The fixture game, as the core would send it. */
export async function readFixture() {
  const read = async (/** @type {string} */ rel) => (await fetch(ROOT + rel)).json();
  const game = await read("game.json");
  const world = await read("world.json");
  const rooms = (await Promise.all(world.rooms.map((/** @type {any} */ r) => read(r.file)))).map(fullRoom);
  const prefabs = await Promise.all(["prefabs/crawler.prefab.json", "prefabs/player.prefab.json"].map(read));
  return { game: { abilities: [], input: {}, ...game }, world: { areas: [], rooms: [], ...world }, rooms, prefabs };
}

/** @param {(event: any) => void} deliver */
export function createGamePreview(deliver) {
  /** @type {{game: any, world: any, rooms: any[], prefabs: any[]} | null} */
  let files = null;
  /** The documents as "saved", to tell what changed. */
  /** @type {Map<string, string>} */
  const saved = new Map();
  let open = false;
  /** @type {string[]} */
  let tabs = [];
  /** @type {string | null} */
  let focus = null;
  /** @type {{target: string, was: string}[]} */
  const done = [];
  /** @type {{target: string, was: string}[]} */
  const undone = [];
  let revision = 1;

  const doc = (/** @type {string} */ target) => {
    if (!files) return null;
    if (target === "game") return files.game;
    if (target === "world") return files.world;
    const [kind, id] = target.split(":");
    return (kind === "room" ? files.rooms : files.prefabs).find((d) => d.id === id) ?? null;
  };
  const replace = (/** @type {string} */ target, /** @type {any} */ value) => {
    if (!files) return;
    if (target === "game") files.game = value;
    else if (target === "world") files.world = value;
    else {
      const [kind, id] = target.split(":");
      const list = kind === "room" ? files.rooms : files.prefabs;
      list[list.findIndex((d) => d.id === id)] = value;
    }
  };
  const dirty = (/** @type {string} */ target) => saved.get(target) !== JSON.stringify(doc(target));
  const targets = () => (files ? ["game", "world", ...files.rooms.map((r) => `room:${r.id}`), ...files.prefabs.map((p) => `prefab:${p.id}`)] : []);

  const homeEvent = () => ({ type: "gameHome", games: files ? [{ path: "crossroads", name: files.game.name, niche: files.game.niche, rooms: files.rooms.length }] : [],
    templates: [{ id: "metroidvania", name: "Metroidvania — side view, rooms joined into one map" }] });
  const projectEvent = () => {
    if (!files || !open) return { type: "gameProject", project: null };
    return {
      type: "gameProject",
      project: {
        path: "crossroads", game: files.game, world: files.world, prefabs: files.prefabs,
        rooms: files.rooms.map((r) => ({ id: r.id, area: files?.world.rooms.find((/** @type {any} */ w) => w.id === r.id)?.area ?? "", size: r.size, dirty: dirty(`room:${r.id}`) })),
        open: tabs, focus, gameDirty: dirty("game"), worldDirty: dirty("world"), dirty: targets().some(dirty),
        canUndo: done.length > 0, canRedo: undone.length > 0, problems: [],
        assets: ["art/crawler.png", "art/fronds.png", "art/ground.png", "art/hills_far.png", "art/hills_near.png", "art/player.png", "art/sky.png"],
      },
    };
  };
  const roomEvent = () => ({ type: "gameRoom", room: open && focus ? doc(`room:${focus}`) : null, revision });
  const all = () => [homeEvent(), projectEvent(), roomEvent()];

  // Read once; the window is told when it has come.
  (async () => {
    try {
      files = await readFixture();
      for (const target of targets()) saved.set(target, JSON.stringify(doc(target)));
      for (const event of all()) deliver(event);
    } catch {
      // No fixture to read: the home page has no games, and that is all.
    }
  })();

  return {
    sync: all,
    /** @param {any} command */
    command(command) {
      if (!files) return [];
      switch (command.type) {
        case "gameOpen":
        case "gameNew":
          open = true;
          tabs = [files.game.start.room];
          focus = tabs[0];
          revision++;
          return all();
        case "gameClose":
          if (!command.discard && targets().some(dirty)) return [{ type: "notice", text: "the game has unsaved changes", level: "warn" }];
          open = false;
          return all();
        case "gameOpenRoom":
        case "gameFocus":
          if (!tabs.includes(command.room)) tabs.push(command.room);
          focus = command.room;
          revision++;
          return [projectEvent(), roomEvent()];
        case "gameCloseRoom": {
          const i = tabs.indexOf(command.room);
          if (i < 0) return [];
          tabs.splice(i, 1);
          if (focus === command.room) focus = tabs[Math.min(i, tabs.length - 1)] ?? null;
          revision++;
          return [projectEvent(), roomEvent()];
        }
        case "gameEdit": {
          const was = JSON.stringify(doc(command.target));
          const next = JSON.parse(was);
          try {
            for (const op of command.ops) apply(next, op);
          } catch (e) {
            return [{ type: "notice", text: String(/** @type {Error} */ (e).message ?? e), level: "warn" }];
          }
          if (JSON.stringify(next) === was) return [];
          replace(command.target, next);
          done.push({ target: command.target, was });
          undone.length = 0;
          revision++;
          return [projectEvent(), roomEvent()];
        }
        case "gameUndo":
        case "gameRedo": {
          const from = command.type === "gameUndo" ? done : undone;
          const to = command.type === "gameUndo" ? undone : done;
          const step = from.pop();
          if (!step) return [];
          to.push({ target: step.target, was: JSON.stringify(doc(step.target)) });
          replace(step.target, JSON.parse(step.was));
          revision++;
          return [projectEvent(), roomEvent()];
        }
        case "gameSave":
          for (const target of targets()) saved.set(target, JSON.stringify(doc(target)));
          return [projectEvent(), roomEvent(), { type: "notice", text: "saved — in the preview, only here", level: "info" }];
        default:
          return [];
      }
    },
  };
}
