use crate::mem::{ConstPtr, MutPtr, SafeRead};
use crate::Environment;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Md5State {
    count: [u32; 2],
    abcd: [u32; 4],
    buf: [u8; 64],
}

unsafe impl SafeRead for Md5State {}

const INITIAL_ABCD: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
const PAD: [u8; 64] = {
    let mut pad = [0; 64];
    pad[0] = 0x80;
    pad
};

pub fn init(env: &mut Environment, pms: MutPtr<Md5State>) {
    let mut state = env.mem.read(pms);
    state.count = [0, 0];
    state.abcd = INITIAL_ABCD;
    env.mem.write(pms, state);
}

pub fn append(env: &mut Environment, pms: MutPtr<Md5State>, data: ConstPtr<u8>, nbytes: i32) {
    if nbytes <= 0 {
        return;
    }

    let mut state = env.mem.read(pms);
    let data = env.mem.bytes_at(data, nbytes as u32);

    append_bytes(&mut state, data);
    env.mem.write(pms, state);
}

pub fn finish(env: &mut Environment, pms: MutPtr<Md5State>, digest: MutPtr<u8>) {
    let mut state = env.mem.read(pms);
    let mut data = [0; 8];

    for (i, byte) in data.iter_mut().enumerate() {
        *byte = (state.count[i >> 2] >> ((i & 3) << 3)) as u8;
    }

    let pad_len = ((55u32.wrapping_sub(state.count[0] >> 3)) & 63) + 1;
    append_bytes(&mut state, &PAD[..pad_len as usize]);
    append_bytes(&mut state, &data);

    for i in 0..16 {
        env.mem.write(
            digest + i as u32,
            (state.abcd[i >> 2] >> ((i & 3) << 3)) as u8,
        );
    }

    env.mem.write(pms, state);
}

fn append_bytes(state: &mut Md5State, mut data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let offset = ((state.count[0] >> 3) & 63) as usize;
    let nbits = (data.len() as u32) << 3;

    state.count[1] = state.count[1].wrapping_add((data.len() as u32) >> 29);
    state.count[0] = state.count[0].wrapping_add(nbits);
    if state.count[0] < nbits {
        state.count[1] = state.count[1].wrapping_add(1);
    }

    if offset != 0 {
        let copy = (64 - offset).min(data.len());
        state.buf[offset..offset + copy].copy_from_slice(&data[..copy]);
        if offset + copy < 64 {
            return;
        }
        let block = state.buf;
        process(state, &block);
        data = &data[copy..];
    }

    while data.len() >= 64 {
        let block: &[u8; 64] = data[..64].try_into().unwrap();
        process(state, block);
        data = &data[64..];
    }

    if !data.is_empty() {
        state.buf[..data.len()].copy_from_slice(data);
    }
}

fn process(state: &mut Md5State, block: &[u8; 64]) {
    fast_md5::transform(&mut state.abcd, block);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(chunks: &[&[u8]]) -> [u8; 16] {
        let mut state = Md5State {
            count: [0, 0],
            abcd: INITIAL_ABCD,
            buf: [0; 64],
        };
        for chunk in chunks {
            append_bytes(&mut state, chunk);
        }

        let mut len = [0; 8];
        for (i, byte) in len.iter_mut().enumerate() {
            *byte = (state.count[i >> 2] >> ((i & 3) << 3)) as u8;
        }

        let pad_len = ((55u32.wrapping_sub(state.count[0] >> 3)) & 63) + 1;
        append_bytes(&mut state, &PAD[..pad_len as usize]);
        append_bytes(&mut state, &len);

        let mut digest = [0; 16];
        for i in 0..16 {
            digest[i] = (state.abcd[i >> 2] >> ((i & 3) << 3)) as u8;
        }
        digest
    }

    #[test]
    fn md5_known_vectors() {
        assert_eq!(digest(&[b""]), fast_md5::digest(b""));
        assert_eq!(digest(&[b"abc"]), fast_md5::digest(b"abc"));
        assert_eq!(
            digest(&[b"The quick brown fox ", b"jumps over the lazy dog"]),
            fast_md5::digest(b"The quick brown fox jumps over the lazy dog")
        );
    }

    #[test]
    fn md5_chunk_boundaries() {
        let data = [b'a'; 200];
        assert_eq!(digest(&[&data[..]]), fast_md5::digest(&data));
        assert_eq!(
            digest(&[&data[..1], &data[1..63], &data[63..64], &data[64..]]),
            fast_md5::digest(&data)
        );
    }
}
