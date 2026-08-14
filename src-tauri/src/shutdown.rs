use crate::DshHandle;
use std::time::{Duration, Instant};

pub fn graceful_kill(state: &DshHandle) {
    let mut g = state.child.lock().unwrap();
    let Some(mut d) = g.take() else {
        return;
    };

    // 1) 优雅：CTRL_BREAK → 组根 cmd.exe，组内 node 同步收到 SIGINT → dsh drain ≤5s
    let ok = unsafe {
        use windows::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
        GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, d.pid).is_ok()
    };
    if !ok {
        // 控制台不可达（AllocConsole 未成功/被释放）→ 直接强杀，不空等
        let _ = d.child.kill();
        let _ = d.child.wait();
        return;
    }

    // 2) 轮询退出，最多 6s（dsh drain 上限 5s，留 1s 余量）
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        match d.child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return,
        }
    }

    // 3) 强制兜底
    let _ = d.child.kill();
    let _ = d.child.wait();
}
