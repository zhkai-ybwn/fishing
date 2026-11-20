//! Local CPU face detection and identification with OpenCV Zoo model weights.

mod photo;
mod pixels;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use pixels::Image;
use std::path::PathBuf;

pub use photo::enroll_photo;

pub const EMBEDDING_MODEL: &str = "sface-2021dec-fp32-v1";
pub const EMBEDDING_LEN: usize = 128;

const YUNET: &[u8] = include_bytes!("../models/face_detection_yunet_2026may.onnx");
const SFACE: &[u8] = include_bytes!("../models/face_recognition_sface_2021dec.onnx");
const MAX_DETECTION_EDGE: usize = 640;
const FACE_EDGE: usize = 112;
const STRIDES: [usize; 3] = [8, 16, 32];
const DETECTION_THRESHOLD: f32 = 0.8;
const OVERLAP_THRESHOLD: f32 = 0.3;
const MAX_NMS_CANDIDATES: usize = 512;

#[derive(Debug)]
pub struct Observation {
    pub face_count: usize,
    pub embeddings: Vec<Vec<f32>>,
}

pub struct Vision {
    detector: Session,
    recognizer: Option<Session>,
}

impl Vision {
    pub fn new() -> Result<Self, String> {
        initialize_runtime()?;
        Ok(Self {
            detector: new_session(YUNET).map_err(|error| format!("YuNet: {error}"))?,
            recognizer: None,
        })
    }

    /// RGB pixels must be tightly packed in row order. Embeddings are unit-length.
    pub fn analyze(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        identify: bool,
    ) -> Result<Observation, String> {
        let image = Image::new(rgb, width as usize, height as usize)?;
        let faces = detect(&mut self.detector, &image)?;
        let embeddings = if identify {
            self.recognize(&image, &faces)?
        } else {
            Vec::new()
        };
        Ok(Observation {
            face_count: faces.len(),
            embeddings,
        })
    }

    fn recognize(&mut self, image: &Image<'_>, faces: &[Face]) -> Result<Vec<Vec<f32>>, String> {
        if faces.is_empty() {
            return Ok(Vec::new());
        }
        if self.recognizer.is_none() {
            self.recognizer = Some(new_session(SFACE).map_err(|error| format!("SFace: {error}"))?);
        }
        let recognizer = self
            .recognizer
            .as_mut()
            .expect("the recognizer was initialized");
        faces
            .iter()
            .filter_map(|face| image.aligned_face(&face.landmarks))
            .map(|aligned| embed(recognizer, aligned))
            .collect()
    }
}

fn initialize_runtime() -> Result<(), String> {
    // Resolve the installed library explicitly so discovery never depends on the working directory.
    let library = if let Some(path) = std::env::var_os("ORT_DYLIB_PATH") {
        PathBuf::from(path)
    } else {
        let name = if cfg!(target_os = "windows") {
            "onnxruntime.dll"
        } else if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else {
            "libonnxruntime.so"
        };
        std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name(name)
    };
    if !library.is_file() {
        return Err(format!(
            "找不到 ONNX Runtime（{}）。从源码构建时请先运行 .\\packaging\\Get-OnnxRuntime.ps1，再重新构建；安装版请重新安装。",
            library.display()
        ));
    }
    let path = std::path::absolute(&library).map_err(|error| error.to_string())?;
    let path = path
        .to_str()
        .ok_or("ONNX Runtime path is not valid Unicode")?;
    // The dynamic loader in ort rc.10 reports an invalid or incompatible DLL by panicking.
    std::panic::catch_unwind(|| ort::init_from(path).commit())
        .map_err(|_| format!("Could not load ONNX Runtime from {path}"))?
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn new_session(model: &[u8]) -> ort::Result<Session> {
    Session::builder()?
        .with_intra_threads(2)?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_log_level(ort::logging::LogLevel::Error)?
        .commit_from_memory(model)
}

#[derive(Debug, Clone, Copy)]
struct Face {
    bounds: [f32; 4],
    landmarks: [[f32; 2]; 5],
    score: f32,
}

fn detect(detector: &mut Session, image: &Image<'_>) -> Result<Vec<Face>, String> {
    let (values, width, height, scaled_width, scaled_height) = image.detector_input();
    let input =
        Tensor::from_array(([1, 3, height, width], values)).map_err(|error| error.to_string())?;
    let outputs = detector
        .run(ort::inputs![input])
        .map_err(|error| format!("YuNet: {error}"))?;

    let mut candidates = Vec::new();
    for stride in STRIDES {
        decode_grid(
            &outputs,
            stride,
            width,
            height,
            scaled_width,
            scaled_height,
            &mut candidates,
        )?;
    }
    candidates.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
    let mut faces: Vec<Face> = Vec::new();
    for candidate in candidates.into_iter().take(MAX_NMS_CANDIDATES) {
        if faces
            .iter()
            .any(|face| overlap(face.bounds, candidate.bounds) > OVERLAP_THRESHOLD)
        {
            continue;
        }
        faces.push(candidate);
        // Keep bounds in detector coordinates until NMS is complete.
    }
    for face in &mut faces {
        for point in &mut face.landmarks {
            point[0] *= image.width as f32 / scaled_width as f32;
            point[1] *= image.height as f32 / scaled_height as f32;
        }
    }
    Ok(faces)
}

fn decode_grid(
    outputs: &SessionOutputs<'_>,
    stride: usize,
    width: usize,
    height: usize,
    scaled_width: usize,
    scaled_height: usize,
    candidates: &mut Vec<Face>,
) -> Result<(), String> {
    let cols = width / stride;
    let cells = cols * (height / stride);
    let classes = get_tensor(outputs, &format!("cls_{stride}"), cells)?;
    let objects = get_tensor(outputs, &format!("obj_{stride}"), cells)?;
    let boxes = get_tensor(outputs, &format!("bbox_{stride}"), cells * 4)?;
    let points = get_tensor(outputs, &format!("kps_{stride}"), cells * 10)?;

    for cell in 0..cells {
        let score = (classes[cell].clamp(0.0, 1.0) * objects[cell].clamp(0.0, 1.0)).sqrt();
        if !score.is_finite() || score < DETECTION_THRESHOLD {
            continue;
        }
        let col = (cell % cols) as f32;
        let row = (cell / cols) as f32;
        let step = stride as f32;
        let center_x = (col + boxes[cell * 4]) * step;
        let center_y = (row + boxes[cell * 4 + 1]) * step;
        let half_width = boxes[cell * 4 + 2].exp() * step * 0.5;
        let half_height = boxes[cell * 4 + 3].exp() * step * 0.5;
        if !(0.0..scaled_width as f32).contains(&center_x)
            || !(0.0..scaled_height as f32).contains(&center_y)
            || !half_width.is_finite()
            || !half_height.is_finite()
            || half_width < 2.0
            || half_height < 2.0
        {
            continue;
        }
        let bounds = [
            (center_x - half_width).max(0.0),
            (center_y - half_height).max(0.0),
            (center_x + half_width).min(scaled_width as f32),
            (center_y + half_height).min(scaled_height as f32),
        ];
        let landmarks = std::array::from_fn(|point| {
            let offset = cell * 10 + point * 2;
            [
                (col + points[offset]) * step,
                (row + points[offset + 1]) * step,
            ]
        });
        candidates.push(Face {
            bounds,
            landmarks,
            score,
        });
    }
    Ok(())
}

fn get_tensor<'a>(
    outputs: &'a SessionOutputs<'_>,
    name: &str,
    len: usize,
) -> Result<&'a [f32], String> {
    let (_, data) = outputs
        .get(name)
        .ok_or_else(|| format!("YuNet output missing: {name}"))?
        .try_extract_tensor::<f32>()
        .map_err(|error| error.to_string())?;
    if data.len() != len {
        return Err(format!("Unexpected YuNet shape for {name}"));
    }
    Ok(data)
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let intersection =
        (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let a_area = (a[2] - a[0]) * (a[3] - a[1]);
    let b_area = (b[2] - b[0]) * (b[3] - b[1]);
    intersection / (a_area + b_area - intersection)
}

