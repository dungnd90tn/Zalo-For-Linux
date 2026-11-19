# Zalo Native 2.0.4-3 — Sửa lỗi ngoại tuyến khi đồng bộ

Bản đóng gói **2.0.4-3** sửa lỗi ứng dụng vừa lên Online đã chuyển ngoại tuyến, khiến nút **Gửi** bị mờ và Enter không gửi được. Lỗi xuất hiện khi bộ đồng bộ gặp tin `webchat` có nội dung dạng đối tượng. Ứng dụng giờ hiển thị phần chữ của tin định dạng phong phú khi nhận dạng được, hoặc dùng dòng thay thế cho định dạng chưa hỗ trợ, rồi tiếp tục đồng bộ các tin còn lại.

Khi gửi chưa khả dụng, tooltip phân biệt ngoại tuyến, đang kết nối, phiên hết hạn và bộ gõ đang soạn chữ. Phím tắt giữ nguyên: **Enter** gửi sau khi chữ đã được chốt. Với x-unikey trên X11, nếu Enter vừa chốt chữ đang soạn, bấm Enter lần nữa để gửi. **Ctrl+Enter** hoặc **Shift+Enter** xuống dòng.

## Tính năng có trong gói này

So với gói `2.0.4-1`, bản này bao gồm các tính năng đã có từ `2.0.4-2`:

- Trả lời kèm trích dẫn cho tin văn bản thường, gửi một tệp tối đa 100 MiB hoặc một ảnh HD tĩnh có xem trước và chú thích.
- Bộ lọc **Chưa đọc** cho tin mới nhận trên máy này. Trạng thái chuyển và đã xem hiển thị khi có xác nhận. Thông báo Linux mặc định chỉ báo có tin mới; bật xem trước trong **⚙ → Thông báo**.
- Chuyển tiếp một tin văn bản cá nhân đến một bạn bè, hoặc thu hồi tin văn bản do bạn gửi trong hội thoại cá nhân khi còn trong thời hạn.
- Xem thành viên theo quyền, mời một bạn bè và rời nhóm thường tối đa 100 thành viên. Lịch sử đã lưu vẫn xem được sau khi rời nhóm.

## Cài đặt và nâng cấp

Tải gói **x86-64** phù hợp cùng `SHA256SUMS` vào một thư mục, rồi chạy lệnh tương ứng:

```bash
sudo apt install ./zalo-native_2.0.4-3_amd64.deb
sudo dnf install ./zalo-native-2.0.4-3.x86_64.rpm
```

Đóng Zalo Native trước khi nâng cấp, rồi mở lại từ menu ứng dụng. Gói tiêu chuẩn dùng feature `live`. Nâng cấp từ `2.0.4-2` giữ schema 16. Khi nâng cấp từ các gói dùng schema cũ hơn, bản mới sẽ nâng hồ sơ lên schema 16; các bản cũ đó không mở được hồ sơ sau nâng cấp.

## Kiểm chứng

804 kiểm thử Rust đạt với 4 luồng, 5 ca tùy chọn được bỏ qua trong lượt đó. Ca TLS/WSS cục bộ xác nhận lô tin hỗn hợp được lưu, kết nối vẫn Online và tin thử được gửi. Đã kiểm tra nâng cấp `.deb` từ `2.0.4-2` trên Ubuntu 20.04 và cài, kiểm tra, gỡ `.rpm` trên AlmaLinux 9. Bản sửa đồng bộ chưa được xác nhận lại trên tài khoản Zalo thật. [Chi tiết kiểm chứng](../rust-client/VALIDATION.md#structured-chat-sync-and-disabled-send-2026-11-19)
