# Kế hoạch Zalo Native 2.0.5

Mục tiêu của 2.0.5 là hoàn thiện các thao tác thường dùng trong cuộc trò chuyện. Kết quả kiểm thử theo từng luồng nằm trong [VALIDATION.md](VALIDATION.md). Danh sách tính năng dài hạn nằm trong [FEATURE_PARITY.md](FEATURE_PARITY.md).

## Thứ tự triển khai

| Ưu tiên | Hạng mục | Tiêu chí hoàn thành |
| --- | --- | --- |
| 1 | Sticker | Gửi từ bộ mặc định trong chat cá nhân và nhóm thường. Tìm và gửi sticker thường từ kết quả danh mục tương thích. Xác nhận đúng hình, đúng hội thoại và giữ bản nháp khi mất kết nối. |
| 2 | Cảm xúc | Thả và gỡ sáu cảm xúc trên tin văn bản đủ điều kiện. Kiểm tra số lượt sau khi nhận lại cùng sự kiện, mở lại app và cập nhật từ thiết bị khác. |
| 3 | Thao tác theo lô | Chọn nhiều tin ngay từ menu và sao chép nội dung theo thứ tự hiển thị. Thoát chế độ chọn bằng bàn phím và khi chuyển hội thoại. |
| 4 | Kiểm thử phát hành | Chạy lại các luồng bên dưới trên bản build đóng gói, rồi chốt phiên bản và ghi chú phát hành. |

Ba hạng mục đầu đã có trong bản thử `2.0.5-2` và qua kiểm thử tổng hợp. Người dùng xác nhận gửi sticker và thả/gỡ cảm xúc hoạt động giữa Linux và điện thoại. [Ghi chú 2.0.5](../changelogs/RELEASE-v2.0.5.md) tóm tắt các tính năng và cách nâng cấp.

## Kiểm thử trước khi phát hành

- Đăng nhập bằng QR, khởi động lại với phiên đã lưu, thử mất và hồi phục kết nối. Kiểm tra hội thoại ẩn sau khi mở và khóa lại bằng PIN.
- X11 và Wayland đã qua thử Enter và nút gửi của chế độ demo với x-unikey bật/tắt trên gói 2.0.5-2. Wayland cần Enter lần hai nếu lần đầu vừa chốt chữ đang soạn. Trên tài khoản thử, tiếp tục kiểm tra trả lời, gửi tệp/ảnh, thông báo, chuyển tiếp, thu hồi và thao tác nhóm.
- Kiểm tra riêng tìm sticker, chat cá nhân và nhóm thường, mất xác nhận gửi, và lấy bù cảm xúc được thêm/gỡ lúc Linux ngoại tuyến. Lượt xác nhận nhanh trên tài khoản thật chưa ghi kết quả theo từng tình huống này.
- Đã thử nâng cấp `.deb` từ `2.0.4-3` lên `2.0.5-1`, rồi lên `2.0.5-2` trên Ubuntu 20.04. Gói `2.0.5-2` qua thử cài/gỡ trên Ubuntu 22.04/24.04, Debian 12, Fedora 42 và AlmaLinux 9. Workflow lấy số gói `-2` từ `Cargo.toml`. Thẻ phát hành sẽ khớp phiên bản Native khi chốt kiểm thử trên tài khoản thực.

## Sau 2.0.5

Nhánh `v2.0.6` đã có nhắc tên và ghim tin văn bản trong nhóm thường, chuyển tiếp tin văn bản cá nhân đến tối đa 10 bạn bè, thu hồi tin văn bản của mình trong nhóm thường, duyệt bộ sticker khi phiên trả danh mục trực tiếp, tải từng trang tin văn bản cũ trong nhóm đủ điều kiện và gửi ảnh gốc JPEG trong chat riêng hoặc nhóm khi tài khoản có quyền Zalo Cloud. Nhánh này cũng gửi album từ 2 đến 6 ảnh HD và hiển thị các ảnh thuộc album trong khung chat. Cần xác nhận các luồng này trên tài khoản thử. Hạng mục tiếp theo là ghi âm.
