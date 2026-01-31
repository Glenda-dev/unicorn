extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use glenda::runtime::platform::DeviceKind;

#[derive(Debug, Clone)]
pub struct DeviceNode {
    pub id: usize,
    pub compatible: String,
    pub base_addr: usize,
    pub size: usize,
    pub irq: u32,
    pub kind: DeviceKind,
    pub parent_id: Option<usize>,
    pub children: Vec<usize>,
}

impl DeviceNode {
    pub fn compatible_str(&self) -> &str {
        &self.compatible
    }
}

pub struct DeviceManager {
    nodes: Vec<DeviceNode>,
    next_id: usize,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self { nodes: Vec::new(), next_id: 0 }
    }
    pub fn add_node(&mut self, node: DeviceNode) {
        self.nodes.push(node);
        self.next_id += 1;
    }

    pub fn get_node(&self, id: usize) -> Option<&DeviceNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn find_compatible(&self, compat: &str) -> Option<&DeviceNode> {
        self.nodes.iter().find(|n| n.compatible_str().contains(compat))
    }

    pub fn get_roots(&self) -> Vec<&DeviceNode> {
        self.nodes.iter().filter(|n| n.parent_id.is_none()).collect()
    }

    pub fn print_tree(&self) {
        let roots = self.get_roots();
        for root in roots {
            self.print_node(root, 0);
        }
    }

    fn print_node(&self, node: &DeviceNode, depth: usize) {
        glenda::println!(
            "{:indent$}- [{}] {} @ {:#x}",
            "",
            node.id,
            node.compatible_str(),
            node.base_addr,
            indent = depth * 2
        );
        for &child_id in &node.children {
            if let Some(child) = self.get_node(child_id) {
                self.print_node(child, depth + 1);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: usize,
    pub dev_type: DeviceKind,
}
