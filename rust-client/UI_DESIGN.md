# Thiết kế UI native Zalo

## Mục đích và phạm vi

Tham chiếu cho agent sửa UI Rust: giữ diện mạo Zalo, đúng logic và phản hồi nhanh. “Hiện tại” mô tả mã nguồn; “Yêu cầu” là điều kiện chấp nhận, không chứng nhận tính năng hoàn tất.

Client dùng egui/eframe native, không WebView/Electron. Không sao chép mã hoặc tài sản từ client Rust khác. Mục tiêu vẫn là tương đương bản Electron Linux đang hoạt động; preview chưa đạt đầy đủ. Theo dõi khoảng trống tại [FEATURE_PARITY.md](FEATURE_PARITY.md).

## Nguồn chuẩn và theme

[UI_THEME.md](UI_THEME.md) là nguồn đối chiếu theme, selector CSS và provenance tài sản; [ARCHITECTURE.md](ARCHITECTURE.md) quy định kiến trúc. Áp dụng tại [visuals.rs](src/ui/visuals.rs). Khi có khác biệt, kiểm tra nguồn thay vì tự chọn màu gần giống.

| Vai trò | Giá trị Light |
| --- | --- |
| Thanh điều hướng / mục chọn | `#005AE0` / `#00378A` |
| Nút chính | `#0068FF` |
| Chữ chính / phụ | `#081B3A` / `#5A6981` |
| Placeholder / disabled | `#77859C` / `#8B96A7` |
| Nền chat / nền trắng | `#E5E7EB` / `#FFFFFF` |
| Tin gửi, vùng chọn / hover | `#E5F1FF` / `#F1F2F4` |
| Viền | Đen, alpha `26/255` |

Font Segoe UI regular/Semibold/Bold và hai icon font gốc khởi tạo một lần; metadata 12, body/button 14, nội dung chat 15, heading 18, tiêu đề modal 16 đơn vị logic egui. Không dùng chữ phụ 9–11,5 để tiết kiệm chỗ. Catalog giữ toàn bộ token và 1.034 glyph/alias đã kiểm cmap; raster chỉ khi dùng. [Kiểm kê](UI_ASSET_INVENTORY.md) phân biệt nguồn đầy đủ với thành phần native.

Cài đặt mở từ rail hoặc màn đăng nhập: Sáng/Tối/Hệ thống/Tùy chỉnh, lưu riêng trong profile bằng worker. System theo tín hiệu egui/winit; thiếu tín hiệu dùng Light, chưa xác nhận mọi desktop Linux. Custom kế thừa Light, ghi đè màu/reset; gradient hai điểm màu hỗ trợ nền chat, rail và surface. Token/editor và danh mục icon nằm trong Nâng cao, đóng mặc định; không tự bật tính năng chưa hỗ trợ.

## Bố cục và thành phần

Hiện tại, [ui.rs](src/ui.rs) tổ chức rail, danh sách và khung chat. Rail rộng 64; danh sách mặc định 344, kéo được trong khoảng 280–420; header chat cao 68.

Dưới chiều rộng 940, danh sách/danh bạ chuyển thành màn riêng; nút quay lại giữ hội thoại và bản nháp. Vùng nội dung cửa sổ mặc định 1280×720 đơn vị logic (16:9), tối thiểu 640×480. Composer cao 100 khi chiều cao còn lại dưới 340, ngược lại 132.

Thông tin hội thoại nằm ở sidebar phải rộng 344 khi còn đủ ít nhất 360 cho chat. Khi hẹp, trang thông tin thay khung chat; quay lại giữ đúng hội thoại/bản nháp. Nội dung chỉ dùng dữ liệu đã lưu, đóng khi đổi tài khoản/thread; không dựng nút thao tác chưa được nối.

Resize/maximize cùng DPI chỉ tăng vùng đọc, không nhân cỡ chữ hay chiều rộng rail/list/sidebar, chiều cao header hoặc hitbox. Ảnh/sticker giữ tỉ lệ, có giới hạn kích thước và không phóng vượt số pixel nguồn; viewer căn ảnh giữa hai chiều trong khung tối đa 740×540, chỉ thu nhỏ khi cửa sổ thiếu chỗ; chú thích cuộn riêng để liên kết ảnh gốc vẫn tới được. Khi DPI màn hình đổi, toolkit raster lại font theo mật độ pixel thật; không ép zoom/DPI toàn ứng dụng hoặc kéo giãn bitmap.

Modal dùng `dialog.rs`: curtain theo token gốc, header cao 48, tiêu đề 16, glyph đóng 24 trong hitbox 30, viền bo 4, body cuộn có padding 14×16. Footer là hàng nút cao 32 cộng padding, không kéo giãn theo chiều cao cửa sổ. Khi modal mở, chặn cả pointer, phím tắt và dispatch nền; đóng modal chỉ khôi phục focus khi tài khoản/thread/generation còn khớp. Settings, mở hội thoại, đăng xuất, kiểm tra gửi và thông tin ứng dụng dùng cùng quy tắc.

