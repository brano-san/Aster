use crate::palette::Palette;
use alacritty_terminal::vte::ansi::Rgb;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/schemes.rs"));

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    pub palette: Palette,
}

pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let (r, g, b) = match h.len() {
        6 => (u8::from_str_radix(&h[0..2], 16).ok()?, u8::from_str_radix(&h[2..4], 16).ok()?, u8::from_str_radix(&h[4..6], 16).ok()?),
        3 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
            (d(0)?, d(1)?, d(2)?)
        }
        _ => return None,
    };
    Some(Rgb { r, g, b })
}

pub fn from_colors(foreground: Rgb, background: Rgb, cursor: Option<Rgb>, colors: &[Rgb]) -> Option<Palette> {
    if colors.len() < 8 {
        return None;
    }
    let mut ansi = [Rgb { r: 0, g: 0, b: 0 }; 16];
    for (i, slot) in ansi.iter_mut().enumerate() {
        *slot = colors.get(i).copied().unwrap_or(colors[i % 8]);
    }
    let base = Palette::DARK;
    Some(Palette { foreground, background, cursor: cursor.unwrap_or(foreground), selection: mix(background, ansi[4], 0.35), ansi, ..base })
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb { r: f(a.r, b.r), g: f(a.g, b.g), b: f(a.b, b.b) }
}

pub fn parse_xresources(text: &str) -> Option<Palette> {
    let mut vars = HashMap::new();
    let mut values = HashMap::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#define") {
            let mut parts = rest.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                vars.insert(k.to_owned(), v.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix("*.").or_else(|| line.strip_prefix('*')) {
            if let Some((k, v)) = rest.split_once(':').or_else(|| rest.split_once(char::is_whitespace)) {
                let v = v.trim();
                values.insert(k.trim().to_owned(), vars.get(v).cloned().unwrap_or_else(|| v.to_owned()));
            }
        }
    }
    let get = |k: &str| values.get(k).and_then(|v| parse_hex(v));
    let colors: Vec<Rgb> = (0..).map_while(|i| get(&format!("color{i}"))).take(16).collect();
    from_colors(get("foreground")?, get("background")?, get("cursorColor"), &colors)
}

pub fn builtin() -> Vec<Scheme> {
    let mut out = vec![Scheme { name: "Aster Dark".into(), palette: Palette::DARK }, Scheme { name: "Aster Light".into(), palette: Palette::LIGHT }];
    out.extend(COMMUNITY.iter().filter_map(|(name, text)| parse_xresources(text).map(|palette| Scheme { name: (*name).to_owned(), palette })));
    out
}
