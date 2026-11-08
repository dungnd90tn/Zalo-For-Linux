# Thiết kế UI native Zalo

## Mục đích và phạm vi

Tham chiếu cho agent sửa UI Rust: theo bản thiết kế lại đã được duyệt (xem [UI_REDESIGN_GUIDE.md](UI_REDESIGN_GUIDE.md)), đúng logic và phản hồi nhanh. “Hiện tại” mô tả mã nguồn, vẫn là diện mạo Zalo gốc cho tới khi được migrate; “Yêu cầu” là điều kiện chấp nhận, không chứng nhận tính năng hoàn tất. Màu, số đo bố cục và trạng thái control mục tiêu nằm trong guide; khi guide và các bảng “Hiện tại” bên dưới khác nhau, guide là đích, bảng là hiện trạng.

Client dùng egui/eframe native, không WebView/Electron. Không sao chép mã hoặc tài sản từ client Rust khác. Mục tiêu vẫn là tương đương bản Electron Linux đang hoạt động; preview chưa đạt đầy đủ. Theo dõi khoảng trống tại [FEATURE_PARITY.md](FEATURE_PARITY.md).

## Nguồn chuẩn và theme

[UI_THEME.md](UI_THEME.md) là nguồn đối chiếu theme, selector CSS và provenance tài sản; [ARCHITECTURE.md](ARCHITECTURE.md) quy định kiến trúc. Áp dụng tại [visuals.rs](src/ui/visuals.rs). Bảng dưới là giá trị Light **hiện tại** (thiết kế đã duyệt, đặt trong [redesign.rs](src/theme/redesign.rs)); giá trị Dark và toàn bộ ánh xạ vai trò nằm ở bảng token trong [UI_REDESIGN_GUIDE.md](UI_REDESIGN_GUIDE.md). Catalog token gốc vẫn là nguồn đối chiếu cho token không có trong guide: kiểm tra nguồn thay vì tự chọn màu gần giống.

| Vai trò | Giá trị Light |
| --- | --- |
| Thanh điều hướng / mục chọn | `#0A2540` / `#1B3A5C` |
| Nút chính, bong bóng gửi | `#0068FF` |
| Chữ chính / phụ | `#121826` / `#5B6577` |
| Placeholder / disabled | `#6E7789` / `#8B96A7` |
| Nền chat / nền trắng | `#F3F5F9` / `#FFFFFF` |
| Vùng chọn / hover | `#E8F1FF` / `#F3F5F9` |
| Viền | `#E3E8F0` (đặc, không alpha) |

Font Segoe UI regular/Semibold/Bold và hai icon font gốc khởi tạo một lần; metadata 12, body/button 14, nội dung chat 15, heading 18, tiêu đề modal 16 đơn vị logic egui. Không dùng chữ phụ 9–11,5 để tiết kiệm chỗ. Catalog giữ toàn bộ token và 1.034 glyph/alias đã kiểm cmap; raster chỉ khi dùng. [Kiểm kê](UI_ASSET_INVENTORY.md) phân biệt nguồn đầy đủ với thành phần native.

Bấm bánh răng mở menu; chọn “Thiết lập giao diện” để mở Sáng/Tối/Hệ thống/Tùy chỉnh, lưu riêng trong profile bằng worker. Khi đã đăng nhập, mục “Đăng xuất” riêng chỉ mở hộp xác nhận; Hủy giữ phiên và bản nháp. Màn đăng nhập chỉ có mục giao diện. System theo tín hiệu egui/winit; thiếu tín hiệu dùng Light, chưa xác nhận mọi desktop Linux. Custom kế thừa Light, ghi đè màu/reset; gradient hai điểm màu hỗ trợ nền chat, rail và surface. Token/editor và danh mục icon nằm trong Nâng cao, đóng mặc định; không tự bật tính năng chưa hỗ trợ. Nút Sáng/Tối trên rail (nhãn chữ vì chưa có glyph gốc) chỉ đảo Sáng↔Tối và lưu qua cùng worker; System và Tùy chỉnh vẫn chọn trong Cài đặt.

## Bố cục và thành phần

Hiện tại, [ui.rs](src/ui.rs) tổ chức rail, danh sách và khung chat theo bản thiết kế đã duyệt. Rail rộng 68 với nút bo tròn 48; danh sách mặc định 344, kéo được trong khoảng 280–420, hàng hội thoại cao 72, ô tìm kiếm và chip lọc cao 44; header chat cao 76.

