//! The handful of DAP shapes Proton actually uses.
//!
//! Hand-rolled rather than generated, for the reason `proton-lsp`'s protocol
//! module gives: the surface an editor touches is small, and every message in
//! it has a subtlety worth writing down rather than inheriting from a schema.
//!
//! Everything here is *incoming*. Requests are built inline where they are
//! sent, because a request is one JSON object and a struct per verb would be
//! ceremony around nothing.

use std::path::PathBuf;

/// A frame on the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    /// Where it is, if the adapter knows. A frame in a system library has no
    /// source, and that is ordinary rather than an error.
    pub path: Option<PathBuf>,
    pub line: usize,
}

/// A named group of variables — "Locals", "Arguments".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub name: String,
    pub reference: i64,
    /// Fetching it is slow enough that the adapter would rather you did not
    /// unless asked. Registers, usually.
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    /// Non-zero when it has children to expand — a struct, an array.
    pub children: i64,
}

/// Something the adapter said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// The program stopped. `reason` is the adapter's word for why —
    /// "breakpoint", "step", "exception".
    Stopped {
        thread: i64,
        reason: String,
    },
    Continued,
    /// The program finished. `None` when the adapter did not say what with.
    Exited {
        code: Option<i64>,
    },
    Terminated,
    /// Program output, or the adapter talking about itself.
    Output {
        text: String,
    },
    /// A breakpoint's real state, which arrives *after* the response that
    /// created it — see the handshake note in the crate docs.
    BreakpointChanged {
        path: Option<PathBuf>,
        line: usize,
        verified: bool,
    },
    Stack(Vec<Frame>),
    Scopes(Vec<Scope>),
    Variables {
        reference: i64,
        items: Vec<Variable>,
    },
    /// A request the adapter refused. Worth surfacing: a debugger that
    /// silently does not step is one you cannot debug.
    Failed {
        command: String,
        message: String,
    },
}

/// Turn one adapter message into whatever it means. Possibly nothing.
pub fn interpret(message: &serde_json::Value) -> Vec<Incoming> {
    match message["type"].as_str() {
        Some("event") => event(message),
        Some("response") => response(message),
        _ => Vec::new(),
    }
}

