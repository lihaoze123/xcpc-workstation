use base64::Engine;

use crate::error::{PluginError, Result};

const MAX_RENDER_DIM: u32 = 4096;

/// Renders an SVG string to a PNG image and returns it as a base64-encoded string.
pub fn svg_to_png_base64(svg: &str, scale: Option<f32>) -> Result<String> {
    let scale = scale.unwrap_or(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PluginError::Render(
            "scale must be a positive finite number".into(),
        ));
    }
    let scale = scale.clamp(0.1, 8.0);

    let opts = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &opts)
        .map_err(|e| PluginError::Render(format!("invalid svg: {e}")))?;

    let size = tree.size();
    let width = ((size.width() * scale).ceil() as u32).clamp(1, MAX_RENDER_DIM);
    let height = ((size.height() * scale).ceil() as u32).clamp(1, MAX_RENDER_DIM);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        PluginError::Render(format!("failed to allocate pixmap {width}x{height}"))
    })?;

    let sx = width as f32 / size.width().max(1.0);
    let sy = height as f32 / size.height().max(1.0);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| PluginError::Render(format!("png header error: {e}")))?;
        writer
            .write_image_data(pixmap.data())
            .map_err(|e| PluginError::Render(format!("png encode error: {e}")))?;
    }

    Ok(base64::engine::general_purpose::STANDARD.encode(png_bytes))
}
