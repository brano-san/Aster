use crate::err;
use aster_gfx::hash::FxHashMap;
use aster_gfx::text::{FontFamily, FontMetrics, FontStyle, GlyphBitmap, GlyphKey, ShapedGlyph, ShapedText, TextBackend};
use aster_platform::PlatformError;
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::slice::from_raw_parts;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::Graphics::DirectWrite::*;
use windows_core::{implement, ComObject, IUnknown, Ref, BOOL};

struct Run {
    face: IDWriteFontFace,
    x: f32,
    y: f32,
    rtl: bool,
    ids: Vec<u16>,
    advances: Vec<f32>,
    offsets: Vec<(f32, f32)>,
    text_pos: u32,
    cluster_map: Vec<u16>,
}

#[implement(IDWriteTextRenderer)]
struct Collector {
    runs: RefCell<Vec<Run>>,
}

impl IDWritePixelSnapping_Impl for Collector_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> windows_core::Result<BOOL> {
        Ok(true.into())
    }
    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> windows_core::Result<()> {
        unsafe { *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
        Ok(())
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> windows_core::Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for Collector_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        desc: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: Ref<IUnknown>,
    ) -> windows_core::Result<()> {
        unsafe {
            let run = &*run;
            let desc = &*desc;
            let Some(face) = (*run.fontFace).clone() else { return Ok(()) };
            let n = run.glyphCount as usize;
            let ids = if n == 0 || run.glyphIndices.is_null() { vec![0; n] } else { from_raw_parts(run.glyphIndices, n).to_vec() };
            let advances = if run.glyphAdvances.is_null() { vec![0.0; n] } else { from_raw_parts(run.glyphAdvances, n).to_vec() };
            let offsets = if run.glyphOffsets.is_null() {
                vec![(0.0, 0.0); n]
            } else {
                from_raw_parts(run.glyphOffsets, n).iter().map(|o| (o.advanceOffset, o.ascenderOffset)).collect()
            };
            let cluster_map = if desc.clusterMap.is_null() { Vec::new() } else { from_raw_parts(desc.clusterMap, desc.stringLength as usize).to_vec() };
            self.runs.borrow_mut().push(Run { face, x, y, rtl: run.bidiLevel & 1 == 1, ids, advances, offsets, text_pos: desc.textPosition, cluster_map });
        }
        Ok(())
    }
    fn DrawUnderline(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_UNDERLINE, _: Ref<IUnknown>) -> windows_core::Result<()> {
        Ok(())
    }
    fn DrawStrikethrough(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_STRIKETHROUGH, _: Ref<IUnknown>) -> windows_core::Result<()> {
        Ok(())
    }
    fn DrawInlineObject(&self, _: *const c_void, _: f32, _: f32, _: Ref<IDWriteInlineObject>, _: BOOL, _: BOOL, _: Ref<IUnknown>) -> windows_core::Result<()> {
        Ok(())
    }
}

struct Fallback {
    mapping: IDWriteFontFallback,
    family: Vec<u16>,
    symbol: Option<IDWriteFont>,
}

fn system_font_collection(factory: &IDWriteFactory2) -> Result<IDWriteFontCollection, PlatformError> {
    let mut collection = None;
    unsafe { factory.GetSystemFontCollection(&mut collection, false) }.map_err(err)?;
    collection.ok_or_else(|| PlatformError::new(-1, "no system font collection"))
}

pub fn font_families(monospace: bool) -> Result<Vec<String>, PlatformError> {
    let factory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(err)?;
    let collection = system_font_collection(&factory)?;
    let mut locale = [0u16; 85];
    let locale = if unsafe { GetUserDefaultLocaleName(&mut locale) } > 0 { PCWSTR(locale.as_ptr()) } else { w!("en-us") };
    Ok(collection_families(&collection, monospace, locale))
}

