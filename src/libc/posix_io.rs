pub mod stat;

use crate::abi::DotDotDot;
use crate::fs::{GuestFile, GuestOpenOptions, GuestPath};
use crate::libc::sys::socket::close_socket;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestISize, GuestUSize, MutVoidPtr};
use crate::Environment;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Default)]
pub struct State {
    /// File descriptors _other than stdin, stdout, and stderr_
    files: Vec<Option<PosixFileHostObject>>,
}
impl State {
    fn file_for_fd(&mut self, fd: FileDescriptor) -> Option<&mut PosixFileHostObject> {
        self.files
            .get_mut(fd_to_file_idx(fd))
            .and_then(|file_or_none| file_or_none.as_mut())
    }
}

struct PosixFileHostObject {
    file: GuestFile,
    needs_flush: bool,
    reached_eof: bool,
    flags: i32,
}

// TODO: stdin/stdout/stderr handling somehow
fn file_idx_to_fd(idx: usize) -> FileDescriptor {
    FileDescriptor::try_from(idx)
        .unwrap()
        .checked_add(NORMAL_FILENO_BASE)
        .unwrap()
}
fn fd_to_file_idx(fd: FileDescriptor) -> usize {
    fd.checked_sub(NORMAL_FILENO_BASE).unwrap() as usize
}

/// File descriptor type. This alias is for readability, POSIX just uses `int`.
pub type FileDescriptor = i32;
#[allow(dead_code)]
pub const STDIN_FILENO: FileDescriptor = 0;
#[allow(dead_code)]
pub const STDOUT_FILENO: FileDescriptor = 1;
pub const STDERR_FILENO: FileDescriptor = 2;
const NORMAL_FILENO_BASE: FileDescriptor = STDERR_FILENO + 1;

/// Flags bitfield for `open`. This alias is for readability, POSIX just uses
/// `int`.
pub type OpenFlag = i32;
pub const O_RDONLY: OpenFlag = 0x0;
pub const O_WRONLY: OpenFlag = 0x1;
pub const O_RDWR: OpenFlag = 0x2;
pub const O_ACCMODE: OpenFlag = O_RDWR | O_WRONLY | O_RDONLY;

pub const O_NONBLOCK: OpenFlag = 0x4;
pub const O_APPEND: OpenFlag = 0x8;
pub const O_NOFOLLOW: OpenFlag = 0x100;
pub const O_CREAT: OpenFlag = 0x200;
pub const O_TRUNC: OpenFlag = 0x400;
pub const O_EXCL: OpenFlag = 0x800;

/// File control command flags.
/// This alias is for readability, POSIX just uses `int`.
pub type FileControlCommand = i32;
const F_GETFD: FileControlCommand = 1;
const F_SETFD: FileControlCommand = 2;
const F_GETFL: FileControlCommand = 3;
const F_SETFL: FileControlCommand = 4;

/// File Descriptor flags.
/// This alias is for readability, POSIX just uses `int`.
pub type FDFlag = i32;
pub const FD_CLOEXEC: FDFlag = 1;

