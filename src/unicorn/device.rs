use super::DeviceState;
use crate::layout::MMIO_CAP;
use crate::unicorn::UnicornManager;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use glenda::arch::mem::PGSIZE;
use glenda::cap::{Frame, IrqHandler};
use glenda::error::Error;
use glenda::interface::{DeviceService, ResourceService};
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceDescNode;
use glenda::protocol::resource::ResourceType;
use glenda::utils::manager::CSpaceService;

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
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
        self.tree.print();
        Ok(())
    }

    fn get_mmio(&mut self, badge: Badge, id: usize) -> Result<(Frame, usize, usize), Error> {
        // 1. Find device node by driver badge
        let driver_id = badge.bits();
        let &node_id = self.pids.get(&driver_id).ok_or(Error::InvalidArgs)?;

        let (base_addr, size, name) = {
            let node = self.tree.get_node(node_id).ok_or(Error::InvalidArgs)?;
            if id >= node.desc.mmio.len() {
                return Err(Error::InvalidArgs);
            }
            let region = &node.desc.mmio[id];
            (region.base_addr, region.size, node.desc.name.clone())
        };

        // 2. Alloc slot for the MMIO capability
        let slot = self.cspace_mgr.alloc(self.res_client)?;

        // 3. Request MMIO capability
        if name == "dtb" || name == "acpi" {
            // For platform drivers, request from resource manager (kernel/warren)
            self.res_client.get_cap(Badge::new(driver_id), ResourceType::Mmio, base_addr, slot)?;
        } else {
            // For other drivers, slice from our MMIO cap
            let pages = (size + PGSIZE - 1) / PGSIZE;

            // Check if we already have it?
            // Ideally we should cache, but for now just mint new frame.
            // MMIO_CAP is our handle to the IO Space. We slice it.
            MMIO_CAP.get_frame(base_addr, pages, slot)?;
        }

        Ok((Frame::from(slot), base_addr, size))
    }

    fn get_irq(&mut self, badge: Badge, id: usize) -> Result<IrqHandler, Error> {
        let driver_id = badge.bits();
        // 1. Find device node by driver badge
        let &node_id = self.pids.get(&driver_id).ok_or(Error::InvalidArgs)?;

        let irq_num = {
            let node = self.tree.get_node(node_id).ok_or(Error::InvalidArgs)?;
            if id >= node.desc.irq.len() {
                return Err(Error::InvalidArgs);
            }
            node.desc.irq[id]
        };

        // 2. Alloc slot for IRQ capability
        let slot = self.cspace_mgr.alloc(self.res_client)?;

        // 3. Request IRQ capability from Resource Manager
        self.res_client.get_cap(Badge::new(driver_id), ResourceType::Irq, irq_num, slot)?;

        Ok(IrqHandler::from(slot))
    }

    fn report(&mut self, badge: Badge, desc: Vec<DeviceDescNode>) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(&node_id) = self.pids.get(&driver_id) {
            self.tree.mount_subtree(node_id, desc)?;
            // Automatically scan to start drivers for new devices
            UnicornManager::scan_platform(self, Badge::null())
        } else {
            Err(Error::InvalidArgs)
        }
    }
}
