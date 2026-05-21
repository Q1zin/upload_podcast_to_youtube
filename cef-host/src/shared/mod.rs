pub mod app;
pub mod commands;
pub mod handler;
pub mod rpc;
pub mod state;

use cef::*;

#[cfg(target_os = "macos")]
pub type Library = library_loader::LibraryLoader;

#[cfg(not(target_os = "macos"))]
pub struct Library;

pub fn load_cef() -> Library {
    #[cfg(target_os = "macos")]
    let library = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load());
        loader
    };
    #[cfg(not(target_os = "macos"))]
    let library = Library;

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    #[cfg(target_os = "macos")]
    crate::mac::setup_host_application();

    library
}

pub fn run_main(main_args: &MainArgs, cmd_line: &CommandLine, sandbox_info: *mut u8) {
    let switch = CefString::from("type");
    let is_browser_process = cmd_line.has_switch(Some(&switch)) != 1;

    let ret = execute_process(Some(main_args), None, sandbox_info);

    if is_browser_process {
        assert_eq!(ret, -1, "cannot execute browser process");
    } else {
        // Non-browser subprocess: execute_process handled everything.
        assert!(ret >= 0, "cannot execute non-browser process");
        return;
    }

    // Extract --user-data-dir=<path> for persistent profile.
    let user_data_dir = CefString::from(
        &cmd_line.switch_value(Some(&CefString::from("user-data-dir"))),
    )
    .to_string();
    if user_data_dir.is_empty() {
        eprintln!("cef-host: missing --user-data-dir=<path>");
        std::process::exit(2);
    }
    if let Err(err) = std::fs::create_dir_all(&user_data_dir) {
        eprintln!("cef-host: failed to create user-data-dir {user_data_dir}: {err}");
        std::process::exit(2);
    }

    let mut app = app::HostApp::new();

    let mut settings = Settings {
        no_sandbox: 1,
        ..Default::default()
    };
    settings.cache_path = CefString::from(user_data_dir.as_str());
    settings.root_cache_path = CefString::from(user_data_dir.as_str());
    settings.persist_session_cookies = 1;

    assert_eq!(
        initialize(Some(main_args), Some(&settings), Some(&mut app), sandbox_info),
        1,
        "cef initialize failed",
    );

    #[cfg(target_os = "macos")]
    let _delegate = crate::mac::setup_host_app_delegate();

    run_message_loop();
    shutdown();
}
