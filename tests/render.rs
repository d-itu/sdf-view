#![cfg(feature = "gpu-test")]

use sdf_view::{Error, RenderOptions, Renderer};

fn render(renderer: &mut Renderer, sdf: &str, options: RenderOptions) -> TestImage {
    let width = options.width;
    let height = options.height;
    let view = renderer.render(sdf, options).unwrap();
    TestImage {
        width,
        height,
        pixels: packed(&view, width, height),
    }
}

struct TestImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn packed(view: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = width as usize * 4;
    let stride = row_bytes.div_ceil(256) * 256;
    view.chunks_exact(stride)
        .take(height as usize)
        .flat_map(|row| row[..row_bytes].iter().copied())
        .collect()
}

#[test]
fn antialiasing_preserves_straight_alpha() {
    use sdf_view::{Antialiasing, DirectionalLight};

    let mut renderer = Renderer::new().expect("a working graphics adapter is required");
    let source = include_str!("../examples/sphere.glsl");
    let options = RenderOptions {
        width: 129,
        height: 97,
        light: DirectionalLight {
            intensity: 0.0,
            ambient: 1.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let default = render(&mut renderer, source, options);
    let single = render(
        &mut renderer,
        source,
        RenderOptions {
            antialiasing: Antialiasing::X1,
            ..options
        },
    );
    assert_eq!(default.pixels, single.pixels);
    assert!(
        single
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 0 || p[3] == 255)
    );

    let reference = single
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .find(|p| p[3] == 255)
        .unwrap();
    let multi = render(
        &mut renderer,
        source,
        RenderOptions {
            antialiasing: Antialiasing::X4,
            ..options
        },
    );
    let mut coverage = [false; 5];
    for pixel in multi.pixels.as_chunks::<4>().0 {
        let index = match pixel[3] {
            0 => 0,
            64 => 1,
            127 | 128 => 2,
            191 => 3,
            255 => 4,
            alpha => panic!("unexpected four-sample coverage: {alpha}"),
        };
        coverage[index] = true;
        if index == 0 {
            assert_eq!(pixel, &[0, 0, 0, 0]);
        } else {
            // Constant lighting must not darken partially covered pixels.
            assert_eq!(&pixel[..3], &reference[..3]);
        }
    }
    assert!(coverage.into_iter().all(|present| present));
    let center = ((options.height / 2 * options.width + options.width / 2) * 4) as usize;
    assert_eq!(multi.pixels[center + 3], 255);
    assert_eq!(&multi.pixels[..4], &[0, 0, 0, 0]);
}

// This is an actual graphics integration test. A missing adapter is a failure,
// not a silent skip; software Vulkan implementations can run it in CI.
#[test]
fn sphere_rendering_and_error_recovery() {
    let mut renderer = Renderer::new().expect("a working graphics adapter is required");
    eprintln!("Testing adapter: {:?}", renderer.adapter_info());
    let pipeline = sdf_view::ScenePipeline::new(
        renderer.device(),
        include_str!("../examples/sphere.glsl"),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
    .unwrap();

    for object_color in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0; 3]] {
        let pixels = renderer
            .render_with_pipeline(
                &pipeline,
                RenderOptions {
                    width: 1,
                    height: 1,
                    object_color,
                    light: sdf_view::DirectionalLight {
                        intensity: 0.0,
                        ambient: 1.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(&pixels[..3], &object_color.map(|v| (v * 255.0) as u8));
        assert_eq!(pixels[3], 255);
    }

    for options in [
        RenderOptions {
            width: 0,
            height: 32,
            ..Default::default()
        },
        RenderOptions {
            width: 32,
            height: 0,
            ..Default::default()
        },
        RenderOptions {
            width: u32::MAX,
            height: 32,
            ..Default::default()
        },
        RenderOptions {
            width: 32,
            height: u32::MAX,
            ..Default::default()
        },
    ] {
        if options.width == 0 || options.height == 0 || options.width == u32::MAX {
            std::assert_matches!(
                renderer.render(include_str!("../examples/sphere.glsl"), options),
                Err(Error::Settings(_))
            );
            std::assert_matches!(
                renderer.render_with_pipeline(&pipeline, options),
                Err(Error::Settings(_))
            );
        } else {
            std::assert_matches!(
                renderer.render(include_str!("../examples/sphere.glsl"), options),
                Err(Error::Gpu(_))
            );
            std::assert_matches!(
                renderer.render_with_pipeline(&pipeline, options),
                Err(Error::Gpu(_))
            );
        }
    }
    for source in [
        "float sdf(vec3 p) { return invalid_symbol; }",
        "float another_function(vec3 p) { return length(p); }",
        "float sdf(vec3 p) { broken syntax }",
        "layout(set = 0, binding = 0) uniform Params { float radius; };\nfloat sdf(vec3 p) { return length(p) - radius; }",
    ] {
        std::assert_matches!(
            renderer.render(
                source,
                RenderOptions {
                    width: 32,
                    height: 32,
                    ..Default::default()
                }
            ),
            Err(Error::Gpu(_))
        );
    }

    // Non-square images exercise aspect ratio; odd widths exercise row padding.
    for (width, height) in [(129, 97), (97, 129), (128, 96), (1, 1)] {
        let image = render(
            &mut renderer,
            include_str!("../examples/sphere.glsl"),
            RenderOptions {
                width,
                height,
                ..Default::default()
            },
        );
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!(image.pixels.len(), (width * height * 4) as usize);
        let center = ((height / 2 * width + width / 2) * 4) as usize;
        assert_eq!(image.pixels[center + 3], 255);
        assert!(image.pixels[center..center + 3].iter().all(|&c| c > 0));

        // Analytic silhouette: a unit sphere viewed from distance 3 projects to
        // radius h / (2 * tan(fov / 2) * sqrt(3^2 - 1)). Allow one edge pixel for
        // the ray-marching tolerance and floating point differences.
        let radius =
            f64::from(height) / (2.0 * (std::f64::consts::PI / 8.0).tan() * 8.0_f64.sqrt());
        for y in 0..height {
            for x in 0..width {
                let dx = f64::from(x) + 0.5 - f64::from(width) / 2.0;
                let dy = f64::from(y) + 0.5 - f64::from(height) / 2.0;
                let distance = dx.hypot(dy);
                let pixel = ((y * width + x) * 4) as usize;
                if distance < radius - 1.0 {
                    assert_eq!(image.pixels[pixel + 3], 255, "hole at {x}, {y}");
                } else if distance > radius + 1.0 {
                    assert_eq!(
                        &image.pixels[pixel..pixel + 4],
                        &[0, 0, 0, 0],
                        "unexpected hit at {x}, {y}"
                    );
                }
            }
        }
        if width > 1 {
            let sample = |x, y| image.pixels[((y * width + x) * 4) as usize];
            assert!(
                sample(width / 2, height / 3) > sample(width / 2, height * 2 / 3),
                "the top should face the light"
            );
        }
    }

    let empty = renderer
        .render(
            "float sdf(vec3 p) { return 1.0; }",
            RenderOptions {
                width: 17,
                height: 13,
                ..Default::default()
            },
        )
        .unwrap();
    drop(renderer);
    assert_eq!(empty.len(), 256 * 13);
    assert!(
        empty
            .as_chunks::<256>()
            .0
            .iter()
            .all(|row| row[..17 * 4].iter().all(|&value| value == 0))
    );
}
