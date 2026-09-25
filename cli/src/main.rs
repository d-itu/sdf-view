use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use sdf_view::{Camera, DirectionalLight, Image, RenderOptions, Renderer};

/// Render a GLSL signed distance function to a PNG image.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// GLSL file defining float sdf(vec3 p), without #version or main
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Destination PNG file (overwrites an existing file)
    #[arg(short, long, value_name = "PNG")]
    output: PathBuf,

    /// Image width in pixels
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(1..))]
    width: u32,

    /// Image height in pixels
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(1..))]
    height: u32,

    /// Camera position in world coordinates
    #[arg(long, default_value = "0,0,3", value_parser = parse_vec3, value_name = "X,Y,Z", allow_hyphen_values = true)]
    camera_position: [f32; 3],

    /// Point the camera looks at
    #[arg(long, default_value = "0,0,0", value_parser = parse_vec3, value_name = "X,Y,Z", allow_hyphen_values = true)]
    camera_target: [f32; 3],

    /// Camera up direction (must not be parallel to the viewing direction)
    #[arg(long, default_value = "0,1,0", value_parser = parse_vec3, value_name = "X,Y,Z", allow_hyphen_values = true)]
    camera_up: [f32; 3],

    /// Vertical field of view in degrees, strictly between 0 and 180
    #[arg(long, default_value_t = 45.0, allow_negative_numbers = true)]
    fov: f32,

    /// World-space direction from the surface toward the light
    #[arg(long, default_value = "-0.5,0.8,1", value_parser = parse_vec3, value_name = "X,Y,Z", allow_hyphen_values = true)]
    light_direction: [f32; 3],

    /// Linear RGB light color, each component between 0 and 1
    #[arg(long, default_value = "1,1,1", value_parser = parse_vec3, value_name = "R,G,B", allow_hyphen_values = true)]
    light_color: [f32; 3],

    /// Nonnegative directional light strength
    #[arg(long, default_value_t = 0.85, allow_negative_numbers = true)]
    light_intensity: f32,

    /// Nonnegative white ambient light strength
    #[arg(long, default_value_t = 0.15, allow_negative_numbers = true)]
    ambient: f32,
}

fn parse_vec3(value: &str) -> Result<[f32; 3], String> {
    let values: Vec<f32> = value
        .split(',')
        .map(|part| part.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| "expected three comma-separated numbers".to_string())?;
    let vector: [f32; 3] = values
        .try_into()
        .map_err(|_| "expected exactly three comma-separated numbers".to_string())?;
    if !vector.iter().all(|value| value.is_finite()) {
        return Err("vector components must be finite".into());
    }
    Ok(vector)
}

impl Args {
    fn render_options(&self) -> RenderOptions {
        RenderOptions {
            width: self.width,
            height: self.height,
            camera: Camera {
                position: self.camera_position,
                target: self.camera_target,
                up: self.camera_up,
                vertical_fov_degrees: self.fov,
            },
            light: DirectionalLight {
                direction: self.light_direction,
                color: self.light_color,
                intensity: self.light_intensity,
                ambient: self.ambient,
            },
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let source = fs::read_to_string(&args.input)
        .map_err(|error| format!("could not read '{}': {error}", args.input.display()))?;
    if let (Ok(input), Ok(output)) = (
        fs::canonicalize(&args.input),
        fs::canonicalize(&args.output),
    ) && input == output
    {
        return Err("input and output must be different files".into());
    }
    let mut renderer =
        Renderer::new().map_err(|error| format!("could not initialize renderer: {error}"))?;
    let image = renderer
        .render(&source, args.render_options())
        .map_err(|error| format!("could not render '{}': {error}", args.input.display()))?;
    save_png(&image, &args.output)
        .map_err(|error| format!("could not write '{}': {error}", args.output.display()))?;
    eprintln!(
        "Saved {} ({}x{})",
        args.output.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

fn write_png(image: &Image, output: impl Write) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(output, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    {
        let mut stream = writer.stream_writer()?;
        for row in image.rows() {
            stream.write_all(row)?;
        }
        stream.finish()?;
    }
    writer.finish()
}

fn save_png(image: &Image, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(fs::File::create(path)?);
    write_png(image, &mut output)?;
    output.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Err(error) = args.render_options().validate() {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_output_errors_are_returned() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("test write failure"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut renderer = Renderer::new().expect("a working graphics adapter is required");
        let image = renderer
            .render(
                "float sdf(vec3 p) { return length(p) - 1.0; }",
                RenderOptions {
                    width: 1,
                    height: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(write_png(&image, BrokenWriter).is_err());
    }
}
