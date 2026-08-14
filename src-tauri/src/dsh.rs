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
pub fn resolve_dsh_home() -> PathBuf {
    if let Ok(p) = std::env::var("DSH_HOME") {
        return PathBuf::from(p);
    }
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata)
        .join("DeepSeekHarness")
        .join("dsh-home")
}

// 启动失败时的错误类型，供 splash 显示对应提示。
pub enum BootError {
    NodeMissing(PathBuf),
    BundleMissing(PathBuf),
    Spawn(String),
}

pub fn boot(app: &AppHandle) -> Result<(), BootError> {
    // dev 模式下 resource_dir() 返回 target/debug，但 Tauri 不会把 resources 复制过去；
    // release 打包后 resource_dir() 指向安装目录的资源目录，能正常找到。
    // 回退到 CARGO_MANIFEST_DIR/resources 覆盖 dev 场景。
    let resource_dir = app
        .path()
        .resource_dir()
        .ok()
        .filter(|p| p.join("node-portable").join("node.exe").exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));

    // 便携 Node：resources/node-portable/node.exe
    let node_exe = resource_dir.join("node-portable").join("node.exe");
    if !node_exe.exists() {
        return Err(BootError::NodeMissing(node_exe));
    }

    // dsh bundle：resources/dsh-bundle/node_modules/@deepseek-ai/dsh/lib/bin.js
    let dsh_bin = resource_dir
        .join("dsh-bundle")
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    if !dsh_bin.exists() {
        return Err(BootError::BundleMissing(dsh_bin));
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

    let job = JobObject::new().map_err(|e| BootError::Spawn(format!("job: {}", e)))?;

    let mut cmd = Command::new(&node_exe);
    cmd.arg(&dsh_bin)
        .arg("web")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
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
        Err(e) => return Err(BootError::Spawn(format!("spawn node: {}", e))),
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
    if let Some(state) = app.try_state::<crate::DshHandle>() {
        *state.child.lock().unwrap() = Some(dsh_child);
        *state.job.lock().unwrap() = Some(job);
    }

    let app2 = app.clone();
    std::thread::spawn(move || {
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
                let _ = w.eval(&format!("window.location.replace('{}')", url));
            } else {
                let lp = log_path.display().to_string().replace('\\', "/");
                let _ = w.eval(&format!(
                    "window.__showBootError('dsh 启动超时（45s）。日志：{}')",
                    lp
                ));
            }
        }
    });

    Ok(())
}
