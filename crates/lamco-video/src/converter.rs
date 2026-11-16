//! Video Frame Converter
//!
//! Converts PipeWire VideoFrame structs into RDP-ready bitmap data.
//! This module handles:
//! - Pixel format conversion to RDP-compatible formats
//! - Stride alignment for RDP protocol requirements
//! - Buffer pooling for memory efficiency
//! - Damage region tracking and optimization
//! - SIMD-optimized conversion routines where available
//!
//! The converter prepares data structures ready for RDP transmission.
//! When IronRDP becomes available, these will integrate seamlessly with
//! IronRDP's bitmap encoding functionality.

use std::sync::Arc;
use std::time::Instant;

use lamco_pipewire::{FfiDamageRegion, PixelFormat, VideoFrame, convert_format};
use parking_lot::RwLock;

/// Alignment boundary for RDP bitmaps (64 bytes for SIMD optimization)
const RDP_BITMAP_ALIGNMENT: usize = 64;

/// Maximum number of pooled buffers
const BUFFER_POOL_SIZE: usize = 8;

/// Damage threshold for forcing full update (75% of screen)
const DAMAGE_THRESHOLD: f32 = 0.75;

/// RDP pixel format (subset of formats supported by RDP protocol)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdpPixelFormat {
    /// 32-bit BGRX (B8G8R8X8)
    BgrX32,
    /// 24-bit BGR (B8G8R8)
    Bgr24,
    /// 16-bit RGB (R5G6B5)
    Rgb16,
    /// 15-bit RGB (X1R5G5B5)
    Rgb15,
}

impl RdpPixelFormat {
    /// Get bytes per pixel for this format
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::BgrX32 => 4,
            Self::Bgr24 => 3,
            Self::Rgb16 | Self::Rgb15 => 2,
        }
    }

    /// Get the best RDP format for a given PipeWire format
    pub fn from_pixel_format(format: PixelFormat) -> Self {
        match format {
            PixelFormat::BGRA | PixelFormat::BGRx => Self::BgrX32,
            PixelFormat::RGBA | PixelFormat::RGBx => Self::BgrX32,
            PixelFormat::RGB | PixelFormat::BGR => Self::Bgr24,
            // YUV formats convert to BGRX32
            PixelFormat::NV12 | PixelFormat::YUY2 | PixelFormat::I420 => Self::BgrX32,
            PixelFormat::GRAY8 => Self::BgrX32, // Expand to RGB
        }
    }
}

/// Rectangle representing a region
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rectangle {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl Rectangle {
    /// Create a new rectangle
    pub fn new(left: u16, top: u16, right: u16, bottom: u16) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Get width
    pub fn width(&self) -> u16 {
        self.right.saturating_sub(self.left)
    }

    /// Get height
    pub fn height(&self) -> u16 {
        self.bottom.saturating_sub(self.top)
    }

    /// Get area
    pub fn area(&self) -> u32 {
        self.width() as u32 * self.height() as u32
    }

    /// Check if this rectangle intersects another
    pub fn intersects(&self, other: &Rectangle) -> bool {
        !(self.right <= other.left || other.right <= self.left || self.bottom <= other.top || other.bottom <= self.top)
    }

    /// Merge with another rectangle (union)
    pub fn merge(&mut self, other: &Rectangle) {
        self.left = self.left.min(other.left);
        self.top = self.top.min(other.top);
        self.right = self.right.max(other.right);
        self.bottom = self.bottom.max(other.bottom);
    }
}

impl From<FfiDamageRegion> for Rectangle {
    fn from(damage: FfiDamageRegion) -> Self {
        Self {
            left: damage.x as u16,
            top: damage.y as u16,
            right: ((damage.x as u32) + damage.width) as u16,
            bottom: ((damage.y as u32) + damage.height) as u16,
        }
    }
}

/// RDP-ready bitmap data
#[derive(Debug, Clone)]
pub struct BitmapData {
    /// Region this bitmap covers
    pub rectangle: Rectangle,
    /// Pixel format
    pub format: RdpPixelFormat,
    /// Raw pixel data
    pub data: Vec<u8>,
    /// Whether data is compressed
    pub compressed: bool,
}

/// Bitmap update containing one or more changed regions
#[derive(Debug, Clone)]
pub struct BitmapUpdate {
    /// Rectangles to update
    pub rectangles: Vec<BitmapData>,
}

/// Output buffers kept between conversions, so a frame reuses one instead of allocating.
///
/// A conversion acquires its buffer and releases it before returning, so in steady state one
/// buffer serves every frame.
struct BufferPool {
    free: Vec<Vec<u8>>,
    max_free: usize,
}

impl BufferPool {
    fn new(max_free: usize) -> Self {
        Self {
            free: Vec::with_capacity(max_free),
            max_free,
        }
    }

    /// Returns a zeroed buffer of `size` bytes, reusing the most recently released one if any.
    fn acquire(&mut self, size: usize) -> Vec<u8> {
        let mut buffer = self
            .free
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(align_to_boundary(size, RDP_BITMAP_ALIGNMENT)));
        buffer.clear();
        buffer.resize(size, 0);
        buffer
    }

    /// Keeps `buffer` for a later conversion, or drops it when the pool already keeps enough.
    fn release(&mut self, buffer: Vec<u8>) {
        if self.free.len() < self.max_free {
            self.free.push(buffer);
        }
    }
}

