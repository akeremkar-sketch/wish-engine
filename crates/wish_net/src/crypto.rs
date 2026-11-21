//! The cryptography of the protocol, from reviewed libraries: X25519 key
//! agreement (`x25519-dalek`), ChaCha20-Poly1305 sealing
//! (`chacha20poly1305`) and SHA-256 for key derivation and cookies (`sha2`).
//!
//! Keys: both sides compute `shared = X25519(client, host)`; each direction
//! has its own key, `SHA-256("wish-net/1 key c2h" | shared | client | host)`
//! and the same with `h2c`. A packet's nonce is its packet number (a 64-bit
//! counter per direction, never reused); its header is the associated data.

use std::net::{IpAddr, SocketAddr};

use chacha20poly1305::{AeadInOut, ChaCha20Poly1305, Key, KeyInit, Nonce, Tag};
use sha2::{Digest, Sha256};
use wish_core::{Diagnostic, Result};
use x25519_dalek::{PublicKey, StaticSecret};

/// The size of a public key in bytes.
pub const PUBLIC_KEY_LEN: usize = 32;
/// The size of an authentication tag in bytes.
pub const TAG_LEN: usize = 16;
/// The size of a handshake cookie in bytes.
pub const COOKIE_LEN: usize = 16;
/// The size of a client token in bytes.
pub const TOKEN_LEN: usize = 16;

/// The packet number that seals the room code of a `Connect` (data packets
/// count up from 0 and never reach it, so the nonce is never reused for
/// different text).
pub const CONNECT_NUMBER: u64 = u64::MAX;

/// Where secrets come from: a hash-based generator seeded either by the
/// operating system or, for tests and the simulator, by a number.
#[derive(Clone)]
pub struct Entropy {
    seed: [u8; 32],
    counter: u64,
}

impl std::fmt::Debug for Entropy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Entropy(..)")
    }
}

impl Entropy {
    /// Seeded by the operating system's random number generator: use this
    /// for every real match.
    pub fn os() -> Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|error| {
            Diagnostic::error(
                "net_entropy_failed",
                format!("the operating system gave no random numbers: {error}"),
                "This should not happen on a working system; restart the program, and report it if it stays.",
            )
        })?;
        Ok(Self { seed, counter: 0 })
    }

    /// Seeded by a number: the same seed gives the same keys, so a simulated
    /// match is the same byte for byte. **Not secret**: for tests and the
    /// simulator only.
    pub fn seeded(seed: u64) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"wish-net/1 seeded entropy");
        hasher.update(seed.to_le_bytes());
        Self {
            seed: hasher.finalize().into(),
            counter: 0,
        }
    }

    /// Fills `out` with fresh bytes.
    pub fn fill(&mut self, out: &mut [u8]) {
        for chunk in out.chunks_mut(32) {
            let mut hasher = Sha256::new();
            hasher.update(b"wish-net/1 entropy block");
            hasher.update(self.seed);
            hasher.update(self.counter.to_le_bytes());
            self.counter = self.counter.wrapping_add(1);
            let block: [u8; 32] = hasher.finalize().into();
            chunk.copy_from_slice(&block[..chunk.len()]);
        }
    }

    /// `N` fresh bytes.
    pub fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        self.fill(&mut out);
        out
    }
}

/// An X25519 key pair.
pub struct KeyPair {
    secret: StaticSecret,
    public: [u8; PUBLIC_KEY_LEN],
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

impl KeyPair {
    /// A new key pair from `entropy`.
    pub fn generate(entropy: &mut Entropy) -> Self {
        let secret = StaticSecret::from(entropy.bytes::<32>());
        let public = PublicKey::from(&secret).to_bytes();
        Self { secret, public }
    }

    /// The public half, sent to the other side.
    pub fn public(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.public
    }

    /// The secret shared with the owner of `theirs`. Refuses a public key
    /// whose result carries no secret (a point of low order, which a hostile
    /// peer could send to force a known key).
    pub fn agree(&self, theirs: &[u8; PUBLIC_KEY_LEN]) -> Result<[u8; 32]> {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*theirs));
        if !shared.was_contributory() {
            return Err(Diagnostic::error(
                "net_key_rejected",
                "the other side sent a public key that gives no shared secret",
                "The peer is broken or hostile; nothing was sent to it. Join again, or ask the host to check its program.",
            ));
        }
        Ok(shared.to_bytes())
    }
}

