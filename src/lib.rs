//! vice-city-web
//!
//! NEON BAY / Vice City Web —— GTA Vice City 风格开放世界浏览器游戏。
//!
//! 技术栈:`euv`(VDOM + 宏)+ `euv-engine`(数学 / 3D 数学)+ 手写
//! WebGL2 / Canvas2D 渲染后端 + `wasm-bindgen` → GitHub Pages。
//!
//! 模块划分:
//! - [`const`] —— 全项目字符串常量的单一来源(§1.3c)。
//! - [`mesh`] —— 资产 JSON schema 解析 → GPU 顶点布局。
//! - [`camera`] —— 轨道相机、投影矩阵、背面剔除。
//! - [`collision`] —— 二维圆形 vs AABB / 圆分离与世界边界。
//! - [`interior`] —— 三维楼板 / 隔墙 / 楼梯的垂直支撑与室内分离。
//! - [`player`] —— 玩家状态、步态骨架与第三人称移动。
//! - [`combat`] —— 武器、敌人 AI、通缉等级、任务与伤害结算。
//! - [`traffic`] —— 车队 AI、上下车、拾取物。
//! - [`render`] —— WebGL2 / Canvas2D 两个渲染后端 + 共享光照参数。
//! - [`game`] —— 场景蓝图、异步加载、固定步长循环、输入、昼夜循环。

mod r#camera;
mod r#collision;
mod r#combat;
mod r#const;
mod r#enemy;
mod r#game;
mod r#interior;
mod r#mesh;
mod r#player;
mod r#render;
mod r#spawn;
mod r#traffic;
mod r#type;
mod r#webgpu;

pub use euv::{App, wasm_bindgen::prelude::*};

/// §6.1:子模块只能 `use super::*;`,所以 `euv` 的这几个模块在 crate 根
/// **重新导出**一次 —— `src/webgpu/mod.rs` 就能通过 `pub use super::…`
/// 拿到它们,不必自己写 `use euv::…`。
pub use euv::{js_sys, wasm_bindgen, wasm_bindgen_futures, web_sys};

/// §6.1 同理:`std` 的常用常量在 crate 根**重新导出**一次,子模块就能
/// `use super::*` 直接拿到 `FRAC_PI_2` / `PI`,不必在每个文件里写全路径。
pub use std::f32::consts::{FRAC_PI_2, PI};

/// 挂载静态视图树并启动游戏循环。
///
/// 挂载一棵**静态** `html!` 树(canvas + 加载进度条 + HUD + 帮助),
/// 挂载完成后游戏循环和全部输入都由裸 `web_sys` + `Closure` 驱动,
/// VDOM 不参与每帧渲染 —— 这是 canvas 游戏的正确做法
/// (在事件回调里调用 euv hook 会因 hook context 丢失而静默白屏)。
#[wasm_bindgen]
pub fn main() {
    console_error_panic_hook::set_once();
    App::mount(r#const::APP_SELECTOR, r#game::app_root);
    r#game::boot();
}
