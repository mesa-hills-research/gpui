use image::{Rgba, RgbaImage};

/// How far an image may stray from its golden and still match. The default is an exact match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tolerance {
    /// The largest difference allowed in any channel of a pixel, from 0 to 255.
    pub channel: u8,
    /// How many pixels may differ by more than `channel`.
    pub pixels: usize,
}

impl Tolerance {
    pub const EXACT: Self = Self {
        channel: 0,
        pixels: 0,
    };

    /// Allows every channel of every pixel to differ by up to `channel`.
    pub const fn channel(channel: u8) -> Self {
        Self { channel, pixels: 0 }
    }

    /// Also allows `pixels` pixels to differ by more than the channel tolerance.
    pub const fn pixels(self, pixels: usize) -> Self {
        Self { pixels, ..self }
    }
}

/// How an image differs from its golden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    pub expected_size: (u32, u32),
    pub actual_size: (u32, u32),
    /// Pixels with any channel differing at all.
    pub changed: usize,
    /// Pixels with a channel differing by more than the tolerance's `channel`.
    pub over: usize,
    /// The largest difference in any channel of any pixel.
    pub max_channel: u8,
}

impl Difference {
    pub fn same_size(&self) -> bool {
        self.expected_size == self.actual_size
    }

    /// Whether the images match within `tolerance`, the one they were compared with.
    pub fn within(&self, tolerance: Tolerance) -> bool {
        self.same_size() && self.over <= tolerance.pixels
    }
}

/// Compares `actual` with `expected` pixel by pixel. Images of different sizes count every pixel
/// as changed.
pub fn compare(expected: &RgbaImage, actual: &RgbaImage, tolerance: Tolerance) -> Difference {
    let mut difference = Difference {
        expected_size: expected.dimensions(),
        actual_size: actual.dimensions(),
        changed: 0,
        over: 0,
        max_channel: 0,
    };
    if !difference.same_size() {
        let (width, height) = actual.dimensions();
        difference.changed = width as usize * height as usize;
        difference.over = difference.changed;
        difference.max_channel = u8::MAX;
        return difference;
    }
    for (expected, actual) in expected.pixels().zip(actual.pixels()) {
        let delta = channel_delta(expected, actual);
        if delta > 0 {
            difference.changed += 1;
            difference.max_channel = difference.max_channel.max(delta);
        }
        if delta > tolerance.channel {
            difference.over += 1;
        }
    }
    difference
}

/// An image that shows where `actual` differs from `expected`: pixels over the tolerance in red,
/// pixels within it in amber, and the rest of `expected` faded, for context. `None` when the
/// sizes differ.
pub fn diff_image(
    expected: &RgbaImage,
    actual: &RgbaImage,
    tolerance: Tolerance,
) -> Option<RgbaImage> {
    if expected.dimensions() != actual.dimensions() {
        return None;
    }
    let (width, height) = expected.dimensions();
    Some(RgbaImage::from_fn(width, height, |x, y| {
        let (before, after) = (expected.get_pixel(x, y), actual.get_pixel(x, y));
        match channel_delta(before, after) {
            0 => {
                let [r, g, b, _] = before.0;
                let luma = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
                let faded = 255 - (255 - luma) * 3 / 10;
                Rgba([faded as u8, faded as u8, faded as u8, 255])
            }
            delta if delta <= tolerance.channel => Rgba([255, 176, 0, 255]),
            _ => Rgba([230, 0, 0, 255]),
        }
    }))
}

fn channel_delta(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    a.0.iter()
        .zip(b.0.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixels: &[[u8; 4]]) -> RgbaImage {
        RgbaImage::from_fn(pixels.len() as u32, 1, |x, _| Rgba(pixels[x as usize]))
    }

    #[test]
    fn identical_images_match_exactly() {
        let a = image(&[[1, 2, 3, 255], [4, 5, 6, 255]]);
        let difference = compare(&a, &a.clone(), Tolerance::EXACT);
        assert_eq!(difference.changed, 0);
        assert!(difference.within(Tolerance::EXACT));
    }

    #[test]
    fn channel_tolerance_absorbs_small_differences() {
        let a = image(&[[100, 100, 100, 255], [0, 0, 0, 255], [9, 9, 9, 255]]);
        let b = image(&[[103, 100, 99, 255], [0, 0, 0, 255], [9, 30, 9, 255]]);
        let difference = compare(&a, &b, Tolerance::channel(3));
        assert_eq!(difference.changed, 2);
        assert_eq!(difference.over, 1);
        assert_eq!(difference.max_channel, 21);
        assert!(!difference.within(Tolerance::channel(3)));
        assert!(difference.within(Tolerance::channel(3).pixels(1)));
    }

    #[test]
    fn different_sizes_never_match() {
        let a = image(&[[0, 0, 0, 255]]);
        let b = image(&[[0, 0, 0, 255], [0, 0, 0, 255]]);
        let difference = compare(&a, &b, Tolerance::channel(255).pixels(usize::MAX));
        assert!(!difference.within(Tolerance::channel(255).pixels(usize::MAX)));
        assert!(diff_image(&a, &b, Tolerance::EXACT).is_none());
    }

    #[test]
    fn diff_marks_pixels_by_tolerance() {
        let a = image(&[[255, 255, 255, 255], [10, 10, 10, 255], [10, 10, 10, 255]]);
        let b = image(&[[255, 255, 255, 255], [12, 10, 10, 255], [90, 10, 10, 255]]);
        let diff = diff_image(&a, &b, Tolerance::channel(2)).unwrap();
        assert_eq!(diff.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(diff.get_pixel(1, 0).0, [255, 176, 0, 255]);
        assert_eq!(diff.get_pixel(2, 0).0, [230, 0, 0, 255]);
    }
}
