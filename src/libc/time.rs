use crate::mem::SafeRead;

#[allow(non_camel_case_types)]
/// Time in seconds since UNIX epoch (1970-01-01 00:00:00)
pub type time_t = i32;

#[allow(non_camel_case_types)]
type suseconds_t = i32;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C, packed)]
pub(super) struct timeval {
    pub(super) tv_sec: time_t,
    pub(super) tv_usec: suseconds_t,
}

unsafe impl SafeRead for timeval {}
