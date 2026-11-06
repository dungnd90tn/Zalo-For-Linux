# Zalo Native cho Linux

**Ứng dụng Zalo trên Linux, phát triển bằng Rust với giao diện native.**

Dự án kết hợp giao diện `egui`/`eframe`, lịch sử trò chuyện lưu bằng SQLite và các tác vụ nền xử lý kết nối. Ứng dụng hiển thị trực tiếp, không dùng WebView; đồng thời tận dụng bộ màu, phông chữ và biểu tượng Zalo có sẵn trong kho mã để giữ trải nghiệm quen thuộc.

**Hiện trạng: bản thử nghiệm đang được phát triển.** Đăng nhập QR, khôi phục phiên, nhận tin nhắn văn bản và ảnh đã được sử dụng với tài khoản thực. Ứng dụng chưa hoàn thiện toàn bộ tính năng; gửi–nhận hai chiều, đồng bộ đầy đủ và độ ổn định khi chạy lâu vẫn cần được xác nhận thêm. Dự án chưa có kết quả so sánh hiệu năng trên các tác vụ tương đương.

Đây là dự án cộng đồng, không thuộc Zalo hay VNG và không phải ứng dụng chính thức của các đơn vị này.

## Những gì ứng dụng hiện có

- **Đăng nhập bằng QR:** tự kiểm tra phiên đã lưu khi mở ứng dụng; nếu chưa đăng nhập thì hiển thị QR. Phiên được xác nhận hết hạn sẽ yêu cầu quét lại; lỗi kết nối tạm thời vẫn giữ phiên và cho phép thử lại.
- **Đọc lịch sử và soạn nháp khi ngoại tuyến:** giữ lịch sử, tên liên hệ/nhóm và bản nháp riêng cho từng cuộc trò chuyện. Nội dung chỉ hiển thị khi đã xác minh được trạng thái hội thoại ẩn; nếu lần kiểm tra mới thất bại, ứng dụng tạm giữ kín nội dung. Người gửi chưa xác định được tên có nhãn thay thế trung tính.
- **Tôn trọng hội thoại đã ẩn trên Zalo:** loại khỏi danh sách thông thường; tìm tên và nhập PIN Zalo hiện có để mở riêng hội thoại được chọn.
- **Hiển thị nội dung nhận được:** văn bản, thông báo hệ thống, ảnh xem trước của sticker tĩnh, ảnh và một số thẻ OA/nội dung được chia sẻ. Hỗ trợ PNG/JPEG/WebP và một phần JPEG XL tĩnh; ảnh đã tải có thể mở trong khung xem của ứng dụng.
- **Thích ứng với kích thước cửa sổ:** bố cục rộng và gọn giữ lại hội thoại đang chọn cùng bản nháp. Cỡ chữ và giới hạn khung ảnh được giữ nhất quán khi thay đổi kích thước.
- **Dùng giao diện quen thuộc của Zalo:** có chế độ sáng, tối, theo hệ thống và tùy chỉnh; lựa chọn giao diện được lưu theo từng hồ sơ.
- **Có thông tin chẩn đoán:** lỗi kết nối hiển thị giai đoạn/nhóm lỗi, kèm nhật ký có giới hạn dung lượng để hỗ trợ điều tra.

Tin nhắn bình chọn hiện hiển thị thông báo/câu hỏi đính kèm; chưa có chi tiết, kết quả hay thao tác bỏ phiếu. Với mẫu thẻ chưa hỗ trợ, ứng dụng thông báo rõ thay vì hiển thị các thao tác không hoạt động.

## Biên dịch và chạy

### Cài các gói cần thiết trên Linux

Đây là ứng dụng chạy trong môi trường người dùng: hãy chọn gói theo **bản phân phối Linux**, không cần biên dịch riêng theo phiên bản kernel hay cài kernel headers. Giao diện cần phiên đăng nhập desktop và trình điều khiển đồ họa hỗ trợ OpenGL phù hợp với phần cứng.

