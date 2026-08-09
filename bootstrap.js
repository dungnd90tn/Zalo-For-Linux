// The app serves media through a custom zfile: scheme but never declares it, so Chromium
// treats it as non-standard with an opaque origin: no fetch support, no CORS, and nothing CSP
// can grant it. Declaring it gives the scheme a real origin. Must run before app ready.
try {
    const { protocol } = require('electron');
    protocol.registerSchemesAsPrivileged([
        { scheme: 'zfile', privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true } },
    ]);
} catch (_) {}

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