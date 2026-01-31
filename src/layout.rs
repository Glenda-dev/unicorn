use glenda::cap::{CapPtr, Endpoint, Frame};

pub const FACTOTUM_SLOT: CapPtr = CapPtr::from(3);
pub const PLATFORM_SLOT: CapPtr = CapPtr::from(6);
pub const UNTYPED_SLOT: CapPtr = CapPtr::from(7);
pub const MMIO_SLOT: CapPtr = CapPtr::from(8);
pub const IRQ_SLOT: CapPtr = CapPtr::from(9);
pub const REPLY_SLOT: CapPtr = CapPtr::from(12);
pub const UNICORN_ENDPOINT_SLOT: CapPtr = CapPtr::from(11);

pub const FACTOTUM_CAP: Endpoint = Endpoint::from(FACTOTUM_SLOT);
pub const PLATFORM_CAP: Frame = Frame::from(PLATFORM_SLOT);
pub const UNICORN_ENDPOINT_CAP: Endpoint = Endpoint::from(UNICORN_ENDPOINT_SLOT);
pub const REPLY_CAP: Endpoint = Endpoint::from(REPLY_SLOT);
