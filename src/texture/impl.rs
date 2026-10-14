use super::*;

impl AtlasImage {
    /// Total byte length a GPU upload expects.
    ///
    /// # Returns
    ///
    /// - `usize` - `width * height * 4`.
    pub fn byte_len(&self) -> usize {
        (self.get_width() * self.get_height() * 4) as usize
    }
}
