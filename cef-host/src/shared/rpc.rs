use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    sync::{Mutex, OnceLock},
    thread,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response<'a> {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'a str>,
}

static WRITER: OnceLock<Mutex<std::io::Stdout>> = OnceLock::new();

fn writer() -> &'static Mutex<std::io::Stdout> {
    WRITER.get_or_init(|| Mutex::new(std::io::stdout()))
}

pub fn write_message(value: Value) {
    let line = serde_json::to_string(&value).expect("serialize message");
    let stdout = writer();
    let mut guard = stdout.lock().expect("stdout lock");
    let _ = guard.write_all(line.as_bytes());
    let _ = guard.write_all(b"\n");
    let _ = guard.flush();
}

pub fn write_response_ok(id: u64, result: Value) {
    write_message(json!({ "id": id, "result": result }));
}

pub fn write_response_err(id: u64, error: impl Into<String>) {
    let error: String = error.into();
    write_message(json!({ "id": id, "error": error }));
}

pub fn write_notification(method: &str, params: Value) {
    write_message(json!({ "method": method, "params": params }));
}

/// Spawn the stdin reader thread. Each line is parsed as a `Request` and dispatched
/// onto the CEF UI thread via `post_task`.
pub fn spawn_stdin_loop() {
    thread::spawn(|| {
        let stdin = std::io::stdin();
        let reader = BufReader::new(stdin.lock());
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<Request>(line) {
                Ok(req) => {
                    crate::shared::commands::dispatch_on_ui_thread(req);
                }
                Err(err) => {
                    // Best-effort error notification (no id available).
                    write_notification(
                        "parse_error",
                        json!({ "error": err.to_string(), "raw": line }),
                    );
                }
            }
        }
        // Stdin closed — parent likely exited. Best-effort: shut down on the UI thread.
        crate::shared::commands::dispatch_shutdown_on_ui_thread();
    });
}
