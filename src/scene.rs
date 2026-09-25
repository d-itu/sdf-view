use crate::Error;

/// A perspective camera in right-handed world coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    /// Approximate up direction, orthogonalized against the viewing direction.
    pub up: [f32; 3],
    /// Vertical field of view in degrees, strictly between 0 and 180.
    pub vertical_fov_degrees: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 3.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        }
    }
}

pub(crate) struct CameraBasis {
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
}

impl Camera {
    pub(crate) fn basis(self) -> Result<CameraBasis, Error> {
        if !self
            .position
            .iter()
            .chain(&self.target)
            .chain(&self.up)
            .all(|v| v.is_finite())
        {
            return Err(Error::Settings("camera vectors must be finite"));
        }
        if !self.vertical_fov_degrees.is_finite()
            || self.vertical_fov_degrees <= 0.0
            || self.vertical_fov_degrees >= 180.0
            || (self.vertical_fov_degrees.to_radians() * 0.5).tan() <= 0.0
        {
            return Err(Error::Settings(
                "vertical FOV must be finite and strictly between 0 and 180 degrees",
            ));
        }
        // Calculate differences in f64 to avoid overflow for finite f32 inputs.
        let forward = normalize(std::array::from_fn(|i| {
            f64::from(self.target[i]) - f64::from(self.position[i])
        }))
        .ok_or(Error::Settings("camera position and target must differ"))?;
        let up = normalize(self.up.map(f64::from))
            .ok_or(Error::Settings("camera up must be nonzero"))?;
        let right = cross(forward, up);
        if right.iter().map(|v| v * v).sum::<f64>() < 1e-12 {
            return Err(Error::Settings(
                "camera up must not be parallel to the viewing direction",
            ));
        }
        let right = normalize(right).unwrap();
        Ok(CameraBasis {
            forward: forward.map(|v| v as f32),
            right: right.map(|v| v as f32),
            up: cross(right, forward).map(|v| v as f32),
        })
    }
}

/// A world-space directional light and a white ambient contribution.
#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    /// Direction from the surface toward the light; need not be normalized.
    pub direction: [f32; 3],
    /// Linear RGB color; each component must be in [0, 1].
    pub color: [f32; 3],
    /// Nonnegative diffuse strength. Values above 1 can saturate the PNG output.
    pub intensity: f32,
    /// Nonnegative white ambient strength, independent of the directional light.
    pub ambient: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: [-0.5, 0.8, 1.0],
            color: [1.0; 3],
            intensity: 0.85,
            ambient: 0.15,
        }
    }
}

impl DirectionalLight {
    pub(crate) fn normalized_direction(self) -> Result<[f32; 3], Error> {
        if !self.direction.iter().all(|v| v.is_finite()) {
            return Err(Error::Settings("light direction must be finite"));
        }
        if !self
            .color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err(Error::Settings(
                "light color components must be finite and between 0 and 1",
            ));
        }
        if !self.intensity.is_finite()
            || self.intensity < 0.0
            || !self.ambient.is_finite()
            || self.ambient < 0.0
            || !(self.intensity + self.ambient).is_finite()
        {
            return Err(Error::Settings(
                "light strengths must be finite and nonnegative, with a finite sum",
            ));
        }
        normalize(self.direction.map(f64::from))
            .map(|v| v.map(|c| c as f32))
            .ok_or(Error::Settings("light direction must be nonzero"))
    }
}

fn normalize(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
    (length > 0.0).then(|| v.map(|c| c / length))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