| Thành phần | Nơi sửa chính |
| --- | --- |
| Đăng nhập, QR, thử lại | [login.rs](src/ui/login.rs), [live.rs](src/ui/live.rs) |
| Danh bạ, nhóm, phân trang | [directory.rs](src/ui/directory.rs) |
| Tên người gửi | [senders.rs](src/ui/senders.rs) |
| Lượt gửi chưa rõ kết quả | [delivery.rs](src/ui/delivery.rs) |
| Thumbnail nhãn dán | [stickers.rs](src/ui/stickers.rs) |
| Ảnh, thẻ OA và viewer | [media.rs](src/ui/media.rs), [worker ảnh](src/app/media.rs) |

Yêu cầu: tiêu đề dài phải rút gọn, có thể xem tên đầy đủ qua hover; nội dung tin vẫn xuống dòng. Composer và nút gửi không tràn khi resize. Control phải thực hiện hành vi đã nối hoặc thực sự disabled kèm giải thích; không tạo biểu tượng bấm được nhưng không làm gì.

## Luồng đăng nhập và kết nối

Hiện tại, mở `--live` tự gửi lệnh startup đến worker. Worker đọc phiên: hợp lệ thì xác minh và mở chat; chưa có phiên thì tạo QR. Chỉ xác nhận hết hiệu lực đăng nhập hoặc yêu cầu đổi danh tính client mới chuyển sang QR mới. Lỗi mạng, HTTP, cấu hình hoặc lưu trữ giữ phiên để thử lại.

Hủy/đăng xuất không tự chạy lại startup trong cửa sổ hiện tại. Generation loại bỏ kết quả cũ. Đăng nhập số điện thoại chưa hỗ trợ; tab hiện chỉ giải thích giới hạn, giữ lần QR đang chạy và cho phép hủy. Nhóm nút dưới QR có khung hữu hạn, căn cùng trục giữa với mã QR; thao tác chuyển tab không bắt đầu một phiên xác thực khác.

Trạng thái Online chỉ đến từ xác thực transport. Khi Offline, vẫn cho đọc cache, chọn liên hệ và soạn nháp. Banner phải nêu nguyên nhân an toàn, có kết nối lại/đăng xuất; không suy diễn online của từng người. Lỗi batch lưu dữ liệu tạm dừng transport, giữ lịch sử/cursor và thao tác cục bộ.

## Tin nhắn và danh tính

Hiện tại, tin đến nằm trái, tin của tài khoản nằm phải; thời gian/ngày phân tách dùng dữ liệu đã lưu. Nhận ACK từ máy chủ không đồng nghĩa người nhận đã đọc.

Thông báo `webchat` / `msginfo.actionlist` hiển thị văn bản tiếng Việt từ template, tên gợi ý và tiêu đề gốc. Toàn bộ cấu trúc được giữ riêng trong SQLite, kể cả action và trường chưa biết; UI hiện chỉ đọc nội dung, chưa thực thi action hoặc tải URL/ảnh trong thông báo. Không biến object thành JSON thô hay placeholder làm mất dữ liệu, và không dùng sự kiện thông báo để xác nhận tin gửi.

Ảnh nhận có thumbnail, tiêu đề/chú thích và viewer native dùng pixel đã cache; ảnh gốc chỉ mở trình duyệt khi bấm. Thẻ OA dùng preview tiếng Việt đã đối chiếu nguồn và metadata; mẫu remote chưa biết phải giải thích giới hạn, giữ toàn bộ dữ liệu, không chạy JavaScript/action. History snapshot chỉ chứa projection tối đa 16 KiB; raw content thuộc worker/SQLite. Tham chiếu [PROTOCOL_MEDIA.md](PROTOCOL_MEDIA.md) cho codec/CDN và điều kiện làm giàu placeholder cũ.

`chat.recommended` dùng thẻ native đọc nội dung chia sẻ; chỉ action liên kết đã đối chiếu nguồn cho phép mở trình duyệt khi bấm. `group.poll` dùng glyph bình chọn gốc, thông báo/câu hỏi có trong tin và trạng thái ẩn danh khi nguồn có boolean rõ ràng. Chưa tải chi tiết lựa chọn/kết quả hay gửi phiếu; không tạo số liệu hoặc nút bỏ phiếu giả. Metadata mới cập nhật đúng tin trên trang đang mở, giữ thứ tự, vị trí trang và bản nháp.

Tên lấy từ cache danh bạ đúng tài khoản. DM chỉ dùng tiêu đề hội thoại khi thread và người gửi khớp. Nhóm không dùng tên nhóm làm tên thành viên. Chưa biết tên thì hiện “Người gửi”, avatar `?`; không thay tên bằng ID kỹ thuật. Avatar hiện là chữ viết tắt, chưa tải ảnh hồ sơ; cùng tên dùng cùng hàm tạo avatar ở header và bubble.

