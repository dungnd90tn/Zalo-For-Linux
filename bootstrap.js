// The app serves media through a custom zfile: scheme but never declares it, so Chromium
// treats it as non-standard with an opaque origin: no fetch support, no CORS, and nothing CSP
// can grant it. Declaring it gives the scheme a real origin. Must run before app ready.
try {
    const { protocol } = require('electron');
    protocol.registerSchemesAsPrivileged([
        { scheme: 'zfile', privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true } },
    ]);
} catch (_) {}

// --- linux relaunch shim ---
// Electron's app.relaunch() on Linux starts the successor through its relauncher helper,
// which is forked via the zygote and therefore runs with no_new_privs set. The relaunched
// instance inherits that, so the setuid chrome-sandbox can no longer elevate; Chromium falls
// back to the user-namespace sandbox, and on Ubuntu 24.04+ (apparmor_restrict_unprivileged_
// userns=1) that is denied. It then dies in ZygoteHostImpl::Init with SIGTRAP before any JS
// runs, and its stderr is /dev/null, so nothing is logged anywhere. Every path that restarts
// the app hit this: post-update relaunch, database reset, backup restore, compact-app switch.
// Spawn the successor from the browser process instead, where no_new_privs is clear, through
// a tiny shell relauncher that waits for this pid to exit (single-instance lock) first.
if (process.platform === 'linux') {
    const { app } = require('electron');
    const nativeRelaunch = app.relaunch.bind(app);
    app.relaunch = (options) => {
        try {
            const fs = require('fs');
            const path = require('path');
            const { spawn } = require('child_process');
            const opts = options || {};
            const execPath = opts.execPath || process.execPath;
            const argv = process.argv.slice(1);
            let args = argv;
            if (Array.isArray(opts.args)) {
                // Upstream assumes a packaged app whose executable *is* the app, so `args`
                // replaces argv wholesale. This port runs `electron <appDir>`, so keep the
                // runtime switches the launcher put before the app directory, keep the app
                // directory, and append the requested args after it.
                const appPath = app.getAppPath();
                const idx = argv.findIndex(a => !a.startsWith('--') && path.resolve(a) === appPath);
                const switches = idx >= 0 ? argv.slice(0, idx) : argv.filter(a => a.startsWith('--'));
                args = switches.concat([appPath], opts.args.filter(a => a !== appPath));
            }
            // Keep the successor's output: it is the only trace left if it fails to start.
            let out = 'ignore';
            try { out = fs.openSync(path.join(app.getPath('userData'), 'relaunch.log'), 'w', 0o600); } catch (_) {}
            const child = spawn('/bin/sh', [
                '-c', 'while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; shift; exec "$@"',
                'zalo-relaunch', String(process.pid), execPath, ...args,
            ], { detached: true, stdio: ['ignore', out, out], cwd: process.cwd(), env: process.env });
            child.unref();
            if (out !== 'ignore') fs.closeSync(out);
        } catch (err) {
            nativeRelaunch(options);
        }
    };
}
// --- end linux relaunch shim ---

const handleEntryCompactApp = () => {
    return require('./main-dist/compact-app');
};

function bootstrap() {
    require('./libs/perf-tracing/runtime');
    perf.record(perf.STARTUP);
    require('./main-dist/migration');
    perf.record(perf.MIGRATION_DONE);

    const isCompactApp = process.argv.some(e => e.startsWith('--launch-compact-app'));

    if (isCompactApp) {
        if (require('electron').app.requestSingleInstanceLock()) {
            return handleEntryCompactApp();
        } else {
            require('./main-dist/second-instance');
        }
    }

    if (require('electron').app.requestSingleInstanceLock()) {
        perf.record(perf.MAIN_SCRIPT);
        require('./main-dist/main');
    } else {
        require('./main-dist/second-instance');
    }
}

bootstrap();