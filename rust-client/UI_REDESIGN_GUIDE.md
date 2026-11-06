# Hướng dẫn agent: áp dụng bản thiết kế "Zalo Chat Redesign" (Sáng/Tối)

## Trạng thái và điều kiện bắt buộc

Đây là **hợp đồng đích** cho giao diện native, được chủ repo duyệt ngày 2026-11-06. [src/ui/AGENTS.md](src/ui/AGENTS.md), [UI_DESIGN.md](UI_DESIGN.md) và [UI_THEME.md](UI_THEME.md) đã được sửa để trỏ về đây: khi guide khác CSS Zalo gốc hoặc các đoạn "Hiện tại", guide thắng.

- **Đã migrate (2026-11-06):** bảng `Palette` Light/Dark (`src/theme/redesign.rs` đặt trên catalog gốc), role mới (`text_tertiary`, `border_strong`, `accent_soft`, `accent_ink`, `chip_on_bg`, `chip_on_text`, 6 màu avatar), rail 68 với nút bo 48, nút Sáng/Tối, panel danh sách (tiêu đề, ô tìm 44, chip, hàng 72), header 76, bong bóng, đường phân ngày, thẻ composer, nút icon 44, bo góc điều khiển chung 12, modal (bo 16, header 56, nút đóng 44, footer 44), sidebar thông tin (nút đóng/quay lại 44), màn Danh bạ (tiêu đề, chip Liên hệ/Nhóm/Cập nhật, ô tìm 44, hàng 72 với avatar 48 và bo 14, chip phân trang), nút đăng nhập 44 (36 khi cửa sổ compact). Kiểm bằng test (độ tương phản AA, giá trị token, hình học) và ảnh synthetic: 1200×780 và 640×480 cả hai theme, hover/pressed/focus bàn phím, modal cài đặt, sidebar thông tin, Danh bạ và đăng nhập QR; chạy được trên Wayland thuần (`DISPLAY` bỏ).
- **Chưa làm / lệch có chủ ý:** xem mục "Lệch so với thiết kế" cuối tài liệu. Mỗi lần migrate thêm, cập nhật mục đó và dòng này.
- Không thêm chế độ theme mới. Light, Dark, System, Custom đã tồn tại (`ThemeMode` trong [src/theme.rs](src/theme.rs)); bản thiết kế định nghĩa **giá trị** và **bố cục** mới cho Light/Dark. Custom vẫn kế thừa Light.
- Bản thiết kế dựng bằng HTML trên canvas Claude: https://claude.ai/artifact/J3UiUke1xqjd6mNDTcZwtT (riêng tư, chủ tài khoản phải chia sẻ). Bốn artboard: desktop sáng/tối (có nút đổi theme ở đáy rail, gửi tin cục bộ) và danh sách hội thoại mobile sáng/tối.
- Toàn bộ tên và tin nhắn trong bản thiết kế là **dữ liệu giả**. Không đưa vào fixture, ảnh chụp hay mã nguồn như dữ liệu thật.

## Token màu và ánh xạ sang `Palette`

Hai cột giá trị là giá trị trong bản thiết kế. Cột cuối là trường `Palette` ([src/theme.rs](src/theme.rs)) sẽ phải đổi nếu được duyệt. `—` nghĩa là chưa có role tương ứng, cần quyết định thêm role hay suy ra từ role sẵn có.

| Token thiết kế | Sáng | Tối | `Palette` |
| --- | --- | --- | --- |
| `bg` (nền khung chat) | `#F3F5F9` | `#0E1420` | `canvas` |
| `surface` (list, header, composer, bong bóng đến) | `#FFFFFF` | `#151D2B` | `surface`, `bubble_incoming` |
| `border` | `#E3E8F0` | `#263043` | `border`, `divider` |
| `border-strong` (ô nhập, chip tắt) | `#D5DCE8` | `#34415A` | — |
| `text` | `#121826` | `#EAF0FA` | `text_primary` |
| `text-2` (giờ, trạng thái, xem trước) | `#5B6577` | `#A3AEC2` | `text_secondary` |
| `text-3` (icon thanh công cụ) | `#3F4A5C` | `#C3CCDC` | — |
| `active` (dòng hội thoại đang chọn) | `#E8F1FF` | `#1C2B47` | `selected` |
| `accent` (nút gửi, bong bóng gửi, huy hiệu) | `#0068FF` | `#0068FF` | `primary`, `bubble_own` |
| `accent-soft` / `accent-ink` | `#EAF2FF` / `#0050C8` | `#1A2D4F` / `#8DB8FF` | — |
| `rail` / `rail-active` / `rail-text` | `#0A2540` / `#1B3A5C` / `#B9C6D8` | `#070B12` / `#1B2638` / `#8FA0B8` | `nav_background` / `nav_selected` / `nav_text` |
| `focus` | `#0A2540` | `#8DB8FF` | `focus` |
| `chip-on-bg` / `chip-on-text` | `#0A2540` / `#FFFFFF` | `#EAF0FA` / `#0E1420` | — |
| Chữ trên màu (avatar, bong bóng gửi) | `#FFFFFF` | `#FFFFFF` | `on_color` |

