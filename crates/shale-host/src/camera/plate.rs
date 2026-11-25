//! Whether the plate in front of the camera matches a picture of it empty.
//!
//! Both images are reduced to a 32×32 grey grid and compared by the mean
//! absolute difference. A part on the plate moves that number. A little
//! noise, or a change of light, does not. There is no model of the part:
//! the reference is a picture the operator took of the empty plate.

use image::{imageops::FilterType, DynamicImage, RgbImage};

#[derive(Clone, Debug, PartialEq)]
pub struct PlateVerdict {
    pub clear: bool,
    /// Mean absolute difference of the grey grids, 0 to 255.
    pub difference: f64,
    pub note: String,
}

/// `max_difference` is the largest mean change that still counts as the same
/// plate. 12 is enough for sensor noise and not enough for a part.
pub fn plates_match(reference: &RgbImage, current: &RgbImage, max_difference: f64) -> PlateVerdict {
    let difference = mean_abs(reference, current);
    if difference <= max_difference {
        PlateVerdict {
            clear: true,
            difference,
            note: "The plate matches the empty-plate picture.".into(),
        }
    } else {
        PlateVerdict {
            clear: false,
            difference,
            note: format!(
                "The plate does not match the empty-plate picture (difference {difference:.0}). \
                 The queue has stopped so the next job is not printed into a part that is still there."
            ),
        }
    }
}

fn mean_abs(reference: &RgbImage, current: &RgbImage) -> f64 {
    let left = down(reference);
    let right = down(current);
    let mut sum = 0.0;
    let mut n = 0.0;
    for (a, b) in left.pixels().zip(right.pixels()) {
        let la = luma(a[0], a[1], a[2]);
        let lb = luma(b[0], b[1], b[2]);
        sum += (la - lb).abs();
        n += 1.0;
    }
    if n == 0.0 {
        255.0
    } else {
        sum / n
    }
}

fn down(image: &RgbImage) -> RgbImage {
    DynamicImage::ImageRgb8(image.clone())
        .resize_exact(32, 32, FilterType::Nearest)
        .to_rgb8()
}

fn luma(r: u8, g: u8, b: u8) -> f64 {
    0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
}

/// Decode a JPEG into the image the plate check compares. A file that is not
/// a JPEG is an error the caller can show.
pub fn decode_jpeg(bytes: &[u8]) -> Result<RgbImage, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| format!("that picture could not be read ({err})"))?;
    Ok(image.to_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn plate() -> RgbImage {
        RgbImage::from_pixel(80, 60, Rgb([70, 72, 74]))
    }

    fn with_part() -> RgbImage {
        let mut image = plate();
        for y in 8..52 {
            for x in 10..70 {
                image.put_pixel(x, y, Rgb([210, 180, 40]));
            }
        }
        image
    }

    #[test]
    fn an_empty_plate_matches_itself_and_a_part_does_not() {
        let empty = plate();
        let same = plates_match(&empty, &plate(), 12.0);
        assert!(same.clear, "{}", same.note);
        assert!(same.difference < 1.0);

        let occupied = plates_match(&empty, &with_part(), 12.0);
        assert!(!occupied.clear, "a part must not count as an empty plate");
        assert!(occupied.difference > 18.0);
        assert!(occupied.note.contains("stopped"));
    }

    #[test]
    fn a_little_noise_is_still_the_empty_plate() {
        let reference = plate();
        let mut noisy = plate();
        for (index, pixel) in noisy.pixels_mut().enumerate() {
            if index % 7 == 0 {
                pixel[0] = pixel[0].saturating_add(6);
            }
        }
        let verdict = plates_match(&reference, &noisy, 12.0);
        assert!(verdict.clear, "difference {}", verdict.difference);
    }
}
