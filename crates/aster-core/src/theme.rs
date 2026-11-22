use aster_gfx::render::Color;
use aster_vt::{Palette, Rgb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: Color,
    pub foreground: Color,
    pub chrome: Color,
    pub tab_active: Color,
    pub tab_hover: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub divider: Color,
    pub button_hover: Color,
    pub button_press: Color,
    pub close_hover: Color,
    pub overlay: Color,
    pub overlay_border: Color,
    pub selection: Color,
    pub shadow: Color,
    pub dim: Color,
    pub surface: Color,
}

pub fn color(c: Rgb) -> Color {
    Color::rgb((c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32)
}

pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Rgb { r: f(a.r, b.r), g: f(a.g, b.g), b: f(a.b, b.b) }
}

fn luminance(c: Rgb) -> f32 {
    static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let ch = LINEAR.get_or_init(|| std::array::from_fn(|v| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }));
    0.2126 * ch[c.r as usize] + 0.7152 * ch[c.g as usize] + 0.0722 * ch[c.b as usize]
}

fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

pub fn ensure_contrast(fg: Rgb, bg: Rgb, ratio: f32) -> Rgb {
    if ratio <= 1.0 || !ratio.is_finite() || contrast_ratio(fg, bg) >= ratio {
        return fg;
    }
    let black = Rgb { r: 0, g: 0, b: 0 };
    let white = Rgb { r: 255, g: 255, b: 255 };
    let target = if contrast_ratio(white, bg) > contrast_ratio(black, bg) { white } else { black };
    let (mut lo, mut hi) = (0, 255);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if contrast_ratio(mix(fg, target, mid as f32 / 255.0), bg) >= ratio {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    mix(fg, target, hi as f32 / 255.0)
}

impl Theme {
    pub fn from_palette(p: &Palette) -> Theme {
        Self::build(p, true)
    }

    pub(crate) fn fixed(p: &Palette, dark: bool) -> Theme {
        let mut theme = Self::build(if dark { &Palette::DARK } else { &Palette::LIGHT }, false);
        theme.background = color(p.background);
        theme.foreground = color(p.foreground);
        theme
    }

    fn build(p: &Palette, correct: bool) -> Theme {
        let bg = p.background;
        let fg = p.foreground;
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb { r: 255, g: 255, b: 255 };
        let dark = if correct { contrast_ratio(white, bg) > contrast_ratio(black, bg) } else { luminance(bg) < 0.4 };
        let shade = if dark { white } else { black };
        let surface_color = |rgb| if correct { ensure_contrast(rgb, shade, 12.0) } else { rgb };
        let surface = surface_color(bg);
        let chrome = surface_color(mix(surface, black, if dark { 0.35 } else { 0.07 }));
        let overlay = if dark { surface_color(mix(surface, white, 0.06)) } else { surface };
        let hover = surface_color(mix(chrome, shade, 0.07));
        let limiting = [surface, chrome, overlay, hover].into_iter().map(|surface| mix(surface, shade, 80.0 / 255.0))
            .min_by(|&a, &b| contrast_ratio(shade, a).total_cmp(&contrast_ratio(shade, b))).unwrap_or(surface);
        let readable = |ink| if correct { ensure_contrast(ink, limiting, 4.5) } else { ink };
        let accent = readable(p.ansi[4]);
        Theme {
            dark,
            background: color(bg),
            foreground: color(fg),
            chrome: color(chrome),
            tab_active: color(surface),
            tab_hover: color(hover),
            text: color(readable(fg)),
            text_dim: color(readable(mix(fg, surface, 0.45))),
            accent: color(accent),
            on_accent: color(ensure_contrast(surface, accent, 4.5)),
            divider: color(mix(surface, shade, if dark { 0.14 } else { 0.18 })),
            button_hover: color(shade).alpha(if dark { 0x1A } else { 0x14 }),
            button_press: color(shade).alpha(if dark { 0x2A } else { 0x22 }),
            close_hover: Color::rgb(0xC42B1C),
            overlay: color(overlay),
            overlay_border: color(mix(overlay, shade, 0.12)),
            selection: color(accent).alpha(0x40),
            shadow: Color::rgba(0x0000_0000).alpha(if dark { 0x90 } else { 0x40 }),
            dim: color(bg).alpha(0x40),
            surface: color(surface),
        }
    }
}

#[cfg(test)]
#[path = "../test/theme.rs"]
mod tests;
