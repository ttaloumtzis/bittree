use crate::tree::Node;
use std::cmp::Ordering;

/// Wraps a Node for use in a BinaryHeap.
///
/// BinaryHeap is a max-heap by default, so Ord below is reversed to get
/// min-heap behavior (smallest frequency popped first).
///
/// `seq` breaks ties when two nodes have equal frequency. Without it,
/// tie order would depend on HashMap iteration order (randomized per
/// instance), which could make compress and decompress build different
/// tree shapes from the same frequencies - silently swapping codes.
pub struct HeapNode {
    pub node: Node,
    pub seq: u64,
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        let freq_cmp = other.node.freq().cmp(&self.node.freq());
        if freq_cmp == Ordering::Equal {
            other.seq.cmp(&self.seq)
        } else {
            freq_cmp
        }
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.node.freq() == other.node.freq() && self.seq == other.seq
    }
}

impl Eq for HeapNode {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_smallest_frequency_first() {
        let mut heap = std::collections::BinaryHeap::new();
        heap.push(HeapNode {
            node: Node::Leaf {
                byte: b'a',
                freq: 50,
            },
            seq: 0,
        });
        heap.push(HeapNode {
            node: Node::Leaf {
                byte: b'b',
                freq: 10,
            },
            seq: 1,
        });
        heap.push(HeapNode {
            node: Node::Leaf {
                byte: b'c',
                freq: 30,
            },
            seq: 2,
        });

        assert_eq!(heap.pop().unwrap().node.freq(), 10);
        assert_eq!(heap.pop().unwrap().node.freq(), 30);
        assert_eq!(heap.pop().unwrap().node.freq(), 50);
    }
}
