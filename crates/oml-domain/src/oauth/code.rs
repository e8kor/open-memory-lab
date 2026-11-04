//! The connect code: twelve symbols of Crockford's base32, 60 bits, shown
//! as three groups of four, entered under Crockford's decoding rule, stored
//! as the SHA-256 of its decoded symbols (ADR-0031 d8;
//! `docs/architecture/roles-and-keys.md`, "Connections"). The randomness is
//! the caller's: this module only formats it.

use std::fmt;

use sha2::{Digest, Sha256};

use super::refusal::Refusal;

/// Crockford's base32 alphabet: `0`–`9` and `A`–`Z` without `I`, `L`, `O`
/// and `U`, in value order.
pub const CROCKFORD_ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Symbols in a connect code, five bits each: 60 bits.
pub const CODE_SYMBOLS: usize = 12;

/// A connect code, held as its twelve canonical symbols. Its `Debug` prints
/// no symbol, and it has no `Display`: the code appears in the issuance
/// response and on the CLI's stdout and nowhere else, never in a log line
/// (ADR-0031 d8, d10). [`ConnectCode::grouped`] is the one rendering.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectCode([u8; CODE_SYMBOLS]);

impl ConnectCode {
    /// The code for eight bytes the caller drew from a cryptographically
    /// secure generator: the low 60 bits of their big-endian value, most
    /// significant symbol first. The top four bits are discarded, so the 60
    /// kept are as uniform as the 64 drawn.
    #[must_use]
    pub fn from_random(random: [u8; 8]) -> Self {
        let value = u64::from_be_bytes(random);
        let alphabet = CROCKFORD_ALPHABET.as_bytes();
        let mut symbols = [0_u8; CODE_SYMBOLS];
        for (i, symbol) in symbols.iter_mut().enumerate() {
            let shift = 5 * (CODE_SYMBOLS - 1 - i);
            // Masked to five bits, so the index is below 32.
            *symbol = alphabet[usize::try_from((value >> shift) & 0x1f).unwrap_or(0)];
        }
        ConnectCode(symbols)
    }

    /// An entry decoded by Crockford's rule: ASCII case ignored, hyphens
    /// and spaces ignored, `O` read as `0`, `I` and `L` as `1`. Anything
    /// else outside the alphabet — `U` included — or a count of symbols
    /// other than twelve never decodes, and answers what an unknown code
    /// answers, `oauth.connect_code_invalid` (ADR-0031 d8, d9).
    pub fn parse(entry: &str) -> Result<Self, Refusal> {
        let mut symbols = [0_u8; CODE_SYMBOLS];
        let mut count = 0;
        for byte in entry.bytes() {
            let symbol = match byte.to_ascii_uppercase() {
                b'-' | b' ' => continue,
                b'O' => b'0',
                b'I' | b'L' => b'1',
                b'U' => return Err(Refusal::ConnectCodeInvalid),
                other if CROCKFORD_ALPHABET.as_bytes().contains(&other) => other,
                _ => return Err(Refusal::ConnectCodeInvalid),
            };
            let slot = symbols.get_mut(count).ok_or(Refusal::ConnectCodeInvalid)?;
            *slot = symbol;
            count += 1;
        }
        if count == CODE_SYMBOLS {
            Ok(ConnectCode(symbols))
        } else {
            Err(Refusal::ConnectCodeInvalid)
        }
    }

    /// The twelve canonical symbols, ungrouped.
    #[must_use]
    pub fn symbols(&self) -> &str {
        // Every symbol is a byte of the ASCII alphabet.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// The code as shown: three groups of four, `7KQ2-M9XD-4TPA`.
    #[must_use]
    pub fn grouped(&self) -> String {
        let s = self.symbols();
        format!("{}-{}-{}", &s[..4], &s[4..8], &s[8..])
    }

    /// `connect_codes.hash`: the SHA-256 of the twelve decoded symbols.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl fmt::Debug for ConnectCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConnectCode(..)")
    }
}
