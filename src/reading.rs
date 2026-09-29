//! Page / spread navigation for single-page and two-page reading modes.

/// What is currently shown in the reader viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageView {
    /// One page alone (cover, back cover, or leftover middle page).
    Single(usize),
    /// Two facing pages (left, right), 0-based indices.
    Spread(usize, usize),
}

impl PageView {
    /// Leftmost (or only) page index — used as the navigation anchor.
    pub fn anchor(self) -> usize {
        match self {
            Self::Single(p) | Self::Spread(p, _) => p,
        }
    }
}

/// Resolve which view should display `page` given total `count`.
///
/// Two-page rule: page 0 (cover) and page `count-1` (back cover) are always
/// alone. Middle pages are paired left-to-right; an odd leftover middle page
/// is shown alone before the back cover.
pub fn view_for_page(page: usize, count: usize, two_page: bool) -> Option<PageView> {
    if count == 0 || page >= count {
        return None;
    }
    if !two_page || count <= 2 {
        return Some(PageView::Single(page));
    }
    if page == 0 {
        return Some(PageView::Single(0));
    }
    if page == count - 1 {
        return Some(PageView::Single(count - 1));
    }

    // Middle range: 1 .. count-2 inclusive.
    let left = if (page - 1) % 2 == 0 { page } else { page - 1 };
    let right = left + 1;
    if right >= count - 1 {
        Some(PageView::Single(left))
    } else {
        Some(PageView::Spread(left, right))
    }
}

/// Next navigation step (one page or one spread).
pub fn next_anchor(current: usize, count: usize, two_page: bool) -> Option<usize> {
    let view = view_for_page(current, count, two_page)?;
    let next = match view {
        PageView::Single(p) => p + 1,
        PageView::Spread(_, r) => r + 1,
    };
    if next < count {
        Some(view_for_page(next, count, two_page)?.anchor())
    } else {
        None
    }
}

/// Previous navigation step (one page or one spread).
pub fn prev_anchor(current: usize, count: usize, two_page: bool) -> Option<usize> {
    let view = view_for_page(current, count, two_page)?;
    let anchor = view.anchor();
    if anchor == 0 {
        return None;
    }
    Some(view_for_page(anchor - 1, count, two_page)?.anchor())
}

pub fn can_go_prev(current: usize, count: usize, two_page: bool) -> bool {
    prev_anchor(current, count, two_page).is_some()
}

pub fn can_go_next(current: usize, count: usize, two_page: bool) -> bool {
    next_anchor(current, count, two_page).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_and_back_alone() {
        // 6 pages: 0 | 1-2 | 3-4 | 5
        assert_eq!(view_for_page(0, 6, true), Some(PageView::Single(0)));
        assert_eq!(view_for_page(1, 6, true), Some(PageView::Spread(1, 2)));
        assert_eq!(view_for_page(2, 6, true), Some(PageView::Spread(1, 2)));
        assert_eq!(view_for_page(3, 6, true), Some(PageView::Spread(3, 4)));
        assert_eq!(view_for_page(5, 6, true), Some(PageView::Single(5)));
    }

    #[test]
    fn odd_middle_leftover() {
        // 5 pages: 0 | 1-2 | 3 | 4
        assert_eq!(view_for_page(3, 5, true), Some(PageView::Single(3)));
        assert_eq!(view_for_page(4, 5, true), Some(PageView::Single(4)));
        assert_eq!(next_anchor(0, 5, true), Some(1));
        assert_eq!(next_anchor(1, 5, true), Some(3));
        assert_eq!(next_anchor(3, 5, true), Some(4));
        assert_eq!(next_anchor(4, 5, true), None);
        assert_eq!(prev_anchor(4, 5, true), Some(3));
        assert_eq!(prev_anchor(3, 5, true), Some(1));
        assert_eq!(prev_anchor(1, 5, true), Some(0));
    }

    #[test]
    fn two_pages_each_alone() {
        assert_eq!(view_for_page(0, 2, true), Some(PageView::Single(0)));
        assert_eq!(view_for_page(1, 2, true), Some(PageView::Single(1)));
        assert_eq!(next_anchor(0, 2, true), Some(1));
    }

    #[test]
    fn single_mode_steps_one() {
        assert_eq!(next_anchor(2, 10, false), Some(3));
        assert_eq!(prev_anchor(2, 10, false), Some(1));
    }
}
