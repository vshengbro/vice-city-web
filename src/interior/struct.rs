use super::*;
/// 一栋可进入建筑的全部室内碰撞体。
#[derive(Data, Clone, Debug)]
pub struct FloorWorld {
    /// 全部楼板与隔墙。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) floors: Vec<Floor>,
}
