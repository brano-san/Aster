use super::*;

#[test]
fn empty_lists_keep_transport_defaults_and_preferred_order() {
    let (result, warnings) = preferred(&Algorithms::default());
    let p = result.unwrap();
    assert!(warnings.is_empty());
    assert_eq!(&p.kex[..2], &[kex::MLKEM768X25519_SHA256, kex::CURVE25519]);
    assert_eq!(&p.cipher[..2], &[cipher::CHACHA20_POLY1305, cipher::AES_256_GCM]);
    assert_eq!(&p.mac[..2], &[mac::HMAC_SHA512_ETM, mac::HMAC_SHA256_ETM]);
    assert_eq!(p.kex, Preferred::DEFAULT.kex);
    assert_eq!(p.cipher, Preferred::DEFAULT.cipher);
    assert_eq!(p.mac, Preferred::DEFAULT.mac);
    assert_eq!(p.key, Preferred::DEFAULT.key);
    assert_eq!(p.compression, Preferred::DEFAULT.compression);
}

#[test]
fn configured_algorithms_preserve_order_and_protocol_extensions() {
    let options = Algorithms {
        kex: vec!["ecdh-sha2-nistp256".into(), "curve25519-sha256".into()],
        cipher: vec!["aes128-gcm@openssh.com".into(), "absent-cipher".into(), "aes128-ctr".into()],
        mac: vec!["hmac-sha2-256".into()],
        host_key: vec!["rsa-sha2-512".into(), "absent-key".into(), "ssh-ed25519".into()],
        compression: vec!["zlib@openssh.com".into(), "none".into()],
    };
    let (result, warnings) = preferred(&options);
    let p = result.unwrap();
    assert_eq!(p.kex.as_ref(), &[kex::ECDH_SHA2_NISTP256, kex::CURVE25519, kex::EXTENSION_SUPPORT_AS_CLIENT, kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT]);
    assert_eq!(p.cipher.as_ref(), &[cipher::AES_128_GCM, cipher::AES_128_CTR]);
    assert_eq!(p.mac.as_ref(), &[mac::HMAC_SHA256]);
    assert_eq!(p.key.iter().map(|k| k.as_str()).collect::<Vec<_>>(), ["rsa-sha2-512", "ssh-ed25519"]);
    assert_eq!(p.compression.as_ref(), &[compression::ZLIB_LEGACY, compression::NONE]);
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].contains("absent-cipher"));
    assert!(warnings[1].contains("absent-key"));
}

#[test]
fn entirely_unsupported_list_rejects_connection() {
    let options = Algorithms { kex: vec!["unknown".into()], ..Algorithms::default() };
    let (result, warnings) = preferred(&options);
    assert!(result.unwrap_err().contains("kex"));
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("unknown"));
}

#[test]
fn algorithm_catalog_defaults_match_negotiation_and_hide_protocol_markers() {
    let catalog = catalog();
    let (selected, warnings) = preferred(&catalog.defaults);
    let selected = selected.unwrap();
    assert!(warnings.is_empty());
    assert_eq!(exchange_names(selected.kex.iter()), exchange_names(Preferred::DEFAULT.kex.iter()));
    assert_eq!(selected.cipher, Preferred::DEFAULT.cipher);
    assert_eq!(selected.mac, Preferred::DEFAULT.mac);
    assert_eq!(selected.key, Preferred::DEFAULT.key);
    assert_eq!(selected.compression, Preferred::DEFAULT.compression);
    assert!(!catalog.supported.kex.iter().any(|name| name.starts_with("ext-info") || name.starts_with("kex-strict")));
    assert!(!catalog.supported.cipher.iter().any(|name| name == "clear" || name == "none"));
}
