/// Errors from initialization or rendering.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error(transparent)]
    Device(#[from] wgpu::RequestDeviceError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Gpu(#[from] wgpu::Error),
    #[error(transparent)]
    Poll(#[from] wgpu::PollError),
    #[error(transparent)]
    Map(#[from] wgpu::BufferAsyncError),
    #[error(transparent)]
    MapRange(#[from] wgpu::MapRangeError),
}

/// Invalid dimensions, camera settings, or lighting parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// Either image dimension is zero.
    #[error("width and height must be nonzero")]
    ZeroDimensions,
    /// The unpadded RGBA row size exceeds u32.
    #[error("row size overflow")]
    RowSizeOverflow,
    /// The aligned RGBA row size exceeds u32.
    #[error("padded row size overflow")]
    PaddedRowSizeOverflow,
    /// The readback buffer size exceeds usize.
    #[error("image exceeds the addressable readback size")]
    ReadbackSizeOverflow,
    /// Camera position, target, or up contains a nonfinite component.
    #[error("vectors must be finite")]
    NonFiniteFloat,
    /// The vertical field of view cannot define a valid perspective projection.
    #[error("vertical FOV must be finite and strictly between 0 and 180 degrees")]
    InvalidVerticalFov,
    /// Camera position and target coincide.
    #[error("camera position and target must differ")]
    CoincidentCameraPositionAndTarget,
    /// The camera up vector is zero.
    #[error("camera up must be nonzero")]
    ZeroCameraUp,
    /// Camera up is parallel or too close to parallel to the viewing direction.
    #[error("camera up must not be parallel to the viewing direction")]
    ParallelCameraUp,
    /// Light strengths are negative, nonfinite, or have a nonfinite sum.
    #[error("light strengths must be finite and nonnegative, with a finite sum")]
    InvalidLightStrength,
    /// The light direction vector is zero.
    #[error("light direction must be nonzero")]
    ZeroLightDirection,
}
