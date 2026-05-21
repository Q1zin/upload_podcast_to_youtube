mod cef;

use cef::CefSidecarSlot;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CefSidecarSlot::default())
        .invoke_handler(tauri::generate_handler![
            cef::cef_open,
            cef::cef_navigate,
            cef::cef_eval,
            cef::cef_query,
            cef::cef_show,
            cef::cef_hide,
            cef::cef_dev_tools,
            cef::cef_close,
            cef::cef_shutdown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
