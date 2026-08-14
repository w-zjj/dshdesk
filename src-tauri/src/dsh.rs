use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::job_object::JobObject;
use crate::log_writer::spawn_log_pump;
use crate::port::pick_free_port_127;

// CREATE_NEW_PROCESS_GROUP：让子进程成组根，CTRL_BREAK 可定向投递
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

// 锁定的 DSH 版本。升级 DSH 时改这里 + 跑 scripts/fetch-dsh.ps1 -DshVersion <新版本>
pub const DSH_PINNED_VERSION: &str = "0.1.0-rc.6";
// 锁定的 Node 版本。升级时改这里 + 跑 scripts/fetch-dsh.ps1 -NodeVersion <新版本>
pub const NODE_PINNED_VERSION: &str = "v24.16.0";

pub struct DshChild {
    pub child: Child,
    pub pid: u32,
    pub port: u16,
    pub log_path: PathBuf,
}

// DSH_HOME 指向用户可写目录。这是插件生态不受影响的关键：
// profile（含用户装的插件）初始化到 $DSH_HOME/profiles/web，与只读的 bundle 隔离；
// dsh 解析插件时先查 bundle 内置、再查 profile 的 node_modules，互不冲突。
// 同时 zip 解压也落到这里，避免写只读的安装目录。
pub fn resolve_dsh_home() -> PathBuf {
    if let Ok(p) = std::env::var("DSH_HOME") {
        return PathBuf::from(p);
    }
    // APPDATA 在正常 Windows 用户会话下一定存在（Roaming 目录）。
    // 回退到 LOCALAPPDATA（有些精简环境只有这个），再不行用可执行文件同级的 dsh-home。
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."))
        });
    base.join("DeepSeekHarness").join("dsh-home")
}

// 按修改时间降序排序，保留最新 N 个，删除其余 dsh-*.log
fn cleanup_old_logs(dsh_home: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dsh_home) else {
        return;
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("dsh-") || !name.ends_with(".log") {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, path))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in logs.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

// 启动失败时的错误类型，供 splash 显示对应提示。
pub enum BootError {
    NodeMissing(PathBuf),
    BundleMissing(PathBuf),
    Spawn(String),
}

// 解压 zip 到 dest（dest 必须已创建）。用 enclosed_name 防止路径穿越。
fn extract_zip(zip_path: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("open {}: {}", zip_path.display(), e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("read zip {}: {}", zip_path.display(), e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("entry {}: {}", i, e))?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let outpath = dest.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("mkdir {}: {}", outpath.display(), e))?;
        } else {
            if let Some(p) = outpath.parent() {
                std::fs::create_dir_all(p).map_err(|e| format!("mkdir {}: {}", p.display(), e))?;
            }
            let mut out = File::create(&outpath)
                .map_err(|e| format!("create {}: {}", outpath.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("copy {}: {}", outpath.display(), e))?;
        }
    }
    Ok(())
}

// 首启或版本变更时，把 dsh-bundle.zip / node-portable.zip 解压到
// dsh_home/bundle、dsh_home/node。用 .bundle.ver 做版本标记避免重复解压。
fn ensure_extracted(
    dsh_home: &std::path::Path,
    zip_bundle: &std::path::Path,
    zip_node: &std::path::Path,
) -> Result<(), String> {
    let bundle_dir = dsh_home.join("bundle");
    let node_dir = dsh_home.join("node");
    let ver_file = dsh_home.join(".bundle.ver");
    let current_ver = format!("{}|{}", DSH_PINNED_VERSION, NODE_PINNED_VERSION);

    let bin_ok = bundle_dir
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js")
        .exists();
    let node_ok = node_dir.join("node.exe").exists();
    let ver_ok = std::fs::read_to_string(&ver_file)
        .map(|s| s.trim() == current_ver)
        .unwrap_or(false);

    if bin_ok && node_ok && ver_ok {
        return Ok(());
    }

    // 清理旧的（损坏或版本过期）
    let _ = std::fs::remove_dir_all(&bundle_dir);
    let _ = std::fs::remove_dir_all(&node_dir);
    std::fs::create_dir_all(&bundle_dir).map_err(|e| format!("mkdir bundle: {}", e))?;
    std::fs::create_dir_all(&node_dir).map_err(|e| format!("mkdir node: {}", e))?;

    extract_zip(zip_node, &node_dir)?;
    extract_zip(zip_bundle, &bundle_dir)?;

    let _ = std::fs::write(&ver_file, &current_ver);
    Ok(())
}

