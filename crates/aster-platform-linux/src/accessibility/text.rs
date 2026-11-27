use super::{index, Action, Rect, Role, Text, Widget};
use aster_core::accessibility::TextLine;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
use zbus::interface;

pub(super) struct AccessibleText(pub(super) Widget);

#[interface(name = "org.a11y.atspi.Text")]
impl AccessibleText {
    #[zbus(property)]
    fn character_count(&self) -> i32 {
        self.read(|text| index(text.value.chars().count()))
    }

    #[zbus(property)]
    fn caret_offset(&self) -> i32 {
        self.read(|text| text.caret).map_or(-1, index)
    }

    fn get_text(&self, start: i32, end: i32) -> String {
        self.read(|text| text_slice(&text.value, start, end).to_owned())
    }

    fn get_character_at_offset(&self, offset: i32) -> i32 {
        self.read(|text| {
            text.value
                .chars()
                .nth(offset as usize)
                .map_or(0, |ch| ch as i32)
        })
    }

    #[zbus(out_args("text", "startOffset", "endOffset"))]
    fn get_string_at_offset(&self, offset: i32, granularity: u32) -> (String, i32, i32) {
        self.read(|text| string_at(text, offset, granularity))
    }

    #[zbus(out_args("text", "startOffset", "endOffset"))]
    fn get_text_at_offset(&self, offset: i32, boundary: u32) -> (String, i32, i32) {
        self.read(|text| legacy(text, offset, boundary, 0))
    }

    #[zbus(out_args("text", "startOffset", "endOffset"))]
    fn get_text_before_offset(&self, offset: i32, boundary: u32) -> (String, i32, i32) {
        self.read(|text| legacy(text, offset, boundary, -1))
    }

    #[zbus(out_args("text", "startOffset", "endOffset"))]
    fn get_text_after_offset(&self, offset: i32, boundary: u32) -> (String, i32, i32) {
        self.read(|text| legacy(text, offset, boundary, 1))
    }

    fn get_n_selections(&self) -> i32 {
        i32::from(self.read(|text| text.selection.is_some()))
    }

    #[zbus(out_args("startOffset", "endOffset"))]
    fn get_selection(&self, selection: i32) -> (i32, i32) {
        if selection != 0 {
            return (-1, -1);
        }
        self.read(|text| text.selection)
            .map_or((-1, -1), |(start, end)| (index(start), index(end)))
    }

    fn add_selection(&self, start: i32, end: i32) -> bool {
        start < end && self.select(start, end, false)
    }

    fn set_selection(&self, selection: i32, start: i32, end: i32) -> bool {
        selection == 0 && self.select(start, end, true)
    }

    fn remove_selection(&self, selection: i32) -> bool {
        selection == 0
            && self.0.apply(|node| {
                let text = node.text.as_ref()?;
                let (start, _) = text.selection?;
                let caret = text.caret.unwrap_or(start);
                Some(Action::SelectText {
                    node: node.id,
                    start: caret,
                    end: caret,
                })
            })
    }

    fn set_caret_offset(&self, _offset: i32) -> bool {
        false
    }

    #[zbus(out_args("x", "y", "width", "height"))]
    fn get_character_extents(&self, offset: i32, coords: u32) -> (i32, i32, i32, i32) {
        self.get_range_extents(offset, offset.saturating_add(1), coords)
    }

    #[zbus(out_args("x", "y", "width", "height"))]
    fn get_range_extents(&self, start: i32, end: i32, coords: u32) -> (i32, i32, i32, i32) {
        self.read(|text| range_bounds(text, start, end))
            .map_or((-1, -1, 0, 0), |bounds| self.0.extents(bounds, coords))
    }

