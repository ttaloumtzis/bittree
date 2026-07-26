use crate::tree::Node;
use std::collections::HashMap;

/// Maps each byte to its Huffman code, represented as a sequence of bits.
/// `false` = 0 = "go left", `true` = 1 = "go right".
pub type CodeTable = HashMap<u8, Vec<bool>>;

/// Walk the tree and build the byte code table.
pub fn build_codes(root: &Node) -> CodeTable {
    let mut table: CodeTable = HashMap::new();
    let mut path: Vec<bool> = Vec::new();

    walk(root, &mut path, &mut table);

    table
}

/// Recursive helper: walks the tree, tracking the path taken so far.
/// When it reaches a leaf, it records that path as the leaf's code.
fn walk(node: &Node, path: &mut Vec<bool>, table: &mut CodeTable) {
    match node {
        Node::Leaf { byte, .. } => {
            let code = path.clone();
            table.insert(*byte, code);
        }
        Node::Internal { left, right, .. } => {
            path.push(false); // going left = bit 0
            walk(left, path, table);
            path.pop(); // undo before trying the other branch

            path.push(true); // going right = bit 1
            walk(right, path, table);
            path.pop();
        }
    }
}
