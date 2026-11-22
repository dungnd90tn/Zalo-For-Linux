//! Opaque previewed photo capability with an immutable worker-owned upload spool.
use crate::domain::PhotoResolution;
use std::{
    fs::File,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
pub(super) const MAX_PREPARED_BYTES: u64 = 32 * 1024 * 1024;
pub(super) static PREPARED_BYTES: AtomicU64 = AtomicU64::new(0);
pub(super) struct CacheCharge {
    counter: &'static AtomicU64,
    bytes: u64,
}
impl CacheCharge {
    pub fn reserve(
        counter: &'static AtomicU64,
        encoded: u64,
        rgba: usize,
        limit: u64,
    ) -> Result<Self, PhotoPreparationFailure> {
        let bytes = encoded
            .checked_add(rgba as u64)
            .ok_or(PhotoPreparationFailure::Resource)?;
        loop {
            let used = counter.load(Ordering::Acquire);
            let next = used
                .checked_add(bytes)
                .filter(|next| *next <= limit)
                .ok_or(PhotoPreparationFailure::Resource)?;
            if counter
                .compare_exchange(used, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }
        Ok(Self { counter, bytes })
    }
}
impl Drop for CacheCharge {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Clone)]
pub struct PhotoThumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
impl std::fmt::Debug for PhotoThumbnail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhotoThumbnail")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
pub(super) struct PhotoData {
    pub file: File,
    pub resolution: PhotoResolution,
    pub checksum: String,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub source_size: u64,
    pub thumbnail: PhotoThumbnail,
    pub account: String,
    pub thread: String,
    pub lease: u64,
    pub _charge: CacheCharge,
}
#[derive(Clone)]
pub struct PreparedPhoto(pub(super) Arc<PhotoData>);
impl PartialEq for PreparedPhoto {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for PreparedPhoto {}
impl std::fmt::Debug for PreparedPhoto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedPhoto")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("size_bytes", &self.size_bytes())
            .finish_non_exhaustive()
    }
}
impl PreparedPhoto {
    pub fn resolution(&self) -> PhotoResolution {
        self.0.resolution
    }
    pub fn width(&self) -> u32 {
        self.0.width
    }
    pub fn height(&self) -> u32 {
        self.0.height
    }
    pub fn size_bytes(&self) -> u64 {
        self.0.size
    }
    pub fn thumbnail(&self) -> &PhotoThumbnail {
        &self.0.thumbnail
    }
    pub fn same_instance(&self, other: &Self) -> bool {
        self == other
    }
    pub(super) fn fits_limits(&self, limits: crate::protocol::bootstrap::PhotoLimits) -> bool {
        self.width() <= limits.width
            && self.height() <= limits.height
            && self.size_bytes() <= limits.size
            && self.0.source_width <= limits.input_width
            && self.0.source_height <= limits.input_height
            && self.0.source_size <= limits.input_size
            && (self.resolution() == PhotoResolution::Hd || self.0.source_size <= limits.size)
    }
    pub(super) fn permits(&self, account: &str, thread: &str, lease: u64) -> bool {
        self.0.account == account && self.0.thread == thread && self.0.lease == lease
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoPreparationFailure {
    Cancelled,
    Unavailable,
    InvalidFormat,
    OriginalFormat,
    OriginalOrientation,
    Animated,
    Resource,
    Changed,
    Storage,
}
impl std::fmt::Display for PhotoPreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "Đã hủy chuẩn bị ảnh.",
            Self::Unavailable => "Phiên này chưa hỗ trợ gửi ảnh gốc.",
            Self::InvalidFormat => "Chỉ hỗ trợ ảnh JPEG, PNG hoặc WebP tĩnh hợp lệ.",
            Self::OriginalFormat => {
                "Ảnh gốc cần là JPEG tĩnh hợp lệ. Bạn có thể gửi ảnh dưới dạng tệp."
            }
            Self::OriginalOrientation => {
                "Ảnh JPEG có thông tin xoay hoặc lật. Hãy chọn ảnh thường hoặc gửi dưới dạng tệp."
            }
            Self::Animated => "Ảnh động chưa được hỗ trợ; chọn gửi dưới dạng tệp.",
            Self::Resource => {
                "Ảnh vượt giới hạn xử lý hoặc bộ nhớ bản nháp; hãy bỏ ảnh không dùng."
            }
            Self::Changed => "Ảnh đã thay đổi sau khi chọn; hãy chọn lại.",
            Self::Storage => "Chưa tạo được bản xem trước trên thiết bị.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn tiny_jpegs_still_charge_the_nonempty_rgba_preview_and_cancelled_leases_release() {
        let mut leases = Vec::new();
        let bytes = 512 + 256 * 256 * 4;
        while let Ok(lease) = CacheCharge::reserve(&COUNTER, 512, 256 * 256 * 4, MAX_PREPARED_BYTES)
        {
            leases.push(lease);
        }
        assert_eq!(COUNTER.load(Ordering::Acquire), leases.len() as u64 * bytes);
        assert!(
            leases.len() < 128,
            "Tiny JPEGs cannot accumulate unbounded retained previews"
        );
        drop(leases.pop());
        let replacement =
            CacheCharge::reserve(&COUNTER, 512, 256 * 256 * 4, MAX_PREPARED_BYTES).unwrap();
        drop(replacement);
        drop(leases);
        assert_eq!(COUNTER.load(Ordering::Acquire), 0);
        {
            let _cancelled_unpublished =
                CacheCharge::reserve(&COUNTER, 512, 1024, MAX_PREPARED_BYTES).unwrap();
        }
        assert_eq!(COUNTER.load(Ordering::Acquire), 0);
    }
}
