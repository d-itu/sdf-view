use std::{fs, path::PathBuf, process::ExitCode};

use clap::Parser;
use sdf_view::{RenderOptions, Renderer};

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
        .render(
            &source,
            RenderOptions {
                width: args.width,
                height: args.height,
            },
        )
        .map_err(|error| format!("could not render '{}': {error}", args.input.display()))?;
    image
        .save_png(&args.output)
        .map_err(|error| format!("could not write '{}': {error}", args.output.display()))?;
    eprintln!(
        "Saved {} ({}x{})",
        args.output.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
