//! UnifiedStream desktop application entry point.
//!
//! All application logic lives here rather than in `main.rs` so mobile builds, which replace
//! `main()` with a generated entry point, share the same code path.

mod app;

use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

/// Initialize logging, then run the Tauri event loop.
///
/// # Panics
///
/// Panics if the Tauri context cannot be built or the app config directory is unavailable —
/// both are environment faults worth surfacing loudly at startup rather than limping on.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,unifiedstream_net=debug,desktop=debug")),
        )
        .init();

    tracing::info!(
        control_port = unifiedstream_net::DEFAULT_CONTROL_PORT,
        media_port = unifiedstream_net::DEFAULT_MEDIA_PORT,
        "starting UnifiedStream desktop"
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|tauri_app| {
            let config_dir = tauri_app
                .path()
                .app_config_dir()
                .expect("app config directory must be resolvable");

            let handle = tauri_app.handle().clone();
            tauri::async_runtime::block_on(async move {
                match app::AppState::new(config_dir).await {
                    Ok(state) => app::init(&handle, Arc::new(state)),
                    Err(e) => tracing::error!(error = %e, "could not initialize app state"),
                }
            });

            // Advertise as soon as the window opens. The product's promise is that the phone
            // finds the PC without anyone typing an IP; making the user press a button first
            // would be friction with nothing behind it. The UI can still stop it.
            let handle = tauri_app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = app::start_advertising_now(&handle).await {
                    tracing::error!(error = %e, "could not start advertising at launch");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::get_status,
            app::start_advertising,
            app::stop_advertising,
            app::respond_pairing,
            app::disconnect,
            app::forget_devices,
            app::start_test_stream,
            app::stop_test_stream,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
