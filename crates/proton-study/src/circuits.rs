//! Circuit practice: digital logic problems to build in Circuit mode, each
//! with its inputs and outputs, a test table it must pass, and a reference
//! answer — expressions for one without state, a state machine for one with
//! a clock — that Proton builds and runs against the table before the
//! student sees the problem, so no problem's table is wrong.

use proton_circuit::circ::Project;
use proton_circuit::{analysis, fsm, testlang};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/// A signal of a problem: its name and width.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCircuitSignal {
    pub name: String,
    pub bits: u8,
}

/// A problem to build in Circuit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCircuitProblem {
    /// `c1`.
    pub id: String,
    pub title: String,
    /// What to build, in Markdown.
    pub statement: String,
    pub inputs: Vec<StudyCircuitSignal>,
    pub outputs: Vec<StudyCircuitSignal>,
    /// Whether it has a clock, `Clk`, and keeps state.
    pub clocked: bool,
    /// The test table, in Digital's language, that a right answer passes.
    pub table: String,
    /// The reference answer: `expressions` or `machine`, and its text.
    pub reference_kind: String,
    pub reference: String,
    /// Each gives away more than the last.
    pub hints: Vec<String>,
    /// Why the reference works.
    pub explanation: String,
    /// `easy`, `medium` or `hard`.
    pub difficulty: String,
    pub topic: String,
    pub cites: Vec<String>,
}

pub fn prompt(count: usize, level: &str, focus: &str, course: &str) -> String {
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!("\n\nCentre them on: {focus}"),
    };
    format!(
        "Write about {count} digital logic problems for a student of {course}, to build as circuits, on what these sources teach — \
         gates, adders, multiplexers, encoders, comparators, flip-flops, counters, state machines — like the exercises and labs \
         among them. {level}\n\n\
         Each problem has:\n\
         - `title`;\n\
         - `statement`, in Markdown: what the circuit does, its inputs and outputs by name, an example;\n\
         - `inputs` and `outputs`: each a name (a letter, then letters, digits or _) and its width in bits, at most 8 inputs \
         in all; a problem with state has a clock as well, named `Clk`, which is not listed;\n\
         - `clocked`: whether it has the clock;\n\
         - `table`: its test table in Digital's test language — a first line of the signal names, inputs first (then `Clk` when \
         clocked), then outputs; then a row each of values: numbers, `X` for an output that may be anything, and for `Clk` the \
         letter `C`, which sets the inputs, pulses the clock once and then checks the outputs. `loop(i, n)` … `end loop` repeats \
         rows with `(i>>1)&1` and the like in them. Cover every row for a few inputs; for a clocked one, a sequence from its \
         start of at least eight clock pulses, its flip-flops starting at 0;\n\
         - `referenceKind` and `reference`: for one without state, `expressions`, with one line per output — `S = A ^ B` — \
         using `'` for not, `·` or side by side for and, `^` for xor, `+` for or, over one-bit inputs named as listed (a \
         multi-bit signal's bits as A0, A1… are not allowed here: keep such problems to one-bit signals); for one with state, \
         `machine`, a state machine as text:\n\
         ```\n\
         inputs X\n\
         outputs Z\n\
         state S0\n\
         state S1 Z=1          # a Moore output\n\
         S0 -> S1 when X\n\
         S1 -> S0 when X' / Z=0   # a Mealy output, on the transition\n\
         ```\n\
         It starts in its first state; at each clock pulse it takes the first transition whose condition holds, or stays;\n\
         - `hints`, two or three, each giving away more than the last;\n\
         - `explanation`: why the reference works;\n\
         - `difficulty`: easy, medium or hard; `topic`; and `cites`, the sources' citations it draws on.\n\n\
         Every table is run against the reference built as a circuit before the student sees it, so they must agree.{focus}"
    )
}

