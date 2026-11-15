use alacritty_terminal::term::search::RegexSearch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn pattern(query: &str, options: SearchOptions) -> String {
    let body = if options.regex { query.to_owned() } else { escape(query) };
    let body = if options.whole_word { format!(r"(?-u:\b)(?:{body})(?-u:\b)") } else { body };
    if options.case_sensitive {
        format!("(?-i){body}")
    } else {
        format!("(?i){body}")
    }
}

pub fn compile(query: &str, options: SearchOptions) -> Result<RegexSearch, String> {
    RegexSearch::new(&pattern(query, options)).map_err(|e| e.to_string())
}
