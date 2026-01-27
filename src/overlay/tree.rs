use std::collections::HashMap;
use peerreview_rust::types::node::NodeId;

#[derive(Clone)]
pub struct TreeNode {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

pub type Tree = HashMap<NodeId, TreeNode>;

#[derive(Clone)]
pub struct Overlay {
    pub trees: Vec<Tree>,
}

impl Overlay {
    pub fn children(&self, tree: usize, node: NodeId) -> &[NodeId] {
        &self.trees[tree][&node].children
    }
}

pub fn build_trees(nodes: &[NodeId], k: usize) -> Overlay {
    let mut trees = Vec::new();

    for _ in 0..k {
        let mut tree = Tree::new();

        for (i, &node) in nodes.iter().enumerate() {
            let parent = if i == 0 {
                None
            } else {
                Some(nodes[(i - 1) / 2])
            };

            let mut children = Vec::new();
            let left = 2 * i + 1;
            let right = 2 * i + 2;

            if left < nodes.len() {
                children.push(nodes[left]);
            }
            if right < nodes.len() {
                children.push(nodes[right]);
            }

            tree.insert(node, TreeNode { parent, children });
        }

        trees.push(tree);
    }

    Overlay { trees }
}
