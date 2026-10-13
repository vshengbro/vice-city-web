//! 贴图图集常量:三张贴图的名字与图集边长。

/// The albedo map name — the one every textured surface is guaranteed to have.
///
/// `pub(crate)`: every reader lives inside this crate (§18 wants an external
/// reader before granting crate-wide `pub`).
pub(crate) const MAP_ALBEDO: &str = "albedo";

/// Edge length every atlas map was authored at.
///
/// `pub(crate)` for the same reason as [`MAP_ALBEDO`].
pub(crate) const ATLAS_TEX_SIZE: u32 = 128;

/// Normal (tangent-space) map name.
///
/// Only the tests read this today, because the loader takes the map name as an
/// argument so the renderer can ask for whichever of the three it needs. Kept
/// `pub(crate)` rather than `pub` while nothing outside the crate names it
/// directly (§18).
pub(crate) const MAP_NORMAL: &str = "normal";

/// Occlusion / roughness / metalness map name.
///
/// `pub(crate)` for the same reason as [`MAP_NORMAL`].
pub(crate) const MAP_ORM: &str = "orm";
