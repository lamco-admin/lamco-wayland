//! Buffer Metadata Extraction
//!
//! Reads SPA metadata from PipeWire buffers. Compositors attach metadata
//! to signal buffer orientation (VideoTransform), timing (Header),
//! crop regions (VideoCrop), damage rectangles (VideoDamage), and
//! cursor state (Cursor).
//!
//! All metadata types are optional — compositors only provide what they
//! support, and consumers must handle absent metadata gracefully.

use tracing::{debug, trace};

/// Metadata extracted from a PipeWire buffer's SPA metadata array.
///
/// All fields are `Option` because compositors may not provide all types,
/// even when requested during stream negotiation.
#[derive(Debug, Clone, Default)]
pub struct BufferMeta {
    /// SPA_META_Header: presentation timestamp, flags, sequence number.
    /// Provides compositor-authoritative timing and buffer health signals
    /// (CORRUPTED, DISCONT, GAP flags).
    pub header: Option<HeaderMeta>,

    /// SPA_META_VideoTransform: buffer orientation (0-7).
    /// Maps 1:1 to wl_output.transform values.
    pub transform: Option<u32>,

    /// SPA_META_VideoCrop: source crop region within the buffer.
    /// Compositor may send oversized buffers with crop hints.
    pub crop: Option<CropRegion>,

    /// SPA_META_VideoDamage: changed regions since last buffer.
    /// Enables partial-frame encoding for bandwidth optimization.
    pub damage: Vec<DamageRect>,

    /// SPA_META_Cursor: cursor position, hotspot, and bitmap info.
    pub cursor: Option<CursorMeta>,

    /// Chunk stride from spa_chunk (signed i32).
    /// Negative stride means the buffer is stored bottom-up (GL convention).
    /// Set from the buffer's data chunk, not from SPA metadata.
    pub chunk_stride: i32,

    /// Buffer data type (1=MemPtr, 2=MemFd, 3=DmaBuf).
    /// Different buffer types may require different transform handling
    /// due to compositor-specific code paths for each type.
    pub buffer_type: u32,

    /// Whether the producer set `SPA_DATA_FLAG_MAPPABLE` on the first data block.
    ///
    /// A CPU read of a block without this flag is out of contract. It does not
    /// fail loudly: mapping host-resident GPU memory succeeds and returns
    /// zeros, so this flag is the only advance warning that a mmap will produce
    /// nothing. Set from the data block, not from SPA metadata.
    pub mappable: bool,

    /// Whether the producer set `SPA_CHUNK_FLAG_EMPTY` on the first chunk.
    ///
    /// An explicit "nothing this cycle", as opposed to a zero size, which is
    /// only an absence. Set from the data chunk, not from SPA metadata.
    pub chunk_empty: bool,
}

/// Timing and health metadata from SPA_META_Header.
#[derive(Debug, Clone)]
pub struct HeaderMeta {
    /// Presentation timestamp in nanoseconds
    pub pts: i64,
    /// Decode timestamp offset from PTS
    pub dts_offset: i64,
    /// Sequence number (increments per frame)
    pub seq: u64,
    /// Buffer flags (CORRUPTED, DISCONT, GAP, DELTA_UNIT)
    pub flags: u32,
}

impl HeaderMeta {
    /// Producer marked this buffer as not continuous with the previous one.
    ///
    /// A capture consumer that tracks damage regions incrementally has to treat
    /// a discontinuity as a full-frame invalidation: the accumulated damage
    /// state describes a sequence this buffer is not part of.
    pub fn is_discont(&self) -> bool {
        self.flags & crate::ffi::spa_flags::SPA_META_HEADER_FLAG_DISCONT != 0
    }

    /// Producer marked this buffer's data as possibly corrupted.
    ///
    /// Independent of the chunk-level `SPA_CHUNK_FLAG_CORRUPTED`; a producer
    /// may set either, so both are read.
    pub fn is_corrupted(&self) -> bool {
        self.flags & crate::ffi::spa_flags::SPA_META_HEADER_FLAG_CORRUPTED != 0
    }

