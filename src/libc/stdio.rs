use crate::fs::GuestPath;
use crate::mem::ConstPtr;
use crate::Environment;
pub mod printf;

fn remove(env: &mut Environment, path: ConstPtr<u8>) -> i32 {
    match env
        .fs
        .remove(GuestPath::new(&env.mem.cstr_at_utf8(path).unwrap()))
    {
        Ok(()) => {
            log_dbg!("remove({:?}) => 0", path);
            0
        }
        Err(_) => {
            // TODO: set errno
            log!("Warning: remove({:?}) failed, returning -1", path);
            -1
        }
    }
}
