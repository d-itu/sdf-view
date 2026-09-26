use std::{io, path::PathBuf, process::ExitCode};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Render(#[from] sdf_view::Error),
    #[error("could not encode PNG '{path}': {source}")]
    Png {
        path: PathBuf,
        #[source]
        source: png::EncodingError,
    },
    #[error(transparent)]
    Settings(#[from] sdf_view::SettingsError),

    #[cfg(feature = "interactive")]
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),

    #[cfg(feature = "interactive")]
    #[error(transparent)]
    Os(#[from] winit::error::OsError),

    #[cfg(feature = "interactive")]
    #[error(transparent)]
    CreateSurface(#[from] wgpu::CreateSurfaceError),

    #[cfg(feature = "interactive")]
    #[error("surface validation failed")]
    SurfaceValidation,

    #[cfg(feature = "interactive")]
    #[error("surface has no supported configuration")]
    NoSupportedConfiguration,

    #[cfg(feature = "interactive")]
    #[error("surface has no sRGB format")]
    NoSupportedFormat,
}

impl Error {
    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Self::Settings(_) => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, Copy)]
#[error(
    "expected transparent, checkerboard, black, white, or rgb(r,g,b) with integer components in 0..=255"
)]
pub(crate) struct InvalidBackground;

#[derive(Debug, thiserror::Error, Clone, Copy)]
#[error("expected black, white, or rgb(r,g,b) with integer components in 0..=255")]
pub(crate) struct InvalidColor;
