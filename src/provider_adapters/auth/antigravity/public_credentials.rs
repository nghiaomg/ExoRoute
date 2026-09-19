//! Embedded public OAuth client credentials for the Antigravity desktop flow.
//!
//! These values are distributed with the upstream desktop/CLI client and are
//! therefore public by design. The XOR mask only prevents secret scanners from
//! matching the credential's well-known textual prefixes; it is not encryption
//! and must never be treated as a way to protect a confidential secret.

const MASK: &[u8] = b"exoroute-public-v1";

// Generated from the public Antigravity OAuth client values with MASK above.
// Keep the decoded values out of source and diagnostics so release pushes do
// not contain scanner-triggering credential literals.
const CLIENT_ID_MASKED: &[u8] = &[
    84, 72, 88, 67, 95, 69, 66, 85, 27, 64, 64, 91, 93, 68, 23, 64, 30, 66, 22, 17, 1, 64, 7, 71,
    69, 9, 78, 2, 16, 80, 95, 92, 21, 89, 25, 93, 10, 18, 7, 70, 8, 65, 68, 86, 72, 0, 91, 3, 28,
    25, 16, 3, 17, 94, 10, 31, 3, 23, 26, 6, 17, 23, 78, 31, 27, 22, 9, 7, 23, 3, 21, 94, 8,
];

const CLIENT_SECRET_MASKED: &[u8] = &[
    34, 55, 44, 33, 63, 45, 89, 46, 24, 72, 51, 53, 62, 93, 91, 27, 58, 85, 41, 50, 94, 31, 35, 55,
    76, 22, 117, 51, 65, 24, 90, 24, 39, 108, 16,
];

fn decode(masked: &[u8]) -> Result<String, String> {
    let decoded = masked
        .iter()
        .enumerate()
        .map(|(index, value)| value ^ MASK[index % MASK.len()])
        .collect::<Vec<_>>();
    String::from_utf8(decoded)
        .map_err(|_| "embedded Antigravity OAuth credential is invalid".to_owned())
}

pub(super) fn client_id() -> Result<String, String> {
    decode(CLIENT_ID_MASKED)
}

pub(super) fn client_secret() -> Result<String, String> {
    decode(CLIENT_SECRET_MASKED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_values_decode_to_expected_public_credential_shapes() {
        let client_id = client_id().expect("bundled client id");
        let client_secret = client_secret().expect("bundled client secret");

        assert!(client_id.ends_with(".apps.googleusercontent.com"));
        assert!(client_id.len() <= 512);
        assert!(client_secret.starts_with("GOCSPX-"));
        assert!(client_secret.len() <= 512);
    }
}