/// Damage tracker for optimizing bitmap updates
struct DamageTracker {
    regions: Vec<Rectangle>,
    full_update: bool,
    screen_width: u16,
    screen_height: u16,
}

impl DamageTracker {
    fn new(width: u16, height: u16) -> Self {
        Self {
            regions: Vec::new(),
            full_update: false,
            screen_width: width,
            screen_height: height,
        }
    }

    /// Add a damage region
    fn add_damage(&mut self, region: Rectangle) {
        if self.full_update {
            return;
        }

        // Check if this damage should trigger a full update
        let total_area = self.screen_width as u32 * self.screen_height as u32;
        let damage_area = region.area();

        if damage_area as f32 / total_area as f32 > DAMAGE_THRESHOLD {
            self.full_update = true;
            self.regions.clear();
            return;
        }

        // Try to merge with existing regions
        let mut merged = false;
        for existing in &mut self.regions {
            if existing.intersects(&region) {
                existing.merge(&region);
                merged = true;
                break;
            }
        }

        if !merged {
            self.regions.push(region);
        }

        // Consolidate overlapping regions
        self.consolidate_regions();
    }

    /// Consolidate overlapping regions
    fn consolidate_regions(&mut self) {
        if self.regions.len() < 2 {
            return;
        }

        let mut consolidated = Vec::new();
        let mut used = vec![false; self.regions.len()];

        for i in 0..self.regions.len() {
            if used[i] {
                continue;
            }

            let mut current = self.regions[i];
            used[i] = true;

            for j in (i + 1)..self.regions.len() {
                if used[j] {
                    continue;
                }

                if current.intersects(&self.regions[j]) {
                    current.merge(&self.regions[j]);
                    used[j] = true;
                }
            }

            consolidated.push(current);
        }

        self.regions = consolidated;
    }

    /// Get damage regions to update
    fn get_damage_regions(&self) -> Vec<Rectangle> {
        if self.full_update {
            vec![Rectangle::new(0, 0, self.screen_width, self.screen_height)]
        } else {
            self.regions.clone()
        }
    }

    /// Reset damage tracking
    fn reset(&mut self) {
        self.regions.clear();
        self.full_update = false;
    }
}

/// Conversion statistics
#[derive(Debug, Clone, Default)]
pub struct ConversionStats {
    /// Total frames converted
    pub frames_converted: u64,
    /// Total bytes processed
    pub bytes_processed: u64,
    /// Total conversion time (nanoseconds)
    pub conversion_time_ns: u64,
    /// Frames with SIMD optimization
    pub simd_optimized_frames: u64,
}

