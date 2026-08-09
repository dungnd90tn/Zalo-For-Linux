# v1.3.2

Sửa lỗi ảnh trong tin nhắn không hiển thị trên Electron 43 — chỉ ra nút tải xuống, bấm vào cũng không load. Đây là hồi quy của bản v1.3.1.

## Nguyên nhân

Zalo thương lượng **JPEG XL** cho ảnh tin nhắn: client gắn `?jxlstatus=1`, CDN trả `content-type: image/jxl`, nên thứ nằm trong `media/<uid>/ZaloDownloads/picture/` là `.jxl` chứ không phải `.jpg`.

Upstream trông cậy vào Chromium để giải mã — `main.js` gọi `appendSwitch("enable-features", "JXL")` vô điều kiện — và chỉ dùng addon `zjxl` khi Chromium không lo được.

**Chromium bỏ JPEG XL từ bản 110.** Trên Electron 22 (Chromium 108) cờ đó còn tác dụng; trên Electron 43 (Chromium 150) nó là no-op, đã kiểm chứng cả khi bật lẫn khi tắt. Mà upstream chỉ ship `zjxl` dạng Mach-O/PE, nên Linux mất cả hai bộ giải mã. Mỗi ảnh chat rơi về placeholder tải xuống, và bấm tải lại chỉ lấy về đúng file `.jxl` không giải mã được.

## `zjxl` cho Linux (mới)

`generate-jxl-addon.py` dựng addon N-API còn thiếu, theo cùng khuôn mẫu nhúng C++ như `generate-addon.py`, cung cấp đủ 7 entry point mà bundle gọi với hợp đồng `(error, data, status_code)` và `SUCCESS_STATUS = 1`.

- libjxl, các phụ thuộc bắc cầu và libjpeg-turbo được **chép cạnh `jxl.node`**, link bằng **`DT_RPATH` (`--disable-new-dtags`) chứ không phải `DT_RUNPATH`** — chỉ tag cũ mới được kế thừa khi phân giải phụ thuộc của phụ thuộc, mà `libjxl.so` còn kéo theo `libjxl_cms`/`libhwy`/brotli. Dùng `DT_RUNPATH` thì những cái đó rơi về bản hệ thống và addon hỏng trên mọi distro không có libjxl khớp phiên bản. Cả 9 thư viện đều phân giải trong thư mục addon.
- libjpeg link động: `libjpeg.a` của Ubuntu build không có `-fPIC`, không nhét vào shared object được.

Kèm một patch bundle: `regenImageFromUrlUsingNativeElectron` ném thẳng byte nguồn vào `createImageBitmap`, gặp JXL là `DOMException`. Nay nó nhận diện chữ ký JXL (`ff 0a`, hoặc box ISOBMFF `JXL `) rồi đi qua `decodeToJpeg` trước.

## Cách rẻ hơn đã thử và đã loại

Ép `jxl.enable_convertible = false` để client xin `/gr/jpg/…` thay vì `/gr/jxl/…`. Cờ có hiệu lực thật trong app đang chạy, và hàm đổi URL trả về đúng link JPEG tồn tại thật (HTTP 200) — **nhưng app vẫn tải `.jxl`**. Gắn stack-trace vào đúng hàm gắn `jxlstatus=1` cho thấy nó **không được gọi lần nào**: các URL ấy đã nằm sẵn trong kho tin nhắn từ những lần đồng bộ trước. Sai tầng, đã revert sạch.

## Kết quả đo

Cùng một hội thoại, cùng profile, trước và sau:

| | trước | sau |
|---|---|---|
| `regen with native electron error` | 8 | **0** |
| thumbnail sinh ra | 1 | **7** |
| ảnh render trong khung chat | 2/8 | **8/8** |

Giải mã trong chính app: JXL 1290×2796 (126 KB) → JPEG 300×650 (31,8 KB), khoảng 19 ms.

## Lưu ý

Không thêm phụ thuộc hệ thống nào — thư viện đi kèm trong gói. Ảnh `.jxl` đã tải về từ trước hiển thị được ngay, không cần tải lại.

README đã viết lại theo đúng hiện trạng dự án: bổ sung bản `.deb` và tính năng chụp màn hình, sửa phiên bản Electron, thêm phần bảo mật và xử lý sự cố.