/// The two keys of one connection, one per direction.
pub struct SessionKeys {
    send: ChaCha20Poly1305,
    recv: ChaCha20Poly1305,
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionKeys(..)")
    }
}

fn derive(label: &[u8], shared: &[u8; 32], client: &[u8; 32], host: &[u8; 32]) -> ChaCha20Poly1305 {
    let mut hasher = Sha256::new();
    hasher.update(b"wish-net/1 key ");
    hasher.update(label);
    hasher.update(shared);
    hasher.update(client);
    hasher.update(host);
    let key: [u8; 32] = hasher.finalize().into();
    ChaCha20Poly1305::new(&Key::from(key))
}

fn nonce(number: u64) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[..8].copy_from_slice(&number.to_le_bytes());
    Nonce::from(bytes)
}

impl SessionKeys {
    /// The client's keys: it sends with `c2h` and receives with `h2c`.
    pub fn client(shared: &[u8; 32], client: &[u8; 32], host: &[u8; 32]) -> Self {
        Self {
            send: derive(b"c2h", shared, client, host),
            recv: derive(b"h2c", shared, client, host),
        }
    }

    /// The host's keys: it sends with `h2c` and receives with `c2h`.
    pub fn host(shared: &[u8; 32], client: &[u8; 32], host: &[u8; 32]) -> Self {
        Self {
            send: derive(b"h2c", shared, client, host),
            recv: derive(b"c2h", shared, client, host),
        }
    }

    /// Seals `text` in place and appends the tag: `text` becomes the sealed
    /// body. `aad` is authenticated but not hidden.
    pub fn seal(&self, number: u64, aad: &[u8], text: &mut Vec<u8>) -> Result<()> {
        let tag = self
            .send
            .encrypt_inout_detached(&nonce(number), aad, text.as_mut_slice().into())
            .map_err(|_| {
                Diagnostic::error(
                    "net_crypto_failed",
                    "a packet could not be sealed",
                    "The packet is larger than the cipher allows; this is an engine bug, please report it.",
                )
            })?;
        text.extend_from_slice(&tag);
        Ok(())
    }

    /// Opens a sealed body in place. Returns the length of the opened text
    /// (at the start of `sealed`), or `None` when the body was not sealed by
    /// the other side with this number and `aad` (tampered, replayed under
    /// another number, or another key).
    pub fn open(&self, number: u64, aad: &[u8], sealed: &mut [u8]) -> Option<usize> {
        let len = sealed.len().checked_sub(TAG_LEN)?;
        let (text, tag) = sealed.split_at_mut(len);
        let tag = Tag::try_from(&tag[..]).ok()?;
        self.recv
            .decrypt_inout_detached(&nonce(number), aad, text.into(), &tag)
            .ok()?;
        Some(len)
    }
}

/// The host's cookie for a handshake: proof that the client receives at
/// `addr`. `bucket` is the time divided by the cookie's lifetime.
pub fn cookie(
    secret: &[u8; 32],
    bucket: u64,
    addr: SocketAddr,
    client: &[u8; PUBLIC_KEY_LEN],
    token: &[u8; TOKEN_LEN],
) -> [u8; COOKIE_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(b"wish-net/1 cookie");
    hasher.update(secret);
    hasher.update(bucket.to_le_bytes());
    match addr.ip() {
        IpAddr::V4(ip) => {
            hasher.update([4]);
            hasher.update(ip.octets());
        }
        IpAddr::V6(ip) => {
            hasher.update([6]);
            hasher.update(ip.octets());
        }
    }
    hasher.update(addr.port().to_le_bytes());
    hasher.update(client);
    hasher.update(token);
    let full: [u8; 32] = hasher.finalize().into();
    let mut out = [0u8; COOKIE_LEN];
    out.copy_from_slice(&full[..COOKIE_LEN]);
    out
}