impl ConversionStats {
    /// Get average conversion time in milliseconds
    pub fn avg_conversion_time_ms(&self) -> f64 {
        if self.frames_converted == 0 {
            0.0
        } else {
            (self.conversion_time_ns as f64 / self.frames_converted as f64) / 1_000_000.0
        }
    }

    /// Get throughput in MB/s
    pub fn throughput_mbps(&self) -> f64 {
        if self.conversion_time_ns == 0 {
            0.0
        } else {
            (self.bytes_processed as f64 / 1_048_576.0) / (self.conversion_time_ns as f64 / 1_000_000_000.0)
        }
    }
}

/// Bitmap converter
pub struct BitmapConverter {
    buffer_pool: Arc<RwLock<BufferPool>>,
    damage_tracker: Arc<RwLock<DamageTracker>>,
    last_frame_hash: u64,
    enable_simd: bool,
    stats: Arc<RwLock<ConversionStats>>,
}

impl BitmapConverter {
    /// Create a new bitmap converter
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer_pool: Arc::new(RwLock::new(BufferPool::new(BUFFER_POOL_SIZE))),
            damage_tracker: Arc::new(RwLock::new(DamageTracker::new(width, height))),
            last_frame_hash: u64::MAX, // Sentinel: ensures first frame is never skipped
            enable_simd: Self::detect_simd_support(),
            stats: Arc::new(RwLock::new(ConversionStats::default())),
        }
    }

    /// Detect SIMD support
    // nonminimal_bool misfires on the is_x86_feature_detected! macro expansion;
    // `avx2 || ssse3` is already minimal (two distinct features).
    #[allow(clippy::nonminimal_bool)]
    fn detect_simd_support() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("avx2") || is_x86_feature_detected!("ssse3")
        }
        #[cfg(target_arch = "aarch64")]
        {
            std::arch::is_aarch64_feature_detected!("neon")
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            false
        }
    }

    /// Convert a video frame to RDP bitmap update
    ///
    /// # Arguments
    /// * `frame` - The video frame to convert
    ///
    /// # Returns
    /// A `BitmapUpdate` containing the changed regions ready for RDP transmission
    ///
    /// # Errors
    /// Returns an error if:
    /// - Frame format is unsupported
    /// - Frame data is invalid
    /// - Buffer allocation fails
    pub fn convert_frame(&mut self, frame: &VideoFrame) -> Result<BitmapUpdate, ConversionError> {
        let start_time = Instant::now();

        // Validate frame
        if !frame.is_valid() {
            return Err(ConversionError::InvalidFrame(
                "Frame is corrupted or incomplete".to_string(),
            ));
        }

        // Calculate frame hash for change detection
        let pixel_data = frame.data().ok_or_else(|| {
            ConversionError::InvalidFrame("DMA-BUF frames not supported in bitmap conversion".to_string())
        })?;
        let frame_hash = self.calculate_frame_hash(pixel_data);

        // Check if frame has changed
        if frame_hash == self.last_frame_hash {
            return Ok(BitmapUpdate { rectangles: vec![] });
        }
        self.last_frame_hash = frame_hash;

        // Process damage regions
        if !frame.damage_regions.is_empty() {
            let mut tracker = self.damage_tracker.write();
            for damage in &frame.damage_regions {
                tracker.add_damage(Rectangle::from(*damage));
            }
        } else {
            // No damage info = full update
            self.damage_tracker.write().full_update = true;
        }

        let damage_regions = self.damage_tracker.read().get_damage_regions();

        // Calculate output size
        let rdp_format = RdpPixelFormat::from_pixel_format(frame.format);
        let output_size = Self::calculate_output_size(frame.width, frame.height, rdp_format);

        // Acquire buffer from pool
        let mut output_buffer = self.buffer_pool.write().acquire(output_size);

        // Perform format conversion
        self.convert_frame_data(frame, &mut output_buffer, rdp_format)?;

        // Create bitmap data for each damage region
        let mut rectangles = Vec::new();

        for region in damage_regions {
            let bitmap_data = self.create_bitmap_data(&output_buffer, region, frame.width, frame.height, rdp_format)?;
            rectangles.push(bitmap_data);
        }

        // Update statistics
        let elapsed = start_time.elapsed();
        let mut stats = self.stats.write();
        stats.frames_converted += 1;
        stats.bytes_processed += frame.data_size() as u64;
        stats.conversion_time_ns += elapsed.as_nanos() as u64;
        if self.enable_simd {
            stats.simd_optimized_frames += 1;
        }

        // Release buffer back to pool
        self.buffer_pool.write().release(output_buffer);

        // Reset damage tracker
        self.damage_tracker.write().reset();

        Ok(BitmapUpdate { rectangles })
    }

    /// Calculate frame hash for change detection
    fn calculate_frame_hash(&self, data: &[u8]) -> u64 {
        // Simple FNV-1a hash, sampling every 64th byte for speed
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in data.iter().step_by(64) {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    /// Calculate output buffer size
    fn calculate_output_size(width: u32, height: u32, format: RdpPixelFormat) -> usize {
        let stride = calculate_rdp_stride(width, format);
        (stride * height) as usize
    }

    /// Convert frame data to target format
    fn convert_frame_data(
        &self,
        frame: &VideoFrame,
        dst: &mut [u8],
        rdp_format: RdpPixelFormat,
    ) -> Result<(), ConversionError> {
        // Use lamco-pipewire format conversion as base
        // Then ensure RDP-compatible stride alignment
        let target_format = match rdp_format {
            RdpPixelFormat::BgrX32 => PixelFormat::BGRx,
            RdpPixelFormat::Bgr24 => PixelFormat::BGR,
            RdpPixelFormat::Rgb16 | RdpPixelFormat::Rgb15 => {
                return Err(ConversionError::UnsupportedFormat(frame.format));
            }
        };

        let pixel_data = frame.data().ok_or_else(|| {
            ConversionError::InvalidFrame("DMA-BUF frames not supported in format conversion".to_string())
        })?;
        let dst_stride = calculate_rdp_stride(frame.width, rdp_format);

        convert_format(
            pixel_data,
            dst,
            frame.format,
            target_format,
            frame.width,
            frame.height,
            frame.stride,
            dst_stride,
        )
        .map_err(|e| ConversionError::ConversionFailed(e.to_string()))
    }

    /// Create bitmap data for a specific region
    fn create_bitmap_data(
        &self,
        buffer: &[u8],
        region: Rectangle,
        width: u32,
        height: u32,
        format: RdpPixelFormat,
    ) -> Result<BitmapData, ConversionError> {
        let region_width = region.width() as u32;
        let region_height = region.height() as u32;
        let stride = calculate_rdp_stride(width, format);
        let bpp = format.bytes_per_pixel() as u32;

        // Extract region data
        let mut region_data = Vec::with_capacity((region_width * region_height * bpp) as usize);

        for y in region.top..region.bottom {
            if y >= height as u16 {
                break;
            }

            let src_offset = (y as u32 * stride + region.left as u32 * bpp) as usize;
            let row_size = (region_width * bpp) as usize;

            if src_offset + row_size <= buffer.len() {
                region_data.extend_from_slice(&buffer[src_offset..src_offset + row_size]);
            } else {
                return Err(ConversionError::BufferTooSmall {
                    required: src_offset + row_size,
                    provided: buffer.len(),
                });
            }
        }

        Ok(BitmapData {
            rectangle: region,
            format,
            data: region_data,
            compressed: false,
        })
    }

    /// Force a full update on the next frame
    pub fn force_full_update(&mut self) {
        self.damage_tracker.write().full_update = true;
    }

    /// Get conversion statistics
    pub fn get_statistics(&self) -> ConversionStats {
        self.stats.read().clone()
    }

    /// Reset statistics
    pub fn reset_statistics(&mut self) {
        *self.stats.write() = ConversionStats::default();
    }
}

/// Calculate RDP-compatible stride (aligned to 64 bytes)
fn calculate_rdp_stride(width: u32, format: RdpPixelFormat) -> u32 {
    let bpp = format.bytes_per_pixel() as u32;
    let row_bytes = width * bpp;
    align_to_boundary(row_bytes as usize, RDP_BITMAP_ALIGNMENT) as u32
}

/// Align value to boundary
fn align_to_boundary(value: usize, boundary: usize) -> usize {
    (value + boundary - 1) & !(boundary - 1)
}

/// Conversion errors
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    #[error("Unsupported pixel format: {0:?}")]
    UnsupportedFormat(PixelFormat),

    #[error("Buffer too small: required {required} bytes, provided {provided} bytes")]
    BufferTooSmall { required: usize, provided: usize },

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("Conversion failed: {0}")]
    ConversionFailed(String),

    #[error("Allocation failed for {0} bytes")]
    AllocationFailed(usize),
}