Dưới chiều rộng 940, danh sách/danh bạ chuyển thành màn riêng; nút quay lại giữ hội thoại và bản nháp. Vùng nội dung cửa sổ mặc định 1280×720 đơn vị logic (16:9), tối thiểu 640×480. Composer là một thẻ bo 16 (thanh công cụ 44 phía trên, ô soạn và nút gửi 72×44 phía dưới), vùng cấp phát mặc định cao 136; chỉ tăng lên 168 khi bản nháp có ít nhất ba dòng phân cách bằng Enter, cửa sổ không ở chế độ compact và chiều cao còn lại từ 340. Nội dung dài cuộn trong editor; Enter gửi ngoài IME composition, Ctrl+Enter (hoặc Shift+Enter) xuống dòng. Kích thước này theo bản thiết kế lại; thanh công cụ và vùng nhập vẫn co theo nội dung.

Cả hàng hội thoại nhận click, gồm avatar, tên, giờ, preview và khoảng trống; text vẽ trong cùng hitbox, không tạo widget con giành pointer. ID tương tác gắn tài khoản/thread. Bong bóng text đo nội dung, tên người gửi và trạng thái để ôm tin ngắn; giới hạn xuống dòng vẫn dành đủ chỗ cho padding và avatar, không ép mọi tin gửi rộng tối đa.

Thông tin hội thoại nằm ở sidebar phải rộng 344 khi còn đủ ít nhất 360 cho chat. Nội dung gồm: avatar, tên và loại hội thoại; hàng hành động nhanh (Tắt thông báo và Ghim luôn tắt kèm lý do vì cài đặt đó nằm trên máy chủ Zalo; chỉ "Sao chép tên" hoạt động); số tin nhắn và khoảng thời gian đã lưu; và ba tab Ảnh / Tệp / Liên kết liệt kê tối đa 50 mục gần nhất từ lịch sử đã lưu, kèm tổng số. Dữ liệu do worker đọc từ SQLite (`conversation_overview`), tuân theo cùng cơ chế che hội thoại ẩn như trang tin nhắn và gắn với thế hệ chọn hội thoại; hàng chỉ mở liên kết http(s) đã kiểm tra khi người dùng bấm. Khi hẹp, trang thông tin thay khung chat; quay lại giữ đúng hội thoại/bản nháp. Nội dung chỉ dùng dữ liệu đã lưu, đóng khi đổi tài khoản/thread; không dựng nút thao tác chưa được nối.

Resize/maximize cùng DPI chỉ tăng vùng đọc, không nhân cỡ chữ hay chiều rộng rail/list/sidebar, chiều cao header hoặc hitbox. Ảnh/sticker giữ tỉ lệ, có giới hạn kích thước và không phóng vượt số pixel nguồn; viewer căn ảnh giữa hai chiều trong khung tối đa 740×540, chỉ thu nhỏ khi cửa sổ thiếu chỗ; chú thích cuộn riêng để liên kết ảnh gốc vẫn tới được. Khi DPI màn hình đổi, toolkit raster lại font theo mật độ pixel thật; không ép zoom/DPI toàn ứng dụng hoặc kéo giãn bitmap.

Modal dùng `dialog.rs`: curtain theo token, header cao 56, tiêu đề 16, glyph đóng 24 trong hitbox 44, viền bo 16, body cuộn có padding 14×16. Footer là hàng nút cao 44 cộng padding, không kéo giãn theo chiều cao cửa sổ. Khi modal mở, chặn cả pointer, phím tắt và dispatch nền; đóng modal chỉ khôi phục focus khi tài khoản/thread/generation còn khớp. Settings, mở hội thoại, đăng xuất, kiểm tra gửi và thông tin ứng dụng dùng cùng quy tắc. Popup avatar/bánh răng cũng chặn thao tác nền; Escape, bấm ngoài hoặc bấm lại nút neo để đóng. Vị trí popup bám nút neo khi resize; đổi tài khoản, thế hệ đăng nhập hoặc hội thoại hủy popup và không khôi phục focus sang scope khác.

| Thành phần | Nơi sửa chính |
| --- | --- |
| Đăng nhập, QR, thử lại | [login.rs](src/ui/login.rs), [live.rs](src/ui/live.rs) |
| Danh bạ, nhóm, phân trang | [directory.rs](src/ui/directory.rs) |
| Tên người gửi | [senders.rs](src/ui/senders.rs) |
| Avatar tài khoản, liên hệ và nhóm | [avatars.rs](src/ui/avatars.rs) |
| Lượt gửi chưa rõ kết quả | [delivery.rs](src/ui/delivery.rs) |
| Thumbnail nhãn dán | [stickers.rs](src/ui/stickers.rs) |
| Ảnh, thẻ OA và viewer | [media.rs](src/ui/media.rs), [worker ảnh](src/app/media.rs) |

