use super::*;
use aster_core::accessibility::{Text, TextLine};
use std::ops::Range;
use windows::core::{Ref, BSTR};
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Globalization::{FindStringOrdinal, FIND_FROMEND, FIND_FROMSTART};
use windows::Win32::System::Variant::VT_R8;

#[implement(ITextProvider2)]
pub(super) struct Provider(pub(super) ElementHandle);

impl Provider_Impl {
    fn document(&self) -> Result<Arc<Document>> {
        let (_, tree) = self.0.snapshot()?;
        Document::new(self.0.id, tree)
    }
}

impl ITextProvider_Impl for Provider_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let document = self.document()?;
        let text = document.text();
        let selected = text
            .selection
            .or_else(|| text.caret.map(|caret| (caret, caret)));
        let ranges =
            selected.map(|(start, end)| TextRange::create(self.0.clone(), document, start..end));
        unknown_array(ranges.as_slice())
    }

    fn GetVisibleRanges(&self) -> Result<*mut SAFEARRAY> {
        if self.0.bounds()?.height <= 0.0 {
            return unknown_array::<ITextRangeProvider>(&[]);
        }
        unknown_array(&[self.DocumentRange()?])
    }

    fn RangeFromChild(&self, _: Ref<IRawElementProviderSimple>) -> Result<ITextRangeProvider> {
        Err(E_INVALIDARG.into())
    }

    fn RangeFromPoint(&self, point: &UiaPoint) -> Result<ITextRangeProvider> {
        let (hwnd, tree) = self.0.snapshot()?;
        let origin = origin(hwnd)?;
        let x = (point.x - f64::from(origin.x)) as f32;
        let y = (point.y - f64::from(origin.y)) as f32;
        let document = Document::new(self.0.id, tree)?;
        let offset = document.offset_at(x, y);
        Ok(TextRange::create(self.0.clone(), document, offset..offset))
    }

    fn DocumentRange(&self) -> Result<ITextRangeProvider> {
        let document = self.document()?;
        let range = 0..document.len();
        Ok(TextRange::create(self.0.clone(), document, range))
    }

    fn SupportedTextSelection(&self) -> Result<SupportedTextSelection> {
        let (_, tree) = self.0.snapshot()?;
        Ok(
            if tree.node(self.0.id).ok_or_else(unavailable)?.role == Role::Terminal {
                SupportedTextSelection_Single
            } else {
                SupportedTextSelection_None
            },
        )
    }
}

impl ITextProvider2_Impl for Provider_Impl {
    fn RangeFromAnnotation(&self, _: Ref<IRawElementProviderSimple>) -> Result<ITextRangeProvider> {
        Err(E_INVALIDARG.into())
    }

    fn GetCaretRange(&self, active: *mut BOOL) -> Result<ITextRangeProvider> {
        if active.is_null() {
            return Err(E_POINTER.into());
        }
        let document = self.document()?;
        let caret = document.text().caret;
        unsafe {
            active.write((document.tree.focus == Some(self.0.id) && caret.is_some()).into());
        }
        let offset = caret
            .or_else(|| document.text().selection.map(|(_, end)| end))
            .unwrap_or(0);
        Ok(TextRange::create(self.0.clone(), document, offset..offset))
    }
}

struct Document {
    id: NodeId,
    tree: Arc<Tree>,
    bytes: Vec<usize>,
}

impl Document {
    fn new(id: NodeId, tree: Arc<Tree>) -> Result<Arc<Self>> {
        let text = tree
            .node(id)
            .and_then(|node| node.text.as_ref())
            .ok_or_else(unavailable)?;
        let bytes = text
            .value
            .char_indices()
            .map(|(byte, _)| byte)
            .chain([text.value.len()])
            .collect();
        Ok(Arc::new(Self { id, tree, bytes }))
    }

    fn text(&self) -> &Text {
        self.tree
            .node(self.id)
            .and_then(|node| node.text.as_ref())
            .expect("document snapshot")
    }

    fn boolean_attribute(&self, attribute: UIA_TEXTATTRIBUTE_ID) -> Option<bool> {
        match attribute {
            UIA_IsReadOnlyAttributeId => Some(
                self.tree
                    .node(self.id)
                    .is_none_or(|node| node.role != Role::Edit),
            ),
            UIA_IsHiddenAttributeId => Some(false),
            _ => None,
        }
    }