// SAFETY: BitmapConverter is Send because all its fields are Send:
// - Arc<RwLock<BufferPool>> is Send when BufferPool is Send (see below)
// - Arc<RwLock<DamageTracker>> is Send when DamageTracker is Send (see below)
// - u64 is Send
// - bool is Send
// - Arc<RwLock<ConversionStats>> is Send when ConversionStats is Send (derives)
#[allow(unsafe_code)]
unsafe impl Send for BitmapConverter {}

// SAFETY: BufferPool is Send because all its fields are Send:
// - Vec<Vec<u8>> and usize are Send
#[allow(unsafe_code)]
unsafe impl Send for BufferPool {}

// SAFETY: DamageTracker is Send because all its fields are Send:
// - Vec<Rectangle> is Send (Rectangle is Copy and contains only primitives)
// - bool, u16 are Send
#[allow(unsafe_code)]
unsafe impl Send for DamageTracker {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rectangle_operations() {
        let rect1 = Rectangle::new(0, 0, 100, 100);
        let rect2 = Rectangle::new(50, 50, 150, 150);

        assert_eq!(rect1.width(), 100);
        assert_eq!(rect1.height(), 100);
        assert_eq!(rect1.area(), 10000);

        assert!(rect1.intersects(&rect2));

        let mut merged = rect1;
        merged.merge(&rect2);
        assert_eq!(merged, Rectangle::new(0, 0, 150, 150));
    }