/// Compares two byte strings in time that does not depend on where they
/// differ.
pub fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (SessionKeys, SessionKeys) {
        let mut entropy = Entropy::seeded(1);
        let client = KeyPair::generate(&mut entropy);
        let host = KeyPair::generate(&mut entropy);
        let a = client.agree(&host.public()).unwrap();
        let b = host.agree(&client.public()).unwrap();
        assert_eq!(a, b);
        (
            SessionKeys::client(&a, &client.public(), &host.public()),
            SessionKeys::host(&b, &client.public(), &host.public()),
        )
    }

    #[test]
    fn a_sealed_packet_opens_and_a_tampered_one_does_not() {
        let (client, host) = pair();
        let mut body = b"hello host".to_vec();
        client.seal(3, b"header", &mut body).unwrap();
        assert_eq!(body.len(), 10 + TAG_LEN);
        let mut copy = body.clone();
        assert_eq!(host.open(3, b"header", &mut copy), Some(10));
        assert_eq!(&copy[..10], b"hello host");
        // Every bit of the body and the tag matters.
        for bit in 0..body.len() * 8 {
            let mut bad = body.clone();
            bad[bit / 8] ^= 1 << (bit % 8);
            assert_eq!(host.open(3, b"header", &mut bad), None, "bit {bit}");
        }
        // So do the header, the number and the direction.
        assert_eq!(host.open(3, b"headex", &mut body.clone()), None);
        assert_eq!(host.open(4, b"header", &mut body.clone()), None);
        assert_eq!(client.open(3, b"header", &mut body.clone()), None);
        assert_eq!(host.open(3, b"header", &mut body[..5].to_vec()), None);
    }

    #[test]
    fn another_key_does_not_open() {
        let (client, _) = pair();
        let mut entropy = Entropy::seeded(2);
        let other_client = KeyPair::generate(&mut entropy);
        let other_host = KeyPair::generate(&mut entropy);
        let shared = other_host.agree(&other_client.public()).unwrap();
        let stranger = SessionKeys::host(&shared, &other_client.public(), &other_host.public());
        let mut body = b"secret".to_vec();
        client.seal(0, b"", &mut body).unwrap();
        assert_eq!(stranger.open(0, b"", &mut body), None);
    }

    #[test]
    fn a_low_order_key_is_refused() {
        let mut entropy = Entropy::seeded(3);
        let pair = KeyPair::generate(&mut entropy);
        assert_eq!(pair.agree(&[0u8; 32]).unwrap_err().code, "net_key_rejected");
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(pair.agree(&one).is_err());
    }

    #[test]
    fn entropy_is_repeatable_only_when_seeded() {
        let a: [u8; 40] = Entropy::seeded(9).bytes();
        let b: [u8; 40] = Entropy::seeded(9).bytes();
        let c: [u8; 40] = Entropy::seeded(10).bytes();
        assert_eq!(a, b);
        assert_ne!(a, c);
        let x: [u8; 32] = Entropy::os().unwrap().bytes();
        let y: [u8; 32] = Entropy::os().unwrap().bytes();
        assert_ne!(x, y);
    }

    #[test]
    fn cookies_depend_on_everything() {
        let secret = [7u8; 32];
        let addr: SocketAddr = "1.2.3.4:5".parse().unwrap();
        let other: SocketAddr = "1.2.3.4:6".parse().unwrap();
        let base = cookie(&secret, 1, addr, &[1; 32], &[2; 16]);
        assert_ne!(base, cookie(&secret, 2, addr, &[1; 32], &[2; 16]));
        assert_ne!(base, cookie(&secret, 1, other, &[1; 32], &[2; 16]));
        assert_ne!(base, cookie(&secret, 1, addr, &[3; 32], &[2; 16]));
        assert_ne!(base, cookie(&secret, 1, addr, &[1; 32], &[4; 16]));
        assert_ne!(base, cookie(&[8; 32], 1, addr, &[1; 32], &[2; 16]));
        assert!(same_bytes(&base, &base));
        assert!(!same_bytes(&base, &[0; 16]));
    }
}
