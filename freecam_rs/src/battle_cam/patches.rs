use crate::patcher::LocalPatcher;
use iced_x86::code_asm::{dword_ptr, eax, ebx, esi, CodeAssembler};
use std::cell::UnsafeCell;
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// General data structure containing all data that is written to by the game thread.
/// Caution is advised, as most of these values aren't written to with any lock/atomicity guarantees, so invalid data could
/// be present.
#[derive(Default, Clone)]
pub struct RemoteData {
    pub teleport_location: Arc<UnsafeCell<BattleUnitCameraTeleport>>,
    /// Taking advantage of the fact that mov's are (?) atomic in x86.
    pub remote_z: Arc<AtomicU32>,
}

impl Debug for RemoteData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteData")
            .field("teleport_location", &unsafe { *self.teleport_location.get() })
            .field("remote_z", &f32::from_bits(self.remote_z.load(Ordering::SeqCst)))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct BattleUnitCameraTeleport {
    pub x: f32,
    pub z: f32,
    pub y: f32,
}

pub struct DynamicPatch {
    pub patch_addr: usize,
    /// The code to insert into the source code at `patch_addr`.
    pub source_loc: Box<[u8]>,
    /// Dynamically created code which `source_loc` can jump to.
    ///
    /// The dynamic code should jump back towards `patch_addr + OFFSET`.
    pub dynamic_code: Box<[u8]>,
}

impl DynamicPatch {
    /// Apply this patch to the given patcher.
    ///
    /// Starts out disabled.
    pub unsafe fn apply_to_patcher(&self, patcher: &mut LocalPatcher) {
        patcher.patch(self.patch_addr as *mut u8, &self.source_loc, false);
    }
}

/// Create a patch for redirecting the writes to the camera's position when a user completes a unit card teleport click.
pub unsafe fn create_unit_card_teleport_patch(teleport_struct_addr: usize) -> anyhow::Result<DynamicPatch> {
    const PATCH_ADDR: usize = 0x8F8E8B;
    // The assembler executing the code we want
    let mut a = CodeAssembler::new(32)?;

    // X coord
    a.mov(esi, dword_ptr(eax))?;
    a.mov(dword_ptr(teleport_struct_addr), esi)?;
    // Z coord
    a.mov(esi, dword_ptr(eax + 4))?;
    a.mov(dword_ptr(teleport_struct_addr + 4), esi)?;
    // Y coord
    a.mov(esi, dword_ptr(eax + 8))?;
    a.mov(dword_ptr(teleport_struct_addr + 8), esi)?;
    // Jump back to our patch location, but now towards the `pop ebx`
    a.mov(ebx, (PATCH_ADDR + 8) as u32)?;
    a.jmp(ebx)?;

    let dynamic_code = a.assemble(0x0)?.into_boxed_slice();

    // Call location assembler to jump to our trampoline.
    // 0:  53                      push   ebx
    // 1:  bb 80 80 80 80          mov    ebx,ADDR
    // 6:  ff e3                   jmp    ebx
    // 8:  5b                      pop    ebx
    // Followed by enough NOPS to overwrite other moves (15 bytes that we need to patch from 0x8F8E8B..0x8F8E9A (NOT INCLUSIVE!))
    let addr = (dynamic_code.as_ptr() as u32).to_le_bytes();
    let source_jump = [
        0x53, 0xBB, addr[0], addr[1], addr[2], addr[3], 0xFF, 0xE3, 0x5B, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ];

    Ok(DynamicPatch {
        patch_addr: PATCH_ADDR,
        source_loc: Box::new(source_jump),
        dynamic_code,
    })
}

pub fn apply_general_z_remote_patch(patcher: &mut LocalPatcher, remote_data: &RemoteData) {
    // One of the `movss` which moved values to the battlecam address _anyway_
    // We have 15 bytes of `nops` atm at that address.
    let address_to_patch = 0x008F8C6C;
    let address_to_patch_2 = 0x008F9439;
    let address = (remote_data.remote_z.as_ptr() as u32).to_le_bytes();

    // 0:  52                      push   edx
    // 1:  ba 11 23 67 80          mov    edx,ADDRESS
    // 6:  f3 0f 11 0a             movss  DWORD PTR [edx],xmm1
    // a:  5a                      pop    edx
    let mut assembly_patch = [
        0x52, 0xBA, address[0], address[1], address[2], address[3], 0xF3, 0x0F, 0x11, 0x0A, 0x5A,
    ];

    unsafe { patcher.patch(address_to_patch as *mut u8, &assembly_patch, false) }
    // 6:  f3 0f 11 02             movss  DWORD PTR [edx],xmm0
    assembly_patch[9] = 0x02;
    unsafe { patcher.patch(address_to_patch_2 as *mut u8, &assembly_patch, false) }
    // TODO: Do the same above thing, but for _all_ pointers instead of `NOPing`  them
    // At the very least do the numbered ones here, as they're the ones which `teleport` us when double clicking a unit!
    // ALSO TODO: Remove the below from the standard patch list!
    // 52     "0x8F8E8B",
    //     "0x95B7F4",
    //
    //
    // 66     "0x8F8E97",
    //     "0x95B805",
    //
    //
    // 82     "0x8F8E91",
    //     "0x95B7FC",
}