    #[test]
    fn test_damage_tracker() {
        let mut tracker = DamageTracker::new(1920, 1080);

        // Add non-overlapping regions
        tracker.add_damage(Rectangle::new(0, 0, 100, 100));
        tracker.add_damage(Rectangle::new(200, 200, 300, 300));
        assert_eq!(tracker.regions.len(), 2);

        // Add overlapping region - should merge
        tracker.add_damage(Rectangle::new(50, 50, 150, 150));
        assert_eq!(tracker.regions.len(), 2);

        tracker.reset();
        assert_eq!(tracker.regions.len(), 0);
    }

    // oracle: regression lamco-video 0.4.1 BufferPool, whose acquire never returned a released buffer
    #[test]
    fn a_released_buffer_serves_the_next_frame() {
        let mut pool = BufferPool::new(4);
        let first = pool.acquire(1024);
        let first_address = first.as_ptr();
        pool.release(first);

        let second = pool.acquire(1024);

        assert_eq!(second.as_ptr(), first_address);
        assert!(pool.free.is_empty());
    }

    // oracle: mutant a pool that hands a reused buffer back with the last frame's bytes or length
    #[test]
    fn a_reused_buffer_is_zeroed_at_the_size_asked_for() {
        let mut pool = BufferPool::new(4);
        let mut used = pool.acquire(8);
        used.fill(0xAB);
        pool.release(used);

        let smaller = pool.acquire(4);

        assert_eq!(smaller, [0; 4]);
    }

