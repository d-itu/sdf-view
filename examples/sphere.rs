use sdf_view::{RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let image = renderer.render(include_str!("sphere.glsl"), RenderOptions::default())?;
    println!("Rendered {}x{} RGBA8 pixels", image.width(), image.height());
    for row in image.rows() {
        assert_eq!(row.len(), image.width() as usize * 4);
    }
    Ok(())
}
