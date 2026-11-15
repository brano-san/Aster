const LINES: [&str; 128] = [
    "0101", "0202", "1010", "2020", "0101", "0202", "1010", "2020", "0101", "0202", "1010", "2020", "0110", "0210", "0120", "0220", "0011", "0012", "0021", "0022", "1100", "1200", "2100", "2200", "1001", "1002",
    "2001", "2002", "1110", "1210", "2110", "1120", "2120", "2210", "1220", "2220", "1011", "1012", "2011", "1021", "2021", "2012", "1022", "2022", "0111", "0112", "0211", "0212", "0121", "0122", "0221", "0222",
    "1101", "1102", "1201", "1202", "2101", "2102", "2201", "2202", "1111", "1112", "1211", "1212", "2111", "1121", "2121", "2112", "2211", "1122", "1221", "2212", "1222", "2122", "2221", "2222", "0101", "0202",
    "1010", "2020", "0303", "3030", "0310", "0130", "0330", "0013", "0031", "0033", "1300", "3100", "3300", "1003", "3001", "3003", "1310", "3130", "3330", "1013", "3031", "3033", "0313", "0131", "0333", "1303",
    "3101", "3303", "1313", "3131", "3333", "r", "r", "r", "r", "d", "d", "d", "0001", "1000", "0100", "0010", "0002", "2000", "0200", "0020", "0201", "1020", "0102", "2010",
];

pub fn is_procedural(c: char) -> bool {
    match c as u32 {
        code @ 0x2500..=0x257F => !LINES[(code - 0x2500) as usize].contains('3'),
        0x2580..=0x259F | 0xE0B0..=0xE0B3 => true,
        _ => false,
    }
}

struct Mask {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

impl Mask {
    fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, a: u8) {
        let (xa, xb) = (x0.round().max(0.0) as usize, (x1.round().max(0.0) as usize).min(self.w));
        let (ya, yb) = (y0.round().max(0.0) as usize, (y1.round().max(0.0) as usize).min(self.h));
        for y in ya..yb {
            for x in xa..xb {
                let p = &mut self.data[y * self.w + x];
                *p = (*p).max(a);
            }
        }
    }

