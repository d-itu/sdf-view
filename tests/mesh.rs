#![cfg(feature = "gpu-test")]

use sdf_view::{
    Antialiasing, Background, Error, MeshError, MeshOptions, RenderOptions, Renderer, ScenePipeline,
};

#[test]
fn mesh_rasterization_and_recovery() {
    let renderer = Renderer::new().unwrap();
    let build = |source: &str| {
        ScenePipeline::new_mesh(
            renderer.device(),
            renderer.queue(),
            source,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            MeshOptions::default(),
        )
    };
    assert!(matches!(build("invalid shader"), Err(Error::Gpu(_))));
    assert!(matches!(
        build(
            "layout(set=1,binding=0) uniform Extra { float value; } extra; float sdf(vec3 p) { return 1.0; }"
        ),
        Err(Error::Mesh(MeshError::ResourceBindings))
    ));
    assert!(matches!(
        build("float sdf(vec3 p) { return uintBitsToFloat(0x7f800000u); }"),
        Err(Error::Mesh(MeshError::NonFiniteSamples))
    ));
    let pipeline = build("float sdf(vec3 p) { return length(p) - 1.0; }").unwrap();
    let options = RenderOptions {
        width: 64,
        height: 64,
        ..Default::default()
    };
    let image = renderer.render_with_pipeline(&pipeline, options).unwrap();
    let center = &image[32 * 256 + 32 * 4..][..4];
    assert_eq!(center[3], 255);
    assert!(center[2] > center[0]);
    assert_eq!(&image[..4], &[0, 0, 0, 0]);
    let traced = renderer
        .render("float sdf(vec3 p) { return length(p) - 1.0; }", options)
        .unwrap();
    for i in 0..3 {
        assert!(
            center[i].abs_diff(traced[32 * 256 + 32 * 4 + i]) <= 3,
            "depth test and outward normals must match the visible sphere surface"
        );
    }
    let aa = renderer
        .render_with_pipeline(
            &pipeline,
            RenderOptions {
                antialiasing: Antialiasing::X4,
                ..options
            },
        )
        .unwrap();
    assert!(aa.as_chunks::<4>().0.iter().any(|p| p[3] > 0 && p[3] < 255));
    let opaque = renderer
        .render_with_pipeline(
            &pipeline,
            RenderOptions {
                background: Background::Checkerboard,
                antialiasing: Antialiasing::X4,
                ..options
            },
        )
        .unwrap();
    assert!(opaque.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    let empty = build("float sdf(vec3 p) { return 1.0; }").unwrap();
    let image = renderer.render_with_pipeline(&empty, options).unwrap();
    assert!(image.iter().all(|&v| v == 0));
    let resized = renderer
        .render_with_pipeline(
            &pipeline,
            RenderOptions {
                width: 32,
                height: 16,
                ..options
            },
        )
        .unwrap();
    assert_eq!(resized.len(), 256 * 16);
}

#[test]
fn invalid_mesh_settings() {
    for options in [
        MeshOptions {
            resolution: 0,
            ..Default::default()
        },
        MeshOptions {
            resolution: 257,
            ..Default::default()
        },
        MeshOptions {
            min: [2.0; 3],
            ..Default::default()
        },
        MeshOptions {
            max: [f32::NAN; 3],
            ..Default::default()
        },
    ] {
        assert!(options.validate().is_err());
    }
}