fn embed(recognizer: &mut Session, aligned: Vec<f32>) -> Result<Vec<f32>, String> {
    let input = Tensor::from_array(([1, 3, FACE_EDGE, FACE_EDGE], aligned))
        .map_err(|error| error.to_string())?;
    let outputs = recognizer
        .run(ort::inputs![input])
        .map_err(|error| format!("SFace: {error}"))?;
    let (_, value) = outputs.iter().next().ok_or("SFace has no output tensor")?;
    let (_, embedding) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| error.to_string())?;
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if embedding.len() != EMBEDDING_LEN || !norm.is_finite() || norm <= f32::EPSILON {
        return Err("SFace produced an invalid embedding".to_owned());
    }
    Ok(embedding.iter().map(|value| value / norm).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_models_accept_dynamic_sizes_and_produce_unit_embeddings() {
        let mut vision = Vision::new().expect("set ORT_DYLIB_PATH to the packaged runtime");
        for (width, height) in [(320, 239), (100, 200)] {
            let frame = vec![128; (width * height * 3) as usize];
            let result = vision.analyze(&frame, width, height, false).unwrap();
            assert_eq!(result.face_count, 0);
            assert!(result.embeddings.is_empty());
        }
        assert!(vision.recognizer.is_none());

        let mut recognizer = new_session(SFACE).unwrap();
        let pixels = (0..FACE_EDGE * FACE_EDGE * 3)
            .map(|index| (index % 255) as f32)
            .collect();
        let vector = embed(&mut recognizer, pixels).unwrap();
        assert_eq!(vector.len(), EMBEDDING_LEN);
        let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5);
    }

    #[test]
    fn portrait_is_recognized_in_a_frame_with_two_faces() {
        const EDGE: usize = 160;
        let portrait = include_bytes!("../tests/data/portrait.rgb");
        let mut vision = Vision::new().unwrap();
        let single = vision
            .analyze(portrait, EDGE as u32, EDGE as u32, true)
            .unwrap();
        assert_eq!(single.face_count, 1);
        assert_eq!(single.embeddings.len(), 1);

        let mut side_by_side = Vec::with_capacity(portrait.len() * 2);
        for row in portrait.chunks_exact(EDGE * 3) {
            side_by_side.extend_from_slice(row);
            side_by_side.extend_from_slice(row);
        }
        let double = vision
            .analyze(&side_by_side, (EDGE * 2) as u32, EDGE as u32, true)
            .unwrap();
        assert_eq!(double.face_count, 2);
        assert_eq!(double.embeddings.len(), 2);
        for embedding in double.embeddings {
            let similarity = embedding
                .iter()
                .zip(&single.embeddings[0])
                .map(|(left, right)| left * right)
                .sum::<f32>();
            assert!(
                similarity > 0.85,
                "unexpected same-face similarity: {similarity}"
            );
        }
    }
}
