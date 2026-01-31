use crate::device::{DeviceManager, DeviceNode};
use alloc::string::String;
use alloc::vec::Vec;
use glenda::println;
use glenda::runtime::platform::PlatformInfo;

pub struct PlatformManager;

impl PlatformManager {
    pub fn scan(info: &PlatformInfo, dev_mgr: &mut DeviceManager) {
        println!("Unicorn: Scanning Platform Info...");
        println!("Model: {}", info.model());

        // PlatformInfo has a flat array with parent_index.
        // We need to map array index to device ID.
        // Let's assume device ID = array index.

        let mut nodes = Vec::new();

        // First pass: Create nodes
        for (i, dev_desc) in info.devices[..info.device_count].iter().enumerate() {
            let parent_id = if dev_desc.parent_index == u32::MAX {
                None
            } else {
                Some(dev_desc.parent_index as usize)
            };

            // Convert [u8] to String
            let compat_len = dev_desc
                .compatible
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(dev_desc.compatible.len());
            let compat_str =
                core::str::from_utf8(&dev_desc.compatible[..compat_len]).unwrap_or("???");

            let node = DeviceNode {
                id: i,
                compatible: String::from(compat_str),
                base_addr: dev_desc.base_addr,
                size: dev_desc.size,
                irq: dev_desc.irq,
                kind: dev_desc.kind,
                parent_id,
                children: Vec::new(),
            };
            nodes.push(node);
        }

        // Second pass: Build children relationships
        // Since we are pushing to `dev_mgr` which stores `Vec<DeviceNode>`, we can't easily mutate indices.
        // So we build children lists locally first.

        // A naive way:
        for i in 0..nodes.len() {
            if let Some(pid) = nodes[i].parent_id {
                if pid < nodes.len() {
                    nodes[pid].children.push(i);
                }
            }
        }

        // Add to manager
        for node in nodes {
            dev_mgr.add_node(node);
        }
    }
}
