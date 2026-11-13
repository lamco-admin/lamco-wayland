//! Format Conversion Utilities
//!
//! Provides pixel format conversion between various video formats.
//! Includes optimized SIMD implementations where available.

use libspa::param::video::VideoFormat;

use crate::error::{PipeWireError, Result};

/// Pixel format enum for our internal use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// BGRA 32-bit
    BGRA,
    /// BGRX 32-bit (no alpha)
    BGRx,
    /// RGBA 32-bit
    RGBA,
    /// RGBX 32-bit (no alpha)
    RGBx,
    /// RGB 24-bit
    RGB,
    /// BGR 24-bit
    BGR,
    /// Grayscale 8-bit
    GRAY8,
    /// YUV 4:2:0 semi-planar (NV12)
    NV12,
    /// YUV 4:2:2 packed (YUY2)
    YUY2,
    /// YUV 4:2:0 planar (I420)
    I420,
}

impl PixelFormat {
    /// Convert from SPA VideoFormat
    pub fn from_spa(format: VideoFormat) -> Option<Self> {
        match format {
            VideoFormat::BGRA => Some(Self::BGRA),
            VideoFormat::BGRx => Some(Self::BGRx),
            VideoFormat::RGBA => Some(Self::RGBA),
            VideoFormat::RGBx => Some(Self::RGBx),
            VideoFormat::RGB => Some(Self::RGB),
            VideoFormat::BGR => Some(Self::BGR),
            VideoFormat::GRAY8 => Some(Self::GRAY8),
            VideoFormat::NV12 => Some(Self::NV12),
            VideoFormat::YUY2 => Some(Self::YUY2),
            VideoFormat::I420 => Some(Self::I420),
            _ => None,
        }
    }

    /// Convert to SPA VideoFormat
    pub fn to_spa(&self) -> VideoFormat {
        match self {
            Self::BGRA => VideoFormat::BGRA,
            Self::BGRx => VideoFormat::BGRx,
            Self::RGBA => VideoFormat::RGBA,
            Self::RGBx => VideoFormat::RGBx,
            Self::RGB => VideoFormat::RGB,
            Self::BGR => VideoFormat::BGR,
            Self::GRAY8 => VideoFormat::GRAY8,
            Self::NV12 => VideoFormat::NV12,
            Self::YUY2 => VideoFormat::YUY2,
            Self::I420 => VideoFormat::I420,
        }
    }

    /// Get bytes per pixel (for packed formats)
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::BGRA | Self::BGRx | Self::RGBA | Self::RGBx => 4,
            Self::RGB | Self::BGR => 3,
            Self::GRAY8 => 1,
            Self::NV12 | Self::I420 => 1, // Y plane
            Self::YUY2 => 2,
        }
    }
}

