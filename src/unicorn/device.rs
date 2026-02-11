use crate::layout::{IRQ_SLOT, MMIO_SLOT};
use crate::log;
use crate::unicorn::UnicornManager;
use alloc::string::String;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Frame, IrqHandler};
use glenda::error::Error;
use glenda::interface::{DeviceService, ProcessService};
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceNode;
use glenda::utils::platform::{DeviceDesc, MemoryType};

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
        let platform = self.platform.as_ref().unwrap();
        // Match and spawn
        for i in 0..platform.device_count {
            let dev = &platform.devices[i];
            let compat = dev.compatible();
            for driver in &self.config.drivers {
                if driver.compatible.iter().any(|c| c == compat) {
                    log!("Matched driver {} for device {}", driver.name, compat);
                    match self.proc_client.spawn(Badge::null(), &driver.name) {
                        Ok(pid) => {
                            log!("Spawned driver {} with pid {}", driver.name, pid);
                            let node = DeviceNode {
                                id: i,
                                compatible: String::from(compat),
                                base_addr: dev.base_addr,
                                size: dev.size,
                                irq: dev.irq as u32,
                                kind: dev.kind,
                                parent_id: None,
                                children: Vec::new(),
                            };
                            self.nodes.push(node);
                            self.pids.insert(pid, self.nodes.len() - 1);
                        }
                        Err(e) => {
                            log!("Failed to spawn driver {}: {:?}", driver.name, e);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn find_compatible(&self, _badge: Badge, compat: &str) -> Result<DeviceNode, Error> {
        for node in &self.nodes {
            if node.compatible == compat {
                return Ok(node.clone());
            }
        }
        Err(Error::NotFound)
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