Quy tắc đi kèm:

- Màu avatar là tập cố định (`#C2185B #0068FF #0B7A55 #7A4BC9 #B4531A #00798C`), chữ trắng, dùng chung hai theme. Map sang `avatar_accents` (hiện 5 màu, bản thiết kế dùng 6).
- **Khác biệt đã duyệt so với Zalo gốc:** bong bóng gửi nền đặc `accent` chữ trắng (`on_color`), thay cho `--blue-message` nhạt (`#E5F1FF`) chữ tối. Bong bóng đến dùng `surface` có viền `border`.
- Không hard-code hex trong `ui/*.rs`. Mọi màu đi qua `crate::theme::palette(ctx)`.
- Độ tương phản đã chọn để chữ vừa đạt AA, nhưng **chưa đo bằng công cụ**. Đo lại trên pixel thật cả hai theme trước khi nhận vào.

## Bố cục (đơn vị logic egui)

| Phần | Thiết kế | Hiện tại |
| --- | --- | --- |
| Rail | 68 rộng, nút 48, logo 40 | 64 rộng |
| Danh sách | 344 rộng, dòng cao tối thiểu 72, avatar 48, bo 14 | 344 (kéo 280–420), bo 6 |
| Header chat | cao tối thiểu 76, avatar 44 | cao 68 |
| Tìm kiếm | cao 44, bo 12 | 32 |
| Chip lọc "Tất cả / Chưa đọc" | cao 44, bo tròn | chưa có trong tài liệu |
| Composer | khối bo 16, hàng công cụ trên, ô nhập + nút gửi dưới | theo `UI_DESIGN.md` |
| Vùng chạm | tối thiểu 44 | — |

- Rail, danh sách, chat giữ thứ tự hiện tại. Cửa sổ hẹp: danh sách thu lại sau nút quay lại, **giữ người nhận đã chọn và bản nháp** (đã là yêu cầu hiện hữu, không bỏ).
- Vùng vẽ phải khớp vùng tương tác (`Sense` rect). Nút 44 trong thiết kế có nghĩa là rect click 44, không chỉ icon 20.
- Thông tin phụ trong thiết kế cỡ 11–13. `UI_THEME.md` cấm chữ phụ 9–11,5 để tiết kiệm chỗ: **giữ sàn 12**, không theo cỡ 11 của giờ tin nhắn.

## Phông và icon

- Thiết kế dùng Inter chỉ vì canvas web không có Segoe. **Giữ Segoe UI nhúng** từ `pc-dist/fonts/`. Không thêm Inter vào binary.
- Icon trong thiết kế là SVG nét 1,8px vẽ tạm. Không dùng chúng. Dùng glyph gốc qua `visuals::icon` / `source_icon` và registry trong [UI_ASSET_INVENTORY.md](UI_ASSET_INVENTORY.md). Icon nào chưa có glyph gốc (ví dụ mặt trăng/mặt trời cho nút theme) thì để mục đó ở trạng thái "chưa ánh xạ", không vẽ thay.
- Nút đổi nhanh Sáng/Tối ở rail là phần mới của redesign (Zalo gốc chỉ có theme trong menu bánh răng, "Thiết lập giao diện"). Nút này chỉ đảo Sáng↔Tối và lưu qua cùng worker `config/theme.json`; nó không thay thế menu: System và Custom vẫn chọn ở đó, và khi đang ở System hay Custom thì nút chuyển sang Light/Dark tường minh. Chưa có glyph gốc cho mặt trăng/mặt trời thì nút dùng nhãn chữ hoặc để "chưa ánh xạ" trong kiểm kê.

## Trạng thái bắt buộc cho mỗi control tự vẽ

Bản thiết kế chỉ minh họa trạng thái thường và đang chọn. Khi triển khai phải làm đủ và kiểm ở cả hai theme: normal, hover, pressed, focus bàn phím, selected, disabled.

- Focus: viền 2px màu `focus`, lệch ngoài 2px. Trên rail dùng `nav_focus_outline`, không dùng `focus_outline`.
- Huy hiệu chưa đọc: ẩn ở hội thoại đang mở (như thiết kế); số lớn hơn 99 phải có quy tắc cắt, thiết kế chưa định nghĩa.
- Nút đang tắt (hành động chưa hỗ trợ, như gọi thoại, gửi ảnh) vẫn phải **disabled và nói rõ lý do**, không để bấm được như trong mockup. Đây là yêu cầu từ `src/ui/AGENTS.md` (hành động chưa hỗ trợ giữ disabled, không giả lập thành công).

## Dữ liệu và danh tính (không đổi)