    fn len(&self) -> usize {
        self.bytes.len() - 1
    }

    fn content(&self, span: Range<usize>) -> &str {
        &self.text().value[self.bytes[span.start]..self.bytes[span.end]]
    }

    fn line_end(&self, line: &TextLine) -> usize {
        let end = line.end.min(self.len());
        if end > line.start && self.content(end - 1..end) == "\n" {
            end - 1
        } else {
            end
        }
    }

    fn line_x(&self, line: &TextLine, offset: usize) -> f32 {
        let relative = offset.saturating_sub(line.start);
        let characters = self.line_end(line).saturating_sub(line.start).max(1);
        line.bounds.x
            + line
                .offsets
                .get(relative)
                .copied()
                .unwrap_or(line.bounds.w * relative as f32 / characters as f32)
    }

    fn offset_at(&self, x: f32, y: f32) -> usize {
        let distance = |bounds: &Rect| (bounds.y - y).max(y - bounds.bottom()).max(0.0);
        let line = self
            .text()
            .lines
            .iter()
            .min_by(|a, b| distance(&a.bounds).total_cmp(&distance(&b.bounds)));
        let Some(line) = line else { return 0 };
        let start = line.start.min(self.len());
        let end = self.line_end(line).max(start);
        if line.bounds.w <= 0.0 {
            return start;
        }
        let mut nearest = start;
        let mut distance = f32::INFINITY;
        for offset in start..=end {
            let current = (self.line_x(line, offset) - x).abs();
            if current <= distance {
                nearest = offset;
                distance = current;
            }
        }
        nearest
    }

    fn boundaries(&self, unit: TextUnit) -> Result<Vec<usize>> {
        let text = self.text();
        let mut offsets = match unit {
            TextUnit_Character => (0..=self.len()).collect(),
            TextUnit_Format | TextUnit_Word => {
                let mut previous = ' ';
                text.value
                    .chars()
                    .enumerate()
                    .filter_map(|(index, ch)| {
                        let boundary = !ch.is_whitespace()
                            && (previous.is_whitespace()
                                || ch.is_alphanumeric() != previous.is_alphanumeric());
                        previous = ch;
                        boundary.then_some(index)
                    })
                    .collect()
            }
            TextUnit_Line if !text.lines.is_empty() => text
                .lines
                .iter()
                .map(|line| line.start.min(self.len()))
                .collect(),
            TextUnit_Line | TextUnit_Paragraph => text
                .value
                .chars()
                .enumerate()
                .filter_map(|(index, ch)| (ch == '\n').then_some(index + 1))
                .collect(),
            TextUnit_Page | TextUnit_Document => Vec::new(),
            _ => return Err(E_INVALIDARG.into()),
        };
        offsets.push(0);
        offsets.push(self.len());
        offsets.sort_unstable();
        offsets.dedup();
        Ok(offsets)
    }
}

#[implement(abi::TextRange)]
struct TextRange {
    element: ElementHandle,
    document: Arc<Document>,
    span: Mutex<Range<usize>>,
}

impl TextRange {
    fn create(
        element: ElementHandle,
        document: Arc<Document>,
        span: Range<usize>,
    ) -> ITextRangeProvider {
        let start = span.start.min(span.end).min(document.len());
        let end = span.start.max(span.end).min(document.len());
        let range: abi::TextRange = Self {
            element,
            document,
            span: Mutex::new(start..end),
        }
        .into();
        range.cast().expect("text range interface")
    }
}

impl TextRange_Impl {
    fn span(&self) -> MutexGuard<'_, Range<usize>> {
        self.span
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn create(&self, span: Range<usize>) -> ITextRangeProvider {
        TextRange::create(self.element.clone(), Arc::clone(&self.document), span)
    }

    fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.element.state, &other.element.state)
            && self.element.id == other.element.id
            && (Arc::ptr_eq(&self.document, &other.document)
                || self.document.text().value == other.document.text().value)
    }

    fn endpoint_from(
        &self,
        target: Ref<ITextRangeProvider>,
        side: TextPatternRangeEndpoint,
    ) -> Result<usize> {
        let target = target.ok()?.cast_object_ref::<TextRange>()?;
        if !self.same_document(target) {
            return Err(E_INVALIDARG.into());
        }
        endpoint(&target.span(), side)
    }
}