Yêu cầu: tiêu đề dài phải rút gọn, có thể xem tên đầy đủ qua hover; nội dung tin vẫn xuống dòng. Composer và nút gửi không tràn khi resize. Control phải thực hiện hành vi đã nối hoặc thực sự disabled kèm giải thích; không tạo biểu tượng bấm được nhưng không làm gì.

## Luồng đăng nhập và kết nối

Hiện tại, mở `--live` tự gửi lệnh startup đến worker. Worker đọc phiên: hợp lệ thì xác minh và mở chat; chưa có phiên thì tạo QR. Chỉ xác nhận hết hiệu lực đăng nhập hoặc yêu cầu đổi danh tính client mới chuyển sang QR mới. Lỗi mạng, HTTP, cấu hình hoặc lưu trữ giữ phiên để thử lại.

Hủy/đăng xuất không tự chạy lại startup trong cửa sổ hiện tại. Generation loại bỏ kết quả cũ. Đăng nhập số điện thoại chưa hỗ trợ; tab hiện chỉ giải thích giới hạn, giữ lần QR đang chạy và cho phép hủy. Nhóm nút dưới QR có khung hữu hạn, căn cùng trục giữa với mã QR; thao tác chuyển tab không bắt đầu một phiên xác thực khác.

Trạng thái Online chỉ đến từ xác thực transport, hiển thị bằng chấm cạnh avatar trên rail; không suy diễn online của từng người. Bấm avatar xem trạng thái, nguyên nhân lỗi an toàn và thao tác kết nối lại/mở hội thoại; phiên hết hạn hướng dẫn đăng xuất qua menu bánh răng. Không đặt banner trạng thái hoặc footer tài khoản/đăng xuất thường trực. Khi Offline và đã xác minh quyền riêng tư trong worker hiện tại, vẫn cho đọc cache được phép, chọn liên hệ và soạn nháp. Lỗi batch lưu dữ liệu tạm dừng transport, giữ lịch sử/cursor và thao tác cục bộ.

## Hội thoại ẩn trên Zalo

Mỗi worker mới phải lấy thành công trạng thái hội thoại ẩn từ Zalo trước khi hiện tên, danh bạ, lịch sử hoặc ảnh. Khi chưa biết hoặc kiểm tra thất bại, chỉ vẽ một workspace trung tính sau rail, cả ở cửa sổ rộng và compact: Checking hiện “Đang xác minh…”, Failed hiện “Chưa xác minh được…”. Avatar giải thích trạng thái và cho thử lại; menu bánh răng vẫn dùng được. Không dùng snapshot cũ để mặc định hiện nội dung khi khởi động lại.

Danh sách thường luôn bỏ hội thoại ẩn, kể cả khi đang mở khóa. Tìm theo tên/ID mới hiện ứng viên chỉ gồm tên công khai và glyph khóa; không preview, giờ hay số chưa đọc. Tên trống/ID thô dùng “Hội thoại ẩn”. Bấm ứng viên hoặc mở ID đã ẩn đều yêu cầu mã PIN Zalo có sẵn: đúng bốn chữ số ASCII, che ký tự, không lưu/log PIN hoặc verifier. Sai PIN không mở; Hủy vô hiệu cả yêu cầu đang chờ.

Xác minh đúng chỉ mở hội thoại đang yêu cầu. Quay lại, đổi hội thoại/danh bạ, đóng worker hoặc hết phiên khóa lại; kết quả cũ không được mở lại nội dung. Xóa cache trình bày khi khóa nhưng giữ nháp, lịch sử thô, cursor và ACK/outbox; ACK tương ứng vẫn giải quyết lượt gửi cũ, không xóa phần sửa mới.

Worker kiểm tra khi khởi động, lấy focus/thử lại và theo chu kỳ khoảng 30 giây sau lần kiểm tra hoàn tất. Thời điểm nhận thay đổi từ điện thoại còn phụ thuộc thời gian HTTP (deadline 15 giây), không bảo đảm cập nhật trong tối đa 30 giây. Refresh cùng policy giữ quyền đang mở; lỗi hoặc policy/PIN thay đổi thu hồi quyền. Policy/lease nằm trong RAM của Rust và SQLite TEMP memory, không ghi vào schema bền vững. Schema hiện tại là 8 để lưu projection thẻ tệp. PIN chỉ kiểm soát quyền hiển thị trong app, không mã hóa SQLite. Chưa hỗ trợ ẩn/bỏ ẩn, đổi/reset PIN hoặc kênh cập nhật tức thì từ client native.

