use sdf_view::{RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let options = RenderOptions::default();
    let pixels = renderer.render(include_str!("sphere.glsl"), options)?;
    println!("Rendered {}x{} RGBA8 pixels", options.width, options.height);
    let row_bytes = options.width as usize * 4;
    let stride = row_bytes.div_ceil(256) * 256;
    for row in pixels.chunks_exact(stride).take(options.height as usize) {
        assert_eq!(row[..row_bytes].len(), row_bytes);
    }
    Ok(())
}
