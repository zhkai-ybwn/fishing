use super::{FACE_EDGE, MAX_DETECTION_EDGE};

// Five-point alignment template from OpenCV's SFace model documentation.
const FACE_TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

pub(crate) struct Image<'a> {
    rgb: &'a [u8],
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl<'a> Image<'a> {
    pub(crate) fn new(rgb: &'a [u8], width: usize, height: usize) -> Result<Self, String> {
        let byte_count = width
            .checked_mul(height)
            .and_then(|size| size.checked_mul(3));
        if width == 0 || height == 0 || byte_count != Some(rgb.len()) {
            return Err("Camera frame dimensions do not match its RGB data".to_owned());
        }
        Ok(Self { rgb, width, height })
    }

    pub(crate) fn detector_input(&self) -> (Vec<f32>, usize, usize, usize, usize) {
        let edge = self.width.max(self.height);
        let (scaled_width, scaled_height) = if edge <= MAX_DETECTION_EDGE {
            (self.width, self.height)
        } else {
            let ratio = MAX_DETECTION_EDGE as f32 / edge as f32;
            (
                (self.width as f32 * ratio).round().max(1.0) as usize,
                (self.height as f32 * ratio).round().max(1.0) as usize,
            )
        };
        // YuNet's output grids use strides of 8, 16 and 32. Pad on the right and bottom.
        let width = scaled_width.next_multiple_of(32);
        let height = scaled_height.next_multiple_of(32);
        let plane = width * height;
        let mut input = vec![0.0; plane * 3];
        let x_scale = self.width as f32 / scaled_width as f32;
        let y_scale = self.height as f32 / scaled_height as f32;
        for y in 0..scaled_height {
            for x in 0..scaled_width {
                let color = if edge <= MAX_DETECTION_EDGE {
                    self.pixel(x as isize, y as isize)
                } else {
                    let source_x = (x as f32 + 0.5) * x_scale - 0.5;
                    let source_y = (y as f32 + 0.5) * y_scale - 0.5;
                    self.bilinear(source_x, source_y)
                };
                let index = y * width + x;
                input[index] = color[2];
                input[plane + index] = color[1];
                input[2 * plane + index] = color[0];
            }
        }
        (input, width, height, scaled_width, scaled_height)
    }

    pub(crate) fn aligned_face(&self, landmarks: &[[f32; 2]; 5]) -> Option<Vec<f32>> {
        let transform = Alignment::fit(landmarks)?;
        let plane = FACE_EDGE * FACE_EDGE;
        let mut input = vec![0.0; plane * 3];
        for y in 0..FACE_EDGE {
            for x in 0..FACE_EDGE {
                let [source_x, source_y] = transform.source_at(x as f32, y as f32);
                let color = self.bilinear(source_x, source_y);
                let index = y * FACE_EDGE + x;
                for channel in 0..3 {
                    input[channel * plane + index] = color[channel];
                }
            }
        }
        Some(input)
    }

    fn pixel(&self, x: isize, y: isize) -> [f32; 3] {
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return [0.0; 3];
        }
        let index = (y * self.width + x) * 3;
        [
            self.rgb[index] as f32,
            self.rgb[index + 1] as f32,
            self.rgb[index + 2] as f32,
        ]
    }

    fn bilinear(&self, x: f32, y: f32) -> [f32; 3] {
        if !(-1.0..self.width as f32).contains(&x) || !(-1.0..self.height as f32).contains(&y) {
            return [0.0; 3];
        }
        let left = x.floor() as isize;
        let top = y.floor() as isize;
        let horizontal = x - left as f32;
        let vertical = y - top as f32;
        let top_left = self.pixel(left, top);
        let top_right = self.pixel(left + 1, top);
        let bottom_left = self.pixel(left, top + 1);
        let bottom_right = self.pixel(left + 1, top + 1);
        std::array::from_fn(|channel| {
            let upper = top_left[channel] * (1.0 - horizontal) + top_right[channel] * horizontal;
            let lower =
                bottom_left[channel] * (1.0 - horizontal) + bottom_right[channel] * horizontal;
            upper * (1.0 - vertical) + lower * vertical
        })
    }
}

struct Alignment {
    a: f32,
    b: f32,
    tx: f32,
    ty: f32,
    norm: f32,
}

impl Alignment {
    fn fit(source: &[[f32; 2]; 5]) -> Option<Self> {
        let source_center = center(source);
        let target_center = center(&FACE_TEMPLATE);
        let mut denominator = 0.0;
        let mut dot = 0.0;
        let mut cross = 0.0;
        // Fit rotation, translation and uniform scale from the detected landmarks to the template.
        for ([source_x, source_y], [target_x, target_y]) in source.iter().zip(FACE_TEMPLATE) {
            let sx = source_x - source_center[0];
            let sy = source_y - source_center[1];
            let tx = target_x - target_center[0];
            let ty = target_y - target_center[1];
            denominator += sx * sx + sy * sy;
            dot += sx * tx + sy * ty;
            cross += sx * ty - sy * tx;
        }
        if !denominator.is_finite() || denominator <= f32::EPSILON {
            return None;
        }
        let a = dot / denominator;
        let b = cross / denominator;
        let norm = a * a + b * b;
        if !norm.is_finite() || norm <= f32::EPSILON {
            return None;
        }
        Some(Self {
            a,
            b,
            tx: target_center[0] - a * source_center[0] + b * source_center[1],
            ty: target_center[1] - b * source_center[0] - a * source_center[1],
            norm,
        })
    }

    fn source_at(&self, x: f32, y: f32) -> [f32; 2] {
        let dx = x - self.tx;
        let dy = y - self.ty;
        [
            (self.a * dx + self.b * dy) / self.norm,
            (-self.b * dx + self.a * dy) / self.norm,
        ]
    }
}

fn center(points: &[[f32; 2]; 5]) -> [f32; 2] {
    [
        points.iter().map(|point| point[0]).sum::<f32>() / 5.0,
        points.iter().map(|point| point[1]).sum::<f32>() / 5.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_inverts_translation_rotation_and_uniform_scale() {
        let landmarks = FACE_TEMPLATE.map(|[x, y]| [200.0 - 1.5 * y, 23.0 + 1.5 * x]);
        let alignment = Alignment::fit(&landmarks).unwrap();
        for ([x, y], [expected_x, expected_y]) in FACE_TEMPLATE.into_iter().zip(landmarks) {
            let [actual_x, actual_y] = alignment.source_at(x, y);
            assert!((actual_x - expected_x).abs() < 1e-4);
            assert!((actual_y - expected_y).abs() < 1e-4);
        }
    }
}