    fn get_offset_at_point(&self, x: i32, y: i32, coords: u32) -> i32 {
        let Some((dx, dy)) = self.0.origin(coords) else {
            return -1;
        };
        let (x, y) = (x as f32 - dx, y as f32 - dy);
        self.read(|text| {
            text.lines
                .iter()
                .find(|line| line.bounds.contains(x, y))
                .map(|line| {
                    let relative_x = x - line.bounds.x;
                    let last = line
                        .offsets
                        .partition_point(|&boundary| boundary <= relative_x)
                        .saturating_sub(1);
                    line.start
                        + line
                            .offsets
                            .get(last)
                            .map_or(0, |x| line.offsets.partition_point(|boundary| boundary < x))
                })
        })
        .map_or(-1, index)
    }

    fn get_attribute_value(&self, _offset: i32, _attribute: &str) -> &str {
        ""
    }

    #[zbus(out_args("attributes", "startOffset", "endOffset"))]
    fn get_attributes(&self, _offset: i32) -> (HashMap<String, String>, i32, i32) {
        (HashMap::new(), 0, self.character_count())
    }

    #[zbus(out_args("attributes", "startOffset", "endOffset"))]
    fn get_attribute_run(
        &self,
        offset: i32,
        _include_defaults: bool,
    ) -> (HashMap<String, String>, i32, i32) {
        self.get_attributes(offset)
    }

    fn get_default_attributes(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn get_default_attribute_set(&self) -> HashMap<String, String> {
        self.get_default_attributes()
    }
}

impl AccessibleText {
    fn read<T: Default>(&self, read: impl FnOnce(&Text) -> T) -> T {
        self.0
            .read(|_, node| node.text.as_ref().map(read).unwrap_or_default())
    }

    fn select(&self, start: i32, end: i32, existing: bool) -> bool {
        if start < 0 || start > end {
            return false;
        }
        let (start, end) = (start as usize, end as usize);
        self.0.apply(|node| {
            let text = node.text.as_ref()?;
            (node.role == Role::Terminal
                && text.selection.is_some() == existing
                && end <= text.value.chars().count())
            .then_some(Action::SelectText {
                node: node.id,
                start,
                end,
            })
        })
    }
}

fn char_byte(text: &str, offset: usize) -> usize {
    text.char_indices()
        .nth(offset)
        .map_or(text.len(), |(byte, _)| byte)
}

pub(super) fn text_slice(text: &str, start: i32, end: i32) -> &str {
    if start < 0 || (end != -1 && end < start) {
        return "";
    }
    let start = char_byte(text, start as usize);
    let end = if end == -1 {
        text.len()
    } else {
        char_byte(text, end as usize)
    };
    &text[start..end]
}

pub(super) fn text_change<'a>(old: &'a str, new: &'a str) -> (usize, &'a str, &'a str) {
    let mut prefix_chars = 0;
    let mut prefix_bytes = 0;
    for (left, _) in old
        .chars()
        .zip(new.chars())
        .take_while(|(left, right)| left == right)
    {
        prefix_chars += 1;
        prefix_bytes += left.len_utf8();
    }
    let (old, new) = (&old[prefix_bytes..], &new[prefix_bytes..]);
    let suffix: usize = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum();
    (
        prefix_chars,
        &old[..old.len() - suffix],
        &new[..new.len() - suffix],
    )
}

fn segment_bounds<'a>(
    chunks: impl Iterator<Item = &'a str> + 'a,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    chunks.scan(0, |start, chunk| {
        let end = *start + chunk.chars().count();
        let range = (*start, end);
        *start = end;
        Some(range)
    })
}

fn lines(text: &Text) -> impl Iterator<Item = (usize, usize)> + '_ {
    let physical = (!text.lines.is_empty())
        .then(|| text.lines.iter().map(|line| (line.start, line.end)))
        .into_iter()
        .flatten();
    let logical = text
        .lines
        .is_empty()
        .then(|| segment_bounds(text.value.split_inclusive('\n')))
        .into_iter()
        .flatten();
    physical.chain(logical)
}

fn slice_bounds(text: &str, range: Option<(usize, usize)>) -> (String, i32, i32) {
    range.map_or_else(
        || (String::new(), -1, -1),
        |(start, end)| {
            let (start, end) = (index(start), index(end));
            (text_slice(text, start, end).to_owned(), start, end)
        },
    )
}

