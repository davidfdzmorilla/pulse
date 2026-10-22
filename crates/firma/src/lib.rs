//! Pulse's own signature on what it publishes for Windows: an Ed25519
//! signature of the whole zip, in `<zip>.firma` beside it. The updater only
//! installs a zip signed with the key whose public half is [`PUBLIC_KEY`], so
//! the GitHub account alone (its password, a token, a renamed repository) is
//! not enough to put a program on everyone's computer. The private half is
//! the author's, in a file outside the repository (see docs/PUBLICAR.md).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// The public half of the key Pulse's releases are signed with.
/// (d69a6153d30776975952a532c596690a49f2930e1f0ce028120c127972b12033)
pub const PUBLIC_KEY: [u8; 32] = [214, 154, 97, 83, 211, 7, 118, 151, 89, 82, 165, 50, 197, 150, 105, 10, 73, 242, 147, 14, 31, 12, 224, 40, 18, 12, 18, 121, 114, 177, 32, 51];

/// What a signature file starts with (and its format's version).
const HEADER: &str = "pulse-firma 1";

/// Bytes as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex back to bytes (exactly `N` of them).
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != 2 * N || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The public half of the private key `secret`.
pub fn public_of(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

/// The signature file's text for `data`.
pub fn sign(secret: &[u8; 32], data: &[u8]) -> String {
    let signature = SigningKey::from_bytes(secret).sign(data);
    format!("{HEADER}\n{}\n", hex(&signature.to_bytes()))
}

/// Whether `signature` (a signature file's text) is `public`'s for `data`.
pub fn verify(public: &[u8; 32], data: &[u8], signature: &str) -> Result<(), String> {
    let mut lines = signature.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Err("La firma no tiene el formato de Pulse".into());
    }
    let bytes = lines.next().and_then(unhex::<64>).ok_or("La firma está dañada")?;
    let key = VerifyingKey::from_bytes(public).map_err(|_| "La clave pública de Pulse no es válida".to_string())?;
    key.verify(data, &Signature::from_bytes(&bytes)).map_err(|_| "La firma no es de Pulse, o el archivo cambió después de firmarlo".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_holds_for_its_file_and_key_only() {
        let secret = [7u8; 32];
        let public = public_of(&secret);
        let data = b"Pulse-0.2.0-windows-x64.zip";
        let signature = sign(&secret, data);
        assert_eq!(verify(&public, data, &signature), Ok(()));
        assert!(verify(&public, b"otro archivo", &signature).is_err(), "another file");
        assert!(verify(&public_of(&[8u8; 32]), data, &signature).is_err(), "another key");
        let mut damaged = signature.clone().into_bytes();
        damaged[20] = if damaged[20] == b'0' { b'1' } else { b'0' };
        assert!(verify(&public, data, &String::from_utf8(damaged).unwrap()).is_err(), "a changed signature");
        assert!(verify(&public, data, "").is_err() && verify(&public, data, "pulse-firma 1\nzz").is_err());
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex::<3>(&hex(&[0, 171, 255])), Some([0, 171, 255]));
        assert_eq!(unhex::<3>("00ab"), None);
        assert_eq!(unhex::<1>("zz"), None);
    }

    /// The key built in is a real one (not the placeholder).
    #[test]
    fn the_public_key_is_set() {
        assert_ne!(PUBLIC_KEY, [0; 32]);
        assert!(VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
    }
}
