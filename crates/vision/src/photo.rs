use crate::{Vision, detect, pixels::Image};
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits, RgbImage};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_SIDE: u32 = 8192;
const MAX_PIXELS: u64 = 24_000_000;

/// Read one local JPEG or PNG portrait and return its face embedding.
pub fn enroll_photo(path: &Path) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|error| format!("无法打开照片：{error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取照片：{error}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("照片文件超过 25 MB，请换一张较小的照片。".to_owned());
    }
    enroll_decoded(&decode(&bytes)?)
}

fn decode(bytes: &[u8]) -> Result<RgbImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("无法读取照片：{error}"))?;
    if !matches!(reader.format(), Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        return Err("请选择可读取的 JPG、JPEG 或 PNG 照片。".to_owned());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("照片尺寸过大，请换一张较小的照片。".to_owned());
    }
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    Ok(image.into_rgb8())
}

fn decode_error(error: ImageError) -> String {
    match error {
        ImageError::Limits(_) => "照片尺寸过大，请换一张较小的照片。".to_owned(),
        _ => "无法解码这张照片，请换一张 JPG、JPEG 或 PNG 照片。".to_owned(),
    }
}

fn enroll_decoded(rgb: &RgbImage) -> Result<Vec<f32>, String> {
    let image = Image::new(rgb.as_raw(), rgb.width() as usize, rgb.height() as usize)?;
    let mut vision = Vision::new().map_err(|error| format!("无法启动照片识别：{error}"))?;
    let faces = detect(&mut vision.detector, &image)
        .map_err(|error| format!("无法检测照片人脸：{error}"))?;
    match faces.len() {
        0 => return Err("照片中未检测到人脸，请换一张清晰的正脸照。".to_owned()),
        1 => {}
        _ => return Err("照片中检测到多张人脸，请选择只有一个人的照片。".to_owned()),
    }
    vision
        .recognize(&image, &faces)
        .map_err(|error| format!("无法提取照片中的人脸特征：{error}"))?
        .pop()
        .ok_or_else(|| "无法提取这张脸的特征，请换一张清晰的正脸照。".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EMBEDDING_LEN;
    use image::{ImageEncoder, codecs::jpeg::JpegEncoder};

    #[test]
    fn respects_jpeg_exif_orientation() {
        let mut encoded = Vec::new();
        let mut encoder = JpegEncoder::new(&mut encoded);
        // Little-endian TIFF EXIF data with orientation 6 (rotate 90° clockwise).
        let exif = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\x01\x03\x00\x01\x00\x00\x00\x06\x00\x00\x00\x00\x00\x00\x00";
        encoder.set_exif_metadata(exif.to_vec()).unwrap();
        encoder.encode_image(&RgbImage::new(20, 32)).unwrap();
        assert_eq!(decode(&encoded).unwrap().dimensions(), (32, 20));
    }

    #[test]
    fn enrolls_jpeg_and_png_and_requires_exactly_one_face() {
        const EDGE: u32 = 160;
        let portrait = RgbImage::from_raw(
            EDGE,
            EDGE,
            include_bytes!("../tests/data/portrait.rgb").to_vec(),
        )
        .unwrap();
        for format in [ImageFormat::Jpeg, ImageFormat::Png] {
            let mut encoded = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(portrait.clone())
                .write_to(&mut encoded, format)
                .unwrap();
            let feature = enroll_decoded(&decode(&encoded.into_inner()).unwrap()).unwrap();
            assert_eq!(feature.len(), EMBEDDING_LEN);
        }
        assert!(decode(b"not a photograph").unwrap_err().contains("JPG"));
        assert!(
            enroll_decoded(&RgbImage::new(EDGE, EDGE))
                .unwrap_err()
                .contains("未检测到")
        );

        let double = RgbImage::from_fn(EDGE * 2, EDGE, |x, y| *portrait.get_pixel(x % EDGE, y));
        assert!(enroll_decoded(&double).unwrap_err().contains("多张人脸"));
    }
}
