//! An indexed binary min-heap with a total order, for the searches.
//!
//! Elements are small integers `0..capacity` (a cell of a sector, a portal of
//! the abstract graph) and each carries a key of two fixed-point numbers, `(f,
//! h)`. The order is `(f, h, element)`, lexicographic, so two different
//! elements are never equal and the pop order depends on nothing but the
//! keys: the tie-break "lowest f, then lowest h, then lowest index" of
//! `docs/rules/pathfinding.md`. Because an element is in the heap at most once
//! (a better key *decreases* it in place) the heap never holds more than
//! `capacity` entries and never allocates after it is made.

/// The position of an element that is not in the heap.
const NOT_IN: u32 = u32::MAX;

/// The key of an element: estimated total cost, then the estimate to the
/// goal. Smaller is better.
pub(crate) type Key = (u64, u64);

/// A min-heap of the elements `0..capacity`, each at most once.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct IndexedHeap {
    /// The elements in heap order.
    heap: Vec<u32>,
    /// For each element its place in `heap`, or [`NOT_IN`].
    pos: Vec<u32>,
    /// For each element its key; meaningful while it is in the heap.
    key: Vec<Key>,
}

impl IndexedHeap {
    /// A heap for the elements `0..capacity`. The only allocation it makes.
    pub fn new(capacity: usize) -> IndexedHeap {
        IndexedHeap {
            heap: Vec::with_capacity(capacity),
            pos: vec![NOT_IN; capacity],
            key: vec![(0, 0); capacity],
        }
    }

    /// Whether nothing is in the heap.
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Takes everything out. Costs the number of elements in it.
    pub fn clear(&mut self) {
        for &e in &self.heap {
            self.pos[e as usize] = NOT_IN;
        }
        self.heap.clear();
    }

    /// Whether `element` is in the heap.
    #[cfg(test)]
    pub fn contains(&self, element: u32) -> bool {
        self.pos[element as usize] != NOT_IN
    }

    /// Puts `element` in with `key`, or lowers its key to `key` if it is in
    /// already with a higher one. A key that is not lower changes nothing.
    pub fn push_or_lower(&mut self, element: u32, key: Key) {
        let at = self.pos[element as usize];
        if at == NOT_IN {
            self.key[element as usize] = key;
            self.heap.push(element);
            let last = self.heap.len() - 1;
            self.pos[element as usize] = last as u32;
            self.sift_up(last);
        } else if key < self.key[element as usize] {
            self.key[element as usize] = key;
            self.sift_up(at as usize);
        }
    }

    /// Takes out the element with the smallest key.
    pub fn pop(&mut self) -> Option<u32> {
        let top = *self.heap.first()?;
        self.pos[top as usize] = NOT_IN;
        let last = self.heap.pop()?;
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.pos[last as usize] = 0;
            self.sift_down(0);
        }
        Some(top)
    }

    /// `(key, element)` of the place `a` is before the one of place `b`.
    fn before(&self, a: usize, b: usize) -> bool {
        let (ea, eb) = (self.heap[a], self.heap[b]);
        let (ka, kb) = (self.key[ea as usize], self.key[eb as usize]);
        (ka, ea) < (kb, eb)
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.heap.swap(a, b);
        self.pos[self.heap[a] as usize] = a as u32;
        self.pos[self.heap[b] as usize] = b as u32;
    }

    fn sift_up(&mut self, mut at: usize) {
        while at > 0 {
            let parent = (at - 1) / 2;
            if !self.before(at, parent) {
                break;
            }
            self.swap(at, parent);
            at = parent;
        }
    }

    fn sift_down(&mut self, mut at: usize) {
        let len = self.heap.len();
        loop {
            let (l, r) = (2 * at + 1, 2 * at + 2);
            let mut best = at;
            if l < len && self.before(l, best) {
                best = l;
            }
            if r < len && self.before(r, best) {
                best = r;
            }
            if best == at {
                break;
            }
            self.swap(at, best);
            at = best;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_in_key_order_with_ties_broken_by_h_then_index() {
        let mut h = IndexedHeap::new(8);
        h.push_or_lower(5, (10, 3));
        h.push_or_lower(2, (10, 3));
        h.push_or_lower(7, (10, 1));
        h.push_or_lower(0, (4, 9));
        h.push_or_lower(3, (12, 0));
        let order: Vec<u32> = std::iter::from_fn(|| h.pop()).collect();
        // f 4 first; f 10 with h 1 next; then h 3 by index; f 12 last.
        assert_eq!(order, [0, 7, 2, 5, 3]);
        assert!(h.is_empty());
    }

    #[test]
    fn a_lower_key_moves_an_element_up_and_a_higher_one_is_ignored() {
        let mut h = IndexedHeap::new(4);
        h.push_or_lower(0, (50, 0));
        h.push_or_lower(1, (40, 0));
        h.push_or_lower(2, (30, 0));
        h.push_or_lower(0, (10, 0));
        h.push_or_lower(2, (99, 0));
        assert_eq!(h.pop(), Some(0));
        assert_eq!(h.pop(), Some(2));
        assert_eq!(h.pop(), Some(1));
        assert_eq!(h.pop(), None);
    }

    #[test]
    fn clear_forgets_everything_and_the_heap_can_be_reused() {
        let mut h = IndexedHeap::new(3);
        h.push_or_lower(1, (1, 1));
        h.push_or_lower(2, (2, 2));
        assert!(h.contains(1));
        h.clear();
        assert!(!h.contains(1) && !h.contains(2) && h.is_empty());
        h.push_or_lower(2, (9, 9));
        assert_eq!(h.pop(), Some(2));
    }

    #[test]
    fn matches_a_sort_on_pseudo_random_keys() {
        let n = 200;
        let mut h = IndexedHeap::new(n);
        let mut keys = Vec::new();
        let mut x: u64 = 0x1234_5678_9ABC_DEF0;
        for e in 0..n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let key = (x % 17, (x >> 20) % 5);
            keys.push((key, e as u32));
            h.push_or_lower(e as u32, key);
        }
        keys.sort();
        let popped: Vec<u32> = std::iter::from_fn(|| h.pop()).collect();
        let expected: Vec<u32> = keys.iter().map(|&(_, e)| e).collect();
        assert_eq!(popped, expected);
    }
}