Môi trường kiểm thử hiện tại là Linux x86_64. Tên gói dưới đây đã được đối chiếu với danh mục của từng bản phân phối; chưa có nghĩa là ứng dụng đã được biên dịch và kiểm tra giao diện đầy đủ trên tất cả các hệ này.

**Ubuntu / Debian** — [gói trên Ubuntu](https://packages.ubuntu.com/noble/libgl1-mesa-dev), [gói trên Debian](https://packages.debian.org/trixie/libxkbcommon-x11-0):

```bash
sudo apt update
sudo apt install build-essential pkg-config git curl ca-certificates python3 \
  libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libx11-dev libgl1-mesa-dev
```

**Fedora** — [gói phát triển OpenGL](https://packages.fedoraproject.org/pkgs/libglvnd/libglvnd-devel/):

```bash
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config git curl ca-certificates python3 \
  libxkbcommon-devel libxkbcommon-x11 wayland-devel libX11-devel libglvnd-devel
```

**Arch Linux** — [bộ công cụ biên dịch](https://archlinux.org/packages/core/any/base-devel/), [thư viện bàn phím X11](https://archlinux.org/packages/extra/x86_64/libxkbcommon-x11/):

```bash
sudo pacman -Syu --needed base-devel git curl ca-certificates python \
  libxkbcommon libxkbcommon-x11 wayland libx11 libglvnd
```

### Tải mã nguồn

```bash
git clone https://github.com/dungnd90tn/Zalo-For-Linux.git
cd Zalo-For-Linux
```

Các lệnh bên dưới được chạy từ thư mục gốc của kho mã, trừ khi có ghi chú khác.

### Cài bộ công cụ Rust và biên dịch

Cài `rustup` theo [hướng dẫn chính thức của Rust](https://rust-lang.org/tools/install/), sau đó mở lại terminal để các công cụ có trong `PATH`. Dự án cố định **Rust 1.99.1**, kèm `rustfmt` và `clippy`, trong [rust-toolchain.toml](rust-client/rust-toolchain.toml):

```bash
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy
(cd rust-client && cargo fetch --locked)
./scripts/rust/build.sh --offline
```

Lần tải thư viện cần kết nối mạng. Khi đã có đủ thư viện trong bộ nhớ đệm, có thể biên dịch và kiểm tra bằng các lệnh ngoại tuyến. Script của dự án tự chọn bộ công cụ đã cố định; nếu gọi Cargo trực tiếp, hãy chạy trong `rust-client/`. Nếu dùng thư mục đích Cargo tùy chỉnh, thay đường dẫn mặc định của tệp thực thi trong các ví dụ dưới đây cho phù hợp.

### Dùng thử không cần tài khoản

```bash
./scripts/rust/run.sh --demo
```

Chế độ demo dùng hội thoại giả lập, không kết nối đến Zalo. Để kiểm tra tác vụ nền và lưu trữ mà không mở cửa sổ:

```bash
./scripts/rust/run.sh --demo --headless
```

### Đăng nhập Zalo

```bash
./scripts/rust/run.sh --live
```

Script biên dịch với các tính năng kết nối rồi mở ứng dụng. Ở lần đầu, quét mã QR và xác nhận đăng nhập trên điện thoại; những lần sau ứng dụng tự kiểm tra phiên đã lưu. Tab số điện thoại hiện chỉ cung cấp thông tin: **chưa hỗ trợ đăng nhập bằng số điện thoại**.

Sau khi biên dịch bản live, có thể chạy trực tiếp mà không cần Cargo:

```bash
./rust-client/target/release/zalo-rust --live
```

Chế độ live kết nối đến Zalo bằng hồ sơ đã chọn. Bản mặc định/demo không bao gồm các thành phần HTTP, WebSocket và mô-đun xác thực tùy chọn. Bản live cũng chưa kích hoạt thành phần Signal đang thử nghiệm.

### Mở hội thoại đã ẩn

Nhập tên trong ô tìm kiếm, chọn kết quả hội thoại ẩn rồi nhập **PIN Zalo hiện có gồm 4 chữ số**. PIN đúng chỉ mở hội thoại vừa chọn. Chuyển sang hội thoại khác, quay lại danh sách, mở danh bạ, đăng xuất hoặc khởi động lại ứng dụng sẽ khóa lại. Client Linux chưa hỗ trợ tạo, đặt lại PIN hay bỏ ẩn hội thoại.

Ứng dụng lấy danh sách ẩn mới khi mở, khi cửa sổ được lấy nét trở lại và định kỳ khoảng 30 giây. Thay đổi trên điện thoại **chưa được đồng bộ tức thời**. Khi chưa xác minh được danh sách, nội dung được giữ kín, kể cả lịch sử đã lưu; bấm avatar để xem trạng thái hoặc chọn **Kiểm tra lại quyền riêng tư**.

### Thêm ứng dụng vào menu Linux

Biên dịch bản live ngay trước khi cài: mục trong menu chạy với `--live`, trong khi một lần biên dịch mặc định/demo sau đó có thể ghi đè tệp thực thi trong thư mục mã nguồn.

```bash
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/install-desktop.py --dry-run
python3 scripts/rust/install-desktop.py
```

Mở **Zalo Native** từ menu ứng dụng. Trình cài đặt sao chép tệp thực thi, tạo mục ứng dụng cho người dùng hiện tại và cài biểu tượng Zalo có sẵn. Quá trình này không mở ứng dụng, bật tự khởi động hay thay đổi hồ sơ.

Sau mỗi lần biên dịch bản mới, chạy lại trình cài đặt để cập nhật bản đã cài, rồi đóng cửa sổ cũ và mở lại Zalo Native. Xem [tích hợp desktop](rust-client/linux/README.md) để biết vị trí các tệp, cách chọn tệp thực thi và cài thử vào thư mục riêng.

## Những phần còn cần hoàn thiện

[Bảng đối chiếu tính năng](rust-client/FEATURE_PARITY.md) phân biệt phần đã triển khai với phần đã được kiểm chứng. Các nhóm việc chính còn lại:

| Nhóm | Phần còn thiếu hoặc cần xác nhận |
| --- | --- |
| Nhắn tin | Xác nhận đầy đủ gửi–nhận cá nhân/nhóm với tài khoản thực, kết nối lại và vận hành lâu dài |
| Đồng bộ | Toàn bộ lịch sử từ máy chủ, danh sách thành viên đầy đủ và tải ảnh đại diện |
| Nội dung | Gửi ảnh, tệp, ghi âm, sticker động và các mẫu thẻ OA còn lại |
| Thao tác tin nhắn | Trả lời, chuyển tiếp, nhắc tên, thả cảm xúc, thu hồi/xóa và trạng thái đã đọc |
| Bình chọn và tìm kiếm | Chi tiết/kết quả/bỏ phiếu và tìm kiếm toàn bộ tin nhắn |
| Tích hợp Linux | Khay hệ thống, thông báo, chụp màn hình và kiểm tra trên nhiều môi trường desktop |
| Đăng nhập và mã hóa | Đăng nhập số điện thoại và nhắn tin Signal/E2EE trong bản sử dụng thực tế |
| Phân phối | Gói cài đặt native, cập nhật tự động và nhập bản sao lưu/lịch sử |

Chưa hỗ trợ gọi thoại/video. Nội dung có cơ chế mã hóa chưa được hỗ trợ sẽ không bị xử lý như văn bản thông thường. Thẻ chia sẻ, bình chọn và các mẫu OA còn lại vẫn cần kiểm tra bằng tài khoản thực; bộ dữ liệu kiểm thử không chứng minh ứng dụng hỗ trợ mọi mẫu thẻ hay đã hoàn thiện toàn bộ tính năng.

Đã có kiểm thử nhập liệu/hiển thị trên X11 và bộ tích hợp IME. Độ rõ của chữ, tích hợp cửa sổ và hoạt động của bộ gõ trên Wayland native vẫn cần xác nhận trên desktop thực tế. Kết quả kiểm thử cục bộ không bảo đảm tương thích với mọi trình quản lý cửa sổ hay bộ gõ.

## Hồ sơ dữ liệu và quyền riêng tư

Ứng dụng lưu dữ liệu, cấu hình và bộ nhớ đệm trong các thư mục XDG tương ứng, dưới `zalo-rust/profiles/<name>`. Tên hồ sơ mặc định:

| Chế độ | Hồ sơ |
| --- | --- |
| `--demo` | `rust-demo` |
| `--live` | `rust-live` |
| Không có hai cờ trên | `rust-pilot` |

Dùng `--profile-root` và `--profile` để chọn vị trí/tên hồ sơ, hoặc đặt `ZALO_RUST_PROFILE_ROOT` và `ZALO_RUST_PROFILE` khi dùng script chạy ứng dụng. Cờ dòng lệnh được ưu tiên. Khi phát triển, hãy dùng hồ sơ thử nghiệm riêng; không chia sẻ thông tin đăng nhập, tệp phiên hay cơ sở dữ liệu tin nhắn.

PIN và giá trị băm dùng để xác minh PIN chỉ được giữ trong bộ nhớ khi ứng dụng chạy, không lưu bền vững vào hồ sơ. Cơ chế khóa hội thoại kiểm soát việc hiển thị; **PIN không mã hóa cơ sở dữ liệu SQLite**.

Quá trình nâng cấp cấu trúc cơ sở dữ liệu SQLite giữ lại dữ liệu cũ được hỗ trợ. Tệp thực thi cũ có thể không đọc được cấu trúc dữ liệu mới; thay tệp thực thi không đồng nghĩa với hạ phiên bản cơ sở dữ liệu. Ứng dụng chưa tự nhập lịch sử từ ứng dụng desktop đang có.

### Chẩn đoán lỗi

Xem các khả năng đã biên dịch và tìm đường dẫn nhật ký của hồ sơ live mà không khởi động đăng nhập hay ghi dữ liệu hồ sơ:

```bash
./rust-client/target/release/zalo-rust --doctor --profile rust-live
```

Ứng dụng tự ghi nhật ký cấu trúc tại `diagnostics/native.log` trong thư mục bộ nhớ đệm của hồ sơ; nhật ký được xoay vòng và giới hạn dung lượng. Terminal và kết quả `--doctor` cho biết đường dẫn cụ thể. Khi tái hiện lỗi kết nối, có thể bổ sung thông tin giao thức có giới hạn:

```bash
./rust-client/target/release/zalo-rust --live --debug-log
```

Nhật ký không ghi nội dung tin nhắn, mã định danh tài khoản, URL, cookie, thông tin đăng nhập hay phản hồi thô từ máy chủ. Kiểm tra lại báo cáo trước khi chia sẻ. Khi gặp lỗi kết nối, hãy dựa vào giai đoạn/nhóm lỗi trong chẩn đoán; không cần xóa hồ sơ như một bước xử lý thông thường.

## Phát triển và kiểm thử

Chạy bộ kiểm tra mặc định từ thư mục gốc:

```bash
./scripts/rust/check.sh
```

Lệnh này kiểm tra định dạng, các bài kiểm thử Rust/Python, Clippy, giới hạn phụ thuộc và các chế độ dòng lệnh không cần giao diện. Để kiểm tra tích hợp các tính năng tùy chọn:

```bash
(
  cd rust-client
  cargo test --locked --offline --all-features
  cargo clippy --locked --offline --all-targets --all-features -- -D warnings
)
```

Kiểm thử giao thức cần lệnh `openssl` và quyền mở socket trên địa chỉ loopback cục bộ. Để kiểm tra nhanh bản release live với hồ sơ và dữ liệu giả lập riêng:

```bash
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/smoke-release.py --expect-live
```

Chạy khối lệnh trên từ thư mục gốc. Kiểm tra release không đăng nhập tài khoản thực. Giao diện, IME và hoạt động với tài khoản thực cần được kiểm tra riêng; [hướng dẫn kiểm chứng](rust-client/VALIDATION.md) ghi lại quy trình và giới hạn của từng loại kiểm tra. `package.json` ở thư mục gốc không phải điểm bắt đầu để biên dịch hay kiểm thử ứng dụng Rust.

## Cấu trúc dự án

| Đường dẫn | Nội dung |
| --- | --- |
| `rust-client/src/` | Mô hình dữ liệu, lưu trữ SQLite, tác vụ nền, giao thức, hồ sơ và giao diện native |
| `rust-client/src/ui/` | Đăng nhập, hội thoại, nội dung đa phương tiện, cài đặt và thành phần giao diện dùng chung |
| `rust-client/examples/` | Công cụ đo hiệu năng và lưu trữ với dữ liệu giả lập |
| `scripts/rust/` | Script biên dịch/chạy/kiểm tra, trình cài desktop và kiểm thử công cụ |
| `rust-client/vendor/egui-winit/` | Các thay đổi tích hợp IME trên Linux của phiên bản đang cố định |
| `pc-dist/` | Phông chữ, biểu tượng, giao diện Zalo và các bundle tham chiếu giao thức có sẵn |

Xử lý mạng, truy cập dữ liệu và giải mã ảnh phải chạy ngoài luồng dựng giao diện. Giữ giới hạn hàng đợi/bộ nhớ đệm, tách đúng phạm vi tài khoản/hội thoại và loại bỏ kết quả bất đồng bộ đã lỗi thời. Xem [kiến trúc](rust-client/ARCHITECTURE.md) để hiểu cách tổ chức ứng dụng.

## Đóng góp và tài liệu

Đọc [hướng dẫn của kho mã](AGENTS.md) trước khi đóng góp. Mô tả hành vi thay đổi, cách đã kiểm tra và đính kèm ảnh chụp nếu sửa giao diện. Phần giao diện native tuân theo [thiết kế UI](rust-client/UI_DESIGN.md) và [quy tắc cho agent UI](rust-client/src/ui/AGENTS.md), bao gồm bộ màu/biểu tượng hiện có và kích thước nhất quán.

- [Kiến trúc](rust-client/ARCHITECTURE.md): ranh giới các thành phần, tác vụ nền và lưu trữ.
- [Đối chiếu tính năng](rust-client/FEATURE_PARITY.md): phần đã làm, phần còn thiếu và tiêu chí xác nhận.
- [Giao diện và theme](rust-client/UI_THEME.md): các chế độ hiển thị, tài nguyên gốc và phạm vi áp dụng token.
- [Quy ước nội dung đa phương tiện](rust-client/PROTOCOL_MEDIA.md): định dạng hỗ trợ, giải mã và giới hạn tài nguyên.
- [Kiểm chứng](rust-client/VALIDATION.md): lệnh kiểm thử và bằng chứng kiểm tra trên desktop.
- [Tiến độ phát triển](rust-client/MIGRATION_STATUS.md): các mốc triển khai và lịch sử kiểm chứng.

Thương hiệu Zalo và tài nguyên đi kèm thuộc về chủ sở hữu tương ứng. Việc tích hợp không thể hiện sự liên kết hay bảo chứng; mã bên thứ ba giữ nguyên các thông báo bản quyền/giấy phép của mình.

Gửi lỗi có thể tái hiện hoặc đề xuất cải tiến tại [GitHub Issues](https://github.com/dungnd90tn/Zalo-For-Linux/issues). Khi báo lỗi, cung cấp phiên bản ứng dụng, môi trường desktop, X11/Wayland, các bước tái hiện và thông tin chẩn đoán đã loại bỏ dữ liệu nhạy cảm nếu có.
