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

// This is an actual graphics integration test. A missing adapter is a failure,
// not a silent skip; software Vulkan implementations can run it in CI.
#[test]
fn sphere_rendering_and_error_recovery() {
    let mut renderer = Renderer::new().expect("a working graphics adapter is required");
    eprintln!("Testing adapter: {:?}", renderer.adapter_info());

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
    ] {
        std::assert_matches!(
            renderer.render(include_str!("../examples/sphere.glsl"), options),
            Err(Error::Dimensions(_))
        );
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