fn collection_families(collection: &IDWriteFontCollection, monospace: bool, locale: PCWSTR) -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..unsafe { collection.GetFontFamilyCount() } {
        let Ok(family) = (unsafe { collection.GetFontFamily(i) }) else { continue };
        if monospace {
            let normal = unsafe { family.GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL) };
            if !normal.and_then(|f| f.cast::<IDWriteFont1>()).is_ok_and(|f| unsafe { f.IsMonospacedFont() }.as_bool()) {
                continue;
            }
        }
        if let Some(name) = DWriteText::family_name(&family, locale).filter(|s| !s.is_empty()) {
            names.push(name);
        }
    }
    names.sort_by_cached_key(|s| s.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

pub struct DWriteText {
    factory: IDWriteFactory2,
    collection: IDWriteFontCollection,
    default_ui: PCWSTR,
    ui_family: Vec<u16>,
    icon_family: PCWSTR,
    default_mono: PCWSTR,
    mono_family: Vec<u16>,
    mono_weights: [u16; 2],
    mono_symbol: Option<IDWriteFont>,
    fallback: Option<Fallback>,
    typography: [IDWriteTypography; 2],
    formats: FxHashMap<FontStyle, IDWriteTextFormat>,
    primary: FxHashMap<(FontFamily, bool, bool), Option<DWRITE_FONT_METRICS>>,
    faces: Vec<IDWriteFontFace>,
    collector: ComObject<Collector>,
    renderer: IDWriteTextRenderer,
    wide: Vec<u16>,
    byte_of: Vec<u32>,
}

impl DWriteText {
    pub fn new() -> Result<Self, PlatformError> {
        unsafe {
            let factory: IDWriteFactory2 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(err)?;
            let collection = system_font_collection(&factory)?;
            let has = |name: PCWSTR| {
                let (mut i, mut exists) = (0u32, BOOL(0));
                collection.FindFamilyName(name, &mut i, &mut exists).is_ok() && exists.as_bool()
            };
            let default_ui = if has(w!("Segoe UI Variable Text")) { w!("Segoe UI Variable Text") } else { w!("Segoe UI") };
            let icon_family = if has(w!("Segoe Fluent Icons")) { w!("Segoe Fluent Icons") } else { w!("Segoe MDL2 Assets") };
            let mono_family = if has(w!("Cascadia Mono")) { w!("Cascadia Mono") } else { w!("Consolas") };
            let typography = |parameter| -> windows_core::Result<IDWriteTypography> {
                let typography = factory.CreateTypography()?;
                for tag in [DWRITE_FONT_FEATURE_TAG_STANDARD_LIGATURES, DWRITE_FONT_FEATURE_TAG_CONTEXTUAL_ALTERNATES] {
                    typography.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: tag, parameter })?;
                }
                Ok(typography)
            };
            let typography = [typography(0).map_err(err)?, typography(1).map_err(err)?];
            let collector = ComObject::new(Collector { runs: RefCell::default() });
            let renderer: IDWriteTextRenderer = collector.to_interface();
            Ok(Self {
                factory,
                collection,
                default_ui,
                ui_family: default_ui.as_wide().iter().copied().chain([0]).collect(),
                icon_family,
                default_mono: mono_family,
                mono_family: mono_family.as_wide().iter().copied().chain([0]).collect(),
                mono_weights: [400, 700],
                mono_symbol: None,
                fallback: None,
                typography,
                formats: FxHashMap::default(),
                primary: FxHashMap::default(),
                faces: Vec::new(),
                collector,
                renderer,
                wide: Vec::new(),
                byte_of: Vec::new(),
            })
        }
    }

    fn family(&self, f: FontFamily) -> PCWSTR {
        match f {
            FontFamily::Ui => PCWSTR(self.ui_family.as_ptr()),
            FontFamily::Icons => self.icon_family,
            FontFamily::Mono => PCWSTR(self.mono_family.as_ptr()),
        }
    }

    fn has_family(&self, name: PCWSTR) -> bool {
        let (mut i, mut exists) = (0u32, BOOL(0));
        unsafe { self.collection.FindFamilyName(name, &mut i, &mut exists).is_ok() && exists.as_bool() }
    }

    fn symbol_font(&self, family: PCWSTR) -> Option<IDWriteFont> {
        unsafe {
            let (mut i, mut exists) = (0, BOOL(0));
            self.collection.FindFamilyName(family, &mut i, &mut exists).ok()?;
            let font = self.collection.GetFontFamily(i).ok()?.GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL).ok()?;
            font.IsSymbolFont().as_bool().then_some(font)
        }
    }

    fn fallback(&self, name: &str) -> Option<Fallback> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        if name.is_empty() || name.contains('\0') || !self.has_family(PCWSTR(wide.as_ptr())) {
            return None;
        }
        unsafe {
            let builder = self.factory.CreateFontFallbackBuilder().ok()?;
            let range = DWRITE_UNICODE_RANGE { first: 0, last: 0x10ffff };
            builder.AddMapping(&[range], &[wide.as_ptr()], &self.collection, PCWSTR::null(), PCWSTR::null(), 1.0).ok()?;
            builder.AddMappings(&self.factory.GetSystemFontFallback().ok()?).ok()?;
            let mapping = builder.CreateFontFallback().ok()?;
            Some(Fallback { mapping, symbol: self.symbol_font(PCWSTR(wide.as_ptr())), family: wide })
        }
    }

    fn apply_symbol_fallback(&self, layout: &IDWriteTextLayout, text: &str) -> windows_core::Result<()> {
        let (Some(primary), Some(fallback)) = (&self.mono_symbol, &self.fallback) else { return Ok(()) };
        let mut start = 0;
        for c in text.chars() {
            let range = DWRITE_TEXT_RANGE { startPosition: start, length: c.len_utf16() as u32 };
            start += range.length;
            unsafe {
                if !primary.HasCharacter(c as u32)?.as_bool() {
                    let family = match &fallback.symbol {
                        Some(symbol) if !symbol.HasCharacter(c as u32)?.as_bool() => self.default_mono,
                        _ => PCWSTR(fallback.family.as_ptr()),
                    };
                    layout.SetFontFamilyName(family, range)?;
                }
            }
        }
        Ok(())
    }

    fn family_name(family: &IDWriteFontFamily, locale: PCWSTR) -> Option<String> {
        unsafe {
            let names = family.GetFamilyNames().ok()?;
            let (mut i, mut exists) = (0u32, BOOL(0));
            if names.FindLocaleName(locale, &mut i, &mut exists).is_err() || !exists.as_bool() {
                let _ = names.FindLocaleName(w!("en-us"), &mut i, &mut exists);
            }
            let i = if exists.as_bool() { i } else { 0 };
            let len = names.GetStringLength(i).ok()? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(i, &mut buf).ok()?;
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    fn weight(&self, family: FontFamily, bold: bool) -> DWRITE_FONT_WEIGHT {
        match (bold, family) {
            (_, FontFamily::Mono) => DWRITE_FONT_WEIGHT(i32::from(self.mono_weights[usize::from(bold)])),
            (false, _) => DWRITE_FONT_WEIGHT_NORMAL,
            (true, _) => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        }
    }

    fn slant(italic: bool) -> DWRITE_FONT_STYLE {
        if italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        }
    }

    fn format(&mut self, style: FontStyle) -> Option<IDWriteTextFormat> {
        if let Some(f) = self.formats.get(&style) {
            return Some(f.clone());
        }
        let f = unsafe {
            let f = self
                .factory
                .CreateTextFormat(self.family(style.family), &self.collection, self.weight(style.family, style.bold), Self::slant(style.italic), DWRITE_FONT_STRETCH_NORMAL, style.px().max(1.0), w!("en-us"))
                .ok()?;
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            if let Some(fallback) = self.fallback.as_ref().filter(|_| style.family == FontFamily::Mono) {
                f.cast::<IDWriteTextFormat1>().ok()?.SetFontFallback(&fallback.mapping).ok()?;
            }
            f
        };
        self.formats.insert(style, f.clone());
        Some(f)
    }

    fn face_index(&mut self, face: &IDWriteFontFace) -> u16 {
        match self.faces.iter().position(|f| f.as_raw() == face.as_raw()) {
            Some(i) => i as u16,
            None => {
                self.faces.push(face.clone());
                (self.faces.len() - 1) as u16
            }
        }
    }

    fn design_metrics(&mut self, family: FontFamily, bold: bool, italic: bool) -> Option<DWRITE_FONT_METRICS> {
        if let Some(m) = self.primary.get(&(family, bold, italic)) {
            return *m;
        }
        let m = unsafe {
            (|| {
                let (mut i, mut exists) = (0u32, BOOL(0));
                self.collection.FindFamilyName(self.family(family), &mut i, &mut exists).ok()?;
                let font = self.collection.GetFontFamily(i).ok()?.GetFirstMatchingFont(self.weight(family, bold), DWRITE_FONT_STRETCH_NORMAL, Self::slant(italic)).ok()?;
                let mut m = DWRITE_FONT_METRICS::default();
                font.GetMetrics(&mut m);
                Some(m)
            })()
        };
        self.primary.insert((family, bold, italic), m);
        m
    }
}

