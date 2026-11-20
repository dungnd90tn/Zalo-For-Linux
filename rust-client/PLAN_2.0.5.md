# Kế hoạch Zalo Native 2.0.5

Mục tiêu của 2.0.5 là hoàn thiện các thao tác thường dùng trong cuộc trò chuyện. Kết quả kiểm thử theo từng luồng nằm trong [VALIDATION.md](VALIDATION.md). Danh sách tính năng dài hạn nằm trong [FEATURE_PARITY.md](FEATURE_PARITY.md).

## Thứ tự triển khai

| Ưu tiên | Hạng mục | Tiêu chí hoàn thành |
| --- | --- | --- |
| 1 | Sticker | Gửi từ bộ mặc định trong chat cá nhân và nhóm thường. Tìm và gửi sticker thường từ kết quả danh mục tương thích. Xác nhận đúng hình, đúng hội thoại và giữ bản nháp khi mất kết nối. |
| 2 | Cảm xúc | Thả và gỡ sáu cảm xúc trên tin văn bản đủ điều kiện. Kiểm tra số lượt sau khi nhận lại cùng sự kiện, mở lại app và cập nhật từ thiết bị khác. |
| 3 | Thao tác theo lô | Chọn nhiều tin ngay từ menu và sao chép nội dung theo thứ tự hiển thị. Thoát chế độ chọn bằng bàn phím và khi chuyển hội thoại. |
| 4 | Kiểm thử phát hành | Chạy lại các luồng bên dưới trên bản build đóng gói, rồi chốt phiên bản và ghi chú phát hành. |

Ba hạng mục đầu đã có trong bản chạy thử và qua kiểm thử tổng hợp. Trên tài khoản thử, ưu tiên xác nhận tìm và gửi sticker, sau đó kiểm tra thêm/gỡ cảm xúc từ điện thoại khi Linux đang ngoại tuyến rồi mở lại ứng dụng.

## Kiểm thử trước khi phát hành

- Đăng nhập bằng QR, khởi động lại với phiên đã lưu, thử mất và hồi phục kết nối. Kiểm tra hội thoại ẩn sau khi mở và khóa lại bằng PIN.
- Gửi văn bản bằng Enter và nút **Gửi** khi bật/tắt x-unikey. Ghi kết quả riêng cho X11 và Wayland. Kiểm tra trả lời, gửi tệp/ảnh, thông báo, chuyển tiếp, thu hồi và thao tác nhóm trên tài khoản thử.
- Kiểm tra sticker và cảm xúc giữa Linux và điện thoại trong cả chat cá nhân và nhóm thường. Kiểm tra riêng trường hợp mất xác nhận gửi và mở lại app.
- Gói thử `2.0.5-2` đã sẵn sàng. Đã thử nâng cấp `.deb` từ `2.0.4-3` lên `2.0.5-1`, rồi từ `2.0.5-1` lên `2.0.5-2` trên Ubuntu 20.04. Gói `.rpm` đã qua thử cài/gỡ trên AlmaLinux 9. Thẻ phát hành sẽ khớp phiên bản Native khi chốt kiểm thử trên tài khoản thực.

## Sau 2.0.5

Ưu tiên nhắc tên thành viên trong nhóm, ghim tin, chuyển tiếp nhiều người và thu hồi trong nhóm. Sau đó đánh giá duyệt theo gói sticker, đồng bộ lịch sử từ xa, ảnh gốc, album và ghi âm. Thứ tự có thể đổi theo kết quả thử trên tài khoản thực.
