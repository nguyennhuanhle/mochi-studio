//! Mochi Studio — Tauri shell.
//! Chỉ đăng ký command/event và quản lý cửa sổ; logic nằm ở các crate `mochi-*`.

mod captions_app;
mod dev_snapshot;
mod editor_app;
mod export_app;
mod jobs_app;
mod recorder_app;
mod settings;
mod single_instance;

use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Event};

fn specta_builder() -> tauri_specta::Builder {
    tauri_specta::Builder::new()
        .commands(collect_commands![
            settings::settings_get,
            settings::settings_set,
            recorder_app::recorder_list_displays,
            recorder_app::recorder_list_windows,
            recorder_app::recorder_list_devices,
            recorder_app::recorder_thumbnail,
            recorder_app::recorder_state,
            recorder_app::recorder_start,
            recorder_app::recorder_pause,
            recorder_app::recorder_resume,
            recorder_app::recorder_stop,
            recorder_app::recorder_cancel,
            recorder_app::recorder_set_mic_muted,
            recorder_app::recorder_discard,
            recorder_app::recovery_list,
            recorder_app::recovery_restore,
            recorder_app::recovery_discard,
            recorder_app::open_privacy_settings,
            recorder_app::reveal_path,
            recorder_app::hotkeys_apply,
            recorder_app::hotkeys_set,
            recorder_app::hotkeys_suspend,
            recorder_app::system_audio_level,
            recorder_app::system_audio_meter_stop,
            recorder_app::region_picker_open,
            recorder_app::region_picker_finish,
            editor_app::project_open,
            editor_app::project_open_backup,
            editor_app::project_create,
            editor_app::project_view,
            editor_app::project_save,
            editor_app::project_close,
            editor_app::projects_unclean,
            editor_app::app_exit,
            editor_app::project_backups,
            editor_app::recent_projects,
            editor_app::media_import,
            editor_app::media_import_duplicate,
            editor_app::media_relink,
            editor_app::media_relink_folder,
            editor_app::media_remove,
            editor_app::media_thumbnail,
            editor_app::media_waveform,
            editor_app::pick_media_files,
            editor_app::pick_folder,
            editor_app::timeline_apply,
            editor_app::timeline_regenerate_zoom,
            editor_app::timeline_undo,
            editor_app::timeline_redo,
            editor_app::preview_attach,
            editor_app::preview_resize,
            editor_app::preview_set_visible,
            editor_app::preview_detach,
            editor_app::player_toggle,
            editor_app::player_pause,
            editor_app::player_seek,
            editor_app::player_step,
            editor_app::player_save_frame,
            editor_app::project_pack,
            export_app::open_releases_page,
            export_app::export_project_file,
            captions_app::captions_models,
            captions_app::captions_cpu_check,
            captions_app::captions_cancel,
            captions_app::captions_download,
            captions_app::captions_generate,
            captions_app::captions_export,
            captions_app::captions_import,
            export_app::app_version,
            editor_app::project_unpack_pick,
            export_app::export_capabilities,
            export_app::export_estimate,
            export_app::export_start,
            export_app::export_cancel,
            export_app::export_pick_path,
            export_app::export_default_dir,
            export_app::export_open_file,
            jobs_app::jobs_status,
            jobs_app::jobs_retry,
            jobs_app::preview_use_proxies,
            jobs_app::cache_set_limit,
            jobs_app::cache_info,
            jobs_app::cache_clear,
            jobs_app::system_gpu_check,
            dev_snapshot::dev_snapshot_enabled,
            dev_snapshot::dev_snapshot_mode,
            dev_snapshot::dev_snapshot,
            dev_snapshot::dev_snapshot_finish,
            dev_snapshot::dev_snapshot_close_main,
        ])
        .events(collect_events![
            recorder_app::RecorderEventMsg,
            recorder_app::RegionSelected,
            editor_app::PlayerTime,
            editor_app::PlayerStats,
            editor_app::AutosaveStatus,
            editor_app::CloseSaveFailed,
            editor_app::PackProgress,
            captions_app::CaptionsProgress,
            export_app::ExportEvent,
            jobs_app::JobsUpdate,
            jobs_app::CacheTrimmed
        ])
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // K-R1: instance thứ hai không chạy tiếp (không có RecorderService riêng).
    let Some(_instance) = single_instance::acquire() else {
        return;
    };

    // Tauri dùng chung runtime stack-lớn của main.rs thay vì tự tạo runtime riêng.
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    let builder = specta_builder();

    // Sinh type TS dùng chung cho UI (chỉ ở bản dev, khi chạy từ source).
    #[cfg(debug_assertions)]
    {
        let path = std::path::Path::new("../src/bindings.ts");
        if path.parent().is_some_and(|p| p.exists()) {
            if let Err(e) = builder.export(Typescript::default().header("// @ts-nocheck"), path) {
                tracing::warn!("Không xuất được bindings.ts: {e}");
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .manage(recorder_app::RecorderAppState::new())
        .manage(editor_app::EditorAppState::default())
        .manage(export_app::ExportAppState::default())
        .manage(jobs_app::JobsAppState::default())
        .setup(move |app| {
            builder.mount_events(app);
            app.manage(settings::SettingsState::load(app.handle())?);
            recorder_app::setup(app.handle());
            editor_app::setup(app.handle());
            export_app::setup();
            jobs_app::setup(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == recorder_app::MAIN
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
                && let Err(error) = editor_app::save_on_exit(window.app_handle())
            {
                // S01: không lưu được → không thoát; UI hỏi thử lại / bỏ thay đổi (`app_exit`).
                api.prevent_close();
                editor_app::CloseSaveFailed { error }.emit(window.app_handle()).ok();
            }
        })
        .run(tauri::generate_context!())
        .expect("không khởi động được Mochi Studio");
}
