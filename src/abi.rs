use crate::Environment;

pub trait CallFromGuest {
    fn call_from_guest(&self, env: &mut Environment);
}

// TODO: implementations for other parameter types and counts
impl CallFromGuest for fn(&mut Environment, u32) {
    fn call_from_guest(&self, env: &mut Environment) {
        let arg = env.cpu.regs()[0];
        self(env, arg)
    }
}
