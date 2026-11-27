#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub target: String,
}

const SCHEMES: &[&str] = &["https://", "http://", "ftp://", "file://"];

fn stops(c: char) -> bool {
    c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`' | '|' | '\0')
}

fn trim_tail(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let c = chars[end - 1];
        let open = |o: char| chars[start..end].iter().filter(|&&x| x == o).count();
        let unbalanced = match c {
            ')' => open('(') < open(')'),
            ']' => open('[') < open(']'),
            '}' => open('{') < open('}'),
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            _ => false,
        };
        if !unbalanced {
            break;
        }
        end -= 1;
    }
    end
}

fn starts_with_at(chars: &[char], at: usize, pat: &str) -> bool {
    chars.get(at..at + pat.len()).is_some_and(|text| {
        text.iter().zip(pat.bytes()).all(|(a, b)| a.eq_ignore_ascii_case(&char::from(b)))
    })
}

fn word_start(chars: &[char], at: usize) -> bool {
    at == 0 || chars[at - 1].is_whitespace() || matches!(chars[at - 1], '(' | '[' | '"' | '\'' | '=' | ':' | '<')
}

pub fn find_links(text: &str) -> Vec<Link> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let url = SCHEMES.iter().any(|s| starts_with_at(&chars, i, s));
        let drive = i + 2 < chars.len() && chars[i].is_ascii_alphabetic() && chars[i + 1] == ':' && (chars[i + 2] == '\\' || chars[i + 2] == '/') && word_start(&chars, i);
        let unix = word_start(&chars, i)
            && ((chars[i] == '/' && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric() || *c == '.' || *c == '_'))
                || (chars[i] == '~' && chars.get(i + 1) == Some(&'/')));
        if url || drive || unix {
            let mut end = i;
            while end < chars.len() && !stops(chars[end]) {
                end += 1;
            }
            let end = trim_tail(&chars, i, end);
            let target: String = chars[i..end].iter().collect();
            let valid = if url { SCHEMES.iter().any(|s| target.len() > s.len() && target.to_ascii_lowercase().starts_with(s)) } else if unix { target.matches('/').count() >= 2 || target.starts_with("~/") } else { target.len() > 3 };
            if valid {
                out.push(Link { start: i, end, target });
                i = end;
                continue;
            }
            i = end.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
#[path = "../test/links.rs"]
mod tests;
