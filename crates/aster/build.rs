#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use std::{env, path::PathBuf};

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=aster.rc");
    println!("cargo:rerun-if-changed=../../assets/windows/aster.ico");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSDKVersion");

    let target = env::var("TARGET").unwrap();
    let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("aster.res");
    let status = cc::windows_registry::find(&target, "rc.exe")
        .expect("install the Windows SDK resource compiler")
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg("aster.rc")
        .status()
        .expect("run the Windows SDK resource compiler");
    assert!(status.success(), "compiling the application icon failed: {status}");
    println!("cargo:rustc-link-arg-bin=aster={}", resource.display());
}
