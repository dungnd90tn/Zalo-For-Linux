//! A file the user chose to send, checked before anything is sent or stored. Inspecting a path
//! touches the filesystem, so callers run [`Attachment::inspect`] on a background thread.
use std::path::{Path, PathBuf};

/// The original client's `small_file_size_limit`: larger files go through Zalo Cloud, which this
/// client does not implement.
pub const MAX_DIRECT_FILE_BYTES: u64 = 104_857_600;
/// Longest accepted file name, in bytes (the common filesystem limit).
pub const MAX_FILE_NAME_BYTES: usize = 255;
const MAX_EXTENSION_CHARS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentError {
    Missing,
    NotAFile,
    Empty,
    TooLarge,
    BadName,
    Unreadable,
}
impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "Không tìm thấy tệp đã chọn.",
            Self::NotAFile => "Chỉ gửi được tệp thông thường, không phải thư mục hay thiết bị.",
            Self::Empty => "Tệp trống nên không thể gửi.",
            Self::TooLarge => "Tệp lớn hơn 100 MB. Tệp lớn cần Zalo Cloud, bản native chưa hỗ trợ.",
            Self::BadName => "Tên tệp không hợp lệ hoặc quá dài.",
            Self::Unreadable => "Không đọc được tệp đã chọn.",
        })
    }
}
impl std::error::Error for AttachmentError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub path: PathBuf,
    /// File name without directories, exactly as it will be shown and sent.
    pub name: String,
    /// Lowercase extension without the dot; empty when there is none.
    pub extension: String,
    pub size_bytes: u64,
}

impl Attachment {
    /// Check `path` and read its size. Follows symbolic links like a file dialog does, but only
    /// a regular file qualifies; the file content is not read.
    pub fn inspect(path: &Path) -> Result<Self, AttachmentError> {
        let metadata = std::fs::metadata(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => AttachmentError::Missing,
            _ => AttachmentError::Unreadable,
        })?;
        if !metadata.is_file() {
            return Err(AttachmentError::NotAFile);
        }
        let size_bytes = metadata.len();
        if size_bytes == 0 {
            return Err(AttachmentError::Empty);
        }
        if size_bytes > MAX_DIRECT_FILE_BYTES {
            return Err(AttachmentError::TooLarge);
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AttachmentError::BadName)?
            .to_owned();
        if name.is_empty()
            || name.len() > MAX_FILE_NAME_BYTES
            || name.chars().any(char::is_control)
            || name.contains(['/', '\\'])
        {
            return Err(AttachmentError::BadName);
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_lowercase)
            .filter(|extension| extension.chars().count() <= MAX_EXTENSION_CHARS)
            .unwrap_or_default();
        Ok(Self {
            path: path.to_owned(),
            name,
            extension,
            size_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zalo-attachment-{tag}-{}",
            crate::domain::local_message_id().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }

    #[test]
    fn a_regular_file_reports_its_name_extension_and_size() {
        let dir = temp_dir("ok");
        let path = write(&dir, "Báo-Cáo.PDF", &[7; 2048]);
        let attachment = Attachment::inspect(&path).unwrap();
        assert_eq!(attachment.name, "Báo-Cáo.PDF");
        assert_eq!(attachment.extension, "pdf", "extensions are lowercase");
        assert_eq!(attachment.size_bytes, 2048);
        assert_eq!(attachment.path, path);
        let bare = Attachment::inspect(&write(&dir, "README", b"x")).unwrap();
        assert_eq!(bare.extension, "");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn anything_but_a_non_empty_regular_file_within_the_limit_is_refused() {
        let dir = temp_dir("bad");
        assert_eq!(
            Attachment::inspect(&dir.join("absent.txt")),
            Err(AttachmentError::Missing)
        );
        assert_eq!(Attachment::inspect(&dir), Err(AttachmentError::NotAFile));
        assert_eq!(
            Attachment::inspect(&write(&dir, "empty.txt", b"")),
            Err(AttachmentError::Empty)
        );
        // A sparse file reaches the size check without writing 100 MB.
        let big = dir.join("big.bin");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_DIRECT_FILE_BYTES + 1).unwrap();
        assert_eq!(Attachment::inspect(&big), Err(AttachmentError::TooLarge));
        file.set_len(MAX_DIRECT_FILE_BYTES).unwrap();
        assert_eq!(
            Attachment::inspect(&big).unwrap().size_bytes,
            MAX_DIRECT_FILE_BYTES,
            "exactly the limit is allowed"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_with_control_characters_or_excess_length_are_refused() {
        let dir = temp_dir("names");
        let long = write(&dir, &format!("{}.txt", "a".repeat(200)), b"x");
        assert_eq!(Attachment::inspect(&long).unwrap().name.len(), 204);
        // 256 bytes is one too many for most filesystems, so the path cannot exist; the check
        // is on the name itself.
        let too_long = dir.join("é".repeat(128));
        assert!(matches!(
            Attachment::inspect(&too_long),
            Err(AttachmentError::Missing | AttachmentError::Unreadable | AttachmentError::BadName)
        ));
        let control = write(&dir, "tab\there.txt", b"x");
        assert_eq!(Attachment::inspect(&control), Err(AttachmentError::BadName));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_symbolic_link_to_a_regular_file_is_followed_but_one_to_a_directory_is_not() {
        let dir = temp_dir("links");
        let target = write(&dir, "real.txt", b"hello");
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let attachment = Attachment::inspect(&link).unwrap();
        assert_eq!(
            (attachment.name.as_str(), attachment.size_bytes),
            ("link.txt", 5)
        );
        let dir_link = dir.join("dir-link");
        std::os::unix::fs::symlink(&dir, &dir_link).unwrap();
        assert_eq!(
            Attachment::inspect(&dir_link),
            Err(AttachmentError::NotAFile)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
