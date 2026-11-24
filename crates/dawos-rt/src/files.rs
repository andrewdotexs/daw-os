//! Files of the kernel that are read without ever waiting for them.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Open `path` for reading so that a read with nothing to give returns
/// at once (`io::ErrorKind::WouldBlock`) instead of sleeping. For the
/// streams the kernel serves under `/sys/kernel/tracing`, read by a
/// thread that has other things to do.
pub fn open_without_waiting(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}
