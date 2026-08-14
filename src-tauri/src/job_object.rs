use std::mem::size_of;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOBOBJECTINFOCLASS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub struct JobObject {
    handle: HANDLE,
}

impl JobObject {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            let handle = CreateJobObjectW(None, PCWSTR::null())?;
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(Self { handle })
        }
    }

    pub fn assign_pid(&self, pid: u32) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false.into(), pid)?;
            AssignProcessToJobObject(self.handle, h)?;
            let _ = CloseHandle(h);
        }
        Ok(())
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

// 防止 JobObjectInfoClass 类型在某些版本中未被推导
const _CLASS: JOBOBJECTINFOCLASS = JobObjectExtendedLimitInformation;

unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}
