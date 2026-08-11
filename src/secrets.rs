/// Return the canonical tenant-local map name used for contract secrets.
///
/// The public integration guide describes `tenant-did()` as returning the
/// hexadecimal string form, while the bundled WIT still documents a raw
/// 20-byte `CompactDid`. Accept both representations so neither is encoded
/// twice and both resolve to the same `z:<tenant-id>:secrets` map.
pub(crate) fn secrets_map_name(
    tenant_did: &[u8],
) -> Result<alloc::string::String, alloc::string::String> {
    let tenant_id = if tenant_did.len() == 20 {
        hex::encode(tenant_did)
    } else {
        let text = core::str::from_utf8(tenant_did)
            .map_err(|e| alloc::format!("tenant DID returned by host is not valid UTF-8: {e}"))?;
        let tenant_id = text.strip_prefix("did:t3n:").unwrap_or(text);

        if tenant_id.len() != 40 || !tenant_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(
                "tenant DID returned by host is not raw 20-byte or canonical hexadecimal text"
                    .into(),
            );
        }

        tenant_id.to_ascii_lowercase()
    };

    Ok(alloc::format!("z:{tenant_id}:secrets"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn current_secrets_map_name() -> Result<alloc::string::String, alloc::string::String> {
    use crate::host::tenant::tenant_context;

    secrets_map_name(&tenant_context::tenant_did())
}

#[cfg(test)]
mod tests {
    use super::secrets_map_name;

    #[test]
    fn secrets_map_name_uses_hex_text_without_reencoding() {
        let tenant_did = b"0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            secrets_map_name(tenant_did).unwrap(),
            "z:0123456789abcdef0123456789abcdef01234567:secrets",
            "the host-provided DID text must be used verbatim"
        );
    }

    #[test]
    fn secrets_map_name_accepts_full_did_text() {
        let tenant_did = b"did:t3n:0123456789ABCDEF0123456789ABCDEF01234567";
        assert_eq!(
            secrets_map_name(tenant_did).unwrap(),
            "z:0123456789abcdef0123456789abcdef01234567:secrets"
        );
    }

    #[test]
    fn secrets_map_name_accepts_raw_compact_did() {
        let tenant_did = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        ];
        assert_eq!(
            secrets_map_name(&tenant_did).unwrap(),
            "z:0123456789abcdef0123456789abcdef01234567:secrets"
        );
    }

    #[test]
    fn secrets_map_name_rejects_unknown_shapes() {
        assert!(secrets_map_name(b"not-a-compact-did").is_err());
    }
}
