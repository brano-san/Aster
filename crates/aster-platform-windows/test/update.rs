use super::*;

fn split(line: &str) -> Vec<String> {
    use windows::Win32::UI::Shell::CommandLineToArgvW;
    let wide: Vec<u16> = format!("aster.exe {line}").encode_utf16().chain([0]).collect();
    let mut n = 0;
    let argv = unsafe { CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut n) };
    let args = unsafe { std::slice::from_raw_parts(argv, n as usize) }.iter().skip(1).map(|a| unsafe { a.to_string() }.unwrap()).collect();
    unsafe {
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(argv.cast())));
    }
    args
}

#[test]
fn arguments_for_the_elevated_copy_arrive_exactly_as_given() {
    let args: Vec<String> = [r"--install-update", r"C:\Users\Ann Lee\AppData\Local\Temp\aster-update-7.zip", "c2lnbmF0dXJl+/==", r"C:\dir\", r#"say "hi""#, ""].map(str::to_owned).to_vec();
    assert_eq!(split(&command_line(&args)), args);
}

#[test]
fn no_arguments_make_an_empty_command_line() {
    assert_eq!(command_line(&[]), "");
}

#[test]
fn plain_http_is_refused_before_any_connection() {
    let mut out = Vec::new();
    let e = download("http://example.com/a.zip", &mut out, &mut |_, _| true).unwrap_err();
    assert!(e.message.contains("https"), "{}", e.message);
    assert!(out.is_empty());
}

#[test]
fn waiting_for_a_process_that_is_gone_returns_at_once() {
    let start = std::time::Instant::now();
    wait_exit(u32::MAX - 3, Duration::from_secs(5));
    assert!(start.elapsed() < Duration::from_secs(2));
}
