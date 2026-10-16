//! vice-city-web
//!
//! NEON BAY / Vice City Web —— GTA Vice City 风格开放世界浏览器游戏。
//!
//! 技术栈:`euv`(VDOM + 宏)+ `euv-engine`(数学 / 3D 数学)+ 手写
//! WebGL2 / Canvas2D 渲染后端 + `wasm-bindgen` → GitHub Pages。
//!
//! 模块划分:
//! - [`config`] —— 全项目字符串常量的单一来源(§1.3c)。
//! - [`mesh`] —— 资产 JSON schema 解析 → GPU 顶点布局。
//! - [`camera`] —— 轨道相机、投影矩阵、背面剔除。
//! - [`collision`] —— 二维圆形 vs AABB / 圆分离与世界边界。
//! - [`interior`] —— 三维楼板 / 隔墙 / 楼梯的垂直支撑与室内分离。
//! - [`player`] —— 玩家状态、步态骨架与第三人称移动。
//! - [`combat`] —— 武器、敌人 AI、通缉等级、任务与伤害结算。
//! - [`traffic`] —— 车队 AI、上下车、拾取物。
//! - [`render`] —— WebGL2 / Canvas2D 两个渲染后端 + 共享光照参数。
//! - [`game`] —— 场景蓝图、异步加载、固定步长循环、输入、昼夜循环。
//! - [`texture`] —— 运行时贴图图集:二进制图集的解析与按资产/表面族取图。

mod r#camera;
mod r#collision;
mod r#combat;
mod r#config;
mod r#enemy;
mod r#game;
mod r#interior;
mod r#mesh;
mod r#orient;
mod r#player;
mod r#render;
mod r#spawn;
mod r#texture;
mod r#traffic;
mod r#type;
mod r#webgpu;

pub use euv::{
    App, Data, VirtualNode, html, js_sys, js_sys::Function, js_sys::Promise, js_sys::Undefined,
    wasm_bindgen, wasm_bindgen::JsCast, wasm_bindgen::prelude::*, wasm_bindgen_futures,
    wasm_bindgen_futures::JsFuture, wasm_bindgen_futures::spawn_local, web_sys, web_sys::Document,
    web_sys::DomRect, web_sys::Element, web_sys::Event, web_sys::EventTarget,
    web_sys::HtmlCanvasElement, web_sys::HtmlInputElement, web_sys::KeyboardEvent,
    web_sys::MouseEvent, web_sys::Node, web_sys::Response, web_sys::TouchEvent, web_sys::TouchList,
    web_sys::WebGl2RenderingContext, web_sys::WebGlBuffer, web_sys::WebGlFramebuffer,
    web_sys::WebGlProgram, web_sys::WebGlShader, web_sys::WebGlTexture,
    web_sys::WebGlUniformLocation, web_sys::WebGlVertexArrayObject, web_sys::WheelEvent,
    web_sys::Window, web_sys::window,
};

pub use serde::Deserialize;
pub use serde_json::{self, Error};

// §1.3d dedup: these constants are defined once and re-exported, so no two
// modules may carry a second copy of the same value.
pub(crate) use crate::{
    collision::INSIDE_EPSILON, collision::RESOLVE_ITERATIONS, game::PED_POOL, game::STREET_PITCH,
};

pub use std::{
    cell::Cell, cell::Ref, cell::RefCell, cell::RefMut, cmp::Ordering, f32::consts::FRAC_PI_2,
    f32::consts::FRAC_PI_4, f32::consts::PI, f32::consts::TAU, fmt::Display, fs, iter::once,
    mem::swap, mem::take, rc::Rc,
};

pub(crate) use std::{collections::HashMap, iter, sync::Mutex, sync::MutexGuard, sync::OnceLock};

/// 挂载静态视图树并启动游戏循环。
///
/// 挂载一棵**静态** `html!` 树(canvas + 加载进度条 + HUD + 帮助),
/// 挂载完成后游戏循环和全部输入都由裸 `web_sys` + `Closure` 驱动,
/// VDOM 不参与每帧渲染 —— 这是 canvas 游戏的正确做法
/// (在事件回调里调用 euv hook 会因 hook context 丢失而静默白屏)。
#[wasm_bindgen]
pub fn main() {
    console_error_panic_hook::set_once();
    App::mount(r#config::APP_SELECTOR, r#game::app_root);
    r#game::boot();
}