/// Convert pixel data from one format to another
///
/// The target is `BGRA` or `BGRx`. The two share a byte layout and differ only in what the fourth byte means,
/// so every conversion that writes one writes the other: a source without alpha gets an opaque fourth byte,
/// and a source with alpha keeps its own. `BGRA` and `BGRx` also convert to each other, and to themselves
/// when only the stride differs.
pub fn convert_format(
    src: &[u8],
    dst: &mut [u8],
    src_format: PixelFormat,
    dst_format: PixelFormat,
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
) -> Result<()> {
    // Fast path: no conversion needed
    if src_format == dst_format && src_stride == dst_stride {
        let row_bytes = (width * src_format.bytes_per_pixel() as u32) as usize;
        if row_bytes == src_stride as usize {
            // Can do a single memcpy
            dst[..src.len()].copy_from_slice(src);
        } else {
            // Copy row by row
            for y in 0..height {
                let src_offset = (y * src_stride) as usize;
                let dst_offset = (y * dst_stride) as usize;
                dst[dst_offset..dst_offset + row_bytes].copy_from_slice(&src[src_offset..src_offset + row_bytes]);
            }
        }
        return Ok(());
    }

    // Conversion needed
    if !matches!(dst_format, PixelFormat::BGRA | PixelFormat::BGRx) {
        return Err(PipeWireError::FormatConversionFailed(format!(
            "Unsupported conversion: {:?} -> {:?}",
            src_format, dst_format
        )));
    }

    match src_format {
        // A capture that delivers BGRA feeds a consumer that asked for BGRx (Hyprland's ext capture does).
        // The fourth byte is carried: BGRx gives it no meaning.
        PixelFormat::BGRA => copy_bgr32_rows(src, dst, width, height, src_stride, dst_stride, false),

        // BGRx leaves its fourth byte undefined, so as alpha it has to be written.
        PixelFormat::BGRx => copy_bgr32_rows(
            src,
            dst,
            width,
            height,
            src_stride,
            dst_stride,
            dst_format == PixelFormat::BGRA,
        ),

        PixelFormat::RGB => convert_rgb_to_bgra(src, dst, width, height, src_stride, dst_stride),

        PixelFormat::RGBA => convert_rgba_to_bgra(src, dst, width, height, src_stride, dst_stride, false),

        PixelFormat::RGBx => convert_rgba_to_bgra(src, dst, width, height, src_stride, dst_stride, true),

        PixelFormat::BGR => convert_bgr_to_bgra(src, dst, width, height, src_stride, dst_stride),

        PixelFormat::NV12 => convert_nv12_to_bgra(src, dst, width, height),

        PixelFormat::YUY2 => convert_yuy2_to_bgra(src, dst, width, height, src_stride, dst_stride),

        PixelFormat::I420 => convert_i420_to_bgra(src, dst, width, height),

        PixelFormat::GRAY8 => Err(PipeWireError::FormatConversionFailed(format!(
            "Unsupported conversion: {:?} -> {:?}",
            src_format, dst_format
        ))),
    }
}

/// Copy BGRA or BGRx rows between buffers whose strides may differ, writing an opaque fourth byte when asked
fn copy_bgr32_rows(
    src: &[u8],
    dst: &mut [u8],
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
    opaque: bool,
) -> Result<()> {
    let row_bytes = width as usize * 4;
    for y in 0..height {
        let src_offset = (y * src_stride) as usize;
        let dst_offset = (y * dst_stride) as usize;
        let dst_row = &mut dst[dst_offset..dst_offset + row_bytes];
        dst_row.copy_from_slice(&src[src_offset..src_offset + row_bytes]);
        if opaque {
            for alpha in dst_row.iter_mut().skip(3).step_by(4) {
                *alpha = 255;
            }
        }
    }
    Ok(())
}

/// Convert RGB to BGRA
fn convert_rgb_to_bgra(
    src: &[u8],
    dst: &mut [u8],
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
) -> Result<()> {
    for y in 0..height {
        let src_row = &src[(y * src_stride) as usize..];
        let dst_row = &mut dst[(y * dst_stride) as usize..];

        for x in 0..width as usize {
            let src_idx = x * 3;
            let dst_idx = x * 4;

            dst_row[dst_idx] = src_row[src_idx + 2]; // B
            dst_row[dst_idx + 1] = src_row[src_idx + 1]; // G
            dst_row[dst_idx + 2] = src_row[src_idx]; // R
            dst_row[dst_idx + 3] = 255; // A
        }
    }
    Ok(())
}

/// Convert RGBA or RGBx to BGRA
///
/// `opaque` writes 255 as the fourth byte in place of the source's, for RGBx, which leaves it undefined.
fn convert_rgba_to_bgra(
    src: &[u8],
    dst: &mut [u8],
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
    opaque: bool,
) -> Result<()> {
    for y in 0..height {
        let src_row = &src[(y * src_stride) as usize..];
        let dst_row = &mut dst[(y * dst_stride) as usize..];

        for x in 0..width as usize {
            let src_idx = x * 4;
            let dst_idx = x * 4;

            dst_row[dst_idx] = src_row[src_idx + 2]; // B
            dst_row[dst_idx + 1] = src_row[src_idx + 1]; // G
            dst_row[dst_idx + 2] = src_row[src_idx]; // R
            dst_row[dst_idx + 3] = if opaque { 255 } else { src_row[src_idx + 3] }; // A
        }
    }
    Ok(())
}

