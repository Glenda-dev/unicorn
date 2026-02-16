use crate::config::Manifest;
use crate::log;
use crate::unicorn::platform::{DeviceId, DeviceState, DeviceTree};
use alloc::collections::BTreeMap;
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

        log!("Initializing root platform: {}", name);

        let root_desc = DeviceDesc {
            name: String::from(name),
            compatible: Vec::new(),
            mmio: alloc::vec![MMIORegion { base_addr: addr, size }],
            irq: Vec::new(),
        };
        self.tree.insert(None, root_desc)?;
        Ok(())
    }
    fn start_driver(&mut self, id: DeviceId) -> Result<(), Error> {
        // 1. Get Node and clone name to release borrow
        let (drv_name, drv_compat) = {
            let node_ref = self.tree.get_node(id).ok_or(Error::InvalidArgs)?;
            if node_ref.state != DeviceState::Ready {
                return Ok(());
            }
            (node_ref.desc.name.clone(), node_ref.desc.compatible.clone())
        };

        // 2. Match driver
        // Simplified matching: check by name or compatible string for now
        // In real world, use PCI ID / Compatible string

        let drv_binary = if let Some(bin) = self.match_driver(&drv_name, &drv_compat) {
            bin.to_string()
        } else {
            // No driver found, ignore
            return Ok(());
        };

        log!("Checking driver for device: {}", drv_name);
        log!("Starting driver {} for device {}", drv_binary, drv_name);

        match self.proc_client.spawn(Badge::null(), &drv_binary) {
            Ok(pid) => {
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                self.pids.insert(pid, id);
                node.state = DeviceState::Running;
                Ok(())
            }
            Err(e) => {
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                log!("Failed to spawn driver {}: {:?}", drv_binary, e);
                node.state = DeviceState::Error;
                Err(e)
            }
        }
    }

    fn match_driver(&self, dev_name: &str, dev_compat: &[String]) -> Option<&str> {
        // Iterate over manifest drivers
        for drv in &self.config.drivers {
            // Simple match: if driver name matches device name
            // Or if driver handles the "device_name"
            if drv.compatible.iter().any(|c| c == dev_name) {
                return Some(&drv.name);
            }
            // Check if driver matches any of the device's compatible strings
            for dc in dev_compat {
                if drv.compatible.iter().any(|c| c == dc) {
                    return Some(&drv.name);
                }
            }
        }
        None
    }
}
