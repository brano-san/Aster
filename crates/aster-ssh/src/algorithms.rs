use aster_proto::ssh::{AlgorithmCatalog, Algorithms};
use russh::{cipher, compression, kex, mac, Preferred};
use std::borrow::Cow;

fn select<T: Clone + Eq>(
    list: &mut Cow<'static, [T]>, names: &[String], kind: &str, warnings: &mut Vec<String>,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    if names.len() == 1 && names[0].starts_with(['+', '-', '^']) {
        warnings.push(format!("Unsupported SSH {kind} list modifier in {:?}. Use an explicit list of algorithm names.", names.join(",")));
        return Err(format!("Cannot apply the configured SSH {kind} list"));
    }
    let mut selected = Vec::new();
    for name in names {
        if let Some(algorithm) = parse(name) {
            if !selected.contains(&algorithm) {
                selected.push(algorithm);
            }
        } else {
            warnings.push(format!("Unsupported SSH {kind} algorithm {name:?}"));
        }
    }
    if selected.is_empty() {
        return Err(format!("No supported SSH {kind} algorithm in the configured list"));
    }
    *list = Cow::Owned(selected);
    Ok(())
}

pub(crate) fn preferred(options: &Algorithms) -> (Result<Preferred, String>, Vec<String>) {
    let mut p = Preferred::default();
    let mut warnings = Vec::new();
    let results = [
        select(&mut p.kex, &options.kex, "kex", &mut warnings, |s| kex::Name::try_from(s).ok().filter(|n| *n != kex::NONE)),
        select(&mut p.cipher, &options.cipher, "cipher", &mut warnings, |s| cipher::Name::try_from(s).ok().filter(|n| *n != cipher::CLEAR && *n != cipher::NONE)),
        select(&mut p.mac, &options.mac, "mac", &mut warnings, |s| mac::Name::try_from(s).ok().filter(|n| *n != mac::NONE)),
        select(&mut p.key, &options.host_key, "host-key", &mut warnings, |s| russh::keys::key::ALL_KEY_TYPES.iter().find(|k| k.as_str() == s).cloned()),
        select(&mut p.compression, &options.compression, "compression", &mut warnings, |s| compression::Name::try_from(s).ok()),
    ];
    if let Some(error) = results.into_iter().find_map(Result::err) {
        return (Err(error), warnings);
    }
    if !options.kex.is_empty() {
        p.kex.to_mut().extend([kex::EXTENSION_SUPPORT_AS_CLIENT, kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT]);
    }
    (Ok(p), warnings)
}

pub fn catalog() -> AlgorithmCatalog {
    let p = Preferred::default();
    AlgorithmCatalog {
        supported: Algorithms {
            kex: exchange_names(kex::ALL_KEX_ALGORITHMS.iter().copied()),
            cipher: names(cipher::ALL_CIPHERS.iter().copied().filter(|n| **n != cipher::CLEAR && **n != cipher::NONE)),
            mac: names(mac::ALL_MAC_ALGORITHMS.iter().copied().filter(|n| **n != mac::NONE)),
            host_key: russh::keys::key::ALL_KEY_TYPES.iter().map(|key| key.as_str().into()).collect(),
            compression: names(compression::ALL_COMPRESSION_ALGORITHMS.iter().copied()),
        },
        defaults: Algorithms {
            kex: exchange_names(p.kex.iter()), cipher: names(p.cipher.iter()), mac: names(p.mac.iter()),
            host_key: p.key.iter().map(|key| key.as_str().into()).collect(), compression: names(p.compression.iter()),
        },
    }
}

fn names<'a, T: AsRef<str> + 'a>(items: impl IntoIterator<Item = &'a T>) -> Vec<String> {
    items.into_iter().map(|name| name.as_ref().into()).collect()
}

fn exchange_names<'a>(items: impl IntoIterator<Item = &'a kex::Name>) -> Vec<String> {
    names(items.into_iter().filter(|name| **name != kex::NONE && !name.as_ref().starts_with("ext-info-") && !name.as_ref().starts_with("kex-strict-")))
}

#[cfg(test)]
#[path = "../test/algorithms.rs"]
mod tests;
