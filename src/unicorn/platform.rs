use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use glenda::error::Error;
use glenda::protocol::device::{DeviceDesc, DeviceNodeMeta, MMIORegion};

// 1. 强类型的 ID (句柄)
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct DeviceId {
    pub index: u32,  // 在 Vec 中的数组下标
    generation: u32, // 代数 (用于解决 ABA 问题)
}

// 2. 树节点
pub struct DeviceNode {
    pub parent: Option<DeviceId>,
    pub children: Vec<DeviceId>, // 子节点列表
    pub id: DeviceId,
    pub source: DeviceSource,
    pub desc: DeviceDesc,            // 设备描述符
    pub meta: DeviceMeta,            // 统一元数据与驱动提示
    pub state: DeviceState,          // 设备状态 (如已初始化、未初始化等)
    pub logical_devices: Vec<usize>, // 逻辑设备列表
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceState {
    Starting,
    Running,
    Ready,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceSource {
    Unknown,
    Dtb,
    Acpi,
    Runtime,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceBus {
    Unknown,
    Platform,
    Serial,
    Pci,
    Virtio,
}

#[derive(Clone, Debug)]
pub struct DeviceResourceSummary {
    pub mmio_count: usize,
    pub irq_count: usize,
    pub mmio_span: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct DeviceDriverHint {
    pub matched_driver: Option<String>,
    pub declared_dependencies: Vec<String>,
    pub missing_dependencies: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DeviceMeta {
    pub bus: DeviceBus,
    pub unit_addr: Option<usize>,
    pub tags: Vec<String>,
    pub properties: BTreeMap<String, String>,
    pub resources: DeviceResourceSummary,
    pub driver_hint: DeviceDriverHint,
}

#[derive(Clone, Debug)]
pub struct DeviceIrNode {
    pub id: DeviceId,
    pub parent: Option<DeviceId>,
    pub children: Vec<DeviceId>,
    pub source: DeviceSource,
    pub name: alloc::string::String,
    pub compatible: Vec<alloc::string::String>,
    pub mmio: Vec<MMIORegion>,
    pub irq: Vec<usize>,
    pub logical_devices: Vec<usize>,
    pub meta: DeviceMeta,
    pub state: DeviceState,
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

    fn infer_bus(desc: &DeviceDesc) -> DeviceBus {
        if desc.compatible.iter().any(|c| c.contains("virtio")) {
            return DeviceBus::Virtio;
        }
        if desc.compatible.iter().any(|c| c.contains("pci")) {
            return DeviceBus::Pci;
        }
        if desc.compatible.iter().any(|c| c.contains("uart") || c.contains("serial")) {
            return DeviceBus::Serial;
        }
        if !desc.mmio.is_empty() {
            return DeviceBus::Platform;
        }
        DeviceBus::Unknown
    }

    fn parse_unit_addr(name: &str) -> Option<usize> {
        let (_, suffix) = name.rsplit_once('@')?;
        let suffix = suffix.trim_start_matches("0x");
        usize::from_str_radix(suffix, 16).ok()
    }

    fn summarize_resources(desc: &DeviceDesc) -> DeviceResourceSummary {
        let mut start = usize::MAX;
        let mut end = 0usize;
        for reg in &desc.mmio {
            start = core::cmp::min(start, reg.base_addr);
            end = core::cmp::max(end, reg.base_addr.saturating_add(reg.size));
        }
        DeviceResourceSummary {
            mmio_count: desc.mmio.len(),
            irq_count: desc.irq.len(),
            mmio_span: if desc.mmio.is_empty() { None } else { Some((start, end)) },
        }
    }

    fn build_meta(desc: &DeviceDesc, source: DeviceSource) -> DeviceMeta {
        let mut tags = Vec::new();
        tags.push(
            match source {
                DeviceSource::Unknown => "src:unknown",
                DeviceSource::Dtb => "src:dtb",
                DeviceSource::Acpi => "src:acpi",
                DeviceSource::Runtime => "src:runtime",
            }
            .to_string(),
        );

        for comp in &desc.compatible {
            tags.push(alloc::format!("compat:{}", comp));
        }

        DeviceMeta {
            bus: Self::infer_bus(desc),
            unit_addr: Self::parse_unit_addr(&desc.name),
            tags,
            properties: BTreeMap::new(),
            resources: Self::summarize_resources(desc),
            driver_hint: DeviceDriverHint {
                matched_driver: None,
                declared_dependencies: Vec::new(),
                missing_dependencies: Vec::new(),
            },
        }
    }

    pub fn insert(
        &mut self,
        parent_id: Option<DeviceId>,
        desc: DeviceDesc,
    ) -> Result<DeviceId, Error> {
        let source = if let Some(pid) = parent_id {
            self.get_node(pid).map(|n| n.source).unwrap_or(DeviceSource::Unknown)
        } else {
            DeviceSource::Unknown
        };
        self.insert_with_source(parent_id, desc, source)
    }

    pub fn insert_with_source(
        &mut self,
        parent_id: Option<DeviceId>,
        desc: DeviceDesc,
        source: DeviceSource,
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
            source,
            meta: Self::build_meta(&desc, source),
            desc,
            state: DeviceState::Ready,
            logical_devices: Vec::new(),
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

    pub fn print(&self) {
        if let Some(root) = self.root {
            log!("Device Tree Dump:");
            self.print_recursive(root, 0);
        } else {
            log!("Device Tree is Empty.");
        }
    }

    fn print_recursive(&self, id: DeviceId, level: usize) {
        if let Some(node) = self.get_node(id) {
            let indent = "  ".repeat(level);
            // Print basic info: name, type, and status
            let status = match node.state {
                DeviceState::Starting => "STARTING",
                DeviceState::Running => "RUNNING",
                DeviceState::Ready => "READY",
                DeviceState::Error => "ERROR",
            };
            let source = match node.source {
                DeviceSource::Unknown => "unknown",
                DeviceSource::Dtb => "dtb",
                DeviceSource::Acpi => "acpi",
                DeviceSource::Runtime => "runtime",
            };
            let bus = match node.meta.bus {
                DeviceBus::Unknown => "unknown",
                DeviceBus::Platform => "platform",
                DeviceBus::Serial => "serial",
                DeviceBus::Pci => "pci",
                DeviceBus::Virtio => "virtio",
            };

            // Format resource info if any
            let mut res_info = alloc::string::String::new();
            if !node.desc.mmio.is_empty() {
                res_info.push_str(" MMIO:[");
                for (i, reg) in node.desc.mmio.iter().enumerate() {
                    if i > 0 {
                        res_info.push_str(", ");
                    }
                    res_info.push_str(&alloc::format!(
                        "{:#x}-{:#x}",
                        reg.base_addr,
                        reg.base_addr + reg.size
                    ));
                }
                res_info.push(']');
            }
            if !node.desc.irq.is_empty() {
                res_info.push_str(" IRQ:[");
                for (i, irq) in node.desc.irq.iter().enumerate() {
                    if i > 0 {
                        res_info.push_str(", ");
                    }
                    res_info.push_str(&alloc::format!("{}", irq));
                }
                res_info.push(']');
            }

            let compat = if node.desc.compatible.is_empty() {
                alloc::string::String::from("Unknown")
            } else {
                node.desc.compatible.join(", ")
            };

            log!(
                "{} - {} ({}) [{}] <src:{} bus:{} driver:{:?}> {}",
                indent,
                node.desc.name,
                compat,
                status,
                source,
                bus,
                node.meta.driver_hint.matched_driver,
                res_info
            );

            for child in &node.children {
                self.print_recursive(*child, level + 1);
            }
        }
    }
}

use glenda::protocol::device::DeviceDescNode;

impl DeviceTree {
    fn parse_bus(value: &str) -> DeviceBus {
        match value {
            "platform" | "simple-bus" => DeviceBus::Platform,
            "serial" | "uart" => DeviceBus::Serial,
            "pci" => DeviceBus::Pci,
            "virtio" => DeviceBus::Virtio,
            _ => DeviceBus::Unknown,
        }
    }

    fn apply_reported_meta(&mut self, id: DeviceId, meta: DeviceNodeMeta) -> Result<(), Error> {
        let node = self.get_node_mut(id).ok_or(Error::NotFound)?;

        if let Some(bus) = meta.bus {
            let parsed = Self::parse_bus(bus.as_str());
            if parsed != DeviceBus::Unknown {
                node.meta.bus = parsed;
            }
        }
        if meta.unit_addr.is_some() {
            node.meta.unit_addr = meta.unit_addr;
        }
        for tag in meta.tags {
            if !node.meta.tags.iter().any(|t| t == &tag) {
                node.meta.tags.push(tag);
            }
        }
        for (k, v) in meta.properties {
            node.meta.properties.insert(k, v);
        }

        Ok(())
    }

    pub fn to_ir_node(&self, id: DeviceId) -> Option<DeviceIrNode> {
        let node = self.get_node(id)?;
        Some(DeviceIrNode {
            id,
            parent: node.parent,
            children: node.children.clone(),
            source: node.source,
            name: node.desc.name.clone(),
            compatible: node.desc.compatible.clone(),
            mmio: node.desc.mmio.clone(),
            irq: node.desc.irq.clone(),
            logical_devices: node.logical_devices.clone(),
            meta: node.meta.clone(),
            state: node.state,
        })
    }

    pub fn set_driver_hint(
        &mut self,
        id: DeviceId,
        matched_driver: Option<String>,
        declared_dependencies: Vec<String>,
        missing_dependencies: Vec<String>,
    ) -> Result<(), Error> {
        let node = self.get_node_mut(id).ok_or(Error::NotFound)?;
        node.meta.driver_hint =
            DeviceDriverHint { matched_driver, declared_dependencies, missing_dependencies };
        Ok(())
    }

    pub fn set_property(&mut self, id: DeviceId, key: &str, value: &str) -> Result<(), Error> {
        let node = self.get_node_mut(id).ok_or(Error::NotFound)?;
        node.meta.properties.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn find_by_compatible(&self, compatible: &str) -> Vec<DeviceId> {
        let mut out = Vec::new();
        for node in self.nodes.iter().flatten() {
            if node.desc.compatible.iter().any(|c| c == compatible) {
                out.push(node.id);
            }
        }
        out
    }

    pub fn find_by_bus(&self, bus: DeviceBus) -> Vec<DeviceId> {
        let mut out = Vec::new();
        for node in self.nodes.iter().flatten() {
            if node.meta.bus == bus {
                out.push(node.id);
            }
        }
        out
    }

    pub fn collect_subtree_ir(&self, root: DeviceId) -> Vec<DeviceIrNode> {
        let mut out = Vec::new();
        let mut queue = Vec::new();
        queue.push(root);

        while let Some(id) = queue.pop() {
            if let Some(ir) = self.to_ir_node(id) {
                for child in &ir.children {
                    queue.push(*child);
                }
                out.push(ir);
            }
        }

        out
    }

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

        let inherited_source =
            self.get_node(mount_point).map(|n| n.source).unwrap_or(DeviceSource::Unknown);

        // Map from `nodes` index to real `DeviceId`
        let mut index_map: BTreeMap<usize, DeviceId> = BTreeMap::new();

        for (i, node_desc) in nodes.into_iter().enumerate() {
            let DeviceDescNode { parent, desc, meta } = node_desc;
            let parent_id = if parent == usize::MAX {
                mount_point
            } else {
                *index_map.get(&parent).ok_or(Error::InvalidArgs)?
            };
            let new_id = self.insert_with_source(Some(parent_id), desc, inherited_source)?;
            self.apply_reported_meta(new_id, meta)?;
            index_map.insert(i, new_id);
        }

        Ok(())
    }
}
