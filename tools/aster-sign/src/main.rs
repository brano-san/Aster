use aster_core::update::{base64_decode, base64_encode, platform_of_asset, render_manifest};
use ed25519_dalek::{Signer, SigningKey, Verifier, SECRET_KEY_LENGTH};
use std::path::Path;
use std::process::ExitCode;

const KEY_VAR: &str = "ASTER_UPDATE_KEY";
const USAGE: &str = "usage:
  aster-sign keygen
  aster-sign sign <file>
  aster-sign manifest <version> <pub-date> <base-url> <notes-file> <archive>...
sign and manifest read the base64 secret key from ASTER_UPDATE_KEY";

fn keygen() -> Result<(), String> {
    let mut seed = [0u8; SECRET_KEY_LENGTH];
    getrandom::fill(&mut seed).map_err(|e| format!("system random source: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    println!("public: {}", base64_encode(key.verifying_key().as_bytes()));
    println!("secret: {}", base64_encode(&seed));
    Ok(())
}

fn key() -> Result<SigningKey, String> {
    let text = std::env::var(KEY_VAR).map_err(|_| format!("{KEY_VAR} is not set"))?;
    let seed = base64_decode(&text).ok_or_else(|| format!("{KEY_VAR} is not base64"))?;
    let seed: [u8; SECRET_KEY_LENGTH] = seed.try_into().map_err(|_| format!("{KEY_VAR} must hold {SECRET_KEY_LENGTH} bytes"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn sign(key: &SigningKey, file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = key.sign(&bytes);
    key.verifying_key().verify(&bytes, &signature).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(base64_encode(&signature.to_bytes()))
}

fn manifest(version: &str, date: &str, base: &str, notes: &Path, archives: &[&str]) -> Result<(), String> {
    let key = key()?;
    let notes = std::fs::read_to_string(notes).map_err(|e| format!("{}: {e}", notes.display()))?;
    let mut platforms = Vec::new();
    for archive in archives {
        let path = Path::new(archive);
        let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("{archive}: no file name"))?;
        let platform = platform_of_asset(name).ok_or_else(|| format!("{name}: not an Aster release archive"))?;
        platforms.push((platform, format!("{}/{name}", base.trim_end_matches('/')), sign(&key, path)?));
    }
    print!("{}", render_manifest(version, notes.trim_start_matches('\u{feff}').trim(), date, &platforms));
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["keygen"] => keygen(),
        ["sign", file] => key().and_then(|key| sign(&key, Path::new(file))).map(|s| println!("{s}")),
        ["manifest", version, date, base, notes, archives @ ..] if !archives.is_empty() => manifest(version, date, base, Path::new(notes), archives),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aster-sign: {e}");
            ExitCode::FAILURE
        }
    }
}
