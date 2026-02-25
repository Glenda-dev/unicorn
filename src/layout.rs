use glenda::cap::{CapPtr, Endpoint, Frame, IrqHandler, Kernel};

pub const BOOTINFO_SLOT: CapPtr = CapPtr::from(9);
pub const IRQ_CONTROL_SLOT: CapPtr = CapPtr::from(10);
pub const KERNEL_SLOT: CapPtr = CapPtr::from(11);
pub const KERNEL_CAP: Kernel = Kernel::from(KERNEL_SLOT);
pub const IRQ_CONTROL_CAP: IrqHandler = IrqHandler::from(IRQ_CONTROL_SLOT);

pub const INIT_SLOT: CapPtr = CapPtr::from(13);
pub const MANIFEST_SLOT: CapPtr = CapPtr::from(14);
pub const RESOURCE_SLOT: CapPtr = CapPtr::from(15);
pub const INIT_CAP: Endpoint = Endpoint::from(INIT_SLOT);

pub const RESOURCE_CAP: Frame = Frame::from(RESOURCE_SLOT);
pub const MANIFEST_CAP: Frame = Frame::from(MANIFEST_SLOT);

pub const RESOURCE_ADDR: usize = 0x3000_0000;
pub const BOOTINFO_ADDR: usize = 0x3100_0000;
