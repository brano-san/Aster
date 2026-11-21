use std::collections::BTreeMap;

pub type Variables = BTreeMap<String, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalEnvironment {
    pub global: Variables,
    pub profile: Variables,
    pub refresh: bool,
    pub set_comspec: bool,
}

pub struct RegistryValue {
    pub name: String,
    pub value: String,
    pub expandable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Syntax {
    Unix,
    Windows,
    Registry,
}

impl Syntax {
    fn for_os(windows: bool) -> Self {
        if windows { Self::Windows } else { Self::Unix }
    }

    fn key(self, name: &str) -> String {
        if self == Self::Unix { name.to_owned() } else { name.to_uppercase() }
    }
}

fn reference(text: &str, syntax: Syntax) -> Option<(&str, usize)> {
    let rest = &text[1..];
    match text.as_bytes()[0] {
        b'%' if syntax != Syntax::Unix => {
            let end = rest.find('%')?;
            Some((&rest[..end], end + 2))
        }
        b'$' if syntax != Syntax::Registry => {
            if let Some(braced) = rest.strip_prefix('{') {
                let end = braced.find('}')?;
                return Some((&braced[..end], end + 3));
            }
            let first = *rest.as_bytes().first()?;
            if !first.is_ascii_alphabetic() && first != b'_' {
                return None;
            }
            let end = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
            Some((&rest[..end], end + 1))
        }
        _ => None,
    }
}

fn expand_with(mut text: &str, syntax: Syntax, mut lookup: impl FnMut(&str) -> Option<String>) -> String {
    let original = text;
    let mut out = String::new();
    while let Some(offset) = text.find(['%', '$']) {
        out.push_str(&text[..offset]);
        text = &text[offset..];
        let symbol = text.as_bytes()[0];
        if syntax != Syntax::Registry && text.as_bytes().get(1) == Some(&symbol) {
            out.push(char::from(symbol));
            text = &text[2..];
        } else if let Some((name, end)) = reference(text, syntax) {
            match lookup(name) {
                Some(value) if out.len().saturating_add(value.len()) <= 131072 => out.push_str(&value),
                Some(_) => return original.to_owned(),
                None => out.push_str(&text[..end]),
            }
            text = &text[end..];
        } else {
            out.push(char::from(symbol));
            text = &text[1..];
        }
    }
    out.push_str(text);
    out
}

pub fn expand(value: &str, vars: &Variables, windows: bool) -> String {
    let syntax = Syntax::for_os(windows);
    let vars: BTreeMap<_, _> = vars.iter().map(|(name, value)| (syntax.key(name), value)).collect();
    expand_with(value, syntax, |name| vars.get(&syntax.key(name)).map(|value| (*value).clone()))
}

fn valid(name: &str, value: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0']) && !value.contains('\0')
}

type ValueIndex<'a> = BTreeMap<String, (&'a str, &'a str, bool)>;

struct Resolver<'a> {
    base: BTreeMap<String, &'a str>,
    entries: &'a ValueIndex<'a>,
    done: Variables,
    active: Vec<String>,
    syntax: Syntax,
}

impl Resolver<'_> {
    fn value(&mut self, key: &str) -> Option<String> {
        if let Some(value) = self.done.get(key) {
            return Some(value.clone());
        }
        if self.active.len() >= 32 || self.active.iter().any(|active| active == key) {
            return self.base.get(key).map(|value| (*value).to_owned());
        }
        let Some((_, value, expandable)) = self.entries.get(key).copied() else {
            return self.base.get(key).map(|value| (*value).to_owned());
        };
        let value = if expandable {
            self.active.push(key.to_owned());
            let value = expand_with(value, self.syntax, |name| self.value(&self.syntax.key(name)));
            self.active.pop();
            value
        } else {
            value.to_owned()
        };
        self.done.insert(key.to_owned(), value.clone());
        Some(value)
    }
}

fn overlay<'a>(base: &mut Variables, values: impl IntoIterator<Item = (&'a str, &'a str, bool)>, syntax: Syntax) {
    let entries: ValueIndex<'_> = values.into_iter().filter(|(key, value, _)| valid(key, value))
        .map(|(name, value, expand)| (syntax.key(name), (name, value, expand))).collect();
    let mut resolver = Resolver {
        base: base.iter().map(|(key, value)| (syntax.key(key), value.as_str())).collect(),
        entries: &entries, done: Variables::new(), active: Vec::new(), syntax,
    };
    let resolved: Vec<_> = entries.iter().filter_map(|(key, (name, _, _))| resolver.value(key).map(|v| (*name, v))).collect();
    base.retain(|name, _| !entries.contains_key(&syntax.key(name)));
    base.extend(resolved.into_iter().map(|(name, value)| (name.to_owned(), value)));
}

impl LocalEnvironment {
    pub fn apply(&self, mut base: Variables, overrides: &[(String, String)], windows: bool) -> Variables {
        let syntax = Syntax::for_os(windows);
        if windows && self.set_comspec {
            overlay(&mut base, [("ASTER", "1", false)], syntax);
        }
        let contains = |name: &str| self.global.keys().chain(self.profile.keys()).any(|key| syntax.key(key) == syntax.key(name));
        overlay(&mut base, overrides.iter().filter(|(key, _)| !contains(key)).map(|(k, v)| (k.as_str(), v.as_str(), false)), syntax);
        for layer in [&self.global, &self.profile] {
            overlay(&mut base, layer.iter().map(|(k, v)| (k.as_str(), v.as_str(), true)), syntax);
        }
        base
    }
}

fn path(vars: &Variables) -> Option<(&str, &str)> {
    vars.iter().find(|(k, _)| k.eq_ignore_ascii_case("Path")).map(|(k, v)| (k.as_str(), v.as_str()))
}

pub fn merge_registry(mut process: Variables, machine: &[RegistryValue], user: &[RegistryValue]) -> Variables {
    overlay(&mut process, machine.iter().map(|e| (e.name.as_str(), e.value.as_str(), e.expandable)), Syntax::Registry);
    let system_path = if machine.iter().any(|e| e.name.eq_ignore_ascii_case("Path")) { path(&process).map(|(_, v)| v.to_owned()) } else { None };
    overlay(&mut process, user.iter().map(|e| (e.name.as_str(), e.value.as_str(), e.expandable)), Syntax::Registry);
    if let (Some(system), Some(user_path), Some((name, value))) = (
        system_path.as_deref(), user.iter().find(|e| e.name.eq_ignore_ascii_case("Path")), path(&process),
    ) {
        let refers_to_path = user_path.expandable && user_path.value.to_ascii_uppercase().contains("%PATH%");
        if !system.is_empty() && !refers_to_path {
            let combined = if value.is_empty() { system.to_owned() } else { format!("{system};{value}") };
            process.insert(name.to_owned(), combined);
        }
    }
    process
}

#[cfg(test)]
#[path = "../test/environment.rs"]
mod tests;
