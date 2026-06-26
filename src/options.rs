use std::num::NonZeroU32;

pub const OPTIONS_HELP: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/OPTIONS_HELP.txt"));

#[derive(Clone)]
pub struct Options {
    pub fullscreen: bool,
    pub scale: NonZeroU32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scale: NonZeroU32::new(1).unwrap(),
            fullscreen: false,
        }
    }
}

impl Options {
    pub fn parse_argument(&mut self, arg: &str) -> Result<bool, String> {
        if arg == "--fullscreen" {
            self.fullscreen = true;
        } else if let Some(value) = arg.strip_prefix("--scale=") {
            self.scale = value
                .parse()
                .map_err(|_| "Invalid scale hack factor".to_string())?;
        } else {
            return Ok(false);
        }
        Ok(true)
    }
}