    /// Buffer carries media-neutral filler rather than real content.
    pub fn has_gap(&self) -> bool {
        self.flags & crate::ffi::spa_flags::SPA_META_HEADER_FLAG_GAP != 0
    }
}

/// Crop region from SPA_META_VideoCrop.
#[derive(Debug, Clone)]
pub struct CropRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Damage rectangle from SPA_META_VideoDamage.
#[derive(Debug, Clone)]
pub struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Cursor metadata from SPA_META_Cursor.
#[derive(Debug, Clone)]
pub struct CursorMeta {
    /// Cursor ID (0 = invalid/no cursor)
    pub id: u32,
    /// Position on screen
    pub position: (i32, i32),
    /// Hotspot offset within the cursor bitmap
    pub hotspot: (i32, i32),
    /// Offset to bitmap data within the meta struct.
    /// 0 = no bitmap, >= sizeof(spa_meta_cursor) = bitmap follows.
    pub bitmap_offset: u32,
}

/// Extract all metadata from a PipeWire buffer's SPA metadata array.
///
/// # Safety
///
/// `spa_buf` must point to a valid `spa_buffer` that is currently dequeued
/// from a PipeWire stream (i.e., valid for the duration of a process callback).
pub unsafe fn extract_buffer_meta(spa_buf: *const libspa_sys::spa_buffer) -> BufferMeta {
    if spa_buf.is_null() {
        return BufferMeta::default();
    }

    let mut meta = BufferMeta::default();

    // SAFETY: spa_buf is non-null (checked above) and valid for the process callback duration.
    // Each read function checks for null metadata pointers before dereferencing.
    unsafe {
        meta.header = read_header_meta(spa_buf);
        meta.transform = read_transform_meta(spa_buf);
        meta.crop = read_crop_meta(spa_buf);
        meta.damage = read_damage_meta(spa_buf);
        meta.cursor = read_cursor_meta(spa_buf);
    }

    meta
}

/// Read SPA_META_Header from buffer.
///
/// # Safety
/// `spa_buf` must be a valid, non-null pointer to a dequeued spa_buffer.
unsafe fn read_header_meta(spa_buf: *const libspa_sys::spa_buffer) -> Option<HeaderMeta> {
    // SAFETY: spa_buf validity guaranteed by caller
    let ptr = unsafe {
        libspa_sys::spa_buffer_find_meta_data(
            spa_buf,
            libspa_sys::SPA_META_Header,
            std::mem::size_of::<libspa_sys::spa_meta_header>(),
        )
    };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is non-null and points to memory of at least spa_meta_header size
    let header = unsafe { &*(ptr as *const libspa_sys::spa_meta_header) };
    trace!(
        "SPA_META_Header: pts={}, dts_offset={}, seq={}, flags={:#x}",
        header.pts, header.dts_offset, header.seq, header.flags
    );
    Some(HeaderMeta {
        pts: header.pts,
        dts_offset: header.dts_offset,
        seq: header.seq,
        flags: header.flags,
    })
}

/// Read SPA_META_VideoTransform from buffer.
///
/// # Safety
/// `spa_buf` must be a valid, non-null pointer to a dequeued spa_buffer.
unsafe fn read_transform_meta(spa_buf: *const libspa_sys::spa_buffer) -> Option<u32> {
    // SAFETY: spa_buf validity guaranteed by caller
    let ptr = unsafe {
        libspa_sys::spa_buffer_find_meta_data(
            spa_buf,
            libspa_sys::SPA_META_VideoTransform,
            std::mem::size_of::<libspa_sys::spa_meta_videotransform>(),
        )
    };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is non-null and points to memory of at least spa_meta_videotransform size
    let transform = unsafe { &*(ptr as *const libspa_sys::spa_meta_videotransform) };
    debug!("SPA_META_VideoTransform: value={}", transform.transform);
    Some(transform.transform)
}

