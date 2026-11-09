# Changelog

Nhật ký thay đổi của **Zalo Native** (ứng dụng Rust trong `rust-client/`). Định dạng theo
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/); phiên bản theo `Cargo.toml`.
Ghi chú phát hành dành cho người dùng nằm trong [changelogs/](../changelogs/RELEASE-v2.0.0.md).

## [Unreleased]

### Thêm mới
- Menu chuột phải trong bong bóng cho phép **sao chép tin nhắn**, chú thích ảnh, tên tệp,
  nội dung thẻ và liên kết. Menu giữ nguyên bản nháp, đóng khi đổi hội thoại/tài khoản
  hoặc khóa lại chat ẩn; chưa hỗ trợ sao chép pixel ảnh hay định dạng HTML.
- **Tải ảnh gốc** từ thẻ ảnh hoặc cửa sổ xem ảnh: lưu nguyên byte vào Tải xuống/Zalo Native,
  có tiến độ, hủy và thử lại; tối đa 100 MiB và không ghi đè tệp. Hủy vẫn dừng được khi máy chủ
  đang treo; ảnh thiếu liên kết gốc khả dụng không dùng thumbnail để thay thế.
- Tìm kiếm lịch sử đã lưu có nút **Tìm cũ hơn** và **Mới nhất**. Mỗi trang tối đa 50 kết quả;
  có thể tìm tiếp sau mỗi lượt quét 20.000 tin, kể cả khi trang hiện tại chưa có kết quả.
  Tin mới đến không làm dịch trang cũ; lỗi tải trang giữ kết quả và mốc tiếp tục để thử lại.

## [2.0.3] - 2026-11-08

### Thêm mới
- **Tìm tin nhắn trong hội thoại** (panel *Thông tin hội thoại* → *Tìm tin nhắn*): tìm không dấu, không phân biệt
  hoa thường trong lịch sử đã lưu trên máy (không hỏi máy chủ Zalo). Ảnh, tệp, thẻ liên kết và bình chọn được tìm
  theo chú thích/tên tệp/tiêu đề; nhãn dán và chuỗi giữ chỗ "[Tin nhắn … chưa được hỗ trợ]" không bao giờ khớp.
  Tối đa 50 kết quả mới nhất, mỗi lần quét tối đa 20 000 tin; khi có thể còn kết quả cũ hơn thì ghi rõ.
- Bấm một kết quả sẽ mở đúng trang chứa tin đó (kèm khoảng 10 tin mới hơn), cuộn tới và tô sáng tin trong vài giây;
  ở cửa sổ hẹp panel thông tin tự đóng để thấy tin.
- Hội thoại ẩn (mã PIN) hoặc khi trạng thái riêng tư chưa xác minh: tìm kiếm không trả về gì, giống phân trang
  lịch sử; kết quả cho hội thoại bị khóa bị lọc trước khi tới giao diện.

## [2.0.2] - 2026-11-08

### Sửa lỗi
- Tên và ảnh đại diện của nhóm và bạn bè cập nhật khi mở hội thoại, không cần khởi động
  lại: mở hội thoại sẽ đồng bộ lại danh sách bạn bè và nhóm (tối đa một lần mỗi 2 phút, không chạy chồng).

## [2.0.1] - 2026-11-08

### Thay đổi
- **Không còn màn hình "Đang xác minh trạng thái riêng tư" che giao diện.** Danh sách hội thoại ẩn (mã PIN) mà Zalo
  xác nhận lần gần nhất được nhớ trên máy (chỉ mã hội thoại) và áp dụng ngay khi mở ứng dụng; kiểm tra lại chạy nền.
  Kiểm tra lỗi không còn khóa giao diện và không còn chặn kết nối nhận tin. Lần chạy đầu tiên chưa có danh sách nhớ.
- `scripts/rust/run.sh --live` dựng kèm `signal-crypto`, nên hội thoại mã hóa đầu cuối gửi/nhận được.
  `ZALO_RUST_NO_E2EE=1` dựng bản `live` không có engine.

### Sửa lỗi
- **Hội thoại "lạ" trong lịch sử:** thiệp sinh nhật (`chat.ecard` loại 1) và tin quảng bá của Official Account
  (ZInstant) không còn tạo/đẩy hội thoại lên danh sách, giống `skipUpdateConvListByPreviewItem` /
  `shouldShowOAInConvList` của bản gốc; tin vẫn nằm trong lịch sử. Dữ liệu cũ được dọn một lần khi mở cơ sở dữ liệu.
