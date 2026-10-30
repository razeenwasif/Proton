/* Logic for the browser preview: one worksheet in a pretend folder, and
 * the core's answers for its lines, dumped from crates/proton-core by
 * `cargo test -p proton-core --lib dump_answers_for_the_preview -- --ignored
 * --nocapture`. The engine is the core's; this answers only the sample's
 * lines as they were, and must never grow into a second engine. */

/** The sample worksheet, as a new one starts. */
const WORKSHEET = "# A worksheet: a line beginning ? is asked. Put the cursor on one.\n? p -> q\n? p, p -> q |= q\n? p -> q == not q -> not p\n? (p and q) or (not p and r)\n";

/** The core's answer for each of its lines that has one. @type {Record<string, any>} */
const ANSWERS = {"2":{"atoms":["p","q"],"column":"1101","columns":[{"name":"p -> q","values":"1101"}],"countermodel":null,"differ":null,"exact":true,"falsifier":[true,false],"forms":[{"name":"Least sum of products","text":"not p or q"},{"name":"Least product of sums","text":"not p or q"},{"name":"Conjunctive normal form","text":"not p or q"},{"name":"Disjunctive normal form","text":"not p or q"},{"name":"Negation normal form","text":"not p or q"},{"name":"Full DNF","text":"not p and not q or not p and q or p and q"},{"name":"Full CNF","text":"not p or q"},{"name":"NAND only","text":"nand(p, nand(q, q))"},{"name":"NOR only","text":"nor(nor(nor(p, p), q))"}],"generation":0,"kind":"formula","line":2,"message":null,"model":[false,false],"models":3,"more":false,"posTerms":[[2,3]],"same":null,"sopTerms":[[0,2],[1,1]],"status":"contingent","text":"? p -> q","valid":null},"3":{"atoms":["p","q"],"column":null,"columns":[{"name":"p","values":"0011"},{"name":"p -> q","values":"1101"},{"name":"q","values":"0101"}],"countermodel":null,"differ":null,"exact":false,"falsifier":null,"forms":[],"generation":0,"kind":"argument","line":3,"message":null,"model":null,"models":null,"more":false,"posTerms":[],"same":null,"sopTerms":[],"status":null,"text":"? p, p -> q |= q","valid":true},"4":{"atoms":["p","q"],"column":null,"columns":[{"name":"p -> q","values":"1101"},{"name":"not q -> not p","values":"1101"}],"countermodel":null,"differ":null,"exact":false,"falsifier":null,"forms":[],"generation":0,"kind":"same","line":4,"message":null,"model":null,"models":null,"more":false,"posTerms":[],"same":true,"sopTerms":[],"status":null,"text":"? p -> q == not q -> not p","valid":null},"5":{"atoms":["p","q","r"],"column":"01010011","columns":[{"name":"p and q","values":"00000011"},{"name":"not p","values":"11110000"},{"name":"not p and r","values":"01010000"},{"name":"p and q or not p and r","values":"01010011"}],"countermodel":null,"differ":null,"exact":true,"falsifier":[false,false,false],"forms":[{"name":"Least sum of products","text":"not p and r or p and q"},{"name":"Least product of sums","text":"(p or r) and (not p or q)"},{"name":"Conjunctive normal form","text":"(p or r) and (not p or q) and (q or r)"},{"name":"Disjunctive normal form","text":"p and q or not p and r"},{"name":"Negation normal form","text":"p and q or not p and r"},{"name":"Full DNF","text":"not p and not q and r or not p and q and r or p and q and not r or p and q and r"},{"name":"Full CNF","text":"(p or q or r) and (p or not q or r) and (not p or q or r) and (not p or q or not r)"},{"name":"NAND only","text":"nand(nand(nand(p, p), r), nand(p, q))"},{"name":"NOR only","text":"nor(nor(p, r), nor(nor(p, p), q))"}],"generation":0,"kind":"formula","line":5,"message":null,"model":[false,false,true],"models":4,"more":false,"posTerms":[[0,5],[4,6]],"same":null,"sopTerms":[[1,5],[6,6]],"status":"contingent","text":"? p and q or not p and r","valid":null}};

/** The sample theory, as a new one starts, and its models. */
const THEORY = "# A theory: sorts, then what is named, then what is true of them.\nperson = Ann | Bo | Cy\nage = 1..3\nof: person -> age  one-to-one\n\nof(Ann) > of(Bo)\nof(Cy) != 3\n";
const MODELS = {"generation":0,"running":false,"found":2,"finished":true,"timedOut":false,"counted":true,"elapsedMs":1,"choices":6,"models":[{"sizes":[],"tables":[{"name":"of","shape":"row","rows":[],"cols":["Ann","Bo","Cy"],"cells":[["3","2","1"]]}]},{"sizes":[],"tables":[{"name":"of","shape":"row","rows":[],"cols":["Ann","Bo","Cy"],"cells":[["3","1","2"]]}]}],"error":null};

