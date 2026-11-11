# v2.0.4 — Củng cố trạng thái gửi và kiểm tra phiên mã hóa

Bản cập nhật **Zalo Native** tập trung vào tính nhất quán khi gửi, lưu và khôi phục tin nhắn.
Ứng dụng vẫn là bản thử nghiệm cộng đồng dành cho Linux.

## Thay đổi chính

- Phân biệt lỗi lưu tin đã được máy chủ xác nhận với trường hợp chưa biết gửi thành công hay chưa.
  Bản nháp được giữ lại; ứng dụng không tự gửi lại hoặc mã hóa lại tin khi gặp các lỗi này.
- Khởi động lại có thể khôi phục cục bộ tin E2EE đã có xác nhận, đúng tài khoản, theo từng lô nhỏ;
  không cần gửi lại lên máy chủ.
- Trong bản dựng E2EE tùy chọn, kiểm tra mới danh sách thiết bị, bộ khóa và trạng thái phiên trước
  khi gửi. Dữ liệu thiếu, phiên cũ không khớp hoặc danh tính thay đổi sẽ chặn gửi thay vì tiếp tục
  với trạng thái chưa được xác minh. Tin đến vẫn được xử lý trong lúc chờ kiểm tra.
- Nâng cơ sở dữ liệu lên **schema 9**, bổ sung trạng thái khởi tạo phiên để tránh gửi tiếp khi chưa
  rõ kết quả của lần khởi tạo trước, kể cả sau khi mở lại ứng dụng.

## Phạm vi bản dựng

Gói tiêu chuẩn dùng feature `live`: có đăng nhập và nhắn tin, **không kèm engine Signal**.
Các cải tiến E2EE chỉ áp dụng khi dựng riêng với `signal-crypto`; chúng chưa đồng nghĩa với hỗ trợ
E2EE hoàn chỉnh. Chưa có tự làm mới phiên cũ, xác nhận đổi danh tính hay hàng đợi E2EE ngoại tuyến
đầy đủ. Các lần khởi tạo cũ thiếu bản ghi ý định chưa thể được phân loại tin cậy.

Native dùng phiên bản trong `rust-client/Cargo.toml`; `version.txt` tiếp tục dành cho gói Electron cũ.

## Kiểm chứng và giới hạn

Trước lượt đóng gói 2.0.4, mã nguồn đã vượt qua **652 kiểm thử** với mọi feature; bốn fixture tùy chọn
được bỏ qua. Định dạng Rust, Clippy nghiêm ngặt và kiểm tra bản `live` đều đạt. Kiểm thử HTTPS/WSS
cục bộ dùng dữ liệu tổng hợp kiểm tra việc gửi đúng thiết bị, xử lý ACK và các trường hợp từ chối.

Smoke của bản release `live` kiểm tra schema 9, 19 tin giả lập trong sáu hội thoại, mở lại ứng dụng,
quyền riêng tư của hồ sơ và khóa tiến trình. Đây là kiểm chứng tổng hợp, **chưa phải xác nhận gửi
E2EE với tài khoản hoặc máy chủ Zalo thật**. Kết quả kiểm chứng theo từng mốc nằm trong
[VALIDATION.md](../rust-client/VALIDATION.md#checked-composer-device-readiness-2026-11-11).
