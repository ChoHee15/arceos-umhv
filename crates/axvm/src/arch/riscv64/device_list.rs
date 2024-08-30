use super::csrs::RiscvCsrTrait;
use super::regs::GprIndex;
use super::vcpu::{VCpu, VmCpuStatus};
use super::vmexit::PrivilegeLevel;
use super::{devices::plic::PlicState, traps, vcpu, vm_pages::fetch_guest_instruction, CSR};
use crate::{AxVMHal, GuestPhysAddr, GuestVirtAddr};
use axerrno::{AxError, AxResult};
use axvcpu::AxArchVCpuExitReason;
use riscv::register::hstatus;
use core::panic;
use riscv_decode::Instruction;

/// the devices that belongs to a vm
pub struct DeviceList<H: AxVMHal> {
    plic: PlicState,
    _marker: core::marker::PhantomData<H>,
}

impl<H: AxVMHal> DeviceList<H> {
    /// Create a new VM with `vcpus` vCPUs and `gpt` as the guest page table.
    pub fn new() -> Self {
        DeviceList {
            plic: PlicState::new(0xC00_0000),
            _marker: core::marker::PhantomData,
        }
    }

    pub fn vmexit_handler(&mut self, vcpu: &mut VCpu<H>, vm_exit_info: AxArchVCpuExitReason) {
        match vm_exit_info {
            AxArchVCpuExitReason::NestedPageFault { addr: fault_addr } => {
                info!("handling NestedPageFault!");
                let falut_pc = vcpu.regs().guest_regs.sepc;
                let inst = vcpu.regs().trap_csrs.htinst as u32;
                let priv_level = PrivilegeLevel::from_hstatus(vcpu.regs().guest_regs.hstatus);
                // error!("hstatus1: {:#x}", hstatus::read().bits());
                // error!("g_hstatus: {:#x}", vcpu.regs().guest_regs.hstatus);
                match priv_level {
                    PrivilegeLevel::Supervisor => {
                        match self.handle_page_fault(falut_pc, inst, fault_addr, vcpu) {
                            Ok(inst_len) => {
                                vcpu.advance_pc(inst_len);
                            }
                            Err(err) => {
                                panic!(
                                    "Page fault at {:#x} addr@{:#x} with error {:?}",
                                    falut_pc, fault_addr, err
                                )
                            }
                        }
                    }
                    PrivilegeLevel::User => {
                        panic!("User page fault")
                    }
                }
            }
            AxArchVCpuExitReason::ExternalInterrupt { .. } => self.handle_irq(),
            AxArchVCpuExitReason::RV_START { hartid, start_addr, opaque } => {
                debug!("HART start: {}, {:#x}, {:#x}", hartid, start_addr, opaque);
                vcpu.set_gpr(GprIndex::A0, hartid);
                vcpu.set_gpr(GprIndex::A1, opaque);
                vcpu.set_spec(start_addr);

                // vcpu.set_status(VmCpuStatus::Runnable);
            }
            _ => {}
        }
    }
}

// Privaie methods implementation
impl<H: AxVMHal> DeviceList<H> {
    fn handle_page_fault(
        &mut self,
        inst_addr: GuestVirtAddr,
        inst: u32,
        fault_addr: GuestPhysAddr,
        vcpu: &mut VCpu<H>,
    ) -> AxResult<usize> {
        //  plic
        if fault_addr >= self.plic.base() && fault_addr < self.plic.base() + 0x0400_0000 {
            self.handle_plic(inst_addr, inst, fault_addr, vcpu)
        } else {
            error!("inst_addr: {:#x}, fault_addr: {:#x}", inst_addr, fault_addr);
            Err(AxError::BadAddress)
        }
    }

    #[allow(clippy::needless_late_init)]
    fn handle_plic(
        &mut self,
        inst_addr: GuestVirtAddr,
        mut inst: u32,
        fault_addr: GuestPhysAddr,
        vcpu: &mut VCpu<H>,
    ) -> AxResult<usize> {
        // error!("hstatus2: {:#x}", hstatus::read().bits());
        // CSR.hstatus.write_value(0x200000180);
        
        error!("inst_addr: {:#x}, fault_addr: {:#x}", inst_addr, fault_addr);
        info!("plic1!");
        if inst == 0 {
            // If hinst does not provide information about trap,
            // we must read the instruction from guest's memory maunally.
            inst = fetch_guest_instruction(inst_addr)?;
        }
        info!("plic2!");
        let i1 = inst as u16;
        let len = riscv_decode::instruction_length(i1);
        let inst = match len {
            2 => i1 as u32,
            4 => inst,
            _ => unreachable!(),
        };
        info!("plic3!");
        // assert!(len == 4);
        let decode_inst = riscv_decode::decode(inst).map_err(|_| AxError::InvalidData)?;
        info!("plic4!");
        match decode_inst {
            Instruction::Sw(i) => {
                info!("plic5!");
                let val = vcpu.get_gpr(GprIndex::from_raw(i.rs2()).unwrap()) as u32;
                info!("write({:#x}, {:#x})!", fault_addr, val);
                self.plic.write_u32(fault_addr, val);
                info!("plic555!");
            }
            Instruction::Lw(i) => {
                info!("plic6!");
                let val = self.plic.read_u32(fault_addr);
                vcpu.set_gpr(GprIndex::from_raw(i.rd()).unwrap(), val as usize)
            }
            _ => return Err(AxError::BadAddress),
        }
        Ok(len)
    }

    fn handle_irq(&mut self) {
        let context_id = 1;
        let claim_and_complete_addr = self.plic.base() + 0x0020_0004 + 0x1000 * context_id;
        let irq = unsafe { core::ptr::read_volatile(claim_and_complete_addr as *const u32) };
        assert!(irq != 0);
        self.plic.claim_complete[context_id] = irq;

        error!("get IRQ: {}", irq);

        CSR.hvip
            .read_and_set_bits(traps::interrupt::VIRTUAL_SUPERVISOR_EXTERNAL);
    }
}
