use crate::shared::{handler::HostHandler, rpc::spawn_stdin_loop};
use cef::*;
use std::sync::{Arc, Mutex};

wrap_app! {
    pub struct HostApp;

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            // HostHandler is a singleton — get_or_init returns the same Arc every time.
            Some(HostBrowserProcessHandler::new(HostHandler::get_or_init()))
        }
    }
}

wrap_browser_process_handler! {
    struct HostBrowserProcessHandler {
        _handler: Arc<Mutex<HostHandler>>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            // CEF is ready — start serving JSON-RPC requests from stdin.
            spawn_stdin_loop();
            // Tell the parent process we're ready to receive commands.
            crate::shared::rpc::write_notification("ready", serde_json::json!({}));
        }
    }
}
