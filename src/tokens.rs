//! Ren logikk for engangskoder: generering og hashing. Fri for database- og
//! HTTP-avhengigheter, så den kan enhetstestes isolert. Flyten ligger i `auth`,
//! lagringen i `db::magic_token`.

use rand::RngExt;
use rand::seq::IndexedRandom;
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// Tegnsettet koder trekkes fra. Forveksbare tegn (`I`/`L`/`1`, `O`/`0`) er
/// utelatt så koden er lett å taste fra en e-post.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// 6 tegn fra et alfabet på 31 gir ~887 millioner kombinasjoner. Sammen med kort
/// levetid og forsøkssperren gjør det brute-force upraktisk.
const CODE_LENGTH: usize = 6;

/// Genererer en ny engangskode, f.eks. `ABC-123`. Tegnene trekkes fra en
/// kryptografisk sikker RNG (`rand::rng()` implementerer `CryptoRng`), så koden
/// ikke kan forutsies. Bindestreken er kun for lesbarhet og strippes i `hash_token`.
pub fn generate_code() -> String {
    let mut rng = rand::rng();
    let chars: Vec<u8> = (0..CODE_LENGTH)
        .map(|_| *CODE_ALPHABET.choose(&mut rng).expect("alfabetet er ikke tomt"))
        .collect();
    let code = String::from_utf8(chars).expect("alfabetet er ren ASCII");

    let midt = CODE_LENGTH / 2;
    format!("{}-{}", &code[..midt], &code[midt..])
}

/// Antall tilfeldige byte i et sesjonstoken. 32 byte = 256 bit entropi, langt mer
/// enn engangskoden trenger – et sesjonstoken tastes aldri for hånd, det lagres av
/// klienten og sendes på hvert kall, så her er det ingen grunn til å spare på lengden.
const SESSION_TOKEN_BYTES: usize = 32;

/// Genererer et sesjonstoken: 32 kryptografisk tilfeldige byte hex-kodet til en
/// 64-tegns streng. Trekkes fra `rand::rng()` (en `CryptoRng`), så det er
/// uforutsigbart og praktisk talt umulig å gjette. I motsetning til engangskoden
/// er dette ikke ment å være lesbart – det er en ugjennomsiktig nøkkel.
///
/// Tokenet returneres i klartekst kun her, til klienten. Det vi *lagrer* er
/// `hash_token(...)` av det, slik at en databaselekkasje ikke gir kaprede sesjoner.
pub fn generate_session_token() -> String {
    let bytes: [u8; SESSION_TOKEN_BYTES] = rand::rng().random();
    to_hex(&bytes)
}

/// Normaliserer en kode før hashing: store bokstaver, kun bokstaver/tall. Da hasher
/// `"abc-123"` og `" ABC 123 "` likt, så brukeren slipper å treffe formateringen.
fn canonicalize(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Hasher en kode med SHA-256 til en hex-streng. Vi lagrer kun hashen, aldri
/// klartekst, så en databaselekkasje ikke gir innlogging. SHA-256 (ikke
/// bcrypt/argon2) holder fordi sikkerheten hviler på at koden er kortlevd,
/// engangs og forsøksbegrenset – ikke på hash-styrke.
pub fn hash_token(code: &str) -> String {
    let canonical = canonicalize(code);
    let digest = Sha256::digest(canonical.as_bytes());
    to_hex(&digest)
}

/// Hex-encoder bytes til lowercase. Gjort for hånd for å slippe en `hex`-crate.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{:02x}", b).expect("skriving til String feiler ikke");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generert_kode_har_forventet_format() {
        let code = generate_code();
        // Formatet er XXX-XXX: to grupper på tre tegn, skilt med bindestrek.
        let deler: Vec<&str> = code.split('-').collect();
        assert_eq!(deler.len(), 2);
        assert_eq!(deler[0].len(), 3);
        assert_eq!(deler[1].len(), 3);
        // Alle tegn (utenom bindestrek) skal komme fra alfabetet.
        for byte in code.bytes().filter(|&b| b != b'-') {
            assert!(CODE_ALPHABET.contains(&byte), "uventet tegn: {}", byte as char);
        }
    }

    #[test]
    fn genererte_koder_er_unike() {
        // Ikke et bevis på tilfeldighet, men fanger en RNG som er satt fast.
        let a = generate_code();
        let b = generate_code();
        assert_ne!(a, b);
    }

    #[test]
    fn sesjonstoken_er_64_hex_tegn() {
        // 32 byte hex-kodet -> 64 tegn, alle gyldige hex-sifre.
        let token = generate_session_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sesjonstoken_er_unike() {
        // Fanger en RNG som er satt fast; ikke et bevis på tilfeldighet.
        assert_ne!(generate_session_token(), generate_session_token());
    }

    #[test]
    fn hash_er_uavhengig_av_formatering() {
        // Bindestrek, mellomrom og store/små bokstaver skal ikke påvirke hashen.
        let kanonisk = hash_token("ABC123");
        assert_eq!(hash_token("abc-123"), kanonisk);
        assert_eq!(hash_token(" ABC-123 "), kanonisk);
        assert_eq!(hash_token("a b c 1 2 3"), kanonisk);
    }

    #[test]
    fn hash_skiller_ulike_koder() {
        assert_ne!(hash_token("ABC-123"), hash_token("ABC-124"));
    }

    #[test]
    fn hash_er_hex_av_sha256() {
        // SHA-256 gir 32 byte -> 64 hex-tegn.
        let hash = hash_token("ABC-123");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
