use std::process::{Command, Output};
#[cfg(feature = "gpu-test")]
use std::{fs, io::BufReader, path::PathBuf};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sdf-view"))
        .args(args)
        .env_remove("RUST_LOG")
        .output()
        .unwrap()
}

#[test]
fn logs_to_stderr_with_environment_filter() {
    for filter in [None, Some("invalid["), Some("off")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sdf-view"));
        command.args(["unused.glsl", "-o", "unused.png", "--fov", "180"]);
        command.env_remove("RUST_LOG");
        if let Some(filter) = filter {
            command.env("RUST_LOG", filter);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        if filter == Some("off") {
            assert!(output.stderr.is_empty());
        } else {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("ERROR"), "{stderr}");
            assert!(stderr.contains("vertical FOV"), "{stderr}");
            assert!(!stderr.contains('\u{1b}'));
        }
    }
}

#[test]
fn help_version_and_invalid_arguments() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for flag in [
        "<INPUT>",
        "--output",
        "--width",
        "--height",
        "--antialiasing",
        "--background",
    ] {
        assert!(text.contains(flag), "missing {flag}");
    }
    for extra in [
        vec!["--background", "rgb(256,0,0)"],
        vec!["--background", "rgb(1,2)"],
        vec!["--checkerboard"],
        vec!["--antialiasing", "0"],
        vec!["--antialiasing", "2"],
        vec!["--antialiasing", "16"],
        vec!["--antialiasing", "x4"],
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
        #[cfg(not(feature = "interactive"))]
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

#[cfg(feature = "gpu-test")]
struct Workspace(PathBuf);

#[cfg(feature = "gpu-test")]
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
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    }
}

#[cfg(feature = "gpu-test")]
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(feature = "gpu-test")]
fn renders_png_and_reports_runtime_errors() {
    let workspace = Workspace::new();
    let source = include_str!("../../examples/sphere.glsl");
    fs::write(workspace.0.join("sphere.glsl"), source).unwrap();

    let missing = workspace.run(&["missing.glsl", "-o", "image.png"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("No such file or directory"));

    assert_eq!(
        fs::read_to_string(workspace.0.join("sphere.glsl")).unwrap(),
        source
    );

    for (dimensions, samples, antialiasing) in [
        (vec![], "1", sdf_view::Antialiasing::X1),
        (
            vec!["--width", "129", "--height", "97"],
            "4",
            sdf_view::Antialiasing::X4,
        ),
    ] {
        let mut args = vec!["sphere.glsl", "-o", "image.png", "--antialiasing", samples];
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
        let renderer = sdf_view::Renderer::new().unwrap();
        let mapped = renderer
            .render(
                source,
                sdf_view::RenderOptions {
                    width: expected.0,
                    height: expected.1,
                    antialiasing,
                    ..Default::default()
                },
            )
            .unwrap();
        let row_bytes = expected.0 as usize * 4;
        let stride = row_bytes.div_ceil(256) * 256;
        assert_eq!(mapped.len(), stride * expected.1 as usize);
        for (decoded, row) in pixels[..frame.buffer_size()]
            .chunks_exact(row_bytes)
            .zip(mapped.chunks_exact(stride))
        {
            assert_eq!(decoded, &row[..row_bytes]);
        }
        assert_eq!(pixels[3], 0);
        let center = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
        assert_eq!(pixels[center + 3], 255);
    }

    fs::write(
        workspace.0.join("empty.glsl"),
        "float sdf(vec3 p) { return 200.0; }",
    )
    .unwrap();
    for background in ["transparent", "checkerboard", "rgb(12,128,255)"] {
        let output = workspace.run(&[
            "empty.glsl",
            "-o",
            "background.png",
            "--width",
            "33",
            "--height",
            "17",
            "--background",
            background,
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut reader = png::Decoder::new(BufReader::new(
            fs::File::open(workspace.0.join("background.png")).unwrap(),
        ))
        .read_info()
        .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        match background {
            "transparent" => assert!(pixels.iter().all(|v| *v == 0)),
            "checkerboard" => {
                assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
                assert_ne!(&pixels[..4], &pixels[64..68]);
            }
            _ => assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p == &[12, 128, 255, 255])
            ),
        }
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
        "rgb(255,0,0)",
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

    for (flag, other, intensity, ambient) in [
        ("--object-color", "--light-color", "0", "1"),
        ("--light-color", "--object-color", "1", "0"),
    ] {
        let result = workspace.run(&[
            "sphere.glsl",
            "-o",
            "color.png",
            "--width",
            "1",
            "--height",
            "1",
            flag,
            "rgb(12,128,255)",
            other,
            "white",
            "--light-direction",
            "0,0,1",
            "--light-intensity",
            intensity,
            "--ambient",
            ambient,
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut reader = png::Decoder::new(BufReader::new(
            fs::File::open(workspace.0.join("color.png")).unwrap(),
        ))
        .read_info()
        .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        for (actual, expected) in pixels[..3].iter().zip([12u8, 128, 255]) {
            assert!(actual.abs_diff(expected) <= 1, "{flag}: {pixels:?}");
        }
        assert_eq!(pixels[3], 255);
    }

    let previous_png = fs::read(workspace.0.join("image.png")).unwrap();
    fs::write(
        workspace.0.join("broken.glsl"),
        "float sdf(vec3 p) { invalid }",
    )
    .unwrap();
    let broken = workspace.run(&["broken.glsl", "-o", "image.png"]);
    assert_eq!(broken.status.code(), Some(1));
    assert_eq!(
        fs::read(workspace.0.join("image.png")).unwrap(),
        previous_png
    );

    let unwritable = workspace.run(&["sphere.glsl", "-o", "missing/image.png"]);
    assert_eq!(unwritable.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unwritable.stderr).contains("No such file or directory"));
}
