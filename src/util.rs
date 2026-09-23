//! Convenience utilities.
//!
//! Ported from `src/adb/util.ts`.

use tokio::io::AsyncRead;

use crate::auth::{Auth, ExtendedPublicKey};
use crate::error::Result;

/// Utilities for working with streams and public keys.
pub struct Util;

impl Util {
    /// Takes a stream and reads everything it outputs until the stream ends,
    /// resolving with the collected output. Convenient with
    /// [`crate::DeviceClient::shell`].
    pub async fn read_all<R: AsyncRead + Unpin>(stream: R) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut { stream }, &mut out).await?;
        Ok(out)
    }

    /// Parses an Android-formatted mincrypt public key, e.g.
    /// `~/.android/adbkey.pub`.
    pub fn parse_public_key(key: &str) -> Result<ExtendedPublicKey> {
        Auth::parse_public_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_all() {
        let out = Util::read_all(&b"hello"[..]).await.unwrap();
        assert_eq!(out, b"hello".to_vec());
    }

    #[tokio::test]
    async fn test_read_all_empty() {
        let out = Util::read_all(&b""[..]).await.unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn test_parse_public_key() {
        assert!(Util::parse_public_key("garbage").is_err());
    }
}
