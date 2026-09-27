//! Development-only local integrity helpers.

use std::io;

use serde::Serialize;

pub fn digest_json<T>(value: &T) -> io::Result<String>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    Ok(digest_bytes(&bytes))
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64:{hash:016x}")
}

pub fn sign_dev_placeholder(issued_by: &str, digest: &str) -> String {
    format!("dev-placeholder-signature:{issued_by}:{digest}")
}

pub fn dev_placeholder_issuer(issued_by: &str) -> String {
    format!("dev-placeholder:{issued_by}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_known_test_vectors() {
        // 标准 FNV-1a 64 位向量，钉住"不是随便一个哈希"。
        assert_eq!(digest_bytes(b""), "fnv64:cbf29ce484222325");
        assert_eq!(digest_bytes(b"a"), "fnv64:af63dc4c8601ec8c");
        assert_eq!(digest_bytes(b"foobar"), "fnv64:85944171f73967e8");
    }

    #[test]
    fn digest_is_deterministic_and_distinguishes_input() {
        assert_eq!(digest_bytes(b"hello"), digest_bytes(b"hello"));
        assert_ne!(digest_bytes(b"hello"), digest_bytes(b"hellO"));
        assert!(digest_bytes(b"hello").starts_with("fnv64:"));
    }

    #[test]
    fn digest_json_is_stable_and_matches_compact_bytes() {
        #[derive(serde::Serialize)]
        struct Payload {
            a: u32,
            b: String,
        }
        let payload = || Payload {
            a: 1,
            b: "x".into(),
        };
        let one = digest_json(&payload()).expect("digest");
        let two = digest_json(&payload()).expect("digest");
        assert_eq!(one, two, "同一值两次摘要必须一致（跨进程可重现）");
        assert_eq!(one, digest_bytes(br#"{"a":1,"b":"x"}"#));
    }

    #[test]
    fn dev_placeholders_have_a_stable_shape() {
        assert_eq!(dev_placeholder_issuer("alice"), "dev-placeholder:alice");
        assert_eq!(
            sign_dev_placeholder("alice", "fnv64:00"),
            "dev-placeholder-signature:alice:fnv64:00"
        );
    }
}
