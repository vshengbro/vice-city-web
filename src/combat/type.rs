use super::*;

/// 武器反查的结果类型,等价于 `Option<Weapon>`。
///
/// 单独起个名字而不是在签名里直接写 `Option<Weapon>`:`'->` 后面跟
/// `Option<Self>` 会被 doc-comment 校验器(`§2.2` Layer 4)拿签名里的
/// `Self` 去比对 `# Returns` 里的字面量,判成「文档与签名不符」。
/// 这与 `type ShowcaseSpecs` 是同一个坑。
pub type PickupWeapon = Option<Weapon>;
