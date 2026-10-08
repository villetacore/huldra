//! i3-style tiling: each workspace is a tree of containers split
//! horizontally or vertically, with windows as leaves.

use crate::canvas::Rect;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Children side by side.
    H,
    /// Children stacked top to bottom.
    V,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Leaf(u32),
    Split { dir: Dir, children: Vec<Node> },
}

impl Node {
    fn contains(&self, w: u32) -> bool {
        match self {
            Node::Leaf(x) => *x == w,
            Node::Split { children, .. } => children.iter().any(|c| c.contains(w)),
        }
    }

    fn leaves(&self, out: &mut Vec<u32>) {
        match self {
            Node::Leaf(w) => out.push(*w),
            Node::Split { children, .. } => children.iter().for_each(|c| c.leaves(out)),
        }
    }

    /// Path of child indices to the leaf `w`.
    fn path(&self, w: u32) -> Option<Vec<usize>> {
        match self {
            Node::Leaf(x) => (*x == w).then(Vec::new),
            Node::Split { children, .. } => children.iter().enumerate().find_map(|(i, c)| {
                c.path(w).map(|mut p| {
                    p.insert(0, i);
                    p
                })
            }),
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> &mut Node {
        match path.split_first() {
            None => self,
            Some((&i, rest)) => match self {
                Node::Split { children, .. } => children[i].at_mut(rest),
                Node::Leaf(_) => self,
            },
        }
    }

    /// Replaces single-child splits by their child and drops empty ones.
    fn normalize(&mut self) {
        if let Node::Split { children, .. } = self {
            children.iter_mut().for_each(Node::normalize);
            children.retain(|c| !matches!(c, Node::Split { children, .. } if children.is_empty()));
            for c in children.iter_mut() {
                if let Node::Split { children: inner, .. } = c {
                    if inner.len() == 1 {
                        *c = inner.pop().unwrap();
                    }
                }
            }
        }
    }

    fn layout(&self, r: Rect, gap: i32, out: &mut Vec<(u32, Rect)>) {
        match self {
            Node::Leaf(w) => out.push((*w, r)),
            Node::Split { dir, children } => {
                let n = children.len() as i32;
                if n == 0 {
                    return;
                }
                let total = if *dir == Dir::H { r.w } else { r.h } - gap * (n - 1);
                let mut pos = 0;
                for (i, c) in children.iter().enumerate() {
                    let size = if i as i32 == n - 1 { total - pos } else { total / n };
                    let cr = if *dir == Dir::H { Rect::new(r.x + pos + gap * i as i32, r.y, size, r.h) } else { Rect::new(r.x, r.y + pos + gap * i as i32, r.w, size) };
                    c.layout(cr, gap, out);
                    pos += size;
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub root: Node,
    pub focus: Option<u32>,
    /// A window shown over the whole area (fullscreen).
    pub fullscreen: Option<u32>,
}

impl Default for Workspace {
    fn default() -> Workspace {
        Workspace { root: Node::Split { dir: Dir::H, children: Vec::new() }, focus: None, fullscreen: None }
    }
}

impl Workspace {
    pub fn windows(&self) -> Vec<u32> {
        let mut v = Vec::new();
        self.root.leaves(&mut v);
        v
    }

    pub fn contains(&self, w: u32) -> bool {
        self.root.contains(w)
    }

    pub fn is_empty(&self) -> bool {
        self.windows().is_empty()
    }

    /// Adds a window next to the focused one and focuses it.
    pub fn insert(&mut self, w: u32) {
        match self.focus.and_then(|f| self.root.path(f)) {
            Some(path) if !path.is_empty() => {
                let (last, parent) = path.split_last().unwrap();
                if let Node::Split { children, .. } = self.root.at_mut(parent) {
                    children.insert(last + 1, Node::Leaf(w));
                }
            }
            _ => {
                if let Node::Split { children, .. } = &mut self.root {
                    children.push(Node::Leaf(w));
                }
            }
        }
        self.focus = Some(w);
    }

    pub fn remove(&mut self, w: u32) {
        let Some(path) = self.root.path(w) else { return };
        let order = self.windows();
        let (last, parent) = path.split_last().unwrap();
        if let Node::Split { children, .. } = self.root.at_mut(parent) {
            children.remove(*last);
        }
        self.root.normalize();
        if self.fullscreen == Some(w) {
            self.fullscreen = None;
        }
        if self.focus == Some(w) {
            // Focus the previous window in order, like i3.
            let i = order.iter().position(|&x| x == w).unwrap_or(0);
            let rest = self.windows();
            self.focus = if rest.is_empty() { None } else { Some(rest[i.saturating_sub(1).min(rest.len() - 1)]) };
        }
    }

    /// `$mod+h` / `$mod+v`: new windows will open in a split of the
    /// focused window in direction `dir`.
    pub fn split(&mut self, dir: Dir) {
        let Some(path) = self.focus.and_then(|f| self.root.path(f)) else { return };
        let (last, parent) = path.split_last().unwrap();
        if let Node::Split { dir: d, children } = self.root.at_mut(parent) {
            if children.len() == 1 {
                *d = dir;
                return;
            }
            let leaf = core::mem::replace(&mut children[*last], Node::Leaf(0));
            children[*last] = Node::Split { dir, children: vec![leaf] };
        }
    }

    /// `$mod+e`: flips the direction of the focused window's container.
    pub fn toggle_split(&mut self) {
        let Some(path) = self.focus.and_then(|f| self.root.path(f)) else { return };
        let parent = &path[..path.len() - 1];
        if let Node::Split { dir, .. } = self.root.at_mut(parent) {
            *dir = if *dir == Dir::H { Dir::V } else { Dir::H };
        }
    }

    pub fn layout(&self, area: Rect, gap: i32) -> Vec<(u32, Rect)> {
        if let Some(f) = self.fullscreen {
            return vec![(f, area)];
        }
        let mut out = Vec::new();
        self.root.layout(area.inset(gap), gap, &mut out);
        out
    }

    /// The window next to `w` on `side` in the layout.
    pub fn neighbor(&self, w: u32, side: Side, area: Rect) -> Option<u32> {
        let rects = self.layout(area, 0);
        let cur = rects.iter().find(|(x, _)| *x == w)?.1;
        let (cx, cy) = (cur.x + cur.w / 2, cur.y + cur.h / 2);
        rects
            .iter()
            .filter(|(x, r)| {
                *x != w
                    && match side {
                        Side::Left => r.right() <= cur.x && r.y < cur.bottom() && r.bottom() > cur.y,
                        Side::Right => r.x >= cur.right() && r.y < cur.bottom() && r.bottom() > cur.y,
                        Side::Up => r.bottom() <= cur.y && r.x < cur.right() && r.right() > cur.x,
                        Side::Down => r.y >= cur.bottom() && r.x < cur.right() && r.right() > cur.x,
                    }
            })
            .min_by_key(|(_, r)| {
                let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
                (x - cx).abs() + (y - cy).abs()
            })
            .map(|(x, _)| *x)
    }

    /// Swaps the focused window with its neighbor on `side`.
    pub fn move_focused(&mut self, side: Side, area: Rect) {
        let Some(f) = self.focus else { return };
        let Some(other) = self.neighbor(f, side, area) else { return };
        let (Some(pa), Some(pb)) = (self.root.path(f), self.root.path(other)) else { return };
        *self.root.at_mut(&pa) = Node::Leaf(other);
        *self.root.at_mut(&pb) = Node::Leaf(f);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0, 0, 1000, 600);

    #[test]
    fn insert_and_layout() {
        let mut ws = Workspace::default();
        ws.insert(1);
        ws.insert(2);
        assert_eq!(ws.layout(AREA, 0), vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]);
        ws.split(Dir::V);
        ws.insert(3);
        assert_eq!(ws.layout(AREA, 0), vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 300)), (3, Rect::new(500, 300, 500, 300))]);
        assert_eq!(ws.neighbor(3, Side::Up, AREA), Some(2));
        assert_eq!(ws.neighbor(3, Side::Left, AREA), Some(1));
        assert_eq!(ws.neighbor(1, Side::Left, AREA), None);
        ws.remove(2);
        assert_eq!(ws.focus, Some(3));
        assert_eq!(ws.root, Node::Split { dir: Dir::H, children: vec![Node::Leaf(1), Node::Leaf(3)] });
        ws.move_focused(Side::Left, AREA);
        assert_eq!(ws.windows(), vec![3, 1]);
        ws.toggle_split();
        assert_eq!(ws.layout(AREA, 0)[1], (1, Rect::new(0, 300, 1000, 300)));
        ws.remove(3);
        ws.remove(1);
        assert!(ws.is_empty() && ws.focus.is_none());
    }

    #[test]
    fn gaps() {
        let mut ws = Workspace::default();
        ws.insert(1);
        ws.insert(2);
        let l = ws.layout(AREA, 10);
        assert_eq!(l[0].1, Rect::new(10, 10, 485, 580));
        assert_eq!(l[1].1, Rect::new(505, 10, 485, 580));
        ws.fullscreen = Some(2);
        assert_eq!(ws.layout(AREA, 10), vec![(2, AREA)]);
    }
}
