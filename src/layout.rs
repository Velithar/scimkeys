//! Bit layout of the Scimitar PRO RGB's vendor HID report (report ID 0x03)
//! and the pure functions used to turn a raw report into a button mask.

/// Maps each side button (G1..G12) to a `(byte_index, bit_mask)` pair.
/// `byte_index` indexes into the full HID report, including the report ID
/// byte at index 0. Index `i` in this array corresponds to button G(i+1),
/// so index 0 is G1.
///
/// Confirmed empirically on 2026-08-21 (firmware 3.14) by pressing G1..G12
/// one at a time and watching the raw report: G1..G8 set bits 0x01..0x80 of
/// byte 2, G9..G12 set bits 0x01..0x08 of byte 3. If a different firmware or
/// model lays the bits out differently, run `scimkeys --dump`, press each
/// button, and update this table.
pub const G_BITS: [(usize, u8); 12] = [
    (2, 0x01), // G1
    (2, 0x02), // G2
    (2, 0x04), // G3
    (2, 0x08), // G4
    (2, 0x10), // G5
    (2, 0x20), // G6
    (2, 0x40), // G7
    (2, 0x80), // G8
    (3, 0x01), // G9
    (3, 0x02), // G10
    (3, 0x04), // G11
    (3, 0x08), // G12
];

/// Extract a 12-bit mask (bit 0 = G1 .. bit 11 = G12) from a full HID
/// report, using `G_BITS`. Does not allocate.
pub fn extract_mask(report: &[u8]) -> u16 {
    let mut mask = 0u16;
    for (i, (byte_index, bit_mask)) in G_BITS.iter().enumerate() {
        if let Some(byte) = report.get(*byte_index) {
            if byte & bit_mask != 0 {
                mask |= 1 << i;
            }
        }
    }
    mask
}

/// Diff two 12-bit button masks. Returns `(pressed, released)`, where
/// `pressed` has a bit set for every button that went from clear to set,
/// and `released` has a bit set for every button that went from set to
/// clear.
pub fn diff_mask(prev: u16, new: u16) -> (u16, u16) {
    let pressed = new & !prev;
    let released = prev & !new;
    (pressed, released)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_mask_detects_presses_and_releases() {
        let prev = 0b0000_0000_0001; // only G1 held
        let new = 0b0000_0000_0110; // G1 released, G2 and G3 pressed
        let (pressed, released) = diff_mask(prev, new);
        assert_eq!(pressed, 0b0000_0000_0110);
        assert_eq!(released, 0b0000_0000_0001);
    }

    #[test]
    fn diff_mask_no_change_is_empty() {
        let (pressed, released) = diff_mask(0b1010_1010, 0b1010_1010);
        assert_eq!(pressed, 0);
        assert_eq!(released, 0);
    }

    #[test]
    fn diff_mask_all_released() {
        let (pressed, released) = diff_mask(0b0000_1111, 0);
        assert_eq!(pressed, 0);
        assert_eq!(released, 0b0000_1111);
    }

    #[test]
    fn extract_mask_reads_g1_through_g8_from_byte_two() {
        let mut report = [0u8; 64];
        report[0] = 0x03;
        report[2] = 0b1010_0101; // G1, G3, G6, G8 pressed
        let mask = extract_mask(&report);
        assert_eq!(mask & 0x00FF, 0b1010_0101);
    }

    #[test]
    fn extract_mask_reads_g9_through_g12_from_byte_three() {
        let mut report = [0u8; 64];
        report[0] = 0x03;
        report[3] = 0b0000_1001; // G9 and G12 pressed
        let mask = extract_mask(&report);
        assert_eq!((mask >> 8) & 0x0F, 0b0000_1001);
    }

    #[test]
    fn extract_mask_ignores_short_reports_out_of_range() {
        // A report too short to contain byte index 3 should not panic, and
        // should simply treat missing bytes as zero.
        let report = [0x03u8, 0x00, 0b0000_0001];
        let mask = extract_mask(&report);
        assert_eq!(mask, 0b0000_0000_0001);
    }
}
