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
    pub divider: Color,
    pub button_hover: Color,
    pub button_press: Color,
    pub close_hover: Color,
    pub overlay: Color,
    pub overlay_border: Color,
    pub selection: Color,
    pub shadow: Color,
    pub dim: Color,
}

pub fn color(c: Rgb) -> Color {
    Color::rgb((c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Rgb { r: f(a.r, b.r), g: f(a.g, b.g), b: f(a.b, b.b) }
}

fn luminance(c: Rgb) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

impl Theme {
    pub fn from_palette(p: &Palette) -> Theme {
        let bg = p.background;
        let fg = p.foreground;
        let dark = luminance(bg) < 0.4;
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb { r: 255, g: 255, b: 255 };
        let chrome = if dark { mix(bg, black, 0.35) } else { mix(bg, black, 0.07) };
        let overlay = if dark { mix(bg, white, 0.06) } else { bg };
        let accent = p.ansi[4];
        let shade = if dark { white } else { black };
        Theme {
            dark,
            background: color(bg),
            foreground: color(fg),
            chrome: color(chrome),
            tab_active: color(bg),
            tab_hover: color(mix(chrome, shade, 0.07)),
            text: color(fg),
            text_dim: color(mix(fg, bg, 0.45)),
            accent: color(accent),
            divider: color(mix(bg, shade, if dark { 0.14 } else { 0.18 })),
            button_hover: color(shade).alpha(if dark { 0x1A } else { 0x14 }),
            button_press: color(shade).alpha(if dark { 0x2A } else { 0x22 }),
            close_hover: Color::rgb(0xC42B1C),
            overlay: color(overlay),
            overlay_border: color(mix(overlay, shade, 0.12)),
            selection: color(accent).alpha(0x40),
            shadow: Color::rgba(0x0000_0000).alpha(if dark { 0x90 } else { 0x40 }),
            dim: color(bg).alpha(0x40),
        }
    }
}
