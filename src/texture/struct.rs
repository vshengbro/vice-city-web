use super::*;

/// One decoded atlas: an RGBA8 image plus the width/height it was built at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasImage {
    /// RGBA8 texels, row-major, `width * height * 4` bytes.
    pub(crate) pixels: Vec<u8>,
    /// Width in texels.
    pub(crate) width: u32,
    /// Height in texels.
    pub(crate) height: u32,
}
/// Every atlas in one asset file, keyed `"<surface kind>/<map name>"`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AtlasSet {
    /// Decoded images by key.
    pub images: HashMap<String, AtlasImage>,
}
/// The JSON fields an asset part gains when it is textured.
#[derive(Clone, Debug, Deserialize)]
pub struct TextureRef {
    /// Path of the atlas `.bin`, relative to `assets/`.
    #[serde(rename = "texture_atlas")]
    pub atlas: String,
    /// Surface family, e.g. `paint` or `metal`.
    #[serde(rename = "surface_kind")]
    pub surface_kind: String,
}
