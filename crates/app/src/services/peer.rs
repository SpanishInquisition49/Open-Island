//! Who is at the other end of a local socket, as the kernel knows it.

use std::os::fd::{AsRawFd, BorrowedFd};

/// The user id of the process at the other end. None when the kernel does not say.
#[cfg(target_os = "linux")]
pub fn uid(socket: BorrowedFd<'_>) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: u32::MAX };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is open for the call; `cred` and `len` describe a buffer of the right size
    let r = unsafe { libc::getsockopt(socket.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, std::ptr::addr_of_mut!(cred).cast(), &mut len) };
    (r == 0 && len as usize == std::mem::size_of::<libc::ucred>()).then_some(cred.uid)
}

#[cfg(not(target_os = "linux"))]
pub fn uid(socket: BorrowedFd<'_>) -> Option<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the descriptor is open for the call, and both pointers are to values of the types asked for
    let r = unsafe { libc::getpeereid(socket.as_raw_fd(), &mut uid, &mut gid) };
    (r == 0).then_some(uid)
}

/// The user this process acts as.
pub fn own() -> u32 {
    // SAFETY: geteuid cannot fail
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn the_other_end_of_a_pair_is_this_user() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(uid(a.as_fd()), Some(own()));
        let file = std::fs::File::open("/dev/null").unwrap();
        assert_eq!(uid(file.as_fd()), None, "what is no socket has no other end");
    }
}
