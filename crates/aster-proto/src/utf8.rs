pub(crate) fn decode(pending: &mut Vec<u8>, out: &mut String) {
    let mut consumed = 0;
    while consumed < pending.len() {
        match std::str::from_utf8(&pending[consumed..]) {
            Ok(valid) => {
                out.push_str(valid);
                consumed = pending.len();
            }
            Err(error) => {
                let end = consumed + error.valid_up_to();
                out.push_str(std::str::from_utf8(&pending[consumed..end]).unwrap_or_default());
                consumed = end;
                match error.error_len() {
                    Some(length) => {
                        out.push('\u{fffd}');
                        consumed += length;
                    }
                    None => break,
                }
            }
        }
    }
    pending.drain(..consumed);
}

#[cfg(test)]
#[path = "../test/utf8.rs"]
mod tests;