pub fn schema() -> Value {
    let text = json!({ "type": "string" });
    let signals = json!({ "type": "array", "items": {
        "type": "object", "additionalProperties": false, "required": ["name", "bits"],
        "properties": { "name": text, "bits": { "type": "integer", "minimum": 1, "maximum": 16 } }
    }});
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "problems"],
        "properties": {
            "title": text,
            "problems": { "type": "array", "minItems": 1, "maxItems": 12, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["title", "statement", "inputs", "outputs", "clocked", "table", "referenceKind", "reference", "hints", "explanation", "difficulty", "topic", "cites"],
                "properties": {
                    "title": text, "statement": text, "inputs": signals, "outputs": signals,
                    "clocked": { "type": "boolean" }, "table": text,
                    "referenceKind": { "type": "string", "enum": ["expressions", "machine"] },
                    "reference": text,
                    "hints": { "type": "array", "items": text },
                    "explanation": text,
                    "difficulty": { "type": "string", "enum": ["easy", "medium", "hard"] },
                    "topic": text,
                    "cites": { "type": "array", "items": text }
                }
            }}
        }
    })
}

pub fn mend(value: &mut Value) {
    if let Some(title) = value.get_mut("title") {
        crate::tex::mend_json(title);
    }
    for problem in value.get_mut("problems").and_then(Value::as_array_mut).into_iter().flatten() {
        for key in ["title", "statement", "hints", "explanation", "topic"] {
            if let Some(field) = problem.get_mut(key) {
                crate::tex::mend_json(field);
            }
        }
    }
}

/// The reference built as a circuit, and the problem's table run on it:
/// why it does not pass, if it does not.
pub fn check(problem: &StudyCircuitProblem) -> Result<(), String> {
    testlang::check(&problem.table)?;
    let mut project = Project::new();
    project.circuits.clear();
    match problem.reference_kind.as_str() {
        "expressions" => {
            let table = analysis::expressions(&problem.reference)?;
            // The reference's names are the problem's: an input it does not
            // use is still one of the circuit's.
            for i in &table.inputs {
                if !problem.inputs.iter().any(|s| s.name == *i) {
                    return Err(format!("the reference uses {i}, which is not one of the inputs"));
                }
            }
            let mut drawing = analysis::synthesise(&table, analysis::Style::AndOr)?;
            for (k, s) in problem.inputs.iter().enumerate() {
                if !table.inputs.contains(&s.name) {
                    drawing.parts.push(proton_circuit::circ::Part {
                        id: s.name.clone(),
                        kind: "in".into(),
                        x: -10,
                        y: 2 * k as i32,
                        rot: 0,
                        mirror: false,
                        attrs: [("label".to_string(), s.name.clone())].into_iter().collect(),
                    });
                }
            }
            project.circuits.insert("answer".into(), drawing);
            project.top = "answer".into();
        }
        "machine" => {
            let m = fsm::parse(&problem.reference)?;
            fsm::build(&m, fsm::FlipFlop::D, "answer", &mut project)?;
            project.top = "answer".into();
        }
        other => return Err(format!("“{other}” is not a kind of reference")),
    }
    let built = project.build("answer")?;
    let o = testlang::run(&built, &problem.title, &problem.table);
    if let Some(why) = o.error {
        return Err(why);
    }
    if let Some(first) = o.failures.first() {
        return Err(format!("the reference does not pass the table: {first}"));
    }
    if o.rows == 0 {
        return Err("the table has no rows".into());
    }
    Ok(())
}

