//! A simple FIFO queue of pending commands.
//!
//! Ported from `lib/monkey/queue.js`. The TypeScript original uses a hand-rolled
//! linked list; a [`VecDeque`] provides the same behaviour without unsafe code.

use std::collections::VecDeque;

/// A first-in, first-out queue.
///
/// Ported from the `Queue` class of `lib/monkey/queue.js`.
#[derive(Debug, Default)]
pub struct Queue<T> {
    items: VecDeque<T>,
}

impl<T> Queue<T> {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Queue {
            items: VecDeque::new(),
        }
    }

    /// Adds an item to the end of the queue.
    ///
    /// Ported from `Queue.enqueue()`.
    pub fn enqueue(&mut self, item: T) {
        self.items.push_back(item);
    }

    /// Removes the item at the front of the queue.
    ///
    /// Ported from `Queue.dequeue()`.
    pub fn dequeue(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// The number of items in the queue.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T> From<Vec<T>> for Queue<T> {
    fn from(items: Vec<T>) -> Self {
        Queue {
            items: items.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_order() {
        let mut queue = Queue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        queue.enqueue(1);
        queue.enqueue(2);
        queue.enqueue(3);
        assert_eq!(queue.len(), 3);
        assert!(!queue.is_empty());

        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.dequeue(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_alternating() {
        let mut queue = Queue::new();
        queue.enqueue("a");
        assert_eq!(queue.dequeue(), Some("a"));
        queue.enqueue("b");
        queue.enqueue("c");
        assert_eq!(queue.dequeue(), Some("b"));
        queue.enqueue("d");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dequeue(), Some("c"));
        assert_eq!(queue.dequeue(), Some("d"));
        assert_eq!(queue.dequeue(), None);
    }
}
