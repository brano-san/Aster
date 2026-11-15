use std::fmt::Write;
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=schemes");
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("schemes");
    let mut names: Vec<String> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    let mut out = String::from("pub static COMMUNITY: &[(&str, &str)] = &[\n");
    for n in names {
        let path = dir.join(&n);
        writeln!(out, "    ({n:?}, include_str!({:?})),", path.to_string_lossy()).unwrap();
    }
    out.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("schemes.rs"), out).unwrap();
}
