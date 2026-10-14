use super::*;
/// One decoded atlas: an RGBA8 image plus the width/height it was built at.
#[derive(Data, Clone, Debug, PartialEq, Eq)]
pub struct AtlasImage {
    /// RGBA8 texels, row-major, `width * height * 4` bytes.
    #[get(skip, pub(crate))]
    pub(crate) pixels: Vec<u8>,
    /// Width in texels.
    #[get(type(copy), pub(crate))]
    pub(crate) width: u32,
    /// Height in texels.
    #[get(type(copy), pub(crate))]
    pub(crate) height: u32,
}
/// Every atlas in one asset file, keyed `"<surface kind>/<map name>"`.
#[derive(Data, Clone, Debug, Default, PartialEq, Eq)]
pub struct AtlasSet {
    /// Decoded images by key.
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) images: HashMap<String, AtlasImage>,
}
/// The JSON fields an asset part gains when it is textured.
#[derive(Data, Clone, Debug, Deserialize)]
pub struct TextureRef {
    /// Path of the atlas `.bin`, relative to `assets/`.
    #[serde(rename = "texture_atlas")]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) atlas: String,
    /// Surface family, e.g. `paint` or `metal`.
    #[serde(rename = "surface_kind")]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) surface_kind: String,
}
