use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
    last_used: u64,
}

pub struct Atlas {
    size: u32,
    shelves: Vec<Shelf>,
    frame: u64,
    generation: u64,
}

const PAD: u32 = 1;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

impl Atlas {
    pub fn new(size: u32) -> Self {
        Self { size, shelves: Vec::new(), frame: 0, generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed) }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn clear(&mut self) {
        self.shelves.clear();
        self.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
    }

    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    pub(crate) fn touch(&mut self, slot: Slot) {
        let i = self.shelves.binary_search_by_key(&slot.y, |s| s.y).expect("resident atlas shelf");
        self.shelves[i].last_used = self.frame;
    }

    pub(crate) fn retain_row(&mut self, y: u32) {
        if let Ok(i) = self.shelves.binary_search_by_key(&y, |s| s.y) {
            self.shelves[i].last_used = self.frame;
        }
    }

    pub fn alloc(&mut self, w: u32, h: u32) -> Option<Slot> {
        let (pw, ph) = (w.checked_add(PAD)?, h.checked_add(PAD)?);
        if pw > self.size || ph > self.size {
            return None;
        }
        let fit = self.shelves.iter().enumerate()
            .filter(|(_, s)| s.h >= ph && self.size - s.x >= pw)
            .min_by_key(|(_, s)| s.h).map(|(i, _)| i);
        let i = fit.filter(|&i| self.shelves[i].h <= ph.saturating_add(ph / 4 + 1))
            .or_else(|| self.insert_shelf(ph)).or(fit)?;
        let shelf = &mut self.shelves[i];
        let slot = Slot { x: shelf.x, y: shelf.y, w, h };
        shelf.x += pw;
        shelf.last_used = self.frame;
        Some(slot)
    }

    fn insert_shelf(&mut self, h: u32) -> Option<usize> {
        let mut y = 0;
        let mut index = self.shelves.len();
        for (i, s) in self.shelves.iter().enumerate() {
            if s.y - y >= h {
                index = i;
                break;
            }
            y = s.y + s.h;
        }
        if self.size - y < h {
            return None;
        }
        self.shelves.insert(index, Shelf { y, h, x: 0, last_used: self.frame });
        Some(index)
    }

    pub(crate) fn evict(&mut self, w: u32, h: u32) -> Option<Slot> {
        if w.checked_add(PAD)? > self.size || h.checked_add(PAD)? > self.size {
            return None;
        }
        let mut best = None;
        let mut start_y = 0;
        for (i, first) in self.shelves.iter().enumerate() {
            let mut newest = 0;
            for (j, shelf) in self.shelves.iter().enumerate().skip(i) {
                if shelf.last_used == self.frame {
                    break;
                }
                newest = newest.max(shelf.last_used);
                let end_y = self.shelves.get(j + 1).map_or(self.size, |next| next.y);
                if end_y - start_y >= h + PAD {
                    let candidate = (newest, j - i + 1, i);
                    if best.is_none_or(|old| candidate < old) {
                        best = Some(candidate);
                    }
                    break;
                }
            }
            start_y = first.y + first.h;
        }
        let (_, count, i) = best?;
        let y = self.shelves[i].y;
        let last = &self.shelves[i + count - 1];
        let area = Slot { x: 0, y, w: self.size, h: last.y + last.h - y };
        self.shelves.drain(i..i + count);
        Some(area)
    }
}

#[cfg(test)]
#[path = "../test/atlas.rs"]
mod tests;