/// Read SPA_META_VideoCrop from buffer.
///
/// # Safety
/// `spa_buf` must be a valid, non-null pointer to a dequeued spa_buffer.
unsafe fn read_crop_meta(spa_buf: *const libspa_sys::spa_buffer) -> Option<CropRegion> {
    // SAFETY: spa_buf validity guaranteed by caller
    let ptr = unsafe {
        libspa_sys::spa_buffer_find_meta_data(
            spa_buf,
            libspa_sys::SPA_META_VideoCrop,
            std::mem::size_of::<libspa_sys::spa_meta_region>(),
        )
    };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is non-null and points to memory of at least spa_meta_region size
    let region = unsafe { &*(ptr as *const libspa_sys::spa_meta_region) };
    let r = &region.region;
    trace!(
        "SPA_META_VideoCrop: {}x{} at ({},{})",
        r.size.width, r.size.height, r.position.x, r.position.y
    );
    Some(CropRegion {
        x: r.position.x,
        y: r.position.y,
        width: r.size.width,
        height: r.size.height,
    })
}

/// Read SPA_META_VideoDamage from buffer.
///
/// This is an array of spa_meta_region entries. An invalid entry
/// (zero-sized region) marks the end of the array.
///
/// # Safety
/// `spa_buf` must be a valid, non-null pointer to a dequeued spa_buffer.
unsafe fn read_damage_meta(spa_buf: *const libspa_sys::spa_buffer) -> Vec<DamageRect> {
    let mut rects = Vec::new();

    // SAFETY: spa_buf validity guaranteed by caller
    let meta_ptr = unsafe { libspa_sys::spa_buffer_find_meta(spa_buf, libspa_sys::SPA_META_VideoDamage) };
    if meta_ptr.is_null() {
        return rects;
    }

    // SAFETY: meta_ptr is non-null, returned by spa_buffer_find_meta
    let meta = unsafe { &*meta_ptr };
    if meta.data.is_null() || meta.size == 0 {
        return rects;
    }

    let region_size = std::mem::size_of::<libspa_sys::spa_meta_region>();
    let max_regions = meta.size as usize / region_size;

    let regions = meta.data as *const libspa_sys::spa_meta_region;
    for i in 0..max_regions {
        // SAFETY: i < max_regions which is bounded by the allocated meta.size
        let region = unsafe { &*regions.add(i) };
        let r = &region.region;
        // Zero-size region marks end of array
        if r.size.width == 0 || r.size.height == 0 {
            break;
        }
        rects.push(DamageRect {
            x: r.position.x,
            y: r.position.y,
            width: r.size.width,
            height: r.size.height,
        });
    }

    if !rects.is_empty() {
        trace!("SPA_META_VideoDamage: {} regions", rects.len());
    }
    rects
}

/// Read SPA_META_Cursor from buffer.
///
/// # Safety
/// `spa_buf` must be a valid, non-null pointer to a dequeued spa_buffer.
unsafe fn read_cursor_meta(spa_buf: *const libspa_sys::spa_buffer) -> Option<CursorMeta> {
    // SAFETY: spa_buf validity guaranteed by caller
    let ptr = unsafe {
        libspa_sys::spa_buffer_find_meta_data(
            spa_buf,
            libspa_sys::SPA_META_Cursor,
            std::mem::size_of::<libspa_sys::spa_meta_cursor>(),
        )
    };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is non-null and points to memory of at least spa_meta_cursor size
    let cursor = unsafe { &*(ptr as *const libspa_sys::spa_meta_cursor) };
    // id=0 means no valid cursor data
    if cursor.id == 0 {
        return None;
    }
    trace!(
        "SPA_META_Cursor: id={}, pos=({},{}), hotspot=({},{}), bitmap_offset={}",
        cursor.id, cursor.position.x, cursor.position.y, cursor.hotspot.x, cursor.hotspot.y, cursor.bitmap_offset
    );
    Some(CursorMeta {
        id: cursor.id,
        position: (cursor.position.x, cursor.position.y),
        hotspot: (cursor.hotspot.x, cursor.hotspot.y),
        bitmap_offset: cursor.bitmap_offset,
    })
}

/// Maximum number of damage rectangles to request.
/// 16 is a reasonable upper bound — most compositors report fewer.
pub const MAX_DAMAGE_REGIONS: usize = 16;