- Nhật ký cuộc gọi hiện "Cuộc gọi thoại đến · 1 phút 30 giây", "Cuộc gọi thoại nhỡ"… thay cho tiêu đề
  `sendBubbleMessage`; bản ghi đã lưu được dựng lại từ nội dung gốc.

### Thêm mới
- Hiển thị chữ cho `chat.ecard`, `chat.gif`, `chat.voice`, `chat.video.msg`, `chat.location.new`, `chat.doodle`,
  `chat.todo`, `chat.undo`, `chat.delete.everyone`, `chat.list.action` theo chuỗi xem trước của bản gốc. Tin đã lưu
  dưới dạng "[Tin nhắn … chưa được hỗ trợ]" được thay khi máy chủ gửi lại.

## [2.0.0] - 2026-11-06

### Thay đổi có thể làm hỏng thói quen
- **Phím gửi tin:** `Enter` gửi tin; `Ctrl+Enter` (hoặc `Shift+Enter`) xuống dòng. Trước đây `Enter`
  xuống dòng và `Ctrl+Enter` gửi. `Enter` không bao giờ chèn dòng mới, và không gửi khi bộ gõ (IME)
  đang soạn, trong cùng khung với lần commit, hoặc khi giữ phím lặp.
- Số phiên bản đổi từ `0.1.0` sang `2.0.0`; `zalo-rust --version` và các gói cài đặt dùng cùng số này.

### Thêm mới
- **E2EE trong luồng gửi/nhận bình thường (cần bản dựng có `signal-crypto`)**: soạn tin trong hội thoại mã hóa đầu cuối
  không còn bị từ chối. Nếu thiết bị chưa có phiên với người nhận, ứng dụng làm bắt tay (560, 541 `isReInit`, 543) rồi
  mã hóa và gửi qua `chat_e2e`, giống `sendE2eeMsg` của bản Electron (thử lại tối đa 3 lần; lỗi -3/-5 không thử lại).
  Tin mã hóa đẩy về (551) được giải mã và lưu mà không cần biến môi trường. Bảng đối chiếu từng hành vi gốc:
  [E2EE_PARITY.md](E2EE_PARITY.md).
- **Giao diện thiết kế lại, chế độ Sáng và Tối** ([hướng dẫn](UI_REDESIGN_GUIDE.md)):
  - bảng màu mới trong `src/theme/redesign.rs`, đặt trên danh mục token gốc; Tùy chỉnh vẫn ghi đè được;
  - rail 68 px với nút bo tròn 48 px và nút đổi nhanh **Sáng/Tối** (nhãn chữ, vì chưa có glyph gốc);
  - panel danh sách có tiêu đề, ô tìm kiếm 44 px, chip lọc, hàng hội thoại 72 px;
  - header chat 76 px, bong bóng góc bất đối xứng (tin gửi nền xanh, chữ trắng), đường phân ngày dạng viên thuốc;
  - composer dạng thẻ bo 16 px; nút biểu tượng 44 px; modal bo 16 px;
  - màn Danh bạ, sidebar thông tin và đăng nhập theo cùng ngôn ngữ thiết kế; bố cục gọn cho cửa sổ nhỏ.
- **Gói cài đặt `.deb` và `.rpm`** (`scripts/rust/package.sh`): cài bằng `apt`/`dnf`/`yum`,
  biên dịch trong Ubuntu 20.04 (glibc 2.31), có kiểm tra cài/gỡ trong container sạch
  (`scripts/rust/package/verify.sh`).
- Công cụ chụp ảnh giao diện từ framebuffer của chính ứng dụng (`ui/shot_fixture.rs`, bài kiểm thử
  `--ignored`); không chụp màn hình desktop.
- Các vai trò màu mới trong `Palette`: `text_tertiary`, `border_strong`, `accent_soft`, `accent_ink`,
  `chip_on_bg`, `chip_on_text`; bộ 6 màu avatar.

- **Sticker động và nhiều định dạng:** nhận dạng định dạng theo chữ ký tệp (không tin `Content-Type`):
  PNG tĩnh, **sprite PNG nhiều khung** (2–64 khung), JPEG, WebP tĩnh/động và GIF. Sticker động chạy
  theo đồng hồ khung, đồng bộ giữa mọi bản sao trên màn hình, chỉ chạy khi cửa sổ đang được lấy nét.
  Tải tài nguyên sprite trước rồi mới lùi về ảnh tĩnh.
- **Tin sticker không có bong bóng:** chỉ còn hình sticker (không nền, không viền, không đệm), rộng vừa
  khít hình thay vì rộng bằng cả bong bóng.
