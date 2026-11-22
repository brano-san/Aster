use crate::app::{App, Effect};
use aster_gfx::render::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Stretch,
    Tile,
}

pub struct Bitmap {
    pub source_size: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct State {
    pub revision: u64,
    pub bitmap: Option<Bitmap>,
    pub failed: bool,
}

pub(crate) struct Placement {
    pub rect: Rect,
    pub tile: Option<[f32; 2]>,
}

pub(crate) fn placement(pane: Rect, size: [u32; 2], fit: Fit) -> Option<Placement> {
    if size.contains(&0) || pane.w <= 0.0 || pane.h <= 0.0 {
        return None;
    }
    let [w, h] = size.map(|value| value as f32);
    let rect = match fit {
        Fit::Cover | Fit::Contain => {
            let xscale = pane.w / w;
            let yscale = pane.h / h;
            let scale = if fit == Fit::Cover { xscale.max(yscale) } else { xscale.min(yscale) };
            let (w, h) = (w * scale, h * scale);
            Rect::new(pane.x + (pane.w - w) / 2.0, pane.y + (pane.h - h) / 2.0, w, h)
        }
        Fit::Stretch | Fit::Tile => pane,
    };
    Some(Placement { rect, tile: (fit == Fit::Tile).then_some([w, h]) })
}

impl App {
    pub(crate) fn request_background_image(&mut self) {
        self.background.revision = self.background.revision.wrapping_add(1);
        self.background.bitmap = None;
        self.background.failed = false;
        let path = self.config.appearance.background_image.trim();
        if !path.is_empty() {
            self.push(Effect::LoadBackgroundImage { revision: self.background.revision, path: path.to_owned() });
        }
    }

    pub fn background_image_loaded(&mut self, revision: u64, bitmap: Option<Bitmap>) {
        if revision == self.background.revision {
            self.background.failed = bitmap.is_none();
            self.background.bitmap = bitmap;
            self.dirty = true;
        }
    }
}

#[cfg(test)]
#[path = "../test/background.rs"]
mod tests;
