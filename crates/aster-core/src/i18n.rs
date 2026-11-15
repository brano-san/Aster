use std::collections::HashMap;
use std::sync::OnceLock;

const EN: u8 = 0;
const RU: u8 = 1;

pub const RU_CATALOG: &str = include_str!("../locales/ru.toml");

#[cfg(not(test))]
mod current {
    use std::sync::atomic::{AtomicU8, Ordering};

    static LANGUAGE: AtomicU8 = AtomicU8::new(super::EN);

    pub fn get() -> u8 {
        LANGUAGE.load(Ordering::Relaxed)
    }

    pub fn put(value: u8) {
        LANGUAGE.store(value, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod current {
    use std::cell::Cell;

    thread_local! {
        static LANGUAGE: Cell<u8> = const { Cell::new(super::EN) };
    }

    pub fn get() -> u8 {
        LANGUAGE.with(Cell::get)
    }

    pub fn put(value: u8) {
        LANGUAGE.with(|l| l.set(value));
    }
}

pub fn set_language(code: &str) {
    current::put(if code == "ru" { RU } else { EN });
}

pub fn language() -> &'static str {
    match current::get() {
        RU => "ru",
        _ => "en",
    }
}

pub fn resolve(setting: &str, system: &str) -> &'static str {
    match setting.trim().to_ascii_lowercase().as_str() {
        "en" => "en",
        "ru" => "ru",
        _ if system.trim().to_ascii_lowercase().starts_with("ru") => "ru",
        _ => "en",
    }
}

fn parse(text: &str) -> HashMap<&'static str, &'static str> {
    let table: toml::Table = toml::from_str(text).unwrap_or_default();
    table
        .into_iter()
        .filter_map(|(k, v)| match v {
            toml::Value::String(s) => Some((&*Box::leak(k.into_boxed_str()), &*Box::leak(s.into_boxed_str()))),
            _ => None,
        })
        .collect()
}

fn russian() -> &'static HashMap<&'static str, &'static str> {
    static CATALOG: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(RU_CATALOG))
}

pub fn t(text: &'static str) -> &'static str {
    match current::get() {
        RU => russian().get(text).copied().unwrap_or(text),
        _ => text,
    }
}

pub fn tf(text: &'static str, args: &[(&str, &str)]) -> String {
    let mut rest = t(text);
    let mut out = String::with_capacity(rest.len());
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let value = tail.find('}').and_then(|close| args.iter().find(|(name, _)| *name == &tail[1..close]).map(|(_, v)| (close, *v)));
        match value {
            Some((close, v)) => {
                out.push_str(v);
                rest = &tail[close + 1..];
            }
            None => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "../test/i18n.rs"]
mod tests;