/// Convert BGR to BGRA
fn convert_bgr_to_bgra(
    src: &[u8],
    dst: &mut [u8],
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
) -> Result<()> {
    for y in 0..height {
        let src_row = &src[(y * src_stride) as usize..];
        let dst_row = &mut dst[(y * dst_stride) as usize..];

        for x in 0..width as usize {
            let src_idx = x * 3;
            let dst_idx = x * 4;

            dst_row[dst_idx] = src_row[src_idx]; // B
            dst_row[dst_idx + 1] = src_row[src_idx + 1]; // G
            dst_row[dst_idx + 2] = src_row[src_idx + 2]; // R
            dst_row[dst_idx + 3] = 255; // A
        }
    }
    Ok(())
}

/// Convert NV12 to BGRA
fn convert_nv12_to_bgra(src: &[u8], dst: &mut [u8], width: u32, height: u32) -> Result<()> {
    let y_plane = &src[0..(width * height) as usize];
    let uv_plane = &src[(width * height) as usize..];

    for y in 0..height {
        for x in 0..width {
            // Get Y value
            let y_val = y_plane[(y * width + x) as usize] as i32;

            // Get UV values (subsampled 2x2)
            let uv_x = x / 2;
            let uv_y = y / 2;
            let uv_idx = (uv_y * width + uv_x * 2) as usize;
            let u_val = uv_plane[uv_idx] as i32;
            let v_val = uv_plane[uv_idx + 1] as i32;

            // YUV to RGB conversion
            let c = y_val - 16;
            let d = u_val - 128;
            let e = v_val - 128;

            let r = (298 * c + 409 * e + 128) >> 8;
            let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
            let b = (298 * c + 516 * d + 128) >> 8;

            // Clamp and write BGRA
            let dst_idx = ((y * width + x) * 4) as usize;
            dst[dst_idx] = clamp(b, 0, 255) as u8;
            dst[dst_idx + 1] = clamp(g, 0, 255) as u8;
            dst[dst_idx + 2] = clamp(r, 0, 255) as u8;
            dst[dst_idx + 3] = 255;
        }
    }
    Ok(())
}

/// Convert YUY2 to BGRA
fn convert_yuy2_to_bgra(
    src: &[u8],
    dst: &mut [u8],
    width: u32,
    height: u32,
    src_stride: u32,
    dst_stride: u32,
) -> Result<()> {
    for y in 0..height {
        let src_row = &src[(y * src_stride) as usize..];
        let dst_row = &mut dst[(y * dst_stride) as usize..];

        for x in (0..width as usize).step_by(2) {
            let src_idx = x * 2;

            let y0 = src_row[src_idx] as i32;
            let u = src_row[src_idx + 1] as i32;
            let y1 = src_row[src_idx + 2] as i32;
            let v = src_row[src_idx + 3] as i32;

            // Convert first pixel
            let (r0, g0, b0) = yuv_to_rgb(y0, u, v);
            let dst_idx0 = x * 4;
            dst_row[dst_idx0] = b0;
            dst_row[dst_idx0 + 1] = g0;
            dst_row[dst_idx0 + 2] = r0;
            dst_row[dst_idx0 + 3] = 255;

            // Convert second pixel
            if x + 1 < width as usize {
                let (r1, g1, b1) = yuv_to_rgb(y1, u, v);
                let dst_idx1 = (x + 1) * 4;
                dst_row[dst_idx1] = b1;
                dst_row[dst_idx1 + 1] = g1;
                dst_row[dst_idx1 + 2] = r1;
                dst_row[dst_idx1 + 3] = 255;
            }
        }
    }
    Ok(())
}

