use std::cell::UnsafeCell;
macro_rules! data_pointers {
    ($
    (
        $(#[$inner:ident $($args:tt)*])*
        $name:ident: $typ:ty = $addr:expr;
    )*
    ) => {
        $(
        $(#[$inner$($args)*])*
        pub const $name: *mut $typ = $addr as *mut $typ;
        )*
    };
}

data_pointers!(
    /// Contains the delta value between the current game camera `z` and the ground.
    ///
    /// This is seemingly used to a constant elevation for the camera whilst moving around.
    Z_FIX_DELTA_GROUND_ADDR: f32 = 0x0193F364;
    /// When the given `u32 != 0` then the game is currently in a battle.
    BATTLE_ONGOING_ADDR: u32 = 0x193D683;
    /// Holds the config value for the current camera type (RTS/TotalWar/etc).
    BATTLE_CAM_CONF_TYPE_ADDR: BattleCameraType = 0x1639F14;
    /// The address for the semi-authoritative camera position when using TotalWar camera.
    ///
    /// Is different when using RTS.
    BATTLE_CAM_ADDR: BattleCameraView = 0x193D598;
    /// The address for the semi-authoritative camera target position when using TotalWar camera.
    ///
    /// Is different when using RTS.
    BATTLE_CAM_TARGET_ADDR: BattleCameraTargetView = 0x193D5DC;
);

/// 0x0193D598, seems to represent the true map coordinates when using TotalWar Camera
/// When using RTS/General it seems correlated to BattleCameraPosition in some way (and gets constantly overwritten by values)
/// It seems to act sort of like BattleCameraTargetView when in RTS Camera mode.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BattleCameraView {
    /// 0x0193D598
    pub x_coord: f32,
    /// 0x0193D5A0
    pub z_coord: f32,
    /// 0x0193D59C
    pub y_coord: f32,
}

/// 0x193D5DC
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BattleCameraTargetView {
    /// 0x0193D5DC
    pub x_coord: f32,
    /// 0x0193D5E0
    pub z_coord: f32,
    /// 0x0193D5E4
    pub y_coord: f32,
}

/// 0x0193f34c, seems to represent the true map coordinates when using RTS/General camera
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BattleCameraPosition {
    pub x_coord: f32,
    pad_0: u32,
    pub y_coord: f32,
    pad_1: [u32; 5],
    pub z_coord: f32,
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
#[allow(dead_code)]
pub enum BattleCameraType {
    TotalWar = 0,
    GeneralCamera = 1,
    Rts = 2,
}

/// Highly unsafe Cell type used for interfacing with game patches.
///
/// Patches would write to this memory, usually without synchronisation.
/// Breaks several Rust guarantees with regard to exclusive `mut` ownership. If something mis-compiles, this is likely to blame.
#[derive(Default, Debug)]
#[repr(transparent)]
pub struct GameCell<T: ?Sized>(UnsafeCell<T>);

#[allow(dead_code)]
impl<T> GameCell<T> {
    pub fn new(item: T) -> Self {
        Self(UnsafeCell::new(item))
    }

    pub unsafe fn as_ref(&self) -> &T {
        &*self.0.get()
    }

    pub unsafe fn as_mut(&self) -> &mut T {
        &mut *self.0.get()
    }

    pub const fn get_ptr(&self) -> *const T {
        self.0.get()
    }

    pub const fn get_mut_ptr(&self) -> *mut T {
        self.0.get()
    }
}

/// Check whether we're currently in a battle or not.
///
/// Hacky work-around for now.
/// Not compatible with remote process approach.
pub fn is_in_battle() -> bool {
    unsafe { *BATTLE_ONGOING_ADDR != 0 }
}