Kiểm thử synthetic UI và TLS loopback xác minh các nhánh tương tác, scope và storage; không thay thế kiểm chứng hidden/PIN trên tài khoản thật. Nguồn đối chiếu là hidden manager `EFQ6` (`_updateAllDataHidden`/`isKeyPIN`) trong `pc-dist/compact-app-pc.e08d0d44f38873747a6b.js`.

## Tin nhắn và danh tính

Hiện tại, tin đến nằm trái, tin của tài khoản nằm phải; thời gian/ngày phân tách dùng dữ liệu đã lưu. Nhận ACK từ máy chủ không đồng nghĩa người nhận đã đọc.

Thông báo `webchat` / `msginfo.actionlist` hiển thị văn bản tiếng Việt từ template, tên gợi ý và tiêu đề gốc. Toàn bộ cấu trúc được giữ riêng trong SQLite, kể cả action và trường chưa biết; UI hiện chỉ đọc nội dung, chưa thực thi action hoặc tải URL/ảnh trong thông báo. Không biến object thành JSON thô hay placeholder làm mất dữ liệu, và không dùng sự kiện thông báo để xác nhận tin gửi.

Ảnh nhận có thumbnail, tiêu đề/chú thích và viewer native dùng pixel đã cache; ảnh gốc chỉ mở trình duyệt khi bấm. Thẻ OA dùng preview tiếng Việt đã đối chiếu nguồn và metadata; mẫu remote chưa biết phải giải thích giới hạn, giữ toàn bộ dữ liệu, không chạy JavaScript/action. History snapshot chỉ chứa projection tối đa 16 KiB; raw content thuộc worker/SQLite. Tham chiếu [PROTOCOL_MEDIA.md](PROTOCOL_MEDIA.md) cho codec/CDN và điều kiện làm giàu placeholder cũ.

`share.file` dùng thẻ tệp với tên, phần mở rộng, dung lượng và glyph gốc. Chỉ liên kết hợp lệ từ tin nhắn mới có thao tác “Mở tệp trong trình duyệt”; không tự tải, thực thi hoặc tạo nút mở giả. Tệp thiếu URL hoặc có khóa mã hóa giải thích giới hạn. Nội dung gốc vẫn được lưu, chưa có trình tải tệp/giải mã native.

`chat.recommended` dùng thẻ native đọc nội dung chia sẻ; chỉ action liên kết đã đối chiếu nguồn cho phép mở trình duyệt khi bấm. `group.poll` dùng glyph bình chọn gốc, thông báo/câu hỏi có trong tin và trạng thái ẩn danh khi nguồn có boolean rõ ràng. Chưa tải chi tiết lựa chọn/kết quả hay gửi phiếu; không tạo số liệu hoặc nút bỏ phiếu giả. Metadata mới cập nhật đúng tin trên trang đang mở, giữ thứ tự, vị trí trang và bản nháp.

Tên lấy từ cache danh bạ đúng tài khoản. DM chỉ dùng tiêu đề hội thoại khi thread và người gửi khớp. Nhóm không dùng tên nhóm làm tên thành viên. Chưa biết tên thì hiện “Người gửi”, avatar `?`; không thay tên bằng ID kỹ thuật.

Avatar lấy URL đã đồng bộ cho tài khoản, liên hệ hoặc nhóm khi vị trí tương ứng đang hiển thị; thiếu ảnh dùng chữ viết tắt. Rail, danh sách, header, thông tin hội thoại và bubble cắt ảnh tròn, giữ tỉ lệ; `g:id` và `d:id` khác nhau, ảnh nhóm không thay ảnh thành viên. Metadata dùng lookup có giới hạn, không đọc toàn bộ danh bạ. Tải/giải mã dùng pipeline media ở nền, chỉ từ CDN Zalo, không cookie và không theo redirect; ảnh đầu ra tối đa 256 pixel mỗi chiều. Đổi tài khoản, worker, quyền riêng tư hoặc khóa lại chat ẩn loại cache/kết quả cũ trước khi vẽ; metadata và pixel đều được kiểm scope.

Tra tên dùng snapshot có scope tài khoản/trang/thread và epoch riêng. Chỉ một lookup đang chờ; cập nhật danh bạ được gộp, bỏ câu trả lời cũ và giữ nguyên tin/bản nháp. Không đổi `Message.sender`, ID hoặc điều kiện bảo mật để sửa hiển thị.

## Nhập liệu và khả năng tiếp cận

