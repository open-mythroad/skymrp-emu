mod generic_char;
pub mod stdio;
pub mod stdlib;
pub mod string;

#[derive(Default)]
pub struct State {
    stdlib: stdlib::State,
}
