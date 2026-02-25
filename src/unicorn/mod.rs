use crate::config::Manifest;
use crate::layout::IRQ_CONTROL_CAP;
use crate::unicorn::platform::{DeviceId, DeviceState, DeviceTree};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Endpoint, Reply};
use glenda::client::{InitClient, ProcessClient, ResourceClient};
use glenda::error::Error;
use glenda::interface::ProcessService;
use glenda::ipc::Badge;
use glenda::protocol::device::{DeviceDesc, HookTarget, LogicDeviceDesc, MMIORegion};
use glenda::utils::bootinfo::{BootInfo, PlatformType};
use glenda::utils::manager::CSpaceManager;
use glenda_drivers::protocol::thermal::ThermalZones;

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
    pub init_client: &'a mut InitClient,
    pub config: Manifest,
    pub tree: DeviceTree,
    pub pids: BTreeMap<usize, DeviceId>, // driver_badge -> node_id
    pub irqs: BTreeMap<usize, DeviceId>, // irq_num -> node_id
    pub irq_caps: BTreeMap<usize, CapPtr>,
    pub mmio_caps: BTreeMap<usize, CapPtr>, // base_addr -> slot
    pub logical_devices: BTreeMap<usize, (LogicDeviceDesc, CapPtr, String)>, // (desc, endpoint, name)
    pub thermal_zones: BTreeMap<usize, (ThermalZones, String)>,              // (zones, driver_name)
    pub hooks: Vec<(HookTarget, CapPtr)>,
    pub next_logic_id: usize,
    pub disk_count: usize,
    pub net_count: usize,
    pub fb_count: usize,
    pub uart_count: usize,
    pub input_count: usize,
    pub gpio_count: usize,
    pub platform_count: usize,
    pub thermal_count: usize,
    pub battery_count: usize,
}

impl<'a> UnicornManager<'a> {
    pub fn new(
        cspace_mgr: &'a mut CSpaceManager,
        res_client: &'a mut ResourceClient,
        proc_client: &'a mut ProcessClient,
        init_client: &'a mut InitClient,
    ) -> Self {
        Self {
            running: false,
            endpoint: Endpoint::from(CapPtr::null()),
            reply: Reply::from(CapPtr::null()),
            recv: CapPtr::null(),
            cspace_mgr,
            res_client,
            proc_client,
            init_client,
            config: Manifest::new(),
            tree: DeviceTree::new(),
            pids: BTreeMap::new(),
            irqs: BTreeMap::new(),
            irq_caps: BTreeMap::new(),
            mmio_caps: BTreeMap::new(),
            logical_devices: BTreeMap::new(),
            thermal_zones: BTreeMap::new(),
            hooks: Vec::new(),
            next_logic_id: 1,
            disk_count: 0,
            net_count: 0,
            fb_count: 0,
            uart_count: 0,
            input_count: 0,
            gpio_count: 0,
            platform_count: 0,
            thermal_count: 0,
            battery_count: 0,
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
        let cpus = bootinfo.cpus as usize;
        for cpu_id in 0..cpus {
            IRQ_CONTROL_CAP.set_threshold(cpu_id, 0)?;
        }

        Ok(())
    }

    pub fn init_initrd_device(&mut self) -> Result<(), Error> {
        let bootinfo = unsafe { &*(crate::layout::BOOTINFO_ADDR as *const BootInfo) };
        if bootinfo.initrd_size == 0 {
            return Ok(());
        }

        log!(
            "Initializing Initrd Ramdisk device (paddr={:#x}, size={:#x})",
            bootinfo.initrd_paddr,
            bootinfo.initrd_size
        );

        let ramdisk_desc = DeviceDesc {
            name: String::from("ramdisk"),
            compatible: alloc::vec![String::from("ramdisk")],
            mmio: alloc::vec![MMIORegion {
                base_addr: bootinfo.initrd_paddr,
                size: bootinfo.initrd_size,
            }],
            irq: Vec::new(),
        };

        // Add under root node
        self.tree.insert(self.tree.root, ramdisk_desc)?;
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

        log!("Starting driver {} for device {}", drv_binary, id.index);

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
