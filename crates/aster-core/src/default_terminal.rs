#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Unavailable,
    Available,
    Selected,
}

impl State {
    pub fn offers(self, enable: bool) -> bool {
        if enable { self == Self::Available } else { self == Self::Selected }
    }
}

pub fn valid_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}
