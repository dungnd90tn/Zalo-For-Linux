# v1.3.0

Nâng Electron **22.3.27 → 43.3.0** (Chromium 108 → 150, hết EOL), thêm gói `.deb`, và bổ sung chụp màn hình — tính năng chưa từng chạy trên Linux.

## Electron 43

Chromium 108 đã EOL từ 2023. Nâng cấp cần vá 6 chỗ trong bundle vì code viết cho hành vi Chromium cũ:

- **Origin mờ của `file://`** *(nguyên nhân gốc)* — Chromium mới cấp origin mờ cho tài liệu `file://`, nên `postMessage` cùng cửa sổ tới nơi với `origin === "null"` thay vì `"file://"`. Bộ kiểm tra origin âm thầm loại bỏ MessagePort → kênh DAL không mở → SQLite không khởi tạo → treo vĩnh viễn ở "Đang đăng nhập…" **không một thông báo lỗi nào**.
- **`DnsOverHttps`** — gọi như biến toàn cục nhưng không định nghĩa ở đâu trong `pc-dist`; luôn ném `ReferenceError`, chặn `mergeServicesMap_v3`.
- **`TrustedIdentity.updateStatus`** và **`upgradeRequired`** — treo không timeout; thêm timeout 5s.
- **`webContents.incrementCapturerCount`** — API bị gỡ ở Electron 23.
- **Key crypto qua contextBridge** — Node 24 từ chối TypedArray khác realm.

## Chụp màn hình (mới)

Upstream ship UI chụp màn hình dạng binary Qt riêng từng nền tảng (`ZaloCap.exe`, `ZaloHelper.app`) và **không có nhánh Linux**. Đã dựng helper thay thế nói đúng giao thức stdio của Zalo, chạy bằng Electron đã bundle nên không thêm runtime.

Chọn vùng + chú thích (khung, elip, mũi tên, bút, đánh dấu, chữ; 7 màu; undo), xuất ở độ phân giải thiết bị. Dùng `org.freedesktop.portal.Screenshot` — **chỉ hỏi quyền lần đầu**, sau đó im lặng (~0,7s/lần); `desktopCapturer` thì mở picker ở mọi lần gọi.

## Bảo mật

- **Ghi file tuỳ ý khi phục hồi backup (nghiêm trọng)** — tên file trong container ZDB4.0 ghép thẳng vào thư mục đích. Đã chứng minh bản cũ ghi được file ra ngoài qua `../` và đường dẫn tuyệt đối. Nay qua `safe_join_under()`, kèm bounds-check header.
- **Sandbox Chromium** — bỏ `--no-sandbox` vô điều kiện cho distro nền Debian; sandbox chạy tốt trên Ubuntu 24.04+. `.deb` cài `chrome-sandbox` setuid root.
- **Ghim version + SHA-256** cho mọi thứ tải về; GitHub Actions ghim theo commit SHA.
- **`/tmp` đoán trước được** — thay bằng `mktemp`; trước đây có đường dẫn cố định rồi **đem thực thi**.
- Tắt auto-download/auto-install của `electron-updater`.

## Đóng gói

- Gói **`.deb`** mới (`/opt/zalo`), icon 10 kích thước, phụ thuộc `libsecret-1-0`, `python3-gi`.
- AppImage build được trên **Ubuntu 24.04+**: `appimagetool` được duy trì (binary tĩnh) + runtime FUSE3, không cần `libfuse2`.

## Lưu ý

Nâng Electron major sẽ ghi lại schema IndexedDB trong `~/.config/ZaloData`; lần đầu chạy có thể phải đăng nhập lại.
