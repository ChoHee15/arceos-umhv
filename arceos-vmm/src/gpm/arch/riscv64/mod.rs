mod dtb_riscv64;

use self::dtb_riscv64::MachineMeta;
use axalloc::GlobalPage;
use axerrno::{AxError, AxResult};
use axhal::mem::virt_to_phys;
use axvm::{GuestPhysAddr, HostPhysAddr, HostVirtAddr};
use memory_addr::PhysAddr;
use page_table_entry::MappingFlags;

use crate::gpm::{GuestMemoryRegion, GuestPhysMemorySet};

// for nimbos & linux
// pub const GUEST_PHYS_MEMORY_BASE: GuestPhysAddr = 0x9000_0000;
// pub const DTB_ENTRY: GuestPhysAddr = 0x9000_0000;
// pub const GUEST_ENTRY: GuestPhysAddr = 0x9020_0000;
// pub const GUEST_PHYS_MEMORY_SIZE: usize = 0x400_0000; // TODO: 64M?
// // pub const GUEST_PHYS_MEMORY_SIZE: usize = 0x800_0000;

// for arceos
pub const GUEST_PHYS_MEMORY_BASE: GuestPhysAddr = 0x9000_0000;
pub const DTB_ENTRY: GuestPhysAddr = 0x9000_0000;
pub const GUEST_ENTRY: GuestPhysAddr = 0x9020_0000;
pub const GUEST_PHYS_MEMORY_SIZE: usize = 0x400_0000; // TODO: 64M?

#[repr(align(4096))]
struct AlignedMemory<const LEN: usize>([u8; LEN]);

static mut GUEST_PHYS_MEMORY: AlignedMemory<GUEST_PHYS_MEMORY_SIZE> =
    AlignedMemory([0; GUEST_PHYS_MEMORY_SIZE]);

// TODO:need use gpm to transfer
fn gpa_as_mut_ptr(guest_paddr: GuestPhysAddr) -> *mut u8 {
    let offset = unsafe { core::ptr::addr_of!(GUEST_PHYS_MEMORY) as *const _ as usize };
    debug!("GUEST_PHYS_MEMORY: {:#x}", offset);
    let host_vaddr = guest_paddr + offset - GUEST_PHYS_MEMORY_BASE;
    host_vaddr as *mut u8
}

fn load_guest_image_from_file_system(file_name: &str, load_gpa: GuestPhysAddr) -> AxResult {
    use std::io::{BufReader, Read};
    let file = std::fs::File::open(file_name).map_err(|err| {
        warn!(
            "Failed to open {}, err {:?}, please check your disk.img",
            file_name, err
        );
        AxError::NotFound
    })?;
    let buffer = unsafe {
        core::slice::from_raw_parts_mut(
            gpa_as_mut_ptr(load_gpa),
            file.metadata()
                .map_err(|err| {
                    warn!(
                        "Failed to get metadate of file {}, err {:?}",
                        file_name, err
                    );
                    AxError::Io
                })?
                .size() as usize,
        )
    };
    let mut file = BufReader::new(file);
    file.read_exact(buffer).map_err(|err| {
        warn!("Failed to read from file {}, err {:?}", file_name, err);
        AxError::Io
    })?;
    Ok(())
}

pub fn setup_gpm() -> AxResult<GuestPhysMemorySet> {
    // load_guest_image_from_file_system("nimbos.dtb", DTB_ENTRY)?;
    // load_guest_image_from_file_system("nimbos.bin", GUEST_ENTRY)?;
    // load_guest_image_from_file_system("linux.dtb", DTB_ENTRY)?;
    // load_guest_image_from_file_system("linux.bin", GUEST_ENTRY)?;
    load_guest_image_from_file_system("arceos_parallel.dtb", DTB_ENTRY)?;
    // load_guest_image_from_file_system("arceos_parallel.bin", GUEST_ENTRY)?;
    load_guest_image_from_file_system("arceos_parallel.bin", GUEST_ENTRY)?;

    let mut gpm = GuestPhysMemorySet::new()?;
    let tmp = gpa_as_mut_ptr(DTB_ENTRY) as usize;
    // let tmp = DTB_ENTRY as usize;
    let meta = MachineMeta::parse(tmp);
    if let Some(test) = meta.test_finisher_address {
        info!("test: {:#x?}", test);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: test.base_address,
                hpa: test.base_address.into(),
                size: test.size + 0x1000, // ? why + 0x1000 : it's for goldfish_rtc int 0x10_1000
                // size: test.size,
                flags: MappingFlags::READ
                    | MappingFlags::WRITE
                    | MappingFlags::USER
                    | MappingFlags::EXECUTE,
            }
            .into(),
        )?;
    }
    for virtio in meta.virtio.iter() {
        info!("virtio: {:#x?}", virtio);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: virtio.base_address,
                hpa: virtio.base_address.into(),
                size: virtio.size,
                flags: MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
            }
            .into(),
        )?;
    }

    if let Some(uart) = meta.uart {
        info!("uart: {:#x?}", uart);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: uart.base_address,
                hpa: uart.base_address.into(),
                size: 0x1000, // ? why 0x1000
                // size: uart.size,
                flags: MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
            }
            .into(),
        )?;
    }

    if let Some(clint) = meta.clint {
        info!("clint: {:#x?}", clint);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: clint.base_address,
                hpa: clint.base_address.into(),
                size: clint.size,
                flags: MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
            }
            .into(),
        )?;
    }

    if let Some(plic) = meta.plic {
        info!("plic: {:#x?}", plic);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: plic.base_address,
                hpa: plic.base_address.into(),
                size: 0x20_0000, // ? why 0x20_0000
                // size: plic.size,
                flags: MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
            }
            .into(),
        )?;
    }

    if let Some(pci) = meta.pci {
        info!("pci: {:#x?}", pci);
        gpm.map_region(
            GuestMemoryRegion {
                gpa: pci.base_address,
                hpa: pci.base_address.into(),
                size: pci.size,
                flags: MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
            }
            .into(),
        )?;
    }

    info!(
        "physical memory: [{:#x}: {:#x})",
        meta.physical_memory_offset,
        meta.physical_memory_offset + meta.physical_memory_size
    );

    let gmr = GuestMemoryRegion {
        gpa: meta.physical_memory_offset,
        hpa: virt_to_phys(HostVirtAddr::from(
            gpa_as_mut_ptr(GUEST_PHYS_MEMORY_BASE) as usize
        )),
        size: meta.physical_memory_size,
        flags: MappingFlags::READ
            | MappingFlags::WRITE
            | MappingFlags::EXECUTE
            | MappingFlags::USER,
    };

    // let tt = GuestMemoryRegion {
    //     gpa: meta.physical_memory_offset,
    //     hpa: PhysAddr::from(meta.physical_memory_offset),
    //     size: meta.physical_memory_size,
    //     flags: MappingFlags::READ
    //         | MappingFlags::WRITE
    //         | MappingFlags::EXECUTE
    //         | MappingFlags::USER,
    // };

    error!("MAP: {:#x?}", gmr);


    gpm.map_region(
        gmr.into(),
    )?;

    Ok(gpm)
}
