//! Layout check (port of IaraKit.Debug.LayoutCheck): pure rules over a measured widget tree.
//! `kit::layout_tree` builds the tree from GTK allocations.

pub const TOLERANCE: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Node {
    pub path: String,
    pub rect: Rect,
    /// Horizontal box: children must share its vertical centre.
    pub row: bool,
    /// Scroll views: content may exceed on purpose.
    pub clips: bool,
    /// List scroll view: rows inside must be wholly visible or hidden at rest.
    pub viewport: bool,
    /// A list row (whole-row rule).
    pub list_row: bool,
    /// Label whose text is ellipsized or cut.
    pub clipped_text: bool,
    /// Must be allocated square (avatars are circles, never pills).
    pub square: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

pub fn find(root: &Node) -> Vec<Issue> {
    let mut out = Vec::new();
    walk(root, None, &mut out);
    out
}

/// `find` plus the root against the window `frame`: content the window cuts off is never scrolled to.
pub fn find_in(frame: Rect, root: &Node) -> Vec<Issue> {
    let mut out = Vec::new();
    overflow(frame, root, &mut out);
    walk(root, None, &mut out);
    out
}

fn walk(n: &Node, view: Option<Rect>, out: &mut Vec<Issue>) {
    let view = if n.viewport { Some(n.rect) } else { view };
    if let (true, Some(v)) = (n.list_row, view) {
        let (top, bottom) = (n.rect.y.max(v.y), (n.rect.y + n.rect.h).min(v.y + v.h));
        let visible = bottom - top;
        if visible > TOLERANCE && visible < n.rect.h - TOLERANCE {
            out.push(Issue {
                path: n.path.clone(),
                message: format!(
                    "{} row is cut by its scroll view ({visible} of {} px visible)",
                    n.path, n.rect.h
                ),
            });
        }
    }
    if n.clipped_text {
        out.push(Issue {
            path: n.path.clone(),
            message: format!("{} text is clipped", n.path),
        });
    }
    if n.square && (n.rect.w - n.rect.h).abs() > TOLERANCE {
        out.push(Issue {
            path: n.path.clone(),
            message: format!("{} is not square ({}x{} px)", n.path, n.rect.w, n.rect.h),
        });
    }
    if !n.clips {
        for c in &n.children {
            overflow(n.rect, c, out);
        }
    }
    if n.row {
        let centre = n.rect.y * 2 + n.rect.h;
        for c in &n.children {
            let off = (c.rect.y * 2 + c.rect.h - centre).abs() / 2;
            if off > TOLERANCE {
                out.push(Issue {
                    path: c.path.clone(),
                    message: format!("{} is {off} px off the row's vertical centre", c.path),
                });
            }
        }
    }
    for c in &n.children {
        walk(c, view, out);
    }
}

fn overflow(p: Rect, c: &Node, out: &mut Vec<Issue>) {
    let b = c.rect;
    for (px, side) in [
        (b.x + b.w - (p.x + p.w), "on the right"),
        (b.y + b.h - (p.y + p.h), "at the bottom"),
        (p.x - b.x, "on the left"),
        (p.y - b.y, "at the top"),
    ] {
        if px > TOLERANCE {
            out.push(Issue {
                path: c.path.clone(),
                message: format!("{} overflowed by {px} px {side}", c.path),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(path: &str, x: i32, y: i32, w: i32, h: i32) -> Node {
        Node {
            path: path.into(),
            rect: Rect { x, y, w, h },
            ..Default::default()
        }
    }

    #[test]
    fn square_widgets_must_be_square() {
        for (w, h, bad) in [
            (32, 32, false),
            (33, 32, false),
            (32, 52, true),
            (48, 32, true),
        ] {
            let mut root = n("root", 0, 0, 100, 100);
            let mut a = n("Label.avatar", 0, 0, w, h);
            a.square = true;
            root.children = vec![a];
            let i = find(&root);
            assert_eq!(!i.is_empty(), bad, "{w}x{h}: {i:?}");
            if bad {
                assert!(i[0].message.contains("not square"), "{}", i[0].message);
            }
        }
    }

    #[test]
    fn fits_is_clean() {
        let mut root = n("root", 0, 0, 100, 40);
        root.children = vec![n("a", 0, 0, 100, 40), n("b", 1, -1, 100, 40)]; // within tolerance
        assert!(find(&root).is_empty());
    }

    #[test]
    fn overflow_each_side() {
        for (c, side) in [
            (n("c", 10, 0, 100, 40), "right"),
            (n("c", 0, 5, 10, 40), "bottom"),
            (n("c", -3, 0, 10, 10), "left"),
            (n("c", 0, -3, 10, 10), "top"),
        ] {
            let mut root = n("root", 0, 0, 100, 40);
            root.children = vec![c];
            let i = find(&root);
            assert_eq!(i.len(), 1, "{side}");
            assert!(i[0].message.contains(side));
        }
    }

    #[test]
    fn scroll_content_exempt() {
        let mut root = n("scroll", 0, 0, 100, 40);
        root.clips = true;
        root.children = vec![n("long", 0, 0, 100, 400)];
        assert!(find(&root).is_empty());
    }

    #[test]
    fn content_cut_by_the_window_is_reported() {
        let frame = Rect {
            x: 0,
            y: 0,
            w: 480,
            h: 360,
        };
        let root = n("Box.app-window", 0, 0, 480, 520);
        let i = find_in(frame, &root);
        assert_eq!(i.len(), 1);
        assert!(i[0].message.contains("at the bottom"), "{}", i[0].message);
        assert!(find_in(frame, &n("Box.app-window", 0, 0, 480, 360)).is_empty());
    }

    #[test]
    fn row_alignment() {
        let mut row = n("row", 0, 0, 100, 28);
        row.row = true;
        row.children = vec![n("icon", 0, 6, 16, 16), n("label", 20, 0, 40, 18)];
        let i = find(&row);
        assert_eq!(i.len(), 1);
        assert_eq!(i[0].path, "label");
        assert!(i[0].message.contains("5 px off"));
    }

    #[test]
    fn partial_rows_in_scroll_views() {
        let mut scroll = n("scroll", 0, 0, 100, 100);
        scroll.clips = true;
        scroll.viewport = true;
        let mut list = n("list", 0, 0, 100, 160);
        let row = |y| {
            let mut r = n("row", 0, y, 100, 40);
            r.list_row = true;
            r
        };
        // 0..40, 40..80 whole; 80..120 cut at 100; 120..160 hidden.
        list.children = vec![row(0), row(40), row(80), row(120)];
        scroll.children = vec![list];
        let i = find(&scroll);
        assert_eq!(i.len(), 1, "{i:?}");
        assert!(i[0].message.contains("row is cut"), "{}", i[0].message);
        scroll.rect.h = 80;
        assert!(find(&scroll).is_empty());
    }

    #[test]
    fn clipped_text_reported() {
        let mut root = n("root", 0, 0, 100, 20);
        let mut l = n("Label \"Wi-Fi\"", 0, 0, 50, 20);
        l.clipped_text = true;
        root.children = vec![l];
        assert_eq!(find(&root).len(), 1);
    }
}
