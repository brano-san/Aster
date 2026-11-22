use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Option<Rgb>,
    pub selection: Rgb,
    pub selection_text: Option<Rgb>,
    pub ansi: [Rgb; 16],
    pub bold_is_bright: bool,
    pub(crate) extended: Option<[Rgb; 240]>,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8 }
}

impl Palette {
    pub const DARK: Palette = Palette {
        foreground: rgb(0xcacaca),
        background: rgb(0x171717),
        cursor: rgb(0xbbbbbb),
        cursor_text: None,
        selection: rgb(0x3a4a66),
        selection_text: None,
        ansi: [
            rgb(0x000000),
            rgb(0xff615a),
            rgb(0xb1e969),
            rgb(0xebd99c),
            rgb(0x5da9f6),
            rgb(0xe86aff),
            rgb(0x82fff7),
            rgb(0xdedacf),
            rgb(0x313131),
            rgb(0xf58c80),
            rgb(0xddf88f),
            rgb(0xeee5b2),
            rgb(0xa5c7ff),
            rgb(0xddaaff),
            rgb(0xb7fff9),
            rgb(0xffffff),
        ],
        bold_is_bright: true,
        extended: None,
    };

    pub const LIGHT: Palette = Palette {
        foreground: rgb(0x4d4d4c),
        background: rgb(0xffffff),
        cursor: rgb(0x4d4d4c),
        cursor_text: None,
        selection: rgb(0xc8d8f0),
        selection_text: None,
        ansi: [
            rgb(0x000000),
            rgb(0xc82829),
            rgb(0x718c00),
            rgb(0xeab700),
            rgb(0x4271ae),
            rgb(0x8959a8),
            rgb(0x3e999f),
            rgb(0xffffff),
            rgb(0x000000),
            rgb(0xc82829),
            rgb(0x718c00),
            rgb(0xeab700),
            rgb(0x4271ae),
            rgb(0x8959a8),
            rgb(0x3e999f),
            rgb(0xffffff),
        ],
        bold_is_bright: true,
        extended: None,
    };

    pub fn with_extended_colors(mut self, harmonious: bool) -> Self {
        self.extended = Some(crate::indexed_colors::generate(&self, harmonious));
        self
    }

    pub fn indexed(&self, i: u8) -> Rgb {
        if i >= 16 {
            if let Some(colors) = &self.extended {
                return colors[usize::from(i - 16)];
            }
        }
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let i = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
                Rgb { r: level(i / 36), g: level((i / 6) % 6), b: level(i % 6) }
            }
            _ => {
                let v = (i - 232) * 10 + 8;
                Rgb { r: v, g: v, b: v }
            }
        }
    }

    fn named(&self, c: NamedColor, overrides: &Colors) -> Rgb {
        if let Some(rgb) = overrides[c] {
            return rgb;
        }
        let n = c as usize;
        match c {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::DimForeground => dim(self.foreground),
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            _ if n < 16 => self.ansi[n],
            _ => dim(self.ansi[(n - NamedColor::DimBlack as usize) & 7]),
        }
    }

    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Named(c) => self.named(c, overrides),
            Color::Indexed(i) => overrides[i as usize].unwrap_or_else(|| self.indexed(i)),
        }
    }

    pub fn background(&self, overrides: &Colors) -> Rgb {
        self.named(NamedColor::Background, overrides)
    }

    pub fn cell(&self, cell: &Cell, overrides: &Colors) -> (Rgb, Rgb) {
        let flags = cell.flags;
        let mut fg_color = cell.fg;
        if self.bold_is_bright && flags.contains(Flags::BOLD) && !flags.contains(Flags::DIM) {
            fg_color = match fg_color {
                Color::Named(c) if (c as usize) < 8 => Color::Named(c.to_bright()),
                Color::Indexed(i) if i < 8 => Color::Indexed(i + 8),
                other => other,
            };
        }
        let mut fg = self.resolve(fg_color, overrides);
        let mut bg = self.resolve(cell.bg, overrides);
        if flags.contains(Flags::DIM) {
            fg = dim(fg);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        (fg, bg)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::DARK
    }
}

fn dim(c: Rgb) -> Rgb {
    let f = |v: u8| (v as u16 * 2 / 3) as u8;
    Rgb { r: f(c.r), g: f(c.g), b: f(c.b) }
}
