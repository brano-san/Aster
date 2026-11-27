#![cfg(target_os = "linux")]

use aster_core::app::App;
use aster_core::config::Config;
use aster_core::profile::Profile;
use aster_gfx::text::Gfx;
use aster_platform_linux::{GlDevice, GlRenderer, LinuxText};
use aster_vt::fixture::{Chunking, Fixture};
use std::sync::Arc;

fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w as usize * 3 + 1) * h as usize);
    for y in (0..h as usize).rev() {
        raw.push(0);
        raw.extend(rgba[y * w as usize * 4..(y + 1) * w as usize * 4].chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(0xFFFF).collect();
    for (i, b) in blocks.iter().enumerate() {
        let n = b.len() as u16;
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (a, b) = raw.iter().fold((1u32, 0u32), |(a, b), &x| ((a + x as u32) % 65521, (b + a + x as u32) % 65521));
    z.extend(((b << 16) | a).to_be_bytes());
    let crc = |data: &[u8]| {
        let mut c = !0u32;
        for &byte in data {
            c ^= byte as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let body = [kind, data].concat();
        out.extend_from_slice(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    chunk(b"IHDR", &[&w.to_be_bytes()[..], &h.to_be_bytes(), &[8, 2, 0, 0, 0]].concat());
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn render(app: &mut App, gfx: &mut Gfx<LinuxText>, renderer: &mut GlRenderer) -> Vec<u8> {
    app.woken();
    app.frame(gfx, false);
    for (slot, offset) in &gfx.uploads.rects {
        let len = (slot.w * slot.h * 4) as usize;
        renderer.upload(*slot, &gfx.uploads.data[*offset..*offset + len]);
    }
    gfx.uploads.clear();
    renderer.render(&gfx.dl).expect("render");
    renderer.read_pixels()
}

#[test]
fn a_frame_with_text_colors_and_box_lines_renders_offscreen() {
    let Ok(device) = GlDevice::headless() else {
        eprintln!("no EGL device, skipping");
        return;
    };
    let (w, h) = (900, 420);
    let mut renderer = GlRenderer::offscreen(&device, (w, h), 2048).expect("offscreen renderer");
    let mut gfx = Gfx::new(LinuxText::new(), 2048);
    let mut app = App::new(Config::default(), vec![Profile::new("local:sh", "sh", "/bin/sh", &[])], true, None, Arc::new(|| {}));
    app.set_viewport(w, h, 1.0);
    app.open_default_tab();
    app.frame(&mut gfx, false);
    let terminal = app.focused_terminal().expect("terminal");
    terminal.lock().feed("\x1b]0;aster linux\x07$ ls --color\r\n\x1b[1;34mdir\x1b[0m  \x1b[32mscript.sh\x1b[0m  \x1b[3mitalic\x1b[0m  \x1b[4munderline\x1b[0m  \x1b[4:3mcurl\x1b[0m\r\n┌──┬──┐ ╭──╮ █▓▒░ \u{E0B0}\r\n│ab│cd│ ╰──╯ Привет 日本\r\n└──┴──┘\r\n$ ".as_bytes());
    let pixels = render(&mut app, &mut gfx, &mut renderer);
    assert!(pixels.chunks_exact(4).any(|p| p[0] != pixels[0] || p[1] != pixels[1]), "the frame is not blank");
    if let Some(path) = std::env::var_os("ASTER_RENDER_PNG") {
        std::fs::write(path, png(w, h, &pixels)).unwrap();
    }
}

#[test]
fn replay_fixture_renders_the_same_pixels_across_read_boundaries() {
    let Ok(device) = GlDevice::headless() else {
        eprintln!("no EGL device, skipping");
        return;
    };
    let (w, h) = (480, 260);
    let mut renderer = GlRenderer::offscreen(&device, (w, h), 2048).expect("offscreen renderer");
    let mut gfx = Gfx::new(LinuxText::new(), 2048);
    let mut app = App::new(Config::default(), vec![Profile::new("local:sh", "sh", "/bin/sh", &[])], true, None, Arc::new(|| {}));
    app.set_system_animations(false);
    app.set_viewport(w, h, 1.0);
    app.open_default_tab();
    let terminal = app.focused_terminal().expect("terminal");
    let blank = render(&mut app, &mut gfx, &mut renderer);
    let fixture = Fixture::from_json(include_bytes!("../../aster-vt/test/fixtures/reflow.json")).unwrap();
    let mut expected = None;
    for chunking in Chunking::ALL {
        *terminal.lock() = fixture.verify_and_replay(chunking).unwrap();
        let pixels = render(&mut app, &mut gfx, &mut renderer);
        if let Some(expected) = &expected {
            assert!(pixels == *expected, "frame pixels differ for {chunking:?}");
        } else {
            assert!(pixels != blank, "fixture output must draw visible text");
            expected = Some(pixels);
        }
    }
}
