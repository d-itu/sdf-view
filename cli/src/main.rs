use std::{
    fs,
    io::{self, BufWriter, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[cfg(feature = "interactive")]
mod interactive;

use clap::Parser;
use sdf_view::{Antialiasing, Background, Camera, DirectionalLight, RenderOptions, Renderer};

mod error;
use error::{Error, InvalidBackground, InvalidColor};

/// Render a GLSL signed distance function to a PNG image.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// GLSL file defining float sdf(vec3 p), without #version or main
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Destination PNG file (overwrites an existing file)
    #[arg(short, long, value_name = "PNG")]
    #[cfg_attr(not(feature = "interactive"), arg(required = true))]
    output: Option<PathBuf>,

    #[cfg(feature = "interactive")]
    /// Open an interactive preview window (default when -o is omitted)
    #[arg(short, long, conflicts_with = "output")]
    interactive: bool,

    /// Background: transparent, checkerboard, rgb(r,g,b) in 0-255, black, or white
    #[arg(long, default_value = "transparent", value_parser = parse_background)]
    background: Background,

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

    /// sRGB object color: rgb(r,g,b), black, or white
    #[arg(long, default_value = "rgb(137,196,237)", value_parser = parse_color)]
    object_color: [u8; 3],

    /// sRGB light color: rgb(r,g,b), black, or white
    #[arg(long, default_value = "white", value_parser = parse_color)]
    light_color: [u8; 3],

    /// Nonnegative directional light strength
    #[arg(long, default_value_t = 0.85, allow_negative_numbers = true)]
    light_intensity: f32,

    /// Nonnegative white ambient light strength
    #[arg(long, default_value_t = 0.15, allow_negative_numbers = true)]
    ambient: f32,
}

fn parse_background(value: &str) -> Result<Background, InvalidBackground> {
    match value {
        "transparent" => Ok(Background::Transparent),
        "checkerboard" => Ok(Background::Checkerboard),
        _ => parse_color(value)
            .map(Background::Rgb)
            .map_err(|_| InvalidBackground),
    }
}

fn parse_color(value: &str) -> Result<[u8; 3], InvalidColor> {
    match value {
        "black" => Ok([0; 3]),
        "white" => Ok([255; 3]),
        _ => {
            let body = value
                .strip_prefix("rgb(")
                .and_then(|v| v.strip_suffix(')'))
                .ok_or(InvalidColor)?;
            let mut parts = body.split(',');
            let mut color = [0; 3];
            for component in &mut color {
                *component = parts
                    .next()
                    .ok_or(InvalidColor)?
                    .trim()
                    .parse()
                    .map_err(|_| InvalidColor)?;
            }
            if parts.next().is_some() {
                return Err(InvalidColor);
            }
            Ok(color)
        }
    }
}

fn linear_color(color: [u8; 3]) -> [f32; 3] {
    color.map(|value| {
        let srgb = f32::from(value) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    })
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
                color: linear_color(self.light_color),
                intensity: self.light_intensity,
                ambient: self.ambient,
            },
            background: self.background,
            object_color: linear_color(self.object_color),
        }
    }
}

fn run(args: Args) -> Result<(), Error> {
    let source = fs::read_to_string(&args.input)?;
    #[cfg(feature = "interactive")]
    if args.interactive || args.output.is_none() {
        return interactive::run(args, source);
    }
    let output = args
        .output
        .as_ref()
        .expect("clap requires output in batch mode");
    let renderer = Renderer::new()?;
    let pixels = renderer.render(&source, args.render_options())?;
    save_png(args.width, args.height, &pixels, output)?;
    tracing::info!(path = %output.display(), width = args.width, height = args.height, "Saved PNG");
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

fn save_png(width: u32, height: u32, pixels: &[u8], path: &Path) -> Result<(), Error> {
    let file = fs::File::create(path)?;
    let mut output = BufWriter::new(file);
    write_png(width, height, pixels, &mut output).map_err(|source| Error::Png {
        path: path.to_owned(),
        source,
    })?;
    output.flush()?;
    Ok(())
}

fn padded_row_bytes(width: u32) -> usize {
    let row_bytes = width as usize * 4;
    row_bytes.div_ceil(256) * 256
}

fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,sdf_view=info".into()),
        )
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .without_time()
        .init();
    if let Err(error) = args.render_options().validate() {
        let error = Error::Settings(error);
        tracing::error!("{error}");
        return error.exit_code();
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            error.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn color_arguments_share_srgb_format() {
        use super::*;
        for flag in ["--background", "--light-color", "--object-color"] {
            for value in ["black", "white", "rgb(12, 128,255)"] {
                let args =
                    Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png", flag, value])
                        .unwrap();
                let color = parse_color(value).unwrap();
                let options = args.render_options();
                match flag {
                    "--background" => assert_eq!(options.background, Background::Rgb(color)),
                    "--light-color" => assert_eq!(options.light.color, linear_color(color)),
                    _ => assert_eq!(options.object_color, linear_color(color)),
                }
            }
            for value in [
                "rgb(256,0,0)",
                "rgb(-1,0,0)",
                "rgb(1.5,0,0)",
                "rgb(1,2)",
                "rgb(1,2,3,4)",
                "1,0,0",
            ] {
                assert!(
                    Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png", flag, value])
                        .is_err()
                );
            }
        }
        assert!((linear_color([128; 3])[0] - 0.21586).abs() < 0.00001);
        let args = Args::try_parse_from(["sdf-view", "sphere.glsl", "-o", "out.png"]).unwrap();
        assert_eq!(
            args.render_options().object_color,
            RenderOptions::default().object_color
        );
    }

    use super::*;

    #[test]
    #[cfg(feature = "interactive")]
    fn interactive_arguments() {
        let args = Args::try_parse_from(["sdf-view", "sphere.glsl", "--interactive"]).unwrap();
        assert!(args.interactive);
        assert!(args.output.is_none());
        assert_eq!(args.background, Background::Transparent);
        let implicit = Args::try_parse_from(["sdf-view", "sphere.glsl"]).unwrap();
        assert!(implicit.output.is_none());
        for mode in [vec![], vec!["--interactive"], vec!["-o", "out.png"]] {
            for (value, expected) in [
                ("transparent", Background::Transparent),
                ("checkerboard", Background::Checkerboard),
                ("rgb(12, 128,255)", Background::Rgb([12, 128, 255])),
            ] {
                let mut argv = vec!["sdf-view", "sphere.glsl"];
                argv.extend(&mode);
                argv.extend(["--background", value]);
                let args = Args::try_parse_from(argv).unwrap();
                assert_eq!(args.render_options().background, expected);
            }
        }
        for value in [
            "rgb(256,0,0)",
            "rgb(-1,0,0)",
            "rgb(1,2)",
            "rgb(1,2,3,4)",
            "rgb(1.5,2,3)",
            "red",
            "rgb(1,2,3",
            "",
        ] {
            assert!(parse_background(value).is_err(), "{value}");
        }
        assert!(
            Args::try_parse_from(["sdf-view", "sphere.glsl", "--interactive", "--checkerboard"])
                .is_err()
        );
        let args = Args::try_parse_from([
            "sdf-view",
            "sphere.glsl",
            "--interactive",
            "--antialiasing",
            "4",
        ])
        .unwrap();
        assert_eq!(args.antialiasing, Antialiasing::X4);
        for flag in ["--interactive", "-i"] {
            assert!(
                Args::try_parse_from(["sdf-view", "sphere.glsl", flag, "-o", "out.png"]).is_err()
            );
        }
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
    #[cfg(feature = "gpu-test")]
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
