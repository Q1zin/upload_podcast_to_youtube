// Library crate that hosts the shared CEF code used by both `cef-host` (main process)
// and `cef_host_helper` (subprocess). The helper binary just calls `cef::execute_process`
// directly, while the main binary uses `shared::run_main` to drive the message loop.

pub mod shared;

#[cfg(target_os = "macos")]
pub mod mac;
