use super::DeviceState;
use super::platform::DeviceId;
use crate::layout::MMIO_CAP;
use crate::log;
use crate::unicorn::UnicornManager;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use glenda::arch::mem::PGSIZE;
use glenda::cap::{Endpoint, Frame, IrqHandler, Rights};
use glenda::error::Error;
use glenda::interface::{DeviceService, ResourceService};
use glenda::ipc::Badge;
use glenda::protocol::device::DeviceDescNode;
use glenda::protocol::resource::ResourceType;
use glenda::utils::manager::CSpaceService;

impl<'a> UnicornManager<'a> {
    fn scan_subtree(&mut self, start_id: DeviceId) -> Result<(), Error> {
        // BFS traversal to find ready nodes starting from a specific node
        let mut queue = VecDeque::new();
        queue.push_back(start_id);

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

    fn query_recursive(
        &self,
        id: DeviceId,
        query: &glenda::protocol::device::DeviceQuery,
        results: &mut Vec<alloc::string::String>,
    ) {
        if let Some(node) = self.tree.get_node(id) {
            if query.compatible.is_empty() {
                results.push(node.desc.name.clone());
            } else {
                for comp in &query.compatible {
                    if node.desc.compatible.contains(comp) {
                        results.push(node.desc.name.clone());
                        break;
                    }
                }
            }
            for child in &node.children {
                self.query_recursive(*child, query, results);
            }
        }
    }

    fn find_desc_recursive(
        &self,
        id: DeviceId,
        name: &str,
    ) -> Option<glenda::protocol::device::DeviceDesc> {
        if let Some(node) = self.tree.get_node(id) {
            if node.desc.name == name {
                return Some(node.desc.clone());
            }
            for child in &node.children {
                if let Some(desc) = self.find_desc_recursive(*child, name) {
                    return Some(desc);
                }
            }
        }
        None
    }
}

impl<'a> DeviceService for UnicornManager<'a> {
    fn scan_platform(&mut self, _badge: Badge) -> Result<(), Error> {
        if let Some(root) = self.tree.root { self.scan_subtree(root) } else { Ok(()) }
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
            self.scan_subtree(node_id)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn update(
        &mut self,
        badge: Badge,
        compatible: Vec<alloc::string::String>,
    ) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(node_id) = self.pids.remove(&driver_id) {
            let node = self.tree.get_node_mut(node_id).ok_or(Error::InvalidArgs)?;
            node.desc.compatible = compatible;
            node.state = super::DeviceState::Ready;
            // Scan to start the new driver
            self.scan_subtree(node_id)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn register_logic(
        &mut self,
        _badge: Badge,
        desc: glenda::protocol::device::LogicDeviceDesc,
        endpoint: glenda::cap::CapPtr,
    ) -> Result<(), Error> {
        let ep = if !endpoint.is_null() {
            let slot = self.cspace_mgr.alloc(self.res_client)?;
            if let Some(b) = desc.badge {
                self.cspace_mgr.root().mint(endpoint, slot, Badge::new(b as usize), Rights::ALL)?;
                // After minting a badged copy, we don't need the original cap?
                // Actually the IPC moved the original cap to our recv slot.
                // We should probably delete it or move it somewhere else.
            } else {
                self.cspace_mgr.root().move_cap(endpoint, slot)?;
            }
            slot
        } else {
            return Err(Error::InvalidArgs);
        };

        let name = match desc.dev_type {
            glenda::protocol::device::LogicDeviceType::RawBlock(_) => {
                let n = alloc::format!("disk{}", self.disk_count);
                self.disk_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Net => {
                let n = alloc::format!("net{}", self.net_count);
                self.net_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Fb => {
                let n = alloc::format!("fb{}", self.fb_count);
                self.fb_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Uart => {
                let n = alloc::format!("uart{}", self.uart_count);
                self.uart_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Input => {
                let n = alloc::format!("input{}", self.input_count);
                self.input_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Gpio => {
                let n = alloc::format!("gpio{}", self.gpio_count);
                self.gpio_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Platform => {
                let n = alloc::format!("platform{}", self.platform_count);
                self.platform_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Thermal => {
                let n = alloc::format!("thermal{}", self.thermal_count);
                self.thermal_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Battery => {
                let n = alloc::format!("battery{}", self.battery_count);
                self.battery_count += 1;
                n
            }
            glenda::protocol::device::LogicDeviceType::Block(_) => {
                let count = self
                    .logical_devices
                    .values()
                    .filter(|(d, _, _)| {
                        matches!(d.dev_type, glenda::protocol::device::LogicDeviceType::Block(_))
                            && d.parent_name == desc.parent_name
                    })
                    .count();
                alloc::format!("{}p{}", desc.parent_name, count + 1)
            }
        };

        let id = self.next_logic_id;
        self.next_logic_id += 1;

        log!("Registering logical device: {} -> {:?}", name, ep);

        // For raw devices, store the driver's endpoint directly.
        self.logical_devices.insert(id, (desc.clone(), ep, name.clone()));

        if let glenda::protocol::device::LogicDeviceType::RawBlock(_) = desc.dev_type {
            log!("Triggering partition probe for {}", name);

            let partitions = self.probe_partitions(Endpoint::from(ep), &name)?;

            for p_desc in partitions {
                let p_idx = self.next_logic_id;
                self.next_logic_id += 1;

                // For sub-devices (partitions), mint a badged copy of Unicorn's own endpoint.
                let slot = self.cspace_mgr.alloc(self.res_client)?;
                self.cspace_mgr.root().mint(
                    self.endpoint.cap(),
                    slot,
                    Badge::new(p_idx),
                    Rights::ALL,
                )?;

                let p_name = {
                    let count = self
                        .logical_devices
                        .values()
                        .filter(|(d, _, _)| {
                            matches!(
                                d.dev_type,
                                glenda::protocol::device::LogicDeviceType::Block(_)
                            ) && d.parent_name == p_desc.parent_name
                        })
                        .count();
                    alloc::format!("{}p{}", p_desc.parent_name, count + 1)
                };

                log!("Registered logical proxy: {} (badge: {})", p_name, p_idx);
                self.logical_devices.insert(p_idx, (p_desc, slot, p_name));
            }
        }
        Ok(())
    }

    fn alloc_logic(
        &mut self,
        _badge: Badge,
        dev_type: u32,
        criteria: &str,
    ) -> Result<Endpoint, Error> {
        // dev_type: 1=RawBlock, 2=Block, 3=Net, 4=Fb
        for (id, (desc, _ep, name)) in self.logical_devices.iter() {
            let matched = match (&desc.dev_type, dev_type) {
                (glenda::protocol::device::LogicDeviceType::RawBlock(_), 1) => true,
                (glenda::protocol::device::LogicDeviceType::Block(_), 2) => true,
                (glenda::protocol::device::LogicDeviceType::Net, 3) => true,
                (glenda::protocol::device::LogicDeviceType::Fb, 4) => true,
                (glenda::protocol::device::LogicDeviceType::Uart, 5) => true,
                (glenda::protocol::device::LogicDeviceType::Input, 6) => true,
                (glenda::protocol::device::LogicDeviceType::Gpio, 7) => true,
                (glenda::protocol::device::LogicDeviceType::Platform, 8) => true,
                (glenda::protocol::device::LogicDeviceType::Thermal, 9) => true,
                (glenda::protocol::device::LogicDeviceType::Battery, 10) => true,
                _ => false,
            };
            if matched && name == criteria {
                let slot = self.cspace_mgr.alloc(self.res_client)?;
                self.cspace_mgr.root().mint(
                    self.endpoint.cap(),
                    slot,
                    Badge::new(*id),
                    Rights::ALL,
                )?;
                return Ok(Endpoint::from(slot));
            }
        }
        Err(Error::NotFound)
    }

    fn query(
        &mut self,
        _badge: Badge,
        query: glenda::protocol::device::DeviceQuery,
    ) -> Result<Vec<alloc::string::String>, Error> {
        let mut results = Vec::new();
        if let Some(root) = self.tree.root {
            self.query_recursive(root, &query, &mut results);
        }
        // Also add logical devices if no compatible filter is provided or matches name
        for (_id, (_desc, _ep, name)) in self.logical_devices.iter() {
            if query.compatible.is_empty() {
                results.push(name.clone());
            }
        }
        Ok(results)
    }

    fn get_desc(
        &mut self,
        _badge: Badge,
        name: &str,
    ) -> Result<glenda::protocol::device::DeviceDesc, Error> {
        if let Some(root) = self.tree.root {
            return self.find_desc_recursive(root, name).ok_or(Error::NotFound);
        }
        Err(Error::NotFound)
    }
}

impl<'a> glenda::interface::ThermalService for UnicornManager<'a> {
    fn get_thermal_zones(
        &mut self,
    ) -> Result<glenda::protocol::device::thermal::ThermalZones, Error> {
        let mut all_zones = glenda::protocol::device::thermal::ThermalZones::default();
        for (zones, _) in self.thermal_zones.values() {
            all_zones.zones.extend(zones.zones.clone());
        }
        Ok(all_zones)
    }

    fn update_thermal_zones(
        &mut self,
        badge: Badge,
        zones: glenda::protocol::device::thermal::ThermalZones,
    ) -> Result<(), Error> {
        let driver_id = badge.bits();
        let node_name = if let Some(&node_id) = self.pids.get(&driver_id) {
            self.tree
                .get_node(node_id)
                .map(|n| n.desc.name.clone())
                .unwrap_or_else(|| "unknown".into())
        } else {
            "unknown".into()
        };

        self.thermal_zones.insert(driver_id, (zones, node_name));
        Ok(())
    }
}
