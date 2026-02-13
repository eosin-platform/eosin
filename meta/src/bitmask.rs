//! Bitmask utilities for dense per-pixel annotation masks.
//!
//! Bitmask layout:
//! - Pixels are stored in **row-major order** (scanlines).
//! - Each pixel is a single bit: `1` means "inside / labeled", `0` means "outside / background".
//! - Each row is packed into bytes:
//!   - For a row of `width` pixels, the number of bytes is `row_stride = (width + 7) / 8`.
//!   - Total size in bytes = `row_stride * height`.
//! - Within each byte, bit 0 (LSB) corresponds to the left-most pixel in the group,
//!   bit 7 (MSB) corresponds to the right-most pixel.
//!
//! Example for width=10:
//!   Byte 0: bits 0-7 represent pixels 0-7 (pixel 0 at bit 0, pixel 7 at bit 7)
//!   Byte 1: bits 0-1 represent pixels 8-9 (pixel 8 at bit 0, pixel 9 at bit 1), bits 2-7 are padding

#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// A bitmask for dense per-pixel annotation.
#[derive(Debug, Clone)]
pub struct Bitmask {
    /// Width in pixels
    pub width: i32,
    /// Height in pixels  
    pub height: i32,
    /// Packed bitmask data (1 bit per pixel, row-major order)
    pub data: Vec<u8>,
}

impl Bitmask {
    /// Calculate the row stride (bytes per row) for a given width.
    /// This rounds up to the nearest byte.
    pub fn row_stride(width: i32) -> usize {
        ((width as usize) + 7) / 8
    }

    /// Calculate the expected data size in bytes for given dimensions.
    pub fn expected_size(width: i32, height: i32) -> usize {
        Self::row_stride(width) * (height as usize)
    }

    /// Create a new empty bitmask with all pixels set to 0 (background).
    pub fn new(width: i32, height: i32) -> Self {
        let size = Self::expected_size(width, height);
        Self {
            width,
            height,
            data: vec![0u8; size],
        }
    }

    /// Create a bitmask from packed byte data.
    /// Validates that the data length matches the expected size.
    pub fn from_data(width: i32, height: i32, data: Vec<u8>) -> Result<Self> {
        let expected = Self::expected_size(width, height);
        if data.len() != expected {
            bail!(
                "bitmask data length {} does not match expected size {} for {}x{} mask",
                data.len(),
                expected,
                width,
                height
            );
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Create a bitmask from a base64-encoded string.
    pub fn from_base64(width: i32, height: i32, base64_data: &str) -> Result<Self> {
        let data = BASE64
            .decode(base64_data)
            .context("failed to decode base64 bitmask data")?;
        Self::from_data(width, height, data)
    }

    /// Encode the bitmask data as a base64 string.
    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.data)
    }

    /// Create a bitmask from a 2D boolean array (row-major order).
    pub fn from_bool_array(pixels: &[Vec<bool>]) -> Result<Self> {
        if pixels.is_empty() {
            return Ok(Self::new(0, 0));
        }

        let height = pixels.len() as i32;
        let width = pixels[0].len() as i32;

        // Validate all rows have the same width
        for row in pixels {
            if row.len() != width as usize {
                bail!("all rows must have the same width");
            }
        }

        let mut bitmask = Self::new(width, height);
        for (y, row) in pixels.iter().enumerate() {
            for (x, &pixel) in row.iter().enumerate() {
                if pixel {
                    bitmask.set_pixel(x as i32, y as i32, true);
                }
            }
        }
        Ok(bitmask)
    }

    /// Get the value of a pixel at (x, y).
    /// Returns false if coordinates are out of bounds.
    pub fn get_pixel(&self, x: i32, y: i32) -> bool {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return false;
        }

        let row_stride = Self::row_stride(self.width);
        let byte_offset = (y as usize) * row_stride + (x as usize) / 8;
        let bit_offset = (x as usize) % 8;

        if byte_offset >= self.data.len() {
            return false;
        }

