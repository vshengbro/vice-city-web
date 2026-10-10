mod r#const;
mod r#fn;
mod r#impl;
mod r#struct;

pub use ::core::array;

pub use super::{
    js_sys::{Array, Function, JSON, Object, Promise, Reflect, Uint8Array},
    wasm_bindgen::{JsCast, JsValue},
    wasm_bindgen_futures::JsFuture,
    web_sys::{HtmlCanvasElement, Navigator},
};

pub use r#const::*;
pub use r#fn::*;
pub use r#fn::{distance_to, flatten_shading};
pub use r#struct::*;


pub(crate) use crate::{
    camera::Mat4,
    r#const::{
        AO_BLUR_RADIUS, BAKED_CONTACT_AO_HEIGHT, BLOOM_BLUR_SPREAD, BLOOM_MIN_GAIN, BLOOM_SCALE,
        BLOOM_STRENGTH, BLOOM_THRESHOLD, CONTACT_SHADOW_FEATHER, CONTACT_SHADOW_FLOOR,
        CONTACT_SHADOW_MIN_HEIGHT, CONTACT_SHADOW_REACH, REPLACE_MESH_OUT_OF_RANGE,
        SHADOW_MAP_SIZE, SHADOW_PCF_RADIUS, SSAO_POWER, SSAO_RADIUS,
    },
    mesh::{f32_slice_to_bytes, u32_slice_to_bytes},
    render::{
        FLOATS_PER_INSTANCE, INSTANCE_STRIDE_BYTES, STRIDE_FLOATS, SceneLighting,
        instance_affects_shadow, shadow_view_projection,
    },
    r#type::Vec3,
};
