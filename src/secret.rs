//! Secret bytes that never show up in Debug output and are wiped on drop.

use zeroize::Zeroizing;

pub struct BecomePassword(Zeroizing<Vec<u8>>);

impl BecomePassword {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for BecomePassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BecomePassword(<redacted>)")
    }
}
