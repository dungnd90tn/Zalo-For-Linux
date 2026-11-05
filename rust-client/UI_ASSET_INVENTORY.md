# Kiểm kê theme, icon và mức phủ native

## Phạm vi và nguồn chuẩn

Đây là kiểm kê **nguồn hiện có**, không phải tuyên bố native đã phủ đầy đủ theme, icon hoặc tính năng Electron. Mục tiêu thiết kế là đối chiếu toàn bộ nguồn và theo dõi mọi phần chưa nối; bộ Light hoặc một tập icon nhỏ không đủ để gọi là hoàn tất.

- [UI_ASSET_INVENTORY.json](UI_ASSET_INVENTORY.json): dữ liệu sinh từ tài sản repository, gồm selector, khai báo, codepoint, font và fingerprint nguồn.
- [inventory-ui-assets.py](../scripts/rust/inventory-ui-assets.py): bộ trích xuất offline, có kiểm tra tính tái lập; không tải font/tài sản từ Internet.
- [UI_THEME.md](UI_THEME.md): ánh xạ theme native hiện tại; [UI_DESIGN.md](UI_DESIGN.md): luồng, bố cục và điều kiện nghiệm thu.

Phạm vi máy đọc gồm CSS cấp đầu, thư mục font, URL khai báo trực tiếp trong CSS, binding native và anchor runtime theme. SVG/PNG độc lập, catalog sticker hoặc tài sản phát sinh từ JavaScript chưa thuộc kiểm kê nếu không được URL CSS tham chiếu; chúng cần kiểm kê riêng khi nối tính năng tương ứng.

Chạy lại từ thư mục repository:

```sh
python3 scripts/rust/inventory-ui-assets.py --check
```

Kiểm tra này so nguồn với catalog đã sinh, không chứng minh cascade CSS hoặc hình ảnh native tương đương. Khi bundle thay đổi, cập nhật catalog bằng lệnh ghi được mô tả trong `--help`, xem diff và chạy kiểm thử bộ trích xuất. Không sửa tay số lượng/hash để làm kiểm tra xanh.

## Theme và chế độ

Có 7 bundle CSS cấp đầu trong `pc-dist/`, 16 tệp trong `pc-dist/fonts/`. Phải giữ đầy đủ khai báo, thứ tự và ngữ cảnh selector/at-rule, kể cả token lặp hoặc ghi đè; không chỉ lấy màu từ `:root` đầu tiên.

Snapshot hiện tại trong JSON:

| Dữ liệu nguồn | Số lượng và ý nghĩa |
| --- | --- |
| Custom property | 5.759 khai báo; 758 tên riêng biệt, gồm lặp/override giữa bundle |
| Quy tắc `content` | 3.661; gồm chuỗi rỗng và nội dung trang trí, không chỉ icon |
| Class `fa-*` liên hệ với `content` | 1.021 class; gồm alias và class trong selector ghép, không phải 1.021 glyph riêng biệt |
| `@font-face` / tệp font | 46 khai báo / 16 tệp; không đồng nghĩa 46 font khác nhau |
| Binding native | 29 role / 32 token nguồn; 15 icon ngữ nghĩa |
| Registry có thể vẽ | 1.034 glyph/alias xác minh cmap: 1.020 Zalo + 14 Screencap |
| Tham chiếu `var()` / URL CSS | 443 tên biến / 183 lần tham chiếu URL |

Mỗi nguồn CSS, font và tài sản URL cục bộ có fingerprint SHA-256 trong JSON; runtime JS có hash và anchor enum/query. Số lượng là snapshot kiểm tra bằng `--check`, không phải giới hạn cố định để giữ khi tài sản đổi.

JSON ghi 51 tên biến được tham chiếu nhưng không có khai báo trong CSS thuộc phạm vi này; cần kiểm tra fallback hoặc nguồn runtime trước khi nối. 726 token chưa có binding Palette tường minh vẫn nằm trong catalog. Không tự kết luận mọi class là icon riêng cần thêm hoặc mọi biến chưa khai báo là lỗi.

Runtime gốc trong `pc-dist/compact-app-pc.e08d0d44f38873747a6b.js` có theme `light`, `dark`, `custom`; chế độ cấu hình `LIGHT=0`, `DARK=1`, `SYSTEM=2`, `CUSTOM=3` và truy vấn `(prefers-color-scheme: dark)`.

| Phạm vi nguồn | Native hiện tại | Phần phải theo dõi |
| --- | --- | --- |
| `:root`, `.light` và token thành phần | Role Light cho thành phần native qua `theme.rs` | Các component Electron chưa port không được xem là đã hoạt động |
| `.dark` | Role Dark cho login, rail/list, chat, composer, directory/dialog/error | Tiếp tục kiểm tra ảnh/trạng thái khi thêm thành phần mới |
| Chế độ System | Chọn/lưu mode, theo tín hiệu egui/winit; thiếu tín hiệu dùng Light | Fixture không chứng minh mọi desktop Linux/native Wayland |
| `.custom`, `.mask-sidebar` và cấu hình runtime | Overlay Light, màu/Reset/lưu bền; gradient thật cho 3 role nền | Không giả lập CSS browser hoặc tự coi Custom là một palette tĩnh |

Cùng một tên token không bảo đảm cùng giá trị ở mọi selector. Bảng JSON là bằng chứng khai báo; khi nối vào native, ghi rõ theme, selector/context và component được tác động. Không phát minh màu thay thế để lấp ô trống.

## Icon và font

