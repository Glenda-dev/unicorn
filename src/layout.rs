use glenda::cap::{CapPtr, Endpoint};

pub const BOOTINFO_SLOT: CapPtr = CapPtr::from(9);
pub const UNTYPED_SLOT: CapPtr = CapPtr::from(10);
pub const MMIO_SLOT: CapPtr = CapPtr::from(11);
pub const IRQ_SLOT: CapPtr = CapPtr::from(12);
pub const PLATFORM_SLOT: CapPtr = CapPtr::from(13);

pub const MANIFEST_SLOT: CapPtr = CapPtr::from(15);
pub const MANIFEST_CAP: Endpoint = Endpoint::from(MANIFEST_SLOT);

pub const RESOURCE_ADDR: usize = 0x3000_0000;
pub const BOOTINFO_ADDR: usize = 0x3100_0000;
pub const PLATFORM_ADDR: usize = 0x3200_0000;