fn eval_js(app: &AppHandle, js: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.eval(js);
    }
}

fn eval_status(app: &AppHandle, msg: &str) {
    let s = serde_json::to_string(msg).unwrap_or_else(|_| "\"\"".into());
    eval_js(app, &format!("window.__setBootStatus({})", s));
}

fn eval_error(app: &AppHandle, msg: &str) {
    let s = serde_json::to_string(msg).unwrap_or_else(|_| "\"\"".into());
    eval_js(app, &format!("window.__showBootError({})", s));
}

pub fn boot(app: &AppHandle) -> Result<(), BootError> {
    // Tauri 2 打包规则：tauri.conf.json 里配 "resources/*.zip"，
    // 安装后放在 $RESOURCE/resources/（保留原始目录结构）。
    // dev 模式 resource_dir() 返回 target/debug（无 resources 子目录），
    // 回退到源码目录的 resources/。
    let resource_dir = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| BootError::Spawn(format!("resource_dir: {}", e)))?
            .join("resources")
    };

    // 资源是两个 zip（fetch-dsh.ps1 产出），首启解压到 dsh_home，避免 NSIS 打包几万散文件
    let zip_bundle = resource_dir.join("dsh-bundle.zip");
    let zip_node = resource_dir.join("node-portable.zip");
    if !zip_node.exists() {
        return Err(BootError::NodeMissing(zip_node));
    }
    if !zip_bundle.exists() {
        return Err(BootError::BundleMissing(zip_bundle));
    }

    let port = match pick_free_port_127() {
        Ok(p) => p,
        Err(e) => return Err(BootError::Spawn(format!("pick port: {}", e))),
    };
    let dsh_home = resolve_dsh_home();
    if let Err(e) = std::fs::create_dir_all(&dsh_home) {
        return Err(BootError::Spawn(format!("create dsh_home: {}", e)));
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let log_path = dsh_home.join(format!("dsh-{}.log", ts));

    // 保留最新 30 个日志，旧的自动清理，避免长期堆积
    cleanup_old_logs(&dsh_home, 30);

    // 解压 + spawn 放后台线程，避免 setup 阻塞导致窗口延迟出现
    let app2 = app.clone();
    std::thread::spawn(move || {
        // 1. 首启解压（耗时操作，放后台）
        eval_status(&app2, "正在准备运行环境…");
        if let Err(e) = ensure_extracted(&dsh_home, &zip_bundle, &zip_node) {
            eval_error(&app2, &format!("解压运行环境失败：{}", e));
            return;
        }

        // 2. 启动 DSH
        eval_status(&app2, "正在启动 DeepSeek Harness…");
        let node_exe = dsh_home.join("node").join("node.exe");
        let dsh_bin = dsh_home
            .join("bundle")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");

        let job = match JobObject::new() {
            Ok(j) => j,
            Err(e) => {
                eval_error(&app2, &format!("创建 Job Object 失败：{}", e));
                return;
            }
        };

        let mut cmd = Command::new(&node_exe);
        cmd.arg(&dsh_bin)
            .arg("web")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .current_dir(&dsh_home)
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("DSH_HOME", &dsh_home)
            .env("NODE_USE_ENV_PROXY", "1");
        if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
            cmd.env("DEEPSEEK_API_KEY", k);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eval_error(&app2, &format!("启动 Node 失败：{}", e));
                return;
            }
        };
        let pid = child.id();
        if let Err(e) = job.assign_pid(pid) {
            eprintln!("warn: AssignProcessToJobObject failed: {}", e);
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        spawn_log_pump(stdout, stderr, log_path.clone());

        let dsh_child = DshChild {
            child,
            pid,
            port,
            log_path: log_path.clone(),
        };
        if let Some(state) = app2.try_state::<crate::DshHandle>() {
            *state.child.lock().unwrap() = Some(dsh_child);
            *state.job.lock().unwrap() = Some(job);
        }

        // 3. 轮询端口，就绪后跳转到 DSH WebUI
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut ready = false;
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
                std::thread::sleep(Duration::from_millis(300));
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }

        if let Some(w) = app2.get_webview_window("main") {
            if ready {
                let url = format!("http://127.0.0.1:{}/", port);
                let _ = w.eval(&format!(
                    "window.location.replace({})",
                    serde_json::to_string(&url).unwrap_or_default()
                ));
            } else {
                let msg = format!("dsh 启动超时（45s）。日志：{}", log_path.display());
                eval_error(&app2, &msg);
            }
        }
    });

    Ok(())
}