- **Thông tin hội thoại đầy đủ hơn:** hành động nhanh (Sao chép tên; Tắt thông báo và Ghim hiện
  nhưng tắt, vì cài đặt đó nằm trên máy chủ Zalo), số tin nhắn và khoảng thời gian đã lưu, cùng ba tab
  **Ảnh / Tệp / Liên kết** liệt kê các mục từng xuất hiện trong lịch sử đã lưu (50 mục gần nhất, kèm tổng
  số; liên kết trong tin nhắn thường được nhận dạng và loại trùng). Dữ liệu đọc ở worker, tôn trọng
  hội thoại ẩn.
- CI (`.github/workflows/build-packages.yml`) build gói `.deb`/`.rpm` của ứng dụng Rust khi phát hành,
  thay cho bản build AppImage Electron; có kiểm tra thẻ phát hành khớp phiên bản và kiểm cài đặt bằng
  `apt`/`dnf` trước khi tải lên.

- **Đính kèm tệp (giao diện và xử lý cục bộ):** nút ghim giấy mở hộp thoại chọn tệp của desktop
  (`zenity`/`kdialog`), hoặc kéo-thả một tệp vào cửa sổ; khay phía trên ô soạn cho thấy tên, loại, dung
  lượng và nút bỏ. Tệp hợp lệ khi là tệp thông thường, không rỗng, tối đa 100 MB (như bản gốc), tên hợp
  lệ; tệp thuộc về bản nháp của từng hội thoại. Ở chế độ demo, tin tệp được lưu cục bộ. **Chưa gửi được
  tới Zalo**: giao thức tải lên chưa được kiểm chứng với máy chủ thật, nên bản live giữ nút tắt kèm lý
  do. Luồng gốc đã được ghi lại trong [PROTOCOL_FILES.md](PROTOCOL_FILES.md).

- **Tải tệp đã nhận (bản live):** thẻ tệp có nút **Tải về** (chỉ khi bạn bấm). Liên kết phải là HTTPS trên
  CDN Zalo (`*.zdn.vn`/`*.zadn.vn`), không cookie, không theo chuyển hướng; tối đa 100 MB. Tệp được ghi
  theo luồng vào `Tải xuống/Zalo Native` (theo `XDG_DOWNLOAD_DIR`), qua tệp `.part` rồi đổi tên, không bao
  giờ ghi đè (thêm ` (1)`, ` (2)`…), quyền `0600` không có bit thực thi, tên được làm sạch (bỏ đường dẫn,
  ký tự điều khiển/đảo chiều). Có thanh tiến độ, nút Hủy (xóa phần đã tải), Thử lại và mở thư mục chứa
  tệp. Lỗi máy chủ hiện rõ mã HTTP và được ghi vào nhật ký chẩn đoán (chỉ mã trạng thái và số byte), để
  đối chiếu chiều tải tệp với máy chủ thật. Tệp mã hóa đầu cuối và thư mục vẫn chưa hỗ trợ.

- **Thăm dò E2EE chỉ-đọc (tùy chọn, mặc định tắt):** đặt `ZALO_E2EE_PROBE=/đường/dẫn/tuyệt/đối.txt` khi chạy
  `--live` để ghi cấu trúc phản hồi lệnh 564/541 của chính tài khoản (tên trường, kiểu, độ dài; không có
  khóa, mã hay nội dung). Không thay đổi gì trên tài khoản. Đây là bước 1 của
  [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md); gửi/nhận E2EE vẫn chưa hỗ trợ.

- **Thay khóa E2EE PC của tài khoản (tùy chọn, mặc định tắt, chưa thử với máy chủ thật):** đặt
  `ZALO_E2EE_REPLACE_PC_KEY=/đường/dẫn/tuyệt/đối.txt` khi chạy `./scripts/rust/run.sh --live`. Tạo khóa mới,
  lưu bền rồi gửi lệnh 540, xác nhận bằng 564/541 chỉ-đọc và kích hoạt cục bộ trong một giao dịch SQL. Nếu
  chết giữa chừng, lần chạy sau chỉ đọc để xác nhận, không bao giờ gửi 540 lần hai. Sau bước này client
  giữ khóa nhưng vẫn chưa gửi/nhận được tin E2EE. Xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- **Hội thoại E2EE hiện trong danh sách (dạng khóa, chưa đọc được):** sau khi đăng nhập, bản live lấy danh
  sách E2EE của tài khoản (lệnh 563, chỉ đọc) và thêm mỗi người bật E2EE (`threads` và `uids`) vào danh sách
  hội thoại với dòng "[Cuộc trò chuyện mã hóa đầu cuối]", lấy tên từ danh bạ nếu có. Hội thoại đã có tin
  được giữ nguyên; khi có tin thật thì xem trước thật thay thế. Tin trong các hội thoại này vẫn chưa đọc/gửi
  được (xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md)).