fn endpoint(span: &Range<usize>, side: TextPatternRangeEndpoint) -> Result<usize> {
    match side {
        TextPatternRangeEndpoint_Start => Ok(span.start),
        TextPatternRangeEndpoint_End => Ok(span.end),
        _ => Err(E_INVALIDARG.into()),
    }
}

fn set_endpoint(
    span: &mut Range<usize>,
    side: TextPatternRangeEndpoint,
    next: usize,
) -> Result<()> {
    match side {
        TextPatternRangeEndpoint_Start => *span = next..span.end.max(next),
        TextPatternRangeEndpoint_End => *span = span.start.min(next)..next,
        _ => return Err(E_INVALIDARG.into()),
    }
    Ok(())
}

fn move_endpoint(boundaries: &[usize], position: usize, count: i32) -> (usize, i32) {
    if count == 0 {
        return (position, 0);
    }
    let base = if count > 0 {
        boundaries
            .partition_point(|offset| *offset <= position)
            .saturating_sub(1)
    } else {
        boundaries.partition_point(|offset| *offset < position)
    };
    let target = base
        .saturating_add_signed(count as isize)
        .min(boundaries.len() - 1);
    (boundaries[target], (target as i64 - base as i64) as i32)
}

impl ITextRangeProvider_Impl for TextRange_Impl {
    fn Clone(&self) -> Result<ITextRangeProvider> {
        self.element.snapshot()?;
        Ok(self.create(self.span().clone()))
    }

    fn Compare(&self, range: Ref<ITextRangeProvider>) -> Result<BOOL> {
        self.element.snapshot()?;
        let other = range.ok()?.cast_object_ref::<TextRange>()?;
        let span = self.span().clone();
        Ok((self.same_document(other) && span == *other.span()).into())
    }