        (self.data[byte_offset] >> bit_offset) & 1 == 1
    }

    /// Set the value of a pixel at (x, y).
    /// Does nothing if coordinates are out of bounds.
    pub fn set_pixel(&mut self, x: i32, y: i32, value: bool) {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return;
        }

        let row_stride = Self::row_stride(self.width);
        let byte_offset = (y as usize) * row_stride + (x as usize) / 8;
        let bit_offset = (x as usize) % 8;

        if byte_offset >= self.data.len() {
            return;
        }

        if value {
            self.data[byte_offset] |= 1 << bit_offset;
        } else {
            self.data[byte_offset] &= !(1 << bit_offset);
        }
    }

    /// Convert the bitmask to a 2D boolean array (row-major order).
    pub fn to_bool_array(&self) -> Vec<Vec<bool>> {
        let mut result = Vec::with_capacity(self.height as usize);
        for y in 0..self.height {
            let mut row = Vec::with_capacity(self.width as usize);
            for x in 0..self.width {
                row.push(self.get_pixel(x, y));
            }
            result.push(row);
        }
        result
    }

    /// Count the number of pixels set to 1 (labeled/inside).
    pub fn count_set_pixels(&self) -> usize {
        let mut count = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get_pixel(x, y) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Validate that the data length is correct for the dimensions.
    pub fn validate(&self) -> Result<()> {
        let expected = Self::expected_size(self.width, self.height);
        if self.data.len() != expected {
            bail!(
                "bitmask data length {} does not match expected size {} for {}x{} mask",
                self.data.len(),
                expected,
                self.width,
                self.height
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_stride() {
        assert_eq!(Bitmask::row_stride(1), 1);
        assert_eq!(Bitmask::row_stride(7), 1);
        assert_eq!(Bitmask::row_stride(8), 1);
        assert_eq!(Bitmask::row_stride(9), 2);
        assert_eq!(Bitmask::row_stride(16), 2);
        assert_eq!(Bitmask::row_stride(17), 3);
    }

    #[test]
    fn test_expected_size() {
        assert_eq!(Bitmask::expected_size(8, 8), 8);
        assert_eq!(Bitmask::expected_size(10, 10), 20); // 2 bytes per row * 10 rows
        assert_eq!(Bitmask::expected_size(512, 512), 32768); // 64 bytes per row * 512 rows
    }

    #[test]
    fn test_new_bitmask() {
        let bm = Bitmask::new(10, 10);
        assert_eq!(bm.width, 10);
        assert_eq!(bm.height, 10);
        assert_eq!(bm.data.len(), 20);
        // All pixels should be 0
        for y in 0..10 {
            for x in 0..10 {
                assert!(!bm.get_pixel(x, y));
            }
        }
    }

    #[test]
    fn test_set_get_pixel() {
        let mut bm = Bitmask::new(16, 16);

        // Set some pixels
        bm.set_pixel(0, 0, true);
        bm.set_pixel(7, 0, true);
        bm.set_pixel(8, 0, true);
        bm.set_pixel(15, 15, true);

        // Verify
        assert!(bm.get_pixel(0, 0));
        assert!(bm.get_pixel(7, 0));
        assert!(bm.get_pixel(8, 0));
        assert!(bm.get_pixel(15, 15));

        // Unset pixels should be false
        assert!(!bm.get_pixel(1, 0));
        assert!(!bm.get_pixel(0, 1));

        // Out of bounds should return false
        assert!(!bm.get_pixel(-1, 0));
        assert!(!bm.get_pixel(16, 0));
        assert!(!bm.get_pixel(0, 16));
    }

    #[test]
    fn test_base64_roundtrip() {
        let mut bm = Bitmask::new(8, 8);
        bm.set_pixel(0, 0, true);
        bm.set_pixel(7, 7, true);
        bm.set_pixel(3, 4, true);

        let base64 = bm.to_base64();
        let restored = Bitmask::from_base64(8, 8, &base64).unwrap();

        assert!(restored.get_pixel(0, 0));
        assert!(restored.get_pixel(7, 7));
        assert!(restored.get_pixel(3, 4));
        assert!(!restored.get_pixel(1, 1));
    }

    #[test]
    fn test_from_bool_array() {
        let pixels = vec![
            vec![true, false, true, false],
            vec![false, true, false, true],
        ];

        let bm = Bitmask::from_bool_array(&pixels).unwrap();

        assert_eq!(bm.width, 4);
        assert_eq!(bm.height, 2);
        assert!(bm.get_pixel(0, 0));
        assert!(!bm.get_pixel(1, 0));
        assert!(bm.get_pixel(2, 0));
        assert!(!bm.get_pixel(0, 1));
        assert!(bm.get_pixel(1, 1));
    }

    #[test]
    fn test_to_bool_array() {
        let mut bm = Bitmask::new(4, 2);
        bm.set_pixel(0, 0, true);
        bm.set_pixel(2, 0, true);
        bm.set_pixel(1, 1, true);
        bm.set_pixel(3, 1, true);

        let arr = bm.to_bool_array();

        assert_eq!(
            arr,
            vec![
                vec![true, false, true, false],
                vec![false, true, false, true],
            ]
        );
    }

    #[test]
    fn test_count_set_pixels() {
        let mut bm = Bitmask::new(10, 10);
        assert_eq!(bm.count_set_pixels(), 0);

        bm.set_pixel(0, 0, true);
        bm.set_pixel(5, 5, true);
        bm.set_pixel(9, 9, true);

        assert_eq!(bm.count_set_pixels(), 3);
    }

    #[test]
    fn test_invalid_data_length() {
        let result = Bitmask::from_data(8, 8, vec![0u8; 7]);
        assert!(result.is_err());
    }
}
