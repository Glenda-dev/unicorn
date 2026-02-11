use crate::layout::{IRQ_SLOT, MMIO_SLOT, RESOURCE_ADDR};
use crate::log;
use crate::unicorn::UnicornManager;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Frame, IrqHandler};
use glenda::error::Error;
use glenda::interface::{DeviceService, MemoryService, ProcessService};
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceNode;
use glenda::utils::platform::{DeviceDesc, MemoryType};
use virtio_common::consts::*;

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, badge: Badge) -> Result<(), Error> {
        let mut discovered_nodes = Vec::new();
        let device_count = self.platform.as_ref().unwrap().device_count;

        // 1. Discover and probe
        for i in 0..device_count {
            let dev = self.platform.as_ref().unwrap().devices[i];
            let mut compat = String::from(dev.compatible());

            if compat == "virtio,mmio" {
                if let Ok(specific) = self.probe_virtio(&dev) {
                    log!("Probed virtio device: {} -> {}", compat, specific);
                    compat = specific;
                }
            }

            let node = DeviceNode {
                id: i,
                compatible: compat,
                base_addr: dev.base_addr,
                size: dev.size,
                irq: dev.irq as u32,
                kind: dev.kind,
                parent_id: None,
                children: Vec::new(),
            };
            discovered_nodes.push(node);
        }

        // 2. Match and spawn
        for node in discovered_nodes {
            let compat = &node.compatible;
            for driver in &self.config.drivers {
                if driver.compatible.iter().any(|c| c == compat) {
                    log!("Matched driver {} for device {}", driver.name, compat);
                    match self.proc_client.spawn(badge, &driver.name) {
                        Ok(pid) => {
                            log!("Spawned driver {} with pid {}", driver.name, pid);
                            self.nodes.push(node.clone());
                            self.pids.insert(pid, self.nodes.len() - 1);
                        }
                        Err(e) => {
                            log!("Failed to spawn driver {}: {:?}", driver.name, e);
                        }
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    fn get_desc(&mut self, badge: Badge) -> Result<DeviceDesc, Error> {
        let node_idx = *self.pids.get(&badge.bits()).ok_or(Error::PermissionDenied)?;
        let node = &self.nodes[node_idx];
        let platform = self.platform.as_ref().unwrap();
        let dev = platform.devices[node.id];
        Ok(dev)
    }

    fn get_mmio(&mut self, badge: Badge) -> Result<Frame, Error> {
        let node_idx = *self.pids.get(&badge.bits()).ok_or(Error::PermissionDenied)?;
        let node = &self.nodes[node_idx];
        let platform = self.platform.as_ref().unwrap();
        let dev = &platform.devices[node.id];

        // Find index of this MMIO region in memory_regions
        let mut mmio_idx = 0;
        let mut found = false;
        for j in 0..platform.memory_region_count {
            let reg = &platform.memory_regions[j];
            if reg.region_type == MemoryType::Mmio {
                mmio_idx += 1;
                if reg.start == dev.base_addr {
                    found = true;
                    break;
                }
            }
        }

        if !found {
            return Err(Error::NotFound);
        }
        let mmio_ptr = CapPtr::concat(MMIO_SLOT, CapPtr::from(mmio_idx));
        Ok(Frame::from(mmio_ptr))
    }

    fn get_irq(&mut self, badge: Badge) -> Result<IrqHandler, Error> {
        let node_idx = *self.pids.get(&badge.bits()).ok_or(Error::PermissionDenied)?;
        let node = &self.nodes[node_idx];
        let irq = node.irq as usize;
        let irq_ptr = CapPtr::concat(IRQ_SLOT, CapPtr::from(irq));
        Ok(IrqHandler::from(irq_ptr))
    }
}

impl<'a> UnicornManager<'a> {
    fn probe_virtio(&mut self, dev: &DeviceDesc) -> Result<String, Error> {
        let platform = self.platform.as_ref().ok_or(Error::NotFound)?;

        // Find index of this MMIO region in memory_regions
        let mut mmio_idx = 0;
        let mut found = false;
        for j in 0..platform.memory_region_count {
            let reg = &platform.memory_regions[j];
            if reg.region_type == MemoryType::Mmio {
                mmio_idx += 1;
                if reg.start == dev.base_addr {
                    found = true;
                    break;
                }
            }
        }

        if !found {
            return Err(Error::NotFound);
        }

        let mmio_ptr = CapPtr::concat(MMIO_SLOT, CapPtr::from(mmio_idx));
        let frame = Frame::from(mmio_ptr);

        // Map it to RESOURCE_ADDR
        self.res_client.mmap(Badge::null(), frame, RESOURCE_ADDR, 4096)?;

        // Read Device info
        let magic = unsafe { core::ptr::read_volatile((RESOURCE_ADDR + OFF_MAGIC) as *const u32) };
        let version =
            unsafe { core::ptr::read_volatile((RESOURCE_ADDR + OFF_VERSION) as *const u32) };
        let device_id =
            unsafe { core::ptr::read_volatile((RESOURCE_ADDR + OFF_DEVICE_ID) as *const u32) };

        log!(
            "Probe result: ptr={:?}, magic={:#x}, version={:#x}, device_id={:#x}",
            mmio_ptr,
            magic,
            version,
            device_id
        );

        // Unmap
        self.res_client.munmap(Badge::null(), RESOURCE_ADDR, 4096)?;

        if magic != MAGIC_VALUE {
            return Ok(format!("virtio,mmio,unknown-magic-{:#x}", magic));
        }

        match device_id {
            DEV_ID_NET => Ok(String::from("virtio,mmio,net")),
            DEV_ID_BLOCK => Ok(String::from("virtio,mmio,block")),
            DEV_ID_CONSOLE => Ok(String::from("virtio,mmio,console")),
            DEV_ID_GPU => Ok(String::from("virtio,mmio,gpu")),
            DEV_ID_INPUT => Ok(String::from("virtio,mmio,input")),
            _ => Ok(format!("virtio,mmio,unknown-{}", device_id)),
        }
    }
}
