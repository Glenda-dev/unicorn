use super::{BringupPhase, UnicornManager};
use crate::layout::IRQ_CONTROL_CAP;
use crate::unicorn::platform::{DeviceId, DeviceSource, DeviceState};
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use glenda::error::Error;
use glenda::interface::{InitService, ProcessService};
use glenda::ipc::Badge;
use glenda::protocol::device::{DeviceDesc, MMIORegion};
use glenda::protocol::init::ServiceState;
use glenda::utils::bootinfo::{BootInfo, PlatformType};

impl<'a> UnicornManager<'a> {
    fn refresh_driver_hints(&mut self) {
        let Some(root) = self.tree.root else {
            return;
        };

        let mut queue = VecDeque::new();
        queue.push_back(root);

        while let Some(id) = queue.pop_front() {
            if let Some(node) = self.tree.get_node(id) {
                let node_name = node.desc.name.clone();
                let node_compat = node.desc.compatible.clone();
                let children = node.children.clone();

                let (matched, declared, missing) =
                    if let Some(entry) = self.match_driver_entry(&node_name, &node_compat) {
                        (Some(entry.name.clone()), Vec::new(), Vec::new())
                    } else {
                        (None, Vec::new(), Vec::new())
                    };

                let _ = self.tree.set_driver_hint(id, matched, declared, missing);

                for child in children {
                    queue.push_back(child);
                }
            }
        }
    }

    pub(super) fn init_root_platform(&mut self) -> Result<(), Error> {
        let bootinfo = unsafe { &*(crate::layout::BOOTINFO_ADDR as *const BootInfo) };
        let (name, addr, size, source) = match bootinfo.platform_type {
            PlatformType::ACPI => ("acpi", bootinfo.addr, bootinfo.size, DeviceSource::Acpi),
            PlatformType::DTB => ("dtb", bootinfo.addr, bootinfo.size, DeviceSource::Dtb),
            _ => return Ok(()),
        };

        log!("Initializing root platform: {}", name);

        let root_desc = DeviceDesc {
            name: String::from(name),
            compatible: Vec::new(),
            mmio: alloc::vec![MMIORegion { base_addr: addr, size }],
            irq: Vec::new(),
        };
        self.tree.insert_with_source(None, root_desc, source)?;
        self.bringup_phase = BringupPhase::Planning;
        let cpus = bootinfo.cpus as usize;
        for cpu_id in 0..cpus {
            IRQ_CONTROL_CAP.set_threshold(cpu_id, 0)?;
        }

        Ok(())
    }

    pub(super) fn init_initrd_device(&mut self) -> Result<(), Error> {
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
        self.tree.insert_with_source(self.tree.root, ramdisk_desc, DeviceSource::Runtime)?;
        Ok(())
    }

    pub(super) fn enqueue_if_absent(&mut self, id: DeviceId) {
        if self.queued_nodes.insert(id) {
            self.spawn_queue.push_back(id);
        }
    }

    fn match_driver_entry(
        &self,
        dev_name: &str,
        dev_compat: &[String],
    ) -> Option<&crate::config::DriverEntry> {
        for drv in &self.config.drivers {
            if drv.compatible.iter().any(|c| c == dev_name) {
                return Some(drv);
            }
            for dc in dev_compat {
                if drv.compatible.iter().any(|c| c == dc) {
                    return Some(drv);
                }
            }
        }
        None
    }

    pub(super) fn can_start_node(&self, id: DeviceId) -> bool {
        let Some(node) = self.tree.get_node(id) else {
            return false;
        };
        if node.state != DeviceState::Ready {
            return false;
        }
        self.match_driver_entry(&node.desc.name, &node.desc.compatible).is_some()
    }