- **Chạy thử mã hóa gửi tin E2EE (tùy chọn, không gửi gì):** đặt `ZALO_E2EE_SEND_PROBE=/đường/dẫn/tuyệt/đối.txt`
  khi chạy `run.sh --live`; đối tác tự chọn từ danh sách E2EE (người đầu tiên, hoặc thêm `ZALO_E2EE_PEER="tên"` để
  chọn theo tên như hiển thị trong ứng dụng, không cần UID). Lấy bundle thật của đối tác và các thiết bị khác của
  bạn (chỉ đọc), cho engine mã hóa thử một đoạn văn cho mọi thiết bị rồi bỏ kết quả, không gửi, không lưu. Xem
  [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- **Khởi tạo phiên E2EE với các thiết bị khác của tài khoản (tùy chọn, mặc định tắt, chưa chạy thật):** theo bước
  `INIT_SUBDEVICE` của bản gốc. Đặt `ZALO_E2EE_INIT_SUBDEVICES=/đường/dẫn/tuyệt/đối.txt` khi chạy
  `run.sh --live`: với mỗi thiết bị còn lại (mobile, web) lấy bundle (541 `isReInit`), tạo phiên bằng engine, lưu
  bền rồi gửi tin khởi tạo bằng lệnh 543. Thiết bị đã có phiên thì bỏ qua; không bao giờ gửi lại khi kết quả không
  rõ. Xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- **Công cụ ghi lại bản Electron gốc:** `reverse-engineering/tools/electron_cdp_capture.mjs` ghi log console và
  mọi khung websocket của Electron qua DevTools protocol; `cargo run --features live --example decode_ws_capture`
  giải mã chúng (chỉ in hình dạng, mã người dùng được ẩn danh). Kết quả đối chiếu nằm trong
  [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).
- Thay khóa E2EE nay gửi `delSS: 1` như bản gốc, và nếu khóa PC trên máy chủ đã bị client khác thay thì khóa cục
  bộ được coi là cũ và thay lại (epoch mới) thay vì tin nó. Lấy bundle của người khác nay hỏi lệnh 560 rồi 541
  từng thiết bị với `isReInit`, đúng như bản gốc.

- **Đường nhận tin E2EE (ngoại tuyến, chưa nối vào socket thật):** `plan_receive` → `prepare_receive` → commit
  trong một giao dịch SQL (ratchet, khóa dùng một lần, con trỏ hàng đợi, tin nhắn). Có test gửi bằng đường gửi
  thật ở một kho rồi nhận bằng đường nhận ở kho khác. Xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- **Nhận tin E2EE trực tiếp, thụ động (tùy chọn, mặc định tắt, chưa chạy thật):** đặt
  `ZALO_E2EE_RECEIVE=/đường/dẫn/tuyệt/đối.txt` khi chạy `run.sh --live`. Tin 551 từ người khác được giải mã bằng
  engine, lưu bền trong một giao dịch rồi hiện như tin thường. **Không gửi bất kỳ xác nhận nào**; tin gửi lại được
  nhận ra và bỏ qua. Xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- **Tin nhắn bắt tay E2EE để tự khởi tạo cuộc trò chuyện (tùy chọn, mặc định tắt, chưa chạy thật):** đặt
  `ZALO_E2EE_INIT_PEER=/đường/dẫn/tuyệt/đối.txt` (và tùy ý `ZALO_E2EE_PEER="tên"`) khi chạy `run.sh --live`: hỏi 560
  và 541 từng thiết bị với `isReInit`, tạo phiên bằng engine, lưu bền rồi gửi **một** lệnh 543 `type 0` có một mục
  cho mỗi thiết bị của người đó, đúng như bản gốc. Thiết bị đã có phiên thì bỏ qua; không bao giờ gửi lại khi kết
  quả không rõ. Xem [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

- Chọn người cho các lệnh E2EE thử/bắt tay: tên khớp với cả tên trong danh bạ lẫn tên hiển thị trong danh sách hội
  thoại; khi không khớp, báo cáo (tệp riêng 0600) liệt kê các liên hệ E2EE đánh số kèm tên để chọn lại bằng tên
  hoặc bằng `ZALO_E2EE_PEER="#số"`.

- **Gửi một tin văn bản E2EE thật (tùy chọn, mặc định tắt, chưa chạy thật; tin thật sự đi tới người nhận):** đặt
  `ZALO_E2EE_SEND=/đường/dẫn.txt` và `ZALO_E2EE_SEND_TEXT="…"` (cùng `ZALO_E2EE_PEER`) khi chạy `run.sh --live`.
  Mã hóa cho các thiết bị của người đó và các thiết bị khác của bạn, lưu bền, rồi POST có ký tới
  `/api/e2ee/pc/message/sms`. Không bao giờ gửi lại khi phản hồi không rõ. Xem
  [E2EE_LIVE_PLAN.md](E2EE_LIVE_PLAN.md).

### Sửa
- **Gửi tin 1-1 báo "Chưa xác minh được trạng thái mã hóa do định dạng phản hồi máy chủ chưa được hỗ
  trợ":** phản hồi kiểm tra mã hóa (lệnh 563) của máy chủ bỏ trường `err` khi nó bằng 0, còn client đòi
  `err` phải có và bằng 0. Bản gốc chỉ từ chối khi `err` khác rỗng/0; client nay làm như vậy (thiếu,
  null, false hoặc 0 là thành công, mọi giá trị khác vẫn chặn gửi). Các kiểm tra tương quan `reqId`, các
  mảng danh sách và đối tác đã bật mã hóa giữ nguyên. Nguyên nhân suy ra từ nhật ký chẩn đoán
  (`capability_field=status`, `field_type=absent`, lệnh 563) và từ mã gốc; chưa thử với máy chủ thật nên
  nếu còn lỗi, nhật ký sẽ cho biết trường khác.
- Sticker động từng có nguy cơ làm ứng dụng thoát vì vượt giới hạn kích thước texture của egui; nay mọi
  animation được xếp thành atlas ≤ 2048 px mỗi cạnh (xuống hàng khi cần) và kiểm tra thêm giới hạn thật
  của driver đồ họa.
- Giờ gửi và trạng thái "Đã lưu trên máy" trong bong bóng nền xanh bị mờ ở chế độ Sáng (tương phản 3,4:1);
  nay dùng chữ trắng đầy đủ (5,1:1).
- Cài đặt màu tùy chỉnh hiển thị đúng giá trị mặc định của bảng màu mới thay vì giá trị gốc.

### Kiểm thử
- Thêm kiểm thử độ tương phản WCAG AA cho các cặp chữ/nền chính ở cả hai chế độ, kiểm thử giá trị token,
  kiểm thử hình học (rail, header, composer, nút gửi) và kiểm thử phím `Enter`/`Ctrl+Enter`/`Shift+Enter`
  kèm các rào IME.

### Giới hạn đã biết
- Sticker sprite phát với tốc độ cố định 100 ms/khung vì `duration` thật nằm ở API chi tiết sticker
  cần xác thực; WebP/GIF động giữ nhịp riêng. Sticker dạng gói zip (hiệu ứng) chưa hỗ trợ.
- Danh sách ảnh trong thông tin hội thoại chưa có ảnh thu nhỏ; hàng mở ảnh gốc trong trình duyệt khi bấm.
- Chip **Chưa đọc** hiện bị tắt và chưa có huy hiệu số tin chưa đọc, vì dữ liệu hội thoại chưa lưu trạng thái này.
- Bong bóng gửi dạng ảnh, nhãn dán và thẻ giữ nền sáng để chữ nhãn bên trong vẫn đủ tương phản.
- Cửa sổ thấp (dưới 600 px) dùng nút đăng nhập cao 36 px thay vì 44 px.
- Chưa kiểm với tài khoản thật và chưa kiểm bộ gõ (IME) trên Wayland native; mọi bằng chứng trong bản này là dữ liệu
  giả lập. Gói `.deb`/`.rpm` được kiểm cài và gỡ trong container, chưa kiểm trên máy thật.
- Tối thiểu glibc 2.31, chỉ x86_64. Đã kiểm cài/gỡ trên Ubuntu 20.04/22.04/24.04, Debian 12, Fedora 42 và AlmaLinux 9; Debian 11 chưa kiểm được vì kho apt của bản đã hết hạn trả 404.

## [0.1.0]

Bản thử nghiệm ban đầu: giao diện `egui` theo bộ màu Zalo gốc, đăng nhập QR, lưu SQLite, hội thoại ẩn, ảnh xem trước.
Lịch sử chi tiết ở [MIGRATION_STATUS.md](MIGRATION_STATUS.md).
