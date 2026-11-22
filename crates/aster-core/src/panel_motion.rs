use std::collections::HashMap;
use std::time::Instant;

use aster_gfx::render::Rect;

use crate::files::FilesPage;
use crate::motion::{SLIDE, Tween};
use crate::panes::PaneId;
use crate::search::SearchBar;
use crate::ssh_toolbar::Toolbar;

#[derive(Default)]
pub(crate) struct Panels {
    pub sidebar: Slide<Rect>,
    pub files: Slide<Rect>,
    pub closed_files: Option<FilesPage>,
    pub search: Slide<SearchBar>,
    pub headers: HashMap<PaneId, Header>,
}

#[derive(Default)]
pub(crate) struct Header {
    pub title: Slide<()>,
    pub toolbar: Slide<Toolbar>,
}

pub(crate) struct Slide<T> {
    extent: Tween,
    pub content: Option<(f32, T)>,
}

impl<T> Default for Slide<T> {
    fn default() -> Self {
        Self { extent: Tween::at(0.0), content: None }
    }
}

impl<T> Slide<T> {
    pub fn update(&mut self, content: Option<(f32, T)>, animate: bool, now: Instant, motion: &mut bool) -> f32 {
        let target = content.as_ref().map_or(0.0, |(extent, _)| *extent);
        *motion |= self.extent.aim(target, animate, now, SLIDE);
        let extent = self.extent.value(now, SLIDE);
        if content.is_some() || extent == 0.0 {
            self.content = content;
        }
        extent
    }
}

#[cfg(test)]
#[path = "../test/panel_motion.rs"]
mod tests;