/** The column 00010111 over a b c, read back. */
const TABLE = {"generation":0,"line":0,"kind":"table","text":"00010111","message":null,"atoms":["a","b","c"],"columns":[],"status":null,"models":null,"model":null,"falsifier":null,"valid":null,"countermodel":null,"same":null,"differ":null,"forms":[{"name":"Least sum of products","text":"b and c or a and c or a and b"},{"name":"Least product of sums","text":"(b or c) and (a or c) and (a or b)"},{"name":"Full DNF","text":"not a and b and c or a and not b and c or a and b and not c or a and b and c"},{"name":"Full CNF","text":"(a or b or c) and (a or b or not c) and (a or not b or c) and (not a or b or c)"},{"name":"NAND only","text":"nand(nand(b, c), nand(a, c), nand(a, b))"},{"name":"NOR only","text":"nor(nor(b, c), nor(a, c), nor(a, b))"}],"exact":true,"more":false,"column":"00010111","sopTerms":[[3,3],[5,5],[6,6]],"posTerms":[[0,3],[0,5],[0,6]]};

/** @param {(events: any[]) => void} _deliver */
export function createLogicPreview(_deliver) {
  const files = [{ path: "week1/theory.logic", name: "theory", kind: "logic" }, { path: "week1/worksheet.logic", name: "worksheet", kind: "logic" },
    { path: "puzzles/pets.l4f", name: "pets", kind: "l4f" }];
  /** @type {null | {path: string, name: string, kind: string, text: string, saved: string, generation: number}} */
  let open = null;
  let generations = 0;
  const home = () => ({ type: "logicHome", files });
  const fileEvent = () => ({ type: "logicFile", file: open ? { path: open.path, name: open.name, kind: open.kind, text: open.text, dirty: open.text !== open.saved,
    generation: open.generation, diagnostics: [], notes: [] } : null });
  const notice = (/** @type {string} */ message) => ({ type: "notice", message, level: "warn" });
  const show = (/** @type {string} */ path, /** @type {string} */ text) => {
    const f = files.find((x) => x.path === path);
    open = { path, name: f?.name ?? path, kind: f?.kind ?? "logic", text, saved: text, generation: ++generations };
  };
  return {
    sync: () => [home(), fileEvent()],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "logicOpen":
          if (open?.path === command.path) return [home(), fileEvent()];
          if (open && open.text !== open.saved) return [notice("save or discard the open file's changes before opening another")];
          if (command.path === "puzzles/pets.l4f") {
            show(command.path, "Sorts:\npet enum: cat, dog.\nVocabulary:\nfunction f(pet) : pet {all_different}\nConstraints:\nf(cat) = dog.\n");
          } else if (command.path === "week1/worksheet.logic") {
            show(command.path, WORKSHEET);
          } else if (command.path === "week1/theory.logic") {
            show(command.path, THEORY);
          } else {
            return [notice(`${command.path} is not here`)];
          }
          return [fileEvent(), { type: "logicAnswer", answer: null }];
        case "logicNew": {
          const slug = String(command.name).toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "") || "logic";
          const path = `${slug}.logic`;
          if (files.some((f) => f.path === path)) return [notice(`${path} is already there`)];
          files.push({ path, name: slug, kind: "logic" });
          files.sort((a, b) => a.path.localeCompare(b.path));
          show(path, WORKSHEET);
          return [home(), fileEvent(), { type: "logicAnswer", answer: null }];
        }
        case "logicClose":
          open = null;
          return [home(), fileEvent(), { type: "logicAnswer", answer: null }];
        case "logicSave":
          if (open) open.saved = open.text;
          return [home(), fileEvent()];
        case "logicEdit":
          if (!open || open.generation !== command.generation || open.text === command.text) return [];
          open.text = command.text;
          return [fileEvent()];
        case "logicAsk": {
          if (!open || open.generation !== command.generation) return [];
          // The sample's own lines, unchanged, as the core answered them.
          const pristine = open.text === WORKSHEET && open.kind === "logic";
          const line = open.text.split("\n")[command.line - 1] ?? "";
          const canned = pristine ? ANSWERS[String(command.line)] : null;
          const answer = canned ? { ...canned, generation: open.generation }
            : line.trim() === "" || line.trim().startsWith("#") ? { ...ANSWERS["2"], kind: "nothing", generation: open.generation, line: command.line }
            : { ...ANSWERS["2"], kind: "error", generation: open.generation, line: command.line, message: "The preview answers the sample worksheet's own lines; the app answers any." };
          return [{ type: "logicAnswer", answer }];
        }
        case "logicSolve": {
          if (!open || open.generation !== command.generation) return [];
          const models = open.text === THEORY ? { ...MODELS, generation: open.generation, counted: command.count }
            : { ...MODELS, generation: open.generation, found: 0, models: [], error: "The preview finds the sample theory's models; the app finds any theory's." };
          return [{ type: "logicModels", models }];
        }
        case "logicStop":
          return [];
        case "logicFromTable":
          return [{ type: "logicAnswer", answer: command.column.replace(/\s/g, "") === "00010111" ? { ...TABLE, generation: open?.generation ?? 0 }
            : { ...TABLE, kind: "error", message: "The preview reads back only 00010111; the app reads any column." } }];
        default:
          return [];
      }
    },
  };
}
