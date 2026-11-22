use crate::atlas::{Atlas, Slot};
use crate::hash::{self, FxHashMap};
use crate::render::{Color, DrawList, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FontFamily {
    Ui,
    Icons,
    Mono,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FontStyle {
    pub family: FontFamily,
    pub size_q: u16,
    pub bold: bool,
    pub italic: bool,
    pub ligatures: bool,
}

impl FontStyle {
    pub fn new(family: FontFamily, px: f32, bold: bool) -> Self {
        Self { family, size_q: (px * 4.0).round() as u16, bold, italic: false, ligatures: false }
    }
    pub fn italic(self, italic: bool) -> Self {
        Self { italic, ..self }
    }
    pub fn px(&self) -> f32 {
        self.size_q as f32 / 4.0
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShapedGlyph {
    pub font: u16,
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub cluster: u32,
    pub cluster_x: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ShapedText {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GlyphKey {
    pub font: u16,
    pub id: u16,
    pub size_q: u16,
    pub subpx: u8,
}

#[derive(Clone, Debug, Default)]
pub struct GlyphBitmap {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub color: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
}

pub trait TextBackend {
    fn shape(&mut self, text: &str, style: FontStyle, out: &mut ShapedText);
    fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool;
    fn metrics(&mut self, style: FontStyle) -> FontMetrics;
    fn set_ui_family(&mut self, name: &str) -> bool {
        name.is_empty()
    }
    fn families(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn set_mono_family(&mut self, name: &str) -> bool {
        name.is_empty()
    }
    fn set_mono_font(&mut self, name: &str, _weights: [u16; 2]) -> bool {
        self.set_mono_family(name)
    }
    fn set_fallback_family(&mut self, name: &str) -> bool {
        name.is_empty()
    }
    fn set_font_waker(&mut self, _wake: std::sync::Arc<dyn Fn() + Send + Sync>) {}
    fn poll_font_change(&mut self) -> bool {
        false
    }
}

#[derive(Default)]
pub struct Uploads {
    pub rects: Vec<(Slot, usize)>,
    pub data: Vec<u8>,
}

impl Uploads {
    pub fn clear(&mut self) {
        self.rects.clear();
        self.data.clear();
    }
}

#[derive(Clone, Copy)]
struct GlyphSlot {
    slot: Slot,
    last_used: u64,
    left: i32,
    top: i32,
    color: bool,
}

struct Image {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    slot: Option<Slot>,
}

type Shapes = FxHashMap<u64, (Box<str>, ShapedText)>;

const ELLIPSIS: &str = "…";
const MAX_SHAPES: usize = 50_000;
const BACKGROUND_IMAGE: u64 = u64::MAX;

pub struct Gfx<B: TextBackend> {
    pub text: B,
    pub dl: DrawList,
    pub uploads: Uploads,
    atlas: Atlas,
    shapes: Shapes,
    glyphs: FxHashMap<GlyphKey, Option<GlyphSlot>>,
    images: FxHashMap<u64, Image>,
    background_revision: Option<u64>,
    metrics: FxHashMap<FontStyle, FontMetrics>,
    scratch: GlyphBitmap,
    ui_family: String,
    ui_found: bool,
    mono_family: String,
    mono_weights: Option<[u16; 2]>,
    fallback_family: String,
}

impl<B: TextBackend> Gfx<B> {
    pub fn new(text: B, atlas_size: u32) -> Self {
        Self {
            text,
            dl: DrawList::default(),
            uploads: Uploads::default(),
            atlas: Atlas::new(atlas_size),
            shapes: Default::default(),
            glyphs: Default::default(),
            images: Default::default(),
            background_revision: None,
            metrics: Default::default(),
            scratch: GlyphBitmap::default(),
            ui_family: String::new(),
            ui_found: true,
            mono_family: String::new(),
            mono_weights: None,
            fallback_family: String::new(),
        }
    }

    pub fn set_mono_family(&mut self, name: &str) {
        self.set_mono_font(name, self.mono_weights.unwrap_or([400, 700]));
    }

    pub fn set_mono_font(&mut self, name: &str, weights: [u16; 2]) {
        let weights = weights.map(|weight| (weight.clamp(100, 900) + 50) / 100 * 100);
        if self.mono_family != name || self.mono_weights != Some(weights) {
            self.text.set_mono_font(name, weights);
            name.clone_into(&mut self.mono_family);
            self.mono_weights = Some(weights);
            self.shapes.clear();
            self.metrics.clear();
        }
    }

    pub fn set_ui_family(&mut self, name: &str) -> bool {
        if self.ui_family != name {
            self.ui_found = self.text.set_ui_family(name);
            self.ui_family = name.to_owned();
            self.shapes.clear();
            self.metrics.clear();
        }
        self.ui_found
    }

    pub fn set_fallback_family(&mut self, name: &str) {
        if self.fallback_family != name {
            self.text.set_fallback_family(name);
            name.clone_into(&mut self.fallback_family);
            self.shapes.clear();
        }
        if self.text.poll_font_change() {
            self.shapes.clear();
            self.metrics.clear();
        }
    }

    pub fn begin(&mut self, clear: Color) {
        self.dl.reset(clear);
        self.atlas.begin_frame();
        if self.shapes.len() > MAX_SHAPES {
            self.shapes.clear();
        }
    }

    pub fn with_transient_text(&mut self, draw: impl FnOnce(&mut Self)) {
        let cached = std::mem::take(&mut self.shapes);
        draw(self);
        self.shapes = cached;
    }

    pub fn reset_atlas(&mut self) {
        self.atlas.clear();
        self.glyphs.clear();
        for image in self.images.values_mut() {
            image.slot = None;
        }
        self.uploads.clear();
    }

    fn split(&mut self) -> (&mut Shapes, GlyphCtx<'_, B>) {
        let Self { shapes, text, glyphs, images, atlas, uploads, dl, scratch, .. } = self;
        (shapes, GlyphCtx { text, glyphs, images, atlas, uploads, dl, scratch })
    }

    pub fn metrics(&mut self, style: FontStyle) -> FontMetrics {
        let text = &mut self.text;
        *self.metrics.entry(style).or_insert_with(|| text.metrics(style))
    }

    pub fn shape(&mut self, s: &str, style: FontStyle) -> &ShapedText {
        shape(&mut self.shapes, &mut self.text, s, style)
    }

    pub fn measure(&mut self, s: &str, style: FontStyle) -> f32 {
        self.shape(s, style).width
    }

    pub fn caret_x(&mut self, s: &str, style: FontStyle, idx: usize) -> f32 {
        let t = self.shape(s, style);
        t.glyphs.iter().find(|g| g.cluster as usize >= idx).map_or(t.width, |g| g.x)
    }

    pub fn hit(&mut self, s: &str, style: FontStyle, x: f32) -> usize {
        let t = self.shape(s, style);
        let mut best = (s.len(), (t.width - x).abs());
        for g in &t.glyphs {
            let d = (g.x - x).abs();
            if d < best.1 {
                best = (g.cluster as usize, d);
            }
        }
        best.0
    }

    pub fn text(&mut self, x: f32, y: f32, s: &str, style: FontStyle, color: Color, max_w: f32) -> f32 {
        let (shapes, mut ctx) = self.split();
        let width = shape(shapes, ctx.text, s, style).width;
        if width <= max_w {
            let t = &shapes[&key(s, style)].1;
            for g in &t.glyphs {
                ctx.draw(x + g.x, y + g.y, g, style, color);
            }
            return width;
        }
        let ell_w = shape(shapes, ctx.text, ELLIPSIS, style).width;
        let avail = max_w - ell_w;
        let t = &shapes[&key(s, style)].1;
        let mut end = 0.0;
        for (i, g) in t.glyphs.iter().enumerate() {
            let next = t.glyphs.get(i + 1).map_or(t.width, |n| n.x);
            if next > avail {
                break;
            }
            ctx.draw(x + g.x, y + g.y, g, style, color);
            end = next;
        }
        if avail > 0.0 {
            for g in &shapes[&key(ELLIPSIS, style)].1.glyphs {
                ctx.draw(x + end + g.x, y + g.y, g, style, color);
            }
        }
        end + ell_w
    }

    pub fn cell(&mut self, x: f32, baseline: f32, c: char, style: FontStyle, color: Color) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let (shapes, mut ctx) = self.split();
        let t = shape(shapes, ctx.text, s, style);
        for g in &t.glyphs {
            ctx.draw(x + g.x, baseline + g.y, g, style, color);
        }
    }

    pub fn cell_run(&mut self, s: &str, origins: &[(usize, f32)], baseline: f32, style: FontStyle, color: Color) {
        let (shapes, mut ctx) = self.split();
        let t = shape(shapes, ctx.text, s, style);
        for g in &t.glyphs {
            let end = origins.partition_point(|&(byte, _)| byte <= g.cluster as usize);
            if let Some(&(_, x)) = origins[..end].last() {
                ctx.draw(x + (g.x - g.cluster_x), baseline + g.y, g, style, color);
            }
        }
    }

    pub fn cell_box(&mut self, x: f32, y: f32, c: char, w: u32, h: u32, color: Color) -> bool {
        if !crate::boxdraw::is_procedural(c) || w > u16::MAX as u32 || h > u8::MAX as u32 {
            return false;
        }
        let key = GlyphKey { font: u16::MAX, id: c as u16, size_q: w as u16, subpx: h as u8 };
        let (_, mut ctx) = self.split();
        let gs = ctx.glyph(key, |_, _, bm| {
            let Some(data) = crate::boxdraw::rasterize(c, w, h) else { return false };
            *bm = GlyphBitmap { width: w, height: h, data, ..Default::default() };
            true
        });
        if let Some(gs) = gs {
            let size = ctx.atlas.size();
            ctx.dl.glyph(Rect::new(x, y, w as f32, h as f32), uv(gs.slot, size), color);
        }
        true
    }

    pub fn has_image(&self, key: u64) -> bool {
        self.images.contains_key(&key)
    }

    pub fn image_size(&self, key: u64) -> Option<(u32, u32)> {
        self.images.get(&key).map(|i| (i.w, i.h))
    }

    pub fn add_image(&mut self, key: u64, w: u32, h: u32, rgba: Vec<u8>) {
        debug_assert_eq!(rgba.len(), (w * h * 4) as usize);
        self.images.insert(key, Image { w, h, rgba, slot: None });
    }

    pub fn image(&mut self, key: u64, r: Rect) -> bool {
        self.image_alpha(key, r, 255)
    }

    pub fn image_alpha(&mut self, key: u64, r: Rect, alpha: u8) -> bool {
        let Some(slot) = self.image_slot(key) else { return false };
        self.dl.image(r, uv(slot, self.atlas.size()), alpha);
        true
    }

    pub fn set_background_image(&mut self, image: Option<(u64, [u32; 2], &[u8])>) {
        let revision = image.map(|(revision, _, _)| revision);
        if self.background_revision == revision {
            return;
        }
        self.images.remove(&BACKGROUND_IMAGE);
        if let Some((_, [width, height], rgba)) = image {
            self.add_image(BACKGROUND_IMAGE, width, height, rgba.to_vec());
        }
        self.background_revision = revision;
    }

    pub fn prepare_background_image(&mut self) {
        self.image_slot(BACKGROUND_IMAGE);
    }

    pub fn draw_background_image(&mut self, r: Rect, tile: Option<[f32; 2]>, alpha: u8) {
        let Some(slot) = self.image_slot(BACKGROUND_IMAGE) else { return };
        self.dl.tiled_image(r, uv(slot, self.atlas.size()), tile.unwrap_or([r.w, r.h]), alpha);
    }

    fn image_slot(&mut self, key: u64) -> Option<Slot> {
        let img = self.images.get(&key)?;
        let slot = match img.slot {
            Some(s) => {
                self.atlas.touch(s);
                s
            }
            None => {
                let (w, h) = (img.w, img.h);
                let s = self.split().1.allocate(w, h)?;
                let img = self.images.get_mut(&key).expect("registered image");
                self.uploads.rects.push((s, self.uploads.data.len()));
                self.uploads.data.extend_from_slice(&img.rgba);
                img.slot = Some(s);
                s
            }
        };
        Some(slot)
    }
}

fn key(s: &str, style: FontStyle) -> u64 {
    hash::hash(&(s, style))
}

fn shape<'a, B: TextBackend>(
    shapes: &'a mut Shapes,
    text: &mut B,
    s: &str,
    style: FontStyle,
) -> &'a ShapedText {
    let k = key(s, style);
    let e = shapes.entry(k).or_insert_with(|| (Box::from(""), ShapedText { glyphs: Vec::new(), width: -1.0 }));
    if e.1.width < 0.0 || &*e.0 != s {
        e.0 = s.into();
        e.1.glyphs.clear();
        text.shape(s, style, &mut e.1);
    }
    &e.1
}

fn uv(s: Slot, size: u32) -> [f32; 4] {
    let n = size as f32;
    [s.x as f32 / n, s.y as f32 / n, (s.x + s.w) as f32 / n, (s.y + s.h) as f32 / n]
}

struct GlyphCtx<'a, B> {
    text: &'a mut B,
    glyphs: &'a mut FxHashMap<GlyphKey, Option<GlyphSlot>>,
    images: &'a mut FxHashMap<u64, Image>,
    atlas: &'a mut Atlas,
    uploads: &'a mut Uploads,
    dl: &'a mut DrawList,
    scratch: &'a mut GlyphBitmap,
}

impl<B: TextBackend> GlyphCtx<'_, B> {
    fn draw(&mut self, px: f32, py: f32, g: &ShapedGlyph, style: FontStyle, color: Color) {
        let ix = px.floor();
        let subpx = ((px - ix) * 4.0) as u8 & 3;
        let key = GlyphKey { font: g.font, id: g.id, size_q: style.size_q, subpx };
        let gs = self.glyph(key, B::rasterize);
        if let Some(gs) = gs {
            let r = Rect::new(ix + gs.left as f32, py.round() - gs.top as f32, gs.slot.w as f32, gs.slot.h as f32);
            if gs.color {
                self.dl.image(r, uv(gs.slot, self.atlas.size()), (color.0 >> 24) as u8);
            } else {
                self.dl.glyph(r, uv(gs.slot, self.atlas.size()), color);
            }
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<Slot> {
        if let Some(slot) = self.atlas.alloc(w, h) {
            return Some(slot);
        }
        let area = self.atlas.evict(w, h)?;
        let rows = area.y..area.y + area.h;
        self.glyphs.retain(|_, glyph| glyph.is_none_or(|g| !rows.contains(&g.slot.y)));
        for image in self.images.values_mut() {
            if image.slot.is_some_and(|s| rows.contains(&s.y)) {
                image.slot = None;
            }
        }
        self.uploads.rects.push((area, self.uploads.data.len()));
        self.uploads.data.resize(self.uploads.data.len() + area.w as usize * area.h as usize * 4, 0);
        self.atlas.alloc(w, h)
    }

    fn glyph(&mut self, key: GlyphKey, rasterize: impl FnOnce(&mut B, GlyphKey, &mut GlyphBitmap) -> bool) -> Option<GlyphSlot> {
        if let Some(cached) = self.glyphs.get_mut(&key) {
            let glyph = cached.as_mut()?;
            if glyph.last_used != self.atlas.frame() {
                self.atlas.touch(glyph.slot);
                glyph.last_used = self.atlas.frame();
            }
            return Some(*glyph);
        }
        self.scratch.data.clear();
        self.scratch.color = false;
        if !rasterize(self.text, key, self.scratch) || self.scratch.width == 0 || self.scratch.height == 0 {
            self.glyphs.insert(key, None);
            return None;
        }
        let (w, h) = (self.scratch.width, self.scratch.height);
        let per_pixel = if self.scratch.color { 4 } else { 1 };
        let bytes = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(per_pixel));
        if bytes != Some(self.scratch.data.len()) {
            self.glyphs.insert(key, None);
            return None;
        }
        let slot = self.allocate(w, h)?;
        let bm = &*self.scratch;
        self.uploads.rects.push((slot, self.uploads.data.len()));
        if bm.color {
            self.uploads.data.extend_from_slice(&bm.data);
        } else {
            self.uploads.data.extend(bm.data.iter().flat_map(|&a| [a, a, a, a]));
        }
        let glyph = GlyphSlot { slot, last_used: self.atlas.frame(), left: bm.left, top: bm.top, color: bm.color };
        self.glyphs.insert(key, Some(glyph));
        Some(glyph)
    }
}

#[cfg(test)]
#[path = "../test/text.rs"]
mod tests;
