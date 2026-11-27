pub const POWERSHELL_MARKS: &str = include_str!("powershell_marks.ps1");

pub fn encode_powershell_command(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn interactive_powershell_args(args: &[String]) -> Option<Vec<String>> {
    if !args.iter().all(|arg| matches!(arg.to_ascii_lowercase().as_str(), "-nologo" | "-noprofile" | "-noexit" | "-l" | "-login")) {
        return None;
    }
    let mut args = args.to_vec();
    if !args.iter().any(|arg| arg.eq_ignore_ascii_case("-noexit")) {
        args.push("-NoExit".into());
    }
    args.extend(["-EncodedCommand".into(), encode_powershell_command(POWERSHELL_MARKS)]);
    Some(args)
}
