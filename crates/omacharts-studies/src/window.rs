//! The last `n` of something, oldest first.
//!
//! Every item is written twice, `n` slots apart, so the newest `n` are
//! always one contiguous slice and a fold over the window is a fold over a
//! slice. The storage is allocated once, when the study is made; stepping
//! never allocates.

#[derive(Clone, Debug)]
pub(crate) struct Window<T: Copy> {
    slots: Vec<T>,
    capacity: usize,
    /// Where the next item goes, which once full is also the oldest.
    head: usize,
    len: usize,
}

impl<T: Copy + Default> Window<T> {
    pub(crate) fn new(capacity: usize) -> Window<T> {
        let capacity = capacity.max(1);
        Window { slots: vec![T::default(); 2 * capacity], capacity, head: 0, len: 0 }
    }
}

impl<T: Copy> Window<T> {
    #[inline(always)]
    pub(crate) fn is_full(&self) -> bool {
        self.len == self.capacity
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, item: T) {
        let (head, capacity) = (self.head, self.capacity);
        self.slots[head] = item;
        self.slots[head + capacity] = item;
        self.head = if head + 1 == capacity { 0 } else { head + 1 };
        if self.len < capacity {
            self.len += 1;
        }
    }

    /// The items that stay when the next one is pushed, oldest first: the
    /// window a push would leave, less the item pushed.
    #[inline(always)]
    pub(crate) fn staying(&self) -> &[T] {
        if self.is_full() {
            &self.slots[self.head + 1..self.head + self.capacity]
        } else {
            &self.slots[..self.len]
        }
    }
}

/// The last `n` of something when only the oldest is ever read back: one
/// store per item, where a [`Window`] makes two to keep its items in a row.
#[derive(Clone, Debug)]
pub(crate) struct Ring<T: Copy> {
    slots: Vec<T>,
    /// Where the next item goes, which once full is also the oldest.
    head: usize,
    full: bool,
}

impl<T: Copy + Default> Ring<T> {
    pub(crate) fn new(capacity: usize) -> Ring<T> {
        Ring { slots: vec![T::default(); capacity.max(1)], head: 0, full: false }
    }
}

impl<T: Copy> Ring<T> {
    #[inline(always)]
    pub(crate) fn push(&mut self, item: T) {
        self.slots[self.head] = item;
        self.head += 1;
        if self.head == self.slots.len() {
            self.head = 0;
            self.full = true;
        }
    }

    /// Whether the next push leaves it holding `n` items.
    #[inline(always)]
    pub(crate) fn fills_with_next(&self) -> bool {
        self.full || self.head + 1 == self.slots.len()
    }

    /// The item the next push replaces, if it replaces one.
    #[inline(always)]
    pub(crate) fn leaving(&self) -> Option<T> {
        self.full.then(|| self.slots[self.head])
    }

    /// The slot the next push writes: the item it replaces once full, and
    /// until then the default the ring was made with. No branch, for a
    /// caller whose arithmetic the default leaves alone.
    #[inline(always)]
    pub(crate) fn replacing(&self) -> T {
        self.slots[self.head]
    }
}