/// Convert I420 to BGRA
fn convert_i420_to_bgra(src: &[u8], dst: &mut [u8], width: u32, height: u32) -> Result<()> {
    let y_plane_size = (width * height) as usize;
    let uv_plane_size = y_plane_size / 4;

    let y_plane = &src[0..y_plane_size];
    let u_plane = &src[y_plane_size..y_plane_size + uv_plane_size];
    let v_plane = &src[y_plane_size + uv_plane_size..];

    for y in 0..height {
        for x in 0..width {
            // Get Y value
            let y_val = y_plane[(y * width + x) as usize] as i32;

            // Get UV values (subsampled 2x2)
            let uv_x = x / 2;
            let uv_y = y / 2;
            let uv_idx = (uv_y * width / 2 + uv_x) as usize;
            let u_val = u_plane[uv_idx] as i32;
            let v_val = v_plane[uv_idx] as i32;

            // YUV to RGB conversion
            let (r, g, b) = yuv_to_rgb(y_val, u_val, v_val);

            // Write BGRA
            let dst_idx = ((y * width + x) * 4) as usize;
            dst[dst_idx] = b;
            dst[dst_idx + 1] = g;
            dst[dst_idx + 2] = r;
            dst[dst_idx + 3] = 255;
        }
    }
    Ok(())
}

/// YUV to RGB conversion helper
#[inline]
fn yuv_to_rgb(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let c = y - 16;
    let d = u - 128;
    let e = v - 128;

    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;

    (clamp(r, 0, 255) as u8, clamp(g, 0, 255) as u8, clamp(b, 0, 255) as u8)
}

