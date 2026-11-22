use crate::{Palette, Rgb};

#[derive(Clone, Copy)]
struct Lab([f64; 3]);

impl Lab {
    fn from_rgb(rgb: Rgb) -> Self {
        let [r, g, b] = [rgb.r, rgb.g, rgb.b].map(|v| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        });
        let f = |v: f64| if v > 216.0 / 24389.0 { v.cbrt() } else { (841.0 / 108.0) * v + 4.0 / 29.0 };
        let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
        let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
        let z = f((0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883);
        Self([116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)])
    }

    fn rgb(self) -> Rgb {
        let [l, a, b] = self.0;
        let y = (l + 16.0) / 116.0;
        let f = |v: f64| if v > 6.0 / 29.0 { v * v * v } else { (108.0 / 841.0) * (v - 4.0 / 29.0) };
        let (x, y, z) = (f(y + a / 500.0) * 0.95047, f(y), f(y - b / 200.0) * 1.08883);
        let channel = |v: f64| {
            let srgb = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        Rgb {
            r: channel(3.2404542 * x - 1.5371385 * y - 0.4985314 * z),
            g: channel(-0.9692660 * x + 1.8760108 * y + 0.0415560 * z),
            b: channel(0.0556434 * x - 0.2040259 * y + 1.0572252 * z),
        }
    }

    fn mix(self, other: Self, t: f64) -> Self {
        Self(std::array::from_fn(|i| self.0[i] + (other.0[i] - self.0[i]) * t))
    }
}

pub(crate) fn generate(palette: &Palette, harmonious: bool) -> [Rgb; 240] {
    let background = Lab::from_rgb(palette.background);
    let foreground = Lab::from_rgb(palette.foreground);
    let invert = harmonious && background.0[0] > foreground.0[0];
    let mut corners = [0, 4, 2, 6, 1, 5, 3, 7].map(|i| Lab::from_rgb(palette.ansi[i]));
    let (start, end) = if invert {
        let first = foreground.0[0];
        let last = background.0[0];
        for color in &mut corners[1..7] {
            color.0[0] = (first + last - color.0[0]).clamp(first, last);
        }
        (foreground, background)
    } else {
        (background, foreground)
    };
    corners[0] = start.mix(corners[0], 0.20);
    corners[7] = end.mix(corners[7], 0.20);
    std::array::from_fn(|i| {
        if i >= 216 {
            return start.mix(end, (i - 215) as f64 / 25.0).rgb();
        }
        let r = (i / 36) as f64 / 5.0;
        let g = ((i / 6) % 6) as f64 / 5.0;
        let b = (i % 6) as f64 / 5.0;
        let low = corners[0].mix(corners[1], b).mix(corners[2].mix(corners[3], b), g);
        let high = corners[4].mix(corners[5], b).mix(corners[6].mix(corners[7], b), g);
        low.mix(high, r).rgb()
    })
}
