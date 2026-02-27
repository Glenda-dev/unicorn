use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Endpoint, Rights};
use glenda::client::ResourceClient;
use glenda::error::Error;
use glenda::ipc::Badge;
use glenda::protocol::device::{self, DeviceQuery, LogicDeviceDesc, LogicDeviceType};
use glenda::utils::manager::CSpaceService;

pub struct LogicDeviceCounter {
    pub disk: usize,
    pub net: usize,
    pub fb: usize,
    pub uart: usize,
    pub input: usize,
    pub gpio: usize,
    pub platform: usize,
    pub thermal: usize,
    pub battery: usize,
    pub timer: usize,
    pub next_id: usize,
}

impl Default for LogicDeviceCounter {
    fn default() -> Self {
        Self {
            disk: 0,
            net: 0,
            fb: 0,
            uart: 0,
            input: 0,
            gpio: 0,
            platform: 0,
            thermal: 0,
            battery: 0,
            timer: 0,
            next_id: 1,
        }
    }
}

pub struct LogicDeviceService {
    pub devices: BTreeMap<usize, (LogicDeviceDesc, CapPtr, String)>,
    pub counter: LogicDeviceCounter,
}

impl LogicDeviceService {
    pub fn new() -> Self {
        Self { devices: BTreeMap::new(), counter: LogicDeviceCounter::default() }
    }

    pub fn register(
        &mut self,
        cspace_mgr: &mut dyn CSpaceService,
        res_client: &mut ResourceClient,
        desc: LogicDeviceDesc,
        endpoint: CapPtr,
    ) -> Result<(usize, String, CapPtr), Error> {
        let ep = cspace_mgr.alloc(res_client)?;
        cspace_mgr.root().move_cap(endpoint, ep)?;

        let name = match desc.dev_type {
            device::LogicDeviceType::Block => {
                let n = alloc::format!("disk{}", self.counter.disk);
                self.counter.disk += 1;
                n
            }
            device::LogicDeviceType::Net => {
                let n = alloc::format!("net{}", self.counter.net);
                self.counter.net += 1;
                n
            }
            device::LogicDeviceType::Volume => {
                let count = self
                    .devices
                    .values()
                    .filter(|(d, _, _)| {
                        matches!(d.dev_type, device::LogicDeviceType::Volume)
                            && d.parent_name == desc.parent_name
                    })
                    .count();
                alloc::format!("{}p{}", desc.parent_name, count + 1)
            }
            device::LogicDeviceType::Timer => {
                let n = alloc::format!("timer{}", self.counter.timer);
                self.counter.timer += 1;
                n
            }
            device::LogicDeviceType::Platform => "platform".to_string(),
            device::LogicDeviceType::Fb => {
                let n = alloc::format!("fb{}", self.counter.fb);
                self.counter.fb += 1;
                n
            }
            device::LogicDeviceType::Uart => {
                let n = alloc::format!("uart{}", self.counter.uart);
                self.counter.uart += 1;
                n
            }
            device::LogicDeviceType::Input => {
                let n = alloc::format!("input{}", self.counter.input);
                self.counter.input += 1;
                n
            }
            _ => {
                let n = alloc::format!("logic{}", self.counter.next_id);
                n
            }
        };

        log!("Registering logical device: {} -> {:?}", name, ep);
        let id = self.counter.next_id;
        self.counter.next_id += 1;
        self.devices.insert(id, (desc.clone(), ep, name.clone()));
        Ok((id, name, ep))
    }

    pub fn alloc(
        &self,
        cspace_mgr: &mut dyn CSpaceService,
        res_client: &mut ResourceClient,
        badge: Badge,
        dev_type: LogicDeviceType,
        criteria: &str,
    ) -> Result<Endpoint, Error> {
        for (_id, (desc, ep, name)) in self.devices.iter() {
            if desc.dev_type == dev_type && name == criteria {
                let slot = cspace_mgr.alloc(res_client)?;
                cspace_mgr.root().mint(*ep, slot, badge, Rights::ALL)?;
                return Ok(Endpoint::from(slot));
            }
        }
        Err(Error::NotFound)
    }

    pub fn query(&self, query: DeviceQuery) -> Result<Vec<String>, Error> {
        let mut results = Vec::new();
        for (_id, (desc, _ep, assigned_name)) in self.devices.iter() {
            let mut matched = true;

            // 1. Match by name
            if let Some(qn) = &query.name {
                if !assigned_name.contains(qn) && !desc.name.contains(qn) {
                    matched = false;
                }
            }

            // 2. Match by compatibility
            if matched && !query.compatible.is_empty() {
                if !query.compatible.iter().any(|c| assigned_name == c || *c == desc.name) {
                    matched = false;
                }
            }

            // 3. Match by device type
            if matched && query.dev_type.is_some() {
                if desc.dev_type != query.dev_type.unwrap() {
                    matched = false;
                }
            }

            if matched {
                results.push(assigned_name.clone());
            }
        }
        Ok(results)
    }

    pub fn get_desc(&self, name: &str) -> Option<(usize, LogicDeviceDesc)> {
        for (id, (desc, _ep, assigned_name)) in self.devices.iter() {
            if assigned_name == name {
                return Some((*id, desc.clone()));
            }
        }
        None
    }
}