    fn CompareEndpoints(
        &self,
        side: TextPatternRangeEndpoint,
        target: Ref<ITextRangeProvider>,
        other_side: TextPatternRangeEndpoint,
    ) -> Result<i32> {
        self.element.snapshot()?;
        let other = self.endpoint_from(target, other_side)?;
        let this = endpoint(&self.span(), side)?;
        Ok((this as i64 - other as i64).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    }

    fn ExpandToEnclosingUnit(&self, unit: TextUnit) -> Result<()> {
        self.element.snapshot()?;
        let boundaries = self.document.boundaries(unit)?;
        let mut span = self.span();
        if !span.is_empty()
            && boundaries.binary_search(&span.start).is_ok()
            && boundaries.binary_search(&span.end).is_ok()
        {
            return Ok(());
        }
        let first = boundaries
            .partition_point(|offset| *offset <= span.start)
            .saturating_sub(1)
            .min(boundaries.len().saturating_sub(2));
        *span = boundaries[first]
            ..boundaries
                .get(first + 1)
                .copied()
                .unwrap_or(boundaries[first]);
        Ok(())
    }

    fn FindAttribute(
        &self,
        attribute: UIA_TEXTATTRIBUTE_ID,
        value: &VARIANT,
        _: BOOL,
    ) -> Result<ITextRangeProvider> {
        self.element.snapshot()?;
        if self
            .document
            .boolean_attribute(attribute)
            .is_some_and(|expected| bool::try_from(value).ok() == Some(expected))
        {
            Ok(self.create(self.span().clone()))
        } else {
            Err(absent())
        }
    }

    fn FindText(
        &self,
        text: &BSTR,
        backward: BOOL,
        ignore_case: BOOL,
    ) -> Result<ITextRangeProvider> {
        self.element.snapshot()?;
        if text.is_empty() {
            return Err(absent());
        }
        let span = self.span().clone();
        let haystack: Vec<u16> = self.document.content(span.clone()).encode_utf16().collect();
        let flags = if backward.as_bool() {
            FIND_FROMEND
        } else {
            FIND_FROMSTART
        };
        let offset = unsafe { FindStringOrdinal(flags, &haystack, text, ignore_case.as_bool()) };
        if offset < 0 {
            return Err(absent());
        }
        let offset = offset as usize;
        let start = span.start + char::decode_utf16(haystack[..offset].iter().copied()).count();
        let end = start
            + char::decode_utf16(haystack[offset..offset + text.len()].iter().copied()).count();
        Ok(self.create(start..end))
    }

    fn GetAttributeValue(&self, attribute: UIA_TEXTATTRIBUTE_ID) -> Result<VARIANT> {
        self.element.snapshot()?;
        match self.document.boolean_attribute(attribute) {
            Some(value) => Ok(value.into()),
            None => Ok(unsafe { UiaGetReservedNotSupportedValue()? }.into()),
        }
    }

    fn GetBoundingRectangles(&self) -> Result<*mut SAFEARRAY> {
        let (hwnd, _) = self.element.snapshot()?;
        let mut rectangles = Vec::new();
        if self.element.bounds()?.height > 0.0 {
            let origin = origin(hwnd)?;
            let span = self.span().clone();
            for line in &self.document.text().lines {
                let line_end = self.document.line_end(line);
                let start = span.start.max(line.start);
                let end = span.end.min(line_end);
                if start >= end {
                    continue;
                }
                let x = self.document.line_x(line, start);
                let w = self.document.line_x(line, end) - x;
                let bounds = Rect::new(x, line.bounds.y, w, line.bounds.h);
                let bounds = screen_rect(bounds, origin);
                rectangles.extend([bounds.left, bounds.top, bounds.width, bounds.height]);
            }
        }
        safe_array(VT_R8, &rectangles, |value| (value as *const f64).cast())
    }

    fn GetEnclosingElement(&self) -> Result<IRawElementProviderSimple> {
        self.element.element()
    }

    fn GetText(&self, maximum: i32) -> Result<BSTR> {
        self.element.snapshot()?;
        let span = self.span().clone();
        let text = self.document.content(span);
        let mut remaining = match maximum {
            -1 => return Ok(BSTR::from(text)),
            0.. => maximum as usize,
            _ => return Err(E_INVALIDARG.into()),
        };
        let length = text
            .chars()
            .take_while(|ch| {
                let Some(next) = remaining.checked_sub(ch.len_utf16()) else {
                    return false;
                };
                remaining = next;
                true
            })
            .map(char::len_utf8)
            .sum();
        Ok(BSTR::from(&text[..length]))
    }

    fn Move(&self, unit: TextUnit, count: i32) -> Result<i32> {
        self.element.snapshot()?;
        let mut span = self.span();
        let mut boundaries = self.document.boundaries(unit)?;
        if span.is_empty() {
            let (next, moved) = move_endpoint(&boundaries, span.start, count);
            *span = next..next;
            return Ok(moved);
        }
        let Some(end) = boundaries.pop() else {
            return Ok(0);
        };
        if boundaries.is_empty() {
            return Ok(0);
        }
        let start = boundaries
            .partition_point(|offset| *offset <= span.start)
            .saturating_sub(1);
        let next = start
            .saturating_add_signed(count as isize)
            .min(boundaries.len() - 1);
        if next != start {
            *span = boundaries[next]..boundaries.get(next + 1).copied().unwrap_or(end);
        }
        Ok((next as i64 - start as i64) as i32)
    }

    fn MoveEndpointByUnit(
        &self,
        side: TextPatternRangeEndpoint,
        unit: TextUnit,
        count: i32,
    ) -> Result<i32> {
        self.element.snapshot()?;
        let boundaries = self.document.boundaries(unit)?;
        let mut span = self.span();
        let position = endpoint(&span, side)?;
        let (next, moved) = move_endpoint(&boundaries, position, count);
        set_endpoint(&mut span, side, next)?;
        Ok(moved)
    }

    fn MoveEndpointByRange(
        &self,
        side: TextPatternRangeEndpoint,
        target: Ref<ITextRangeProvider>,
        other_side: TextPatternRangeEndpoint,
    ) -> Result<()> {
        self.element.snapshot()?;
        let next = self.endpoint_from(target, other_side)?;
        set_endpoint(&mut self.span(), side, next)
    }

    fn Select(&self) -> Result<()> {
        let span = self.span().clone();
        self.element.state.queue_action(PendingAction {
            action: Action::SelectText {
                node: self.element.id,
                start: span.start,
                end: span.end,
            },
            document: Some(Arc::clone(&self.document.tree)),
        })
    }

    fn AddToSelection(&self) -> Result<()> {
        Err(invalid_operation())
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        Err(invalid_operation())
    }

    fn ScrollIntoView(&self, _: BOOL) -> Result<()> {
        self.element.snapshot()?;
        Ok(())
    }

    fn GetChildren(&self) -> Result<*mut SAFEARRAY> {
        self.element.snapshot()?;
        unknown_array::<IRawElementProviderSimple>(&[])
    }
}
