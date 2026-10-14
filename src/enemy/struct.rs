use super::*;
/// 一个敌人这一帧要做什么的决策结果。
#[derive(Data, Clone, Copy, Debug)]
pub struct EnemyDecision {
    /// 迁移到的新状态。
    #[get(type(copy), pub(crate))]
    pub(crate) next: AiState,
    /// 期望移动方向(XZ,未归一化;零向量 = 原地不动)。
    #[get(type(copy), pub(crate))]
    pub(crate) move_to: Vec2,
    /// 期望朝向。
    #[get(type(copy), pub(crate))]
    pub(crate) face: Vec3,
    /// 本帧是否开火。
    #[get(type(copy), pub(crate))]
    pub(crate) shoot: bool,
}
