use std::io;
use std::process::{Child, Command};

pub struct ProcessGroup {
    #[cfg(windows)]
    job: windows_impl::Job,
    #[cfg(not(windows))]
    pgid: Option<i32>,
}

impl ProcessGroup {
    pub fn new() -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self { job: windows_impl::Job::new()? })
        }
        #[cfg(not(windows))]
        {
            Ok(Self { pgid: None })
        }
    }

    pub fn prepare(&self, cmd: &mut Command) {
        #[cfg(windows)]
        {
            let _ = cmd;
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
    }

    pub fn adopt(&mut self, child: &Child) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.job.assign(child)
        }
        #[cfg(not(windows))]
        {
            self.pgid = Some(child.id() as i32);
            Ok(())
        }
    }

    pub fn kill_tree(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.job.terminate()
        }
        #[cfg(not(windows))]
        {
            if let Some(pgid) = self.pgid.take() {
                unsafe {
                    libc_kill(-pgid, 9);
                }
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JobObjectExtendedLimitInformation,
    };

    pub struct Job {
        handle: HANDLE,
    }

    unsafe impl Send for Job {}

    impl Job {
        pub fn new() -> io::Result<Self> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }

                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

                let ok = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    let err = io::Error::last_os_error();
                    CloseHandle(handle);
                    return Err(err);
                }

                Ok(Self { handle })
            }
        }

        pub fn assign(&self, child: &Child) -> io::Result<()> {
            unsafe {
                let ok = AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE);
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        pub fn terminate(&self) -> io::Result<()> {
            unsafe {
                if TerminateJobObject(self.handle, 1) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}
