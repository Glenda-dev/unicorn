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
        // ResourceClient::mmap will perform the mapping in current VSpace
        let vaddr = self.res_client.mmap(Badge::null(), frame, 0, PGSIZE)?;
        let shm = SharedMemory::from_frame(frame, vaddr, PGSIZE);
        let ring = IoRing::new(shm, 4, 4)?;
        let client_ring = IoRingClient::new(ring);
        client.set_ring(client_ring);

        let mut results = Vec::new();
        let mut sector = Vec::with_capacity(block_size as usize);
        unsafe {
            sector.set_len(block_size as usize);
        }

        // Sector 0
        if let Err(_) = client.read_blocks(0, 1, &mut sector) {
            // Clean up mapping before returning
            let _ = self.res_client.munmap(Badge::null(), vaddr, PGSIZE);
            return Ok(results);
        }

        // Try MBR
        if let Some(mbr) = MBR::parse(&sector) {
            // Check if it's protective GPT
            if mbr.is_protective_gpt() {
                // Try GPT at LBA 1
                if let Ok(_) = client.read_blocks(1, 1, &mut sector) {
                    if let Some(gpt_header) = GPTHeader::parse(&sector) {
                        // Read the partition table entries
                        let header_entries_size = (gpt_header.num_partition_entries
                            * gpt_header.partition_entry_size)
                            as usize;
                        let sectors_to_read = ((header_entries_size + (block_size as usize) - 1)
                            / (block_size as usize))
                            as u64;

                        let mut table_buf =
                            Vec::with_capacity((sectors_to_read * (block_size as u64)) as usize);
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
                            for (idx, p) in gpt_parts.iter().enumerate() {
                                results.push(LogicDeviceDesc {
                                    name: alloc::format!("{}.p{}", parent_name, idx),
                                    parent_name: String::from(parent_name),
                                    dev_type: LogicDeviceType::Block(PartitionMetadata {
                                        parent: ep.cap().bits() as u64,
                                        start_lba: p.first_lba,
                                        num_blocks: p.last_lba - p.first_lba + 1,
                                        block_size: block_size.into(),
                                    }),
                                });
                            }
                        }
                    }
                }
            } else {
                // Real MBR partitions
                for (idx, p) in mbr.partitions.iter().enumerate() {
                    if let Some(p) = p {
                        results.push(LogicDeviceDesc {
                            name: alloc::format!("{}.p{}", parent_name, idx),
                            parent_name: String::from(parent_name),
                            dev_type: LogicDeviceType::Block(PartitionMetadata {
                                parent: ep.cap().bits() as u64,
                                start_lba: p.start_lba as u64,
                                num_blocks: p.sectors_count as u64,
                                block_size: block_size.into(),
                            }),
                        });
                    }
                }
            }
        }

        // Clean up mapping
        self.res_client.munmap(Badge::null(), vaddr, PGSIZE)?;

        Ok(results)
    }
}
