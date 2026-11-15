//! Secret bytes that never show up in Debug output and are wiped on drop.

use zeroize::Zeroizing;

#[derive(Clone)]
pub struct BecomePassword(Zeroizing<Vec<u8>>);

impl BecomePassword {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Ansible strips whitespace around the contents of a password file, so a password with
    /// leading or trailing whitespace (or an empty one) would reach `sudo` altered.
    pub fn is_ansible_safe(&self) -> bool {
        let b = self.expose();
        !b.is_empty()
            && !b.first().unwrap().is_ascii_whitespace()
            && !b.last().unwrap().is_ascii_whitespace()
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
