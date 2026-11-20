# Zalo Native 2.0.5

Zalo Native 2.0.5 bổ sung gửi sticker, thả cảm xúc và chọn nhiều tin nhắn. Gói thử hiện tại là **2.0.5-2** cho Linux x86_64.

## Điểm mới

- **Nhãn dán**. Mở biểu tượng nhãn dán cạnh ô soạn để gửi một trong 30 sticker mặc định. Tab **Tìm nhãn dán** hỗ trợ tìm theo từ khóa khi phiên Zalo có dịch vụ danh mục tương thích. Gửi sticker giữ nguyên bản nháp.
- **Cảm xúc**. Bấm chuột phải vào tin văn bản đủ điều kiện để thả một trong sáu cảm xúc hoặc gỡ cảm xúc của mình. Ứng dụng lấy bù sự kiện khi kết nối lại nếu máy chủ cung cấp hàng đợi tương thích. Huy hiệu phản ánh những lượt ứng dụng đã ghi nhận.
- **Chọn nhiều tin nhắn**. Sao chép các tin văn bản đã chọn trên trang hiện tại theo thứ tự thời gian. Nhấn Esc để thoát chế độ chọn.
- **Menu chuột phải gọn hơn**. Các mục căn trái với icon, hàng cảm xúc nằm trên cùng. Tab hoặc phím mũi tên di chuyển, Enter chọn mục.

## Cài đặt và nâng cấp

Đóng cửa sổ Zalo Native rồi cài gói phù hợp bằng trình quản lý gói.

```bash
sudo apt install ./zalo-native_2.0.5-2_amd64.deb
# Hoặc trên Fedora / AlmaLinux
sudo dnf install ./zalo-native-2.0.5-2.x86_64.rpm
```

Lần mở đầu tiên nâng hồ sơ cũ lên **schema 18**. Kiểm tra nâng cấp từ 2.0.4-3 xác nhận lịch sử thử được giữ lại. Bản cũ dùng schema 16 hoặc 17 từ chối mở hồ sơ đã nâng cấp.

## Kiểm chứng

Người dùng xác nhận gửi sticker và thả/gỡ cảm xúc hoạt động giữa Linux và điện thoại trên 2.0.5-2. Bản cài đã qua kiểm tra trên Ubuntu 20.04/22.04/24.04, Debian 12, Fedora 42 và AlmaLinux 9. Bộ kiểm thử Rust có 828 ca đạt.

Hồ sơ demo qua thử Enter và nút gửi với x-unikey trên X11 và Wayland. Khi Enter trên Wayland vừa chốt chữ đang soạn, nhấn Enter thêm lần nữa để gửi. [Kế hoạch 2.0.5](../rust-client/PLAN_2.0.5.md) · [Tài liệu kiểm chứng](../rust-client/VALIDATION.md)
