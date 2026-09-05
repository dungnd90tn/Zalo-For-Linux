# v1.3.3

Sửa lỗi Zalo **tự khởi động lại rồi chết ngay** — hay gặp nhất ở lần mở đầu tiên sau khi restart máy, khi app có việc phải relaunch (cập nhật, reset DB, phục hồi backup, renderer crash).

## Bằng chứng

Có crash log: `/var/crash/_opt_zalo_electron_electron.1000.crash`, `SIGTRAP`, và trong journal:

```
apparmor="DENIED" operation="capable" profile="unprivileged_userns" capname="sys_admin"
traps: electron[96883] trap int3
```

Tiến trình chết **không phải** instance đầu — mà là bản do `app.relaunch()` sinh ra 4 giây sau. Tái hiện được 100% bằng một app Electron tối giản chạy từ `systemd --user`; thông báo FATAL chỉ bắt được khi ép `--log-file`, vì stderr của bản relaunch bị trỏ `/dev/null`:

```
FATAL:content/browser/zygote_host/zygote_host_impl_linux.cc:207] Check failed: . : Invalid argument (22)
```

## Nguyên nhân

Trên Linux, Electron relaunch qua một helper được fork từ zygote nên kế thừa **`no_new_privs`**. Với cờ đó, `chrome-sandbox` setuid không nâng quyền được; Chromium rơi về user-namespace sandbox; Ubuntu 24.04+ chặn (`apparmor_restrict_unprivileged_userns=1`) → không còn sandbox nào → chết. Bản đầu tiên thì bình thường vì không có `no_new_privs`.

Chạy từ terminal VS Code không tái hiện được — profile AppArmor `vscode` cấp `userns` — nên phải kiểm thử từ `systemd-run --user`.

## Sửa

`bootstrap.js` thay `app.relaunch` trên Linux: spawn bản mới **từ browser process** (chưa có `no_new_privs`) qua một relauncher `/bin/sh` đợi pid hiện tại thoát (giữ single-instance lock) rồi `exec` Electron. Giữ nguyên switch của launcher, chèn lại thư mục app khi upstream truyền `args` (upstream giả định app đã đóng gói), và ghi stdout/stderr của bản mới vào `~/.config/ZaloData/relaunch.log` để lần sau còn dấu vết.

Kiểm chứng trên app thật từ ngữ cảnh desktop: ép renderer crash qua CDP để đi đúng đường relaunch của upstream → bản mới lên với `NoNewPrivs=0`, không trap, không crash report.
