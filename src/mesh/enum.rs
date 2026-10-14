use super::*;

/// 资产解析 / 校验错误。
#[derive(Debug)]
pub enum MeshError {
    /// JSON 语法或结构错误。
    Json(serde_json::Error),
    /// schema 校验失败(缺字段、长度不匹配、索引越界 ...)。
    Schema(String),
}