pub(crate) fn open(
    env: &mut Environment,
    path: ConstPtr<u8>,
    flags: i32,
    _args: DotDotDot,
) -> FileDescriptor {
    self::open_direct(env, path, flags)
}
pub(crate) fn open_direct(env: &mut Environment, path: ConstPtr<u8>, flags: i32) -> FileDescriptor {
    assert!([O_RDONLY, O_WRONLY, O_RDWR].contains(&(flags & O_ACCMODE)));
    // TODO: support more flags, this list is not complete
    assert!(
        flags & !(O_ACCMODE | O_NONBLOCK | O_APPEND | O_NOFOLLOW | O_CREAT | O_TRUNC | O_EXCL) == 0
    );
    // TODO: symlinks don't exist in the FS yet, so we can't "not follow" them.
    // (Should we just ignore this?)
    assert!(flags & O_NOFOLLOW == 0);
    // TODO: exclusive mode not implemented yet
    assert!(flags & O_EXCL == 0);

    // TODO: respect the mode (in the variadic arguments) when creating a file
    // Note: NONBLOCK flag is ignored, assumption is all file I/O is fast
    let mut options = GuestOpenOptions::new();
    match flags & O_ACCMODE {
        O_RDONLY => options.read(),
        O_WRONLY => options.write(),
        O_RDWR => options.read().write(),
        _ => panic!(),
    };
    if (flags & O_APPEND) != 0 {
        options.append();
    }
    if (flags & O_CREAT) != 0 {
        options.create();
    }
    if (flags & O_TRUNC) != 0 {
        options.truncate();
    }

    match env.fs.open_with_options(
        GuestPath::new(&env.mem.cstr_at_utf8(path).unwrap()),
        options,
    ) {
        Ok(file) => {
            let host_object = PosixFileHostObject {
                file,
                needs_flush: false,
                reached_eof: false,
                flags: flags & (O_ACCMODE | O_NONBLOCK | O_APPEND),
            };

            let idx = if let Some(free_idx) = env
                .libc_state
                .posix_io
                .files
                .iter()
                .position(|f| f.is_none())
            {
                env.libc_state.posix_io.files[free_idx] = Some(host_object);
                free_idx
            } else {
                let idx = env.libc_state.posix_io.files.len();
                env.libc_state.posix_io.files.push(Some(host_object));
                idx
            };
            let fd = file_idx_to_fd(idx);
            log_dbg!("open({:?}, {:#x}) => {:?}", path, flags, fd);
            fd
        }
        Err(()) => {
            // TODO: set errno
            log!(
                "Warning: open({:?}, {:#x}) failed, returning -1",
                path,
                flags,
            );
            -1
        }
    }
}

pub(crate) fn read(
    env: &mut Environment,
    fd: FileDescriptor,
    buffer: MutVoidPtr,
    size: GuestUSize,
) -> GuestISize {
    // TODO: error handling for unknown fd?
    let file = env.libc_state.posix_io.file_for_fd(fd).unwrap();

    let buffer_slice = env.mem.bytes_at_mut(buffer.cast(), size);
    // TODO: handle errors
    match file.file.read(buffer_slice) {
        Ok(bytes_read) => {
            if bytes_read < buffer_slice.len() {
                log!(
                    "Warning: read({:?}, {:?}, {:#x}) read only {:#x} bytes",
                    fd,
                    buffer,
                    size,
                    bytes_read,
                );
            } else {
                log_dbg!(
                    "read({:?}, {:?}, {:#x}) => {:#x}",
                    fd,
                    buffer,
                    size,
                    bytes_read,
                );
            }
            bytes_read.try_into().unwrap()
        }
        Err(e) => {
            // TODO: set errno
            log!(
                "Warning: read({:?}, {:?}, {:#x}) encountered error {:?}, returning -1",
                fd,
                buffer,
                size,
                e,
            );
            -1
        }
    }
}

pub(crate) fn write(
    env: &mut Environment,
    fd: FileDescriptor,
    buffer: ConstVoidPtr,
    size: GuestUSize,
) -> GuestISize {
    // TODO: error handling for unknown fd?
    let file = env.libc_state.posix_io.file_for_fd(fd).unwrap();

    let buffer_slice = env.mem.bytes_at(buffer.cast(), size);
    match file.file.write(buffer_slice) {
        Ok(bytes_written) => {
            if bytes_written < buffer_slice.len() {
                log!(
                    "Warning: write({:?}, {:?}, {:#x}) wrote only {:#x} bytes",
                    fd,
                    buffer,
                    size,
                    bytes_written,
                );
            } else {
                log_dbg!(
                    "write({:?}, {:?}, {:#x}) => {:#x}",
                    fd,
                    buffer,
                    size,
                    bytes_written,
                );
            }
            bytes_written.try_into().unwrap()
        }
        Err(e) => {
            // TODO: set errno
            log!(
                "Warning: write({:?}, {:?}, {:#x}) encountered error {:?}, returning -1",
                fd,
                buffer,
                size,
                e,
            );
            -1
        }
    }
}

#[allow(non_camel_case_types)]
pub type off_t = i64;
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