pub(super) fn string_at(text: &Text, offset: i32, granularity: u32) -> (String, i32, i32) {
    if offset < 0 {
        return (String::new(), -1, -1);
    }
    let offset = offset as usize;
    let within = |&(start, end): &(usize, usize)| start <= offset && offset < end;
    let range = match granularity {
        0 => text.value.chars().nth(offset).map(|_| (offset, offset + 1)),
        1 => segment_bounds(text.value.split_word_bounds()).find(within),
        2 => segment_bounds(text.value.split_sentence_bounds()).find(within),
        3 => lines(text).find(within),
        4 => segment_bounds(text.value.split_inclusive('\n')).find(within),
        _ => None,
    };
    slice_bounds(&text.value, range)
}

fn legacy(text: &Text, offset: i32, boundary: u32, direction: i32) -> (String, i32, i32) {
    let length = text.value.chars().count();
    if offset < 0 || offset as usize > length {
        return (String::new(), -1, -1);
    }
    let range = if boundary == 0 {
        let offset = offset.saturating_add(direction);
        (0..index(length))
            .contains(&offset)
            .then(|| (offset as usize, offset as usize + 1))
    } else {
        let mut split = vec![0, length];
        match boundary {
            1 | 2 => {
                let mut start = 0;
                for chunk in text.value.split_word_bounds() {
                    let end = start + chunk.chars().count();
                    if chunk.unicode_words().next().is_some() {
                        split.push(if boundary == 1 { start } else { end });
                    }
                    start = end;
                }
            }
            3 | 4 => split.extend(segment_bounds(text.value.split_sentence_bounds()).map(
                |(start, end)| {
                    if boundary == 3 {
                        start
                    } else {
                        end
                    }
                },
            )),
            5 | 6 => split
                .extend(lines(text).map(|(start, end)| if boundary == 5 { start } else { end })),
            _ => return (String::new(), -1, -1),
        }
        split.sort_unstable();
        split.dedup();
        let at = split.partition_point(|&part| part <= offset as usize) as i32 - 1 + direction;
        let at = at as usize;
        split
            .get(at)
            .zip(split.get(at.saturating_add(1)))
            .map(|(&start, &end)| (start, end))
    };
    slice_bounds(&text.value, range)
}

pub(super) fn range_bounds(text: &Text, start: i32, end: i32) -> Option<Rect> {
    if start < 0 || (end != -1 && end < start) {
        return None;
    }
    let start = start as usize;
    let end = if end == -1 {
        text.value.chars().count()
    } else {
        end as usize
    };
    if start == end {
        let line = text
            .lines
            .iter()
            .find(|line| line.start <= start && start < line.end)
            .or_else(|| text.lines.last().filter(|line| line.end == start))?;
        return Some(line_bounds(line, start, start));
    }
    text.lines
        .iter()
        .filter(|line| line.start < end && start < line.end)
        .map(|line| line_bounds(line, start, end))
        .reduce(|first, last| {
            let x = first.x.min(last.x);
            let y = first.y.min(last.y);
            Rect::new(
                x,
                y,
                first.right().max(last.right()) - x,
                first.bottom().max(last.bottom()) - y,
            )
        })
}

fn line_bounds(line: &TextLine, start: usize, end: usize) -> Rect {
    let bounds = line.bounds;
    let at = |offset: usize| {
        line.offsets
            .get(offset.clamp(line.start, line.end) - line.start)
    };
    let (Some(&left), Some(&right)) = (at(start), at(end)) else {
        return bounds;
    };
    let right = if start < end && at(end - 1) == Some(&right) {
        line.offsets
            .iter()
            .copied()
            .find(|offset| *offset > right)
            .unwrap_or(right)
    } else {
        right
    };
    let (left, right) = (left.clamp(0., bounds.w), right.clamp(0., bounds.w));
    Rect::new(bounds.x + left, bounds.y, (right - left).max(0.), bounds.h)
}
