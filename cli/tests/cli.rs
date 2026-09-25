use std::{
    fs,
    io::BufReader,
    path::PathBuf,
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sdf-view"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_version_and_invalid_arguments() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for flag in ["<INPUT>", "--output", "--width", "--height"] {
        assert!(text.contains(flag), "missing {flag}");
    }
    for extra in [
        vec!["--camera-position", "1,2"],
        vec!["--camera-position", "NaN,0,3"],
        vec!["--camera-target", "0,0,3"],
        vec!["--camera-up", "0,0,1"],
        vec!["--fov", "180"],
        vec!["--fov", "NaN"],
        vec!["--light-direction", "0,0,0"],
        vec!["--light-color", "1,2,1"],
        vec!["--light-intensity", "-1"],
        vec!["--ambient", "inf"],
    ] {
        let mut args = vec!["examples/sphere.glsl", "-o", "unused.png"];
        args.extend(extra);
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    assert!(cli(&["--version"]).status.success());
    for args in [
        vec![],
        vec!["examples/sphere.glsl"],
        vec!["examples/sphere.glsl", "-o", "unused.png", "--width", "0"],
        vec!["examples/sphere.glsl", "-o", "unused.png", "--height", "-1"],
        vec![
            "examples/sphere.glsl",
            "-o",
            "unused.png",
            "--width",
            "4294967296",
        ],
        vec![
            "examples/sphere.glsl",
            "-o",
            "unused.png",
            "--height",
            "abc",
        ],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(!output.stderr.is_empty());
    }
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("cli-test-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sdf-view"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn renders_png_and_reports_runtime_errors() {
    let workspace = Workspace::new();
    let source = include_str!("../../examples/sphere.glsl");
    let mut renderer = sdf_view::Renderer::new().unwrap();
    fs::write(workspace.0.join("sphere.glsl"), source).unwrap();

    let missing = workspace.run(&["missing.glsl", "-o", "image.png"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("could not read 'missing.glsl'"));

    let same = workspace.run(&["sphere.glsl", "-o", "./sphere.glsl"]);
    assert_eq!(same.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(workspace.0.join("sphere.glsl")).unwrap(),
        source
    );

    for dimensions in [vec![], vec!["--width", "129", "--height", "97"]] {
        let mut args = vec!["sphere.glsl", "-o", "image.png"];
        args.extend_from_slice(&dimensions);
        let output = workspace.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        let decoder = png::Decoder::new(BufReader::new(
            fs::File::open(workspace.0.join("image.png")).unwrap(),
        ));
        let mut reader = decoder.read_info().unwrap();
        let expected = if dimensions.is_empty() {
            (512, 512)
        } else {
            (129, 97)
        };
        assert_eq!((reader.info().width, reader.info().height), expected);
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        assert_eq!(frame.bit_depth, png::BitDepth::Eight);
        assert_eq!(
            reader.info().srgb,
            Some(png::SrgbRenderingIntent::Perceptual)
        );
        let image = renderer
            .render(
                source,
                sdf_view::RenderOptions {
                    width: expected.0,
                    height: expected.1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(&pixels[..frame.buffer_size()], image.pixels());
        assert_eq!(pixels[3], 0);
        let center = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
        assert_eq!(pixels[center + 3], 255);
    }

    let custom = workspace.run(&[
        "sphere.glsl",
        "-o",
        "custom.png",
        "--width",
        "65",
        "--height",
        "65",
        "--camera-position",
        "-3,0,0",
        "--camera-target",
        "0,0,0",
        "--camera-up",
        "0,-1,0",
        "--fov",
        "60",
        "--light-direction",
        "-1,0,0",
        "--light-color",
        "1,0,0",
        "--light-intensity",
        "0.5",
        "--ambient",
        "0",
    ]);
    assert!(
        custom.status.success(),
        "{}",
        String::from_utf8_lossy(&custom.stderr)
    );
    let mut reader = png::Decoder::new(BufReader::new(
        fs::File::open(workspace.0.join("custom.png")).unwrap(),
    ))
    .read_info()
    .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut pixels).unwrap();
    let center = (32 * 65 + 32) * 4;
    assert!(pixels[center] > 0);
    assert_eq!(&pixels[center + 1..center + 4], &[0, 0, 255]);

    let previous_png = fs::read(workspace.0.join("image.png")).unwrap();
    fs::write(
        workspace.0.join("broken.glsl"),
        "float sdf(vec3 p) { invalid }",
    )
    .unwrap();
    let broken = workspace.run(&["broken.glsl", "-o", "image.png"]);
    assert_eq!(broken.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&broken.stderr).contains("could not render 'broken.glsl'"));
    assert_eq!(
        fs::read(workspace.0.join("image.png")).unwrap(),
        previous_png
    );

    let unwritable = workspace.run(&["sphere.glsl", "-o", "missing/image.png"]);
    assert_eq!(unwritable.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&unwritable.stderr).contains("could not write 'missing/image.png'")
    );
}
