use crate::ptr::NonNullPtr;
use crate::patcher::LocalPatcher;

pub const PATCH_LOCATIONS_STEAM: [usize; 83] = [
    // Camera X
    0x008F8E10, 0x008F8B50, 0x00E7EF6A, 0x0094FCDC, 0x008FAC69, 0x008F8C6C, 0x008F9439, 0x0095B40E, 0x0095B7F4,
    0x008F8E8B, 0x008F6F29, 0x0095B3B0, 0x0094E996, 0x008F9050, // Camera Y
    0x008F8E1C, 0x008F8B5C, 0x00E7EF7F, 0x0094FCE5, 0x008FAC72, 0x008F8C76, 0x008F9443, 0x0095B429, 0x0095B805,
    0x008F8E97, 0x008F6F39, 0x0095B3BB, 0x0094E9DF, 0x008F905A, // Camera Z
    0x008F8E16, 0x008F8B56, 0x00E7EF74, 0x0094FCE0, 0x0094FD2D, 0x008FAC6D, 0x008F8C71, 0x008F943E, 0x0095B41B,
    0x0095B499, 0x0095B7FC, 0x008F8E91, 0x008F6F2F, 0x0095B3B5, 0x008F9011, // Target X
    0x008F8B78, 0x008F8E38, 0x008F8EB9, 0x00E7EF91, 0x008F6F5F, 0x0095B5CB, 0x0094FB90, 0x0095B828, 0x008F8CB6,
    0x008F9480, 0x008F7056, 0x008FAC5B, // Target Y
    0x008F8B84, 0x008F8E44, 0x008F8EC5, 0x00E7EFA6, 0x008F6F6B, 0x0095B5D4, 0x0094FB9B, 0x0095B831, 0x008F8CC0,
    0x008F948A, 0x008F7060, 0x008FAC63, // Target Z
    0x008F8B7E, 0x008F8E3E, 0x008F8EBF, 0x00E7EF9B, 0x008F6F65, 0x0095B5CF, 0x0094FB95, 0x0094FBCE, 0x0094FDCD,
    0x0095B82C, 0x008F8CBB, 0x008F9485, 0x008F705B, 0x008FAC4E, 0x0094E9BC, 0x008F9055,
];

pub unsafe fn patch_logic(address: &mut NonNullPtr<u8>, patcher: &mut LocalPatcher) {
    let length = if (*patcher.read(address.as_ref())) == 0xF3 { 5 } else { 3 };
    //The 243 or F3 byte means that the operatation in total is 5 bytes long.
    //Otherwise the operation is 3 bytes long. This works for this program as these are the only possibilities
    let to_patch = vec![144; length];

    // Don't immediately activate the patches, causes crashes.
    patcher.patch(address.as_mut(), &to_patch, false);
}
