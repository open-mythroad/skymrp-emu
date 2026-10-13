/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::CpuState;
use crate::mem::{ConstPtr, Memory, MutPtr};
use arm32_cpu::{reg, Cpu as ArmCpu, Memory as ArmMemory, Mode};

pub struct InterpreterCpu {
    cpu: ArmCpu,
}

impl InterpreterCpu {
    pub fn new() -> Self {
        Self { cpu: ArmCpu::new() }
    }

    pub fn reg(&self, index: usize) -> u32 {
        self.cpu.reg_get(Mode::User, index.try_into().unwrap())
    }

    pub fn set_reg(&mut self, index: usize, value: u32) {
        self.cpu
            .reg_set(Mode::User, index.try_into().unwrap(), value);
    }

    pub fn cpsr(&self) -> u32 {
        self.cpu.reg_get(Mode::User, reg::CPSR)
    }

    pub fn set_cpsr(&mut self, cpsr: u32) {
        self.cpu.reg_set(Mode::User, reg::CPSR, cpsr);
    }

    fn svc(&mut self, mem: &Memory) -> Option<u32> {
        if self.cpu.mode() != Mode::Supervisor || self.reg(reg::PC.into()) != 0x08 {
            return None;
        }

        let lr = self.cpu.reg_get(Mode::Supervisor, reg::LR);
        let spsr = self.cpu.reg_get(Mode::Supervisor, reg::SPSR);
        let svc = if spsr & super::Cpu::CPSR_THUMB != 0 {
            let address = lr.checked_sub(2).expect("Invalid Thumb SVC return address");
            let instruction = mem.read::<u16, false>(ConstPtr::from_bits(address));
            assert_eq!(
                instruction & 0xff00,
                0xdf00,
                "Invalid Thumb SVC instruction at {address:#x}"
            );
            u32::from(instruction & 0xff)
        } else {
            let address = lr.checked_sub(4).expect("Invalid ARM SVC return address");
            let instruction = mem.read::<u32, false>(ConstPtr::from_bits(address));
            assert_eq!(
                instruction & 0x0f00_0000,
                0x0f00_0000,
                "Invalid ARM SVC instruction at {address:#x}"
            );
            instruction & 0x00ff_ffff
        };

        self.set_cpsr(spsr);
        self.set_reg(reg::PC.into(), lr);
        Some(svc)
    }

    #[must_use]
    pub fn run(&mut self, mem: &mut Memory, ticks: &mut u64) -> CpuState {
        loop {
            if let Some(svc) = self.svc(mem) {
                return CpuState::Svc(svc);
            }
            if *ticks == 0 {
                return CpuState::Normal;
            }

            let decoded = self.cpu.step(mem);
            *ticks -= 1;

            assert!(decoded, "Undefined instruction during CPU execution");
        }
    }
}

impl ArmMemory for Memory {
    fn r8(&mut self, addr: u32) -> u8 {
        Memory::read(self, ConstPtr::from_bits(addr))
    }

    fn r16(&mut self, addr: u32) -> u16 {
        Memory::read(self, ConstPtr::from_bits(addr))
    }

    fn r32(&mut self, addr: u32) -> u32 {
        Memory::read(self, ConstPtr::from_bits(addr))
    }

    fn w8(&mut self, addr: u32, val: u8) {
        Memory::write(self, MutPtr::from_bits(addr), val);
    }

    fn w16(&mut self, addr: u32, val: u16) {
        Memory::write(self, MutPtr::from_bits(addr), val);
    }

    fn w32(&mut self, addr: u32, val: u32) {
        Memory::write(self, MutPtr::from_bits(addr), val);
    }
}
