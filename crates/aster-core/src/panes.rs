use aster_gfx::render::Rect;

pub type PaneId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn axis(self) -> Axis {
        match self {
            Side::Left | Side::Right => Axis::Horizontal,
            Side::Top | Side::Bottom => Axis::Vertical,
        }
    }

    fn before(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split { axis: Axis, ratio: f32, a: Box<Node>, b: Box<Node> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Splitter {
    pub rect: Rect,
    pub axis: Axis,
    pub path: Vec<bool>,
    pub span: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub panes: Vec<(PaneId, Rect)>,
    pub splitters: Vec<Splitter>,
}

impl Node {
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Node::Leaf(id) => *id == pane,
            Node::Split { a, b, .. } => a.contains(pane) || b.contains(pane),
        }
    }

    pub fn split(&mut self, pane: PaneId, side: Side, new: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == pane => {
                let (old, fresh) = (Box::new(Node::Leaf(pane)), Box::new(Node::Leaf(new)));
                let (a, b) = if side.before() { (fresh, old) } else { (old, fresh) };
                *self = Node::Split { axis: side.axis(), ratio: 0.5, a, b };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(pane, side, new) || b.split(pane, side, new),
        }
    }

    pub fn remove(&mut self, pane: PaneId) -> bool {
        let Node::Split { a, b, .. } = self else { return false };
        if **a == Node::Leaf(pane) {
            *self = std::mem::replace(&mut **b, Node::Leaf(0));
            return true;
        }
        if **b == Node::Leaf(pane) {
            *self = std::mem::replace(&mut **a, Node::Leaf(0));
            return true;
        }
        a.remove(pane) || b.remove(pane)
    }

    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        let Node::Split { ratio: r, a, b, .. } = self else { return };
        match path.split_first() {
            None => *r = ratio.clamp(0.05, 0.95),
            Some((false, rest)) => a.set_ratio(rest, ratio),
            Some((true, rest)) => b.set_ratio(rest, ratio),
        }
    }

    pub fn grow(&mut self, pane: PaneId, axis: Axis, delta: f32) -> bool {
        let Node::Split { axis: own, ratio, a, b } = self else { return false };
        if a.grow(pane, axis, delta) || b.grow(pane, axis, delta) {
            return true;
        }
        if *own != axis {
            return false;
        }
        if a.contains(pane) {
            *ratio = (*ratio + delta).clamp(0.05, 0.95);
            true
        } else if b.contains(pane) {
            *ratio = (*ratio - delta).clamp(0.05, 0.95);
            true
        } else {
            false
        }
    }

    pub fn layout(&self, r: Rect, gap: f32) -> Layout {
        let mut out = Layout::default();
        self.place(r, gap, &mut Vec::new(), &mut out);
        out
    }

    fn place(&self, r: Rect, gap: f32, path: &mut Vec<bool>, out: &mut Layout) {
        match self {
            Node::Leaf(id) => out.panes.push((*id, r)),
            Node::Split { axis, ratio, a, b } => {
                let (ra, rs, rb) = match axis {
                    Axis::Horizontal => {
                        let wa = ((r.w - gap) * ratio).round().max(0.0);
                        (Rect::new(r.x, r.y, wa, r.h), Rect::new(r.x + wa, r.y, gap, r.h), Rect::new(r.x + wa + gap, r.y, (r.w - wa - gap).max(0.0), r.h))
                    }
                    Axis::Vertical => {
                        let ha = ((r.h - gap) * ratio).round().max(0.0);
                        (Rect::new(r.x, r.y, r.w, ha), Rect::new(r.x, r.y + ha, r.w, gap), Rect::new(r.x, r.y + ha + gap, r.w, (r.h - ha - gap).max(0.0)))
                    }
                };
                out.splitters.push(Splitter { rect: rs, axis: *axis, path: path.clone(), span: r });
                path.push(false);
                a.place(ra, gap, path, out);
                path.pop();
                path.push(true);
                b.place(rb, gap, path, out);
                path.pop();
            }
        }
    }
}

pub fn neighbor(layout: &Layout, from: PaneId, side: Side) -> Option<PaneId> {
    let (_, r) = *layout.panes.iter().find(|(id, _)| *id == from)?;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    layout
        .panes
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, o)| match side {
            Side::Left => o.right() <= r.x + 1.0 && o.bottom() > r.y && o.y < r.bottom(),
            Side::Right => o.x >= r.right() - 1.0 && o.bottom() > r.y && o.y < r.bottom(),
            Side::Top => o.bottom() <= r.y + 1.0 && o.right() > r.x && o.x < r.right(),
            Side::Bottom => o.y >= r.bottom() - 1.0 && o.right() > r.x && o.x < r.right(),
        })
        .min_by(|(_, a), (_, b)| {
            let d = |o: &Rect| {
                let (ox, oy) = (o.x + o.w / 2.0, o.y + o.h / 2.0);
                (ox - cx).abs() + (oy - cy).abs()
            };
            d(a).total_cmp(&d(b))
        })
        .map(|(id, _)| *id)
}

#[cfg(test)]
#[path = "../test/panes.rs"]
mod tests;