    // oracle: mutant a pool that keeps every buffer released to it
    #[test]
    fn a_pool_keeps_no_more_than_its_limit() {
        let mut pool = BufferPool::new(2);
        let buffers: Vec<_> = (0..5).map(|_| pool.acquire(16)).collect();

        for buffer in buffers {
            pool.release(buffer);
        }

        assert_eq!(pool.free.len(), 2);
    }

    // oracle: regression lamco-video 0.4.1 BufferPool, which retained up to eight frames it never reused
    #[test]
    fn converting_frames_one_after_another_keeps_one_output_buffer() {
        let mut converter = BitmapConverter::new(4, 2);
        for shade in [10, 200] {
            let frame = VideoFrame::with_data(1, 4, 2, 16, PixelFormat::BGRx, 0, vec![shade; 32]);
            let update = converter.convert_frame(&frame);
            assert!(update.is_ok(), "{:?}", update.err());
        }

        assert_eq!(converter.buffer_pool.read().free.len(), 1);
    }

    #[test]
    fn test_rdp_pixel_format() {
        assert_eq!(RdpPixelFormat::BgrX32.bytes_per_pixel(), 4);
        assert_eq!(RdpPixelFormat::Bgr24.bytes_per_pixel(), 3);
        assert_eq!(RdpPixelFormat::Rgb16.bytes_per_pixel(), 2);

        assert_eq!(
            RdpPixelFormat::from_pixel_format(PixelFormat::BGRA),
            RdpPixelFormat::BgrX32
        );
    }

    #[test]
    fn test_stride_calculation() {
        assert_eq!(calculate_rdp_stride(1920, RdpPixelFormat::BgrX32), 7680);
        assert_eq!(calculate_rdp_stride(1921, RdpPixelFormat::BgrX32), 7744); // Aligned
        assert_eq!(calculate_rdp_stride(1920, RdpPixelFormat::Bgr24), 5760);
    }

    #[test]
    fn test_conversion_stats() {
        let mut stats = ConversionStats::default();
        stats.frames_converted = 100;
        stats.bytes_processed = 100_000_000;
        stats.conversion_time_ns = 500_000_000; // 500ms

        assert_eq!(stats.avg_conversion_time_ms(), 5.0); // 5ms per frame
        assert!(stats.throughput_mbps() > 0.0);
    }

    #[test]
    fn test_bitmap_converter_creation() {
        let converter = BitmapConverter::new(1920, 1080);
        let stats = converter.get_statistics();
        assert_eq!(stats.frames_converted, 0);
    }

    /// A 16x1 frame in each 32-bit format a capture can deliver converts to one BGRx rectangle.
    /// BGRA and RGBA were refused before lamco-pipewire took BGRx as a conversion target,
    /// so a client on the bitmap path got no picture from a compositor that delivers them.
    #[test]
    fn test_convert_frame_takes_every_32_bit_capture_format() {
        let blue_first = [10u8, 20, 30, 40];
        let red_first = [30u8, 20, 10, 40];
        for (format, pixel) in [
            (PixelFormat::BGRx, blue_first),
            (PixelFormat::BGRA, blue_first),
            (PixelFormat::RGBx, red_first),
            (PixelFormat::RGBA, red_first),
        ] {
            let frame = VideoFrame::with_data(1, 16, 1, 64, format, 0, pixel.repeat(16));
            let update = BitmapConverter::new(16, 1).convert_frame(&frame);
            assert!(update.is_ok(), "{format:?} frame refused: {:?}", update.as_ref().err());
            let rectangles = update.map(|update| update.rectangles).unwrap_or_default();
            assert_eq!(rectangles.len(), 1, "{format:?}");
            assert_eq!(rectangles[0].format, RdpPixelFormat::BgrX32, "{format:?}");
            assert_eq!(
                rectangles[0].data[0..3],
                [10, 20, 30],
                "{format:?} came out in the wrong byte order"
            );
        }
    }
}
