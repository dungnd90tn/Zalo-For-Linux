# Changelog

Nhật ký thay đổi của **Zalo Native** (ứng dụng Rust trong `rust-client/`). Định dạng theo
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/); phiên bản theo `Cargo.toml`.
Ghi chú phát hành dành cho người dùng nằm trong [changelogs/](../changelogs/RELEASE-v2.0.0.md).

## [2.0.0] - 2026-11-06

### Thay đổi có thể làm hỏng thói quen
- **Phím gửi tin:** `Enter` gửi tin; `Ctrl+Enter` (hoặc `Shift+Enter`) xuống dòng. Trước đây `Enter`
  xuống dòng và `Ctrl+Enter` gửi. `Enter` không bao giờ chèn dòng mới, và không gửi khi bộ gõ (IME)
  đang soạn, trong cùng khung với lần commit, hoặc khi giữ phím lặp.
- Số phiên bản đổi từ `0.1.0` sang `2.0.0`; `zalo-rust --version` và các gói cài đặt dùng cùng số này.

### Thêm mới
- **Giao diện thiết kế lại, chế độ Sáng và Tối** ([hướng dẫn](UI_REDESIGN_GUIDE.md)):
  - bảng màu mới trong `src/theme/redesign.rs`, đặt trên danh mục token gốc; Tùy chỉnh vẫn ghi đè được;
  - rail 68 px với nút bo tròn 48 px và nút đổi nhanh **Sáng/Tối** (nhãn chữ, vì chưa có glyph gốc);
  - panel danh sách có tiêu đề, ô tìm kiếm 44 px, chip lọc, hàng hội thoại 72 px;
  - header chat 76 px, bong bóng góc bất đối xứng (tin gửi nền xanh, chữ trắng), đường phân ngày dạng viên thuốc;
  - composer dạng thẻ bo 16 px; nút biểu tượng 44 px; modal bo 16 px;
  - màn Danh bạ, sidebar thông tin và đăng nhập theo cùng ngôn ngữ thiết kế; bố cục gọn cho cửa sổ nhỏ.
- **Gói cài đặt `.deb` và `.rpm`** (`scripts/rust/package.sh`): cài bằng `apt`/`dnf`/`yum`,
  biên dịch trong Ubuntu 20.04 (glibc 2.31), có kiểm tra cài/gỡ trong container sạch
  (`scripts/rust/package/verify.sh`).
- Công cụ chụp ảnh giao diện từ framebuffer của chính ứng dụng (`ui/shot_fixture.rs`, bài kiểm thử
  `--ignored`); không chụp màn hình desktop.
- Các vai trò màu mới trong `Palette`: `text_tertiary`, `border_strong`, `accent_soft`, `accent_ink`,
  `chip_on_bg`, `chip_on_text`; bộ 6 màu avatar.

- **Sticker động và nhiều định dạng:** nhận dạng định dạng theo chữ ký tệp (không tin `Content-Type`):
  PNG tĩnh, **sprite PNG nhiều khung** (2–64 khung), JPEG, WebP tĩnh/động và GIF. Sticker động chạy
  theo đồng hồ khung, đồng bộ giữa mọi bản sao trên màn hình, chỉ chạy khi cửa sổ đang được lấy nét.
  Tải tài nguyên sprite trước rồi mới lùi về ảnh tĩnh.
- **Tin sticker không có bong bóng:** chỉ còn hình sticker (không nền, không viền, không đệm), rộng vừa
  khít hình thay vì rộng bằng cả bong bóng.
- **Thông tin hội thoại đầy đủ hơn:** hành động nhanh (Sao chép tên; Tắt thông báo và Ghim hiện
  nhưng tắt, vì cài đặt đó nằm trên máy chủ Zalo), số tin nhắn và khoảng thời gian đã lưu, cùng ba tab
  **Ảnh / Tệp / Liên kết** liệt kê các mục từng xuất hiện trong lịch sử đã lưu (50 mục gần nhất, kèm tổng
  số; liên kết trong tin nhắn thường được nhận dạng và loại trùng). Dữ liệu đọc ở worker, tôn trọng
  hội thoại ẩn.
- CI (`.github/workflows/build-packages.yml`) build gói `.deb`/`.rpm` của ứng dụng Rust khi phát hành,
  thay cho bản build AppImage Electron; có kiểm tra thẻ phát hành khớp phiên bản và kiểm cài đặt bằng
  `apt`/`dnf` trước khi tải lên.

### Sửa
- Sticker động từng có nguy cơ làm ứng dụng thoát vì vượt giới hạn kích thước texture của egui; nay mọi
  animation được xếp thành atlas ≤ 2048 px mỗi cạnh (xuống hàng khi cần) và kiểm tra thêm giới hạn thật
  của driver đồ họa.
- Giờ gửi và trạng thái "Đã lưu trên máy" trong bong bóng nền xanh bị mờ ở chế độ Sáng (tương phản 3,4:1);
  nay dùng chữ trắng đầy đủ (5,1:1).
- Cài đặt màu tùy chỉnh hiển thị đúng giá trị mặc định của bảng màu mới thay vì giá trị gốc.

### Kiểm thử
- Thêm kiểm thử độ tương phản WCAG AA cho các cặp chữ/nền chính ở cả hai chế độ, kiểm thử giá trị token,
  kiểm thử hình học (rail, header, composer, nút gửi) và kiểm thử phím `Enter`/`Ctrl+Enter`/`Shift+Enter`
  kèm các rào IME.

### Giới hạn đã biết
- Sticker sprite phát với tốc độ cố định 100 ms/khung vì `duration` thật nằm ở API chi tiết sticker
  cần xác thực; WebP/GIF động giữ nhịp riêng. Sticker dạng gói zip (hiệu ứng) chưa hỗ trợ.
- Danh sách ảnh trong thông tin hội thoại chưa có ảnh thu nhỏ; hàng mở ảnh gốc trong trình duyệt khi bấm.
- Chip **Chưa đọc** hiện bị tắt và chưa có huy hiệu số tin chưa đọc, vì dữ liệu hội thoại chưa lưu trạng thái này.
- Bong bóng gửi dạng ảnh, nhãn dán và thẻ giữ nền sáng để chữ nhãn bên trong vẫn đủ tương phản.
- Cửa sổ thấp (dưới 600 px) dùng nút đăng nhập cao 36 px thay vì 44 px.
- Chưa kiểm với tài khoản thật và chưa kiểm bộ gõ (IME) trên Wayland native; mọi bằng chứng trong bản này là dữ liệu
  giả lập. Gói `.deb`/`.rpm` được kiểm cài và gỡ trong container, chưa kiểm trên máy thật.
- Tối thiểu glibc 2.31, chỉ x86_64. Đã kiểm cài/gỡ trên Ubuntu 20.04/22.04/24.04, Debian 12, Fedora 42 và AlmaLinux 9; Debian 11 chưa kiểm được vì kho apt của bản đã hết hạn trả 404.

## [0.1.0]

Bản thử nghiệm ban đầu: giao diện `egui` theo bộ màu Zalo gốc, đăng nhập QR, lưu SQLite, hội thoại ẩn, ảnh xem trước.
Lịch sử chi tiết ở [MIGRATION_STATUS.md](MIGRATION_STATUS.md).
