//! Windows Job Object 封装：KILL_ON_JOB_CLOSE —— 壳进程无论怎么退出，内核整棵进程树自动回收。
//! 这是「退出后任务管理器无残留 node」的兜底保证（ARCHITECTURE.md §3.2/§10.5）。
#![cfg(windows)]
use std::io;
use std::mem::MaybeUninit;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub struct JobObject(HANDLE);

impl JobObject {
    pub fn new() -> io::Result<Self> {
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = MaybeUninit::zeroed().assume_init();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(h);
                return Err(io::Error::last_os_error());
            }
            Ok(JobObject(h))
        }
    }

    /// 把子进程挂入 Job（子进程须刚 spawn、仍存活）
    pub fn assign_child(&self, child: &std::process::Child) -> io::Result<()> {
        let handle = child.as_raw_handle();
        let ok = unsafe { AssignProcessToJobObject(self.0, handle) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 树级硬杀（超时兜底）
    pub fn terminate(&self) -> io::Result<()> {
        let ok = unsafe { TerminateJobObject(self.0, 1) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE：关闭句柄 = 整树回收（若仍有存活进程）
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Windows 原生错误对话框（无额外依赖）
pub fn show_error(title: &str, msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let t = wide(title);
    let m = wide(msg);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
    }
}
/// Windows 信息对话框
pub fn show_info(title: &str, msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let t = wide(title);
    let m = wide(msg);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// 确认对话框：返回 true=用户点击「是」
pub fn show_question(title: &str, msg: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONQUESTION, MB_YESNO};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let t = wide(title);
    let m = wide(msg);
    unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_YESNO | MB_ICONQUESTION) == 6 }
}
