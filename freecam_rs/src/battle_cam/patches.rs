use crate::patcher::LocalPatcher;
use crate::ptr::GameCell;
use iced_x86::code_asm::{dword_ptr, eax, ebx, esi, esp, CodeAssembler};
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// General data structure containing all data that is written to by the game thread.
/// Caution is advised, as most of these values aren't written to with any lock/atomicity guarantees, so invalid data could
/// be present.
#[derive(Default, Clone)]
pub struct RemoteData {
    pub teleport_location: Arc<GameCell<BattleUnitCameraTeleport>>,
    /// Taking advantage of the fact that mov's are (?) atomic in x86.
    pub remote_z: Arc<AtomicU32>,
}

impl Debug for RemoteData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteData")
            .field("teleport_location", self.teleport_location.as_ref())
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
    pub x_target: f32,
    pub z_target: f32,
    pub y_target: f32,
}

impl BattleUnitCameraTeleport {
    /// Check whether there is a teleport 'command' ready.
    ///
    /// Will check that _all_ items are no longer equal to ``0.0`. This doesn't eliminate the potential race condition
    /// between the game code and our code, but it does make it less likely!
    pub fn is_available(&self) -> bool {
        self.x != 0.
            && self.y != 0.
            && self.z != 0.
            && self.x_target != 0.
            && self.y_target != 0.
            && self.z_target != 0.
    }
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
pub unsafe fn create_unit_card_teleport_patch(
    teleport_struct_addr: *mut BattleUnitCameraTeleport,
) -> anyhow::Result<(DynamicPatch, DynamicPatch)> {
    const PATCH_ADDR: usize = 0x8F8E8B;
    // The assembler executing the code we want
    let mut a = CodeAssembler::new(32)?;
    let teleport_struct_addr = teleport_struct_addr as usize;

    // X coord View
    a.mov(esi, dword_ptr(eax))?;
    a.mov(dword_ptr(teleport_struct_addr), esi)?;
    // Z coord View
    a.mov(esi, dword_ptr(eax + 4))?;
    a.mov(dword_ptr(teleport_struct_addr + 4), esi)?;
    // Y coord View
    a.mov(esi, dword_ptr(eax + 8))?;
    a.mov(dword_ptr(teleport_struct_addr + 8), esi)?;

    // Save the current `eax` register. Load the address for the Target coordinates
    a.push(eax)?;
    // Game uses `esp + 0x0C`, but we push 2 values onto the stack before this point, so we'll need an additional 0x8 offset.
    a.mov(eax, dword_ptr(esp + 0x14))?;
    // X coord Target
    a.mov(esi, dword_ptr(eax))?;
    a.mov(dword_ptr(teleport_struct_addr + 12), esi)?;
    // Z coord Target
    a.mov(esi, dword_ptr(eax + 4))?;
    a.mov(dword_ptr(teleport_struct_addr + 16), esi)?;
    // Y coord Target
    a.mov(esi, dword_ptr(eax + 8))?;
    a.mov(dword_ptr(teleport_struct_addr + 20), esi)?;
    // Restore `eax`
    a.pop(eax)?;

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

    let teleport_intercept = DynamicPatch {
        patch_addr: PATCH_ADDR,
        source_loc: Box::new(source_jump),
        dynamic_code,
    };
    // 11 NOPS for removing the writes to `target_view` addresses at 0x8F8EB7
    let target_view = DynamicPatch {
        patch_addr: 0x8F8EB7,
        source_loc: Box::new([0x90; 17]),
        dynamic_code: Box::new([]),
    };

    Ok((teleport_intercept, target_view))
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
