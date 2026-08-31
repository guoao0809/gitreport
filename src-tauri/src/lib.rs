//! Tauri 应用入口：command 注册、插件、托盘、窗口关闭隐藏。

mod ai;
mod git;
mod secrets;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 二次启动时唤起已有主窗口
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            git::check_git,
            git::detect_git_repos,
            git::get_git_identity,
            git::get_git_branches,
            git::fetch_commits,
            ai::generate_report,
            ai::generate_report_stream,
            ai::test_ai_connection,
            ai::fetch_models,
            secrets::save_api_key,
            secrets::load_api_key,
            secrets::delete_api_key
        ])
        .setup(|app| {
            // 检测 git 是否可用（失败仅记日志，不阻断启动）
            match std::process::Command::new("git").arg("--version").output() {
                Ok(out) if out.status.success() => {
                    log::info!("git 可用：{}", String::from_utf8_lossy(&out.stdout).trim());
                }
                _ => log::warn!("未检测到可用的 git 命令，仓库扫描与提交拉取将不可用"),
            }

            // 托盘：显示窗口 / 退出
            let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 左键点击显示窗口
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭按钮 → 隐藏到托盘
            if let WindowEvent::CloseRequested { api, .. } = event {
                window.hide().ok();
                api.prevent_close();
            }
        });

    // debug 构建启用日志输出到终端
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_log::Builder::new().build());

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
