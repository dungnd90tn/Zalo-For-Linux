# Zalo Native 2.0.6

Gói `2.0.6-1` dành cho Linux x86_64. Bản này mở rộng thao tác trong nhóm, chuyển tiếp tin văn bản và gửi nội dung đa phương tiện.

## Tính năng mới

- Gõ `@` để nhắc từng thành viên. Chọn **Nhắc tất cả · @All** khi Zalo xác nhận nhóm đủ điều kiện. Gõ `@All` mà chưa chọn gợi ý chỉ gửi văn bản thường.
- Chuyển tiếp một tin văn bản đủ điều kiện từ chat cá nhân hoặc nhóm thường đến tối đa 10 đích. Kết quả hiển thị riêng cho từng người hoặc nhóm. Các lượt chưa rõ kết quả được giữ qua lần mở lại để tránh gửi trùng.
- Xem chi tiết bình chọn, số phiếu được phép xem và bỏ phiếu lần đầu trong bình chọn đang mở chỉ cho chọn một phương án.
- Ghim và bỏ ghim văn bản trong nhóm theo quyền. Tải từng trang lịch sử văn bản của nhóm khi tài khoản được cấp quyền.
- Gửi album từ 2 đến 6 ảnh HD, gửi ảnh gốc JPEG với quyền Zalo Cloud, và thu, nghe lại, gửi tin thoại MP3 tối đa 60 giây.
- Thu hồi tin văn bản do mình gửi, hoặc ảnh và tệp đã gửi từ Linux và có xác nhận. Xóa tin văn bản ở phía mình và nhận sự kiện xóa từ thiết bị khác khi Zalo hỗ trợ đồng bộ.

## Cài đặt và nâng cấp

Đã kiểm tra cài, chạy ngoại tuyến và gỡ gói trên Ubuntu 20.04, 22.04, 24.04, Debian 12, Fedora 42 và AlmaLinux 9. Đã thử nâng cấp từ gói 2.0.5-2 trên Ubuntu 20.04 bằng hồ sơ thử.

```bash
# Ubuntu/Debian
sudo apt install ./zalo-native_2.0.6-1_amd64.deb

# Fedora/RHEL/AlmaLinux/Rocky
sudo dnf install ./zalo-native-2.0.6-1.x86_64.rpm
```

Mở **Zalo Native** từ menu ứng dụng hoặc chạy `zalo-native --live`. Sau khi mở hồ sơ bằng 2.0.6, ứng dụng nâng cơ sở dữ liệu lên **schema 26**. Bản cũ dùng schema 25 trở xuống sẽ từ chối mở hồ sơ đã nâng cấp. Nếu cần quay về bản cũ, giữ một bản sao hồ sơ trước khi mở bản mới.

Các luồng nhắn tin mới đã có kiểm thử tổng hợp. Đồng bộ với điện thoại trên tài khoản Zalo thật vẫn cần xác nhận theo từng luồng. [Kết quả kiểm thử](../rust-client/VALIDATION.md)
