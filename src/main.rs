#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

mod config;
mod layout;
mod unicorn;
mod utils;

use glenda::cap::CapType;
use glenda::cap::{CSPACE_CAP, ENDPOINT_CAP, ENDPOINT_SLOT, MONITOR_CAP, RECV_SLOT, REPLY_SLOT};
use glenda::client::ProcessClient;
use glenda::client::ResourceClient;
use glenda::error::Error;
use glenda::interface::{ResourceService, SystemService};
use glenda::ipc::Badge;
use glenda::utils::manager::CSpaceManager;
use unicorn::UnicornManager;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        glenda::println!("{}Unicorn: {}{}", glenda::console::ANSI_BLUE, format_args!($($arg)*), glenda::console::ANSI_RESET);
    })
}

#[unsafe(no_mangle)]
fn main() -> usize {
    log!("Starting Unicorn Device Driver Manager...");
    let mut res_client = ResourceClient::new(MONITOR_CAP);
    let mut proc_client = ProcessClient::new(MONITOR_CAP);
    res_client
        .alloc(Badge::null(), CapType::Endpoint, 0, ENDPOINT_SLOT)
        .expect("Failed to allocate endpoint cap for unicorn");
    let mut cspace_mgr = CSpaceManager::new(CSPACE_CAP, 16);
    let mut server = UnicornManager::new(&mut cspace_mgr, &mut res_client, &mut proc_client);
    if let Err(e) = load_unicorn(&mut server) {
        log!("Failed to load: {:?}", e);
        return 1;
    }
    server.run().expect("Unicorn exited");
    1
}

fn load_unicorn(server: &mut UnicornManager) -> Result<(), Error> {
    server.listen(ENDPOINT_CAP, REPLY_SLOT, RECV_SLOT)?;
    server.init()?;
    Ok(())
}
