use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=shaders/ui.hlsl");
    println!("cargo:rerun-if-changed=idl/default_terminal.idl");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    #[cfg(windows)]
    compile_default_terminal_proxy(&out);
    let src = fs::read_to_string("shaders/ui.hlsl").unwrap();
    for (entry, target, file) in [("vs_main", "vs_5_0", "ui_vs.cso"), ("ps_main", "ps_5_0", "ui_ps.cso")] {
        let code = compile(&src, entry, target).unwrap_or_else(|e| panic!("{entry}: {e}"));
        fs::write(out.join(file), code).unwrap();
    }
}

#[cfg(windows)]
fn compile_default_terminal_proxy(out: &std::path::Path) {
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSDKVersion");
    let target = env::var("TARGET").unwrap();
    let architecture = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "win32",
        Ok("x86_64") => "x64",
        Ok("aarch64") => "arm64",
        _ => panic!("unsupported Windows target for the default terminal proxy: {target}"),
    };
    let mut midl = cc::windows_registry::find(&target, "midl.exe").expect("install the Windows SDK MIDL compiler");
    let compiler = cc::windows_registry::find_tool(&target, "cl.exe").expect("install the MSVC C compiler");
    let path = midl.get_envs().find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .and_then(|(_, value)| value.map(ToOwned::to_owned)).or_else(|| env::var_os("PATH")).unwrap_or_default();
    let path = env::join_paths(std::iter::once(compiler.path().parent().unwrap().to_owned()).chain(env::split_paths(&path))).unwrap();
    let status = midl.env("PATH", path)
        .args(["/nologo", "/env", architecture, "/target", "NT100", "/robust", "/protocol", "all", "/out"])
        .arg(out)
        .args(["/h", "default_terminal.h", "/iid", "default_terminal_i.c", "/proxy", "default_terminal_p.c", "/dlldata", "default_terminal_data.c"])
        .arg("idl/default_terminal.idl")
        .status().expect("run the Windows SDK MIDL compiler");
    assert!(status.success(), "compiling the default terminal proxy failed: {status}");

    cc::Build::new().include(out)
        .define("DllGetClassObject", "AsterDeftermProxyGetClassObject")
        .define("PROXY_CLSID_IS", "{0xf10cc1ad,0x2771,0x4baf,{0xa9,0x81,0x96,0x37,0xec,0x31,0x59,0xdb}}")
        .file(out.join("default_terminal_i.c"))
        .file(out.join("default_terminal_p.c"))
        .file(out.join("default_terminal_data.c"))
        .compile("aster_defterm_proxy");
    println!("cargo:rustc-link-lib=rpcrt4");
    println!("cargo:rustc-link-lib=ole32");
    println!("cargo:rustc-link-lib=oleaut32");
}

#[cfg(windows)]
fn compile(src: &str, entry: &str, target: &str) -> Result<Vec<u8>, String> {
    use std::ffi::{c_void, CString};
    #[repr(C)]
    struct BlobVtbl {
        _qi: usize,
        _add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        ptr: unsafe extern "system" fn(*mut c_void) -> *const u8,
        size: unsafe extern "system" fn(*mut c_void) -> usize,
    }
    #[link(name = "d3dcompiler_47", kind = "raw-dylib")]
    extern "system" {
        fn D3DCompile(
            src: *const c_void,
            len: usize,
            name: *const i8,
            defines: *const c_void,
            include: *const c_void,
            entry: *const i8,
            target: *const i8,
            flags1: u32,
            flags2: u32,
            code: *mut *mut c_void,
            errors: *mut *mut c_void,
        ) -> i32;
    }
    unsafe fn take(blob: *mut c_void) -> Vec<u8> {
        let vt = &**(blob as *const *const BlobVtbl);
        let v = std::slice::from_raw_parts((vt.ptr)(blob), (vt.size)(blob)).to_vec();
        (vt.release)(blob);
        v
    }
    const OPTIMIZATION_LEVEL3: u32 = 1 << 15;
    let (name, entry, target) = (CString::new("ui.hlsl").unwrap(), CString::new(entry).unwrap(), CString::new(target).unwrap());
    let (mut code, mut errors) = (std::ptr::null_mut(), std::ptr::null_mut());
    unsafe {
        let hr = D3DCompile(
            src.as_ptr().cast(),
            src.len(),
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            entry.as_ptr(),
            target.as_ptr(),
            OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            &mut errors,
        );
        let log = if errors.is_null() { Vec::new() } else { take(errors) };
        if hr < 0 {
            return Err(String::from_utf8_lossy(&log).into_owned());
        }
        Ok(take(code))
    }
}

#[cfg(not(windows))]
fn compile(_: &str, _: &str, _: &str) -> Result<Vec<u8>, String> {
    Err("cross-compiling to Windows needs a Windows host for D3DCompile".into())
}
