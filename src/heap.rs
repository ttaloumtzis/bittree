use crate::tree::Node;
use std::cmp::Ordering;

/// Wraps a Node so we can put it in a BinaryHeap as a MIN-heap
/// (BinaryHeap is a max-heap by default, so we reverse the ordering).
pub struct HeapNode(pub Node);

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare `other` to `self` to flip to a MIN-heap:
        other.0.freq().cmp(&self.0.freq())
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.0.freq() == other.0.freq()
    }
}

impl Eq for HeapNode {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_smallest_frequency_first() {
        let mut heap = std::collections::BinaryHeap::new();
        heap.push(HeapNode(Node::Leaf {
            byte: b'a',
            freq: 50,
        }));
        heap.push(HeapNode(Node::Leaf {
            byte: b'b',
            freq: 10,
        }));
        heap.push(HeapNode(Node::Leaf {
            byte: b'c',
            freq: 30,
        }));

        assert_eq!(heap.pop().unwrap().0.freq(), 10);
        assert_eq!(heap.pop().unwrap().0.freq(), 30);
        assert_eq!(heap.pop().unwrap().0.freq(), 50);
    }
}