Tra tên dùng snapshot có scope tài khoản/trang/thread và epoch riêng. Chỉ một lookup đang chờ; cập nhật danh bạ được gộp, bỏ câu trả lời cũ và giữ nguyên tin/bản nháp. Không đổi `Message.sender`, ID hoặc điều kiện bảo mật để sửa hiển thị.

## Nhập liệu và khả năng tiếp cận

Toàn vùng nhập của composer nhận focus; nội dung nhiều dòng cuộn bên trong. Enter xuống dòng, Ctrl+Enter gửi khi đủ điều kiện. Bản nháp, cursor/undo và IME gắn với tài khoản/hội thoại, không bị xóa khi đổi theme.

Bản vá [egui-winit](vendor/egui-winit/LINUX_IME_PATCH.md) nhận preedit/commit Linux, không để context Enabled khóa Backspace/phím mũi tên; đổi editor tạo lại context. Không gửi trong composition, frame commit hoặc do phím giữ lặp sau commit.

Yêu cầu: kiểm thử bằng x-unikey thật, không chỉ chèn Unicode. Kiểm tra Tab, focus sau đổi cửa sổ/hội thoại, Backspace, chuột và nội dung tiếng Việt. Duy trì chữ dễ đọc, trạng thái disabled phân biệt được; đo tương phản nếu đổi token. Chưa có chứng nhận WCAG hoặc kiểm thử screen reader đầy đủ.

## Dữ liệu bất đồng bộ và ngân sách

UI chỉ gửi command, nhận event đã kiểm tra scope và vẽ. HTTP/WSS, SQLite, đọc phiên và giải mã ảnh thuộc worker. Tin/preview chỉ công bố sau lưu thành công; ACK chưa lưu được phải giữ cảnh báo không tự gửi lại.

| Giới hạn hiện tại | Giá trị |
| --- | --- |
| Hàng đợi command / event messaging | 16 / 4 |
| Drain UI live mỗi frame | Tối đa 8 event, ngân sách 3 ms |
| Tin trong pane / sidebar | 50 / tối đa 200 hội thoại |
| Tra tên người gửi | 64 ID/lượt; tên tối đa 512 byte |
| Danh bạ | 100 dòng/trang; SQLite tối đa 30.000 dòng |
| Texture sticker | 64 mục; payload RGBA 8 MiB |
| Cache sticker phía worker | 64 mục; payload RGBA 8 MiB |
| Cache ảnh media phía worker / GPU | Mỗi bên 32 mục; payload RGBA 16 MiB |
| Hàng đợi request / result ảnh media | 16 / 2; một decoder toàn process |

Nhãn dán hiện chỉ có thumbnail PNG tĩnh; ảnh hiển thị tối đa 130 đơn vị, lỗi có fallback. Tải ở nền, chỉ cho live/ảnh cần hiển thị, không mang cookie sang CDN. Demo không truy cập Internet. Giữ backoff 30 giây cho tải lỗi; không tải lại mỗi frame hoặc giữ RGBA sau upload GPU nếu không cần.

Yêu cầu: không thêm SQL/network vào render, cache không giới hạn hoặc repaint liên tục để chờ I/O. Ngân sách payload không đo tổng RAM/VRAM; đo trước khi so với Electron.

## Bằng chứng và nghiệm thu

Mốc tên người gửi đã qua kiểm tra kỹ thuật và ảnh GUI tổng hợp; kết quả cập nhật trong [VALIDATION.md](VALIDATION.md). Điều này không thay thế nghiệm thu tài khoản thật. X11/IBus có kiểm thử Telex ON/OFF và chuyển focus; native Wayland chưa được xác nhận tương đương.

GUI không mạng dùng `--demo` hoặc fixture TLS cục bộ; `--live` tự đăng nhập.

Khi sửa UI:

1. Đọc tài liệu nguồn và quy tắc agent; ghi rõ hành vi thay đổi, phần chưa hỗ trợ.
2. Thêm kiểm thử cho logic có rủi ro: scope, stale event, ACK, bản nháp, IME; không chỉ kiểm tra ảnh không trắng.
3. Chạy kiểm tra phù hợp theo `VALIDATION.md`, giữ formatting và Clippy sạch.
4. Chụp riêng cửa sổ 1200×780 và 640×480 bằng hồ sơ tổng hợp; kiểm tra QR/retry, DM/nhóm, tên dài/không rõ tên, typing và resize.
5. Ghi điều kiện, kết quả, giới hạn; không dùng dữ liệu tài khoản thật trong ảnh/log/commit.

Ảnh tổng hợp tại `/tmp/zalo-rust-gui-check/`; tái tạo theo `VALIDATION.md`.

Các khoảng trống nghiệp vụ nằm tại `FEATURE_PARITY.md`; không suy diễn mức hoàn tất từ icon/theme.
