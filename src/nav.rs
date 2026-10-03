//! Back/forward history of pages, like a browser. Pages are kept whole (rows included), so going
//! back is instant and a reshuffled radio comes back exactly as it was.

/// Older pages beyond this are dropped; a page is at most a few thousand short rows.
const MAX_BACK: usize = 20;

pub struct History<T> {
    back: Vec<T>,
    current: Option<T>,
    forward: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History { back: Vec::new(), current: None, forward: Vec::new() }
    }
}

impl<T> History<T> {
    /// A newly opened page: the current one goes on the back stack, forward history is dropped.
    pub fn push(&mut self, page: T) {
        if let Some(previous) = self.current.replace(page) {
            self.back.push(previous);
            if self.back.len() > MAX_BACK {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    pub fn back(&mut self) -> Option<&T> {
        let previous = self.back.pop()?;
        if let Some(current) = self.current.replace(previous) {
            self.forward.push(current);
        }
        self.current.as_ref()
    }

    pub fn forward(&mut self) -> Option<&T> {
        let next = self.forward.pop()?;
        if let Some(current) = self.current.replace(next) {
            self.back.push(current);
        }
        self.current.as_ref()
    }

    pub fn current(&self) -> Option<&T> {
        self.current.as_ref()
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_forward_like_a_browser() {
        let mut h = History::default();
        assert_eq!(h.back(), None);
        h.push("a");
        h.push("b");
        h.push("c");
        assert_eq!(h.back(), Some(&"b"));
        assert_eq!(h.back(), Some(&"a"));
        assert_eq!(h.back(), None);
        assert_eq!(h.current(), Some(&"a"));
        assert_eq!(h.forward(), Some(&"b"));
        h.push("d"); // opening a page drops the forward history
        assert!(!h.can_forward());
        assert_eq!(h.back(), Some(&"b"));
    }

    #[test]
    fn back_stack_is_capped() {
        let mut h = History::default();
        for i in 0..MAX_BACK + 5 {
            h.push(i);
        }
        let mut steps = 0;
        while h.back().is_some() {
            steps += 1;
        }
        assert_eq!(steps, MAX_BACK);
    }
}
