use crate::shared::{
    handler::host_client,
    rpc::{Request, write_notification, write_response_err, write_response_ok},
    state::host_state,
};
use cef::*;
use serde_json::{Value, json};

/// Wrap a `Request` into a CEF Task and post it to the UI thread.
pub fn dispatch_on_ui_thread(request: Request) {
    let mut task = RpcTask::new(request);
    post_task(ThreadId::UI, Some(&mut task));
}

/// Post a special "shutdown" task — used when stdin closes (parent exited).
pub fn dispatch_shutdown_on_ui_thread() {
    let req = Request {
        id: 0,
        method: "shutdown".to_string(),
        params: Value::Null,
    };
    dispatch_on_ui_thread(req);
}

wrap_task! {
    struct RpcTask {
        request: Request,
    }

    impl Task {
        fn execute(&self) {
            handle_request(&self.request);
        }
    }
}

fn handle_request(req: &Request) {
    let result = match req.method.as_str() {
        "open" => handle_open(req),
        "navigate" => handle_navigate(req),
        "eval" => handle_eval(req),
        "query" => handle_query(req),
        "close" => handle_close(req),
        "shutdown" => handle_shutdown(req),
        other => Err(format!("unknown method: {other}")),
    };

    match result {
        Ok(None) => {} // deferred response (e.g. `open`)
        Ok(Some(value)) => write_response_ok(req.id, value),
        Err(err) => write_response_err(req.id, err),
    }
}

/// Returns `Ok(None)` when the response is deferred (e.g. after `on_after_created`).
type CmdResult = Result<Option<Value>, String>;

fn handle_open(req: &Request) -> CmdResult {
    let url = req
        .params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("missing 'url' string param")?;

    // Defer the response until on_after_created assigns a browser_id.
    {
        let state = host_state();
        let mut state = state.lock().expect("host state lock");
        state.pending_opens.push_back(req.id);
    }

    let window_info = WindowInfo {
        runtime_style: RuntimeStyle::CHROME,
        ..Default::default()
    };
    let settings = BrowserSettings::default();
    let url = CefString::from(url);
    let mut client = host_client();

    browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&settings),
        None,
        None,
    );

    Ok(None)
}

fn with_browser<F, R>(req: &Request, f: F) -> Result<R, String>
where
    F: FnOnce(&mut Browser) -> Result<R, String>,
{
    let browser_id = req
        .params
        .get("browser_id")
        .and_then(|v| v.as_u64())
        .ok_or("missing 'browser_id'")? as u32;

    let state = host_state();
    let mut state = state.lock().expect("host state lock");
    let browser = state
        .browsers
        .get_mut(&browser_id)
        .ok_or_else(|| format!("no browser with id {browser_id}"))?;
    f(browser)
}

fn handle_navigate(req: &Request) -> CmdResult {
    let url = req
        .params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("missing 'url' string param")?
        .to_string();
    with_browser(req, |browser| {
        let frame = browser.main_frame().ok_or("no main frame")?;
        frame.load_url(Some(&CefString::from(url.as_str())));
        Ok(())
    })?;
    Ok(Some(json!({})))
}

fn handle_eval(req: &Request) -> CmdResult {
    let code = req
        .params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or("missing 'code' string param")?
        .to_string();
    with_browser(req, |browser| {
        let frame = browser.main_frame().ok_or("no main frame")?;
        frame.execute_java_script(Some(&CefString::from(code.as_str())), None, 0);
        Ok(())
    })?;
    Ok(Some(json!({})))
}

fn handle_query(req: &Request) -> CmdResult {
    let code = req
        .params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or("missing 'code' string param")?
        .to_string();
    let tag = req
        .params
        .get("tag")
        .and_then(|v| v.as_str())
        .ok_or("missing 'tag' string param")?
        .to_string();

    // Wrap user code so the result/error is reported via `console.log("CEFHOST::<json>")`.
    // We pick console.log as a one-way bridge — DisplayHandler::on_console_message in the
    // browser process intercepts those lines and emits `js_callback` notifications.
    let tag_json = serde_json::to_string(&tag).unwrap();
    let wrapped = format!(
        "(async () => {{ try {{ const r = await (async () => {{ {code} }})(); \
          console.log('CEFHOST::' + JSON.stringify({{ tag: {tag_json}, ok: r === undefined ? null : r }})); }} \
          catch (e) {{ console.log('CEFHOST::' + JSON.stringify({{ tag: {tag_json}, err: String(e && e.message || e) }})); }} }})();"
    );

    with_browser(req, |browser| {
        let frame = browser.main_frame().ok_or("no main frame")?;
        frame.execute_java_script(Some(&CefString::from(wrapped.as_str())), None, 0);
        Ok(())
    })?;
    Ok(Some(json!({})))
}

fn handle_close(req: &Request) -> CmdResult {
    let browser_id = req
        .params
        .get("browser_id")
        .and_then(|v| v.as_u64())
        .ok_or("missing 'browser_id'")? as u32;

    let host = {
        let state = host_state();
        let state = state.lock().expect("host state lock");
        let browser = state
            .browsers
            .get(&browser_id)
            .ok_or_else(|| format!("no browser with id {browser_id}"))?;
        browser.host().ok_or("no browser host")?
    };
    host.close_browser(0);
    Ok(Some(json!({})))
}

fn handle_shutdown(_req: &Request) -> CmdResult {
    let hosts: Vec<_> = {
        let state = host_state();
        let mut state = state.lock().expect("host state lock");
        state.shutting_down = true;
        state
            .browsers
            .values()
            .filter_map(|b| b.host())
            .collect()
    };

    if hosts.is_empty() {
        quit_message_loop();
    } else {
        for host in hosts {
            host.close_browser(1);
        }
    }
    Ok(Some(json!({})))
}

/// Called by the LifeSpan handler when a new browser has been created. Assigns the next
/// `browser_id`, stores the Browser, and replies to the oldest pending `open` request.
pub fn on_browser_created(browser: Browser) {
    let (rpc_id, browser_id) = {
        let state = host_state();
        let mut state = state.lock().expect("host state lock");
        let browser_id = state.allocate_browser_id();
        state.browsers.insert(browser_id, browser);
        let rpc_id = state.pending_opens.pop_front();
        (rpc_id, browser_id)
    };

    if let Some(rpc_id) = rpc_id {
        write_response_ok(rpc_id, json!({ "browser_id": browser_id }));
    } else {
        // No pending open (popup?). Just notify.
        write_notification(
            "browser_created",
            json!({ "browser_id": browser_id }),
        );
    }
}

/// Called by the LifeSpan handler just before a browser is destroyed.
/// Removes it from the map, emits `browser_closed`, and quits the loop if shutting down.
pub fn on_browser_closed(browser: &mut Browser) {
    let browser_id = browser_id_of(browser);
    let should_quit = {
        let state = host_state();
        let mut state = state.lock().expect("host state lock");
        if let Some(id) = browser_id {
            state.browsers.remove(&id);
        }
        state.shutting_down && state.browsers.is_empty()
    };

    if let Some(id) = browser_id {
        write_notification("browser_closed", json!({ "browser_id": id }));
    }

    if should_quit {
        quit_message_loop();
    }
}

pub fn browser_id_of(browser: &mut Browser) -> Option<u32> {
    let state = host_state();
    let state = state.lock().expect("host state lock");
    for (id, b) in state.browsers.iter() {
        // `is_same` returns 1 if same browser. We need a mutable reference for is_same,
        // but we have an &Browser via iteration; clone to compare.
        let other = b.clone();
        if other.is_same(Some(browser)) != 0 {
            return Some(*id);
        }
    }
    None
}
