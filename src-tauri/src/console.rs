// release 构建是 windows 子系统（无控制台）。GenerateConsoleCtrlEvent 要求调用方拥有
// 控制台，否则返回 ERROR_INVALID_HANDLE。这里 AllocConsole 并隐藏，保证 CTRL_BREAK
// 能送达 dsh 进程组。debug 构建自带控制台，跳过。
#[cfg(not(debug_assertions))]
pub fn ensure_hidden_console() {
    unsafe {
        use windows::Win32::System::Console::{
            AllocConsole, GetConsoleWindow, SetConsoleCtrlHandler,
        };
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

        let _ = AllocConsole();
        let hwnd = GetConsoleWindow();
        let _ = ShowWindow(hwnd, SW_HIDE);
        // 让本进程忽略 CTRL 信号，避免 Tauri 自身被误杀；不影响投递给子进程组
        let _ = SetConsoleCtrlHandler(None, true.into());
    }
}

#[cfg(debug_assertions)]
pub fn ensure_hidden_console() {}
