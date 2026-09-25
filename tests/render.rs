use sdf_view::{Error, RenderOptions, Renderer};

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
        assert!(matches!(
            renderer.render(include_str!("../examples/sphere.glsl"), options),
            Err(Error::Dimensions(_))
        ));
    }
    for source in [
        "float sdf(vec3 p) { return invalid_symbol; }",
        "float another_function(vec3 p) { return length(p); }",
        "float sdf(vec3 p) { broken syntax }",
        "layout(set = 0, binding = 0) uniform Params { float radius; };\nfloat sdf(vec3 p) { return length(p) - radius; }",
    ] {
        assert!(matches!(
            renderer.render(
                source,
                RenderOptions {
                    width: 32,
                    height: 32,
                    ..Default::default()
                }
            ),
            Err(Error::Gpu(_))
        ));
    }

    // Non-square images exercise aspect ratio; odd widths exercise row padding.
    for (width, height) in [(129, 97), (97, 129), (128, 96), (1, 1)] {
        let image = renderer
            .render(
                include_str!("../examples/sphere.glsl"),
                RenderOptions {
                    width,
                    height,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
        assert_eq!(image.pixels().len(), (width * height * 4) as usize);
        let center = ((height / 2 * width + width / 2) * 4) as usize;
        assert_eq!(image.pixels()[center + 3], 255);
        assert!(image.pixels()[center..center + 3].iter().all(|&c| c > 0));

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
                    assert_eq!(image.pixels()[pixel + 3], 255, "hole at {x}, {y}");
                } else if distance > radius + 1.0 {
                    assert_eq!(
                        &image.pixels()[pixel..pixel + 4],
                        &[0, 0, 0, 0],
                        "unexpected hit at {x}, {y}"
                    );
                }
            }
        }
        if width > 1 {
            let sample = |x, y| image.pixels()[((y * width + x) * 4) as usize];
            assert!(
                sample(width / 2, height / 3) > sample(width / 2, height * 2 / 3),
                "the top should face the light"
            );
        }
        assert_eq!(image.rows().len(), height as usize);
        for (row, packed) in image
            .rows()
            .zip(image.pixels().chunks_exact(width as usize * 4))
        {
            assert_eq!(row, packed);
        }
        if width % 64 == 0 {
            assert_eq!(
                image.rows().next().unwrap().as_ptr(),
                image.pixels().as_ptr()
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
    assert!(empty.pixels().iter().all(|&value| value == 0));
}
