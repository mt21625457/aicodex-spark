use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verify a Gitea webhook signature.
///
/// Gitea sends the HMAC-SHA256 signature in `X-Gitea-Signature` as a raw hex
/// digest without a `sha256=` prefix.
pub fn verify_webhook_signature(secret: &[u8], signature_header: &str, payload: &[u8]) -> bool {
    let Ok(expected_signature) = hex::decode(signature_header.trim()) else {
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(secret) else {
        return false;
    };
    mac.update(payload);
    let computed_signature = mac.finalize().into_bytes();

    computed_signature[..].ct_eq(&expected_signature).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_signature() {
        let secret = b"test-secret";
        let payload = b"test payload";

        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(payload);
        let signature = mac.finalize().into_bytes();
        let signature_header = hex::encode(signature);

        assert!(verify_webhook_signature(secret, &signature_header, payload));
    }

    #[test]
    fn test_invalid_signature() {
        let secret = b"test-secret";
        let payload = b"test payload";
        let wrong_signature = "0000000000000000000000000000000000000000000000000000000000000000";

        assert!(!verify_webhook_signature(secret, wrong_signature, payload));
    }

    #[test]
    fn test_invalid_hex() {
        let secret = b"test-secret";
        let payload = b"test payload";
        let invalid_hex = "not-valid-hex";

        assert!(!verify_webhook_signature(secret, invalid_hex, payload));
    }
}