Toàn vùng nhập của composer nhận focus; nội dung nhiều dòng cuộn bên trong. Enter gửi khi đủ điều kiện; Ctrl+Enter (hoặc Shift+Enter) xuống dòng. Enter không bao giờ chèn dòng mới, và không gửi khi IME đang soạn, trong cùng khung với commit hay khi giữ phím lặp. Bản nháp, cursor/undo và IME gắn với tài khoản/hội thoại, không bị xóa khi đổi theme.

Bản vá [egui-winit](vendor/egui-winit/LINUX_IME_PATCH.md) nhận preedit/commit Linux, không để context Enabled khóa Backspace/phím mũi tên; đổi editor tạo lại context. Không gửi trong composition, frame commit hoặc do phím giữ lặp sau commit.

Yêu cầu: kiểm thử bằng x-unikey thật, không chỉ chèn Unicode. Kiểm tra Tab, focus sau đổi cửa sổ/hội thoại, Backspace, chuột và nội dung tiếng Việt. Duy trì chữ dễ đọc, trạng thái disabled phân biệt được; đo tương phản nếu đổi token. Chưa có chứng nhận WCAG hoặc kiểm thử screen reader đầy đủ.

## Dữ liệu bất đồng bộ và ngân sách

UI chỉ gửi command, nhận event đã kiểm tra scope và vẽ. HTTP/WSS, SQLite, đọc phiên và giải mã ảnh thuộc worker. Tin/preview chỉ công bố sau lưu thành công; ACK chưa lưu được phải giữ cảnh báo không tự gửi lại.

| Giới hạn hiện tại | Giá trị |
| --- | --- |
| Hàng đợi command / event messaging | 16 / 4 |
| Drain UI live mỗi frame | Tối đa 8 event, ngân sách 3 ms |
| Tin trong pane / sidebar | 50 / tối đa 200 hội thoại |
| Tra tên người gửi | 64 ID/lượt; tên tối đa 512 byte |
| Danh bạ | 100 dòng/trang; SQLite tối đa 30.000 dòng |
| Texture sticker | 64 mục; payload RGBA 8 MiB |
| Cache sticker phía worker | 64 mục; payload RGBA 8 MiB |
| Cache ảnh media phía worker / GPU | Mỗi bên 32 mục; payload RGBA 16 MiB |
| Hàng đợi request / result ảnh media | 16 / 2; một decoder toàn process |
| Tra metadata avatar / cache metadata UI | Tối đa 64 ID/lượt / 256 mục |
| Cache GPU avatar | Tối đa 64 ảnh; payload RGBA 4 MiB |

Nhãn dán hiện chỉ có thumbnail PNG tĩnh; ảnh hiển thị tối đa 130 đơn vị, lỗi có fallback. Tải ở nền, chỉ cho live/ảnh cần hiển thị, không mang cookie sang CDN. Demo không truy cập Internet. Giữ backoff 30 giây cho tải lỗi; không tải lại mỗi frame hoặc giữ RGBA sau upload GPU nếu không cần.

Yêu cầu: không thêm SQL/network vào render, cache không giới hạn hoặc repaint liên tục để chờ I/O. Ngân sách payload không đo tổng RAM/VRAM; đo trước khi so với Electron.

## Bằng chứng và nghiệm thu

Mốc tên người gửi đã qua kiểm tra kỹ thuật và ảnh GUI tổng hợp; kết quả cập nhật trong [VALIDATION.md](VALIDATION.md). Điều này không thay thế nghiệm thu tài khoản thật. X11/IBus có kiểm thử Telex ON/OFF và chuyển focus; native Wayland chưa được xác nhận tương đương.

GUI không mạng dùng `--demo` hoặc fixture TLS cục bộ; `--live` tự đăng nhập.

Khi sửa UI:

1. Đọc tài liệu nguồn và quy tắc agent; ghi rõ hành vi thay đổi, phần chưa hỗ trợ.
2. Thêm kiểm thử cho logic có rủi ro: scope, stale event, ACK, bản nháp, IME; không chỉ kiểm tra ảnh không trắng.
3. Chạy kiểm tra phù hợp theo `VALIDATION.md`, giữ formatting và Clippy sạch.
4. Chụp riêng cửa sổ 1200×780 và 640×480 bằng hồ sơ tổng hợp; kiểm tra QR/retry, DM/nhóm, tên dài/không rõ tên, typing và resize.
5. Ghi điều kiện, kết quả, giới hạn; không dùng dữ liệu tài khoản thật trong ảnh/log/commit.

Ảnh tổng hợp tại `/tmp/zalo-rust-gui-check/`; tái tạo theo `VALIDATION.md`.

Các khoảng trống nghiệp vụ nằm tại `FEATURE_PARITY.md`; không suy diễn mức hoàn tất từ icon/theme.
