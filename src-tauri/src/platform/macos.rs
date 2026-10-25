//! macOS process queries for window titles (UI-03).

use std::ffi::{c_void, CStr};
use std::mem;
use std::os::fd::RawFd;
use std::path::PathBuf;

/// The foreground process group of the terminal whose master side is `fd`.
pub fn foreground_pgid(fd: RawFd) -> Option<i32> {
    let pgid = unsafe { libc::tcgetpgrp(fd) };
    (pgid > 0).then_some(pgid)
}

/// The executable name of `pid`, or of another member of its process group when the group
/// leader has already exited (e.g. the first command of a pipeline).
pub fn process_name(pid: i32) -> Option<String> {
    name_of(pid).or_else(|| group_member(pid).and_then(name_of))
}

/// The current working directory of `pid`. Fails for processes owned by other users.
pub fn process_cwd(pid: i32) -> Option<PathBuf> {
    cwd_of(pid).or_else(|| group_member(pid).and_then(cwd_of))
}

fn name_of(pid: i32) -> Option<String> {
    let mut buf = [0u8; 256];
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u32) };
    if len <= 0 {
        return None;
    }
    let name = String::from_utf8_lossy(&buf[..len as usize]);
    // Login shells run as `-zsh`.
    Some(name.trim_start_matches('-').to_owned()).filter(|name| !name.is_empty())
}

fn cwd_of(pid: i32) -> Option<PathBuf> {
    let mut info: libc::proc_vnodepathinfo = unsafe { mem::zeroed() };
    let size = mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // `vip_path` is a NUL-terminated MAXPATHLEN buffer split into 32-byte chunks.
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
    let path = path.to_str().ok()?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn group_member(pgid: i32) -> Option<i32> {
    let mut pids = [0 as libc::pid_t; 16];
    let bytes = unsafe {
        libc::proc_listpgrppids(
            pgid,
            pids.as_mut_ptr().cast::<c_void>(),
            mem::size_of_val(&pids) as libc::c_int,
        )
    };
    let count = usize::try_from(bytes).ok()? / mem::size_of::<libc::pid_t>();
    pids[..count.min(pids.len())]
        .iter()
        .copied()
        .find(|&pid| pid > 0 && pid != pgid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_name_and_cwd() {
        let pid = std::process::id() as i32;
        assert!(process_name(pid).is_some());
        assert_eq!(process_cwd(pid), std::env::current_dir().ok());
    }

    #[test]
    fn unknown_process_has_no_name() {
        assert_eq!(process_name(i32::MAX), None);
    }
}