pub(crate) fn lseek(
    env: &mut Environment,
    fd: FileDescriptor,
    offset: off_t,
    whence: i32,
) -> off_t {
    // TODO: error handling for unknown fd?
    let file = env.libc_state.posix_io.file_for_fd(fd).unwrap();

    let from = match whence {
        // not sure whether offset is treated as signed or unsigned when using
        // SEEK_SET, so `.try_into()` seems safer.
        SEEK_SET => SeekFrom::Start(offset.try_into().unwrap()),
        SEEK_CUR => SeekFrom::Current(offset),
        SEEK_END => SeekFrom::End(offset),
        _ => panic!("Unsupported \"whence\" parameter to seek(): {}", whence),
    };

    let res = match file.file.seek(from) {
        Ok(new_offset) => new_offset.try_into().unwrap(),
        // TODO: set errno
        Err(_) => -1,
    };
    log_dbg!("lseek({:?}, {:#x}, {}) => {}", fd, offset, whence, res);
    res
}

pub(crate) fn close(env: &mut Environment, fd: FileDescriptor) -> i32 {
    // TODO: error handling for unknown fd?
    let file = env.libc_state.posix_io.files[fd_to_file_idx(fd)]
        .take()
        .unwrap();
    // The actual closing of the file happens implicitly when `file` falls out
    // of scope. The return value is about whether flushing succeeds.
    match file.file {
        GuestFile::Directory => 0,
        GuestFile::Socket => {
            close_socket(env, fd);
            0
        }
        _ => {
            match file.file.sync_all() {
                Ok(()) => {
                    log_dbg!("close({:?}) => 0", fd);
                    0
                }
                Err(_) => {
                    // TODO: set errno
                    log!("Warning: close({:?}) failed, returning -1", fd);
                    -1
                }
            }
        }
    }
}

pub(crate) fn rename(env: &mut Environment, old: ConstPtr<u8>, new: ConstPtr<u8>) -> i32 {
    // TODO: set errno
    let old = env.mem.cstr_at_utf8(old).unwrap();
    let new = env.mem.cstr_at_utf8(new).unwrap();
    let res = match env.fs.rename(GuestPath::new(&old), GuestPath::new(&new)) {
        Ok(_) => 0,
        Err(_) => -1,
    };
    log_dbg!("rename('{}', '{}') => {}", old, new, res);
    res
}

fn fcntl(
    env: &mut Environment,
    fd: FileDescriptor,
    cmd: FileControlCommand,
    args: DotDotDot,
) -> i32 {
    if fd >= NORMAL_FILENO_BASE
        && env
            .libc_state
            .posix_io
            .files
            .get(fd_to_file_idx(fd))
            .is_none()
    {
        return -1;
    }

    match cmd {
        F_GETFD => return 0,
        F_SETFD => {
            let flags: i32 = args.start().next(env);
            assert!(matches!(flags, FD_CLOEXEC | 0));
            if flags & FD_CLOEXEC == FD_CLOEXEC {
                log!(
                    "TODO: fcntl({}, F_SETFD, {}) called. CLOEXEC currently not supported.",
                    fd,
                    flags
                );
            }
        }
        F_GETFL => {
            let file = env.libc_state.posix_io.file_for_fd(fd).unwrap();
            return file.flags;
        }
        F_SETFL => {
            let flags: i32 = args.start().next(env);
            let file = env.libc_state.posix_io.file_for_fd(fd).unwrap();
            let access_mode = file.flags & O_ACCMODE;
            file.flags = access_mode | (flags & (O_NONBLOCK | O_APPEND));
        }
        _ => unimplemented!(),
    }
    0
}

fn find_or_create_fd(env: &mut Environment, host_object: PosixFileHostObject) -> FileDescriptor {
    let idx = if let Some(free_idx) = env
        .libc_state
        .posix_io
        .files
        .iter()
        .position(|f| f.is_none())
    {
        env.libc_state.posix_io.files[free_idx] = Some(host_object);
        free_idx
    } else {
        let idx = env.libc_state.posix_io.files.len();
        env.libc_state.posix_io.files.push(Some(host_object));
        idx
    };
    file_idx_to_fd(idx)
}

pub fn find_or_create_socket(env: &mut Environment) -> FileDescriptor {
    let host_object = PosixFileHostObject {
        file: GuestFile::Socket,
        needs_flush: false,
        reached_eof: false,
        flags: O_RDWR,
    };
    find_or_create_fd(env, host_object)
}

pub fn is_socket(env: &mut Environment, fd: FileDescriptor) -> bool {
    let guest_file = &env
        .libc_state
        .posix_io
        .files
        .get(fd_to_file_idx(fd))
        .unwrap()
        .as_ref()
        .unwrap()
        .file;
    matches!(guest_file, GuestFile::Socket)
}
