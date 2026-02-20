use crate::log;
use crate::unicorn::UnicornManager;
use crate::utils::gpt::{GPTHeader, GPTPartition};
use crate::utils::mbr::MBR;
use alloc::string::String;
use alloc::vec::Vec;
use glenda::arch::mem::PGSIZE;
use glenda::cap::Endpoint;
use glenda::error::Error;
use glenda::interface::MemoryService;
use glenda::ipc::Badge;
use glenda::mem::shm::SharedMemory;
use glenda::protocol::device::{LogicDeviceDesc, LogicDeviceType, PartitionMetadata};
use glenda_drivers::client::block::BlockClient;
use glenda_drivers::interface::BlockDriver;
use glenda_drivers::io_uring::IoRing;
use glenda_drivers::io_uring::IoRingClient;

impl<'a> UnicornManager<'a> {
    pub fn probe_partitions(
        &mut self,
        ep: Endpoint,
        parent_name: &str,
    ) -> Result<Vec<LogicDeviceDesc>, Error> {
        let mut client = BlockClient::new(ep);

        let block_size = client.block_size();
        let capacity = client.capacity();

        if capacity < 2 {
            return Ok(Vec::new());
        }

        // Setup IO ring for data path
        let frame = client.setup_ring(4, 4)?;

        // Map the ring to our own VSpace
        // We use the same RING_VA to simplify address passing for io_uring
        // although Unicorn doesn't strictly need it to be at identical address,
        // it helps if the driver isn't performing address translation yet.
        let vaddr = 0x7000_0000;
        let vaddr = self.res_client.mmap(Badge::null(), frame, vaddr, PGSIZE)?;
        let shm = SharedMemory::from_frame(frame, vaddr, PGSIZE);
        let ring = IoRing::new(shm, 4, 4)?;
        let mut client_ring = IoRingClient::new(ring);
        client_ring.set_server_notify(ep.clone());
        client.set_ring(client_ring);

        let mut results = Vec::new();

        // Use a sector buffer that is within the shared mapping range.
        // We use the space after the ring header/entries (approx offset 1024)
        let sector_ptr = (vaddr + 1024) as *mut u8;
        let sector = unsafe { core::slice::from_raw_parts_mut(sector_ptr, block_size as usize) };
        sector.fill(0);

        // Sector 0
        if let Ok(_) = client.read_blocks(0, 1, sector) {
            // 1. Detect Initrd Signature (0x99999999)
            let magic = u32::from_le_bytes([sector[0], sector[1], sector[2], sector[3]]);
            if magic == 0x99999999 {
                log!("Detected Initrd signature at {}", parent_name);
                results.push(LogicDeviceDesc {
                    parent_name: String::from(parent_name),
                    dev_type: LogicDeviceType::Block(PartitionMetadata {
                        parent: ep.cap().bits() as u64,
                        start_lba: 0,
                        num_blocks: capacity,
                        block_size: block_size.into(),
                    }),
                    badge: None,
                });
                // Skip further MBR/GPT probing for initrd
                self.res_client.munmap(Badge::null(), vaddr, PGSIZE)?;
                return Ok(results);
            }

            // 2. Try MBR
            if let Some(mbr) = MBR::parse(sector) {
                // Check if it's protective GPT
                if mbr.is_protective_gpt() {
                    // Try GPT at LBA 1
                    if let Ok(_) = client.read_blocks(1, 1, sector) {
                        if let Some(gpt_header) = GPTHeader::parse(sector) {
                            // Read the partition table entries
                            let header_entries_size = (gpt_header.num_partition_entries
                                * gpt_header.partition_entry_size)
                                as usize;
                            let sectors_to_read =
                                ((header_entries_size + (block_size as usize) - 1)
                                    / (block_size as usize)) as u64;

                            let mut table_buf = Vec::with_capacity(
                                (sectors_to_read * (block_size as u64)) as usize,
                            );
                            unsafe {
                                table_buf.set_len((sectors_to_read * (block_size as u64)) as usize);
                            }

                            if let Ok(_) = client.read_blocks(
                                gpt_header.partition_entry_lba,
                                sectors_to_read as u32,
                                &mut table_buf,
                            ) {
                                let gpt_parts = GPTPartition::parse_entries(
                                    &table_buf,
                                    gpt_header.num_partition_entries,
                                    gpt_header.partition_entry_size,
                                );
                                for (_idx, p) in gpt_parts.iter().enumerate() {
                                    results.push(LogicDeviceDesc {
                                        parent_name: String::from(parent_name),
                                        dev_type: LogicDeviceType::Block(PartitionMetadata {
                                            parent: ep.cap().bits() as u64,
                                            start_lba: p.first_lba,
                                            num_blocks: p.last_lba - p.first_lba + 1,
                                            block_size: block_size.into(),
                                        }),
                                        badge: None,
                                    });
                                }
                            }
                        }
                    }
                } else {
                    // Real MBR partitions
                    for (_idx, p) in mbr.partitions.iter().enumerate() {
                        if let Some(p) = p {
                            results.push(LogicDeviceDesc {
                                parent_name: String::from(parent_name),
                                dev_type: LogicDeviceType::Block(PartitionMetadata {
                                    parent: ep.cap().bits() as u64,
                                    start_lba: p.start_lba as u64,
                                    num_blocks: p.sectors_count as u64,
                                    block_size: block_size.into(),
                                }),
                                badge: None,
                            });
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            // If no partitions found, treat the whole device as one partition.
            // This is commonly needed for initrd or floppy-style images.
            results.push(LogicDeviceDesc {
                parent_name: String::from(parent_name),
                dev_type: LogicDeviceType::Block(PartitionMetadata {
                    parent: ep.cap().bits() as u64,
                    start_lba: 0,
                    num_blocks: capacity,
                    block_size: block_size.into(),
                }),
                badge: None,
            });
        }

        // Clean up mapping
        self.res_client.munmap(Badge::null(), vaddr, PGSIZE)?;

        Ok(results)
    }
}
