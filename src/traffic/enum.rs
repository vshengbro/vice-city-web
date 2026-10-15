use super::*;

/// 路口信号机的当前灯色。
///
/// 两种色相就够了:`Red`(对车流为红)和 `Green`(对车流为绿)。南北 /
/// 东西是**两套独立的相位**,由 [`signals_for`] 一起返回,所以不需要
/// 「南北绿 / 东西红」这种组合枚举 —— 直接问「我这个轴现在什么颜色」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalAspect {
    /// 对本车道方向为红,必须停在停止线前。
    Red,
    /// 对本车道方向为绿,可以通行。
    Green,
}
