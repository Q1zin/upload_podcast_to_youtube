use crate::shared::{
    commands::{browser_id_of, on_browser_closed, on_browser_created},
    rpc::write_notification,
    state::host_state,
};
use cef::*;
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};

/// Bridge object — holds shutdown bookkeeping. All browser state lives in `host_state()`.
pub struct HostHandler {
    is_closing: bool,
}

/// Strong reference; the handler outlives the whole process.
static HOST_HANDLER_INSTANCE: OnceLock<Arc<Mutex<HostHandler>>> = OnceLock::new();

impl HostHandler {
    pub fn instance() -> Option<Arc<Mutex<Self>>> {
        HOST_HANDLER_INSTANCE.get().cloned()
    }

    /// Lazily initialize the singleton. Safe to call from multiple `browser_process_handler`
    /// invocations — only the first call constructs the instance.
    pub fn get_or_init() -> Arc<Mutex<Self>> {
        HOST_HANDLER_INSTANCE
            .get_or_init(|| Arc::new(Mutex::new(Self { is_closing: false })))
            .clone()
    }

    pub fn is_closing(&self) -> bool {
        self.is_closing
    }

    pub fn close_all_browsers(&mut self, force_close: bool) {
        self.is_closing = true;
        let state = host_state();
        let state = state.lock().expect("host state lock");
        for browser in state.browsers.values() {
            if let Some(host) = browser.host() {
                host.close_browser(force_close.into());
            }
        }
    }
}

/// Construct a fresh `Client` that wraps the singleton `HostHandler`.
pub fn host_client() -> Client {
    HostClient::new(HostHandler::get_or_init())
}

wrap_client! {
    pub struct HostClient {
        inner: Arc<Mutex<HostHandler>>,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(HostDisplayHandler::new(self.inner.clone()))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(HostLifeSpanHandler::new(self.inner.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(HostLoadHandler::new(self.inner.clone()))
        }
    }
}

wrap_display_handler! {
    struct HostDisplayHandler {
        inner: Arc<Mutex<HostHandler>>,
    }

    impl DisplayHandler {
        fn on_console_message(
            &self,
            browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: i32,
        ) -> i32 {
            let Some(message) = message else { return 0; };
            let text = message.to_string();
            let Some(payload) = text.strip_prefix("CEFHOST::") else {
                return 0; // not our message — let CEF log normally
            };

            let browser_id = browser.and_then(browser_id_of);
            match serde_json::from_str::<serde_json::Value>(payload) {
                Ok(value) => write_notification(
                    "js_callback",
                    json!({ "browser_id": browser_id, "payload": value }),
                ),
                Err(err) => write_notification(
                    "js_callback_error",
                    json!({ "browser_id": browser_id, "raw": payload, "error": err.to_string() }),
                ),
            }
            1 // we handled it — suppress default console logging
        }
    }
}

wrap_life_span_handler! {
    struct HostLifeSpanHandler {
        inner: Arc<Mutex<HostHandler>>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                on_browser_created(browser.clone());
            }
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            // Allow the OS close event to flow.
            0
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                on_browser_closed(browser);
            }
        }
    }
}

wrap_load_handler! {
    struct HostLoadHandler {
        inner: Arc<Mutex<HostHandler>>,
    }

    impl LoadHandler {
        fn on_load_end(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            http_status_code: i32,
        ) {
            let is_main = frame.as_ref().map(|f| f.is_main() != 0).unwrap_or(false);
            if !is_main {
                return;
            }
            let browser_id = browser.and_then(browser_id_of);
            let url = frame
                .and_then(|f| {
                    let userfree = f.url();
                    Some(CefString::from(&userfree).to_string())
                })
                .unwrap_or_default();
            write_notification(
                "load_end",
                json!({
                    "browser_id": browser_id,
                    "url": url,
                    "http_status": http_status_code,
                }),
            );
        }

        fn on_load_error(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let is_main = frame.as_ref().map(|f| f.is_main() != 0).unwrap_or(false);
            if !is_main {
                return;
            }
            let browser_id = browser.and_then(browser_id_of);
            write_notification(
                "load_error",
                json!({
                    "browser_id": browser_id,
                    "url": failed_url.map(CefString::to_string).unwrap_or_default(),
                    "error_code": sys::cef_errorcode_t::from(error_code) as i32,
                    "error_text": error_text.map(CefString::to_string).unwrap_or_default(),
                }),
            );
        }
    }
}