/// Problems an answer set, each kept only if its reference passes its
/// own table; what was wrong with the others.
pub fn read(value: &Value) -> Result<(String, Vec<StudyCircuitProblem>, Vec<String>), String> {
    let title = value["title"].as_str().unwrap_or("Circuit practice").trim().to_string();
    let items = value["problems"].as_array().ok_or("there are no problems")?;
    let mut problems = Vec::new();
    let mut said = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let text = |key: &str| item[key].as_str().unwrap_or_default().trim_end().to_string();
        let problem: StudyCircuitProblem = match serde_json::from_value(json!({
            "id": format!("c{}", problems.len() + 1),
            "title": text("title").trim(), "statement": text("statement"),
            "inputs": item["inputs"].clone(), "outputs": item["outputs"].clone(),
            "clocked": item["clocked"].as_bool().unwrap_or(false),
            "table": format!("{}\n", text("table")), "referenceKind": text("referenceKind"), "reference": format!("{}\n", text("reference")),
            "hints": item["hints"].clone(), "explanation": text("explanation"),
            "difficulty": text("difficulty"), "topic": text("topic"), "cites": item["cites"].clone(),
        })) {
            Ok(problem) => problem,
            Err(e) => {
                said.push(format!("problem {}: {e}", n + 1));
                continue;
            }
        };
        match check(&problem) {
            Ok(()) => problems.push(problem),
            Err(why) => said.push(format!("problem {} ({}): {why}", n + 1, problem.title)),
        }
    }
    if problems.is_empty() {
        return Err(format!("no problem's reference passed its own table: {}", said.join("; ")));
    }
    Ok((title, problems, said))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(problems: Value) -> Value {
        json!({ "title": "Gates and counters", "problems": problems })
    }

    fn problem(title: &str, clocked: bool, table: &str, kind: &str, reference: &str) -> Value {
        json!({
            "title": title, "statement": "Build it.",
            "inputs": [{"name": "A", "bits": 1}, {"name": "B", "bits": 1}],
            "outputs": [{"name": "Y", "bits": 1}],
            "clocked": clocked, "table": table, "referenceKind": kind, "reference": reference,
            "hints": ["Think of XOR."], "explanation": "It is.", "difficulty": "easy", "topic": "gates", "cites": []
        })
    }

    #[test]
    fn problems_are_kept_only_when_their_reference_passes_their_table() {
        let value = answer(json!([
            problem("XOR", false, "A B Y\n0 0 0\n0 1 1\n1 0 1\n1 1 0", "expressions", "Y = A ^ B"),
            problem("Wrong table", false, "A B Y\n1 1 1", "expressions", "Y = A ^ B"),
            problem("Stray input", false, "A B Y\n0 0 0", "expressions", "Y = A ^ C"),
            problem("A follower", true, "A B Clk Y\n1 0 C 1\n0 0 C 0\n1 1 C 1", "machine", "inputs A B\noutputs Y\nstate Lo\nstate Hi Y=1\nLo -> Hi when A\nHi -> Lo when A'"),
            problem("Unused input", false, "A B Y\n0 0 1\n1 1 0", "expressions", "Y = A'"),
        ]));
        let (title, kept, said) = read(&value).unwrap();
        assert_eq!(title, "Gates and counters");
        assert_eq!(kept.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), vec!["XOR", "A follower", "Unused input"]);
        assert_eq!(kept.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["c1", "c2", "c3"]);
        assert_eq!(said.len(), 2);
        assert!(said[0].contains("Wrong table") && said[0].contains("does not pass"), "{said:?}");
        assert!(said[1].contains("C, which is not one of the inputs"), "{said:?}");
        assert!(read(&answer(json!([problem("Bad", false, "A B Y\n1 1 1", "expressions", "Y = A ^ B")]))).is_err());
        assert!(schema()["properties"]["problems"]["items"]["required"].as_array().unwrap().len() == 13);
        assert!(prompt(4, "", "", "COMP2300").contains("COMP2300"));
    }

    #[test]
    fn a_set_is_read_by_the_studio_kept_and_read_back() {
        use crate::studio::{Make, Options, StudyMadeBody};
        let answer = answer(json!([problem("XOR", false, "A B Y\n0 0 0\n0 1 1\n1 0 1\n1 1 0", "expressions", "Y = A ^ B")])).to_string();
        let Ok(body @ StudyMadeBody::Circuits { .. }) = crate::studio::read(Make::Circuits, &Options::default(), &answer, None) else { panic!("not read") };
        let dir = std::env::temp_dir().join(format!("proton-study-circuits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let book = crate::store::Folder::new(&dir).create("Logic", 1).unwrap();
        let made = book.keep_made(Make::Circuits, "Gates", "Claude Code", "J0001", &body).unwrap();
        assert_eq!(made.id, "C001");
        let Some((header, StudyMadeBody::Circuits { problems, .. })) = book.made_one("C001") else { panic!("not read back") };
        assert_eq!((header.kind, problems.len(), problems[0].reference.as_str()), (Make::Circuits, 1, "Y = A ^ B\n"));
        assert!(book.made().iter().any(|m| m.id == "C001"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
