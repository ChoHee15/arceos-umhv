#![cfg_attr(feature = "axstd", no_std)]
#![cfg_attr(feature = "axstd", no_main)]
#![feature(naked_functions)]

#[macro_use]
#[cfg(feature = "axstd")]
extern crate axstd as std;

extern crate alloc;

#[macro_use]
extern crate log;

// mod device_emu;
mod gpm;
mod hal;
// mod vmexit; temporarily removed

use axerrno::{AxError, AxResult};
use axhal::cpu;
use axvm::config::{AxArchVCpuConfig, AxVCpuConfig, AxVMConfig};
use axvm::{AxVM, AxVMPerCpu, GuestPhysAddr, HostPhysAddr, HostVirtAddr};
use page_table_entry::MappingFlags;

use self::gpm::{setup_gpm, GuestMemoryRegion, GuestPhysMemorySet, GUEST_ENTRY};
use self::hal::AxVMHalImpl;

#[percpu::def_percpu]
pub static mut AXVM_PER_CPU: AxVMPerCpu<AxVMHalImpl> = AxVMPerCpu::new_uninit();

use lazy_init::LazyInit;
use alloc::sync::Arc;
static mut HV_VM: LazyInit<Arc<AxVM<AxVMHalImpl>>> = LazyInit::new();

use core::{sync::atomic::{AtomicUsize, Ordering}};

static SYNC_VCPUS: AtomicUsize = AtomicUsize::new(0);

static SMP: usize = axconfig::SMP;

#[cfg_attr(feature = "axstd", no_mangle)]
fn main(cpu_id: usize) {
    println!("Starting virtualization...");
    info!("Hardware support: {:?}", axvm::has_hardware_support());

    let percpu = unsafe { AXVM_PER_CPU.current_ref_mut_raw() };
    percpu.init(0).expect("Failed to initialize percpu state");
    percpu
        .hardware_enable()
        .expect("Failed to enable virtualization");

    let gpm = setup_gpm().expect("Failed to set guest physical memory set");
    debug!("{:#x?}", gpm);

    let config = AxVMConfig {
        // cpu_count: 1,
        cpu_count: 2,
        cpu_config: AxVCpuConfig {
            arch_config: AxArchVCpuConfig {
                setup_config: (),
                create_config: (),
            },
            ap_entry: GUEST_ENTRY,
            bsp_entry: GUEST_ENTRY,
        },
        // gpm: gpm.nest_page_table_root(),
        // gpm : 0.into(),
    };

    // let vm = AxVM::<AxVMHalImpl>::new(config, 0, gpm.nest_page_table_root())
    //     .expect("Failed to create VM");
    unsafe {
        HV_VM.init_by(AxVM::<AxVMHalImpl>::new(config, 0, gpm.nest_page_table_root()).expect("Failed to create VM"));
    }
    let vm = unsafe { HV_VM.get_mut_unchecked() };

    info!("VCPU{} main hart ok and wait for sync", cpu_id);

    SYNC_VCPUS.fetch_add(1, Ordering::Relaxed);
    while !(SYNC_VCPUS.load(Ordering::Acquire) == SMP) {
        core::hint::spin_loop();
    }

    info!("VCPU{} main hart sync done!!!", cpu_id);

    info!("Boot VM...");
    vm.boot().unwrap();
    panic!("VM boot failed")
}

#[no_mangle]
fn secondary_main(cpu_id: usize){
    assert!(cpu_id == 1);

    debug!("HART{} entering hv_secondary_main", cpu_id);

    while let None = unsafe { HV_VM.try_get() } {
        core::hint::spin_loop();
    }

    let percpu = unsafe { AXVM_PER_CPU.current_ref_mut_raw() };
    percpu.init(cpu_id).expect("Failed to initialize percpu state");
    percpu
        .hardware_enable()
        .expect("Failed to enable virtualization");

    info!("VCPU{} init ok and wait for sync", cpu_id);
    SYNC_VCPUS.fetch_add(1, Ordering::Relaxed);

    let vm = unsafe { HV_VM.get_mut_unchecked() };

    vm.run_vcpu(cpu_id);

    panic!("should not reach here");
    
    use axhal::arch::wait_for_irqs;
    loop{
        wait_for_irqs();
    }
}