    pub(super) fn start_driver(&mut self, id: DeviceId) -> Result<(), Error> {
        if !self.can_start_node(id) {
            return Ok(());
        }

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
        let (driver_name, drv_binary) =
            if let Some(entry) = self.match_driver_entry(&drv_name, &drv_compat) {
                (entry.name.clone(), entry.binary.clone())
            } else {
                // No driver found, ignore
                return Ok(());
            };

        log!("Starting driver {} for device {}", drv_binary, id.index);

        match self.proc_client.spawn(Badge::null(), &drv_binary) {
            Ok(pid) => {
                let old_status =
                    self.driver_states.get(&pid).copied().unwrap_or(ServiceState::Stopped);
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                self.pids.insert(pid, id);
                self.driver_states.insert(pid, ServiceState::Starting);
                self.node_driver_names.insert(id, driver_name);
                node.state = DeviceState::Starting;
                self.bringup_phase = BringupPhase::Probing;
                log!(
                    "Service {} transition: {:?} -> {:?}",
                    node.desc.name,
                    old_status,
                    ServiceState::Starting
                );
                Ok(())
            }
            Err(e) => {
                let node = self.tree.get_node_mut(id).ok_or(Error::InvalidArgs)?;
                error!("Failed to spawn driver {}: {:?}", drv_binary, e);
                log!(
                    "Service {} transition: {:?} -> {:?}",
                    node.desc.name,
                    ServiceState::Starting,
                    ServiceState::Failed
                );
                node.state = DeviceState::Error;
                self.bringup_phase = BringupPhase::Planning;
                Ok(())
            }
        }
    }

    pub(super) fn try_report_running(&mut self) {
        self.refresh_driver_hints();

        if self.running_reported {
            return;
        }

        if !self.spawn_queue.is_empty() {
            self.bringup_phase = BringupPhase::Spawning;
            return;
        }

        if self.has_pending_startable_nodes() {
            self.bringup_phase = BringupPhase::Planning;
            return;
        }

        let blocked = self.blocked_driver_nodes();
        if !blocked.is_empty() {
            self.bringup_phase = BringupPhase::Planning;
            self.blocked_count = blocked.len();
        } else {
            self.blocked_count = 0;
        }

        let has_starting_driver =
            self.driver_states.values().any(|state| *state == ServiceState::Starting);
        if has_starting_driver {
            self.bringup_phase = BringupPhase::Probing;
            return;
        }

        let all_running = self
            .pids
            .keys()
            .all(|pid| self.driver_states.get(pid).copied() == Some(ServiceState::Running));

        match self.init_client.report_service(Badge::null(), ServiceState::Running) {
            Ok(_) => {
                self.running_reported = true;
                self.bringup_phase = BringupPhase::Ready;
                if all_running && blocked.is_empty() {
                    log!("All spawned drivers are ready, unicorn reported Running");
                } else {
                    warn!(
                        "Running in degraded mode: running_drivers={}, total_spawned={}, blocked_nodes={}",
                        self.driver_states
                            .values()
                            .filter(|state| **state == ServiceState::Running)
                            .count(),
                        self.driver_states.len(),
                        blocked.len()
                    );
                }
            }
            Err(e) => {
                error!("Failed to report unicorn running state: {:?}", e);
            }
        }
    }

    fn has_pending_startable_nodes(&self) -> bool {
        let Some(root) = self.tree.root else {
            return false;
        };

        let mut queue = VecDeque::new();
        queue.push_back(root);

        while let Some(id) = queue.pop_front() {
            if let Some(node) = self.tree.get_node(id) {
                if self.can_start_node(id) {
                    return true;
                }

                for child in &node.children {
                    queue.push_back(*child);
                }
            }
        }

        false
    }

    fn blocked_driver_nodes(&self) -> Vec<(DeviceId, String, Vec<String>)> {
        let Some(root) = self.tree.root else {
            return Vec::new();
        };

        let mut blocked = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(root);

        while let Some(id) = queue.pop_front() {
            if let Some(node) = self.tree.get_node(id) {
                if node.state == DeviceState::Ready {
                    if self.match_driver_entry(&node.desc.name, &node.desc.compatible).is_none() {
                        blocked.push((
                            id,
                            node.desc.name.clone(),
                            alloc::vec!["no-matching-driver".to_string()],
                        ));
                    }
                }

                for child in &node.children {
                    queue.push_back(*child);
                }
            }
        }

        blocked
    }
}
