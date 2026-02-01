use glenda::error::Error;
use glenda::manager::device::DeviceNode;
use glenda::manager::interface::IDeviceManager;
use glenda::utils::PlatformInfo;

pub struct DeviceManager {
    // Basic Device manager
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {}
    }
}

impl IDeviceManager for DeviceManager {
    fn scan_platform(&mut self, info: &PlatformInfo) {}
    fn find_compatible(&self, compat: &str) -> Option<&DeviceNode> {
        None
    }
    fn get_node(&self, id: usize) -> Option<&DeviceNode> {
        None
    }
}
