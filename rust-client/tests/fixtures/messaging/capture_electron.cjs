// Run with ELECTRON_RUN_AS_NODE unset, DISPLAY provided, and the pinned Electron:
// env -u ELECTRON_RUN_AS_NODE /opt/zalo/electron/electron /absolute/path/to/this.cjs
// Disposable local-only renderer; no Zalo application or account is loaded.
const { app, BrowserWindow, session } = require('electron');
const fs = require('fs');
const os = require('os');
const path = require('path');
const https = require('https');
const crypto = require('crypto');
const { execFileSync } = require('child_process');
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'zalo-local-header-'));
for (const [key, directory] of Object.entries({userData:'profile', sessionData:'sessions', crashDumps:'crashes'})) {
    app.setPath(key, path.join(scratch, directory));
}
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('disable-background-networking');
const certPath = path.join(scratch, 'localhost.pem');
const keyPath = path.join(scratch, 'localhost.key');
execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
    '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=IP:127.0.0.1',
    '-keyout', keyPath, '-out', certPath], {stdio:'ignore'});
const certificate = fs.readFileSync(certPath, 'utf8');
const pin = new crypto.X509Certificate(certificate).fingerprint256;
let timer = setTimeout(() => app.exit(2), 15000);
app.whenReady().then(async () => {
    const server = https.createServer({key:fs.readFileSync(keyPath), cert:certificate});
    server.on('upgrade', (request, socket) => {
        const headers = request.headers;
        console.log('LOCAL_CAPTURE ' + JSON.stringify({
            electron:process.versions.electron, chromium:process.versions.chrome,
            transport:'wss', observer:true, method:request.method,
            httpVersion:request.httpVersion, path:request.url, origin:headers.origin,
            userAgent:headers['user-agent'], extensions:headers['sec-websocket-extensions'],
            protocol:headers['sec-websocket-protocol'] || null, version:headers['sec-websocket-version']
        }));
        socket.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
        clearTimeout(timer);
        setTimeout(() => { server.close(); app.exit(0); }, 50);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const isolated = session.fromPartition('local-header-fixture');
    isolated.webRequest.onBeforeRequest((details, callback) => {
        const url = new URL(details.url);
        callback({cancel:!['file:', 'data:'].includes(url.protocol) && url.hostname !== '127.0.0.1'});
    });
    // The original app installs onSendHeaders at main-dist/main.js:76534.
    isolated.webRequest.onSendHeaders(() => {});
    // Accept only this generated fixture certificate at this loopback hostname.
    isolated.setCertificateVerifyProc((request, callback) => {
        const matches = request.hostname === '127.0.0.1' &&
            new crypto.X509Certificate(request.certificate.data).fingerprint256 === pin;
        callback(matches ? 0 : -2);
    });
    const html = path.join(scratch, 'fixture.html');
    fs.writeFileSync(html, '<!doctype html><meta charset="utf-8"><script>new WebSocket(' +
        JSON.stringify('wss://127.0.0.1:' + server.address().port + '/socket?zpw_ver=681&zpw_type=24&t=1700000000000') + ');</script>');
    const window = new BrowserWindow({show:false, webPreferences:{partition:'local-header-fixture',
        webSecurity:true, allowRunningInsecureContent:false, contextIsolation:true,
        nodeIntegration:false, sandbox:false, backgroundThrottling:false}});
    window.webContents.setUserAgent('ZaloPC-linux-24v681');
    await window.loadFile(html);
});
app.on('will-quit', () => { fs.rmSync(scratch, {recursive:true, force:true}); });
