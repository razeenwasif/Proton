//! End-to-end integration test of the compiled `proton-shell --remote-server`
//! NDJSON server binary over stdio pipes (without SSH).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn remote_server_binary_serves_ready_sync_tree_open_terminal_and_clean_shutdown() {
    let bin = env!("CARGO_BIN_EXE_proton-shell");
    let dir = std::env::temp_dir().join(format!("proton-remote-bin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = dir.join("cfg");
    let ws = dir.join("ws");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/hello.rs"), "pub fn hi() -> i32 { 42 }\n").unwrap();

    let mut child = Command::new(bin)
        .arg("--remote-server")
        .arg(&ws)
        .env("XDG_CONFIG_HOME", &cfg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn proton-shell --remote-server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let read_next = |r: &mut BufReader<_>| -> serde_json::Value {
        loop {
            let mut line = String::new();
            let n = r.read_line(&mut line).expect("read stdout line");
            assert!(n > 0, "remote-server exited unexpectedly");
            let v: serde_json::Value = serde_json::from_str(line.trim()).expect("valid json");
            if v["kind"] == "push" {
                continue;
            }
            return v;
        }
    };

    let ready = read_next(&mut reader);
    assert_eq!(ready["kind"], "ready", "{ready}");
    let root = ready["root"].as_str().unwrap();
    assert_eq!(
        std::path::Path::new(root),
        std::fs::canonicalize(&ws).unwrap()
    );

    writeln!(stdin, r#"{{"kind":"dispatch","id":1,"command":{{"type":"sync"}}}}"#).unwrap();
    stdin.flush().unwrap();
    let reply1 = read_next(&mut reader);
    assert_eq!(reply1["kind"], "reply", "{reply1}");
    assert_eq!(reply1["id"], 1);
    let events1 = reply1["batch"]["events"].as_array().unwrap();
    assert!(
        events1.iter().any(|e| e["type"] == "tree"
            && e["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"] == "src")),
        "expected tree row 'src' in {events1:?}"
    );

    writeln!(
        stdin,
        r#"{{"kind":"dispatch","id":2,"command":{{"type":"openBuffer","path":"src/hello.rs"}}}}"#
    )
    .unwrap();
    stdin.flush().unwrap();
    let reply2 = read_next(&mut reader);
    let events2 = reply2["batch"]["events"].as_array().unwrap();
    assert!(
        events2.iter().any(|e| e["type"] == "viewport"
            && e["lines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("pub fn hi"))),
        "expected file contents in {events2:?}"
    );

    writeln!(stdin, r#"{{"kind":"saveUnsaved","id":3}}"#).unwrap();
    stdin.flush().unwrap();
    let reply3 = read_next(&mut reader);
    assert_eq!(reply3["kind"], "unsaved", "{reply3}");
    assert_eq!(reply3["failed"], serde_json::json!([]));

    writeln!(stdin, r#"{{"kind":"shutdown","id":4}}"#).unwrap();
    stdin.flush().unwrap();
    let ack = read_next(&mut reader);
    assert_eq!(ack["kind"], "ack", "{ack}");
    assert_eq!(ack["id"], 4);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "remote-server exited with {status}");
            break;
        }
        assert!(Instant::now() < deadline, "remote-server did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }

    let _ = std::fs::remove_dir_all(&dir);
}
