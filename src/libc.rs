pub mod dirent;
mod generic_char;
pub mod netdb;
pub mod posix_io;
pub mod stdio;
pub mod stdlib;
pub mod string;
pub mod sys;
pub mod time;

#[derive(Default)]
pub struct State {
    dirent: dirent::State,
    stdlib: stdlib::State,
    posix_io: posix_io::State,
    pub socket: sys::socket::State,
}
