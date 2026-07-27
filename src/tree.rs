#[derive(Debug)]
pub enum Node {
    // Definition of the Node data stucture in the binary tree
    Leaf {
        byte: u8,
        freq: u64,
    },
    Internal {
        freq: u64,
        left: Box<Node>,
        right: Box<Node>,
    },
}

// Helper methods
impl Node {
    pub fn freq(&self) -> u64 {
        match self {
            Node::Leaf { freq, .. } => *freq,
            Node::Internal { freq, .. } => *freq,
        }
    }
}

use crate::heap::HeapNode;
use std::collections::BinaryHeap;
use std::collections::HashMap;

/// Build a Huffman tree from a frequency table.
/// Returns None if the input was empty (no bytes at all).
pub fn build_tree(freqs: &HashMap<u8, u64>) -> Option<Node> {
    // Collect into a Vec and sort by byte value. This gives a fixed,
    // reproducible order to build from every time - completely
    // independent of this particular HashMap's internal (randomized)
    // iteration order.
    let mut entries: Vec<(u8, u64)> = Vec::new();
    for (byte, freq) in freqs {
        entries.push((*byte, *freq));
    }
    entries.sort_by_key(|entry| entry.0);

    let mut next_seq: u64 = 0;

    let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();
    for (byte, freq) in entries {
        let leaf = Node::Leaf { byte, freq };
        heap.push(HeapNode {
            node: leaf,
            seq: next_seq,
        });
        next_seq = next_seq + 1;
    }

    if heap.len() == 1 {
        let popped = heap.pop().unwrap();
        let only_node = popped.node;
        let freq = only_node.freq();

        let dummy_leaf = Node::Leaf { byte: 0, freq: 0 };

        let wrapped = Node::Internal {
            freq: freq,
            left: Box::new(only_node),
            right: Box::new(dummy_leaf),
        };

        return Some(wrapped);
    }

    while heap.len() > 1 {
        let popped_a = heap.pop().unwrap();
        let node_a = popped_a.node;

        let popped_b = heap.pop().unwrap();
        let node_b = popped_b.node;

        let combined_freq = node_a.freq() + node_b.freq();

        let merged = Node::Internal {
            freq: combined_freq,
            left: Box::new(node_a),
            right: Box::new(node_b),
        };

        heap.push(HeapNode {
            node: merged,
            seq: next_seq,
        });
        next_seq = next_seq + 1;
    }

    if heap.is_empty() {
        return None;
    }

    let popped_root = heap.pop().unwrap();
    let root_node = popped_root.node;
    Some(root_node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_from_multiple_symbols() {
        let mut freqs: HashMap<u8, u64> = HashMap::new();
        freqs.insert(b'a', 5);
        freqs.insert(b'b', 2);
        freqs.insert(b'c', 1);

        let tree = build_tree(&freqs).unwrap();
        assert_eq!(tree.freq(), 8); // 5 + 2 + 1
    }

    #[test]
    fn builds_tree_from_single_symbol() {
        let mut freqs: HashMap<u8, u64> = HashMap::new();
        freqs.insert(b'x', 42);

        let tree = build_tree(&freqs).unwrap();
        assert_eq!(tree.freq(), 42);
    }
}