    fn shade(&mut self, f: impl Fn(f32, f32) -> f32) {
        for y in 0..self.h {
            for x in 0..self.w {
                let v = (f(x as f32 + 0.5, y as f32 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                let p = &mut self.data[y * self.w + x];
                *p = (*p).max(v);
            }
        }
    }
}

fn arms(m: &mut Mask, spec: &[u8], light: f32) {
    let (w, h) = (m.w as f32, m.h as f32);
    let heavy = (light * 2.0).max(light + 1.0);
    let thickness = |weight: u8| if weight == b'2' { heavy } else { light };
    let widest = spec.iter().filter(|&&s| s != b'0').map(|&s| thickness(s)).fold(0.0, f32::max);
    let (cx, cy) = ((w / 2.0).floor(), (h / 2.0).floor());
    let start = |t: f32| (t / 2.0).floor();
    let (near, far) = (start(widest), widest - start(widest));
    for (arm, &weight) in spec.iter().enumerate() {
        if weight == b'0' {
            continue;
        }
        let t = thickness(weight);
        let s = start(t);
        match arm {
            0 => m.fill(cx - s, 0.0, cx - s + t, cy + far, 255),
            1 => m.fill(cx - near, cy - s, w, cy - s + t, 255),
            2 => m.fill(cx - s, cy - near, cx - s + t, h, 255),
            _ => m.fill(0.0, cy - s, cx + far, cy - s + t, 255),
        }
    }
}

fn rounded(m: &mut Mask, c: u32, light: f32) {
    let (w, h) = (m.w as f32, m.h as f32);
    let (cx, cy) = ((w / 2.0).floor() + light / 2.0 - (light / 2.0).floor(), (h / 2.0).floor() + light / 2.0 - (light / 2.0).floor());
    let r = (w / 2.0).min(h / 2.0).max(1.0);
    let (sx, sy) = match c {
        0x256D => (1.0, 1.0),
        0x256E => (-1.0, 1.0),
        0x256F => (-1.0, -1.0),
        _ => (1.0, -1.0),
    };
    let (ox, oy) = (cx + sx * r, cy + sy * r);
    m.shade(|x, y| {
        let (dx, dy) = ((x - ox) * -sx, (y - oy) * -sy);
        if dx >= 0.0 && dy >= 0.0 {
            let d = (dx * dx + dy * dy).sqrt();
            light / 2.0 + 0.5 - (d - r).abs()
        } else if dx < 0.0 && dy >= 0.0 {
            light / 2.0 + 0.5 - (y - cy).abs() - if (x - cx) * sx < 0.0 { 9.0 } else { 0.0 }
        } else if dy < 0.0 && dx >= 0.0 {
            light / 2.0 + 0.5 - (x - cx).abs() - if (y - cy) * sy < 0.0 { 9.0 } else { 0.0 }
        } else {
            0.0
        }
    });
}

fn diagonal(m: &mut Mask, c: u32, light: f32) {
    let (w, h) = (m.w as f32, m.h as f32);
    let len = (w * w + h * h).sqrt();
    let rising = |x: f32, y: f32| light / 2.0 + 0.5 - ((h - y) * w - x * h).abs() / len;
    let falling = |x: f32, y: f32| light / 2.0 + 0.5 - (y * w - x * h).abs() / len;
    match c {
        0x2571 => m.shade(rising),
        0x2572 => m.shade(falling),
        _ => m.shade(|x, y| rising(x, y).max(falling(x, y))),
    }
}

fn blocks(m: &mut Mask, c: u32) {
    let (w, h) = (m.w as f32, m.h as f32);
    let e = |n: f32| (n / 8.0);
    match c {
        0x2580 => m.fill(0.0, 0.0, w, h / 2.0, 255),
        0x2581..=0x2588 => m.fill(0.0, h * (1.0 - e((c - 0x2580) as f32)), w, h, 255),
        0x2589..=0x258F => m.fill(0.0, 0.0, w * e((0x2590 - c) as f32), h, 255),
        0x2590 => m.fill(w / 2.0, 0.0, w, h, 255),
        0x2591 => m.fill(0.0, 0.0, w, h, 64),
        0x2592 => m.fill(0.0, 0.0, w, h, 128),
        0x2593 => m.fill(0.0, 0.0, w, h, 192),
        0x2594 => m.fill(0.0, 0.0, w, h * e(1.0), 255),
        0x2595 => m.fill(w * e(7.0), 0.0, w, h, 255),
        _ => {
            let quads: &[(bool, bool)] = match c {
                0x2596 => &[(false, true)],
                0x2597 => &[(true, true)],
                0x2598 => &[(false, false)],
                0x2599 => &[(false, false), (false, true), (true, true)],
                0x259A => &[(false, false), (true, true)],
                0x259B => &[(false, false), (true, false), (false, true)],
                0x259C => &[(false, false), (true, false), (true, true)],
                0x259D => &[(true, false)],
                0x259E => &[(true, false), (false, true)],
                _ => &[(true, false), (false, true), (true, true)],
            };
            for &(right, bottom) in quads {
                let (x0, y0) = (if right { w / 2.0 } else { 0.0 }, if bottom { h / 2.0 } else { 0.0 });
                m.fill(x0, y0, x0 + w / 2.0, y0 + h / 2.0, 255);
            }
        }
    }
}

fn powerline(m: &mut Mask, c: u32, light: f32) {
    let (w, h) = (m.w as f32, m.h as f32);
    let mid = h / 2.0;
    match c {
        0xE0B0 => m.shade(|x, y| (w * (1.0 - (y - mid).abs() / mid) - x + 0.5).min(1.0)),
        0xE0B2 => m.shade(|x, y| (x - w * (y - mid).abs() / mid + 0.5).min(1.0)),
        _ => {
            let flip = c == 0xE0B3;
            let len = (w * w + mid * mid).sqrt();
            m.shade(|x, y| {
                let x = if flip { w - x } else { x };
                let d = ((y - mid).abs() * w - (w - x) * mid).abs() / len;
                light / 2.0 + 0.5 - d
            });
        }
    }
}

pub fn rasterize(c: char, w: u32, h: u32) -> Option<Vec<u8>> {
    if !is_procedural(c) || w == 0 || h == 0 {
        return None;
    }
    let mut m = Mask { w: w as usize, h: h as usize, data: vec![0; (w * h) as usize] };
    let light = (w as f32 / 8.0).round().max(1.0);
    let code = c as u32;
    match code {
        0x2500..=0x257F => match LINES[(code - 0x2500) as usize] {
            "r" => rounded(&mut m, code, light),
            "d" => diagonal(&mut m, code, light),
            spec => arms(&mut m, spec.as_bytes(), light),
        },
        0x2580..=0x259F => blocks(&mut m, code),
        _ => powerline(&mut m, code, light),
    }
    Some(m.data)
}

#[cfg(test)]
#[path = "../test/boxdraw.rs"]
mod tests;
