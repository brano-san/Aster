use std::path::{Path, PathBuf};

const MAX_INCLUDE_DEPTH: usize = 16;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostEntry {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_files: Vec<String>,
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
    pub local_forwards: Vec<String>,
    pub remote_forwards: Vec<String>,
    pub dynamic_forwards: Vec<String>,
}

enum Scope {
    Global,
    Hosts(Vec<String>),
    Skipped,
}

struct Block {
    scope: Scope,
    options: Vec<(String, String)>,
}

pub fn parse(
    text: &str,
    ssh_dir: &Path,
    home: &Path,
    read: &dyn Fn(&Path) -> Option<String>,
) -> Vec<HostEntry> {
    let mut blocks = vec![Block {
        scope: Scope::Global,
        options: Vec::new(),
    }];
    collect(text, ssh_dir, home, read, 0, &mut blocks);

    let mut entries: Vec<HostEntry> = Vec::new();
    for block in &blocks {
        if let Scope::Hosts(patterns) = &block.scope {
            for pattern in patterns {
                if is_plain(pattern) && !entries.iter().any(|entry| &entry.alias == pattern) {
                    entries.push(HostEntry {
                        alias: pattern.clone(),
                        ..HostEntry::default()
                    });
                }
            }
        }
    }

    for entry in &mut entries {
        apply_blocks(&blocks, entry, home);
    }
    entries
}

pub fn resolve(
    text: &str,
    ssh_dir: &Path,
    home: &Path,
    read: &dyn Fn(&Path) -> Option<String>,
    host: &str,
) -> HostEntry {
    let mut blocks = vec![Block {
        scope: Scope::Global,
        options: Vec::new(),
    }];
    collect(text, ssh_dir, home, read, 0, &mut blocks);
    let mut entry = HostEntry {
        alias: host.to_owned(),
        ..HostEntry::default()
    };
    apply_blocks(&blocks, &mut entry, home);
    entry
}

fn apply_blocks(blocks: &[Block], entry: &mut HostEntry, home: &Path) {
    for block in blocks {
        let applies = match &block.scope {
            Scope::Global => true,
            Scope::Hosts(patterns) => matches_patterns(patterns, &entry.alias),
            Scope::Skipped => false,
        };
        if applies {
            for (key, value) in &block.options {
                apply(entry, key, value, home);
            }
        }
    }
}

fn collect(
    text: &str,
    ssh_dir: &Path,
    home: &Path,
    read: &dyn Fn(&Path) -> Option<String>,
    depth: usize,
    blocks: &mut Vec<Block>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = split_key(line);
        let key = key.to_ascii_lowercase();
        match key.as_str() {
            "host" => blocks.push(Block {
                scope: Scope::Hosts(tokenize(rest)),
                options: Vec::new(),
            }),
            "match" => blocks.push(Block {
                scope: Scope::Skipped,
                options: Vec::new(),
            }),
            "include" => {
                let skipped = blocks
                    .last()
                    .is_some_and(|block| matches!(block.scope, Scope::Skipped));
                if skipped || depth >= MAX_INCLUDE_DEPTH {
                    continue;
                }
                for token in tokenize(rest) {
                    if token.contains('*') || token.contains('?') {
                        continue;
                    }
                    let path = resolve_path(&token, ssh_dir, home);
                    if let Some(content) = read(&path) {
                        collect(&content, ssh_dir, home, read, depth + 1, blocks);
                    }
                }
            }
            _ => {
                if let Some(block) = blocks.last_mut() {
                    block.options.push((key, rest.to_string()));
                }
            }
        }
    }
}

fn split_key(line: &str) -> (&str, &str) {
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let key = &line[..end];
    let mut rest = line[end..].trim_start();
    if let Some(stripped) = rest.strip_prefix('=') {
        rest = stripped.trim_start();
    }
    (key, rest.trim_end())
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has_token = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has_token = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(current);
    }
    tokens
}

fn resolve_path(token: &str, ssh_dir: &Path, home: &Path) -> PathBuf {
    if let Some(stripped) = token.strip_prefix("~/") {
        home.join(stripped)
    } else if Path::new(token).is_absolute() {
        PathBuf::from(token)
    } else {
        ssh_dir.join(token)
    }
}

fn is_plain(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.contains(['*', '?', '!'])
}

fn matches_patterns(patterns: &[String], alias: &str) -> bool {
    let mut positive = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard_match(negated, alias) {
                return false;
            }
        } else if wildcard_match(pattern, alias) {
            positive = true;
        }
    }
    positive
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || (pattern[p] != '*' && pattern[p] == text[t]))
        {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

fn apply(entry: &mut HostEntry, key: &str, value: &str, home: &Path) {
    let tokens = tokenize(value);
    let first = tokens.first().cloned();
    match key {
        "hostname" => set_once(&mut entry.host_name, first),
        "user" => set_once(&mut entry.user, first),
        "port" => {
            if entry.port.is_none() {
                entry.port = first.and_then(|token| token.parse::<u16>().ok());
            }
        }
        "proxyjump" => set_once(&mut entry.proxy_jump, (!tokens.is_empty()).then(|| tokens.concat())),
        "proxycommand" => {
            if !value.is_empty() {
                set_once(&mut entry.proxy_command, Some(value.to_string()));
            }
        }
        "identityfile" => {
            if let Some(token) = first {
                let expanded = match token.strip_prefix("~/") {
                    Some(stripped) => home.join(stripped).to_string_lossy().into_owned(),
                    None => token,
                };
                entry.identity_files.push(expanded);
            }
        }
        "localforward" => push_forward(&mut entry.local_forwards, &tokens),
        "remoteforward" => push_forward(&mut entry.remote_forwards, &tokens),
        "dynamicforward" => push_forward(&mut entry.dynamic_forwards, &tokens),
        _ => {}
    }
}

fn set_once(slot: &mut Option<String>, value: Option<String>) {
    if slot.is_none() {
        *slot = value;
    }
}

fn push_forward(target: &mut Vec<String>, tokens: &[String]) {
    if !tokens.is_empty() {
        target.push(tokens.join(" "));
    }
}

#[cfg(test)]
#[path = "../test/config.rs"]
mod tests;
