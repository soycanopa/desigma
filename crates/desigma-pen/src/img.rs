//! Image fills: `data:` URIs and files next to the `.pen`.

use std::path::{Component, Path, PathBuf};

/// Largest image read from disk or decoded from a `data:` URI.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Bytes of the image `url` names, as PNG or JPEG (the renderer decodes only those), or why not.
pub fn load(url: &str, base_dir: Option<&Path>) -> Result<Vec<u8>, String> {
    let raw = if let Some(rest) = url.strip_prefix("data:") { decode_data_uri(rest)? } else { read_file(url, base_dir)? };
    normalize(raw)
}

fn read_file(url: &str, base_dir: Option<&Path>) -> Result<Vec<u8>, String> {
    if url.contains("://") {
        return Err(format!("remote image {url} is not downloaded"));
    }
    let Some(base) = base_dir else { return Err(format!("image {url} is a file next to the .pen, which was opened without its location")) };
    let rel = Path::new(url);
    // Only files below the .pen's folder: a design must not pull in arbitrary local files.
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(format!("image {url} points outside the .pen's folder"));
    }
    let path: PathBuf = base.join(rel);
    let canon = path.canonicalize().map_err(|e| format!("image {url} cannot be read: {e}"))?;
    let base_canon = base.canonicalize().map_err(|e| format!("image {url} cannot be read: {e}"))?;
    if !canon.starts_with(&base_canon) {
        return Err(format!("image {url} points outside the .pen's folder"));
    }
    let len = std::fs::metadata(&canon).map_err(|e| format!("image {url} cannot be read: {e}"))?.len();
    if len > MAX_IMAGE_BYTES {
        return Err(format!("image {url} is too large"));
    }
    std::fs::read(&canon).map_err(|e| format!("image {url} cannot be read: {e}"))
}

fn decode_data_uri(rest: &str) -> Result<Vec<u8>, String> {
    let (meta, data) = rest.split_once(',').ok_or("malformed data: URI")?;
    if !meta.ends_with(";base64") {
        return Err("only base64 data: URIs are supported".into());
    }
    if data.len() as u64 > MAX_IMAGE_BYTES * 4 / 3 + 8 {
        return Err("embedded image is too large".into());
    }
    base64(data).ok_or_else(|| "malformed base64 image".to_string())
}

/// PNG and JPEG pass through; GIF and WebP are re-encoded as PNG.
fn normalize(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png | image::ImageFormat::Jpeg) => Ok(bytes),
        Ok(image::ImageFormat::Gif | image::ImageFormat::WebP) => {
            let img = image::load_from_memory(&bytes).map_err(|e| format!("image cannot be decoded: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("image cannot be re-encoded: {e}"))?;
            Ok(out.into_inner())
        }
        _ => Err("unsupported image format (PNG, JPEG, GIF and WebP are supported)".into()),
    }
}

/// Key under which the bytes are stored in `Document::images`.
pub fn key(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("pen-{h:016x}")
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

    #[test]
    fn base64_round_trips_a_png() {
        let bytes = base64(PNG_1X1).unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
        assert!(load(&format!("data:image/png;base64,{PNG_1X1}"), None).is_ok());
    }

    #[test]
    fn rejects_what_is_not_an_image() {
        assert!(load("data:text/plain;base64,aGVsbG8=", None).is_err());
        assert!(load("data:image/png,rawbytes", None).is_err());
        assert!(load("data:image/png;base64,!!!", None).is_err());
        assert!(load("data:nocomma", None).is_err());
    }

    #[test]
    fn files_stay_below_the_pen_folder() {
        let dir = std::env::temp_dir().join(format!("desigma_pen_img_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.png"), base64(PNG_1X1).unwrap()).unwrap();
        assert!(load("./sub/a.png", Some(&dir)).is_ok());
        assert!(load("../a.png", Some(&dir)).unwrap_err().contains("outside"));
        assert!(load("/etc/passwd", Some(&dir)).unwrap_err().contains("outside"));
        assert!(load("sub/missing.png", Some(&dir)).is_err());
        assert!(load("sub/a.png", None).is_err());
        assert!(load("https://example.com/a.png", Some(&dir)).unwrap_err().contains("not downloaded"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keys_are_stable_and_content_based() {
        assert_eq!(key(b"abc"), key(b"abc"));
        assert_ne!(key(b"abc"), key(b"abd"));
    }
}