/// Clamp value to range
#[inline]
fn clamp(val: i32, min: i32, max: i32) -> i32 {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_conversion() {
        assert_eq!(PixelFormat::from_spa(VideoFormat::BGRA), Some(PixelFormat::BGRA));
        assert_eq!(PixelFormat::BGRA.to_spa(), VideoFormat::BGRA);
    }

    #[test]
    fn test_rgb_to_bgra_conversion() {
        let src = vec![
            255, 0, 0, // Red pixel
            0, 255, 0, // Green pixel
            0, 0, 255, // Blue pixel
        ];
        let mut dst = vec![0u8; 12]; // 3 pixels * 4 bytes

        convert_rgb_to_bgra(&src, &mut dst, 3, 1, 9, 12).unwrap();

        // Red pixel (RGB 255,0,0 -> BGRA 0,0,255,255)
        assert_eq!(dst[0], 0); // B
        assert_eq!(dst[1], 0); // G
        assert_eq!(dst[2], 255); // R
        assert_eq!(dst[3], 255); // A

        // Green pixel (RGB 0,255,0 -> BGRA 0,255,0,255)
        assert_eq!(dst[4], 0); // B
        assert_eq!(dst[5], 255); // G
        assert_eq!(dst[6], 0); // R
        assert_eq!(dst[7], 255); // A

        // Blue pixel (RGB 0,0,255 -> BGRA 255,0,0,255)
        assert_eq!(dst[8], 255); // B
        assert_eq!(dst[9], 0); // G
        assert_eq!(dst[10], 0); // R
        assert_eq!(dst[11], 255); // A
    }

    /// Two pixels a row, two rows, with one pixel of padding after each row on both sides.
    /// The padding holds 0xEE in the source and must come out untouched (0x11) in the target.
    fn convert_2x2(src_format: PixelFormat, dst_format: PixelFormat, pixels: [[u8; 4]; 4]) -> Vec<u8> {
        let mut src = Vec::new();
        for row in pixels.chunks(2) {
            src.extend_from_slice(&row[0]);
            src.extend_from_slice(&row[1]);
            src.extend_from_slice(&[0xEE; 4]);
        }
        let mut dst = vec![0x11u8; 24];
        convert_format(&src, &mut dst, src_format, dst_format, 2, 2, 12, 12).unwrap();
        assert_eq!(dst[8..12], [0x11; 4], "row padding was written");
        assert_eq!(dst[20..24], [0x11; 4], "row padding was written");
        [&dst[0..8], &dst[12..20]].concat()
    }

    const PIXELS: [[u8; 4]; 4] = [[1, 2, 3, 0], [4, 5, 6, 7], [8, 9, 10, 0], [11, 12, 13, 200]];

    #[test]
    fn test_bgra_to_bgrx_carries_every_byte() {
        let out = convert_2x2(PixelFormat::BGRA, PixelFormat::BGRx, PIXELS);
        assert_eq!(out, PIXELS.concat());
    }

    #[test]
    fn test_bgrx_to_bgra_writes_opaque_alpha() {
        let out = convert_2x2(PixelFormat::BGRx, PixelFormat::BGRA, PIXELS);
        assert_eq!(out, [1, 2, 3, 255, 4, 5, 6, 255, 8, 9, 10, 255, 11, 12, 13, 255]);
    }

    #[test]
    fn test_rgba_to_bgrx_swaps_red_and_blue() {
        let out = convert_2x2(PixelFormat::RGBA, PixelFormat::BGRx, PIXELS);
        assert_eq!(out, [3, 2, 1, 0, 6, 5, 4, 7, 10, 9, 8, 0, 13, 12, 11, 200]);
    }

    #[test]
    fn test_rgbx_to_bgra_swaps_and_writes_opaque_alpha() {
        let out = convert_2x2(PixelFormat::RGBx, PixelFormat::BGRA, PIXELS);
        assert_eq!(out, [3, 2, 1, 255, 6, 5, 4, 255, 10, 9, 8, 255, 13, 12, 11, 255]);
    }

    #[test]
    fn test_same_format_with_a_different_stride_copies_rows() {
        let src = [1, 2, 3, 4, 0xEE, 0xEE, 0xEE, 0xEE, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE];
        let mut dst = vec![0x11u8; 8];
        convert_format(&src, &mut dst, PixelFormat::BGRx, PixelFormat::BGRx, 1, 2, 8, 4).unwrap();
        assert_eq!(dst, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_rgb_to_bgrx_through_convert_format() {
        let src = [255, 0, 0, 0, 255, 0];
        let mut dst = vec![0u8; 8];
        convert_format(&src, &mut dst, PixelFormat::RGB, PixelFormat::BGRx, 2, 1, 6, 8).unwrap();
        assert_eq!(dst, [0, 0, 255, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn test_targets_other_than_bgra_and_bgrx_are_refused() {
        let src = [0u8; 8];
        let mut dst = vec![0u8; 8];
        for dst_format in [
            PixelFormat::RGBA,
            PixelFormat::RGBx,
            PixelFormat::BGR,
            PixelFormat::NV12,
        ] {
            assert!(convert_format(&src, &mut dst, PixelFormat::BGRA, dst_format, 2, 1, 8, 8).is_err());
        }
        assert!(convert_format(&src, &mut dst, PixelFormat::GRAY8, PixelFormat::BGRA, 2, 1, 8, 8).is_err());
    }

    #[test]
    fn test_yuv_to_rgb() {
        // Test black (Y=16, U=128, V=128)
        let (r, g, b) = yuv_to_rgb(16, 128, 128);
        assert_eq!((r, g, b), (0, 0, 0));

        // Test white (Y=235, U=128, V=128)
        let (r, g, b) = yuv_to_rgb(235, 128, 128);
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(-10, 0, 255), 0);
        assert_eq!(clamp(300, 0, 255), 255);
        assert_eq!(clamp(128, 0, 255), 128);
    }
}
