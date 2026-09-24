use sdf_view::{RenderOptions, Renderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| "sphere.png".into());
    let mut renderer = Renderer::new()?;
    eprintln!("Adapter: {:?}", renderer.adapter_info());
    let image = renderer.render(include_str!("sphere.glsl"), RenderOptions::default())?;
    image.save_png(&output)?;
    eprintln!("Saved {}", std::path::Path::new(&output).display());
    Ok(())
}
