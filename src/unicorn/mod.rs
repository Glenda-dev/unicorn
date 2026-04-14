use crate::config::Manifest;
use crate::unicorn::platform::{DeviceId, DeviceTree};
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use glenda::cap::{CapPtr, Endpoint, Reply};
use glenda::client::{InitClient, ProcessClient, ResourceClient};
use glenda::drivers::protocol::thermal::ThermalZones;
use glenda::protocol::device::HookTarget;
use glenda::protocol::init::ServiceState;
use glenda::utils::manager::{CSpaceManager, VSpaceManager};

pub mod device;
pub mod init;
pub mod logic;
pub mod platform;
pub mod server;

use logic::LogicDeviceService;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BringupPhase {
    Discovering,
    Planning,
    Spawning,
    Probing,
    Ready,
    Failed,
}

pub struct UnicornIpc {
    pub running: bool,
    pub endpoint: Endpoint,
    pub reply: Reply,
    pub recv: CapPtr,
}

pub struct UnicornManager<'a> {
    pub ipc: UnicornIpc,
    pub cspace_mgr: &'a mut CSpaceManager,
    pub vspace_mgr: &'a mut VSpaceManager,
    pub res_client: &'a mut ResourceClient,
    pub proc_client: &'a mut ProcessClient,
    pub init_client: &'a mut InitClient,
    pub config: Manifest,
    pub tree: DeviceTree,
    pub pids: BTreeMap<usize, DeviceId>, // driver_badge -> node_id
    pub driver_states: BTreeMap<usize, ServiceState>,
    pub irqs: BTreeMap<usize, DeviceId>, // irq_num -> node_id
    pub irq_caps: BTreeMap<usize, CapPtr>,
    pub mmio_caps: BTreeMap<usize, CapPtr>, // base_addr -> slot
    pub logic_service: LogicDeviceService,
    pub thermal_zones: BTreeMap<usize, (ThermalZones, String)>, // (zones, driver_name)
    pub hooks: Vec<(HookTarget, CapPtr)>,
    pub spawn_queue: VecDeque<DeviceId>,
    pub queued_nodes: BTreeSet<DeviceId>,
    pub node_driver_names: BTreeMap<DeviceId, String>,
    pub bringup_phase: BringupPhase,
    pub blocked_count: usize,
    pub running_reported: bool,
}

impl<'a> UnicornManager<'a> {
    pub fn new(
        cspace_mgr: &'a mut CSpaceManager,
        vspace_mgr: &'a mut VSpaceManager,
        res_client: &'a mut ResourceClient,
        proc_client: &'a mut ProcessClient,
        init_client: &'a mut InitClient,
    ) -> Self {
        Self {
            ipc: UnicornIpc {
                running: false,
                endpoint: Endpoint::from(CapPtr::null()),
                reply: Reply::from(CapPtr::null()),
                recv: CapPtr::null(),
            },
            cspace_mgr,
            vspace_mgr,
            res_client,
            proc_client,
            init_client,
            config: Manifest::new(),
            tree: DeviceTree::new(),
            pids: BTreeMap::new(),
            driver_states: BTreeMap::new(),
            irqs: BTreeMap::new(),
            irq_caps: BTreeMap::new(),
            mmio_caps: BTreeMap::new(),
            logic_service: LogicDeviceService::new(),
            thermal_zones: BTreeMap::new(),
            hooks: Vec::new(),
            spawn_queue: VecDeque::new(),
            queued_nodes: BTreeSet::new(),
            node_driver_names: BTreeMap::new(),
            bringup_phase: BringupPhase::Discovering,
            blocked_count: usize::MAX,
            running_reported: false,
        }
    }
}
