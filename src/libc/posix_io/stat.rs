use crate::fs::GuestPath;
use crate::mem::ConstPtr;
use crate::Environment;

#[allow(non_camel_case_types)]
pub type mode_t = u16;

pub(crate) fn mkdir(env: &mut Environment, path: ConstPtr<u8>, mode: mode_t) -> i32 {
    let path_str = env.mem.cstr_at_utf8(path).unwrap();

    // TODO: respect the mode
    match env.fs.create_dir(GuestPath::new(&path_str)) {
        Ok(()) => {
            log_dbg!("mkdir({:?} {:?}, {:#x}) => 0", path, path_str, mode);
            0
        }
        Err(err) => {
            log!(
                "Warning: mkdir({:?} {:?}, {:#x}) failed with {:?}, returning -1",
                path,
                path_str,
                mode,
                err
            );
            -1
        }
    }
}

pub(crate) fn rmdir(env: &mut Environment, path: ConstPtr<u8>) -> i32 {
    let path_str = env.mem.cstr_at_utf8(path).unwrap();

    // TODO: respect the mode
    match env.fs.remove(GuestPath::new(&path_str)) {
        Ok(()) => {
            log_dbg!("rmdir({:?} {:?}) => 0", path, path_str);
            0
        }
        Err(err) => {
            log!(
                "Warning: rmdir({:?} {:?}) failed with {:?}, returning -1",
                path,
                path_str,
                err
            );
            -1
        }
    }
}
