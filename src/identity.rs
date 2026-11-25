use anyhow::{Context, Result, bail};
use ic_agent::Identity;
use ic_agent::identity::BasicIdentity;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::config::HASH_ITERATIONS;

/// Derives the 32-byte Ed25519 seed exactly like Taggr's seed-phrase login:
/// `sha256^HASH_ITERATIONS(utf8(phrase))`.
pub fn derive_seed(phrase: &str) -> [u8; 32] {
    let mut digest = phrase.as_bytes().to_vec();
    for _ in 0..HASH_ITERATIONS {
        digest = Sha256::digest(&digest).to_vec();
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&digest);
    seed
}

pub fn identity_from_seed(seed: &[u8; 32]) -> BasicIdentity {
    BasicIdentity::from_raw_key(seed)
}

pub fn identity_from_phrase(phrase: &str) -> BasicIdentity {
    identity_from_seed(&derive_seed(phrase))
}

pub fn principal_of(identity: &BasicIdentity) -> String {
    identity.sender().map(|p| p.to_text()).unwrap_or_default()
}

/// Persist a raw 32-byte seed as hex with 0600 permissions.
pub fn write_seed(path: &Path, seed: &[u8; 32]) -> Result<()> {
    std::fs::write(path, hex::encode(seed))
        .with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

pub fn read_seed(path: &Path) -> Result<[u8; 32]> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let bytes = hex::decode(raw.trim()).context("identity file is not valid hex")?;
    if bytes.len() != 32 {
        bail!("identity file must contain a 32-byte key");
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_agent::Identity;

    #[test]
    fn deterministic_seed() {
        let a = derive_seed("correct horse battery staple");
        let b = derive_seed("correct horse battery staple");
        assert_eq!(a, b);
        assert_ne!(a, derive_seed("another phrase"));
    }

    #[test]
    fn matches_reference_sha256_chain() {
        // Independently computed with Python hashlib: sha256 applied 15000x to utf8("test").
        assert_eq!(
            hex::encode(derive_seed("test")),
            "f8c995b0a8715d645bf77fb982d2965c8c0ebd15e43193159608afd9b3804c5f"
        );
    }

    #[test]
    fn principal_is_stable() {
        let seed = derive_seed("hello");
        let a = identity_from_seed(&seed).sender().unwrap();
        let b = identity_from_seed(&seed).sender().unwrap();
        assert_eq!(a, b);
    }
}
