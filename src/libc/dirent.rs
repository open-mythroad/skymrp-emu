/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::encoding;
use crate::fs::{FsNodeType, GuestPath};
use crate::mem::{ConstPtr, MutPtr, Ptr, SafeRead};
use crate::Environment;
use std::collections::HashMap;

#[allow(clippy::upper_case_acronyms)]
pub(crate) struct DIR {
    idx: usize,
}
unsafe impl SafeRead for DIR {}

pub const MAXPATHLEN: usize = 1024;

type DirentFileType = u8;
const DT_DIR: DirentFileType = 4;
const DT_REG: DirentFileType = 8;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C, packed)]
pub struct dirent {
    pub d_ino: u64,
    pub d_seekoff: u64,
    pub d_reclen: u16,
    pub d_namlen: u16,
    pub d_type: u8,
    pub d_name: [u8; MAXPATHLEN],
}
unsafe impl SafeRead for dirent {}

#[derive(Default)]
pub struct State {
    open_dirs: HashMap<MutPtr<DIR>, Vec<(String, FsNodeType)>>,
    read_dirs: HashMap<MutPtr<DIR>, Vec<MutPtr<dirent>>>,
}
impl State {
    fn get_mut(env: &mut Environment) -> &mut Self {
        &mut env.libc_state.dirent
    }
}

pub(crate) fn opendir(env: &mut Environment, filename: ConstPtr<u8>) -> MutPtr<DIR> {
    // TODO: set errno
    let path_string = encoding::gb_to_utf8_string(env.mem.cstr_at(filename)).into_owned();
    log_dbg!("opendir: filename {}", path_string);
    let guest_path = GuestPath::new(&path_string);
    let is_dir = env.fs.is_dir(guest_path);
    if is_dir {
        let dir = env.mem.alloc_and_write(DIR { idx: 0 });
        log_dbg!("opendir: new DIR ptr: {:?}", dir);
        let iter = env.fs.enumerate_with_types(guest_path).unwrap();
        let vec = iter.map(|(str, type_)| (str.to_string(), type_)).collect();
        assert!(!State::get_mut(env).open_dirs.contains_key(&dir));
        State::get_mut(env).open_dirs.insert(dir, vec);
        assert!(!State::get_mut(env).read_dirs.contains_key(&dir));
        State::get_mut(env).read_dirs.insert(dir, Vec::new());
        dir
    } else {
        Ptr::null()
    }
}

// TODO: return '.' and '..' entries as well
pub(crate) fn readdir(env: &mut Environment, dirp: MutPtr<DIR>) -> MutPtr<dirent> {
    // TODO: set errno
    let mut dir = env.mem.read(dirp);
    let vec = env.libc_state.dirent.open_dirs.get(&dirp).unwrap();
    log_dbg!(
        "readdir: dirp {:?}, idx {}, entry '{:?}'",
        dirp,
        dir.idx,
        vec.get(dir.idx)
    );
    if let Some((str, type_)) = vec.get(dir.idx) {
        dir.idx += 1;
        env.mem.write(dirp, dir);

        let len = str.len();
        let d_type = match type_ {
            FsNodeType::File => DT_REG,
            FsNodeType::Directory => DT_DIR,
        };
        // TODO: fill other fields
        let mut dirent = dirent {
            d_ino: 0,
            d_seekoff: 0,
            d_reclen: 0,
            d_namlen: len as u16,
            d_type,
            d_name: [b'\0'; MAXPATHLEN],
        };
        dirent.d_name[..len].copy_from_slice(str.as_bytes());
        let res = env.mem.alloc_and_write(dirent);
        env.libc_state
            .dirent
            .read_dirs
            .get_mut(&dirp)
            .unwrap()
            .push(res);
        res
    } else {
        Ptr::null()
    }
}

pub(crate) fn closedir(env: &mut Environment, dirp: MutPtr<DIR>) -> i32 {
    // TODO: set errno
    log_dbg!("closedir: dirp {:?}", dirp);
    if let Some(vec) = env.libc_state.dirent.read_dirs.remove(&dirp) {
        for dirent in vec {
            env.mem.free(dirent.cast());
        }
    }
    if env.libc_state.dirent.open_dirs.remove(&dirp).is_some() {
        // this avoid double free if closedir() is called twice
        env.mem.free(dirp.cast());
    }
    0 // Success
}
