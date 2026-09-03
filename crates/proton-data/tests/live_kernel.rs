//! The kernel client against a real Jupyter kernel.
//!
//! Ignored by default because it starts one, the way the language-server,
//! debug-adapter and shell tests do. Point `PROTON_TEST_KERNEL` at a Python
//! that has `ipykernel` installed:
//!
//! ```bash
//! PROTON_TEST_KERNEL=.venv/Scripts/python.exe \
//! cargo test -p proton-data --test live_kernel -- --include-ignored --test-threads=1
//! ```
//!
//! Everything that does not need a kernel — signing, framing, the delimiter,
//! port reservation — is tested unconditionally in `src/`.

use std::time::{Duration, Instant};

use proton_data::Kernel;

/// The kernel to test with, if the environment named one.
fn python() -> Option<String> {
    let path = std::env::var("PROTON_TEST_KERNEL").ok()?;
    std::path::Path::new(&path).exists().then_some(path)
}

fn start() -> Option<Kernel> {
    let python = python()?;
    let argv = vec![
        python,
        "-m".into(),
        "ipykernel_launcher".into(),
        "-f".into(),
        "{connection_file}".into(),
    ];
    let mut kernel = Kernel::start(&argv, &std::env::temp_dir()).expect("the kernel should start");
    let info = kernel
        .wait_ready(Duration::from_secs(60))
        .unwrap_or_else(|err| panic!("kernel not ready: {err}"));
    assert!(
        info.get("implementation").is_some(),
        "kernel_info_reply should say what it is: {info}"
    );
    Some(kernel)
}

/// Run `code` and collect everything it produced, to the closing `idle`.
///
/// Waiting for `idle` with the right parent is the only honest end-of-cell
/// signal. Stopping at `execute_reply` races: the reply can arrive while
/// output is still coming, and a test that stops there passes or fails
/// depending on the machine.
fn run(kernel: &mut Kernel, code: &str) -> Vec<proton_data::Message> {
    let id = kernel.execute(code).expect("send");
    let mut collected = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        for message in kernel.poll() {
            let mine = message.parent_id() == Some(id.as_str());
            if !mine {
                continue;
            }
            let done =
                message.msg_type() == "status" && message.content["execution_state"] == "idle";
            collected.push(message);
            if done {
                return collected;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the cell never finished; got {} messages", collected.len());
}

fn of_type<'a>(messages: &'a [proton_data::Message], want: &str) -> Vec<&'a proton_data::Message> {
    messages.iter().filter(|m| m.msg_type() == want).collect()
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn a_kernel_starts_and_says_what_it_is() {
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    assert!(kernel.is_alive());
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn code_runs_and_its_result_comes_back() {
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    let messages = run(&mut kernel, "1 + 1");

    let results = of_type(&messages, "execute_result");
    assert_eq!(results.len(), 1, "{messages:?}");
    assert_eq!(results[0].content["data"]["text/plain"], "2");
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn printing_arrives_as_a_stream_not_as_a_result() {
    // The distinction the renderer depends on: what a cell *printed* and what
    // it *evaluated to* are different messages, and a cell can produce both.
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    let messages = run(&mut kernel, "print('hello')\n40 + 2");

    let streams = of_type(&messages, "stream");
    assert!(
        streams.iter().any(|m| m.content["name"] == "stdout"
            && m.content["text"]
                .as_str()
                .is_some_and(|t| t.contains("hello"))),
        "{messages:?}"
    );
    let results = of_type(&messages, "execute_result");
    assert_eq!(results[0].content["data"]["text/plain"], "42");
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn an_error_is_reported_rather_than_lost() {
    // A cell that raises must show the traceback. Losing it is the failure
    // that makes a notebook useless: the cell simply appears to do nothing.
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    let messages = run(&mut kernel, "raise ValueError('nope')");

    let errors = of_type(&messages, "error");
    assert_eq!(errors.len(), 1, "{messages:?}");
    assert_eq!(errors[0].content["ename"], "ValueError");
    assert_eq!(errors[0].content["evalue"], "nope");
    assert!(
        errors[0].content["traceback"]
            .as_array()
            .is_some_and(|t| !t.is_empty()),
        "a traceback is the useful part"
    );
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn state_persists_between_cells() {
    // What makes it a notebook rather than a script runner: the second cell
    // can see what the first one did.
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    run(&mut kernel, "x = 21");
    let messages = run(&mut kernel, "x * 2");
    assert_eq!(
        of_type(&messages, "execute_result")[0].content["data"]["text/plain"],
        "42"
    );
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn output_is_tagged_with_the_cell_that_caused_it() {
    // Two cells in flight at once is the normal case for a notebook, and
    // `parent_header` is the only thing that says which output belongs to
    // which. Getting this wrong puts one cell's output under another.
    let Some(mut kernel) = start() else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    let first = kernel.execute("print('FIRST')").unwrap();
    let second = kernel.execute("print('SECOND')").unwrap();

    let mut said: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline && said.len() < 2 {
        for message in kernel.poll() {
            if message.msg_type() != "stream" {
                continue;
            }
            let Some(parent) = message.parent_id().map(str::to_string) else {
                continue;
            };
            let text = message.content["text"].as_str().unwrap_or("").to_string();
            said.entry(parent).or_default().push_str(&text);
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(said[&first].contains("FIRST"), "{said:?}");
    assert!(said[&second].contains("SECOND"), "{said:?}");
    assert!(
        !said[&first].contains("SECOND"),
        "output crossed cells: {said:?}"
    );
}
