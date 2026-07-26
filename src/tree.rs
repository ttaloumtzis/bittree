#[derive(Debug)]
pub enum Node {
    // Definition of the Node data stucture in the binary tree
    Leaf {
        byte: u8,
        freq: u32,
    },
    Internal {
        freq: u32,
        left: Box<Node>,
        right: Box<Node>,
    },
}

// Helper methods
impl Node {
    pub fn freq(&self) -> u32 {
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
pub fn build_tree(freqs: &HashMap<u8, u32>) -> Option<Node> {
    let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();

    for (byte, freq) in freqs {
        let leaf = Node::Leaf {
            byte: *byte,
            freq: *freq,
        };
        heap.push(HeapNode(leaf));
    }

    // edge case - only one distinct byte in the whole file
    if heap.len() == 1 {
        let popped = heap.pop().unwrap();
        let only_node = popped.0;
        let freq = only_node.freq();

        let dummy_leaf = Node::Leaf { byte: 0, freq: 0 };

        let wrapped = Node::Internal {
            freq: freq,
            left: Box::new(only_node),
            right: Box::new(dummy_leaf),
        };

        return Some(wrapped);
    }

    // repeatedly merge the two smallest nodes until one remains
    while heap.len() > 1 {
        let popped_a = heap.pop().unwrap();
        let node_a = popped_a.0;

        let popped_b = heap.pop().unwrap();
        let node_b = popped_b.0;

        let combined_freq = node_a.freq() + node_b.freq();

        let merged = Node::Internal {
            freq: combined_freq,
            left: Box::new(node_a),
            right: Box::new(node_b),
        };

        heap.push(HeapNode(merged));
    }

    // whatever is left in the heap is the finished tree
    if heap.is_empty() {
        return None;
    }

    let popped_root = heap.pop().unwrap();
    let root_node = popped_root.0;
    Some(root_node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_from_multiple_symbols() {
        let mut freqs = HashMap::new();
        freqs.insert(b'a', 5);
        freqs.insert(b'b', 2);
        freqs.insert(b'c', 1);

        let tree = build_tree(&freqs).unwrap();
        assert_eq!(tree.freq(), 8); // 5 + 2 + 1
    }

    #[test]
    fn builds_tree_from_single_symbol() {
        let mut freqs = HashMap::new();
        freqs.insert(b'x', 42);

        let tree = build_tree(&freqs).unwrap();
        assert_eq!(tree.freq(), 42);
    }
}
