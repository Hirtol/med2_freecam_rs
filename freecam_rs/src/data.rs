// 0x0193D598, seems to represent the true map coordinates when using TotalWar Camera
// When using RTS/General it seems correlated to BattleCameraPosition in some way (and gets constantly overwritten by values)
// It seems to act sort of like BattleCameraTargetView when in TotalWar Camera mode.
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

// 0x193D5DC
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

// 0x0193f34c, seems to represent the true map coordinates when using RTS/General camera
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
pub enum BattleCameraType {
    TotalWar = 0,
    GeneralCamera = 1,
    Rts = 2,
}
