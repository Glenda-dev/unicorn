use glenda::error::Error;
use glenda::protocol::device::DeviceDesc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// 1. 强类型的 ID (句柄)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DeviceId {
    index: u32,      // 在 Vec 中的数组下标
    generation: u32, // 代数 (用于解决 ABA 问题)
}

// 2. 树节点
pub struct DeviceNode {
    parent: Option<DeviceId>,
    children: Vec<DeviceId>, // 子节点列表
    id: DeviceId,
    desc: DeviceDesc,   // 设备描述符
    state: DeviceState, // 设备状态 (如已初始化、未初始化等)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceState {
    Ready,
    NotReady,
    Error,
}

pub struct DeviceTree {
    nodes: Vec<Option<DeviceNode>>,
    generations: Vec<u32>,
    free_head: Option<u32>,
    pub root: Option<DeviceId>, // System Root (Usually "platform")
}

impl DeviceTree {
    pub const fn new() -> Self {
        Self { nodes: Vec::new(), generations: Vec::new(), free_head: None, root: None }
    }

    pub fn insert(
        &mut self,
        parent_id: Option<DeviceId>,
        desc: DeviceDesc,
    ) -> Result<DeviceId, Error> {
        // Validate parent if provided
        if let Some(pid) = parent_id {
            if !self.contains(pid) {
                return Err(Error::InvalidArgs);
            }
        }

        let idx = if let Some(head) = self.free_head {
            self.free_head = None; // Simplified free list logic for now
            head
        } else {
            let idx = self.nodes.len() as u32;
            self.nodes.push(None);
            self.generations.push(0);
            idx
        };

        let id = DeviceId { index: idx, generation: self.generations[idx as usize] };

        let node = DeviceNode {
            parent: parent_id,
            children: Vec::new(),
            id,
            desc,
            state: DeviceState::Ready,
        };

        self.nodes[idx as usize] = Some(node);

        // Link to parent
        if let Some(pid) = parent_id {
            // Use index to avoid double borrow issues with helper methods
            if let Some(Some(p_node)) = self.nodes.get_mut(pid.index as usize) {
                if p_node.id.generation == pid.generation {
                    p_node.children.push(id);
                }
            }
        } else {
            if self.root.is_none() {
                self.root = Some(id);
            }
        }

        Ok(id)
    }

    pub fn get_node(&self, id: DeviceId) -> Option<&DeviceNode> {
        self.nodes.get(id.index as usize)?.as_ref().filter(|n| n.id.generation == id.generation)
    }

    pub fn get_node_mut(&mut self, id: DeviceId) -> Option<&mut DeviceNode> {
        let current_gen = *self.generations.get(id.index as usize)?;
        if current_gen != id.generation {
            return None;
        }
        self.nodes.get_mut(id.index as usize)?.as_mut()
    }

    pub fn contains(&self, id: DeviceId) -> bool {
        self.get_node(id).is_some()
    }
}

use glenda::protocol::device::DeviceDescNode;

impl DeviceTree {
    /// Mount a subtree reported by a driver under `mount_point`.
    /// `nodes` is a flattened list of nodes where `parent` is an index into `nodes`.
    /// If `parent` == usize::MAX, it attaches to `mount_point`.
    pub fn mount_subtree(
        &mut self,
        mount_point: DeviceId,
        nodes: Vec<DeviceDescNode>,
    ) -> Result<(), Error> {
        if !self.contains(mount_point) {
            return Err(Error::InvalidArgs);
        }

        // Map from `nodes` index to real `DeviceId`
        let mut index_map: BTreeMap<usize, DeviceId> = BTreeMap::new();

        for (i, node_desc) in nodes.into_iter().enumerate() {
            let parent_id = if node_desc.parent == usize::MAX {
                mount_point
            } else {
                *index_map.get(&node_desc.parent).ok_or(Error::InvalidArgs)?
            };

            let new_id = self.insert(Some(parent_id), node_desc.desc)?;
            index_map.insert(i, new_id);
        }

        Ok(())
    }
}
