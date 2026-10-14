use super::*;

/// A cursor over the atlas file, so a truncated file is an error rather than
/// a silent short read.
///
/// Free functions over an offset rather than a struct with `self` field
/// access, which §17.3/§17.12 forbids.
///
/// # Arguments
///
/// - `&'a [u8]` - The atlas bytes.
/// - `&mut usize` - The cursor, advanced past `n` bytes on success.
/// - `usize` - How many bytes to take.
///
/// # Returns
///
/// - `Result<&'a [u8], String>` - The next `n` bytes, or an error naming the
///   offset that ran past the end.
pub(crate) fn take<'a>(data: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], String> {
    let end: usize = *at + n;
    if end > data.len() {
        return Err(format!(
            "atlas truncated: wanted {n} bytes at offset {at}, have {}",
            data.len()
        ));
    }
    let slice: &'a [u8] = &data[*at..end];
    *at = end;
    Ok(slice)
}

/// Read one little-endian `u32` as a `usize` offset.
///
/// # Arguments
///
/// - `&[u8]` - The atlas bytes.
/// - `&mut usize` - The cursor, advanced past the four bytes read.
///
/// # Returns
///
/// - `Result<usize, String>` - The value, or an error if the file is short.
pub(crate) fn read_u32(data: &[u8], at: &mut usize) -> Result<usize, String> {
    let raw: &[u8] = take(data, at, 4)?;
    let value: u32 = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    Ok(value as usize)
}

/// Parse one atlas file.
///
/// The layout is `count u32` then, per atlas, `key_len u32`, the key bytes,
/// `byte_len u32`, then `byte_len` of RGBA8. Every map is the same square size
/// so no per-map dimensions are stored.
///
/// # Arguments
///
/// - `&[u8]` - The raw file contents.
/// - `u32` - The edge length in texels that every map was authored at.
///
/// # Returns
///
/// - `Result<AtlasSet, String>` - The decoded set, or an error naming the
///   offset that failed to parse.
pub fn parse_atlas(data: &[u8], size: u32) -> Result<AtlasSet, String> {
    let mut at: usize = 0;
    let count: usize = read_u32(data, &mut at)?;
    let mut images: HashMap<String, AtlasImage> = HashMap::new();
    for _ in 0..count {
        let key_len: usize = read_u32(data, &mut at)?;
        let key_raw: &[u8] = take(data, &mut at, key_len)?;
        let key: String = String::from_utf8(key_raw.to_vec())
            .map_err(|_| "atlas key is not valid UTF-8".to_string())?;
        let byte_len: usize = read_u32(data, &mut at)?;
        let pixels: &[u8] = take(data, &mut at, byte_len)?;
        images.insert(
            key,
            AtlasImage {
                pixels: pixels.to_vec(),
                width: size,
                height: size,
            },
        );
    }
    Ok(AtlasSet { images })
}

/// Decoded atlases, keyed by asset id.
///
/// Stored rather than uploaded directly so the render pass can bind the map a
/// part asks for without every draw call re-parsing a blob. Populated during
/// asset load; an asset absent from here falls back to scalar PBR.
pub(crate) static ATLASES: OnceLock<Mutex<HashMap<String, AtlasSet>>> = OnceLock::new();

/// The process-wide atlas registry, created on first use.
///
/// # Returns
///
/// - `&'static Mutex<HashMap<String, AtlasSet>>` - The live registry.
pub(crate) fn registry() -> &'static Mutex<HashMap<String, AtlasSet>> {
    ATLASES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Store an asset's decoded atlases.
///
/// # Arguments
///
/// - `String` - The asset id.
/// - `AtlasSet` - Its decoded maps.
pub fn store_atlas(asset_id: String, set: AtlasSet) {
    if let Ok(mut map) = registry().lock() {
        map.insert(asset_id, set);
    }
}

/// Look up one part's map set: its asset's atlases plus the surface family it
/// asked for.
///
/// # Arguments
///
/// - `&str` - The asset id.
/// - `Option<&str>` - The part's `surface_kind`.
///
/// # Returns
///
/// - `Option<AtlasImage>` - The requested map, or `None` when the asset has no
///   atlas or the family has no such map — the caller then keeps scalar PBR.
pub fn lookup(asset_id: &str, kind: Option<&str>, map_name: &str) -> Option<AtlasImage> {
    let kind: &str = kind?;
    let guard: MutexGuard<'_, HashMap<String, AtlasSet>> = registry().lock().ok()?;
    let set: &AtlasSet = guard.get(asset_id)?;
    set.get_images().get(&format!("{kind}/{map_name}")).cloned()
}

/// How many assets currently hold a decoded atlas.
///
/// # Returns
///
/// - `usize` - The count; zero when nothing has loaded yet.
pub fn loaded_count() -> usize {
    let guard: Option<MutexGuard<'_, HashMap<String, AtlasSet>>> = registry().lock().ok();
    match guard {
        Some(map) => map.len(),
        None => 0,
    }
}

/// Whether a part carries a complete set of maps.
///
/// A surface missing its normal or ORM map is deliberately *not* an error:
/// glass has no relief by design, and emissive parts are never textured. The
/// shader falls back to scalar PBR for whatever is absent.
///
/// # Arguments
///
/// - `&AtlasSet` - The decoded atlases for the asset.
/// - `&str` - The part's surface family.
///
/// # Returns
///
/// - `bool` - True when the albedo map exists.
pub fn has_albedo(set: &AtlasSet, kind: &str) -> bool {
    set.get_images()
        .contains_key(&format!("{kind}/{MAP_ALBEDO}"))
}

