use super::*;

impl AtlasImage {
    /// The RGBA8 texels, row-major.
    ///
    /// # Returns
    ///
    /// - `&[u8]` - The texel bytes, `width * height * 4` long.
    pub fn get_pixels(&self) -> &[u8] {
        self.pixels.as_slice()
    }

    /// Total byte length a GPU upload expects.
    ///
    /// # Returns
    ///
    /// - `usize` - `width * height * 4`.
    pub fn byte_len(&self) -> usize {
        (self.get_width() * self.get_height() * 4) as usize
    }
}