impl TextBackend for DWriteText {
    fn set_mono_font(&mut self, name: &str, weights: [u16; 2]) -> bool {
        self.mono_weights = weights;
        self.set_mono_family(name)
    }

    fn set_ui_family(&mut self, name: &str) -> bool {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let found = !name.is_empty() && !name.contains('\0') && self.has_family(PCWSTR(wide.as_ptr()));
        self.ui_family = if found { wide } else { unsafe { self.default_ui.as_wide() }.iter().copied().chain([0]).collect() };
        self.formats.retain(|style, _| style.family != FontFamily::Ui);
        self.primary.retain(|(family, _, _), _| *family != FontFamily::Ui);
        found || name.is_empty()
    }

    fn set_mono_family(&mut self, name: &str) -> bool {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let found = !name.is_empty() && !name.contains('\0') && self.has_family(PCWSTR(wide.as_ptr()));
        self.mono_family = if found { wide } else { unsafe { self.default_mono.as_wide() }.iter().copied().chain([0]).collect() };
        self.mono_symbol = self.symbol_font(self.family(FontFamily::Mono));
        self.formats.retain(|style, _| style.family != FontFamily::Mono);
        self.primary.retain(|(family, _, _), _| *family != FontFamily::Mono);
        found || name.is_empty()
    }