fn event(message: &serde_json::Value) -> Vec<Incoming> {
    let body = &message["body"];
    match message["event"].as_str().unwrap_or("") {
        "stopped" => vec![Incoming::Stopped {
            thread: body["threadId"].as_i64().unwrap_or(1),
            reason: body["reason"].as_str().unwrap_or("").to_string(),
        }],
        "continued" => vec![Incoming::Continued],
        "exited" => vec![Incoming::Exited {
            code: body["exitCode"].as_i64(),
        }],
        "terminated" => vec![Incoming::Terminated],
        "output" => body["output"]
            .as_str()
            .map(|text| {
                vec![Incoming::Output {
                    text: text.to_string(),
                }]
            })
            .unwrap_or_default(),
        "breakpoint" => {
            let breakpoint = &body["breakpoint"];
            match breakpoint["line"].as_u64() {
                Some(line) => vec![Incoming::BreakpointChanged {
                    path: source_path(&breakpoint["source"]),
                    line: line as usize,
                    verified: breakpoint["verified"].as_bool().unwrap_or(false),
                }],
                // A breakpoint with no line is one the adapter could not place
                // anywhere, and there is nothing to draw.
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn response(message: &serde_json::Value) -> Vec<Incoming> {
    let command = message["command"].as_str().unwrap_or("").to_string();
    if message["success"].as_bool() == Some(false) {
        return vec![Incoming::Failed {
            message: message["message"]
                .as_str()
                .unwrap_or("the adapter refused")
                .to_string(),
            command,
        }];
    }
    let body = &message["body"];

    match command.as_str() {
        "stackTrace" => vec![Incoming::Stack(
            body["stackFrames"]
                .as_array()
                .map(|frames| frames.iter().map(frame).collect())
                .unwrap_or_default(),
        )],
        "scopes" => vec![Incoming::Scopes(
            body["scopes"]
                .as_array()
                .map(|scopes| scopes.iter().map(scope).collect())
                .unwrap_or_default(),
        )],
        "variables" => vec![Incoming::Variables {
            // The adapter does not echo which reference was asked for, and the
            // core only ever has one outstanding, so this is filled in there.
            reference: 0,
            items: body["variables"]
                .as_array()
                .map(|items| items.iter().map(variable).collect())
                .unwrap_or_default(),
        }],
        // `setBreakpoints` is deliberately not read here. Its answer is
        // provisional — pending until the program loads — and the `breakpoint`
        // event is the one that tells the truth.
        _ => Vec::new(),
    }
}

fn frame(value: &serde_json::Value) -> Frame {
    Frame {
        id: value["id"].as_i64().unwrap_or(0),
        name: value["name"].as_str().unwrap_or("??").to_string(),
        path: source_path(&value["source"]),
        line: value["line"].as_u64().unwrap_or(0) as usize,
    }
}

fn scope(value: &serde_json::Value) -> Scope {
    Scope {
        name: value["name"].as_str().unwrap_or("").to_string(),
        reference: value["variablesReference"].as_i64().unwrap_or(0),
        expensive: value["expensive"].as_bool().unwrap_or(false),
    }
}

fn variable(value: &serde_json::Value) -> Variable {
    Variable {
        name: value["name"].as_str().unwrap_or("").to_string(),
        value: value["value"].as_str().unwrap_or("").to_string(),
        children: value["variablesReference"].as_i64().unwrap_or(0),
    }
}

fn source_path(source: &serde_json::Value) -> Option<PathBuf> {
    Some(PathBuf::from(source["path"].as_str()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Incoming> {
        interpret(&serde_json::from_str(text).expect("valid json"))
    }

    #[test]
    fn a_stop_says_which_thread_and_why() {
        let found = parse(
            r#"{"type":"event","event":"stopped",
                "body":{"reason":"breakpoint","threadId":1,"allThreadsStopped":true}}"#,
        );
        assert_eq!(
            found,
            vec![Incoming::Stopped {
                thread: 1,
                reason: "breakpoint".into()
            }]
        );
    }

    #[test]
    fn a_breakpoint_event_carries_the_verification_the_response_could_not() {
        // `setBreakpoints` answers `pending` before the program is loaded, and
        // this is the message that says whether it actually landed. A UI that
        // reads only the response shows every breakpoint as broken.
        let found = parse(
            r#"{"type":"event","event":"breakpoint","body":{"reason":"changed",
                "breakpoint":{"id":1,"line":4,"verified":true,
                "source":{"name":"demo.c","path":"C:\\tmp\\demo.c"}}}}"#,
        );
        assert_eq!(
            found,
            vec![Incoming::BreakpointChanged {
                path: Some(PathBuf::from(r"C:\tmp\demo.c")),
                line: 4,
                verified: true,
            }]
        );
    }

    #[test]
    fn a_breakpoint_with_nowhere_to_go_is_not_a_mark() {
        let found = parse(
            r#"{"type":"event","event":"breakpoint",
                "body":{"reason":"changed","breakpoint":{"id":1,"verified":false}}}"#,
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_stack_frame_without_source_is_still_a_frame() {
        // A frame in a system library has no source, and that is ordinary.
        let found = parse(
            r#"{"type":"response","command":"stackTrace","success":true,"body":{
                "stackFrames":[
                    {"id":0,"name":"add","line":4,"source":{"path":"C:\\tmp\\demo.c"}},
                    {"id":1,"name":"ntdll!Rtl","line":0}]}}"#,
        );
        let Some(Incoming::Stack(frames)) = found.first() else {
            panic!("{found:?}");
        };
        assert_eq!(frames[0].name, "add");
        assert_eq!(frames[0].path, Some(PathBuf::from(r"C:\tmp\demo.c")));
        assert_eq!(frames[1].path, None);
    }

    #[test]
    fn a_frame_id_of_zero_is_a_real_frame() {
        // gdb numbers from zero, and treating the id as "missing" would drop
        // the innermost frame — the only one anybody is looking at.
        let found = parse(
            r#"{"type":"response","command":"stackTrace","success":true,
                "body":{"stackFrames":[{"id":0,"name":"main","line":9}]}}"#,
        );
        let Some(Incoming::Stack(frames)) = found.first() else {
            panic!("{found:?}");
        };
        assert_eq!(frames[0].id, 0);
    }

    #[test]
    fn variables_come_back_with_their_values() {
        let found = parse(
            r#"{"type":"response","command":"variables","success":true,"body":{
                "variables":[
                    {"name":"a","value":"0","variablesReference":0},
                    {"name":"point","value":"{...}","variablesReference":7}]}}"#,
        );
        let Some(Incoming::Variables { items, .. }) = found.first() else {
            panic!("{found:?}");
        };
        assert_eq!(items[0].value, "0");
        assert_eq!(items[1].children, 7, "a struct can be expanded");
    }

    #[test]
    fn a_refusal_is_reported_rather_than_dropped() {
        // A debugger that silently does not step is one you cannot debug.
        let found = parse(
            r#"{"type":"response","command":"stepIn","success":false,
                "message":"not stopped"}"#,
        );
        assert_eq!(
            found,
            vec![Incoming::Failed {
                command: "stepIn".into(),
                message: "not stopped".into()
            }]
        );
    }

    #[test]
    fn the_set_breakpoints_response_is_deliberately_ignored() {
        // It is provisional. The `breakpoint` event is the one that tells the
        // truth, and reading both would fight.
        let found = parse(
            r#"{"type":"response","command":"setBreakpoints","success":true,
                "body":{"breakpoints":[{"id":1,"verified":false,"reason":"pending"}]}}"#,
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn noise_is_not_an_event() {
        assert!(parse(r#"{"type":"event","event":"module","body":{}}"#).is_empty());
        assert!(parse(r#"{"type":"event","event":"thread","body":{}}"#).is_empty());
    }
}
