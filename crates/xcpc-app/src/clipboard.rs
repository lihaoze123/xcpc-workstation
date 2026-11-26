//! Cross-platform clipboard image helpers.
//!
//! On Windows, `nsh-platform-windows`'s `Clipboard.writeImage` only registers the
//! custom `"PNG"` format without `CF_DIB`, which causes Paint, WeChat, QQ, Word, and
//! classic Win32/.NET apps to ignore pasted images. This module decodes PNG bytes into
//! a standard 32bpp bottom-up `CF_DIB` and writes **both** `CF_DIB` and `"PNG"` to the
//! Windows clipboard in one atomic transaction.

use base64::Engine as _;

/// Decodes either raw base64 PNG or a `data:image/png;base64,...` data URL into raw PNG bytes.
pub fn decode_png_base64(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty image data".to_owned());
    }
    let payload = if let Some(rest) = trimmed.strip_prefix("data:") {
        let Some((header, data)) = rest.split_once(',') else {
            return Err("invalid data URL".to_owned());
        };
        if !header.to_ascii_lowercase().contains(";base64") {
            return Err("data URL must be base64-encoded".to_owned());
        }
        data
    } else {
        trimmed
    };
    let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err("empty base64 payload".to_owned());
    }
    base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| format!("invalid PNG base64: {e}"))
}

/// Decodes PNG bytes into a Windows `CF_DIB` buffer (40-byte `BITMAPINFOHEADER` +
/// bottom-up 32bpp BGRA pixel data with transparent pixels composited onto white).
pub fn png_to_cf_dib(png_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("failed to read PNG header: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG image too large".to_owned())?;
    let mut raw = vec![0u8; buf_size];
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| format!("failed to decode PNG pixels: {e}"))?;

    let width = info.width as usize;
    let height = info.height as usize;
    if width == 0 || height == 0 {
        return Err("PNG has zero dimensions".to_owned());
    }
    let src = &raw[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(width * height * 4);
            for &[r, g, b] in src.as_chunks::<3>().0 {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(width * height * 4);
            for &[g, a] in src.as_chunks::<2>().0 {
                out.extend_from_slice(&[g, g, g, a]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(width * height * 4);
            for &g in src {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => return Err(format!("unsupported decoded PNG color type: {other:?}")),
    };

    let pixel_bytes = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "PNG dimensions overflow".to_owned())?;
    if rgba.len() < pixel_bytes {
        return Err("truncated decoded PNG frame".to_owned());
    }

    const HEADER_SIZE: usize = 40;
    let mut dib = Vec::with_capacity(HEADER_SIZE + pixel_bytes);
    // BITMAPINFOHEADER (little-endian, 40 bytes):
    dib.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // biSize
    dib.extend_from_slice(&(info.width as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&(info.height as i32).to_le_bytes()); // biHeight (>0 = bottom-up)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    let row_stride = width * 4;
    for y in (0..height).rev() {
        let row = &rgba[y * row_stride..(y + 1) * row_stride];
        for &[pr, pg, pb, pa] in row.as_chunks::<4>().0 {
            let a = pa as u32;
            let (r, g, b) = if a == 255 {
                (pr, pg, pb)
            } else {
                // Composite over white so classic Win32 apps that ignore alpha in CF_DIB
                // do not render transparent margins as solid black.
                let inv = 255 - a;
                (
                    ((pr as u32 * a + 255 * inv + 127) / 255) as u8,
                    ((pg as u32 * a + 255 * inv + 127) / 255) as u8,
                    ((pb as u32 * a + 255 * inv + 127) / 255) as u8,
                )
            };
            dib.extend_from_slice(&[b, g, r, 255]);
        }
    }
    Ok(dib)
}

/// Writes a base64-encoded PNG (raw or `data:image/png;base64,...`) to the native Windows
/// clipboard as both `CF_DIB` and `"PNG"`.
///
/// Returns `Ok(true)` when handled natively (Windows) or `Ok(false)` on non-Windows so
/// callers fall back to NSH's built-in `Clipboard.writeImage`.
pub fn write_clipboard_image_png(png_src: &str) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let png_bytes = decode_png_base64(png_src)?;
        let dib_bytes = png_to_cf_dib(&png_bytes)?;
        write_windows_clipboard_dib_and_png(&dib_bytes, &png_bytes)?;
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = png_src;
        Ok(false)
    }
}

#[cfg(windows)]
fn write_windows_clipboard_dib_and_png(dib: &[u8], png: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_DIB;

    unsafe fn set_global_format(format: u32, data: &[u8]) -> Result<(), String> {
        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len()) };
        if hmem.is_null() {
            return Err("GlobalAlloc failed for clipboard image".to_owned());
        }
        let ptr = unsafe { GlobalLock(hmem) };
        if ptr.is_null() {
            unsafe { GlobalFree(hmem) };
            return Err("GlobalLock failed for clipboard image".to_owned());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            GlobalUnlock(hmem);
        }
        let handle = unsafe { SetClipboardData(format, hmem) };
        if handle.is_null() {
            unsafe { GlobalFree(hmem) };
            return Err(format!("SetClipboardData({format}) failed"));
        }
        Ok(())
    }

    let mut opened = false;
    for _ in 0..5 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err("failed to open Windows clipboard".to_owned());
    }

    let res = (|| {
        if unsafe { EmptyClipboard() } == 0 {
            return Err("EmptyClipboard failed".to_owned());
        }
        unsafe { set_global_format(CF_DIB as u32, dib)? };
        let png_name: [u16; 4] = [b'P' as u16, b'N' as u16, b'G' as u16, 0];
        let png_fmt = unsafe { RegisterClipboardFormatW(png_name.as_ptr()) };
        if png_fmt != 0 {
            let _ = unsafe { set_global_format(png_fmt, png) };
        }
        Ok(())
    })();

    unsafe { CloseClipboard() };
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_2x1_rgba_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().expect("png header");
            // Pixel 0: opaque red (255, 0, 0, 255); Pixel 1: fully transparent (0, 0, 0, 0)
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 0, 0, 0])
                .expect("png data");
        }
        out
    }

    #[test]
    fn dib_from_png_builds_valid_bottom_up_32bpp_header_and_pixels() {
        let png_bytes = make_2x1_rgba_png();
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png_bytes)
        );
        let decoded = decode_png_base64(&data_url).expect("decode data url");
        assert_eq!(decoded, png_bytes);

        let dib = png_to_cf_dib(&decoded).expect("convert to CF_DIB");
        assert_eq!(dib.len(), 40 + 2 * 4);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        // Pixel 0: opaque red in BGRA = [0, 0, 255, 255]
        assert_eq!(&dib[40..44], &[0, 0, 255, 255]);
        // Pixel 1: transparent composited onto white = [255, 255, 255, 255]
        assert_eq!(&dib[44..48], &[255, 255, 255, 255]);
    }
}
