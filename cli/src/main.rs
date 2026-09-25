use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[cfg(feature = "interactive")]
mod interactive;

use clap::Parser;
use sdf_view::{Antialiasing, Camera, DirectionalLight, RenderOptions, Renderer};

/// Render a GLSL signed distance function to a PNG image.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// GLSL file defining float sdf(vec3 p), without #version or main
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Destination PNG file (overwrites an existing file)
    #[arg(short, long, value_name = "PNG")]
    #[cfg_attr(feature = "interactive", arg(required_unless_present = "interactive"))]
    #[cfg_attr(not(feature = "interactive"), arg(required = true))]
    output: Option<PathBuf>,

    #[cfg(feature = "interactive")]
    /// Open an interactive preview window
    #[arg(short, long)]
    interactive: bool,

    /// Image width in pixels
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(1..))]
    width: u32,

    /// Image height in pixels
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(1..))]
    height: u32,

    /// Rays per pixel: 1 disables antialiasing, 4 uses a 2-by-2 grid
    #[arg(long, default_value = "1", value_parser = parse_antialiasing, value_name = "1|4")]
    antialiasing: Antialiasing,

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

fn parse_antialiasing(value: &str) -> Result<Antialiasing, &'static str> {
    match value {
        "1" => Ok(Antialiasing::X1),
        "4" => Ok(Antialiasing::X4),
        _ => Err("expected 1 or 4 rays per pixel"),
    }
}

fn parse_vec3(value: &str) -> Result<[f32; 3], &'static str> {
    let mut parts = value.split(',');
    let mut vector = [0.0_f32; 3];
    for component in &mut vector {
        *component = parts
            .next()
            .ok_or("expected exactly three comma-separated numbers")?
            .trim()
            .parse()
            .map_err(|_| "expected three comma-separated numbers")?;
    }
    if parts.next().is_some() {
        return Err("expected exactly three comma-separated numbers");
    }
    if !vector.iter().all(|value| value.is_finite()) {
        return Err("vector components must be finite");
    }
    Ok(vector)
}

impl Args {
    fn render_options(&self) -> RenderOptions {
        RenderOptions {
            width: self.width,
            height: self.height,
            antialiasing: self.antialiasing,
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
    if let Some(output) = &args.output {
        check_output(&args.input, output)?;
    }
    #[cfg(feature = "interactive")]
    if args.interactive {
        return interactive::run(args, source);
    }
    let output = args
        .output
        .as_ref()
        .expect("clap requires output in batch mode");
    let renderer =
        Renderer::new().map_err(|error| format!("could not initialize renderer: {error}"))?;
    let pixels = renderer
        .render(&source, args.render_options())
        .map_err(|error| format!("could not render '{}': {error}", args.input.display()))?;
    save_png(args.width, args.height, &pixels, output)
        .map_err(|error| format!("could not write '{}': {error}", output.display()))?;
    eprintln!(
        "Saved {} ({}x{})",
        output.display(),
        args.width,
        args.height
    );
    Ok(())
}

fn check_output(input: &Path, output: &Path) -> Result<(), String> {
    if let (Ok(input), Ok(output)) = (fs::canonicalize(input), fs::canonicalize(output))
        && input == output
    {
        return Err("input and output must be different files".into());
    }
    Ok(())
}

fn write_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    output: impl Write,
) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    {
        let mut stream = writer.stream_writer()?;
        for row in pixels.chunks_exact(padded_row_bytes(width)) {
            stream.write_all(&row[..width as usize * 4])?;
        }
        stream.finish()?;
    }
    writer.finish()
}

fn save_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(fs::File::create(path)?);
    write_png(width, height, pixels, &mut output)?;
    output.flush()?;
    Ok(())
}

fn padded_row_bytes(width: u32) -> usize {
    let row_bytes = width as usize * 4;
    row_bytes.div_ceil(256) * 256
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
    #[cfg(feature = "interactive")]
    fn interactive_arguments() {
        let args = Args::try_parse_from(["sdf-view", "sphere.glsl", "--interactive"]).unwrap();
        assert!(args.interactive);
        assert!(args.output.is_none());
        let args = Args::try_parse_from([
            "sdf-view",
            "sphere.glsl",
            "--interactive",
            "-o",
            "snapshot.png",
            "--antialiasing",
            "4",
        ])
        .unwrap();
        assert_eq!(args.output.as_deref(), Some(Path::new("snapshot.png")));
        assert_eq!(args.antialiasing, Antialiasing::X4);
        assert!(Args::try_parse_from(["sdf-view", "sphere.glsl"]).is_err());
        assert!(
            Args::try_parse_from([
                "sdf-view",
                "sphere.glsl",
                "--interactive",
                "--antialiasing",
                "2"
            ])
            .is_err()
        );
    }

    #[test]
    #[cfg(not(feature = "interactive"))]
    fn offline_arguments_require_output_and_reject_interactive() {
        assert!(Args::try_parse_from(["sdf-view", "sphere.glsl"]).is_err());
        assert!(
            Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png", "--interactive"])
                .is_err()
        );
        assert!(Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png", "-i"]).is_err());
        let args = Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png"]).unwrap();
        assert_eq!(args.output.as_deref(), Some(Path::new("out.png")));
        assert_eq!(args.background, Background::Transparent);
    }

    #[test]
    fn parses_exactly_three_finite_components() {
        assert_eq!(parse_vec3(" -1, 2.5, 3e-2 "), Ok([-1.0, 2.5, 0.03]));
        for input in [
            "",
            "1,2",
            "1,2,3,4",
            "1,,3",
            "1,2,",
            "x,2,3",
            "NaN,2,3",
            "1,inf,3",
            "1,2,1e100",
        ] {
            assert!(parse_vec3(input).is_err(), "{input}");
        }
    }

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
        let renderer = Renderer::new().expect("a working graphics adapter is required");
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
        assert!(write_png(1, 1, &image, BrokenWriter).is_err());
    }
}