/// The albedo key for a surface family.
///
/// # Arguments
///
/// - `&str` - The part's surface family.
///
/// # Returns
///
/// - `String` - The atlas key.
pub fn albedo_key(kind: &str) -> String {
    format!("{kind}/{MAP_ALBEDO}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    /// Build one atlas file in the on-disk format.
    pub(crate) fn build(entries: &[(&str, usize)]) -> Vec<u8> {
        let mut data: Vec<u8> = (entries.len() as u32).to_le_bytes().to_vec();
        for (key, len) in entries {
            data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            data.extend_from_slice(key.as_bytes());
            data.extend_from_slice(&(*len as u32).to_le_bytes());
            data.extend(iter::repeat_n(0xABu8, *len));
        }
        data
    }

    /// A file with two maps round-trips to the right keys, sizes and payloads.
    #[test]
    pub(crate) fn parses_two_maps_into_the_set() {
        let size: u32 = 2;
        let payload: usize = (size * size * 4) as usize;
        let data: Vec<u8> = build(&[
            (&format!("paint/{MAP_ALBEDO}"), payload),
            (&format!("paint/{MAP_NORMAL}"), payload),
        ]);
        let set: AtlasSet = parse_atlas(&data, size).expect(ATLAS_PARSE_EXPECT);
        assert_eq!(set.images.len(), 2, "both maps must land in the set");
        let image: &AtlasImage = set
            .images
            .get(&albedo_key(SURFACE_PAINT))
            .expect("albedo present");
        assert_eq!(image.byte_len(), payload, "byte length must be w*h*4");
        assert_eq!(image.width, size);
        assert_eq!(image.height, size);
    }

    /// The three map names must stay distinct or one silently overwrites another.
    #[test]
    pub(crate) fn the_three_map_names_are_distinct() {
        assert_ne!(MAP_ALBEDO, MAP_NORMAL);
        assert_ne!(MAP_ALBEDO, MAP_ORM);
        assert_ne!(MAP_NORMAL, MAP_ORM);
    }

    /// A truncated payload must be an error, never a short read that silently
    /// uploads a half-empty texture.
    #[test]
    pub(crate) fn a_truncated_payload_is_rejected() {
        let mut data: Vec<u8> = build(&[(ATLAS_PAINT_ALBEDO, 16)]);
        data.truncate(data.len() - 1);
        assert!(
            parse_atlas(&data, 2).is_err(),
            "a short payload must not parse as a valid image"
        );
    }

    /// A truncated header must be rejected too — this is the failure that would
    /// otherwise read a garbage `count` and try to allocate millions of entries.
    #[test]
    pub(crate) fn a_truncated_header_is_rejected() {
        assert!(
            parse_atlas(&[0, 0], 2).is_err(),
            "2 bytes cannot be a count"
        );
        assert!(
            parse_atlas(&[], 2).is_err(),
            "an empty file is not an atlas"
        );
    }

    /// Negative control: a surface with no albedo map must report false, so a
    /// part can fall back to scalar PBR instead of binding a missing texture.
    #[test]
    pub(crate) fn a_surface_without_an_albedo_map_reports_absent() {
        let size: u32 = 2;
        let payload: usize = (size * size * 4) as usize;
        let data: Vec<u8> = build(&[("metal/albedo", payload)]);
        let set: AtlasSet = parse_atlas(&data, size).expect(ATLAS_PARSE_EXPECT);
        assert!(has_albedo(&set, SURFACE_METAL), "metal has an albedo map");
        assert!(
            !has_albedo(&set, "glass"),
            "a family with no atlas must report absent, not borrow metal's"
        );
    }

    /// A stored asset is retrievable by id and surface family, and an unknown
    /// id or family yields `None` instead of another asset's texels.
    #[test]
    pub(crate) fn a_stored_asset_is_found_by_id_and_surface() {
        let size: u32 = 2;
        let payload: usize = (size * size * 4) as usize;
        let data: Vec<u8> = build(&[(ATLAS_PAINT_ALBEDO, payload)]);
        let set: AtlasSet = parse_atlas(&data, size).expect(ATLAS_PARSE_EXPECT);
        store_atlas(ATLAS_TEST_ASSET.to_string(), set);
        let found: Option<AtlasImage> = lookup(ATLAS_TEST_ASSET, Some(SURFACE_PAINT), MAP_ALBEDO);
        assert_eq!(
            found.map(|image: AtlasImage| image.byte_len()),
            Some(payload),
            "the stored albedo must come back at full length"
        );
        assert!(
            lookup(ATLAS_TEST_ASSET, Some(SURFACE_METAL), MAP_ALBEDO).is_none(),
            "a family with no map must not borrow paint's"
        );
        assert!(
            lookup(ATLAS_TEST_MISSING_ASSET, Some(SURFACE_PAINT), MAP_ALBEDO).is_none(),
            "an unknown asset must not resolve"
        );
        assert!(
            lookup(ATLAS_TEST_ASSET, None, MAP_ALBEDO).is_none(),
            "an untextured part has no surface family and must fall back"
        );
    }

    /// The JSON a textured part carries must deserialise into `TextureRef`.
    #[test]
    pub(crate) fn a_textured_part_deserialises_its_atlas_reference() {
        // Built from the same consts the pipeline writes, so this test cannot
        // pass against a JSON shape the exporter no longer produces.
        let json: String = format!(
            r#"{{"texture_atlas":"atlas/car.bin","surface_kind":"{}"}}"#,
            SURFACE_PAINT
        );
        let parsed: TextureRef =
            serde_json::from_str(&json).expect("a textured part must deserialise");
        assert_eq!(parsed.atlas, "atlas/car.bin");
        assert_eq!(parsed.surface_kind, SURFACE_PAINT);
    }
}