Native hiện khai báo **15** biến thể `Icon` trong `src/ui/visuals.rs`, đều dùng icon font gốc. Mỗi biến thể tra class trong registry đầy đủ thay vì giữ codepoint riêng. Generator giữ alias/font family, chỉ lấy class pseudo-element đích và xác minh cmap: không gán glyph cho ancestor như `.fa-ic_checkbox tick:after`. Không đếm 15 control thành tổng icon Zalo.

| Native `Icon` | Class nguồn | Codepoint |
| --- | --- | --- |
| Chat / Contacts | `.fa-tab-msg` / `.fa-tab-contact` | `U+ED8A` / `U+ED83` |
| Cloud / Settings | `.fa-cloud` / `.fa-tab-icon-setting` | `U+EAA5` / `U+ED88` |
| Search / Plus | `.fa-outline-search` / `.fa-outline-plus` | `U+ECA1` / `U+EC97` |
| Phone / Video | `.fa-outline-call` / `.fa-outline-video` | `U+EC83` / `U+ECAC` |
| Smile / Image / Paperclip | `.fa-chatbar-emoji` / `.fa-chatbar-photo` / `.fa-chatbar-attach` | `U+EA5C` / `U+EA6A` / `U+EA5A` |
| Back / Info | `.fa-back` / `.fa-outline-info` | `U+EA25` / `U+EC8D` |
| Check / Shield | `.fa-check` / `.fa-icon-outline-lock` | `U+EA7E` / `U+EBC5` |

Font được catalog theo đường dẫn, loại tệp, kích thước, SHA-256 và khai báo `@font-face`. Các tệp cũ như EOT vẫn thuộc kiểm kê; điều đó không yêu cầu nạp chúng vào native. Native nhúng 5 tài sản cần dùng: Segoe regular/Semibold/Bold, icon Zalo và Screencap; các weight/font trang trí chưa có thành phần sử dụng vẫn ở catalog, không nạp sẵn.

Một codepoint có binding không có nghĩa control tương ứng đã hoạt động. Gọi thoại/video, upload ảnh, tệp và gửi sticker vẫn là khoảng trống tính năng. Giữ đúng disabled/tooltip cho tới khi có luồng thực. Không thay glyph bằng icon pack khác hoặc nhân bản asset từ client bên ngoài.

## Ma trận trạng thái thành phần

Đây là mức phủ **hiện tại**, không phải tiêu chuẩn đầy đủ đã được nghiệm thu:

| Thành phần | Normal | Hover | Pressed | Disabled | Selected | Focus |
| --- | --- | --- | --- | --- | --- | --- |
| Widget egui chuẩn | Role nền/chữ | Role hover | Role pressed | Disable thật + role/fade | Role selected | Viền focus nguồn |
| `icon_button` | Chữ chính | Nền hover | Nền pressed | Glyph disabled, khóa thao tác | Theo component gọi | Viền focus rõ |
| Rail | Icon navbar | Navbar hover | Navbar selected | Icon navbar disabled | Leftmenu selected | Viền foreground navbar |
| Hàng hội thoại/danh bạ | Nền/chữ theme | Nền hover | Nền pressed | Gate khi chưa có thao tác | Nền selected | Viền focus nguồn |
| Composer/nút gửi | Surface/primary | Primary hover | Primary pressed | Nền/chữ disabled, gate thật | Theo editor | Viền focus/input IME |

Mã hiện nối các trạng thái trên cho thành phần native; đây không phải tuyên bố mọi selector Electron được tái tạo pixel tương đương. Lỗi dùng role `--text-errors`; ảnh, focus bàn phím và thao tác vẫn phải nghiệm thu độc lập.

Cài đặt có Sáng/Tối/Hệ thống/Tùy chỉnh. Phần Nâng cao đóng mặc định, gồm editor token và catalog glyph phân trang 24 mục, không tải trước mọi glyph. Custom nhận màu hex của token màu; gradient hai điểm màu tối đa 256 byte áp dụng thật cho nền chat (`--surface-background`), rail (`--layer-background-navbar-normal`) và surface (`--layer-background`). Góc/vị trí/alpha được giữ trong mesh. Token gradient/layout khác giữ nguyên nguồn; không báo đã áp dụng vào component chưa dùng.

## Điều kiện gọi là hoàn tất

1. Catalog trích xuất hết nguồn trong phạm vi đã công bố, tái lập được và mọi liên kết tài sản được giải thích.
2. Có mapping hoặc lý do chưa áp dụng cho từng theme/token/glyph cần dùng; alias, override, component và sáu trạng thái được kiểm tra riêng.
3. Có kiểm thử native cho theme switching/persistence nếu triển khai, tên dài/tiếng Việt, focus/IME và controls disabled; glyph không thành click giả.
4. Có ảnh GUI tổng hợp rộng/compact cho phạm vi mới và kiểm tra tương phản/trạng thái. Ảnh hiện có chỉ xác nhận các thành phần đã vẽ, không phải toàn catalog.
5. Ghi rõ phần chưa triển khai trong `FEATURE_PARITY.md`; không dùng “đã kiểm kê” để thay “đã chạy đúng”.

Kiểm kê đầy đủ không yêu cầu nạp sẵn toàn bộ font, ảnh hoặc glyph. Chỉ nạp/vẽ tài sản cần cho thành phần đang dùng, giữ hàng đợi và ngân sách cache của `UI_DESIGN.md`. Ngân sách payload RGBA không phải phép đo tổng RAM/VRAM.
