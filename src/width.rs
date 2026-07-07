/// Width of a length prefix or variant index on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Width {
    W8,
    W16,
    #[default]
    W32,
    W64,
}

impl Width {
    pub fn bytes(self) -> u8 {
        match self {
            Width::W8 => 1,
            Width::W16 => 2,
            Width::W32 => 4,
            Width::W64 => 8,
        }
    }

    pub fn from_bytes(n: u8) -> Option<Width> {
        match n {
            1 => Some(Width::W8),
            2 => Some(Width::W16),
            4 => Some(Width::W32),
            8 => Some(Width::W64),
            _ => None,
        }
    }
}
