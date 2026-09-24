use sdf_view::{Camera, DirectionalLight, Error, Image, RenderOptions, Renderer};

#[test]
fn validates_scene_without_a_device() {
    let defaults = RenderOptions::default();
    defaults.validate().unwrap();
    let mut invalid = Vec::new();
    for fov in [0.0, -1.0, 180.0, 200.0, f32::NAN, f32::INFINITY] {
        invalid.push(RenderOptions {
            camera: Camera {
                vertical_fov_degrees: fov,
                ..defaults.camera
            },
            ..defaults
        });
    }
    for camera in [
        Camera {
            target: defaults.camera.position,
            ..defaults.camera
        },
        Camera {
            up: [0.0; 3],
            ..defaults.camera
        },
        Camera {
            up: [0.0, 0.0, 1.0],
            ..defaults.camera
        },
        Camera {
            position: [f32::INFINITY, 0.0, 3.0],
            ..defaults.camera
        },
        Camera {
            target: [f32::NAN, 0.0, 0.0],
            ..defaults.camera
        },
    ] {
        invalid.push(RenderOptions { camera, ..defaults });
    }
    for light in [
        DirectionalLight {
            direction: [0.0; 3],
            ..defaults.light
        },
        DirectionalLight {
            direction: [f32::NAN; 3],
            ..defaults.light
        },
        DirectionalLight {
            color: [-1.0, 1.0, 1.0],
            ..defaults.light
        },
        DirectionalLight {
            color: [2.0; 3],
            ..defaults.light
        },
        DirectionalLight {
            intensity: -1.0,
            ..defaults.light
        },
        DirectionalLight {
            ambient: f32::INFINITY,
            ..defaults.light
        },
    ] {
        invalid.push(RenderOptions { light, ..defaults });
    }
    for options in invalid {
        assert!(
            matches!(options.validate(), Err(Error::Settings(_))),
            "{options:?}"
        );
    }
}

fn coverage(image: &Image) -> usize {
    image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 255)
        .count()
}

fn centroid_y(image: &Image) -> f64 {
    let sum: usize = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, p)| p[3] == 255)
        .map(|(i, _)| i / image.width() as usize)
        .sum();
    sum as f64 / coverage(image) as f64
}

#[test]
fn camera_and_lighting_affect_rendering() {
    let mut renderer = Renderer::new().unwrap();
    let sphere = include_str!("../examples/sphere.glsl");
    let options = RenderOptions {
        width: 97,
        height: 97,
        ..Default::default()
    };
    let default_image = renderer.render(sphere, options).unwrap();
    let wide = renderer
        .render(
            sphere,
            RenderOptions {
                camera: Camera {
                    vertical_fov_degrees: 70.0,
                    ..options.camera
                },
                ..options
            },
        )
        .unwrap();
    assert!(coverage(&wide) < coverage(&default_image) / 2);

    // Translating both the scene and the camera preserves the projection.
    let moved = renderer
        .render(
            "float sdf(vec3 p) { return length(p - vec3(2.0, 0.0, 0.0)) - 1.0; }",
            RenderOptions {
                camera: Camera {
                    position: [2.0, 0.0, 3.0],
                    target: [2.0, 0.0, 0.0],
                    ..options.camera
                },
                ..options
            },
        )
        .unwrap();
    assert_eq!(coverage(&moved), coverage(&default_image));

    // View the same sphere from +X to exercise a non-default camera basis.
    let side = renderer
        .render(
            sphere,
            RenderOptions {
                camera: Camera {
                    position: [3.0, 0.0, 0.0],
                    ..options.camera
                },
                ..options
            },
        )
        .unwrap();
    assert_eq!(coverage(&side), coverage(&default_image));

    let raised_sphere = "float sdf(vec3 p) { return length(p - vec3(0.0, 0.4, 0.0)) - 0.4; }";
    let upright = renderer.render(raised_sphere, options).unwrap();
    let inverted = renderer
        .render(
            raised_sphere,
            RenderOptions {
                camera: Camera {
                    up: [0.0, -1.0, 0.0],
                    ..options.camera
                },
                ..options
            },
        )
        .unwrap();
    assert!(centroid_y(&upright) < 48.0);
    assert!(centroid_y(&inverted) > 48.0);

    let bottom_light = renderer
        .render(
            sphere,
            RenderOptions {
                light: DirectionalLight {
                    direction: [-0.5, -0.8, 1.0],
                    ..options.light
                },
                ..options
            },
        )
        .unwrap();
    let sample = |image: &Image, y: usize| image.pixels()[(y * 97 + 48) * 4];
    assert!(sample(&default_image, 32) > sample(&default_image, 64));
    assert!(sample(&bottom_light, 32) < sample(&bottom_light, 64));

    let red = renderer
        .render(
            sphere,
            RenderOptions {
                light: DirectionalLight {
                    color: [1.0, 0.0, 0.0],
                    ambient: 0.0,
                    ..options.light
                },
                ..options
            },
        )
        .unwrap();
    let center = (48 * 97 + 48) * 4;
    assert!(red.pixels()[center] > 0);
    assert_eq!(&red.pixels()[center + 1..center + 4], &[0, 0, 255]);
    let dark = renderer
        .render(
            sphere,
            RenderOptions {
                light: DirectionalLight {
                    intensity: 0.0,
                    ambient: 0.0,
                    ..options.light
                },
                ..options
            },
        )
        .unwrap();
    assert_eq!(&dark.pixels()[center..center + 4], &[0, 0, 0, 255]);
    let ambient = renderer
        .render(
            sphere,
            RenderOptions {
                light: DirectionalLight {
                    intensity: 0.0,
                    ambient: 0.3,
                    ..options.light
                },
                ..options
            },
        )
        .unwrap();
    let lit = &ambient.pixels()[center..center + 4];
    assert!(lit[..3].iter().all(|&c| c > 0));
    assert!(
        ambient
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .all(|p| p == lit)
    );
}
