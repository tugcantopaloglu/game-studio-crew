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
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(windows_impl::CREATE_NO_WINDOW);
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
            let pgid = child.id() as i32;
            self.pgid = Some(pgid);
            reaper::watch(pgid);
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
                reaper::forget(pgid);
                unsafe {
                    libc_kill(-pgid, 9);
                }
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pgid) = self.pgid.take() {
            reaper::forget(pgid);
        }
    }
}

pub fn install_shutdown_handler() {
    #[cfg(not(windows))]
    reaper::install();
}

#[cfg(not(windows))]
mod reaper {
    use std::sync::Mutex;

    static LIVE: Mutex<Vec<i32>> = Mutex::new(Vec::new());

    pub fn watch(pgid: i32) {
        if let Ok(mut live) = LIVE.lock() {
            live.push(pgid);
        }
    }

    pub fn forget(pgid: i32) {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|held| *held != pgid);
        }
    }

    fn kill_every_worker() {
        let held = match LIVE.lock() {
            Ok(mut live) => std::mem::take(&mut *live),
            Err(_) => return,
        };
        for pgid in held {
            unsafe {
                super::libc_kill(-pgid, 9);
            }
        }
    }

    pub fn install() {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

        let Ok(mut signals) = signal_hook::iterator::Signals::new([
            SIGINT, SIGTERM, SIGHUP, SIGQUIT,
        ]) else {
            return;
        };

        std::thread::spawn(move || {
            if let Some(signal) = signals.forever().next() {
                kill_every_worker();
                std::process::exit(128 + signal);
            }
        });
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

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
