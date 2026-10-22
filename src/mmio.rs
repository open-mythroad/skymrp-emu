/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
type VAddr = u32;
type GuestUSize = u32;

const LEGACY_KEYPAD_BASE: VAddr = 0x8011_0000;
const LEGACY_KEYPAD_SIZE: usize = 16;
const LEGACY_KEYPAD_KEY_COUNT: u32 = 42;

pub struct Mmio {
    legacy_keypad: LegacyKeypad,
}

impl Mmio {
    pub fn new() -> Self {
        Self {
            legacy_keypad: LegacyKeypad::new(),
        }
    }

    pub(crate) fn bytes_at(&self, address: VAddr, size: GuestUSize) -> Option<&[u8]> {
        self.legacy_keypad.register_range(address, size)
    }

    pub(crate) fn bytes_at_mut(&mut self, address: VAddr, size: GuestUSize) -> Option<&mut [u8]> {
        self.legacy_keypad.register_range_mut(address, size)
    }

    pub fn set_keypad_key_state(&mut self, code: u32, pressed: bool) {
        self.legacy_keypad.set_key_state(code, pressed);
    }
}

pub struct LegacyKeypad {
    registers: [u8; LEGACY_KEYPAD_SIZE],
}

impl LegacyKeypad {
    fn new() -> Self {
        Self {
            registers: [u8::MAX; LEGACY_KEYPAD_SIZE],
        }
    }

    fn register_offset(address: VAddr, size: GuestUSize) -> Option<usize> {
        let offset = address.checked_sub(LEGACY_KEYPAD_BASE)? as usize;
        let end = offset.checked_add(size as usize)?;
        (end <= LEGACY_KEYPAD_SIZE).then_some(offset)
    }

    fn register_range(&self, address: VAddr, size: GuestUSize) -> Option<&[u8]> {
        let offset = Self::register_offset(address, size)?;
        Some(&self.registers[offset..offset + size as usize])
    }

    fn register_range_mut(&mut self, address: VAddr, size: GuestUSize) -> Option<&mut [u8]> {
        let offset = Self::register_offset(address, size)?;
        Some(&mut self.registers[offset..offset + size as usize])
    }

    fn set_key_state(&mut self, code: u32, pressed: bool) {
        if code >= LEGACY_KEYPAD_KEY_COUNT {
            return;
        }

        let offset = 4 + code as usize / 16 * 4;
        let mask = 1_u16 << (code % 16);
        let mut value = u16::from_le_bytes(self.registers[offset..offset + 2].try_into().unwrap());
        if pressed {
            value &= !mask;
        } else {
            value |= mask;
        }
        self.registers[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_keypad_tracks_active_low_key_bits() {
        let mut mmio = Mmio::new();

        for (code, offset, mask) in [(0, 4, 1_u16), (17, 8, 2_u16), (41, 12, 512_u16)] {
            let register = LEGACY_KEYPAD_BASE + offset;
            let read_register = |mmio: &Mmio| {
                u16::from_le_bytes(mmio.bytes_at(register, 2).unwrap().try_into().unwrap())
            };
            assert_eq!(read_register(&mmio), u16::MAX);

            mmio.set_keypad_key_state(code, true);
            assert_eq!(read_register(&mmio), !mask);

            mmio.set_keypad_key_state(code, false);
            assert_eq!(read_register(&mmio), u16::MAX);
        }
    }

    #[test]
    fn legacy_keypad_ignores_out_of_range_keys() {
        let mut mmio = Mmio::new();
        mmio.set_keypad_key_state(LEGACY_KEYPAD_KEY_COUNT, true);

        assert_eq!(
            mmio.bytes_at(LEGACY_KEYPAD_BASE + 12, 2),
            Some(u16::MAX.to_le_bytes().as_slice())
        );
    }
}