Thiết kế hiển thị tên người gửi trên bong bóng nhóm khi người gửi đổi. Tuân thủ quy tắc hiện hữu: tên chỉ lấy từ snapshot trình bày có phạm vi; **tiêu đề nhóm không bao giờ là tên thành viên**; thiếu tên thì dùng nhãn trung tính. Không tải toàn bộ danh bạ để đặt tên bong bóng. Mọi truy vấn DB, mạng, giải mã ảnh nằm ngoài luồng vẽ.

## Cách triển khai nếu được duyệt

1. Đọc [UI_DESIGN.md](UI_DESIGN.md), [UI_THEME.md](UI_THEME.md), [src/ui/AGENTS.md](src/ui/AGENTS.md) trước.
2. Với từng hàng "—" trong bảng token, quyết định thêm role vào `Palette` hay suy ra, ghi lý do vào `UI_THEME.md` và cập nhật bảng token ở trên. Đổi giá trị Light/Dark trong `Palette::from_mode`. Custom vẫn kế thừa Light.
3. Đổi số đo bố cục trong `src/ui.rs` và các module `src/ui/*.rs`. Mỗi đổi số đo kèm test bố cục trong `layout_tests.rs`.
4. Bản demo ngoại tuyến vẫn phải gắn nhãn demo rõ ràng. Chạy `./scripts/rust/run.sh --demo` để xem.
5. Chụp ảnh tổng hợp ở **1200×780 và 640×480**, mỗi cỡ cả Sáng và Tối, kiểm cả tương tác: chọn hội thoại, lọc chưa đọc, soạn nháp, đổi theme, tên tiếng Việt dài.
6. Chạy `./scripts/rust/check.sh`. Báo riêng bằng chứng tổng hợp và bằng chứng máy chủ thật (không có đăng nhập thật thì nói rõ).
7. Không commit hoặc push khi chưa được yêu cầu.

## Lệch so với thiết kế (đã triển khai, có chủ ý)

- **Chip "Chưa đọc" bị tắt, không có huy hiệu chưa đọc.** `Conversation` chưa có số chưa đọc, nên lọc hay đếm sẽ là giả. Chip vẫn hiện, disabled, kèm lý do ở hover. Làm thật khi domain/store lưu trạng thái chưa đọc.
- **Bong bóng gửi nền xanh chỉ cho văn bản thuần.** Ảnh, nhãn dán, thẻ OA/tệp/bình chọn tự vẽ nhãn bằng vai trò chữ chuẩn nên giữ bong bóng `surface` có viền; đặt chữ tối lên nền xanh sẽ không đạt tương phản.
- **Nút Sáng/Tối dùng nhãn chữ** ("Tối" khi đang Sáng, "Sáng" khi đang Tối) vì catalog gốc không có glyph mặt trời/mặt trăng.
- **Phông giữ Segoe UI, icon giữ glyph gốc** (xem trên); tiêu đề "Tin nhắn" cỡ 18 theo vai trò heading, không phải 22.
- **Thanh công cụ composer** hiện một dòng gợi ý phím tắt/"CHỈ LƯU CỤC BỘ" ở bên phải khi thẻ rộng từ 480; hộp "Lưu/Gửi" giữ nhãn chữ vì demo và live khác nghĩa.
- **Đường phân ngày** chỉ còn viên thuốc giữa, bỏ hai đường kẻ hai bên.
- **Mật độ compact (cửa sổ cao dưới 600).** Nút đăng nhập giữ 36 thay vì 44 để QR, tab và nút Hủy còn vừa 640×480. Danh bạ ở cửa sổ rộng từ 520 gộp tiêu đề, chip và nút Cập nhật vào một hàng và ẩn hai dòng chú thích khi cao dưới 600, để còn khoảng 3,5 hàng liên hệ.

## Nghiệm thu phạm vi

Ngày 2026-11-06 chủ repo chấp nhận phạm vi migrate hiện tại là hoàn tất: các lệch có chủ ý ở mục trên được giữ nguyên (chip "Chưa đọc" tắt và không có huy hiệu, bong bóng media/thẻ giữ nền sáng, nút Sáng/Tối bằng nhãn chữ, mật độ compact). Hai mục dưới đây do chủ máy tự nghiệm thu, không phải việc còn dở của code.

## Còn để mở

Phần code giao diện đã migrate hết các màn hình dựng được offline. Hai mục dưới đây không phải việc code mà là bằng chứng cần môi trường thật, nên chỉ chủ máy/tài khoản nghiệm thu được:

- **Tài khoản thật và một phiên desktop thật.** Mọi ảnh và test là synthetic; không có dữ liệu thật trong ảnh hay log.
- **IME trên Wayland.** Cần một engine nhập liệu thật; hiển thị trên Wayland thuần đã kiểm, nhập liệu thì chưa.

## Đối chiếu nhanh với bản thiết kế

Muốn xem nguyên trạng giá trị, đọc `project/Main.dc.html` trong artifact: toàn bộ màu nằm trong hàm `themeVars()` (hai bảng Sáng/Tối), toàn bộ số đo nằm inline ở markup.
