mod r#const;
mod r#fn;
mod r#impl;
mod r#struct;

pub use super::{
    js_sys::{Array, Function, JSON, Object, Promise, Reflect, Uint8Array},
    wasm_bindgen::{JsCast, JsValue},
    wasm_bindgen_futures::JsFuture,
    web_sys::{HtmlCanvasElement, Navigator},
};

pub use r#const::*;
pub use r#fn::{distance_to, flatten_shading};
pub use r#fn::*;
pub use r#struct::*;

pub(crate) use crate::{
    camera::Mat4,
    mesh::{f32_slice_to_bytes, u32_slice_to_bytes},
    r#const::{BAKED_CONTACT_AO_HEIGHT, CONTACT_SHADOW_FLOOR, REPLACE_MESH_OUT_OF_RANGE},
    r#type::Vec3,
    render::{FLOATS_PER_INSTANCE, INSTANCE_STRIDE_BYTES, STRIDE_FLOATS, SceneLighting},
};
