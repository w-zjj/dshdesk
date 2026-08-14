mod console;
mod dsh;
mod job_object;
mod log_writer;
mod port;
mod shutdown;
mod update_checker;

use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

pub struct DshHandle {
    pub child: Mutex<Option<dsh::DshChild>>,
    pub job: Mutex<Option<job_object::JobObject>>,
}

fn build_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let about = MenuItem::with_id(app, "about", "关于", true, None::<&str>)?;
    let check_update = MenuItem::with_id(app, "check_update", "检查 DSH 更新", true, None::<&str>)?;
    let open_data = MenuItem::with_id(app, "open_data", "打开数据目录", true, None::<&str>)?;
    let submenu = Submenu::with_items(app, "帮助", true, &[&about, &check_update, &open_data])?;
    let menu = Menu::with_items(app, &[&submenu])?;
    app.set_menu(menu)?;
    Ok(())
}

fn eval_main(win: Option<&WebviewWindow>, js: &str) {
    if let Some(w) = win {
        let _ = w.eval(js);
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    console::ensure_hidden_console();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(DshHandle {
            child: Mutex::new(None),
            job: Mutex::new(None),
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "about" => {
                app.dialog()
                    .message(format!(
                        "Dshdesk v{}\n内置 DSH: {}\n内置 Node: {}",
                        env!("CARGO_PKG_VERSION"),
                        dsh::DSH_PINNED_VERSION,
                        dsh::NODE_PINNED_VERSION
                    ))
                    .title("关于")
                    .show(|_| {});
            }
            "check_update" => {
                let _ = std::process::Command::new("explorer")
                    .arg("https://www.npmjs.com/package/@deepseek-ai/dsh")
                    .spawn();
            }
            "open_data" => {
                let home = dsh::resolve_dsh_home();
                let _ = std::fs::create_dir_all(&home);
                let _ = std::process::Command::new("explorer").arg(home).spawn();
            }
            _ => {}
        })
        .setup(|app| {
            if let Err(e) = build_menu(app) {
                eprintln!("warn: set_menu failed: {}", e);
            }

            // 后台检查更新：GitHub Releases API，有新版弹窗提示
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let local_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
                if let Some(info) = update_checker::check_latest() {
                    if update_checker::is_newer(&info.tag, &local_tag) {
                        let url = info.html_url.clone();
                        let tag = info.tag.clone();
                        let body = if info.body.is_empty() {
                            String::new()
                        } else {
                            format!("\n\n{}", info.body)
                        };
                        app_handle
                            .dialog()
                            .message(format!("发现新版本 {}{}，点击确定前往下载", tag, body))
                            .title("发现新版本")
                            .show(move |confirmed| {
                                if confirmed {
                                    let _ = std::process::Command::new("explorer")
                                        .arg(&url)
                                        .spawn();
                                }
                            });
                    }
                }
            });

            let win = app.get_webview_window("main");
            match dsh::boot(app.handle()) {
                Ok(()) => {}
                Err(dsh::BootError::NodeMissing(p)) => {
                    let m = format!("内置 Node 缺失（打包遗漏）：{}", p.display());
                    eval_main(
                        win.as_ref(),
                        &format!("window.__showBootError({})", json_str(&m)),
                    );
                }
                Err(dsh::BootError::BundleMissing(p)) => {
                    let m = format!("DSH bundle 缺失：{}", p.display());
                    eval_main(
                        win.as_ref(),
                        &format!("window.__showBootError({})", json_str(&m)),
                    );
                }
                Err(dsh::BootError::Spawn(msg)) => {
                    eval_main(
                        win.as_ref(),
                        &format!("window.__showBootError({})", json_str(&msg)),
                    );
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                if let Some(state) = app.try_state::<DshHandle>() {
                    shutdown::graceful_kill(&state);
                }
            }
            _ => {}
        });
}
