use super::DeviceState;
use super::platform::DeviceId;
use crate::layout::MMIO_CAP;
use crate::unicorn::UnicornManager;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use glenda::arch::mem::PGSIZE;
use glenda::cap::{CapPtr, Endpoint, Frame, IrqHandler, Rights};
use glenda::error::Error;
use glenda::interface::{DeviceService, ResourceService};
use glenda::ipc::{Badge, MsgTag, UTCB};
use glenda::protocol::device::{self, DeviceDescNode, HookTarget, LogicDeviceDesc};
use glenda::protocol::resource::ResourceType;
use glenda::utils::manager::CSpaceService;

impl<'a> UnicornManager<'a> {
    fn scan_subtree(&mut self, start_id: DeviceId) -> Result<(), Error> {
        let mut queue = VecDeque::new();
        queue.push_back(start_id);

        while let Some(id) = queue.pop_front() {
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

    fn notify_hook_on_logic(
        &self,
        logic_id: usize,
        hooks: &[(HookTarget, CapPtr)],
    ) -> Result<(), Error> {
        let (desc, ep, name) =
            self.logical_devices.get(&logic_id).cloned().ok_or(Error::NotFound)?;

        let mut notify_eps = Vec::new();
        for (target, hook_ep) in hooks {
            let notify = match target {
                HookTarget::Endpoint(e) => *e == ep.bits() as u64,
                HookTarget::Type(t) => {
                    core::mem::discriminant(t) == core::mem::discriminant(&desc.dev_type)
                }
            };
            if notify {
                notify_eps.push(*hook_ep);
            }
        }

        for hook_ep in notify_eps {
            log!("Notifying hook {:?} for logic device {}", hook_ep, name);
            let mut utcb = unsafe { UTCB::new() };
            utcb.clear();
            utcb.set_msg_tag(MsgTag::new(
                glenda::protocol::DEVICE_PROTO,
                device::NOTIFY_HOOK,
                glenda::ipc::MsgFlags::NONE,
            ));
            Endpoint::from(hook_ep).notify(&mut utcb)?;
        }
        Ok(())
    }

    fn find_node_by_name(&self, name: &str) -> Option<DeviceId> {
        if let Some(root) = self.tree.root {
            let mut queue = VecDeque::new();
            queue.push_back(root);
            while let Some(id) = queue.pop_front() {
                if let Some(node) = self.tree.get_node(id) {
                    if node.desc.name == name {
                        return Some(id);
                    }
                    for child in &node.children {
                        queue.push_back(*child);
                    }
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

        let slot = self.cspace_mgr.alloc(self.res_client)?;
        if name == "dtb" || name == "acpi" {
            self.res_client.get_cap(Badge::new(driver_id), ResourceType::Mmio, base_addr, slot)?;
        } else {
            let pages = (size + PGSIZE - 1) / PGSIZE;
            MMIO_CAP.get_frame(base_addr, pages, slot)?;
        }
        Ok((Frame::from(slot), base_addr, size))
    }

    fn get_irq(&mut self, badge: Badge, id: usize) -> Result<IrqHandler, Error> {
        let driver_id = badge.bits();
        let &node_id = self.pids.get(&driver_id).ok_or(Error::InvalidArgs)?;

        let irq_num = {
            let node = self.tree.get_node(node_id).ok_or(Error::InvalidArgs)?;
            if id >= node.desc.irq.len() {
                return Err(Error::InvalidArgs);
            }
            node.desc.irq[id]
        };

        let slot = self.cspace_mgr.alloc(self.res_client)?;
        self.res_client.get_cap(Badge::new(driver_id), ResourceType::Irq, irq_num, slot)?;
        Ok(IrqHandler::from(slot))
    }

    fn report(&mut self, badge: Badge, desc: Vec<DeviceDescNode>) -> Result<(), Error> {
        let driver_id = badge.bits();
        if let Some(&node_id) = self.pids.get(&driver_id) {
            self.tree.mount_subtree(node_id, desc)?;
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
            {
                let node = self.tree.get_node_mut(node_id).ok_or(Error::InvalidArgs)?;
                node.desc.compatible = compatible;
                node.state = super::DeviceState::Ready;
            }
            self.scan_subtree(node_id)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn register_logic(
        &mut self,
        _badge: Badge,
        desc: LogicDeviceDesc,
        endpoint: CapPtr,
    ) -> Result<(), Error> {
        let ep = if !endpoint.is_null() {
            let slot = self.cspace_mgr.alloc(self.res_client)?;
            if let Some(b) = desc.badge {
                self.cspace_mgr.root().mint(endpoint, slot, Badge::new(b as usize), Rights::ALL)?;
                let _ = self.cspace_mgr.root().delete(endpoint);
            } else {
                self.cspace_mgr.root().move_cap(endpoint, slot)?;
            }
            slot
        } else {
            return Err(Error::InvalidArgs);
        };

        let name = match desc.dev_type {
            device::LogicDeviceType::RawBlock(_) => {
                let n = alloc::format!("disk{}", self.disk_count);
                self.disk_count += 1;
                n
            }
            device::LogicDeviceType::Block(_) => {
                let count = self
                    .logical_devices
                    .values()
                    .filter(|(d, _, _)| {
                        matches!(d.dev_type, device::LogicDeviceType::Block(_))
                            && d.parent_name == desc.parent_name
                    })
                    .count();
                alloc::format!("{}p{}", desc.parent_name, count + 1)
            }
            _ => {
                let n = alloc::format!("logic{}", self.next_logic_id);
                n
            }
        };

        log!("Registering logical device: {} -> {:?}", name, ep);

        let id = self.next_logic_id;
        self.next_logic_id += 1;

        self.logical_devices.insert(id, (desc.clone(), ep, name.clone()));

        if let Some(node_id) = self.find_node_by_name(&desc.parent_name) {
            if let Some(node) = self.tree.get_node_mut(node_id) {
                node.logical_devices.push(id);
            }
        }

        self.notify_hook_on_logic(id, &self.hooks)
    }

    fn alloc_logic(
        &mut self,
        _badge: Badge,
        dev_type: u32,
        criteria: &str,
    ) -> Result<Endpoint, Error> {
        for (_id, (desc, ep, name)) in self.logical_devices.iter() {
            let matched = match (&desc.dev_type, dev_type) {
                (device::LogicDeviceType::RawBlock(_), 1) => true,
                (device::LogicDeviceType::Block(_), 2) => true,
                _ => false,
            };
            if matched && name == criteria {
                let slot = self.cspace_mgr.alloc(self.res_client)?;
                self.cspace_mgr.root().mint(*ep, slot, _badge, Rights::ALL)?;
                return Ok(Endpoint::from(slot));
            }
        }
        Err(Error::NotFound)
    }

    fn query(
        &mut self,
        _badge: Badge,
        query: device::DeviceQuery,
    ) -> Result<Vec<alloc::string::String>, Error> {
        let mut results = Vec::new();
        for (_id, (desc, _ep, assigned_name)) in self.logical_devices.iter() {
            let mut matched = true;

            // 1. Match by name (logic device descriptor name OR assigned name)
            if let Some(qn) = &query.name {
                if !assigned_name.contains(qn) && !desc.name.contains(qn) {
                    matched = false;
                }
            }

            // 2. Match by compatibility (if specified)
            if matched && !query.compatible.is_empty() {
                if !query.compatible.iter().any(|c| assigned_name == c || *c == desc.name) {
                    matched = false;
                }
            }

            // 3. Match by device type (if specified using u32 identifier)
            if matched && query.dev_type.is_some() {
                let qtype = query.dev_type.unwrap();
                let type_match = match (&desc.dev_type, qtype) {
                    (device::LogicDeviceType::RawBlock(_), 1) => true,
                    (device::LogicDeviceType::Block(_), 2) => true,
                    (device::LogicDeviceType::Net, 3) => true,
                    (device::LogicDeviceType::Fb, 4) => true,
                    (device::LogicDeviceType::Uart, 5) => true,
                    (device::LogicDeviceType::Input, 6) => true,
                    (device::LogicDeviceType::Gpio, 7) => true,
                    (device::LogicDeviceType::Platform, 8) => true,
                    (device::LogicDeviceType::Thermal, 9) => true,
                    (device::LogicDeviceType::Battery, 10) => true,
                    _ => false,
                };
                if !type_match {
                    matched = false;
                }
            }

            if matched {
                results.push(assigned_name.clone());
            }
        }
        Ok(results)
    }

    fn get_desc(&mut self, _badge: Badge, name: &str) -> Result<device::DeviceDesc, Error> {
        if let Some(root) = self.tree.root {
            let mut queue = VecDeque::new();
            queue.push_back(root);
            while let Some(id) = queue.pop_front() {
                if let Some(node) = self.tree.get_node(id) {
                    if node.desc.name == name {
                        return Ok(node.desc.clone());
                    }
                    for child in &node.children {
                        queue.push_back(*child);
                    }
                }
            }
        }
        Err(Error::NotFound)
    }

    fn get_logic_desc(
        &mut self,
        _badge: Badge,
        name: &str,
    ) -> Result<(u64, LogicDeviceDesc), Error> {
        log!("Getting logic desc for {}", name);
        for (id, (desc, _ep, dev_name)) in self.logical_devices.iter() {
            if dev_name == name {
                return Ok((*id as u64, desc.clone()));
            }
        }
        Err(Error::NotFound)
    }

    fn hook(&mut self, _badge: Badge, target: HookTarget, endpoint: CapPtr) -> Result<(), Error> {
        log!("Registering hook for target {:?} at endpoint {:?}", target, endpoint);
        let slot = self.cspace_mgr.alloc(self.res_client)?;
        self.cspace_mgr.root().move_cap(endpoint, slot)?;
        let new_hook = (target, slot);
        self.hooks.push(new_hook);

        let logic_ids: Vec<usize> = self.logical_devices.keys().cloned().collect();
        for id in logic_ids {
            let hook_ref = self.hooks.last().expect("Hooks shouldn't be empty");
            let _ = self.notify_hook_on_logic(id, core::slice::from_ref(hook_ref));
        }

        Ok(())
    }

    fn unhook(&mut self, _badge: Badge, _target: HookTarget) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}
