use crate::config::Manifest;
use crate::unicorn::platform::{DeviceId, DeviceState, DeviceTree};
use alloc;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Endpoint, Reply};
use glenda::client::ProcessClient;
use glenda::client::ResourceClient;
use glenda::error::Error;
use glenda::interface::ProcessService;
use glenda::ipc::Badge;
use glenda::protocol::device::{DeviceDesc, MMIORegion};
use glenda::utils::bootinfo::{BootInfo, PlatformType};
use glenda::utils::manager::CSpaceManager;

pub mod device;
pub mod platform;
pub mod server;

pub struct UnicornManager<'a> {
    pub running: bool,
    pub endpoint: Endpoint,
    pub reply: Reply,
    pub recv: CapPtr,
    pub cspace_mgr: &'a mut CSpaceManager,
    pub res_client: &'a mut ResourceClient,
    pub proc_client: &'a mut ProcessClient,
    pub config: Manifest,
    pub tree: DeviceTree,
    pub pids: BTreeMap<usize, DeviceId>, // driver_badge -> node_id
    pub irqs: BTreeMap<usize, DeviceId>, // irq_num -> node_id
    pub irq_caps: BTreeMap<usize, CapPtr>,
    pub mmio_caps: BTreeMap<usize, CapPtr>, // base_addr -> slot
}

impl<'a> UnicornManager<'a> {
    pub fn new(
        cspace_mgr: &'a mut CSpaceManager,
        res_client: &'a mut ResourceClient,
        proc_client: &'a mut ProcessClient,
    ) -> Self {
        Self {
            running: false,
            endpoint: Endpoint::from(CapPtr::null()),
            reply: Reply::from(CapPtr::null()),
            recv: CapPtr::null(),
            cspace_mgr,
            res_client,
            proc_client,
            config: Manifest::new(),
            tree: DeviceTree::new(),
            pids: BTreeMap::new(),
            irqs: BTreeMap::new(),
            irq_caps: BTreeMap::new(),
            mmio_caps: BTreeMap::new(),
        }
    }

    pub fn init_root_platform(&mut self) -> Result<(), Error> {
        let bootinfo = unsafe { &*(crate::layout::BOOTINFO_ADDR as *const BootInfo) };
        let (name, addr, size) = match bootinfo.platform_type {
            PlatformType::ACPI => ("acpi", bootinfo.addr, bootinfo.size),
            PlatformType::DTB => ("dtb", bootinfo.addr, bootinfo.size),
            _ => return Ok(()),
        };

        crate::log!("Initializing root platform: {}", name);

        let root_desc = DeviceDesc {
            name: String::from(name),
            compatible: Vec::new(),
            mmio: alloc::vec![MMIORegion { base_addr: addr, size }],
            irq: Vec::new(),
        };

        let root_id = self.tree.insert(None, root_desc)?;
        self.start_driver(root_id)
    }

    pub fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
        // BFS traversal to find ready nodes
        let mut queue = VecDeque::new();
        if let Some(root) = self.tree.root {
             queue.push_back(root);
        }

        while let Some(id) = queue.pop_front() {
             // 1. Check if node needs driver
             let (needs_start, children) = if let Some(node) = self.tree.get_node(id) {
                 (node.state == DeviceState::Ready, node.children.clone())
             } else {
                 (false, alloc::vec![])
             };
             
             if needs_start {
                 let _ = self.start_driver(id);
             }
             
             for child in children {
                 queue.push_back(child);
             }
        }
        Ok(())
    }

    fn start_driver(&mut self, id: DeviceId) -> Result<(), Error> {
        // 1. Get Node and clone name to release borrow
        let drv_name = {
            let node_ref = self.tree.get_node(id).ok_or(Error::InvalidArgs)?;
            if node_ref.state != DeviceState::Ready {
                return Ok(());
            }
            node_ref.desc.name.clone()
        };

        // 2. Match driver
        // Simplified matching: check by name or compatible string for now
        // In real world, use PCI ID / Compatible string
        
        let drv_binary = if let Some(bin) = self.match_driver(&drv_name) {
            bin.to_string()
        } else {
            // No driver found, ignore
            return Ok(());
        };
        
        crate::log!("Checking driver for device: {}", drv_name);
        crate::log!("Starting driver {} for device {}", drv_binary, drv_name);
        
        match self.proc_client.spawn(Badge::null(), &drv_binary) {
            Ok(pid) => {
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                self.pids.insert(pid, id);
                node.state = DeviceState::Running;
                Ok(())
            }
            Err(e) => {
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                crate::log!("Failed to spawn driver {}: {:?}", drv_binary, e);
                node.state = DeviceState::Error;
                Err(e)
            }
        }
    }

    fn match_driver(&self, dev_name: &str) -> Option<&str> {
        // Iterate over manifest drivers
        for drv in &self.config.drivers {
            // Simple match: if driver name matches device name
            // Or if driver handles the "device_name"
            if drv.compatible.iter().any(|c| c == dev_name) {
                return Some(&drv.name);
            }
        }
        None
    }
}
