use dashmap::DashMap;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, OnceCell, oneshot},
};

/// Long-lived handle to the cef-host sidecar process. Cloned freely via `Arc`.
pub struct CefSidecar {
    stdin: Mutex<ChildStdin>,
    pending: DashMap<u64, oneshot::Sender<Result<Value, String>>>,
    next_id: AtomicU64,
    #[allow(dead_code)] // kept alive so the OS doesn't reap the child while we're running
    child: Mutex<Child>,
}

/// Managed by Tauri so commands can lazily start (or reuse) the sidecar.
#[derive(Default)]
pub struct CefSidecarSlot(pub OnceCell<Arc<CefSidecar>>);

impl CefSidecar {
    /// Resolve the path to the bundled `cef-host` executable.
    /// In dev: env var `CEF_HOST_BIN` overrides; otherwise default to the dev bundle path
    /// relative to this crate (`../cef-host/target/bundle/cef-host.app/Contents/MacOS/cef-host`).
    /// In prod: `<app-resources>/cef-host.app/Contents/MacOS/cef-host`.
    fn resolve_binary(app: &AppHandle) -> Result<PathBuf, String> {
        if let Ok(path) = std::env::var("CEF_HOST_BIN") {
            return Ok(PathBuf::from(path));
        }

        // Try the dev bundle path first (relative to src-tauri).
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dev_path = manifest_dir
            .join("..")
            .join("cef-host")
            .join("target")
            .join("bundle")
            .join("cef-host.app")
            .join("Contents")
            .join("MacOS")
            .join("cef-host");
        if dev_path.exists() {
            return Ok(dev_path);
        }

        // Production: bundled as a resource inside the Tauri .app.
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| format!("resource_dir: {e}"))?;
        let prod_path = resource_dir
            .join("cef-host.app")
            .join("Contents")
            .join("MacOS")
            .join("cef-host");
        if prod_path.exists() {
            return Ok(prod_path);
        }

        Err(format!(
            "cef-host binary not found. Tried {dev_path:?} and {prod_path:?}. \
             Set CEF_HOST_BIN to override.",
        ))
    }

    fn resolve_user_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {e}"))?;
        let dir = base.join("cef-profile");
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir cef-profile: {e}"))?;
        Ok(dir)
    }

    pub async fn spawn(app: &AppHandle) -> Result<Arc<Self>, String> {
        let bin = Self::resolve_binary(app)?;
        let user_data_dir = Self::resolve_user_data_dir(app)?;

        let mut cmd = Command::new(&bin);
        cmd.arg(format!("--user-data-dir={}", user_data_dir.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| format!("spawn cef-host: {e}"))?;
        let stdin = child.stdin.take().ok_or("missing stdin")?;
        let stdout = child.stdout.take().ok_or("missing stdout")?;

        let sidecar = Arc::new(Self {
            stdin: Mutex::new(stdin),
            pending: DashMap::new(),
            next_id: AtomicU64::new(1),
            child: Mutex::new(child),
        });

        // Stdout reader: parse line-delimited JSON, route by id or emit as event.
        let sidecar_clone = sidecar.clone();
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(trimmed) {
                            Ok(value) => sidecar_clone.handle_message(&app_clone, value),
                            Err(err) => {
                                eprintln!("cef-host: bad json {err}: {trimmed}");
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(err) => {
                        eprintln!("cef-host: stdout read error: {err}");
                        break;
                    }
                }
            }

            // Sidecar exited; fail every pending request so callers don't hang forever.
            let pending_ids: Vec<u64> =
                sidecar_clone.pending.iter().map(|e| *e.key()).collect();
            for id in pending_ids {
                if let Some((_, sender)) = sidecar_clone.pending.remove(&id) {
                    let _ = sender.send(Err("cef-host exited".into()));
                }
            }
        });

        Ok(sidecar)
    }

    fn handle_message(&self, app: &AppHandle, value: Value) {
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            // Response to a request we sent.
            if let Some((_, sender)) = self.pending.remove(&id) {
                let payload = if let Some(err) = value.get("error").and_then(Value::as_str) {
                    Err(err.to_string())
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = sender.send(payload);
            }
            return;
        }
        // Notification (no id): forward to the frontend.
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            let event_name = format!("cef://{method}");
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            let _ = app.emit(&event_name, params);
        }
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);

        let request = json!({ "id": id, "method": method, "params": params });
        let line = format!("{request}\n");

        let send_result = {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await
        };
        if let Err(err) = send_result {
            self.pending.remove(&id);
            return Err(format!("write stdin: {err}"));
        }

        rx.await.map_err(|_| "sidecar channel closed".to_string())?
    }
}

/// Lazily start the sidecar (or return the existing one).
pub async fn get_or_start(app: &AppHandle) -> Result<Arc<CefSidecar>, String> {
    let state = app.state::<CefSidecarSlot>();
    if let Some(sidecar) = state.0.get() {
        return Ok(sidecar.clone());
    }
    let sidecar = CefSidecar::spawn(app).await?;
    let _ = state.0.set(sidecar.clone());
    Ok(state.0.get().expect("just set").clone())
}

// ---------------------------------------------------------------------------
// Tauri command wrappers.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cef_open(app: AppHandle, url: String) -> Result<Value, String> {
    let sidecar = get_or_start(&app).await?;
    sidecar.call("open", json!({ "url": url })).await
}

#[tauri::command]
pub async fn cef_navigate(
    app: AppHandle,
    browser_id: u64,
    url: String,
) -> Result<Value, String> {
    let sidecar = get_or_start(&app).await?;
    sidecar
        .call("navigate", json!({ "browser_id": browser_id, "url": url }))
        .await
}

#[tauri::command]
pub async fn cef_eval(
    app: AppHandle,
    browser_id: u64,
    code: String,
) -> Result<Value, String> {
    let sidecar = get_or_start(&app).await?;
    sidecar
        .call("eval", json!({ "browser_id": browser_id, "code": code }))
        .await
}

#[tauri::command]
pub async fn cef_query(
    app: AppHandle,
    browser_id: u64,
    code: String,
    tag: String,
) -> Result<Value, String> {
    let sidecar = get_or_start(&app).await?;
    sidecar
        .call(
            "query",
            json!({ "browser_id": browser_id, "code": code, "tag": tag }),
        )
        .await
}

#[tauri::command]
pub async fn cef_close(app: AppHandle, browser_id: u64) -> Result<Value, String> {
    let sidecar = get_or_start(&app).await?;
    sidecar
        .call("close", json!({ "browser_id": browser_id }))
        .await
}

#[tauri::command]
pub async fn cef_shutdown(app: AppHandle) -> Result<Value, String> {
    let state = app.state::<CefSidecarSlot>();
    let Some(sidecar) = state.0.get() else {
        return Ok(Value::Null);
    };
    sidecar.call("shutdown", json!({})).await
}