    fn families(&mut self) -> Vec<String> {
        collection_families(&self.collection, false, w!("en-us"))
    }

    fn set_fallback_family(&mut self, name: &str) -> bool {
        self.fallback = self.fallback(name);
        self.formats.retain(|style, _| style.family != FontFamily::Mono);
        name.is_empty() || self.fallback.is_some()
    }

    fn shape(&mut self, text: &str, style: FontStyle, out: &mut ShapedText) {
        out.glyphs.clear();
        out.width = 0.0;
        if text.is_empty() {
            return;
        }
        let Some(format) = self.format(style) else { return };
        self.wide.clear();
        self.byte_of.clear();
        for (b, c) in text.char_indices() {
            let mut buf = [0u16; 2];
            for &u in c.encode_utf16(&mut buf).iter() {
                self.wide.push(u);
                self.byte_of.push(b as u32);
            }
        }
        self.byte_of.push(text.len() as u32);
        let runs = unsafe {
            let Ok(layout) = self.factory.CreateTextLayout(&self.wide, &format, 1.0e6, 1.0e6) else { return };
            if style.family == FontFamily::Mono {
                let range = DWRITE_TEXT_RANGE { startPosition: 0, length: self.wide.len() as u32 };
                if layout.SetTypography(&self.typography[style.ligatures as usize], range).is_err() || self.apply_symbol_fallback(&layout, text).is_err() {
                    return;
                }
            }
            self.collector.runs.borrow_mut().clear();
            if layout.Draw(None, &self.renderer, 0.0, 0.0).is_err() {
                return;
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() {
                out.width = m.widthIncludingTrailingWhitespace;
            }
            std::mem::take(&mut *self.collector.runs.borrow_mut())
        };
        let base_y = runs.first().map_or(0.0, |r| r.y);
        for r in runs {
            let font = self.face_index(&r.face);
            let mut first_char = vec![u32::MAX; r.ids.len()];
            for (j, &g) in r.cluster_map.iter().enumerate().rev() {
                if let Some(slot) = first_char.get_mut(g as usize) {
                    *slot = j as u32;
                }
            }
            let mut cluster = 0u32;
            let mut cluster_x = r.x;
            let mut pen = 0.0;
            for (i, &first) in first_char.iter().enumerate() {
                if first != u32::MAX {
                    cluster = first;
                }
                if i == 0 || first != u32::MAX {
                    cluster_x = if r.rtl {
                        let len = 1 + first_char[i + 1..].iter().take_while(|&&n| n == u32::MAX).count();
                        r.x - pen - r.advances[i..i + len].iter().sum::<f32>()
                    } else {
                        r.x + pen
                    };
                }
                let adv = r.advances[i];
                let (ox, oy) = r.offsets[i];
                let x = if r.rtl { r.x - pen - adv - ox } else { r.x + pen + ox };
                let utf16 = (r.text_pos + cluster) as usize;
                let byte = self.byte_of.get(utf16).copied().unwrap_or(text.len() as u32);
                out.glyphs.push(ShapedGlyph { font, id: r.ids[i], x, y: r.y - base_y - oy, cluster: byte, cluster_x });
                pen += adv;
            }
        }
    }

    fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool {
        let Some(face) = self.faces.get(key.font as usize) else { return false };
        let id = key.id;
        let advance = 0.0f32;
        let offset = DWRITE_GLYPH_OFFSET::default();
        let run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(face.clone())),
            fontEmSize: key.size_q as f32 / 4.0,
            glyphCount: 1,
            glyphIndices: &id,
            glyphAdvances: &advance,
            glyphOffsets: &offset,
            isSideways: false.into(),
            bidiLevel: 0,
        };
        let result = unsafe {
            (|| {
                let analysis = self
                    .factory
                    .CreateGlyphRunAnalysis(
                        &run,
                        None,
                        DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                        DWRITE_MEASURING_MODE_NATURAL,
                        DWRITE_GRID_FIT_MODE_DEFAULT,
                        DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                        key.subpx as f32 / 4.0,
                        0.0,
                    )
                    .ok()?;
                let r = analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1).ok()?;
                let (w, h) = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
                out.left = r.left;
                out.top = -r.top;
                out.width = w;
                out.height = h;
                out.data.resize(w as usize * h as usize, 0);
                if w > 0 && h > 0 {
                    analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &r, &mut out.data).ok()?;
                }
                Some(())
            })()
        };
        drop(ManuallyDrop::into_inner(run.fontFace));
        result.is_some()
    }

    fn metrics(&mut self, style: FontStyle) -> FontMetrics {
        let px = style.px();
        match self.design_metrics(style.family, style.bold, style.italic) {
            Some(m) if m.designUnitsPerEm > 0 => {
                let k = px / m.designUnitsPerEm as f32;
                FontMetrics { ascent: m.ascent as f32 * k, descent: m.descent as f32 * k }
            }
            _ => FontMetrics { ascent: px * 0.8, descent: px * 0.2 },
        }
    }
}

#[cfg(test)]
#[path = "../test/dwrite.rs"]
mod tests;
