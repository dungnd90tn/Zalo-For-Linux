(this.webpackJsonp = this.webpackJsonp || []).push([
    [14], {
        "3KOX": function(e, t, r) {
            "use strict";
            var n = r("9jPG"),
                i = (r("CDcE"), r("3EqI")),
                a = r("2axg"),
                s = r("rt1H"),
                o = r("yXvs");
            class c {
                constructor(e) {
                    this.pool = void 0, this.lock = void 0, this.poolInitCounter = void 0, this.workerSrc = void 0, this.PING_TIMEOUT_MS = 3e3, this.PING_INTERVAL_MS = 12e4, this.MAX_TASK_QUEUED = 150, this._healthCheckTimer = void 0, this.workerSrc = e, this.poolInitCounter = 0, this.lock = new i.a
                }
                async intitialize() {
                    await this.getPool(), this.startHealthCheckTimer()
                }
                async terminate() {
                    this.stopHealthCheckTimer(), this.pool && await this.pool.terminate(!0), this.pool = void 0, this.poolInitCounter = 0
                }
                createPool() {
                    return this.pool || (this.pool = Object(n.pool)(this.workerSrc, {
                        workerType: "web",
                        maxWorkers: 1,
                        minWorkers: 1,
                        maxQueueSize: this.MAX_TASK_QUEUED
                    }), this.poolInitCounter++), this.pool
                }
                async getPool() {
                    return this.lock.exec("initPool", (async () => this.createPool()))
                }
                async restartPool(e) {
                    return this.lock.exec("initPool", (async () => {
                        this.pool && (this.stopHealthCheckTimer(), await this.pool.terminate(!!e), this.pool = void 0);
                        const t = this.createPool();
                        return this.startHealthCheckTimer(), t
                    }))
                }
                startHealthCheckTimer() {}
                stopHealthCheckTimer() {
                    this._healthCheckTimer && (clearTimeout(this._healthCheckTimer), this._healthCheckTimer = void 0)
                }
                async executeOnce(e, t, r) {
                    try {
                        const n = await this.getPool(),
                            {
                                payload: i,
                                transferList: a
                            } = Object(s.a)(t);
                        let o = n.exec(e.toString(), i, {
                            transfer: a,
                            on: null == r ? void 0 : r.onMessage
                        });
                        if (null != r && r.timeoutMs && r.timeoutMs > 0) {
                            let t;
                            o = o.timeout(r.timeoutMs);
                            const n = new Promise(((e, n) => {
                                t = setTimeout((() => {
                                    o.cancel(), n()
                                }), r.timeoutMs)
                            }));
                            return Promise.race([o, n]).catch((t => {
                                if ("CancellationError" === (null == t ? void 0 : t.name)) throw new Error(`Task "${e.toString()}" has timed out`);
                                throw t
                            })).finally((() => clearTimeout(t)))
                        }
                        return o.then((e => e))
                    } catch (n) {
                        if (Object(a.b)(n)) throw a.a.wrap(n);
                        throw n
                    }
                }
                async execute(e, t, r) {
                    try {
                        return this.stopHealthCheckTimer(), await this.executeOnce(e, t, r)
                    } catch (n) {
                        throw n
                    } finally {
                        this.startHealthCheckTimer()
                    }
                }
            }
            var d, l = r("D5po"),
                u = r("VuQl"),
                h = r("AtyM"),
                p = r("kPOY");
            class b extends c {
                constructor() {
                    super(__SRC_TRUST_WORKER__), this.WORKER_TASK_TIMEOUT = 1e4
                }
                async execute(e, t) {
                    return super.execute(e, t, {
                        timeoutMs: this.WORKER_TASK_TIMEOUT,
                        onMessage: e => {
                            var t, r, n;
                            if ("worker_log" === (null == e ? void 0 : e.type) && null !== (t = e.data) && void 0 !== t && t.fn && null !== (r = e.data) && void 0 !== r && null !== (n = r.args) && void 0 !== n && n.length) {
                                const t = ["[trust-worker]", ...e.data.args];
                                "logError" === e.data.fn ? o.a.TrustProto.zsymb(18, "dvIHG-", ...t) : o.a.TrustProto.zsymb(0, "YnFXmT", ...t)
                            }
                        }
                    })
                }
            }
            Object(l.c)(u.b.TrustProtocol)(d = Reflect.metadata("design:type", Function)(d = Reflect.metadata("design:paramtypes", [])(d = class {
                constructor() {
                    this.worker = void 0, this.worker = new b
                }
                async init() {
                    const e = h.a.now();
                    try {
                        await this.worker.intitialize(), await this.initWasm(), o.a.TrustProto.zsymb(0, "I71nbu", "[trust-protocol] init worker WASM success. cost " + (h.a.now() - e))
                    } catch (t) {
                        p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.INIT_WASM_WORKER_FAIL, h.a.now() - e, Object(o.b)(t))
                    }
                }
                async terminate() {
                    await this.worker.terminate(), o.a.TrustProto.zsymb(0, "8bUrAR", "[trust-protocol] WASM worker have been terminated")
                }
                async initWasm() {
                    try {
                        return await this.worker.execute("initWasm", [])
                    } catch (e) {
                        throw o.a.TrustProto.zsymb(18, "GuVlyi", "[trust-protocol] FAILED on init WASM. ERR: ", Object(o.b)(e)), e
                    }
                }
                async generateIdentityKeyPairAndLinkingSecret() {
                    try {
                        return await this.worker.execute("generateIdentityKeyPairAndLinkingSecret", [])
                    } catch (e) {
                        throw o.a.TrustProto.zsymb(18, "isMELT", "[trust-protocol] FAILED on generate IK & linkingSecretKey. ERR: ", Object(o.b)(e)), e
                    }
                }
                async generateLinkingQR(e, t, r, n, i, a) {
                    const s = h.a.now();
                    try {
                        return await this.worker.execute("generateLinkingQR", [e, t, r, n, i, a])
                    } catch (c) {
                        throw o.a.TrustProto.zsymb(18, "qbfrXc", "[trust-protocol] FAILED on generate linking QR. ERR: ", Object(o.b)(c)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.GENERATE_LINKING_QR_FAIL, h.a.now() - s, Object(o.b)(c)), c
                    }
                }
                async getRelinkId(e, t, r, n) {
                    const i = h.a.now();
                    try {
                        return await this.worker.execute("getRelinkId", [e, t, r, n])
                    } catch (a) {
                        throw o.a.TrustProto.zsymb(18, "I-BRnj", "[trust-protocol] FAILED on get relinkId. ERR: ", Object(o.b)(a)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.GET_RELINK_ID_FAIL, h.a.now() - i, Object(o.b)(a)), a
                    }
                }
                async createCompanionLinkingProof(e, t, r, n, i, a) {
                    const s = h.a.now();
                    try {
                        return await this.worker.execute("createCompanionLinkingProof", [e, t, r, n, i, a])
                    } catch (c) {
                        throw o.a.TrustProto.zsymb(18, "w26GhA", "[trust-protocol] FAILED on create companion linking proof. ERR: ", Object(o.b)(c)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.CREATE_COMP_LINKING_PROOF_FAIL, h.a.now() - s, Object(o.b)(c)), c
                    }
                }
                async verifyDeviceList(e, t, r, n) {
                    const i = h.a.now();
                    try {
                        return await this.worker.execute("verifyDeviceList", [e, t, r, n])
                    } catch (a) {
                        throw o.a.TrustProto.zsymb(18, "NPbHY5", "[trust-protocol] FAILED on verify devicelist. ERR: ", Object(o.b)(a)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.VERIFY_DEVICE_LIST_FAIL, h.a.now() - i, Object(o.b)(a)), a
                    }
                }
                async verifyDeviceListBytes(e, t, r, n) {
                    const i = h.a.now();
                    try {
                        return await this.worker.execute("verifyDeviceListBytes", [e, t, r, n])
                    } catch (a) {
                        throw o.a.TrustProto.zsymb(18, "LWoLan", "[trust-protocol] FAILED on verify devicelist bytes. ERR: ", Object(o.b)(a)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.VERIFY_DEVICE_LIST_BYTES_FAIL, h.a.now() - i, Object(o.b)(a)), a
                    }
                }
                async verifyIdentity(e, t, r) {
                    const n = h.a.now();
                    try {
                        return await this.worker.execute("verifyIdentity", [e, t, r])
                    } catch (i) {
                        throw o.a.TrustProto.zsymb(18, "kPZJgr", "[trust-protocol] FAILED on verify identity. ERR: ", Object(o.b)(i)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.VERIFY_IDENTITY_FAIL, h.a.now() - n, Object(o.b)(i)), i
                    }
                }
                async checkExpiryTime(e, t, r) {
                    const n = h.a.now();
                    try {
                        return await this.worker.execute("checkExpiryTime", [e, t, r])
                    } catch (i) {
                        throw o.a.TrustProto.zsymb(18, "j75yjs", "[trust-protocol] FAILED on check expiry time. ERR: ", Object(o.b)(i)), p.a.LibZproto.recordLibZprotoError(p.a.LibZproto.FAILURE.CHECK_EXPIRY_TIME_FAIL, h.a.now() - n, Object(o.b)(i)), i
                    }
                }
            }) || d) || d)
        },
        "4Vil": function(e, t, r) {
            "use strict";
            var n = r("T0aS"),
                i = r("jDHv");
            class a extends n.b {
                setSession(e) {
                    this.isNewSession(e) && ($zsessionManager.authenticate(e), super.setSession(e))
                }
            }
            i.ModuleContainer.registerSingleton(n.a, a)
        },
        "5lV7": function(e, t, r) {
            "use strict";
            var n, i = r("wH4e"),
                a = r("ggcH"),
                s = r("zmG6"),
                o = r("LW7J"),
                c = r("jDHv"),
                d = r("fc/q"),
                l = r("J25b"),
                u = r("AH6j"),
                h = r("Mgpg");
            var p = function(e) {
                return e.IDLE = "IDLE", e.CHECKING = "CHECKING", e.STOP = "STOP", e
            }(p || {});
            let b = Object(c.injectable)()(n = function(e, t) {
                return Object(c.inject)(h.ZLoggerFactory)(e, void 0, 0)
            }(n = function(e, t) {
                return Object(c.inject)(d.b)(e, void 0, 1)
            }(n = function(e, t) {
                return Object(c.inject)(l.b)(e, void 0, 2)
            }(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [void 0 === h.ZLoggerFactory ? Object : h.ZLoggerFactory, "undefined" == typeof IQosService ? Object : IQosService, "undefined" == typeof ISystemTime ? Object : ISystemTime])(n = class extends u.b {
                constructor(e, t, r) {
                    super(), this.adapterType = i.a.IDB, this.logger = void 0, this.qos = void 0, this.systemTime = void 0, this.state = void 0, this.lastTsStartCheck = 0, this.intervalCheckDBCorruption = null, this.hasCorruptedThisSession = void 0, this.processRequestRestartAppForBackup = (e, t) => {
                        this.submitReportCorruption(o.e, s.c.RESET_COUNTER), this.dispatchEvent(new a.IndexedDBCorruptionDetectedEvent(this.lastTsStartCheck, t))
                    }, this.logger = e.createZLogger("db-corruption-detector", ["renderer", "indexedDB-adapter"]), this.qos = t, this.systemTime = r, this.state = p.IDLE, this.hasCorruptedThisSession = !1
                }
                start() {
                    this.checkStartUp(), this.startIntervalCheck(), $zsub.$zapp.onRequestRestartAppForBackup(this.processRequestRestartAppForBackup)
                }
                stop() {
                    this.stopIntervalCheckDBCorruption(), $zsub.$zapp.removeListenBackupRestartReq(this.processRequestRestartAppForBackup), this.logger.zsymb(0, "_J8rFz", "IndexedDBCorruptionDetector stopped")
                }
                checkStartUp() {
                    this.checkDatabase(s.b.STARTUP)
                }
                startIntervalCheck() {
                    this.logger.zsymb(0, "6--Qyx", "Start interval check database corruption"), this.intervalCheckDBCorruption && clearInterval(this.intervalCheckDBCorruption), this.intervalCheckDBCorruption = setInterval((() => {
                        this.checkDatabase(s.b.INTERVAL)
                    }), 12e5)
                }
                stopIntervalCheckDBCorruption() {
                    this.intervalCheckDBCorruption && (clearInterval(this.intervalCheckDBCorruption), this.intervalCheckDBCorruption = null), this.logger.zsymb(0, "idQy1u", "Stop interval check database corruption")
                }
                corruptionHasOccurred() {
                    return this.hasCorruptedThisSession
                }
                forceCheckDBCorruption() {
                    return this.checkDatabase(s.b.FORCE_CHECK)
                }
                async checkDatabase(e) {
                    if (this.state === p.STOP) return;
                    if (this.state === p.CHECKING) return void this.logger.zsymb(0, "1yJ9XH", `Checking... Entry check: ${e}`);
                    this.state = p.CHECKING, this.logger.zsymb(0, "h0wOKk", `Let start check database. Entry check: ${e}`), this.lastTsStartCheck = this.systemTime.getTimeNow();
                    const t = await $zdb.getIDBDatabaseHealthStatus(),
                        r = this.isCorruption(t);
                    this.state = p.IDLE, this.submitReportCorruption(e, t), r && (this.state = p.STOP, this.logger.zsymb(18, "RpyXJ0", `Corruption detected. Entry check: ${e}`), this.stopIntervalCheckDBCorruption(), this.hasCorruptedThisSession = !0, $zapp.notifyDbFail(), $zapp.notifyDbCorruption())
                }
                submitReportCorruption(e, t) {
                    const r = o.c[e];
                    this.qos.log({
                        success: !1,
                        commandId: r,
                        subCommandId: 0,
                        errorCode: t,
                        duration: 0,
                        startTime: 0
                    }), this.logger.zsymb(18, "Z99z4M", `Submit report database corruption. ${e}::${r}`)
                }
                isCorruption(e) {
                    switch (e) {
                        case s.c.RESET_COUNTER:
                            return !0;
                        case s.c.INVALID_COUNTER:
                        case s.c.OK:
                        default:
                            return !1
                    }
                }
            }) || n) || n) || n) || n) || n) || n;
            c.ModuleContainer.registerSingleton(a.TDBCorruptionDetectorAdapter, b);
            var g, m = r("Ilem"),
                y = (r("Ofqi"), r("aEKn")),
                f = r("1pet");
            let v = Object(c.injectable)()(g = function(e, t) {
                return Object(c.inject)(h.ZLoggerFactory)(e, void 0, 0)
            }(g = function(e, t) {
                return Object(c.inject)(d.b)(e, void 0, 1)
            }(g = function(e, t) {
                return Object(c.inject)(l.b)(e, void 0, 2)
            }(g = Reflect.metadata("design:type", Function)(g = Reflect.metadata("design:paramtypes", [void 0 === h.ZLoggerFactory ? Object : h.ZLoggerFactory, "undefined" == typeof IQosService ? Object : IQosService, "undefined" == typeof ISystemTime ? Object : ISystemTime])(g = class extends u.b {
                constructor(e, t, r) {
                    super(), this.adapterType = i.a.SQLite, this.logger = void 0, this.qos = void 0, this.systemTime = void 0, this.hasCorruptedThisSession = void 0, this.handleCorruptionEvent = e => {
                        this.hasCorruptedThisSession = !0, this.logger.zsymb(18, "7di01A", "Corruption detected, corruption info", JSON.stringify(e.corruptionInfo)), this.dispatchEvent(new a.SQLiteDBCorruptionDetectedEvent(this.systemTime.getTimeNow(), e.corruptionInfo)), this.submitReportCorruption(e.corruptionInfo)
                    }, this.logger = e.createZLogger("db-corruption-detector", ["renderer", "sqlite-adapter"]), this.qos = t, this.systemTime = r, this.hasCorruptedThisSession = !1
                }
                corruptionHasOccurred() {
                    return this.hasCorruptedThisSession
                }
                async forceCheckDBCorruption() {}
                start() {
                    y.a.getInstance().addEventListener("corruption", this.handleCorruptionEvent)
                }
                stop() {
                    y.a.getInstance().removeEventListener("corruption", this.handleCorruptionEvent), this.logger.zsymb(0, "wYq7ea", "SQLiteDBCorruptionDetector stopped")
                }
                submitReportCorruption(e) {
                    const {
                        commandId: t,
                        errorCode: r
                    } = this.getQosInfo(e);
                    this.qos.log({
                        success: !1,
                        commandId: t,
                        subCommandId: 0,
                        errorCode: r,
                        duration: 0,
                        startTime: 0
                    }), this.logger.zsymb(18, "nQ-IPQ", "Submit report database corruption")
                }
                getQosInfo(e) {
                    var t, r;
                    if ("Message" === e.databaseCorruptInfo.table) {
                        const t = (() => {
                            const [t] = e.databaseCorrupt.split("/").slice(-1);
                            return t.startsWith(f.GROUPID_PREFIX)
                        })();
                        return {
                            commandId: o.d.CORRUPTION_TABLE_MESSAGE,
                            errorCode: t ? o.b.Group : o.b.OneOne
                        }
                    }
                    return e.type === m.a.FULL ? {
                        commandId: o.d.CORRUPTION_IMPACT_FULL,
                        errorCode: null !== (r = o.a[e.databaseCorruptInfo.database]) && void 0 !== r ? r : -1
                    } : {
                        commandId: o.d.CORRUPTION_IMPACT_PARTIAL,
                        errorCode: null !== (t = o.a[e.databaseCorruptInfo.database]) && void 0 !== t ? t : -1
                    }
                }
            }) || g) || g) || g) || g) || g) || g;
            c.ModuleContainer.registerSingleton(a.TDBCorruptionDetectorAdapter, v)
        },
        "9AEM": function(e, t, r) {
            "use strict";
            var n, i = r("ggcH"),
                a = r("wH4e"),
                s = r("pAGP"),
                o = r("zmG6"),
                c = r("jDHv"),
                d = r("AH6j"),
                l = r("Mgpg"),
                u = r("T0aS"),
                h = r("Jz/+");
            let p = Object(c.injectable)()(n = function(e, t) {
                return Object(c.inject)(l.ZLoggerFactory)(e, void 0, 0)
            }(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [void 0 === l.ZLoggerFactory ? Object : l.ZLoggerFactory])(n = class extends d.b {
                constructor(e) {
                    super(), this.logger = void 0, this.corruptionDetectorAdapters = void 0, this.handleCorruptionEvent = e => {
                        this.logger.zsymb(0, "WwI2aD", "Corrupt occurs"), this.dispatchEvent(e)
                    }, this.logger = e.createZLogger("db-corruption-detector", []), this.corruptionDetectorAdapters = new Map, this.startDetectForSharedDB();
                    c.ModuleContainer.resolve(u.a).addEventListener(h.a.SessionChange, (async ({
                        session: e
                    }) => {
                        null !== e && this.startDetectForUserScopedDB(e.userId)
                    }))
                }
                getAdapter(e) {
                    return c.ModuleContainer.resolveAll(i.TDBCorruptionDetectorAdapter).find((t => t.adapterType === e))
                }
                async startDetect(e) {
                    if (!c.ModuleContainer.isRegistered(i.TDBCorruptionDetectorAdapter)) return void this.logger.zsymb(0, "6dDqdS", "Skip track corrupt, no available adapters.");
                    const t = this.getAdapter(e);
                    t && !this.corruptionDetectorAdapters.has(e) && (this.corruptionDetectorAdapters.set(e, t), t.addEventListener(o.a.DB_CORRUPTION_DETECTED, this.handleCorruptionEvent), t.start())
                }
                async startDetectForSharedDB() {
                    const e = await c.ModuleContainer.resolve(s.a).getAdapterTypeOfSharedDatabases();
                    this.startDetect(e)
                }
                async startDetectForUserScopedDB(e) {
                    const t = await c.ModuleContainer.resolve(s.a).getAdapterTypeOfUserScopedDatabases(e);
                    this.startDetect(t)
                }
                async forceCheckDBCorruption() {
                    this.logger.zsymb(0, "d5BJH_", "forceCheckDBCorruption"), this.corruptionDetectorAdapters.forEach((e => e.forceCheckDBCorruption()))
                }
                async forceCheckDBCorruptionFor(e) {
                    switch (this.logger.zsymb(0, "pP_SGG", "forceCheckDBCorruptionFor", e), e) {
                        case a.a.IDB: {
                            const t = this.getAdapter(e);
                            if (!t) {
                                this.logger.zsymb(0, "vN7q-V", "Adapter not found", e);
                                break
                            }
                            await t.forceCheckDBCorruption(), t.corruptionHasOccurred() && this.handleCorruptionEvent(new i.IndexedDBCorruptionDetectedEvent(Date.now(), !1));
                            break
                        }
                        default:
                            throw new Error("Not support" + e)
                    }
                }
                corruptionHasOccurred() {
                    let e = !1;
                    return this.corruptionDetectorAdapters.forEach((t => {
                        e = e || t.corruptionHasOccurred()
                    })), e
                }
            }) || n) || n) || n) || n;
            c.ModuleContainer.registerSingleton(i.TDBCorruptionDetector, p), c.ModuleContainer.resolve(i.TDBCorruptionDetector)
        },
        AW0x: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            }));
            class n {
                constructor() {
                    this._waiting = void 0, this._waiting = new Map
                }
                async exec(e, t) {
                    if (this._waiting.has(e)) {
                        const t = this._waiting.get(e);
                        return await t
                    }
                    if (!t) throw new Error("missing promise to invoke");
                    const r = t();
                    this._waiting.set(e, r);
                    try {
                        return await r
                    } catch (n) {
                        throw n
                    } finally {
                        this._waiting.delete(e)
                    }
                }
                isWaiting(e) {
                    return this._waiting.has(e)
                }
            }
        },
        B3nq: function(e, t, r) {
            "use strict";
            var n = r("jDHv"),
                i = r("8/YW"),
                a = (r("jw3m"), r("Y58e")),
                s = r("UGJm");
            let o = function(e) {
                return e.Idle = "idle", e.Active = "active", e
            }({});
            var c = r("AH6j");
            let d = function(e) {
                return e[e.idle = 0] = "idle", e[e.active = 1] = "active", e
            }({});
            class l extends c.b {
                constructor(e) {
                    super(), this.status = d.active, this.window = void 0, this.idleDelay = void 0, this.minimumIdlePeriod = void 0, this.lastIdleTime = void 0, this.focusStateChangeDebounceTimer = void 0, this.handleDocumentBlur = () => {
                        this.resetFocusStateChangeDebounceTimer(), this.focusStateChangeDebounceTimer = setTimeout((() => {
                            this.setStateToIdle()
                        }), this.idleDelay)
                    }, this.handleDocumentFocus = () => {
                        this.resetFocusStateChangeDebounceTimer(), this.setStateToActive()
                    }, this.idleDelay = e.idleDelay, this.minimumIdlePeriod = e.minimumIdlePeriod, this.window = e.window || window, this.lastIdleTime = 0, this.focusStateChangeDebounceTimer = null
                }
                start() {
                    this.window.addEventListener("blur", this.handleDocumentBlur), this.window.addEventListener("focus", this.handleDocumentFocus)
                }
                stop() {
                    this.window.removeEventListener("blur", this.handleDocumentBlur), this.window.removeEventListener("focus", this.handleDocumentFocus)
                }
                onIdle(e) {
                    return this.addEventListener(o.Idle, e)
                }
                setStateToActive() {
                    this.status !== d.idle || (this.status = d.active, this.dispatchEvent(new Event(o.Active)))
                }
                setStateToIdle() {
                    if (this.status !== d.active) return;
                    this.isThrottlingIdleState() || (this.lastIdleTime = Date.now(), this.status = d.idle, this.dispatchEvent(new Event(o.Idle)))
                }
                isThrottlingIdleState() {
                    return Date.now() - this.lastIdleTime < this.minimumIdlePeriod
                }
                resetFocusStateChangeDebounceTimer() {
                    this.focusStateChangeDebounceTimer && (clearTimeout(this.focusStateChangeDebounceTimer), this.focusStateChangeDebounceTimer = null)
                }
            }
            var u = r("wx14"),
                h = r("q1tI"),
                p = r.n(h),
                b = r("/MKj"),
                g = r("FK2X"),
                m = r("HuPi"),
                y = r("emRR"),
                f = r("xrk1"),
                v = r("ZBGy"),
                O = r("T1Xd"),
                E = r("uzdi");
            const C = Object(E.a)();

            function I() {
                const e = C.useRecoilSnapshot();
                return C.setSnapShot(e), null
            }
            var w = r("QVmZ"),
                T = r("72hn"),
                D = r("ZlRg"),
                S = r("VaDh"),
                j = r("CzFt"),
                _ = r("h0sc"),
                A = r("1pet"),
                L = r("L+5E"),
                k = r("UiPd"),
                R = r("Yi2m"),
                M = r("6uTC"),
                F = r("c51z"),
                N = r("sqm0"),
                z = r("NTw/"),
                B = r("XS0u"),
                P = r("7NAg"),
                K = r("7TIh");
            var U = r("hI9i"),
                V = r("paDR");
            var x = r("q9t1"),
                H = r("zgbJ"),
                $ = r("Jq9Z"),
                G = r("KwLe"),
                W = r("nQja");
            const Q = p.a.lazy((() => r.e(46).then(r.bind(null, "TZgy")))),
                Z = () => p.a.createElement(p.a.Suspense, {
                    fallback: null
                }, p.a.createElement(Q, null));
            var q = r("Jq/t"),
                J = r("ro+J");
            const Y = e => {
                const t = Object(b.f)(),
                    r = Object(h.useMemo)((() => Object(v.d)(t)), [t]),
                    n = Object(v.c)(),
                    i = Object(b.g)((e => ({
                        user: e.user,
                        popup: e.popup,
                        status: e.status,
                        profile: e.profile,
                        chatview: e.chatview
                    })), ((e, t) => e.user == t.user && e.popup == t.popup && e.status == t.status && e.profile == t.profile && e.chatview == t.chatview));
                var a;
                return (e => {
                        const t = Object(h.useCallback)((() => {
                                const e = B.default.getFullDiskChatPopupNoti();
                                let t = !0,
                                    r = !0;
                                [P.g.FULL, P.g.ALMOST_FULL_2].includes(null == e ? void 0 : e.level) && (t = !1, (null == e ? void 0 : e.level) === P.g.FULL && (r = {
                                    data: {
                                        tab: K.h.RES_MANAGEMENT
                                    }
                                })), _.ModalManagerV2.openModal({
                                    windowId: "1",
                                    name: A.ModalIdentitiesDefine.SETTINGS,
                                    params: r,
                                    forceCloseAll: t
                                })
                            }), []),
                            r = Object(h.useCallback)((() => {
                                _.ModalManagerV2.openModal({
                                    windowId: "1",
                                    name: A.ModalIdentitiesDefine.APP_INFO,
                                    params: !0
                                })
                            }), []),
                            n = Object(h.useCallback)(((e, t) => {
                                var r;
                                N.a.setFlagCopyFromCapture(!!t && !!t.isCopyFromCap), N.a.setTimeClipboardChanged(performance.now()), null === (r = Object(z.b)()) || void 0 === r || r.setTimeClipboardChanged(performance.now())
                            }), []),
                            i = (t, r) => {
                                const n = k.default.getFriendsSync();
                                L.a.doActionWithUri(r, e, n)
                            },
                            a = (t, {
                                term: r,
                                data: n
                            }) => {
                                switch (r) {
                                    case "id":
                                        L.a.autoOpenConversationById(n, e), n.startsWith(A.GROUPID_PREFIX) ? R.default.logAction(2220801) : n.startsWith(A.OA_ID_PREFIX) ? R.default.logAction(2220803) : R.default.logAction(2220802);
                                        break;
                                    case "phone":
                                        R.default.logAction(2220804), L.a.autoOpenConversationByPhone(n, e);
                                        break;
                                    case "username":
                                        R.default.logAction(2220805), L.a.autoOpenConversationByUsername(n, e);
                                        break;
                                    default:
                                        M.default.createError(F.a.str("STR_GROUP_NO_EXIST"))
                                }
                            };
                        Object(h.useEffect)((() => ($zsub.$zuri.onNewRequest(i), $zsub.$zuri.onRequestOpenConv(a), () => {
                            $zsub.$zuri.removeListenNewRequest(i), $zsub.$zuri.removeListenRequestOpenConv(a)
                        })), [e]), Object(h.useEffect)((() => {
                            $zsub.$zapp.onRequestShowPreference(t), $zsub.$zapp.onRequestShowAbout(r), $zsub.$zresource.onClipboardChange(n)
                        }), [])
                    })(r), a = r, Object(V.c)(V.a.Dispatch, ((e, t, r) => {
                        a(r)
                    })),
                    function(e) {
                        Object(V.c)(V.a.Emit, ((t, r, n) => {
                            e(n)
                        }))
                    }(n), p.a.createElement(g.c, Object(u.a)({
                        emitter: n,
                        dispatch: r
                    }, i, {
                        shouldShowMigrateScreen: e.shouldShowMigrateScreen,
                        shouldKeepLoadingScreenDueToStartUpPreload: e.shouldKeepLoadingScreenDueToStartUpPreload
                    }))
            };

            function X(e) {
                const t = Object(U.l)(x.b, x.c, (e => {
                        const {
                            status: t
                        } = e;
                        return $.b.includes(t)
                    })),
                    r = Object(U.l)(H.c, H.b, (e => e.shouldKeepLoadingScreen));
                return p.a.createElement(Y, Object(u.a)({}, e, {
                    shouldShowMigrateScreen: t,
                    shouldKeepLoadingScreenDueToStartUpPreload: r
                }))
            }
            const ee = e => {
                    const {
                        appVersionState: t
                    } = Object(G.a)(), {
                        maintenanceMode: r
                    } = Object(q.d)({});
                    if (t === W.a.NONE) return null;
                    const n = t === W.a.VERIFIED;
                    return null === r ? p.a.createElement(q.a, null) : r ? p.a.createElement(q.b, null) : n ? p.a.createElement(X, e) : p.a.createElement(Z, null)
                },
                te = ((...e) => ({
                    children: t
                }) => e.reduceRight(((e, [t, r = {}]) => p.a.createElement(t, r, e)), t))([b.a, {
                    store: y.default
                }], [b.a, {
                    store: j.a,
                    context: j.b
                }], [b.a, {
                    store: w.a,
                    context: T.a
                }], [b.a, {
                    store: D.a,
                    context: S.a
                }], [b.a, {
                    store: U.b,
                    context: U.a
                }], [v.b], [f.c], [O.a]),
                re = () => p.a.createElement(te, null, p.a.createElement(m.a, {
                    _window: window
                }), p.a.createElement(I, null), p.a.createElement(J.a, null, p.a.createElement(ee, null)));
            var ne = r("Mgpg"),
                ie = r("USLQ"),
                ae = r("XPd9"),
                se = r("RoG4"),
                oe = r("RYIJ"),
                ce = r("+pQO"),
                de = r("TmDO"),
                le = r("vtBH"),
                ue = r("iwo6");
            const he = ["category", "label", "value"],
                pe = "zinstant-binded-content",
                be = "zinstant-load-failed",
                ge = "zinstant-reload",
                me = "loading",
                ye = "connected";
            var fe, ve = Object(ae.a)("triggerRetry"),
                Oe = Object(ae.a)("retry"),
                Ee = Object(ae.a)("loaded"),
                Ce = Object(ae.a)("failed"),
                Ie = Object(ae.a)("lang"),
                we = Object(ae.a)("debound"),
                Te = Object(ae.a)("zkey"),
                De = Object(ae.a)("data"),
                Se = Object(ae.a)("msgId"),
                je = Object(ae.a)("userId"),
                _e = Object(ae.a)("extraData"),
                Ae = Object(ae.a)("onContextMenu"),
                Le = Object(ae.a)("onClose"),
                ke = Object(ae.a)("windowId"),
                Re = Object(ae.a)("zinstantEle"),
                Me = Object(ae.a)("theme");
            class Fe extends HTMLElement {
                get zinstantPlugin() {
                    return n.ModuleContainer.resolve(se.d).get(se.e)
                }
                constructor() {
                    super(), Object.defineProperty(this, ve, {
                        writable: !0,
                        value: !1
                    }), Object.defineProperty(this, Oe, {
                        writable: !0,
                        value: 0
                    }), Object.defineProperty(this, Ee, {
                        writable: !0,
                        value: !1
                    }), Object.defineProperty(this, Ce, {
                        writable: !0,
                        value: !1
                    }), Object.defineProperty(this, Ie, {
                        writable: !0,
                        value: F.a.getCurrentLanguageName()
                    }), Object.defineProperty(this, we, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, Te, {
                        writable: !0,
                        value: ""
                    }), Object.defineProperty(this, De, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, Se, {
                        writable: !0,
                        value: ""
                    }), Object.defineProperty(this, je, {
                        writable: !0,
                        value: ""
                    }), Object.defineProperty(this, _e, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, Ae, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, Le, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, ke, {
                        writable: !0,
                        value: ""
                    }), Object.defineProperty(this, Re, {
                        writable: !0,
                        value: null
                    }), Object.defineProperty(this, Me, {
                        writable: !0,
                        value: ""
                    }), this.onContextMenu && (this.oncontextmenu = this.onContextMenu), Object(ie.a)(this, Me)[Me] = ue.a.getTheme()
                }
                connectedCallback() {
                    var e, t, r;
                    this.render(me), Object(ie.a)(this, De)[De] = this.getAttribute("data") ? JSON.parse(this.getAttribute("data")) : null, Object(ie.a)(this, Se)[Se] = null !== (e = this.getAttribute("msgId")) && void 0 !== e ? e : "", Object(ie.a)(this, je)[je] = null !== (t = this.getAttribute("userId")) && void 0 !== t ? t : "", Object(ie.a)(this, _e)[_e] = this.getAttribute("extraData") ? JSON.parse(this.getAttribute("extraData")) : null, Object(ie.a)(this, ke)[ke] = null !== (r = this.getAttribute("windowId")) && void 0 !== r ? r : "1", F.a.callUpdate(this.updateLang.bind(this)), this.render(ye), le.b.on(de.THEME_LISTENER_EVENTS.SETTING_UPDATED, (e => {
                        Object(ie.a)(this, Me)[Me] = e.theme, Object(ie.a)(this, Re)[Re].setAttribute("data-theme", Object(ie.a)(this, Me)[Me])
                    }))
                }
                disconnectedCallback() {
                    Object(ie.a)(this, Le)[Le] && Object(ie.a)(this, Le)[Le](), F.a.removeUpdate(this.updateLang.bind(this)), this.zinstantPlugin.removeUICallback && this.zinstantPlugin.removeUICallback(Object(ie.a)(this, Te)[Te])
                }
                static get observedAttributes() {
                    return ["data", "msgId", "userId", "extraData", "windowId"]
                }
                attributeChangedCallback(e, t, r) {
                    if (t !== r) switch (e) {
                        case "data":
                            JSON.stringify(Object(ie.a)(this, De)[De]) !== r && (Object(ie.a)(this, De)[De] = r ? JSON.parse(r) : null);
                            break;
                        case "userId":
                            Object(ie.a)(this, je)[je] !== r && (Object(ie.a)(this, je)[je] = r);
                            break;
                        case "msgId":
                            Object(ie.a)(this, Se)[Se] !== r && (Object(ie.a)(this, Se)[Se] = r);
                            break;
                        case "windowId":
                            Object(ie.a)(this, ke)[ke] !== r && (Object(ie.a)(this, ke)[ke] = r);
                            break;
                        case "extraData":
                            Object(ie.a)(this, _e)[_e] = r ? JSON.parse(r) : null
                    }
                }
                callbackFromZInstant(e) {
                    var t, r;
                    let {
                        type: n,
                        payload: i
                    } = e || {};
                    switch (n) {
                        case "bindContent":
                            if (Object(ie.a)(this, Ee)[Ee] = !0, Object(ie.a)(this, Ce)[Ce] = !1, Object(ie.a)(this, _e)[_e]) {
                                let e = ce.a.setMessageInfo;
                                "function" == typeof e && e(Object(ie.a)(this, Se)[Se], Object(ie.a)(this, _e)[_e])
                            } else {
                                let e = ce.a.removeMessageInfo;
                                "function" == typeof e && e(Object(ie.a)(this, Se)[Se])
                            }
                            this.render(pe);
                            break;
                        case "downloadUrlFail":
                            this.setStatusFail();
                            break;
                        case "impressionTrigger":
                            var a, s, o;
                            if (Object.keys(i).some((e => he.includes(e) && null !== i[e])) && ((null === (t = Object(ie.a)(this, De)[De]) || void 0 === t ? void 0 : t.pcItem) || (null === (r = Object(ie.a)(this, De)[De]) || void 0 === r ? void 0 : r.bubbleItem)).zinstantdata_id) R.default.logActionInfoV2(R.FeaturesInfo.ZInstant, se.a.TYPE_TRACKING.IMPRESSION, {
                                [se.a.TRACKING_CMD.ID]: null === (a = (null === (s = Object(ie.a)(this, De)[De]) || void 0 === s ? void 0 : s.pcItem) || (null === (o = Object(ie.a)(this, De)[De]) || void 0 === o ? void 0 : o.bubbleItem)) || void 0 === a ? void 0 : a.zinstantdata_id,
                                [se.a.TRACKING_CMD.WIDGET_ID]: i.widget_id,
                                [se.a.TRACKING_CMD.CATEGORY]: i.category,
                                [se.a.TRACKING_CMD.LABEL]: i.label,
                                [se.a.TRACKING_CMD.VALUE]: i.value
                            });
                            break;
                        case "disconnectedCallback":
                            Object(ie.a)(this, Re)[Re].remove(), Object(ie.a)(this, Re)[Re] = null, this.innerHTML = ""
                    }
                }
                set onContextMenu(e) {
                    Object(ie.a)(this, Ae)[Ae] = e
                }
                get onContextMenu() {
                    return Object(ie.a)(this, Ae)[Ae]
                }
                set onClose(e) {
                    Object(ie.a)(this, Le)[Le] = e
                }
                updateLang() {
                    Object(ie.a)(this, Ie)[Ie] = F.a.getCurrentLanguageName(), Object(ie.a)(this, Re)[Re] && (Object(ie.a)(this, Re)[Re].className = `zinstant-container ${Object(ie.a)(this,Ie)[Ie]}`)
                }
                setStatusFail() {
                    if (!Object(ie.a)(this, ve)[ve]) return Object(ie.a)(this, Ee)[Ee] = !0, Object(ie.a)(this, Ce)[Ce] = !0, void this.render(be);
                    Object(ie.a)(this, we)[we] || (Object(ie.a)(this, we)[we] = setTimeout((() => {
                        Object(ie.a)(this, ve)[ve] = !1, Object(ie.a)(this, we)[we] = null, Object(ie.a)(this, Ee)[Ee] = !0, Object(ie.a)(this, Ce)[Ce] = !0, this.render(be)
                    }), 1e3))
                }
                render(e) {
                    var t, r, n, i, a;
                    (e === me && (this.innerHTML = ""), e === ye) ? (Object(ie.a)(this, Re)[Re] || (Object(ie.a)(this, Te)[Te] = this.zinstantPlugin.bindUICallback((e => this.callbackFromZInstant(e))), Object(ie.a)(this, Re)[Re] = document.createElement("z-instant")), Object(ie.a)(this, Re)[Re].className = `zinstant-container ${Object(ie.a)(this,Ie)[Ie]}`, Object(ie.a)(this, Re)[Re].setAttribute("data-uikey", Object(ie.a)(this, Te)[Te]), Object(ie.a)(this, Re)[Re].setAttribute("data-url", null === (t = Object(ie.a)(this, De)[De]) || void 0 === t || null === (r = t.pcItem) || void 0 === r ? void 0 : r.data_url), Object(ie.a)(this, Re)[Re].setAttribute("data-retry", Object(ie.a)(this, Oe)[Oe]), Object(ie.a)(this, Re)[Re].setAttribute("data-msg", Object(ie.a)(this, Se)[Se]), Object(ie.a)(this, Re)[Re].setAttribute("data-local", oe.a.getFilePathFromUrl(null === (n = Object(ie.a)(this, De)[De]) || void 0 === n || null === (i = n.pcItem) || void 0 === i ? void 0 : i.data_url, Object(ie.a)(this, je)[je])), Object(ie.a)(this, Re)[Re].setAttribute("data-zns", JSON.stringify(null === (a = Object(ie.a)(this, De)[De]) || void 0 === a ? void 0 : a.pcItem)), Object(ie.a)(this, Re)[Re].setAttribute("data-theme", Object(ie.a)(this, Me)[Me]), Object(ie.a)(this, Re)[Re].setAttribute("data-locale", Object(ie.a)(this, Ie)[Ie]), Object(ie.a)(this, Re)[Re].oncontextmenu = this.onContextMenu, this.appendChild(Object(ie.a)(this, Re)[Re])) : e === pe || (e === be ? (Object(ie.a)(this, Re)[Re].remove(), Object(ie.a)(this, Re)[Re] = null, this.innerHTML = "") : e === ge && (Object(ie.a)(this, Re)[Re].remove(), Object(ie.a)(this, Re)[Re] = null, this.innerHTML = "", setTimeout((() => {
                        this.render(ye)
                    }), 1e3)))
                }
            }
            customElements.define("z-instant-anywhere", Fe);
            let Ne = Object(n.injectable)()(fe = function(e, t) {
                return Object(n.inject)(a.a)(e, void 0, 0)
            }(fe = function(e, t) {
                return Object(n.inject)(ne.ZLoggerFactory)(e, void 0, 1)
            }(fe = Reflect.metadata("design:type", Function)(fe = Reflect.metadata("design:paramtypes", [void 0 === a.a ? Object : a.a, void 0 === ne.ZLoggerFactory ? Object : ne.ZLoggerFactory])(fe = class extends s.a {
                constructor(e, t) {
                    super("zalo", e, t, {
                        component: re,
                        container: document.getElementById("app")
                    })
                }
                async start() {
                    this.setupIdleDetector(), await super.start()
                }
                getUserID() {
                    var e;
                    return (null === (e = this.getSession()) || void 0 === e ? void 0 : e.userId) || ""
                }
                setupIdleDetector() {
                    const e = this.config.get("idle_detector"),
                        t = new l(e);
                    t.addEventListener(o.Idle, (() => {
                        this.setStateToIdle()
                    })), t.addEventListener(o.Active, (() => this.setStateToActive())), t.start(), this.disposables.add((() => t.stop())), document.hasFocus() ? this.setStateToActive() : this.setStateToIdle()
                }
            }) || fe) || fe) || fe) || fe) || fe;
            n.ModuleContainer.registerSingleton(i.a, Ne)
        },
        DpY6: function(e, t, r) {
            "use strict";
            var n = r("GOkv");
            const i = (e, t) => {
                const r = new Uint8Array(e.length);
                for (let n = 0; n < e.length; n++) r[n] = e[n] ^ t[n % t.length];
                return r
            };
            var a, s = r("uplc"),
                o = r("hn13"),
                c = r("D5po"),
                d = r("VuQl"),
                l = r("yXvs");
            const u = {
                [o.a.AES_256_GCM]: 48,
                [o.a.NOISE_BITWISE]: 57,
                [o.a.SAFE_STORAGE]: 54
            };
            Object(c.c)(d.b.SecureCipher)(a = Reflect.metadata("design:type", Function)(a = Reflect.metadata("design:paramtypes", [])(a = class {
                constructor() {
                    this.isEnableKeychainEnc = void 0, this._needNotifyIsKeyChainEnabledToMain = void 0, this.isEnableKeychainEnc = !0
                }
                getCipherEncType(e) {
                    var t;
                    const r = new DataView(e.buffer);
                    return null === (t = Object.entries(u).find((([e, t]) => t === r.getUint8(0)))) || void 0 === t ? void 0 : t[0]
                }
                concatCipherWithType(e, t) {
                    const r = u[e],
                        n = new Uint8Array(1 + t.length);
                    return new DataView(n.buffer).setUint8(0, r), n.set(t, 1), n
                }
                setEnableKeychainEnc(e) {
                    e !== this.isEnableKeychainEnc && (this.isEnableKeychainEnc = e, this._needNotifyIsKeyChainEnabledToMain = !0, l.a.TrustProto.zsymb(0, "mEvak9", `[secure-cipher] ${e?"enable":"disable"} keychain encryption`))
                }
                async encrypt(e, t, r) {
                    let i;
                    const a = "function" == typeof r ? await r(e) : r;
                    switch (e) {
                        case o.a.AES_256_GCM:
                            i = await this.encryptAesGcm(t, Object(n.b)(a));
                            break;
                        case o.a.NOISE_BITWISE:
                            i = this.encryptNoise(t, Object(n.b)(a));
                            break;
                        case o.a.SAFE_STORAGE:
                            i = (await this.encryptSafeStorage(Object(s.b)(a), [t]))[0];
                            break;
                        default:
                            throw new Error("Unsupported encryption algorithm.")
                    }
                    return this.concatCipherWithType(e, i)
                }
                async notifyIsKeyChainEnabledToMain() {
                    await $zprotocol.syncConfigToMain({
                        isEnableKeychainEnc: this.isEnableKeychainEnc
                    }), this._needNotifyIsKeyChainEnabledToMain = !1
                }
                async decrypt(e, t) {
                    var r;
                    const i = new DataView(e.buffer),
                        a = null === (r = Object.entries(u).find((([e, t]) => t === i.getUint8(0)))) || void 0 === r ? void 0 : r[0],
                        c = e.slice(1);
                    let d;
                    const l = "function" == typeof t ? await t(a || void 0) : t;
                    switch (a) {
                        case o.a.AES_256_GCM:
                            d = await this.decryptAesGcm(c, Object(n.b)(l));
                            break;
                        case o.a.NOISE_BITWISE:
                            d = this.decryptNoise(c, Object(n.b)(l));
                            break;
                        case o.a.SAFE_STORAGE:
                            this._needNotifyIsKeyChainEnabledToMain && await this.notifyIsKeyChainEnabledToMain(), d = (await this.decryptSafeStorage(Object(s.b)(l), [c]))[0];
                            break;
                        default:
                            throw new Error(`Unsupported algorithm ID: ${a}`)
                    }
                    return d
                }
                async encryptMulti(e, t, r) {
                    const n = "function" == typeof r ? await r(e) : r;
                    if (e === o.a.SAFE_STORAGE) {
                        return (await this.encryptSafeStorage(Object(s.b)(n), t)).map((t => this.concatCipherWithType(e, t)))
                    }
                    const i = [],
                        a = [];
                    t.forEach(((t, n) => {
                        a.push((async () => {
                            const a = await this.encrypt(e, t, r);
                            i.push({
                                idx: n,
                                cipher: a
                            })
                        })())
                    })), a.length && await Promise.all(a), i.sort(((e, t) => e.idx - t.idx));
                    return i.map((e => e.cipher))
                }
                async decryptMulti(e, t) {
                    const r = [],
                        n = [],
                        i = [],
                        a = [];
                    if (e.forEach(((e, t) => {
                            this.getCipherEncType(e) === o.a.SAFE_STORAGE ? a.push({
                                cipherWithoutType: e.slice(1),
                                idx: t
                            }) : i.push({
                                cipher: e,
                                idx: t
                            })
                        })), a.length) {
                        this._needNotifyIsKeyChainEnabledToMain && await this.notifyIsKeyChainEnabledToMain();
                        const e = async () => {
                            const e = "function" == typeof t ? await t(o.a.SAFE_STORAGE) : t;
                            (await this.decryptSafeStorage(Object(s.b)(e), a.map((e => e.cipherWithoutType)))).forEach(((e, t) => {
                                r.push({
                                    text: e,
                                    idx: a[t].idx
                                })
                            }))
                        };
                        n.push(e())
                    }
                    i.length && i.forEach((({
                        cipher: e,
                        idx: i
                    }) => {
                        n.push((async () => {
                            const n = await this.decrypt(e, t);
                            r.push({
                                idx: i,
                                text: n
                            })
                        })())
                    })), n.length && await Promise.all(n), r.sort(((e, t) => e.idx - t.idx));
                    return r.map((e => e.text))
                }
                async encryptAesGcm(e, t) {
                    const r = window.crypto.getRandomValues(new Uint8Array(16)),
                        n = window.crypto.getRandomValues(new Uint8Array(12)),
                        i = await this.deriveKey(t, r),
                        a = await window.crypto.subtle.encrypt({
                            name: "AES-GCM",
                            iv: n
                        }, i, e),
                        s = new Uint8Array(r.length + n.length + a.byteLength);
                    return s.set(r, 0), s.set(n, r.length), s.set(new Uint8Array(a), r.length + n.length), s
                }
                async decryptAesGcm(e, t) {
                    const r = e.slice(0, 16),
                        n = e.slice(16, 28),
                        i = e.slice(28),
                        a = await this.deriveKey(t, r);
                    try {
                        const e = await window.crypto.subtle.decrypt({
                            name: "AES-GCM",
                            iv: n
                        }, a, i);
                        return new Uint8Array(e)
                    } catch (s) {
                        throw new Error("Decryption failed. Invalid key or corrupted data.")
                    }
                }
                async deriveKey(e, t) {
                    const r = await crypto.subtle.importKey("raw", e, {
                        name: "PBKDF2"
                    }, !1, ["deriveKey"]);
                    return await crypto.subtle.deriveKey({
                        name: "PBKDF2",
                        salt: t,
                        hash: "SHA-256",
                        iterations: 1e4
                    }, r, {
                        name: "AES-GCM",
                        length: 256
                    }, !0, ["encrypt", "decrypt"])
                }
                encryptNoise(e, t) {
                    return i(e, t)
                }
                decryptNoise(e, t) {
                    return ((e, t) => i(e, t))(e, t)
                }
                async encryptSafeStorage(e, t) {
                    const r = await $zprotocol.encryptWithKeychain(e, t, this.isEnableKeychainEnc);
                    return this._needNotifyIsKeyChainEnabledToMain = !1, r
                }
                async decryptSafeStorage(e, t) {
                    return await $zprotocol.decryptWithKeychain(e, t)
                }
            }) || a) || a)
        },
        GOkv: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            })), r.d(t, "b", (function() {
                return i
            }));
            const n = e => {
                    const t = atob(e);
                    return Uint8Array.from(t, (e => e.charCodeAt(0)))
                },
                i = e => e instanceof Uint8Array ? e : n(e)
        },
        JIt3: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return s
            }));
            var n = r("D5po"),
                i = r("VuQl"),
                a = r("rNEG");
            class s {
                get TrustProtocol() {
                    return n.a.resolve(i.b.TrustProtocol)
                }
                get TrustStorage() {
                    return n.a.resolve(i.b.TrustStorage)
                }
                get currentPlatform() {
                    return a.a.Pc
                }
            }
        },
        Jw0q: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return a
            }));
            var n = r("jDHv"),
                i = r("Mgpg");
            const a = n.ModuleContainer.resolve(i.ZLoggerFactory).createZLogger(["trusted-device"])
        },
        KDNU: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return i
            }));
            var n = r("gbcT");
            const i = async e => {
                try {
                    return await e()
                } catch (t) {
                    if (a(t)) throw t;
                    return Object(n.a)(e, 2)
                }
            }, a = e => {
                var t, r;
                return !("DataError" !== (null == e ? void 0 : e.name) || null == e || null === (t = e.message) || void 0 === t || null === (r = t.includes) || void 0 === r || !r.call(t, "Failed to read large IndexedDB value"))
            }
        },
        KwLe: function(e, t, r) {
            "use strict";
            var n = r("q1tI"),
                i = r("jDHv"),
                a = r("ODpZ"),
                s = r("nQja");
            t.a = () => {
                const [e, t] = Object(n.useState)(s.a.NONE);
                return Object(n.useEffect)((() => {
                    {
                        const e = i.ModuleContainer.resolve(a.a).checkAppVersion();
                        t(e)
                    }
                }), []), {
                    appVersionState: e
                }
            }
        },
        LCnh: function(e, t, r) {
            "use strict";
            var n, i = r("D5po"),
                a = r("VuQl"),
                s = r("AW0x");
            Object(i.b)(a.a.WeakCipher)(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [])(n = class {
                constructor() {
                    this.keySet = void 0, this._promiseWait = void 0, this.keySet = new Map, this._promiseWait = new s.a
                }
                async generateWeakEncryptionKey(e) {
                    const t = this.keySet.get(e);
                    return t || this._promiseWait.exec(`generateEnc_${e}`, (async () => {
                        const t = (new TextEncoder).encode(e),
                            r = await crypto.subtle.importKey("raw", t, {
                                name: "HKDF"
                            }, !1, ["deriveKey"]),
                            n = new Uint8Array([137, 118, 214, 88, 136, 203, 53, 62, 114, 204, 126, 118, 78, 103, 196, 185]),
                            i = await crypto.subtle.deriveKey({
                                name: "HKDF",
                                salt: n,
                                info: (new TextEncoder).encode("EncWeakKDF"),
                                hash: "SHA-256"
                            }, r, {
                                name: "AES-GCM",
                                length: 256
                            }, !0, ["encrypt", "decrypt"]),
                            a = await window.crypto.subtle.exportKey("raw", i),
                            s = new Uint8Array(a);
                        return this.keySet.set(e, s), s
                    }))
                }
            }) || n) || n)
        },
        Msg7: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return i
            }));
            var n = r("bUXd");
            const i = () => n.default.getTimeNow()
        },
        ODpZ: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return i
            }));
            var n = r("jDHv");
            const i = Object(n.define)("app-version-manager")
        },
        Ppsz: function(e, t, r) {
            "use strict";
            var n = r("jDHv"),
                i = r("hmll"),
                a = r("1pet"),
                s = r("NL/6");
            const o = [{
                chord: `${a.CmdOrCtrl}${a.K_I}`,
                commandId: s.a.FOCUS_MAIN_CHAT_INPUT_AT_ACTIVE_WINDOW,
                commandTitle: "",
                isMatchedContext: (e, t) => !0
            }, {
                chord: `${a.CmdOrCtrl}${a.K_I}`,
                commandId: s.a.APPLY_ITALIC_STYLE_FOR_SELECTED_TEXT,
                commandTitle: "",
                isMatchedContext: (e, t) => !0
            }, {
                chord: `${a.CmdOrCtrl}${a.K_B}`,
                commandId: s.a.APPLY_BOLD_STYLE_FOR_SELECTED_TEXT,
                commandTitle: "",
                isMatchedContext: (e, t) => !0
            }, {
                chord: `${a.CmdOrCtrl}${a.K_U}`,
                commandId: s.a.APPLY_UNDERLINE_STYLE_FOR_SELECTED_TEXT,
                commandTitle: "",
                isMatchedContext: (e, t) => !0
            }, {
                chord: `${a.CmdOrCtrl}${a.Shift}${a.K_X}`,
                commandId: s.a.TOGGLE_RTF_MODE,
                commandTitle: "",
                isMatchedContext: (e, t) => !0
            }];
            n.ModuleContainer.register(i.a, class {
                constructor() {
                    this._keybindings = [], o.forEach((e => {
                        this._keybindings.push({
                            chord: e.chord,
                            commandId: e.commandId,
                            commandTitle: e.commandTitle,
                            isMatchedContext: e.isMatchedContext,
                            eventHandlers: []
                        })
                    }))
                }
                registerEventHandlerByCommandId(e, t) {
                    return e && t ? (this._keybindings.forEach((r => {
                        r.commandId !== e || r.eventHandlers.includes(t) || r.eventHandlers.push(t)
                    })), () => {
                        this._keybindings.forEach((r => {
                            r.commandId === e && r.eventHandlers.includes(t) && (r.eventHandlers = r.eventHandlers.filter((e => e !== t)))
                        }))
                    }) : () => {}
                }
                loopkupKeybindingByCommandId(e) {
                    return e ? this._keybindings.reduce(((t, r) => (e === r.commandId && t.push(r), t)), []) : []
                }
                loopkupKeybindingByChord(e) {
                    return e ? this._keybindings.reduce(((t, r) => (e === r.chord && t.push(r), t)), []) : []
                }
            });
            var c = r("XRso"),
                d = r("Xt6Q");
            n.ModuleContainer.register(c.a, class {
                buildChord(e) {
                    let t = "";
                    return e && this._isChord(e) && ((e.ctrlKey || e.metaKey) && (t += a.CmdOrCtrl), e.shiftKey && (t += a.Shift), e.altKey && (t += a.Alt), t += e.keyCode || e.which), t
                }
                buildKeybindingContext(e, t) {
                    return e && t ? `${e}#${t}` : ""
                }
                getChordByCommandId(e) {
                    return e === s.a.TOGGLE_RTF_MODE ? (Object(d.d)() ? "Cmd" : "Ctrl") + " + Shift + X" : ""
                }
                _isOnlyModifier(e) {
                    return ["Shift", "Control", "Alt", "Meta"].includes(e.key)
                }
                _isOnlyNonModifier(e) {
                    return !(e.ctrlKey || e.metaKey || e.shiftKey || e.altKey)
                }
                _isChord(e) {
                    return !(this._isOnlyModifier(e) || this._isOnlyNonModifier(e))
                }
            })
        },
        V48A: function(e, t, r) {
            "use strict";
            var n = r("D5po"),
                i = r("VuQl"),
                a = r("0IcQ"),
                s = r("Z5tB"),
                o = r("KDNU"),
                c = r("hn13");
            var d, l = r("uplc"),
                u = r("yXvs");
            Object(n.c)(i.b.TrustStorage)(d = Reflect.metadata("design:type", Function)(d = Reflect.metadata("design:paramtypes", [])(d = class {
                constructor() {
                    this.encAlgorithm = void 0, this._repository = void 0, this._weakCipher = void 0, this._secureCipher = void 0, this.encAlgorithm = c.a.SAFE_STORAGE, this.encAlgorithm = c.a.SAFE_STORAGE
                }
                get repository() {
                    return this._repository || (this._repository = new s.a), this._repository
                }
                get weakCipher() {
                    return this._weakCipher || (this._weakCipher = n.a.resolve(i.a.WeakCipher)), this._weakCipher
                }
                get secureCipher() {
                    return this._secureCipher || (this._secureCipher = n.a.resolve(i.b.SecureCipher)), this._secureCipher
                }
                get LinkHistory() {
                    return this.repository.LinkHistory
                }
                get TrustedIdentity() {
                    return this.repository.TrustedIdentity
                }
                get DeviceList() {
                    return this.repository.DeviceList
                }
                setEnableKeychainEnc(e) {
                    this.secureCipher.setEnableKeychainEnc(e)
                }
                setEncryptionAlgorithm(e) {
                    e !== this.encAlgorithm && [c.a.AES_256_GCM, c.a.NOISE_BITWISE, c.a.SAFE_STORAGE].includes(e) && (u.a.TrustProto.zsymb(0, "4etp-L", `[trust-storage] changed enc algorithm from ${this.encAlgorithm} to ${e}`), this.encAlgorithm = e)
                }
                isEnabledKeychainEnc() {
                    return this.secureCipher.isEnableKeychainEnc
                }
                getCurrentEncryptionAlgorithm() {
                    return this.encAlgorithm
                }
                async getEncKey(e, t) {
                    return e === c.a.SAFE_STORAGE ? "Trusted ID" + t : await this.weakCipher.generateWeakEncryptionKey(t)
                }
                async saveNewDeviceLink(e, t, r, s, c, d, u, h, p) {
                    const b = n.a.resolve(i.b.SecureCipher),
                        g = await this.getEncKey(this.encAlgorithm, r),
                        m = await b.encryptMulti(this.encAlgorithm, [e.publicKey, e.privateKey, u, s], g);
                    return Object(o.a)((async () => {
                        const e = Object(l.a)(m[0]);
                        return this.repository.addNewIdentityAndUpdateLinkHistory({
                            publicKey: e,
                            privateKey: Object(l.a)(m[1])
                        }, {
                            masterId: c,
                            uid: r,
                            companionId: d,
                            linkingSecret: Object(l.a)(m[2]),
                            identity: e,
                            primaryIdentity: Object(l.a)(m[3]),
                            linkId: h,
                            linkingTimestamp: p
                        }, t, a.a.ACTIVE)
                    }))
                }
            }) || d) || d)
        },
        WrSY: function(e, t, r) {
            "use strict";
            r("EmOc"), r("ezdo"), r("KdAX"), r("mzek"), r("aBYf"), r("zm+Q"), r("WSI5"), r("v/rv"), r("nAZb"), r("c0KM"), r("zheM"), r("CXIs"), r("cZjg");
            var n = r("YrRS");
            Object(n.b)()
        },
        Ws6j: function(e, t, r) {
            "use strict";
            r("Of9+");
            var n, i = r("8/YW"),
                a = r("0B28"),
                s = r("YEoC"),
                o = r("X2RP"),
                c = r("ggcH"),
                d = r("rvru"),
                l = r("jDHv"),
                u = r("rkiK"),
                h = r("Mk04"),
                p = r("PoHQ"),
                b = r("DOOx"),
                g = r("Yggq"),
                m = r("HWGt");
            (new(Object(i.h)()(n = class extends m.a {
                constructor(...e) {
                    super(...e), this.dbQos = null, this.signalDisableCreatingBackup = Object(h.a)((e => {
                        a.a.getInstance().disableCreatingBackup(e)
                    })), this.signalEnableCreatingBackupForIDB = Object(h.a)((() => {
                        a.a.getInstance().enableCreatingBackup(s.a.IDB)
                    })), this.signalOutOfMem = Object(h.a)((() => {
                        p.p.triggerEvent(p.o, [{
                            type: b.b.OUT_OF_MEM,
                            src: b.a.DB
                        }])
                    })), this.signalInvalidVersion = Object(h.a)((() => {
                        $zdb.notifyOpenDBFailed(!0, "VersionError")
                    })), this.signalMissingStore = Object(h.a)((() => {
                        $zdb.notifyOpenDBFailed(!0, "zkey-miss-object-store")
                    })), this.signalExceedingLimitInReopeningDBConnection = Object(h.a)((() => {
                        $zdb.notifyOpenDBFailed(!1)
                    })), this.signalDBClosedAbnormally = Object(h.a)((() => {
                        l.ModuleContainer.resolve(c.TDBCorruptionDetector).forceCheckDBCorruption()
                    }))
                }
                getDBQos() {
                    return null === this.dbQos && (this.dbQos = l.ModuleContainer.resolve(d.a)), this.dbQos
                }
                handleQueryError(e) {
                    this.getDBQos().sendDBErrorQos(e)
                }
                handleOutOfMemError(e) {
                    this.signalOutOfMem(), this.signalDisableCreatingBackup(e.adapterType)
                }
                handleInvalidVersionError() {
                    this.signalInvalidVersion()
                }
                handleMissingStoreError(e) {
                    const {
                        database: t,
                        missingTables: r,
                        adapterType: n
                    } = e;
                    this.signalMissingStore(), this.signalDisableCreatingBackup(n);
                    this.getDBQos().sendMissingTableQos(t, r)
                }
                handleExceedingLimitInReopeningDBConnection(e) {
                    this.signalExceedingLimitInReopeningDBConnection(), this.signalDisableCreatingBackup(e.adapterType)
                }
                handleQueryExec(e) {
                    const {
                        database: t,
                        table: r,
                        method: n,
                        transaction: i,
                        startTime: a,
                        getEndTime: s
                    } = e;
                    g.a.has(t) || s().then((e => {
                        const s = {
                            method: n,
                            database: t,
                            table: r,
                            transaction: i
                        };
                        u.default.recordInfo(u.MetricName.query_resolution_time, {
                            startAt: a,
                            endAt: e,
                            info: s
                        })
                    })).catch((e => {}))
                }
                handleSuccessOpenDB(e) {
                    const {
                        startTime: t,
                        endTime: r,
                        fullname: n,
                        adapterType: i
                    } = e;
                    this.getDBQos().sendSuccessOpenDBDurationQos(n, t, r - t), u.default.recordInfo(u.MetricName.db_ready, {
                        startAt: t,
                        endAt: r,
                        info: {
                            dbName: n
                        }
                    }), i === s.a.IDB && this.signalEnableCreatingBackupForIDB()
                }
                handleConnectionClosedAbnormally() {
                    this.signalDBClosedAbnormally()
                }
                handleSchemaMigratedAbnormally() {}
                handleLongOpenDB(e) {
                    const {
                        startTime: t,
                        endTime: r,
                        fullname: n
                    } = e;
                    this.getDBQos().sendLongOpenRequestQos(n, t, r - t)
                }
                handleTimeConsumingQuery(e, t) {
                    this.getDBQos().sendTimeConsumingQueryQos(e, t)
                }
                handleWarning(e) {
                    const t = this.getDBQos();
                    if (e.type === o.b.FAILED_MULTI) t.sendFailedMultiErrorQos(e)
                }
                onDispose() {
                    this.dispose()
                }
            }) || n)).start()
        },
        Z5tB: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return y
            }));
            var n = r("fsN4"),
                i = r("VTBJ"),
                a = r("bUXd"),
                s = r("wH4e"),
                o = r("KDNU"),
                c = r("D5po"),
                d = r("VuQl"),
                l = r("GOkv"),
                u = r("XmdN");
            class h {
                async findByLatestLinkingTimestamp() {
                    const e = await Object(o.a)((async () => {
                        const e = n.default.getInstance();
                        return (await e.Trust.LinkHistory.getAll({
                            from: [0],
                            to: [a.default.getTimeNow()]
                        }, {
                            index: "linkingTimestamp",
                            direction: s.c.PREV,
                            limit: 1
                        }))[0]
                    }));
                    return e ? this.decipherDbRecord(e) : void 0
                }
                async findByMasterId(e) {
                    const t = await Object(o.a)((async () => {
                        const t = n.default.getInstance();
                        return await t.Trust.LinkHistory.get(e)
                    }));
                    return t ? this.decipherDbRecord(t) : void 0
                }
                async deleteMultiByUid(e) {
                    return Object(o.a)((async () => {
                        const t = n.default.getInstance();
                        return await t.Trust.LinkHistory.findAndDelete(void 0, {
                            filter: {
                                uid: e
                            }
                        })
                    }))
                }
                async deleteByMasterId(e) {
                    return Object(o.a)((async () => {
                        const t = n.default.getInstance();
                        return await t.Trust.LinkHistory.delete(e)
                    }))
                }
                async decipherDbRecord(e) {
                    try {
                        const t = c.a.resolve(d.b.TrustStorage),
                            r = c.a.resolve(d.b.SecureCipher),
                            n = await r.decryptMulti([Object(l.a)(e.identity), Object(l.a)(e.linkingSecret), Object(l.a)(e.primaryIdentity)], (r => t.getEncKey(r, e.uid)));
                        return Object(i.a)(Object(i.a)({}, e), {}, {
                            identity: n[0],
                            linkingSecret: n[1],
                            primaryIdentity: n[2]
                        })
                    } catch (t) {
                        throw new u.a((null == t ? void 0 : t.message) || "", {
                            masterId: e.masterId
                        })
                    }
                }
            }
            var p = r("0IcQ"),
                b = r("uplc");
            class g {
                async findByUid(e) {
                    const t = await Object(o.a)((async () => {
                        const t = n.default.getInstance();
                        return await t.Trust.TrustIdentity.get(e)
                    }));
                    return t ? this.decipherDbRecord(t) : void 0
                }
                async deleteByUid(e) {
                    return Object(o.a)((async () => {
                        const t = n.default.getInstance();
                        return await t.Trust.TrustIdentity.delete(e)
                    }))
                }
                async upsert(e) {
                    const t = await this.encrypt(e);
                    return Object(o.a)((async () => {
                        const e = n.default.getInstance();
                        await e.Trust.TrustIdentity.insert(t)
                    }))
                }
                async updateStatus(e, t) {
                    const r = n.default.getInstance();
                    return await r.Trust.TrustIdentity.update(e, {
                        attributes: ["status"],
                        value: {
                            status: t
                        },
                        ignoreNotFound: !0
                    })
                }
                async encrypt(e) {
                    try {
                        const t = c.a.resolve(d.b.TrustStorage),
                            r = c.a.resolve(d.b.SecureCipher),
                            n = t.getCurrentEncryptionAlgorithm(),
                            a = await t.getEncKey(n, e.uid),
                            s = await r.encryptMulti(n, [e.pubKey, e.privKey, e.primaryKey], a);
                        return Object(i.a)(Object(i.a)({}, e), {}, {
                            pubKey: Object(b.a)(s[0]),
                            privKey: Object(b.a)(s[1]),
                            primaryKey: Object(b.a)(s[2])
                        })
                    } catch (t) {
                        throw t
                    }
                }
                async decipherDbRecord(e) {
                    try {
                        const t = c.a.resolve(d.b.TrustStorage),
                            r = c.a.resolve(d.b.SecureCipher),
                            n = await r.decryptMulti([Object(l.a)(e.pubKey), Object(l.a)(e.privKey), Object(l.a)(e.primaryKey)], (r => t.getEncKey(r, e.uid))),
                            a = function(e) {
                                return Object.values(p.a).includes(e)
                            }(e.status) ? e.status : p.a.ACTIVE;
                        return Object(i.a)(Object(i.a)({}, e), {}, {
                            pubKey: n[0],
                            privKey: n[1],
                            primaryKey: n[2],
                            status: a
                        })
                    } catch (t) {
                        throw new u.a((null == t ? void 0 : t.message) || "", {
                            uid: e.uid,
                            dormancy: e.status === p.a.DORMANT
                        })
                    }
                }
            }
            class m {
                async upsert(e, t) {
                    return Object(o.a)((async () => {
                        const r = n.default.getInstance();
                        await r.E2ee.DeviceList.insert({
                            uid: e,
                            devices: t
                        })
                    }))
                }
                async getByUid(e) {
                    return Object(o.a)((async () => {
                        const t = n.default.getInstance(),
                            r = await t.E2ee.DeviceList.get(e);
                        return null == r ? void 0 : r.devices
                    }))
                }
            }
            class y {
                constructor() {}
                get LinkHistory() {
                    return new h
                }
                get TrustedIdentity() {
                    return new g
                }
                get DeviceList() {
                    return new m
                }
                async addNewIdentityAndUpdateLinkHistory(e, t, r, i) {
                    const a = n.default.getInstance();
                    await a.Trust.runTransactionV2([a.Trust.TrustIdentity, a.Trust.LinkHistory], (async ([n, a]) => {
                        await Promise.all([n.insert({
                            uid: t.uid,
                            pubKey: e.publicKey,
                            privKey: e.privateKey,
                            primaryKey: t.primaryIdentity,
                            verified: r,
                            status: i
                        }), a.insert({
                            masterId: t.masterId,
                            uid: t.uid,
                            companionId: t.companionId,
                            linkingSecret: t.linkingSecret,
                            identity: t.identity,
                            primaryIdentity: t.primaryIdentity,
                            linkId: t.linkId,
                            linkingTimestamp: t.linkingTimestamp
                        })])
                    }))
                }
            }
        },
        bO0B: function(e, t, r) {
            "use strict";
            r("v5OU")
        },
        daJc: function(e, t, r) {
            "use strict";
            var n = r("jDHv"),
                i = r("OMsT"),
                a = r("XS0u"),
                s = r("Rw5y"),
                o = r("IpzU"),
                c = r("7FSS"),
                d = r("xOOu"),
                l = r.n(d),
                u = r("5FGm");
            const h = /^(((main|login|photo|render|shared-worker|preload-shared-worker|u-process-sqlite|preload-sqlite|zalo|zcall|scrshot|compact-app|main-compact-app|zlog-config|cloud-.*|migrate-log)\.(zlog|log|module|meta|dmeta|calf|txt|json))|(voip\.old|voip)\.log|((zavi|crash)\.dmp))|(DNetLog_\d+\.log)$/;
            var p;
            const b = $znode.fs,
                g = $znode.path,
                m = $znode.os;
            let y = Object(n.injectable)()(p = class {
                constructor() {
                    this.appdataFolder = null
                }
                clientDeviceInfo() {
                    const e = m.platform(),
                        t = m.release(),
                        r = m.hostname(),
                        n = m.type();
                    return JSON.stringify({
                        name: r,
                        model: n,
                        os: e,
                        osVersion: t,
                        build: "production-release",
                        pversion: "26.3.20",
                        bhash: "e398d5fa289851dbfe3cd73c299653a062178289"
                    })
                }
                async getLogDir() {
                    return this.appdataFolder || (this.appdataFolder = await Object(o.getuserDataDir)()), this.appdataFolder
                }
                async prepareLogBlob(e) {
                    return new Promise(((t, r) => {
                        let n = "";
                        this.getLogDir().then((e => {
                            const t = u.b;
                            n = g.join(e, t);
                            return {
                                main: {
                                    fn: s.a,
                                    args: [{
                                        fs: b
                                    }, n]
                                },
                                other: {
                                    fn: $zFileManager.scandirLog,
                                    args: [n]
                                }
                            }.other
                        })).then((e => e.fn(...e.args))).then((t => {
                            var r, i;
                            let a = [],
                                s = [];
                            return s = t.filter((e => {
                                const t = h.test(e),
                                    r = e.endsWith(".zlog") || e.endsWith(".calf") || e.endsWith(".module") || e.endsWith(".meta") || e.startsWith("DNetLog_") && h.test(e);
                                return !!h.test(e) && (t && r)
                            })).map((e => g.join(this.appdataFolder, e))), s.push(...e.extraFiles), s.forEach((e => {
                                a.push(this.readFile(e))
                            })), a = (null === (r = s) || void 0 === r ? void 0 : r.map((e => this.readFile(g.join(n, e))))) || [], null == e || null === (i = e.extraFiles) || void 0 === i || i.forEach((e => {
                                var t;
                                a.push((t = e, new Promise((e => {
                                    $zFileManager.getExtraLogFiles(t).then((r => {
                                        e({
                                            name: g.basename(t),
                                            data: r
                                        })
                                    })).catch((r => {
                                        e({
                                            name: g.basename(t),
                                            data: new ArrayBuffer(0)
                                        })
                                    }))
                                }))))
                            })), a
                        })).then((e => Promise.all(e))).then((e => {
                            const t = new l.a;
                            e.forEach((e => {
                                e && e.data.byteLength > 0 && t.file(e.name, e.data)
                            }));
                            {
                                const e = "device.zinfo",
                                    r = new TextEncoder;
                                t.file(e, r.encode(this.clientDeviceInfo()).buffer)
                            }
                            return t.generateAsync({
                                type: "arraybuffer",
                                compression: "DEFLATE"
                            })
                        })).then((r => {
                            let n = e.viewerKey;
                            if (!n) {
                                n = a.default.getLastViewKey() || "";
                                try {
                                    var i;
                                    n = null === (i = n) || void 0 === i ? void 0 : i.split(".")[0]
                                } catch {}
                            }
                            const s = `zlog_${n}_26.3.20-production_${Date.now()}.zip`;
                            if (e.bareContent) t({
                                name: s,
                                data: new Uint8Array(r)
                            });
                            else {
                                const e = new Blob([new Uint8Array(r)]);
                                e.name = s, t(e)
                            }
                        })).catch((e => {
                            c.a.error("[ZLL]: failed to prepare log for sending log", e), r(e)
                        }))
                    }))
                }
                async readFile(e) {
                    const t = g.basename(e);
                    let r;
                    try {
                        r = await $zFileManager.readZLog(t)
                    } catch (n) {
                        r = new ArrayBuffer(0)
                    }
                    return {
                        name: t,
                        data: r
                    }
                }
            }) || p;
            n.ModuleContainer.registerSingleton(i.a, y)
        },
        "f/DQ": function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            }));
            class n {
                static toQualifiedDeviceList(e) {
                    return {
                        masterId: e.devices.masterId,
                        primaryPublicIdentityKey: e.devices.encIdentity,
                        timestamp: e.devices.lastUpdateTs,
                        signature: e.devices.encSignature,
                        data: (e.devices.companions || []).map((e => ({
                            companionId: e.id,
                            linkId: e.linkId,
                            publicIdentityKey: e.encIdentity,
                            platform: e.platform,
                            status: e.status,
                            expiration: e.expireTs,
                            accountSignature: e.encAccSign,
                            deviceSignature: e.encDeviceSign
                        })))
                    }
                }
            }
        },
        fNRA: function(e, t, r) {
            "use strict";
            var n, i = r("8/YW"),
                a = r("Hw41"),
                s = r("YEoC"),
                o = r("PmZf"),
                c = r("bSii"),
                d = r("tHMN"),
                l = r("d/or"),
                u = r("1UUk"),
                h = r("jDHv"),
                p = r("xM06"),
                b = r("8eps"),
                g = r("fsN4"),
                m = r("Mgpg"),
                y = r("PObO");
            let f = Object(i.h)()(n = h.ModuleContainer.injectable()(n = function(e, t) {
                return h.ModuleContainer.inject(d.b)(e, void 0, 0)
            }(n = function(e, t) {
                return h.ModuleContainer.inject(l.a)(e, void 0, 1)
            }(n = function(e, t) {
                return h.ModuleContainer.inject(m.ZLoggerFactory)(e, void 0, 2)
            }(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [void 0 === d.a ? Object : d.a, void 0 === l.a ? Object : l.a, void 0 === m.ZLoggerFactory ? Object : m.ZLoggerFactory])(n = class extends u.a {
                constructor(e, t, r) {
                    super(), this.engine = e, this.settingsManager = t, this.logger = void 0, this.removeListeners = () => {}, this.logger = r.createZLogger("db", ["host"]), this.registerListeners()
                }
                registerListeners() {
                    const e = e => this.dispatchEvent(e),
                        t = [];
                    t.push(this.engine.addEventListener(o.b.UnexpectedError, e)), t.push(this.engine.addEventListener(o.b.QueryExec, e)), t.push(this.engine.addEventListener(o.b.QueryError, e)), t.push(this.engine.addEventListener(o.b.SuccessOpenDB, e)), t.push(this.engine.addEventListener(o.b.LongOpenDB, e)), t.push(this.engine.addEventListener(o.b.TimeConsumingQuery, e)), t.push(this.engine.addEventListener(o.b.ConnectionClosedAbnormally, e)), t.push(this.engine.addEventListener(o.b.SchemaMigratedAbnormally, e)), t.push(this.engine.addEventListener(o.b.Warning, e)), t.push(this.addEventListener(o.b.SessionChange, (() => {
                        Object(c.c)(!1)
                    }))), this.removeListeners = () => t.forEach((e => e()))
                }
                onDispose() {
                    this.removeListeners()
                }
                removePartitionKeyTableMapping() {
                    Object(c.b)() || this.engine.deleteDatabase("Meta").then((() => {
                        Object(c.c)(!0)
                    })).catch((e => {
                        this.logger.zsymb(18, "w2_g0V", "Failed to remove partition key table mapping", e)
                    }))
                }
                install() {
                    $zsub.$zdb.onRequestCloseDbs((async (e, t) => {
                        this.logger.zsymb(0, "mgYvEG", "closing requested databases ..."), await this.closeDBsForThisScript(t), $zdb.notifyFinishCloseDb(__ZaBUNDLENAME__), this.logger.zsymb(0, "bDTuY9", "closed database")
                    })), $zdb.notifyDbConnected(__ZaBUNDLENAME__)
                }
                areYouOk() {
                    return !0
                }
                authenticate(e) {
                    if ($zdb.authenticateDb(e), e) {
                        this.session = e, this.dispatchEvent(new o.g(e)), this.engine.authenticate(e), this.logger.zsymb(0, "jpRtc3", (() => ["authenticated", `id: ${e.userId}`]));
                        h.ModuleContainer.resolve(y.a).authenticate(e)
                    }
                }
                async closeDBsForThisScript(e, t) {
                    const r = g.default.getInstance(),
                        n = this.logger;
                    let i = [];
                    if (e) {
                        if (e.length) {
                            let a = [...e];
                            n.zsymb(3, "AQkT9z", ["Start closing dbs: {}", "5iBscp"], e), n.zsymb(3, "jePbPk", ["Wait for dbs to be closed: {}", "5hHScQ"], a), i = e.map((async e => {
                                let i = r[e];
                                if (void 0 === i) {
                                    const t = h.ModuleContainer.resolve(b.a).getValue().Meta;
                                    if (t.databaseName !== e) return void n.zsymb(21, "LzxknH", ["Can't close invalid db: {}", "RjNGnT"], e);
                                    i = t
                                }
                                await i.closeThisDatabase(t), a = a.filter((t => t !== e)), 0 === a.length ? n.zsymb(0, "4pB89l", "Done closing dbs") : n.zsymb(3, "YQqamh", ["Wait for dbs to be closed: {}", "5hHScQ"], a)
                            }))
                        }
                    } else i = [r.closeAllDatabases(t)];
                    await Promise.all(i)
                }
                async signalCloseDBs(e, t) {
                    var r;
                    this.logger.zsymb(0, "kORK02", "send closing requested databases signal"), null !== (r = window.$zenv) && void 0 !== r && r.isPreloaded && (await $zdb.requestCloseDbs(e, __ZaBUNDLENAME__, t), this.logger.zsymb(0, "dqdLuW", "All clients have finished closing the requested DBs"))
                }
                async closeDBs(e, t) {
                    void 0 === e ? this.logger.zsymb(0, "KmEu7M", "close all databases") : this.logger.zsymb(3, "3SrbQG", ["close the following database: {}", "7Gp0PJ"], e);
                    const r = (await this.settingsManager.getInUseAdaperTypes()).map((async r => {
                        try {
                            if (r === s.a.IDB) return this.signalCloseDBs(e, t);
                            if (r === s.a.SQLite) return e ? this.signalCloseDBs(e, t) : this.signalSQLiteProcessToCloseAllDBs()
                        } catch (n) {
                            this.logger.zsymb(21, "vyMBgf", ["error close database: {}", "a2juJT"], n)
                        }
                    }));
                    await Promise.all(r)
                }
                migrateData(e, t) {
                    throw new Error("Medthod not implement")
                }
                async signalSQLiteProcessToCloseAllDBs() {
                    const e = {
                            typeID: p.j,
                            data: {
                                type: p.e,
                                data: {}
                            }
                        },
                        t = h.ModuleContainer.resolve(a.b),
                        r = await t.getPort();
                    await r.invokeMessage(e)
                }
            }) || n) || n) || n) || n) || n) || n) || n;
            h.ModuleContainer.registerSingleton(u.b, f)
        },
        gbcT: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            }));
            const n = async (e, t = 10, r) => {
                let n = 0;
                const i = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89],
                    a = () => new Promise(((s, o) => {
                        n++;
                        setTimeout((() => {
                            e().then((e => s(e))).catch((e => n >= t ? o(e) : s(a())))
                        }), 1e3 * (r && i[n] > r ? r : i[n]))
                    }));
                return a()
            }
        },
        hn13: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            }));
            let n = function(e) {
                return e.AES_256_GCM = "AES_256_GCM", e.NOISE_BITWISE = "NOISE_BITWISE", e.SAFE_STORAGE = "SAFE_STORAGE", e
            }({})
        },
        jw3m: function(e, t, r) {},
        mneU: function(e, t, r) {
            "use strict";
            var n = r("Y65e"),
                i = r("jDHv"),
                a = r("8/YW"),
                s = r("+ExH"),
                o = r("ycTR"),
                c = r("wH4e"),
                d = r("Mgpg"),
                l = r("CDcE"),
                u = r("ODpZ"),
                h = r("nQja"),
                p = r("rkiK");
            var b = r("QrVG");
            class g {
                constructor(e) {
                    this.versionConfigs = void 0, this.versionConfigs = e || []
                }
                getConfigList() {
                    return this.versionConfigs
                }
                getConfigByDbName(e) {
                    return this.versionConfigs.find((t => t.name === e))
                }
                serialize() {
                    return this.versionConfigs.length ? JSON.stringify(this.versionConfigs) : ""
                }
                toString() {
                    return this.versionConfigs.map((e => `${e.name}::${e.version}`)).join(" | ")
                }
                static deserialize(e) {
                    try {
                        let t = [];
                        const r = JSON.parse(e);
                        return null != r && r.length && r.forEach((e => {
                            null != e && e.name && Number.isInteger(Number(e.version)) && t.push({
                                name: e.name,
                                version: Number(e.version)
                            })
                        })), new this(t)
                    } catch (t) {}
                    return new this
                }
            }
            var m, y, f, v, O, E, C, I, w, T;
            m = Object(a.j)(), y = Object(a.h)(), f = Object(i.singleton)(u.a), v = Reflect.metadata("design:type", Function), O = Reflect.metadata("design:paramtypes", []), E = p.MetriczMessenger.invoker("appVersionManager.saveLastUsedInfo"), C = Reflect.metadata("design:type", Function), I = Reflect.metadata("design:paramtypes", [Number, String]), m(w = y(w = f(w = v(w = O((T = class {
                constructor() {
                    this._logger = void 0, this.currentDbVersion = void 0, this.appVersionState = void 0
                }
                async requestUpdateLastUsedAppVersionInfo(e, t) {
                    return [e, t]
                }
                async onStart() {
                    if (null == this.appVersionState && (this.appVersionState = this.verifyAppVersion(), this.afterVerifyAppVersion()), this.appVersionState === h.a.VERIFIED) {
                        this.currentDbVersion || (this.currentDbVersion = this.getCurrentDBVersion());
                        try {
                            await this.requestUpdateLastUsedAppVersionInfo(5, this.currentDbVersion.serialize()), this.logger.zsymb(0, "oYul56", `save app version success. majorVersion 5; dbVersion ${this.currentDbVersion.toString()}`)
                        } catch (e) {
                            this.logger.zsymb(18, "TbBJjS", `failed on save app version state. ERR ${Object(l.e)(e)}`)
                        }
                    }
                }
                onDispose() {}
                checkAppVersion() {
                    return h.a.VERIFIED
                }
                verifyAppVersion() {
                    return h.a.VERIFIED;
                }
                afterVerifyAppVersion() {}
                verifyDbVersion(e, t) {
                    for (const r of t.getConfigList()) {
                        const t = e.getConfigByDbName(r.name);
                        if (t && t.version > r.version) return this.logger.zsymb(18, "bcNfbp", `verifyDbVersion: old DB version. dbName ${r.name}; current ${r.version}; lastUsed ${t.version}`), !1
                    }
                    return !0
                }
                get logger() {
                    return this._logger || (this._logger = i.ModuleContainer.resolve(d.ZLoggerFactory).createZLogger("app-version", ["manager"])), this._logger
                }
                getCurrentDBVersion() {
                    const e = Object.values(s.a).map((e => e.baseConfig.name)),
                        t = [];
                    e.forEach((e => {
                        const r = Object(o.a)(e, c.a.SQLite);
                        r && t.push({
                            name: e,
                            version: r.version
                        })
                    }));
                    return new g(t)
                }
            }, Object(n.a)(T.prototype, "requestUpdateLastUsedAppVersionInfo", [E, C, I], Object.getOwnPropertyDescriptor(T.prototype, "requestUpdateLastUsedAppVersionInfo"), T.prototype), w = T)) || w) || w) || w) || w)
        },
        nQja: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return n
            }));
            let n = function(e) {
                return e.VERIFIED = "VERIFIED", e.OLD_MAJOR_VERSION = "OLD_MAJOR_VERSION", e.OLD_DB_VERSION = "OLD_DB_VERSION", e.NONE = "NONE", e
            }({})
        },
        o0oJ: function(e, t, r) {
            (function(e) {
                const t = {};

                function r(e) {
                    return t[e] || (t[e] = 0), t[e] += 1, 100 * e + t[e]
                }
                if (!e.perf) {
                    let t = () => Date.now();
                    const n = {
                        STARTUP: r(1),
                        MIGRATION_DONE: r(2),
                        MAIN_SCRIPT: r(2),
                        LOADED_MAIN_SCRIPT: r(3),
                        MAIN_APP_READY: r(3),
                        LOADED_STARTUP_SCRIPT: r(2),
                        STARTUP_BEFORE_HEAVY: r(3),
                        CREATE_MAIN_WINDOW: r(3),
                        SHOW_MAIN_WINDOW: r(3),
                        MAIN_WINDOW_LOADING: r(3),
                        MAIN_WINDOW_LOADED: r(3),
                        APP_STARTUP: r(2),
                        LOAD_APP_SCRIPT: r(3),
                        APP_DID_MOUNT: r(3),
                        CHATBOX_DID_MOUNT: r(3),
                        SELECT_CONVERSATION: r(1),
                        SELECTED_CONVERSATION: r(2)
                    };
                    e.perf = {
                        ...n,
                        perfRecords: [],
                        record: r => {
                            r || (r = 0);
                            const n = [r, t()];
                            e.perf.perfRecords.push(n)
                        }
                    }
                }
            }).call(this, r("yLpj"))
        },
        qguG: function(e, t, r) {
            "use strict";
            r.d(t, "c", (function() {
                return o
            })), r.d(t, "b", (function() {
                return c
            })), r.d(t, "a", (function() {
                return d
            }));
            var n = r("igA5"),
                i = r("GW+e"),
                a = r("g5UK");
            const s = "temp_renew_session_from_login";
            async function o(e) {
                for (let o = 1; o <= 2; o++) try {
                    return n.default.getInstance().setSecureItemForCurrentUser(a.RENEW_SESSION_FROM_LOGIN, JSON.stringify(e)), void(o > 1 && i.a.zsymb(0, "hMMPDp", `setRenewSessionFromLoginFlow succeeded on attempt ${o}`))
                } catch (t) {
                    const a = 2 === o,
                        c = t instanceof Error ? t.message : String(t);
                    if (a) try {
                        return n.default.getInstance().setItem(s, JSON.stringify(e)), void i.a.zsymb(18, "2_NCJa", "SecureLocalStorage not initialized, saved to temp storage")
                    } catch (r) {
                        throw i.a.zsymb(18, "zS4i-B", "Failed to save to temp storage:", r), new Error("Failed to save to both SecureLocalStorage and temp storage")
                    } else i.a.zsymb(18, "7Uqf3M", `setRenewSessionFromLoginFlow error on attempt ${o}`, c || t), await new Promise((e => setTimeout(e, 5e3)))
                }
            }

            function c() {
                const e = n.default.getInstance();
                try {
                    const t = e.getItem(s);
                    if (t) return i.a.zsymb(0, "HQPIYG", "Retrieved renew session from temp storage"), JSON.parse(t)
                } catch (t) {
                    i.a.zsymb(18, "jBrKEC", "Error reading from temp storage:", t)
                }
                try {
                    const t = e.getSecureItemForCurrentUser(a.RENEW_SESSION_FROM_LOGIN);
                    if (!t) return null;
                    return JSON.parse(t) || null
                } catch (t) {
                    return i.a.zsymb(18, "NtYjLb", "get renew session from login flow error:", t), null
                }
            }

            function d() {
                const e = n.default.getInstance();
                try {
                    e.removeItem(s)
                } catch (t) {
                    i.a.zsymb(18, "DUauek", "clear temp renew session error:", t)
                }
                try {
                    e.removeItemForCurrentUser(a.RENEW_SESSION_FROM_LOGIN)
                } catch (t) {
                    throw i.a.zsymb(18, "l1tjjN", "clear renew session from login flow error:", t), t
                }
            }
        },
        rt1H: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return i
            }));
            var n = r("Eb1N");
            const i = e => {
                let t = [];
                return ((e, t) => {
                    const r = new Set,
                        i = e => {
                            if (e && "object" == typeof e && !r.has(e)) {
                                if (r.add(e), (null == e ? void 0 : e.buffer) instanceof ArrayBuffer && (e => {
                                        try {
                                            return "detached" in e ? e.detached : 0 === e.byteLength
                                        } catch (t) {
                                            return !0
                                        }
                                    })(e.buffer)) {
                                    const e = new Error("An ArrayBuffer is detached and could not be cloned");
                                    throw e.name = "DataCloneError", e
                                }
                                if (e instanceof n.a) {
                                    const n = e.buffer;
                                    r.has(n) || (t.push(n), r.add(n))
                                } else if (Array.isArray(e))
                                    for (const t of e) i(t);
                                else if (e instanceof Map) e.forEach(((e, t) => {
                                    i(t), i(e)
                                }));
                                else if (e instanceof Set) e.forEach((e => {
                                    i(e)
                                }));
                                else if (e.constructor === Object)
                                    for (const t in e) Object.prototype.hasOwnProperty.call(e, t) && i(e[t])
                            }
                        };
                    i(e)
                })(e, t), {
                    payload: e,
                    transferList: t
                }
            }
        },
        szdT: function(e, t, r) {
            "use strict";
            var n, i = r("8/YW"),
                a = r("nMWc"),
                s = r("jDHv");
            let o = Object(i.h)()(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [])(n = class extends a.a {
                constructor() {
                    super(), this.disposeEventListeners = () => {}, this.setUpEventListeners()
                }
                setUpEventListeners() {
                    const e = [];
                    {
                        const t = (e, t) => {
                            this.updateEventMapOnStartIdleEvent(t);
                            for (const r of this.startIdleListeners) r(t)
                        };
                        $zsub.$zapp.onStartIdle(t), e.push((() => {
                            $zsub.$zapp.removeListenStartIdle(t)
                        }))
                    } {
                        const t = e => {
                            this.updateEventMapOnEndIdleEvent();
                            for (const t of this.endIdleListeners) t()
                        };
                        $zsub.$zapp.onEndIdle(t), e.push((() => {
                            $zsub.$zapp.removeListenEndIdle(t)
                        }))
                    }
                    this.disposeEventListeners = () => {
                        for (const t of e) t()
                    }
                }
                registerIdleCheckInterval(e) {
                    $zapp.registerIdleCheckInterval(e)
                }
                onDispose() {
                    this.disposeEventListeners()
                }
                preload_onIdleCheckIntervalRegistered(e, t) {
                    throw new Error("Method not implemented.")
                }
            }) || n) || n) || n;
            s.ModuleContainer.registerSingleton(a.b, o)
        },
        tPRg: function(e, t, r) {
            "use strict";
            r("o0oJ"), r("dThN"), r("z0WU"), r("vSga"), r("XS0u");
            perf.record(perf.LOAD_APP_SCRIPT),
                function() {
                    window.__loadTimer && clearTimeout(window.__loadTimer);
                    window.__startTime && $zlogger.loadShellQos(Date.now() - window.__startTime)
                }()
        },
        vUp1: function(e, t, r) {
            "use strict";
            var n, i = r("VTBJ"),
                a = r("JTyQ"),
                s = r("hI9i"),
                o = r("rCQs"),
                c = r("rfrl"),
                d = r("7nHs");
            const l = {
                isShown: !1,
                modalTitle: "",
                guideTitle: "",
                errorCode: "",
                guideItems: [],
                showPrimaryBtn: !0,
                primaryBtnName: "STR_BU_CONFIRM_TEXT_6",
                showSecondaryBtn: !0,
                secondaryBtnName: "STR_BU_CANCEL_TEXT_6",
                adapterType: a.b
            };
            Object(o.b)(d.a)(n = class {
                constructor() {
                    this.name = d.b, this.key = "", this.state = l
                }
                setState(e) {
                    const t = Object(c.b)(this.state, e);
                    this.state !== t && (this.state = t, Object(s.h)(this.name, ""))
                }
                showModal(e) {
                    this.state.isShown || this.setState((t => Object(i.a)(Object(i.a)(Object(i.a)({}, t), e), {}, {
                        isShown: !0
                    })))
                }
                closeModal() {
                    this.state.isShown && this.setState((e => Object(i.a)(Object(i.a)(Object(i.a)({}, e), l), {}, {
                        isShown: !1
                    })))
                }
                init(e) {}
                getItem(e, t) {
                    return this.state
                }
                getList(e, t) {
                    return []
                }
                onGetItemFailure(e, t) {
                    throw new Error("Method not implemented.")
                }
                onGetListFailure(e, t) {
                    throw new Error("Method not implemented.")
                }
            })
        },
        xDXR: function(e, t, r) {
            "use strict";
            var n, i = r("VTBJ"),
                a = r("mwIZ"),
                s = r.n(a),
                o = r("D1y2"),
                c = r.n(o),
                d = r("jDHv"),
                l = r("Y58e"),
                u = r("Ydol"),
                h = r("qkhF"),
                p = r("I9Fb"),
                b = r("Q7+W");
            Object(d.singleton)(b.a)(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [])(n = class extends p.a {
                constructor() {
                    super()
                }
                registerListener() {
                    u.default.on(h.b, this.listener), u.default.on(h.a, this.listener)
                }
            }) || n) || n);
            d.ModuleContainer.registerSingleton(l.a, class {
                get(e) {
                    const t = r("NDmK").default;
                    return s()(t, e)
                }
                set(e, t) {
                    const n = r("NDmK").default,
                        a = Object(i.a)(Object(i.a)({}, s()(n, e)), t);
                    return c()(n, e, a)
                }
                onConfigChange(e) {
                    throw new Error("method not implement")
                }
            })
        },
        xRjI: function(e, t, r) {
            "use strict";
            class n {
                static getInstance() {
                    return void 0 === n._instance && (n._instance = new n), n._instance
                }
                constructor() {
                    if (this.menuConditionCheckers = new Map, this.menuCommandHandlers = new Map, n._instance) throw new Error("Singleton classes can't be instantiated more than once.");
                    return n._instance = this, n._instance
                }
                getConditionChecker(e) {
                    return this._getConditionalChecker(e)
                }
                getCommandHandler(e) {
                    return this._getCommandHandler(e)
                }
                registerCondition(e, t) {
                    this._hasConditionalChecker(e), this._setConditionalChecker(e, t)
                }
                registerCommand(e, t) {
                    this._setCommandHandler(e, t)
                }
                _hasConditionalChecker(e) {
                    return this.menuConditionCheckers.has(e)
                }
                _getConditionalChecker(e) {
                    return this.menuConditionCheckers.get(e)
                }
                _getCommandHandler(e) {
                    return this.menuCommandHandlers.get(e)
                }
                _setConditionalChecker(e, t) {
                    this.menuConditionCheckers.set(e, t)
                }
                _setCommandHandler(e, t) {
                    this.menuCommandHandlers.set(e, t)
                }
            }
            n._instance = void 0;
            n.getInstance()
        },
        xir9: function(e, t, r) {
            "use strict";
            var n = r("jDHv"),
                i = r("fc/q"),
                a = r("PoHQ");
            n.ModuleContainer.registerSingleton(i.b, class {
                log(e) {
                    a.p.triggerEvent(a.i, [{
                        commandId: e.commandId,
                        success: !0 === e.success,
                        subCommandId: e.subCommandId || 0,
                        duration: e.duration || 0,
                        errorCode: e.errorCode || 0,
                        startTime: e.startTime || Date.now(),
                        params: e.params
                    }])
                }
            })
        },
        zgbJ: function(e, t, r) {
            "use strict";
            r.d(t, "a", (function() {
                return i
            })), r.d(t, "b", (function() {
                return a
            })), r.d(t, "c", (function() {
                return s
            }));
            var n = r("jDHv");
            const i = "o2o",
                a = "o2o/ui",
                s = Object(n.define)(i)
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js.map
