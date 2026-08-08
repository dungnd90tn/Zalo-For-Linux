__SCRIPT_TYPE__ = "renderer",
    function(e) {
        function t(t) {
            for (var r, o, n = t[0], c = t[1], l = t[2], u = 0, h = []; u < n.length; u++) o = n[u], Object.prototype.hasOwnProperty.call(a, o) && a[o] && h.push(a[o][0]), a[o] = 0;
            for (r in c) Object.prototype.hasOwnProperty.call(c, r) && (e[r] = c[r]);
            for (d && d(t); h.length;) h.shift()();
            return i.push.apply(i, l || []), s()
        }

        function s() {
            for (var e, t = 0; t < i.length; t++) {
                for (var s = i[t], r = !0, n = 1; n < s.length; n++) {
                    var c = s[n];
                    0 !== a[c] && (r = !1)
                }
                r && (i.splice(t--, 1), e = o(o.s = s[0]))
            }
            return e
        }
        var r = {},
            a = {
                37: 0
            },
            i = [];

        function o(t) {
            if (r[t]) return r[t].exports;
            var s = r[t] = {
                i: t,
                l: !1,
                exports: {}
            };
            return e[t].call(s.exports, s, s.exports, o), s.l = !0, s.exports
        }
        o.e = function(e) {
            var t = [],
                s = a[e];
            if (0 !== s)
                if (s) t.push(s[2]);
                else {
                    var r = new Promise((function(t, r) {
                        s = a[e] = [t, r]
                    }));
                    t.push(s[2] = r);
                    var i, n = document.createElement("script");
                    n.charset = "utf-8", n.timeout = 120, o.nc && n.setAttribute("nonce", o.nc), n.src = function(e) {
                        return o.p + "lazy/" + ({
                            23: "countries",
                            26: "lang-en",
                            27: "lang-vi"
                        } [e] || e) + "." + {
                            12: "7f974bf19beb8f117b97",
                            17: "8a9bbb19b4814c8ea0c9",
                            18: "2736ee958ea402bb72d6",
                            20: "df3a0f6e2eb4ead96894",
                            21: "29909e52f3b49df47145",
                            23: "efd555a8eb5da0f644de",
                            26: "374d11af84cd2f94b547",
                            27: "9589d4a6a0761d387abe",
                            43: "5b106fdb9bb6d76e530e",
                            44: "c66030df3d5a2cd2e64b",
                            45: "c04afeb2fe68ab777f6c",
                            47: "fe0ba758d7259dfd4a25",
                            48: "bcc959683f11d7790fd2",
                            58: "01c0b029a78881325741",
                            59: "87a8a857fdef256298f6",
                            60: "4db464ec8d1dbe548c78",
                            61: "b99f97b993ac3a272cf2",
                            62: "900c6d8533d08cc40278",
                            63: "7f0a6ae57e25540d0908",
                            64: "5b054957a1310ace92d2",
                            65: "e442b3426a232ac78365",
                            66: "8bffa656b4c3e86f751c",
                            67: "2f06a54159da1fc0c17b",
                            68: "b46edf1a4d2a01f2ef72",
                            69: "7339dd4f87c8bc5a2bdc"
                        } [e] + ".js"
                    }(e);
                    var c = new Error;
                    i = function(t) {
                        n.onerror = n.onload = null, clearTimeout(l);
                        var s = a[e];
                        if (0 !== s) {
                            if (s) {
                                var r = t && ("load" === t.type ? "missing" : t.type),
                                    i = t && t.target && t.target.src;
                                c.message = "Loading chunk " + e + " failed.\n(" + r + ": " + i + ")", c.name = "ChunkLoadError", c.type = r, c.request = i, s[1](c)
                            }
                            a[e] = void 0
                        }
                    };
                    var l = setTimeout((function() {
                        i({
                            type: "timeout",
                            target: n
                        })
                    }), 12e4);
                    n.onerror = n.onload = i, document.head.appendChild(n)
                } return Promise.all(t)
        }, o.m = e, o.c = r, o.d = function(e, t, s) {
            o.o(e, t) || Object.defineProperty(e, t, {
                enumerable: !0,
                get: s
            })
        }, o.r = function(e) {
            "undefined" != typeof Symbol && Symbol.toStringTag && Object.defineProperty(e, Symbol.toStringTag, {
                value: "Module"
            }), Object.defineProperty(e, "__esModule", {
                value: !0
            })
        }, o.t = function(e, t) {
            if (1 & t && (e = o(e)), 8 & t) return e;
            if (4 & t && "object" == typeof e && e && e.__esModule) return e;
            var s = Object.create(null);
            if (o.r(s), Object.defineProperty(s, "default", {
                    enumerable: !0,
                    value: e
                }), 2 & t && "string" != typeof e)
                for (var r in e) o.d(s, r, function(t) {
                    return e[t]
                }.bind(null, r));
            return s
        }, o.n = function(e) {
            var t = e && e.__esModule ? function() {
                return e.default
            } : function() {
                return e
            };
            return o.d(t, "a", t), t
        }, o.o = function(e, t) {
            return Object.prototype.hasOwnProperty.call(e, t)
        }, o.p = "", o.oe = function(e) {
            throw e
        };
        var n = this.webpackJsonp = this.webpackJsonp || [],
            c = n.push.bind(n);
        n.push = t, n = n.slice();
        for (var l = 0; l < n.length; l++) t(n[l]);
        var d = c;
        i.push([7, 0, 4, 5, 10, 9, 16, 1, 2, 3, 8, 7, 6, 15]), s()
    }({
        "39pz": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var r = s("jDHv");
            const a = Object(r.define)("resource-scanner-executor")
        },
        7: function(e, t, s) {
            e.exports = s("CdmT")
        },
        "8s2m": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            })), s.d(t, "b", (function() {
                return i
            }));
            var r = s("jDHv");
            let a;
            ! function(e) {
                e.DefaultInsertOptions = {
                    partition: void 0
                };
                let t = function(e) {
                    return e.Message = "message", e.Res = "res", e.Ref = "ref", e
                }({});
                e.TableName = t;
                let s = function(e) {
                    return e.Core = "Core", e.Res = "Res", e.Ref = "Ref", e
                }({});
                e.DBName = s;
                let r = function(e) {
                    return e.Message = "Core/message", e.Res = "Res/res", e.Ref = "Res/ref", e
                }({});
                e.TablePath = r
            }(a || (a = {}));
            const i = Object(r.define)("transfer-msg-database")
        },
        CdmT: function(e, t, s) {
            "use strict";
            s.r(t);
            s("iGh7"), s("TlRV"), s("dUtG"), s("v5OU"), s("jmmm"), s("GSxB");
            var r, a = s("VTBJ"),
                i = s("mwIZ"),
                o = s.n(i),
                n = s("D1y2"),
                c = s.n(n),
                l = s("jDHv"),
                d = s("Y58e"),
                u = s("I9Fb"),
                h = s("Q7+W"),
                g = s("Mgpg");
            Object(l.singleton)(h.a)(r = Reflect.metadata("design:type", Function)(r = Reflect.metadata("design:paramtypes", [])(r = class extends u.a {
                constructor() {
                    super()
                }
                registerListener() {
                    $zsharedWorker.onRemoteConfigChange(((e, t) => {
                        l.ModuleContainer.resolve(g.ZLoggerFactory).createZLogger(["remote-configs-cache"], ["shared-worker"]).zsymb(0, "Cc6F3K", "onRemoteConfigChange updated"), this.listener(t)
                    }))
                }
            }) || r) || r);
            l.ModuleContainer.resolve(h.a);
            l.ModuleContainer.registerSingleton(d.a, class {
                get(e) {
                    const t = s("NDmK").default;
                    return o()(t, e)
                }
                set(e, t) {
                    const r = s("NDmK").default,
                        i = Object(a.a)(Object(a.a)({}, o()(r, e)), t);
                    return c()(r, e, i)
                }
                onConfigChange(e) {
                    throw new Error("method not implement")
                }
            });
            s("EmOc"), s("ezdo"), s("KdAX"), s("mzek"), s("aBYf"), s("zm+Q"), s("WSI5"), s("v/rv"), s("nAZb"), s("c0KM"), s("zheM"), s("CXIs"), s("cZjg");
            var m = s("YrRS");
            Object(m.b)();
            s("33YB"), s("LyOm"), s("lNuO");
            var p = s("T0aS");
            class b extends p.b {
                constructor() {
                    super(), $zsub.$zsessionManager.onReceiveSession(((e, t) => {
                        this.setSession(t)
                    }))
                }
            }
            l.ModuleContainer.registerSingleton(p.a, b);
            s("0rWR"), s("Lq8m"), s("nUpV"), s("5yGw"), s("hRcX"), s("/2rj"), s("gpNb"), s("rhBN"), s("BP4/"), s("cF85"), s("7g9m"), s("oqw1");
            var f, y = s("8/YW"),
                I = s("PmZf"),
                S = s("tHMN"),
                v = s("jIg3"),
                M = s("8eps"),
                w = s("fsN4"),
                _ = s("PObO");
            let C = Object(y.h)()(f = l.ModuleContainer.injectable()(f = function(e, t) {
                return l.ModuleContainer.inject(S.b)(e, void 0, 0)
            }(f = function(e, t) {
                return l.ModuleContainer.inject(g.ZLoggerFactory)(e, void 0, 1)
            }(f = Reflect.metadata("design:type", Function)(f = Reflect.metadata("design:paramtypes", [void 0 === S.a ? Object : S.a, void 0 === g.ZLoggerFactory ? Object : g.ZLoggerFactory])(f = class extends v.a {
                constructor(e, t) {
                    super(), this.engine = e, this.logger = void 0, this.removeListeners = () => {}, this.logger = t.createZLogger("db", ["client"]), this.registerListeners()
                }
                registerListeners() {
                    const e = e => this.dispatchEvent(e),
                        t = [];
                    t.push(this.engine.addEventListener(I.b.UnexpectedError, e)), t.push(this.engine.addEventListener(I.b.QueryExec, e)), t.push(this.engine.addEventListener(I.b.QueryError, e)), t.push(this.engine.addEventListener(I.b.SuccessOpenDB, e)), t.push(this.engine.addEventListener(I.b.LongOpenDB, e)), t.push(this.engine.addEventListener(I.b.TimeConsumingQuery, e)), t.push(this.engine.addEventListener(I.b.ConnectionClosedAbnormally, e)), t.push(this.engine.addEventListener(I.b.SchemaMigratedAbnormally, e)), t.push(this.engine.addEventListener(I.b.Warning, e)), this.removeListeners = () => t.forEach((e => e()))
                }
                onDispose() {
                    this.removeListeners()
                }
                install() {
                    $zsub.$zdb.onReceiveSession(((e, t) => {
                        this.authenticate(t)
                    })), $zdb.notifyDbConnected(__ZaBUNDLENAME__), $zsub.$zdb.onRequestCloseDbs((async (e, t, s) => {
                        this.logger.zsymb(0, "DoqbHx", "closing databases"), await this.closeDBs(t, s), this.logger.zsymb(0, "eJEOCe", "closed databases"), $zdb.notifyFinishCloseDb(__ZaBUNDLENAME__)
                    })), this.logger.zsymb(0, "IuFI8V", "installed")
                }
                areYouOk() {
                    return !0
                }
                authenticate(e) {
                    if (e) {
                        this.session = e, this.dispatchEvent(new I.g(e)), this.engine.authenticate(e), this.logger.zsymb(0, "KiN-qJ", (() => ["authenticated", `id: ${e.userId}`]));
                        l.ModuleContainer.resolve(_.a).authenticate(e)
                    }
                }
                async closeDBs(e, t) {
                    const s = w.default.getInstance(),
                        r = this.logger;
                    let a = [];
                    if (e) {
                        if (e.length) {
                            let i = [...e];
                            r.zsymb(3, "UmSo7p", ["Start closing dbs: {}", "5iBscp"], e), r.zsymb(3, "j0MLPL", ["Wait for dbs to be closed: {}", "5hHScQ"], i), a = e.map((async e => {
                                let a = s[e];
                                if (void 0 === a) {
                                    const t = l.ModuleContainer.resolve(M.a).getValue().Meta;
                                    if (t.databaseName !== e) return void r.zsymb(21, "OEgRNu", ["Can't close invalid db: {}", "RjNGnT"], e);
                                    a = t
                                }
                                await a.closeThisDatabase(t), i = i.filter((t => t !== e)), 0 === i.length ? r.zsymb(0, "ZEryjv", "Done closing dbs") : r.zsymb(3, "wrygsE", ["Wait for dbs to be closed: {}", "5hHScQ"], i)
                            }))
                        }
                    } else a = [s.closeAllDatabases(t)];
                    await Promise.all(a)
                }
                checkShouldMigrate() {
                    throw new Error("Medthod not implement")
                }
                migrateData(e, t) {
                    throw new Error("Medthod not implement")
                }
            }) || f) || f) || f) || f) || f) || f;
            l.ModuleContainer.registerSingleton(v.b, C);
            s("6Pbj"), s("EicM"), s("m+NF"), s("pQlW");
            var E, T = s("UGJm"),
                O = s("hI9i"),
                R = s("q1tI"),
                D = s.n(R),
                k = s("/MKj"),
                F = s("ClHk");

            function z() {
                const {
                    data: e
                } = Object(O.m)(F.a, "all");
                return 0 === e.length ? D.a.createElement("div", null, "No Tasks") : D.a.createElement("table", null, D.a.createElement("thead", null, D.a.createElement("tr", null, D.a.createElement("th", null, "ID"), D.a.createElement("th", null, "Type"), D.a.createElement("th", null, "Status"), D.a.createElement("th", null, "Process"), D.a.createElement("th", null))), D.a.createElement("tbody", null, e.map((e => D.a.createElement(j, {
                    key: e,
                    id: e
                })))))
            }

            function j(e) {
                const t = Object(O.l)(F.a, e.id);
                return t ? D.a.createElement("tr", null, D.a.createElement("td", null, t.id), D.a.createElement("td", null, t.type), D.a.createElement("td", null, t.status), D.a.createElement("td", null, t.progress), D.a.createElement("td", null, new Date(t.startTime).toLocaleString())) : null
            }

            function N() {
                return D.a.createElement(k.a, {
                    store: O.b,
                    context: O.a
                }, D.a.createElement(z, null))
            }
            Object(l.injectable)()(E = Object(l.singleton)(y.a)(E = function(e, t) {
                return Object(l.inject)(d.a)(e, void 0, 0)
            }(E = function(e, t) {
                return Object(l.inject)(g.ZLoggerFactory)(e, void 0, 1)
            }(E = Reflect.metadata("design:type", Function)(E = Reflect.metadata("design:paramtypes", [void 0 === d.a ? Object : d.a, void 0 === g.ZLoggerFactory ? Object : g.ZLoggerFactory])(E = class extends T.a {
                constructor(e, t) {
                    super("zalo", e, t, {
                        component: N,
                        container: document.getElementById("app")
                    })
                }
            }) || E) || E) || E) || E) || E);
            s("KszJ"), s("Hbak"), s("dhzt"), s("O6t0");
            var L, A = s("OgkW"),
                P = s.n(A);
            const U = "PROGRESS",
                x = "CONV_SUCCESS",
                G = "ON_ERROR",
                $ = "ON_SUCCESS",
                B = Object(l.define)("resource-scanner-manager-event-emitter");
            let Q = Object(l.injectable)()(L = class {
                constructor() {
                    this._eventEmitter = new P.a
                }
                subscribe(e, t) {
                    this._eventEmitter.addListener(e, t)
                }
                emit(e, t) {
                    this._eventEmitter.emit(e, t)
                }
                omitAll(e) {
                    this._eventEmitter.removeAllListeners(e)
                }
                omit(e, t) {
                    this._eventEmitter.removeListener(e, t)
                }
            }) || L;
            l.ModuleContainer.registerSingleton(B, Q);
            var q = s("3jnX"),
                V = s("qUG6"),
                Z = s("19h1"),
                H = s("R5gT"),
                K = s("4prX"),
                Y = s("KP/S");
            class W extends q.b {
                constructor(e) {
                    super({
                        type: "REQUEST_NOISE_ID",
                        params: e
                    })
                }
            }
            class J extends q.b {
                constructor() {
                    super({
                        type: "REQUEST_ALL_NOISE_ID",
                        params: {}
                    })
                }
            }
            var X = s("Hy6w"),
                ee = s("h0S/");
            var te, se = new class {
                constructor() {
                    this._writeMsgs = new X.a("writeMsgs"), this._writeFileRes = new X.a("writeFileRes"), this._writeMedia = new X.a("writeMedia"), this._convertVersion = new X.a("convertVersion"), this._readSQL = new X.a("readSQL"), this._denoiseId = new X.a("denoiseId"), this._normalMsg = new X.a("normalMsg"), this._countMsg = new X.a("countMsg"), this._indexSearch = new X.a("indexSearch"), this._mediaRes = new X.a("mediaRes"), this._filterExists = new X.a("filterExists"), this._total = new X.a("totalRestoreMsg"), this.logger = void 0, this.writeMsgs = void 0, this.convertVersion = void 0, this.readSQL = void 0, this.denoiseId = void 0, this.writeFileRes = void 0, this.writeMedia = void 0, this.normalMsg = void 0, this.countMsg = void 0, this.indexSearch = void 0, this.mediaRes = void 0, this.filterExists = void 0, this.total = void 0, this.trackHolder = void 0, this.tracker = void 0, this.writeMsgs = this._writeMsgs.getTracker(), this.convertVersion = this._convertVersion.getTracker(), this.readSQL = this._readSQL.getTracker(), this.denoiseId = this._denoiseId.getTracker(), this.writeFileRes = this._writeFileRes.getTracker(), this.writeMedia = this._writeMedia.getTracker(), this.normalMsg = this._normalMsg.getTracker(), this.countMsg = this._countMsg.getTracker(), this.indexSearch = this._indexSearch.getTracker(), this.total = this._total.getTracker(), this.mediaRes = this._mediaRes.getTracker(), this.filterExists = this._filterExists.getTracker(), this.trackHolder = [this._writeMsgs, this._convertVersion, this._readSQL, this._denoiseId, this._writeFileRes, this._writeMedia, this._mediaRes, this._normalMsg, this._countMsg, this._indexSearch, this._filterExists, this._total], this.tracker = [this.writeMsgs, this.convertVersion, this.readSQL, this.denoiseId, this.writeFileRes, this.writeMedia, this.mediaRes, this.normalMsg, this.countMsg, this.indexSearch, this.filterExists, this.total], this.lock();
                    const e = l.ModuleContainer.resolve(g.ZLoggerFactory);
                    this.logger = e.createZLogger(ee.ZLoggerNametags.msgSync, [ee.ZLoggerNametags.syncMetrics])
                }
                lock() {
                    this.trackHolder.forEach((e => e.lock()))
                }
                unlock() {
                    this.trackHolder.forEach((e => e.unlock()))
                }
                reset() {
                    this.tracker.forEach((e => e.reset()))
                }
                print() {
                    let e = 0;
                    this.trackHolder.forEach((t => {
                        "totalRestoreMsg" !== t.label && (e += t.time)
                    })), this.logger.zsymb(0, "LB7hod", "ph7 total, track, untrack: ", this._total.time, e, this._total.time - e), this.trackHolder.forEach((e => {
                        this.logger.zsymb(0, "ruko5x", `ph7 ${e.info}`)
                    }))
                }
                getSnapShot() {
                    const e = {};
                    return this.trackHolder.forEach((t => {
                        e[t.label] = t.time
                    })), e
                }
            };
            let re = Object(l.singleton)()(te = Object(l.injectable)()(te = class {
                constructor() {
                    this.userId = "", this.cache = new Map
                }
                async init(e) {
                    this.userId !== e && (this.userId = e, this.cache = new Map, await this.tryToFetchCachedNoiseId())
                }
                async tryToFetchCachedNoiseId() {
                    try {
                        const e = new J,
                            t = await e.run();
                        t.length > 0 && (this.cache = new Map(t))
                    } catch (e) {}
                }
                get(e) {
                    if (e) return this.cache.get(e.toString())
                }
                async import(e) {
                    return se.denoiseId.start(), this._getNoiseId(e.uids || [], e.gids || []).then((e => (se.denoiseId.end(), e)))
                }
                async _getNoiseId(e, t) {
                    const s = e.filter((e => !this.cache.has(e) && Number.parseInt(e).toString() === e)),
                        r = t.filter((e => !this.cache.has(e) && Number.parseInt(e).toString() === e));
                    if (s.length !== e.length || r.length !== t.length) {
                        const a = e.length - s.length + t.length - r.length;
                        K.default.increaseFailed(Y.f.REQ_NOISE_ID, 1, a, Y.d.INVALID_NOISE_ID, Date.now())
                    }
                    await this._requestNoiseId(s, r)
                }
                _requestNoiseId(e, t) {
                    return new Promise(((s, r) => {
                        setTimeout((() => {
                            r("_requestNoiseId timeout")
                        }), 1e4);
                        new W({
                            uids: e,
                            gids: t
                        }).run().then((e => {
                            e.forEach((([e, t]) => {
                                this.cache.set(e, t)
                            })), s(!0)
                        })).catch(r)
                    }))
                }
            }) || te) || te;
            var ae = s("1pet"),
                ie = s("z0WU"),
                oe = s("/Bne"),
                ne = s("1qqm"),
                ce = s("8s2m");
            let le = new ne.b(ne.a);
            var de = s("fqRP"),
                ue = s("s2Ee"),
                he = s("PoHQ"),
                ge = s("bUXd"),
                me = s("EYv5"),
                pe = s("v6qY");
            const be = {
                shouldUseNewMediaDBConfig: 1
            };

            function fe() {
                return be.shouldUseNewMediaDBConfig
            }

            function ye(e) {
                be.shouldUseNewMediaDBConfig = e
            }
            var Ie, Se = s("NDmK"),
                ve = s("igA5"),
                Me = s("/YHU"),
                we = s("dsOb");
            let _e = Object(l.injectable)()(Ie = function(e, t) {
                return Object(l.inject)(g.ZLoggerFactory)(e, void 0, 0)
            }(Ie = function(e, t) {
                return Object(l.inject)(de.a)(e, void 0, 1)
            }(Ie = Reflect.metadata("design:type", Function)(Ie = Reflect.metadata("design:paramtypes", [void 0 === g.ZLoggerFactory ? Object : g.ZLoggerFactory, void 0 === de.a ? Object : de.a])(Ie = class {
                constructor(e, t) {
                    this.readyToInsertMsgsQueue = [], this.idStore = void 0, this.coreDB = null, this.resDB = null, this.ttl = void 0, this.logger = void 0, this.checkSum = new Set, this.checkSum2 = new Set, this.waitForNoiseId = [], this.noiseIdQueue = [], this.waitForGlobalId = [], this.lastInsertedMsgId = Date.now(), this.uid = "", this.idStore = l.ModuleContainer.resolveToken(re), this.logger = e.createZLogger("sync-message", ["msg-cross-importer"]), this.ttl = t
                }
                async open(e, t) {
                    await this.close(), this.uid = e, he.p.setSessionUserId(e), l.ModuleContainer.resolve(v.b).authenticate({
                        userId: e,
                        UIN: t
                    }), this.checkSum = new Set, this.checkSum2 = new Set;
                    const s = w.default.getInstance();
                    this.coreDB = s.Core, this.resDB = s.Res, this.waitForGlobalId = []
                }
                close() {
                    this.coreDB && (this.coreDB = null), this.resDB && (this.resDB = null)
                }
                queueNewMessages(e) {
                    let t = this.verifyCrossMsg(e);
                    this.readyToInsertMsgsQueue = this.readyToInsertMsgsQueue.concat(t.readyToInsert), this.waitForNoiseId = this.waitForNoiseId.concat(t.waitForNoiseId), this.noiseIdQueue = this.noiseIdQueue.concat(t.idsToGetNoise)
                }
                resetLastGlobalMsgId() {
                    this.lastInsertedMsgId = Date.now()
                }
                async processImportQueue(e) {
                    if ((e || this.shouldProcessMsgsInWaitForNoiseQueue()) && await this.processWaitForNoiseIdQueue(), (e && 0 === this.readyToInsertMsgsQueue.length || this.waitForGlobalId.length > 99) && this.waitForGlobalId.length > 0) {
                        const e = [];
                        this.processWaitForGlobalIdQueue(e), e.length > 0 && (this.queueNewMessages(e), await this.processWaitForNoiseIdQueue())
                    }
                    if (this.readyToInsertMsgsQueue.length) {
                        const e = this.readyToInsertMsgsQueue.splice(0, 2e3);
                        let s = Math.min(...e.map((e => e.sequenseId)));
                        s = Math.max(s, this.waitForGlobalId.length ? this.waitForGlobalId[0].sequenseId : 0, this.waitForNoiseId.length ? this.waitForNoiseId[0].sequenseId : 0);
                        try {
                            const t = [];
                            this.crossMsgsToNormalMsgs(e, t);
                            return {
                                inserted: (await this.insertToDb(t)).success,
                                minSeqId: s,
                                queued: e,
                                validMsgCount: t.length,
                                hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                            }
                        } catch (t) {
                            this.logger.zsymb(18, "9ekdWt", (() => ["processImportQueue error", {
                                error: t
                            }]))
                        }
                        return {
                            inserted: [],
                            minSeqId: s,
                            queued: e,
                            validMsgCount: e.length,
                            hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                        }
                    }
                    return {
                        inserted: [],
                        minSeqId: -1,
                        queued: [],
                        validMsgCount: 0,
                        hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                    }
                }
                printChecksum() {
                    this.logger.zsymb(12, "U4G6mi", (() => ["size", {
                        waitNoise: this.waitForNoiseId,
                        waitGlobal: this.waitForGlobalId,
                        ready: this.readyToInsertMsgsQueue
                    }])), this.checkSum.size > 0 ? this.logger.zsymb(18, "cFB0pQ", (() => ["checkSum.size > 0", {
                        miss: this.checkSum.values()
                    }])) : this.logger.zsymb(3, "WxUTHt", ["checkSum.size == 0", "U-Egcj"])
                }
                processWaitForGlobalIdQueue(e) {
                    const t = this.waitForGlobalId;
                    this.waitForGlobalId = [], this.lastInsertedMsgId = t[0].cliMsgId;
                    for (let s = t.length - 1; s >= 0; s--) {
                        const r = t[s];
                        r.globalMsgId = `${this.lastInsertedMsgId}_${s.toString().padStart(3,"0")}`, e.push(r)
                    }
                }
                async insertMsgs(e) {
                    e = e.filter((e => !!e.globalMsgId));
                    let t = this.verifyCrossMsg(e).idsToGetNoise;
                    t.length && await this.idStore.import({
                        uids: t
                    });
                    let s = [];
                    return this.crossMsgsToNormalMsgs(e, s), this.insertToDb(s)
                }
                async processWaitForNoiseIdQueue() {
                    if (0 === this.noiseIdQueue.length) return;
                    const e = this.noiseIdQueue,
                        t = this.waitForNoiseId;
                    this.waitForNoiseId = [], this.noiseIdQueue = [], await this.idStore.import({
                        uids: e
                    }), this.readyToInsertMsgsQueue.push(...t)
                }
                shouldProcessMsgsInWaitForNoiseQueue() {
                    return this.noiseIdQueue.length >= 1e3 || this.waitForNoiseId.length > 150
                }
                crossMsgsToNormalMsgs(e, t) {
                    se.normalMsg.start();
                    for (let r = 0; r < e.length; r++) {
                        const a = e[r];
                        try {
                            if ("number" == typeof a.globalMsgId && a.globalMsgId > 0 && (this.lastInsertedMsgId = a.globalMsgId, this.waitForGlobalId.length > 0)) {
                                const e = this.waitForGlobalId;
                                this.waitForGlobalId = [];
                                for (let s = e.length - 1; s >= 0; s--) {
                                    const r = e[s];
                                    r.globalMsgId = `${a.globalMsgId}_${s.toString().padStart(3,"0")}`, this.crossMsgToNormalMsg(r, t)
                                }
                            }
                            this.crossMsgToNormalMsg(a, t)
                        } catch (s) {}
                    }
                    se.normalMsg.end()
                }
                crossMsgToNormalMsg(e, t) {
                    if (!e.globalMsgId) return void this.waitForGlobalId.push(e);
                    const s = e.parsedType || ie.default.normalizeMessageType(e.type);
                    if (s === ae.MSG_UNKNOWN) return void 0;
                    const m = e => {
                            if ("string" != typeof e || !e) return e;
                            const t = e.indexOf("||");
                            if (t >= 0) {
                                const s = e.slice(t + 2).trim();
                                if (s && ("{" === s[0] || "[" === s[0])) return s
                            }
                            return e
                        },
                        y = e => {
                            if ("string" != typeof e || !e) return null;
                            const t = m(e);
                            if ("string" != typeof t || !t) return null;
                            try {
                                const e = JSON.parse(t);
                                return e && "object" == typeof e ? e : null
                            } catch {
                                return null
                            }
                        };
                    // Linux fallback: preserve raw ids when noise-id mapping is unavailable.
                    let r = e.parsedIdTo || this.idStore.get(e.ownerId) || e.ownerId,
                        a = e.parsedUidFrom || (e.fromId == e.userId ? "0" : (this.idStore.get(e.fromId) || e.fromId));
                    if (!a || !r) return void 0;
                    let i = m(e.msg),
                        o = e.attachData;
                    const c0 = y(i);
                    c0 && c0.data && "object" == typeof c0.data && (o.attach || (o.attach = {}), Object.assign(o.attach, c0.data));
                    switch (o.mentions && Array.isArray(o.mentions) && o.mentions.forEach((e => {
                            e.uid = "-1" === e.uid ? "-1" : this.idStore.get(e.uid)
                        })), o.quote && (o.quote.ownerId = this.idStore.get(o.quote.ownerId)), s) {
                        case ae.MSG_TEXT:
                            o && o.attach && o.attach.action && "rtf" == o.attach.action && (o.attach.title || (o.attach.title = e.msg), i = o.attach);
                            break;
                        case ae.MSG_PHOTO:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_STICKER:
                            i = o.attach || {};
                            break;
                        case ae.MSG_GIF:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_VOICE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.params = d(i.params), i.thumb = d(i.thumb), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_VIDEO:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.type = d(i.type);
                            break;
                        case ae.MSG_LOCATION:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.href = d(i.href), i.thumb = d(i.thumb), i.type = d(i.type);
                            break;
                        case ae.MSG_DOODLE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.thumb = d(i.thumb), i.title = d(i.title);
                            break;
                        case ae.MSG_CONTACT:
                            i = o.attach || {}, i.description = d(i.description), i.thumb = d(i.thumb);
                            const t = i.action;
                            "recommened.user" !== t && "recommened.vip" !== t || (i.params = this.idStore.get(i.params));
                            break;
                        case ae.MSG_FILE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.thumb = d(i.thumb), i.type = d(i.type);
                            break;
                        case ae.MSG_UNDO:
                            i = "";
                            break;
                        case ae.MSG_OA:
                        case ae.MSG_ECARD:
                        case ae.MSG_POLL:
                            i = o.attach;
                            break;
                        case ae.MSG_ZINSTANT_OLD:
                        case ae.MSG_ZINSTANT:
                            if (!o.attach || !o.attach.params || -1 == o.attach.params.indexOf("pcItem")) return void this.logger.zsymb(0, "V-Aj3W", "Skip invalid zinstant", null == e ? void 0 : e.globalMsgId);
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action, ""), i.description = d(i.description, ""), i.title = d(i.title, ""), i.href = d(i.href, ""), i.thumb = d(i.thumb, ""), i.params = d(i.params), delete i.childNumber
                    }
                    let n = null;
                    if ("chat.undo" == e.type && e.attach) try {
                        if (o.attach && o.attach.params) {
                            let t = JSON.parse(o.attach.params).custom_message;
                            if (t) {
                                t = JSON.parse(t);
                                const s = t.highLightsV2;
                                s.length && (n = s[0] && s[0].uid, n = this.idStore.get(n), n = n == this.uid ? "0" : n, e.type = "chat.delete.everyone")
                            }
                        }
                    } catch (u) {
                        this.logger.zsymb(18, "F-sKY5", (() => ["parseParams error:", {
                            error: u
                        }]))
                    }
                    const c = {
                        msgId: e.globalMsgId,
                        cliMsgId: e.cliMsgId,
                        msgType: e.type,
                        status: "0" == a ? ae.MSG_DELIVERED : ae.MSG_READ,
                        notify: "1",
                        mentions: o.mentions,
                        quote: o.quote,
                        content: i,
                        ts: e.ts.toString(),
                        uidFrom: a,
                        dName: e.fromName,
                        propertyExt: o.property,
                        ttl: e.ttl
                    };
                    o.reference && (c.reference = o.reference);
                    const l = oe.a.transformMessageFromServer(c, r);

                    function d(e, t = "") {
                        return e instanceof Object && Object.keys(e).length ? t : e || t
                    }
                    l.localDttm = Date.now(), l.src = ae.MSG_SRC.SYNC_MOBILE_DB, l.sequenceId = e.sequenseId, l.uidSenderDel = n;
                    const p = e.localPathRaw;
                    "string" == typeof p && p.length > 3 && /^[\x20-\x7E]+$/.test(p) && (l.localPath = p), l.msgType !== ae.MSG_UNKNOWN ? t.push(l) : this.logger.zsymb(0, "flow15", (() => ["skip message: unknown type", {
                        id: l.cliMsgId,
                        type: e.type
                    }]))
                }
                verifyCrossMsg(e) {
                    const t = [],
                        s = [],
                        r = new Set;
                    for (let n = 0; n < e.length; n++) {
                        var a, i;
                        const c = e[n];
                        if (!c) continue;
                        let l = !1;
                        const d = e => {
                            e && "-1" !== (e = e.toString()) && (this.idStore.get(e) || (l || (l = !0, t.push(c)), r.add(e)))
                        };
                        if (d(c.ownerId), d(c.fromId), !c.attachData) {
                            let e = {};
                            if (c.attach) try {
                                e = JSON.parse(c.attach)
                            } catch (o) {}
                            c.attachData = e
                        }
                        const u = c.attachData,
                            h = null === (a = u.attach) || void 0 === a ? void 0 : a.action;
                        "recommened.user" !== h && "recommened.vip" !== h || null === (i = u.attach) || void 0 === i || !i.params || d(u.attach.params), u.mentions && Array.isArray(u.mentions) && u.mentions.forEach((e => {
                            d(e.uid)
                        })), u.quote && d(u.quote.ownerId), l || s.push(c)
                    }
                    return {
                        waitForNoiseId: t,
                        readyToInsert: s,
                        idsToGetNoise: Array.from(r)
                    }
                }
                async insertToDb(e) {
                    e = await this._filterDeletedMessages(e);
                    const t = await this._insertMessage(e);
                    console.error("[SYNC] insertToDb(v1) input=", null == e ? void 0 : e.length, "ok=", null === (s = t.success) || void 0 === s ? void 0 : s.length, "fail=", null === (r = t.fail) || void 0 === r ? void 0 : r.length);
                    var s, r;
                    return se.mediaRes.start(), await Promise.all([this._insertMedia(t.success), this._insertFilesToResDB(t.success)]), se.mediaRes.end(), await this._insertTTL(t.success), this._sendMediaMsgToMain(t.success), Object(we.b)({
                        messages: e
                    }), t
                }
                _insertMedia(e) {
                    se.writeMedia.start();
                    const t = `${pe.c.TRANSFER_MOBILE}${pe.d.FROM_MSG}`;
                    return l.ModuleContainer.resolve(me.a).addMediasWhenTransferMessage(e, t, fe()).then((e => (se.writeMedia.end(), e)))
                }
                _insertFilesToResDB(e) {
                    se.writeFileRes.start();
                    const t = e.reduce(((e, t) => (t.msgType === ae.MSG_FILE && e.push({
                        msgId: t.msgId,
                        userId: t.toUid,
                        message: t.message,
                        sendDttm: t.sendDttm,
                        fromUid: t.fromUid,
                        type: "file",
                        cliMsgId: t.cliMsgId
                    }), e)), []);
                    return (async (e, t) => {
                        try {
                            let s = [],
                                r = [];
                            for (let e of t) {
                                const {
                                    res: t,
                                    ref: a,
                                    error: i
                                } = le.toDb(e);
                                i || (s.push(t), r.push(a))
                            }
                            s.length > 0 && await e.Res.insertMulti(s), r.length > 0 && await e.Ref.insertMulti(r)
                        } catch {}
                    })(this.resDB, t).then((e => (se.writeFileRes.end(), e)))
                }
                async _insertTTL(e) {
                    const t = e.filter((e => e.ttl && e.ttl > 0)),
                        s = ue.b.createFromUid(this.uid).deriveTTLItems(t);
                    if (t.length <= 0) return;
                    const r = await this.ttl.putMsgs(s);
                    r.ok ? this.logger.zsymb(3, "htUGGr", ["ttl insert success", "bz_uvF"]) : (this.logger.zsymb(21, "-U3mgr", ["ttl insert fail", "BHbgXG"]), this.logger.zsymb(3, "oA3sF-", ["ttl invalid {}, error {}", "bxRY5q"], r.error.invalidItems.length, r.error.errorItems.length))
                }
                async _insertMessage(e) {
                    se.writeMsgs.start();
                    const t = new Map;
                    e.forEach((e => {
                        t.has(e.toUid) ? t.get(e.toUid).push(e) : t.set(e.toUid, [e])
                    }));
                    const s = [];
                    return t.forEach(((e, t) => {
                        s.push(this.coreDB.Message.insertMulti(e, {
                            replace: !1,
                            partition: t
                        }))
                    })), Promise.all(s).then((e => (se.writeMsgs.end(), e.reduce(((e, t) => (e.success.push(...t.success), e.fail.push(...t.fail), e)), {
                        success: [],
                        fail: []
                    }))))
                }
                async _filterExistsMessages(e) {
                    if (!e || !e.length) return [];
                    const t = {};
                    e.forEach((({
                        msgId: e,
                        toUid: s
                    }) => {
                        void 0 === t[s] && (t[s] = []), t[s].push(e)
                    }));
                    const s = Object.entries(t).map((([e, t]) => this.coreDB.Message.getMultiIfExists(t, {
                            partition: e
                        }))),
                        r = await Promise.all(s).then((e => e.flat())),
                        a = new Set(r.map((e => e && e.msgId)));
                    return e.filter((e => !a.has(e.msgId)))
                }
                _sendMediaMsgToMain(e) {
                    if (!ie.default.isWeb()) {
                        const t = ve.default.getInstance().getItem(ae.KEY_CONFIG_TRANSFER_AUTODL),
                            {
                                validDays: s
                            } = t && Object(Me.a)(t),
                            r = 24 * (s || Se.default.auto_download_media_transfered.validDays) * 60 * 60 * 1e3,
                            a = e.filter((e => this._isMediaMsg(e) && !this._isAIStickerMessage(e) && !this._isPhotoStickerMessage(e) && this._isValidTime(e, r)));
                        Array.isArray(a) && a.length > 0 && $zFeatures.transfer.notifyReceiveMediaMsgs(a)
                    }
                }
                _isMediaMsg(e) {
                    var t;
                    const s = e.msgType;
                    if (s === ae.MSG_CONTACT && "recommened.link" === (null === (t = e.message) || void 0 === t ? void 0 : t.action)) return !0;
                    return [ae.MSG_PHOTO, ae.MSG_PHOTO_2, ae.MSG_DOODLE, ae.MSG_FILE, ae.MSG_VOICE, ae.MSG_VIDEO].includes(s)
                }
                _isAIStickerMessage(e) {
                    return !1
                }
                _isPhotoStickerMessage(e) {
                    return !1
                }
                _isValidTime(e, t) {
                    const s = +e.sendDttm;
                    return !(ge.default.getTimeNow() - s >= t)
                }
                async _filterDeletedMessages(e) {
                    let t = {},
                        s = [];
                    for (let i = 0; i < e.length; i++) {
                        const r = e[i];
                        r.msgType == ae.MSG_UNDO ? s.push(r) : (t[r.toUid] || (t[r.toUid] = []), t[r.toUid].push(r))
                    }
                    await this._sendUndo(s);
                    let r = Object.keys(t);
                    const a = [];
                    for (let i = 0; i < r.length; i++) {
                        const e = r[i];
                        a.push(...t[e])
                    }
                    return a
                }
                async _sendUndo(e) {}
            }) || Ie) || Ie) || Ie) || Ie) || Ie;
            class Ce {
                constructor(e) {
                    this.noiseIdStore = void 0, this.importer = void 0, this.session = null, this.logger = void 0, this._searchIndexQueue = null, this.noiseIdStore = l.ModuleContainer.resolveToken(re), this.importer = l.ModuleContainer.resolveToken(_e);
                    const t = l.ModuleContainer.resolve(g.ZLoggerFactory);
                    this.logger = t.createZLogger("msg-sync", ["importer", e])
                }
                async restoreConversations(e) {
                    const t = e.params.noiseUserId,
                        s = e.params.password,
                        r = this.doRestoreConversations.bind(this, e);
                    return this.usingDatabase(t, s, r)
                }
                async restoreMessages(e) {
                    const t = e.params.noiseUserId,
                        s = e.params.password,
                        r = this.doRestoreMessages.bind(this, e);
                    return this.usingDatabase(t, s, r)
                }
                async usingDatabase(e, t, s) {
                    let r, a;
                    await this.noiseIdStore.init(e), await this.open(e, t);
                    try {
                        r = await s()
                    } catch (i) {
                        a = i
                    }
                    if (await this.close(), a) throw a;
                    return r
                }
                async open(e, t) {
                    await this.importer.close(), this.session = {
                        userId: e,
                        UIN: t
                    }, await this.importer.open(e, t)
                }
                async close() {
                    await this.importer.close()
                }
                indexSearch(e, t) {
                    if (t.queueIndexSearch) return this.searchIndexQueue.queueIdxs(e);
                    return H.a.getInstance().executeMessageDbData(e)
                }
                get searchIndexQueue() {
                    var e, t, s, r;
                    this._searchIndexQueue || (null !== (e = this.session) && void 0 !== e && e.userId && null !== (t = this.session) && void 0 !== t && t.UIN && l.ModuleContainer.resolve(v.b).authenticate({
                        userId: null === (s = this.session) || void 0 === s ? void 0 : s.userId,
                        UIN: null === (r = this.session) || void 0 === r ? void 0 : r.UIN
                    }), this._searchIndexQueue = new Z.a);
                    return this._searchIndexQueue
                }
            }
            class Ee extends Ce {
                constructor() {
                    super("format-0"), this._backup = null, this._Logger = void 0
                }
                get Logger() {
                    return this._Logger || (this._Logger = l.ModuleContainer.resolve(g.ZLoggerFactory).createZLogger("sync-msg", ["import-format1"])), this._Logger
                }
                async doRestoreConversations(e) {
                    const {
                        params: t,
                        abort: s
                    } = e, r = await this.getBackup(t.backupPath), a = await r.getAllConversations(), i = await r.getRespondedConversations(), o = new Set(i.map((e => e.ownerId))), n = [], c = [];
                    if (a.forEach((e => {
                            1 === e.ownerType ? n.push(e.ownerId) : c.push(e.ownerId)
                        })), await this.noiseIdStore.import({
                            gids: n,
                            uids: c
                        }), s.aborted) throw new Error("aborted");
                    const l = await r.getLastMsgForEachConversations();
                    if (s.aborted) throw new Error("aborted");
                    let d = await this.importer.insertMsgs(l);
                    const u = {},
                        h = [],
                        g = [];
                    return a.forEach((e => {
                        const t = this.noiseIdStore.get(e.ownerId) || e.ownerId;
                        if (t) {
                            const s = 1 === e.ownerType,
                                r = {
                                    plainId: e.ownerId,
                                    noisedId: t,
                                    isGroup: s,
                                    respondedByMe: o.has(e.ownerId),
                                    backupPath: ""
                                };
                            h.push(r), u[t] = r
                        } else g.push(e.ownerId)
                    })), d.success.forEach((e => {
                        u[e.toUid] && (u[e.toUid].lastMessage = e)
                    })), d.fail.forEach((e => {
                        u[e.toUid] && (u[e.toUid].lastMessage = e)
                    })), h
                }
                async doRestoreMessages(e) {
                    const {
                        params: t,
                        checkpoint: s,
                        abort: r,
                        report: a
                    } = e;
                    if (!s || 0 !== s.format) throw new Error("invalid checkpoint");
                    const i = new Map(t.conversations.filter((e => !e.hasPreviewMsg)).map((e => [e.noiseId, void 0]))),
                        o = t.conversations.filter((e => e.shouldIgnore)).map((e => e.plainId));
                    let n = s.currentSeq,
                        c = s.maxSeq,
                        l = s.minSeq,
                        d = s.syncedConversations,
                        u = 0,
                        h = s.syncedMessages || 0;
                    const g = await this.getBackup(t.backupPath),
                        m = await g.countMessageFrom(l, o);
                    let p = await g.countMessageFrom(n, o),
                        b = m,
                        f = !1;
                    for (;;) {
                        if (r.aborted) return {
                            trackInfo: {}
                        };
                        let e = await g.getMessages(n, l, o, 100);
                        if (0 === e.length) b = 0;
                        else {
                            e[0].sequenseId > c && (c = e[0].sequenseId);
                            for (let t = e.length - 1; t >= 0; t--) {
                                const s = e[t].sequenseId;
                                if (s < n) {
                                    n = s;
                                    break
                                }
                            }
                            this.importer.queueNewMessages(e), b -= e.length
                        }
                        let s = await this.importer.processImportQueue(f);
                        if (0 === s.queued.length) {
                            if (s.hasMore) {
                                f = !0;
                                continue
                            }
                        } else f = !1;
                        if (0 === s.queued.length && 0 === b) {
                            (0 === l || l > n) && (l = n);
                            return a(100, {
                                format: 0,
                                minSeq: l,
                                maxSeq: c,
                                currentSeq: n,
                                syncedMessages: h,
                                syncedConversations: d
                            }), {
                                trackInfo: {}
                            }
                        }
                        let y = new Map;
                        for (let t = 0; t < s.inserted.length; t++) {
                            const e = s.inserted[t];
                            if (i.has(e.toUid)) {
                                const t = i.get(e.toUid);
                                (!t || t < e.sequenceId) && (y.set(e.toUid, e), i.set(e.toUid, e.sequenceId))
                            }
                        }
                        if (y.size > 0) {
                            h += y.size;
                            const e = new V.a(Array.from(y.values()));
                            await e.run()
                        }
                        p += s.validMsgCount;
                        const I = p / m,
                            S = Math.round(95 * I);
                        S !== u && (u = S, s.minSeqId > 0 && (this.Logger.zsymb(3, "X2SILg", ["checkpoint: {} {}/{} ({}%)", "niGhxq"], s.minSeqId, p, m, S), a(S, {
                            format: 0,
                            minSeq: l,
                            maxSeq: c,
                            currentSeq: n,
                            syncedMessages: h,
                            syncedConversations: d
                        }))), await this.indexSearch(s.inserted, {
                            queueIndexSearch: t.meta.queueIndexSearch
                        }), await new Promise((e => setTimeout(e, 50)))
                    }
                }
                async close() {
                    return this._backup && await this._backup.close(), super.close()
                }
                async getBackup(e) {
                    return this._backup || (this._backup = new Te(e), await this._backup.open()), this._backup
                }
            }
            class Te {
                constructor(e) {
                    this._db = void 0, this._db = $zsqlite.createConnection(e, {
                        OPEN_CREATE: !0,
                        OPEN_READWRITE: !0
                    })
                }
                close() {
                    return this._db.close()
                }
                async open() {
                    await this._db.open(), await this._db.runQuery(Re), await this._db.runQuery(De)
                }
                async getLastMsgForEachConversations() {
                    return this._db.getAllStatementResult(ke)
                }
                async getAllConversations() {
                    return (await this._db.getAllQuery(je)).filter((e => /^\d+$/.test(e.ownerId)))
                }
                async getRespondedConversations() {
                    return this._db.getAllQuery(Ne)
                }
                async getMessages(e, t, s, r) {
                    return this._db.getAllStatementResult(Fe, [e, t, s.join(","), r])
                }
                async countMessageFrom(e, t) {
                    const s = await this._db.getStatementResult(ze, [e, t.join(",")]);
                    return s ? s.count : 0
                }
            }
            const Oe = 'type != "chat.webcontent" AND type != "chat.livelocation" AND type != "chat.video.live.msg"',
                Re = 'CREATE INDEX IF NOT EXISTS "sequenseId" ON "chats" ("sequenseId" DESC)',
                De = 'CREATE INDEX IF NOT EXISTS "ts" ON "chats" ("ts" DESC)',
                ke = `SELECT chats.* FROM chats INNER JOIN (SELECT ownerId, MAX(ts) as maxTs from chats WHERE ${Oe} GROUP BY ownerId) AS latest ON chats.ts = latest.maxTs ORDER BY chats.ts DESC`,
                Fe = `SELECT * FROM chats WHERE sequenseId < (?) AND sequenseId > (?) AND cliMsgId NOT NULL AND ownerId NOT IN (?) AND ${Oe} ORDER BY sequenseId DESC LIMIT (?)`,
                ze = `SELECT count(*) as count FROM chats WHERE sequenseId > (?) AND cliMsgId NOT NULL AND ownerId NOT IN (?) AND ${Oe} ORDER BY sequenseId DESC`,
                je = "SELECT ownerId, ownerType, ts  FROM threads",
                Ne = "SELECT DISTINCT ownerId from chats WHERE userId = fromId";
            class Le extends q.b {
                constructor(e) {
                    super({
                        type: "RELOAD_MESSAGE",
                        params: e
                    })
                }
            }
            class Ae extends q.b {
                constructor(e) {
                    super({
                        type: "RELOAD_MEDIA",
                        params: e
                    })
                }
            }
            class Pe {
                constructor() {
                    this._searchIndexQueue = null, this.session = null
                }
                static instance() {
                    return this._instance || (this._instance = new this), this._instance
                }
                init(e) {
                    this.session = e
                }
                indexSearch(e, t) {
                    if (t.queueIndexSearch) return this.searchIndexQueue.queueIdxs(e);
                    return H.a.getInstance().executeMessageDbData(e)
                }
                get searchIndexQueue() {
                    var e, t, s, r;
                    this._searchIndexQueue || (null !== (e = this.session) && void 0 !== e && e.userId && null !== (t = this.session) && void 0 !== t && t.UIN && l.ModuleContainer.resolve(v.b).authenticate({
                        userId: null === (s = this.session) || void 0 === s ? void 0 : s.userId,
                        UIN: null === (r = this.session) || void 0 === r ? void 0 : r.UIN
                    }), this._searchIndexQueue = new Z.a);
                    return this._searchIndexQueue
                }
            }
            Pe._instance = void 0;
            class Ue {
                constructor() {
                    this.readyToInsertMsgsQueue = [], this.idStore = void 0, this.ttl = void 0, this.logger = void 0, this.database = void 0, this.checkSum = new Set, this.checkSum2 = new Set, this.waitForNoiseId = [], this.noiseIdQueue = [], this.waitForGlobalId = [], this.lastInsertedMsgId = Date.now(), this.uid = "";
                    const e = l.ModuleContainer.resolve(g.ZLoggerFactory);
                    this.idStore = l.ModuleContainer.resolveToken(re), this.logger = e.createZLogger("sync-message", ["msg-cross-importer"]), this.ttl = l.ModuleContainer.resolve(de.a), this.database = null
                }
                async open(e, t) {
                    await this.close(), this.uid = e, he.p.setSessionUserId(e), this.checkSum = new Set, this.checkSum2 = new Set, l.ModuleContainer.resolve(v.b).authenticate({
                        userId: e,
                        UIN: t
                    }), this.database = l.ModuleContainer.resolve(ce.b), this.database.authenticate({
                        userId: e,
                        UIN: t
                    }), this.waitForGlobalId = []
                }
                close() {
                    l.ModuleContainer.resolve(ce.b).close()
                }
                queueNewMessages(e) {
                    let t = this.verifyCrossMsg(e);
                    this.readyToInsertMsgsQueue = this.readyToInsertMsgsQueue.concat(t.readyToInsert), this.waitForNoiseId = this.waitForNoiseId.concat(t.waitForNoiseId), this.noiseIdQueue = this.noiseIdQueue.concat(t.idsToGetNoise)
                }
                resetLastGlobalMsgId() {
                    this.lastInsertedMsgId = Date.now()
                }
                async processImportQueue(e) {
                    if ((e || this.shouldProcessMsgsInWaitForNoiseQueue()) && await this.processWaitForNoiseIdQueue(), (e && 0 === this.readyToInsertMsgsQueue.length || this.waitForGlobalId.length > 99) && this.waitForGlobalId.length > 0) {
                        const e = [];
                        this.processWaitForGlobalIdQueue(e), e.length > 0 && (this.queueNewMessages(e), await this.processWaitForNoiseIdQueue())
                    }
                    if (this.readyToInsertMsgsQueue.length) {
                        const e = this.readyToInsertMsgsQueue.splice(0, 2e3);
                        let s = Math.min(...e.map((e => e.sequenseId)));
                        s = Math.max(s, this.waitForGlobalId.length ? this.waitForGlobalId[0].sequenseId : 0, this.waitForNoiseId.length ? this.waitForNoiseId[0].sequenseId : 0);
                        try {
                            const t = [];
                            this.crossMsgsToNormalMsgs(e, t);
                            return {
                                inserted: (await this.insertToDb(t)).success,
                                minSeqId: s,
                                queued: e,
                                validMsgCount: t.length,
                                hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                            }
                        } catch (t) {
                            this.logger.zsymb(18, "d122gg", (() => ["processImportQueue error", {
                                error: t
                            }]))
                        }
                        return {
                            inserted: [],
                            minSeqId: s,
                            queued: e,
                            validMsgCount: e.length,
                            hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                        }
                    }
                    return {
                        inserted: [],
                        minSeqId: -1,
                        queued: [],
                        validMsgCount: 0,
                        hasMore: this.waitForNoiseId.length > 0 || this.waitForGlobalId.length > 0
                    }
                }
                printChecksum() {
                    this.logger.zsymb(12, "TwBGG6", (() => ["size", {
                        waitNoise: this.waitForNoiseId,
                        waitGlobal: this.waitForGlobalId,
                        ready: this.readyToInsertMsgsQueue
                    }])), this.checkSum.size > 0 ? this.logger.zsymb(18, "5yQUTx", (() => ["checkSum.size > 0", {
                        miss: this.checkSum.values()
                    }])) : this.logger.zsymb(3, "247f_r", ["checkSum.size == 0", "U-Egcj"])
                }
                processWaitForGlobalIdQueue(e) {
                    const t = this.waitForGlobalId;
                    this.waitForGlobalId = [], this.lastInsertedMsgId = t[0].cliMsgId;
                    for (let s = t.length - 1; s >= 0; s--) {
                        const r = t[s];
                        r.globalMsgId = `${this.lastInsertedMsgId}_${s.toString().padStart(3,"0")}`, e.push(r)
                    }
                }
                async insertMsgs(e) {
                    e = e.filter((e => !!e.globalMsgId));
                    let t = this.verifyCrossMsg(e).idsToGetNoise;
                    t.length && await this.idStore.import({
                        uids: t
                    });
                    let s = [];
                    return this.crossMsgsToNormalMsgs(e, s), this.insertToDb(s)
                }
                async processWaitForNoiseIdQueue() {
                    if (0 === this.noiseIdQueue.length) return;
                    const e = this.noiseIdQueue,
                        t = this.waitForNoiseId;
                    this.waitForNoiseId = [], this.noiseIdQueue = [], await this.idStore.import({
                        uids: e
                    }), this.readyToInsertMsgsQueue.push(...t)
                }
                shouldProcessMsgsInWaitForNoiseQueue() {
                    return this.noiseIdQueue.length >= 1e3 || this.waitForNoiseId.length > 150
                }
                crossMsgsToNormalMsgs(e, t) {
                    se.normalMsg.start();
                    for (let r = 0; r < e.length; r++) {
                        const a = e[r];
                        try {
                            if ("number" == typeof a.globalMsgId && a.globalMsgId > 0 && (this.lastInsertedMsgId = a.globalMsgId, this.waitForGlobalId.length > 0)) {
                                const e = this.waitForGlobalId;
                                this.waitForGlobalId = [];
                                for (let s = e.length - 1; s >= 0; s--) {
                                    const r = e[s];
                                    r.globalMsgId = `${a.globalMsgId}_${s.toString().padStart(3,"0")}`, this.crossMsgToNormalMsg(r, t)
                                }
                            }
                            this.crossMsgToNormalMsg(a, t)
                        } catch (s) {}
                    }
                    se.normalMsg.end()
                }
                crossMsgToNormalMsg(e, t) {
                    if (!e.globalMsgId) return void this.waitForGlobalId.push(e);
                    const s = e.parsedType || ie.default.normalizeMessageType(e.type);
                    if (s === ae.MSG_UNKNOWN) return void 0;
                    // Linux fallback: preserve raw ids when noise-id mapping is unavailable.
                    let r = e.parsedIdTo || this.idStore.get(e.ownerId) || e.ownerId,
                        a = e.parsedUidFrom || (e.fromId == e.userId ? "0" : (this.idStore.get(e.fromId) || e.fromId));
                    if (!a || !r) return void 0;
                    let i = e.msg,
                        o = e.attachData;
                    switch (o.mentions && Array.isArray(o.mentions) && o.mentions.forEach((e => {
                            e.uid = "-1" === e.uid ? "-1" : this.idStore.get(e.uid)
                        })), o.quote && (o.quote.ownerId = this.idStore.get(o.quote.ownerId)), s) {
                        case ae.MSG_TEXT:
                            o && o.attach && o.attach.action && "rtf" == o.attach.action && (o.attach.title || (o.attach.title = e.msg), i = o.attach);
                            break;
                        case ae.MSG_PHOTO:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_STICKER:
                            i = o.attach || {};
                            break;
                        case ae.MSG_GIF:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_VOICE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.params = d(i.params), i.thumb = d(i.thumb), i.title = d(i.title), i.type = d(i.type);
                            break;
                        case ae.MSG_VIDEO:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.type = d(i.type);
                            break;
                        case ae.MSG_LOCATION:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.href = d(i.href), i.thumb = d(i.thumb), i.type = d(i.type);
                            break;
                        case ae.MSG_DOODLE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.thumb = d(i.thumb), i.title = d(i.title);
                            break;
                        case ae.MSG_CONTACT:
                            i = o.attach || {}, i.description = d(i.description), i.thumb = d(i.thumb);
                            const t = i.action;
                            "recommened.user" !== t && "recommened.vip" !== t || (i.params = this.idStore.get(i.params));
                            break;
                        case ae.MSG_FILE:
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action), i.description = d(i.description), i.thumb = d(i.thumb), i.type = d(i.type);
                            break;
                        case ae.MSG_UNDO:
                            i = "";
                            break;
                        case ae.MSG_OA:
                        case ae.MSG_ECARD:
                        case ae.MSG_POLL:
                            i = o.attach;
                            break;
                        case ae.MSG_ZINSTANT_OLD:
                        case ae.MSG_ZINSTANT:
                            if (!o.attach || !o.attach.params || -1 == o.attach.params.indexOf("pcItem")) return void this.logger.zsymb(0, "YKd3DA", "Skip invalid zinstant", null == e ? void 0 : e.globalMsgId);
                            i = o.attach || {}, i.childnumber = d(i.childnumber, 0), i.action = d(i.action, ""), i.description = d(i.description, ""), i.title = d(i.title, ""), i.href = d(i.href, ""), i.thumb = d(i.thumb, ""), i.params = d(i.params), delete i.childNumber;
                            break;
                        case ae.MSG_ZINSTANT_V2:
                            const s = o.attach || {};
                            i = {}, i.zinstant_msg = d(s.zinstantMsg, ""), i.zinstant_data = d(s.zinstantData, ""), i.action = d(s.action, "")
                    }
                    let n = null;
                    if ("chat.undo" == e.type && e.attach) try {
                        if (o.attach && o.attach.params) {
                            let t = JSON.parse(o.attach.params).custom_message;
                            if (t) {
                                t = JSON.parse(t);
                                const s = t.highLightsV2;
                                s.length && (n = s[0] && s[0].uid, n = this.idStore.get(n), n = n == this.uid ? "0" : n, e.type = "chat.delete.everyone")
                            }
                        }
                    } catch (u) {
                        this.logger.zsymb(18, "Qj6e96", (() => ["parseParams error:", {
                            error: u
                        }]))
                    }
                    const c = {
                        msgId: e.globalMsgId,
                        zglobalMsgId: -1,
                        cliMsgId: e.cliMsgId,
                        msgType: e.type,
                        status: "0" == a ? ae.MSG_DELIVERED : ae.MSG_READ,
                        notify: "1",
                        mentions: o.mentions,
                        quote: o.quote,
                        content: i,
                        ts: e.ts.toString(),
                        uidFrom: a,
                        dName: e.fromName,
                        propertyExt: o.property,
                        ttl: e.ttl
                    };
                    o.reference && (c.reference = o.reference);
                    const l = oe.a.transformMessageFromServer(c, r);

                    function d(e, t = "") {
                        return e instanceof Object && Object.keys(e).length ? t : e || t
                    }
                    l.localDttm = Date.now(), l.src = ae.MSG_SRC.SYNC_MOBILE_DB, l.sequenceId = e.sequenseId, l.uidSenderDel = n, l.msgType !== ae.MSG_UNKNOWN ? t.push(l) : this.logger.zsymb(0, "MB1Gac", (() => ["skip message: unknown type", {
                        id: l.cliMsgId,
                        type: e.type
                    }]))
                }
                verifyCrossMsg(e) {
                    const t = [],
                        s = [],
                        r = new Set;
                    for (let n = 0; n < e.length; n++) {
                        var a, i;
                        const c = e[n];
                        if (!c) continue;
                        let l = !1;
                        const d = e => {
                            e && "-1" !== (e = e.toString()) && (this.idStore.get(e) || (l || (l = !0, t.push(c)), r.add(e)))
                        };
                        if (d(c.ownerId), d(c.fromId), !c.attachData) {
                            let e = {};
                            if (c.attach) try {
                                e = JSON.parse(c.attach)
                            } catch (o) {}
                            c.attachData = e
                        }
                        const u = c.attachData,
                            h = null === (a = u.attach) || void 0 === a ? void 0 : a.action;
                        "recommened.user" !== h && "recommened.vip" !== h || null === (i = u.attach) || void 0 === i || !i.params || d(u.attach.params), u.mentions && Array.isArray(u.mentions) && u.mentions.forEach((e => {
                            d(e.uid)
                        })), u.quote && d(u.quote.ownerId), l || s.push(c)
                    }
                    return {
                        waitForNoiseId: t,
                        readyToInsert: s,
                        idsToGetNoise: Array.from(r)
                    }
                }
                async insertToDb(e) {
                    e = await this._filterDeletedMessages(e);
                    const t = await this._insertMessage(e);
                    console.error("[SYNC] insertToDb(v2) input=", null == e ? void 0 : e.length, "ok=", null === (s = t.success) || void 0 === s ? void 0 : s.length, "fail=", null === (r = t.fail) || void 0 === r ? void 0 : r.length);
                    var s, r;
                    return se.mediaRes.start(), await Promise.all([this._insertMedia(t.success), this._insertFilesToResDB(t.success)]), se.mediaRes.end(), await this._insertTTL(t.success), this._sendMediaMsgToMain(t.success), Object(we.b)({
                        messages: e
                    }), t
                }
                _insertMedia(e) {
                    se.writeMedia.start();
                    const t = `${pe.c.TRANSFER_MOBILE}${pe.d.FROM_MSG}`;
                    return l.ModuleContainer.resolve(me.a).addMediasWhenTransferMessage(e, t, fe()).then((e => (se.writeMedia.end(), e)))
                }
                _insertFilesToResDB(e) {
                    se.writeFileRes.start();
                    const t = e.reduce(((e, t) => (t.msgType === ae.MSG_FILE && e.push({
                        msgId: t.msgId,
                        userId: t.toUid,
                        message: t.message,
                        sendDttm: t.sendDttm,
                        fromUid: t.fromUid,
                        type: "file",
                        cliMsgId: t.cliMsgId
                    }), e)), []);
                    return (async (e, t) => {
                        try {
                            let s = [],
                                r = [];
                            for (let e of t) {
                                const {
                                    res: t,
                                    ref: a,
                                    error: i
                                } = le.toDb(e);
                                i || (s.push(t), r.push(a))
                            }
                            if (s.length > 0) {
                                const t = e.table(ce.a.TablePath.Res);
                                await t.insertMulti(s)
                            }
                            if (r.length > 0) {
                                const t = e.table(ce.a.TablePath.Ref);
                                await t.insertMulti(r)
                            }
                        } catch {}
                    })(this.database, t).then((e => (se.writeFileRes.end(), e)))
                }
                async _insertTTL(e) {
                    const t = e.filter((e => e.ttl && e.ttl > 0)),
                        s = ue.b.createFromUid(this.uid).deriveTTLItems(t);
                    if (t.length <= 0) return;
                    const r = await this.ttl.putMsgs(s);
                    r.ok ? this.logger.zsymb(3, "lHbTGp", ["ttl insert success", "bz_uvF"]) : (this.logger.zsymb(21, "uDxVTW", ["ttl insert fail", "BHbgXG"]), this.logger.zsymb(3, "zeJ9wt", ["ttl invalid {}, error {}", "bxRY5q"], r.error.invalidItems.length, r.error.errorItems.length))
                }
                async _insertMessage(e) {
                    se.writeMsgs.start();
                    const t = new Map;
                    e.forEach((e => {
                        t.has(e.toUid) ? t.get(e.toUid).push(e) : t.set(e.toUid, [e])
                    }));
                    const s = [],
                        r = this.database.table(ce.a.TablePath.Message);
                    return t.forEach(((e, t) => {
                        s.push(r.insertMulti(e, {
                            partition: t
                        }))
                    })), Promise.all(s).then((e => (se.writeMsgs.end(), e.reduce(((e, t) => (e.success.push(...t.success), e.fail.push(...t.fail), e)), {
                        success: [],
                        fail: []
                    })))).catch((e => {
                        throw this.logger.zsymb(18, "G3EWIU", "insert msg err", e), e
                    }))
                }
                _sendMediaMsgToMain(e) {
                    if (!ie.default.isWeb()) {
                        const t = ve.default.getInstance().getItem(ae.KEY_CONFIG_TRANSFER_AUTODL),
                            {
                                validDays: s
                            } = t && Object(Me.a)(t),
                            r = 24 * (s || Se.default.auto_download_media_transfered.validDays) * 60 * 60 * 1e3,
                            a = e.filter((e => this._isMediaMsg(e) && !this._isAIStickerMessage(e) && !this._isPhotoStickerMessage(e) && this._isValidTime(e, r)));
                        Array.isArray(a) && a.length > 0 && $zFeatures.transfer.notifyReceiveMediaMsgs(a)
                    }
                }
                _isMediaMsg(e) {
                    var t;
                    const s = e.msgType;
                    if (s === ae.MSG_CONTACT && "recommened.link" === (null === (t = e.message) || void 0 === t ? void 0 : t.action)) return !0;
                    return [ae.MSG_PHOTO, ae.MSG_PHOTO_2, ae.MSG_DOODLE, ae.MSG_FILE, ae.MSG_VOICE, ae.MSG_VIDEO].includes(s)
                }
                _isAIStickerMessage(e) {
                    return !1
                }
                _isPhotoStickerMessage(e) {
                    return !1
                }
                _isValidTime(e, t) {
                    const s = +e.sendDttm;
                    return !(ge.default.getTimeNow() - s >= t)
                }
                async _filterDeletedMessages(e) {
                    let t = {},
                        s = [];
                    for (let i = 0; i < e.length; i++) {
                        const r = e[i];
                        r.msgType == ae.MSG_UNDO ? s.push(r) : (t[r.toUid] || (t[r.toUid] = []), t[r.toUid].push(r))
                    }
                    await this._sendUndo(s);
                    let r = Object.keys(t);
                    const a = [];
                    for (let i = 0; i < r.length; i++) {
                        const e = r[i];
                        a.push(...t[e])
                    }
                    return a
                }
                async _sendUndo(e) {}
            }
            class xe {
                constructor(e, t, s, r, a) {
                    this.session = e, this.backup = t, this.conv = s, this.lastTimetemp = r, this.abort = a, this.logger = void 0, this.msgInserted = void 0;
                    const i = l.ModuleContainer.resolve(g.ZLoggerFactory);
                    this.logger = i.createZLogger("msg-sync", ["importerv2", "format-2"]), this.msgInserted = !1, this.lastTimetemp || (this.lastTimetemp = Number.MAX_SAFE_INTEGER)
                }
                async execute(e, t) {
                    this.logger.zsymb(0, "A2_Mc2", `restoring ${this.conv.plainId}, ${this.conv.noiseId}`);
                    try {
                        await this.doRestoreMessages(e, t)
                    } catch (s) {
                        this.logger.zsymb(18, "nwtzjO", (() => [`restore error: ${this.conv.plainId}`, {
                            error: s.message || s
                        }]))
                    }
                    this.msgInserted && this.reloadConversation(), this.logger.zsymb(0, "ijgIoT", `done restoring ${this.msgInserted} ${this.conv.noiseId}`)
                }
                async doRestoreMessages(e, t) {
                    const s = {
                            ownerId: this.conv.plainId,
                            ownerType: this.conv.isGroup ? 1 : 0
                        },
                        r = new Ue,
                        a = await this.backup.getBackupOfConversation(s);
                    if (!a) return void this.logger.zsymb(0, "qVOSVU", "backup not found, skip", s.ownerId);
                    const i = await a.getFirstMessageTimeStamp();
                    r.open(this.session.userId, this.session.UIN);
                    let o = 0,
                        n = !1,
                        c = 0,
                        l = !this.conv.hasPreviewMsg;
                    se.countMsg.start();
                    let d = await a.countMessages(this.lastTimetemp);
                    for (se.countMsg.end(), r.resetLastGlobalMsgId();;) {
                        if (this.abort.aborted) return;
                        if (d > 0) {
                            let e = await (null == a ? void 0 : a.getMessages(this.lastTimetemp, c, 2e3, s.ownerId));
                            if (0 === e.length) d = 0;
                            else {
                                c = e[e.length - 1].cliMsgId, (0 === o || e[0].ts > o) && (o = e[0].ts);
                                for (let t = e.length - 1; t >= 0; t--) {
                                    const s = e[t].ts;
                                    if (s < this.lastTimetemp) {
                                        this.lastTimetemp = s;
                                        break
                                    }
                                }
                                r.queueNewMessages(e), d -= e.length
                            }
                        }
                        let u = await r.processImportQueue(n);
                        if (0 === u.queued.length) {
                            if (u.hasMore) {
                                n = !0;
                                continue
                            }
                        } else n = !1;
                        if (this.msgInserted = this.msgInserted || u.inserted.length > 0, 0 === u.queued.length && d <= 0) return r.close(), void a.close();
                        l && u.inserted.length > 0 && (l = !1, this.reloadPreview(u)), se.indexSearch.start(), await Pe.instance().indexSearch(u.inserted, {
                            queueIndexSearch: e
                        }), se.indexSearch.end(), t(u.validMsgCount, u.inserted.length, this.lastTimetemp, i, o), await new Promise((e => setTimeout(e, 10)))
                    }
                }
                reloadConversation() {
                    new Le({
                        convId: this.conv.noiseId,
                        fromTs: 0,
                        toTs: 0
                    }).run(), new Ae({
                        convId: this.conv.noiseId
                    }).run()
                }
                reloadPreview(e) {
                    let t = e.inserted[0];
                    for (let s = 1; s < e.inserted.length; s++) {
                        const r = e.inserted[s];
                        r.sendDttm > t.sendDttm && (t = r)
                    }
                    new V.a([t]).run()
                }
            }
            class Ge {
                constructor(e) {
                    this.noiseIdStore = void 0, this.importer = void 0, this.session = null, this.logger = void 0, this.noiseIdStore = l.ModuleContainer.resolveToken(re), this.importer = l.ModuleContainer.resolveToken(Ue);
                    const t = l.ModuleContainer.resolve(g.ZLoggerFactory);
                    this.logger = t.createZLogger("msg-sync", ["importer", e])
                }
                async restoreConversations(e) {
                    const t = e.params.noiseUserId,
                        s = e.params.password,
                        r = this.doRestoreConversations.bind(this, e);
                    return Pe.instance().init(this.session), this.usingDatabase(t, s, r)
                }
                async restoreMessages(e) {
                    const t = e.params.noiseUserId,
                        s = e.params.password,
                        r = this.doRestoreMessages.bind(this, e);
                    return Pe.instance().init(this.session), this.usingDatabase(t, s, r)
                }
                async usingDatabase(e, t, s) {
                    let r, a;
                    await this.noiseIdStore.init(e), await this.open(e, t);
                    try {
                        r = await s()
                    } catch (i) {
                        a = i
                    }
                    if (await this.close(), a) throw a;
                    return r
                }
                async open(e, t) {
                    await this.importer.close(), this.session = {
                        userId: e,
                        UIN: t
                    }, await this.importer.open(e, t)
                }
                async close() {
                    await this.importer.close()
                }
            }
            let $e = function(e) {
                return e[e.Text = 0] = "Text", e[e.Doodle = 2] = "Doodle", e[e.Photo = 3] = "Photo", e[e.Photo_V2 = 4] = "Photo_V2", e[e.Voice = 6] = "Voice", e[e.Sticker = 10] = "Sticker", e[e.Contact = 12] = "Contact", e[e.OA = 15] = "OA", e[e.Location = 18] = "Location", e[e.Video = 19] = "Video", e[e.ECard = 21] = "ECard", e[e.File = 22] = "File", e[e.Gift = 23] = "Gift", e[e.Webcontent = 24] = "Webcontent", e[e.VideoLive = 25] = "VideoLive", e[e.PhotoV2 = 31] = "PhotoV2", e[e.ChatDelete = 33] = "ChatDelete", e[e.Undo = 36] = "Undo", e
            }({});
            const Be = {
                0: "webchat",
                2: "chat.doodle",
                3: "chat.photo",
                4: "chat.photo",
                6: "chat.voice",
                10: "chat.sticker",
                12: "chat.recommended",
                15: "chat.list.action",
                18: "chat.location.new",
                19: "chat.video.msg",
                20: "webchat",
                22: "share.file",
                23: "chat.gif",
                24: "chat.webcontent",
                53: "chat.webcontent.v2",
                25: "chat.video.live.msg",
                26: "group.poll",
                31: "chat.photo",
                33: "chat.delete",
                36: "chat.undo"
            };
            var Qe = s("nhJq");
            const qe = $znode.nativelibs.dbUtils(),
                Ve = $znode.path;
            class Ze {
                constructor(e) {
                    this.noiseUserId = void 0, this._db = void 0, this.plainUserId = void 0, this.logger = void 0, this.noiseId = void 0, this.backupConvId = "", this._db = $zsqlite.createConnection(e.path, {
                        OPEN_CREATE: !0,
                        OPEN_READWRITE: !0
                    }), this.logger = e.logger, this.noiseUserId = e.noiseUserId, this.plainUserId = e.plainUserId, this.noiseId = e.noiseId;
                    const t = /([g]?\d+)\.db$/i.exec(e.path || "");
                    t && t[1] && (this.backupConvId = t[1].replace(/^g/i, ""))
                }
                close() {
                    return this._db.close()
                }
                open() {
                    return this._db.open()
                }
                doWithLog(e, t, ...s) {
                    return t(...s).catch((t => {
                        throw this.logger.zsymb(18, "Wq3i7G", `backup: ${e}`, ...s, t), t
                    }))
                }
                async getFirstMessageTimeStamp() {
                    const e = await this.doWithLog("getFirstMessageTimeStamp", this._db.getAllQuery, Ye);
                    return 0 === e.length ? 0 : e[0].TimeStamp
                }
                async countMessages(e) {
                    const t = [e],
                        s = await this.doWithLog("countMessages", this._db.getStatementResult, Je, t);
                    return s ? s.count : 0
                }
                async getMessages(e, t, s, r) {
                    const a = [e, t, s];
                    se.readSQL.start();
                    const i = await this.doWithLog("getMessages", this._db.getAllStatementResult, We, a);
                    se.readSQL.end();
                    const o = [];
                    return se.convertVersion.start(), i.forEach((e => {
                        const t = this.convertCrossV2ToCrossV1(e);
                        t && o.push(t)
                    })), se.convertVersion.end(), this.logger.zsymb(0, "QaCYxb", `getMessages: ${r}, old:${i.length}, new:${o.length}`), o
                }
                async hasResponsedByUser(e) {
                    const t = await this.doWithLog("hasResponsedByUser", this._db.getQuery, Xe(e));
                    return !!t && t.count > 0
                }
                convertCrossV2ToCrossV1(e) {
                    const t = Be[e.MsgType] || "webchat";
                    // On Linux, sqlite bindings sometimes return BLOB columns as Uint8Array / ArrayBuffer views or even strings.
                    // Native parseBinNet requires a Node Buffer; if we pass the wrong type it silently produces empty attachData,
                    // and the app falls back to raw MsgContent (which becomes the "\u0019..." corruption you see in Media.db).
                    const _Buf = "undefined" != typeof Buffer && Buffer && Buffer.from ? Buffer : ("undefined" != typeof globalThis && globalThis.Buffer && globalThis.Buffer.from ? globalThis.Buffer : ("function" == typeof require ? (e => {
                        try {
                            return require("buffer").Buffer
                        } catch {
                            return null
                        }
                    })() : null));
                    let _bin = e.BinNet;
                    try {
                        if (_Buf && _bin && !_Buf.isBuffer(_bin)) {
                            // Cross-realm safe typed-array detection (instanceof can fail between Electron contexts)
                            if ("undefined" != typeof ArrayBuffer && ArrayBuffer.isView && ArrayBuffer.isView(_bin) && _bin.buffer) {
                                _bin = _Buf.from(new Uint8Array(_bin.buffer, _bin.byteOffset || 0, _bin.byteLength || _bin.length || 0));
                            } else if (_bin instanceof Uint8Array) _bin = _Buf.from(_bin);
                            else if (_bin && _bin.buffer && typeof _bin.byteLength === "number") _bin = _Buf.from(new Uint8Array(_bin.buffer, _bin.byteOffset || 0, _bin.byteLength));
                            else if (Array.isArray(_bin) && _bin.length && "number" == typeof _bin[0]) _bin = _Buf.from(_bin);
                            else if (_bin && "Buffer" === _bin.type && Array.isArray(_bin.data)) _bin = _Buf.from(_bin.data);
                            else if (_bin && Array.isArray(_bin.data) && _bin.data.length && "number" == typeof _bin.data[0]) _bin = _Buf.from(_bin.data);
                            else if (_bin && "number" == typeof _bin.length && _bin.length > 0 && "number" == typeof _bin[0]) {
                                // Some sqlite bindings return numeric-indexed objects with a length property.
                                const a = new Array(_bin.length);
                                for (let i = 0; i < _bin.length; i++) a[i] = _bin[i] & 255;
                                _bin = _Buf.from(a);
                            }
                            else if ("string" == typeof _bin) {
                                // Try a few common encodings:
                                // - hex string
                                // - base64 string
                                // - JSON string with \\u00XX escapes (double-escaped)
                                const b = _bin.trim();
                                if (/^[0-9a-fA-F]+$/.test(b) && b.length % 2 == 0) _bin = _Buf.from(b, "hex");
                                else if (/^[A-Za-z0-9+/]+={0,2}$/.test(b) && b.length >= 16) _bin = _Buf.from(b, "base64");
                                else if (b.includes("\\u00")) {
                                    const unesc = b.replace(/\\u([0-9a-fA-F]{4})/g, ((_, h) => String.fromCharCode(parseInt(h, 16))));
                                    _bin = _Buf.from(unesc, "binary");
                                } else _bin = _Buf.from(b, "binary");
                            }
                        }
                    } catch {}
                    const s = qe.parseBinNet(_bin) || {};
                    let r = s.data || {};
                    const n = e => {
                        if (null === e || void 0 === e) return "";
                        const t = String(e);
                        // Backup filenames/tables keep group id as `g<id>`, while runtime conversation ids are `<id>`.
                        // If we keep `g` here, many restored messages fail with "Conversation not found".
                        return /^g\d+$/.test(t) ? t.slice(1) : t
                    };
                    // Some backup variants don't keep sender/owner in the same columns.
                    // Pick robust fallbacks to avoid writing invalid fromUid/toUid (which breaks media fetch auth).
                    const a = [e.SenderId, e.FromId, e.fromId, e.FromUid, e.fromUid, r.fromD, r.uid, r.ownerId].find((e => null !== e && void 0 !== e && "" !== e));
                    const i = [e.OwnerId, e.ownerId, e.ToId, e.toId, e.ReceiverId, e.ConversationId, this.backupConvId, this.noiseId].find((e => null !== e && void 0 !== e && "" !== e));
                    const o = null !== a && void 0 !== a ? a : this.noiseId;
                    // Linux fallback: keep message import alive even when BinNet parser is incomplete.
                    // We still preserve msg text and core metadata so conversations can populate.
                    s.result > 0 && (this.logger.zsymb(21, "C2HNDS", ["ParseBinNetFail", "0EO2Al"], {
                        message: s.error_message,
                        result: s.result,
                        innerError: s.inner_error,
                        msgType: e.MsgType,
                        cliMsgId: e.CliMsgId
                    }), r = {});
                    this._debugCrossV2ShapePrinted || (this._debugCrossV2ShapePrinted = !0, console.error("[SYNC] CrossV2 row shape sample " + JSON.stringify({
                        keys: Object.keys(e || {}),
                        binNet: {
                            type: typeof e.BinNet,
                            hasBuffer: !!_Buf,
                            isBuffer: _Buf && _Buf.isBuffer ? _Buf.isBuffer(e.BinNet) : !1,
                            coercedIsBuffer: _Buf && _Buf.isBuffer ? _Buf.isBuffer(_bin) : !1,
                            len: e.BinNet && (e.BinNet.length || e.BinNet.byteLength || 0),
                            coercedLen: _bin && (_bin.length || _bin.byteLength || 0),
                            ctor: e.BinNet && e.BinNet.constructor ? e.BinNet.constructor.name : "",
                            keys0: e.BinNet && "object" == typeof e.BinNet ? Object.keys(e.BinNet).slice(0, 8) : []
                        },
                        parseBinNet: {
                            result: s.result,
                            inner_error: s.inner_error,
                            error_message: s.error_message,
                            dataKeys0: r && "object" == typeof r ? Object.keys(r).slice(0, 12) : [],
                            attachsLen: r && r.attachs && r.attachs.length ? r.attachs.length : 0,
                            attach0Keys0: r && r.attachs && r.attachs[0] && "object" == typeof r.attachs[0] ? Object.keys(r.attachs[0]).slice(0, 16) : [],
                            debug: s && s.debug ? {
                                inputLen: s.debug.inputLen,
                                tlvEndian: s.debug.tlvEndian,
                                topKeyCount: s.debug.topKeyCount,
                                topKeys0: s.debug.topKeys && s.debug.topKeys.length ? s.debug.topKeys.slice(0, 16) : [],
                                attachCount: s.debug.attachCount,
                                attachCandidateCount: s.debug.attachCandidateCount,
                                attachCandidates0: s.debug.attachCandidates && s.debug.attachCandidates.length ? s.debug.attachCandidates.slice(0, 6) : []
                            } : {}
                        },
                        senderCandidates: {
                            SenderId: e.SenderId,
                            FromId: e.FromId,
                            fromId: e.fromId,
                            FromUid: e.FromUid,
                            fromUid: e.fromUid,
                            fromD: r.fromD
                        },
                        ownerCandidates: {
                            OwnerId: e.OwnerId,
                            ownerId: e.ownerId,
                            ToId: e.ToId,
                            ReceiverId: e.ReceiverId,
                            ConversationId: e.ConversationId,
                            backupConvId: this.backupConvId,
                            fallbackNoiseId: this.noiseId
                        },
                        chosen: {
                            fromIdRaw: o,
                            fromIdNormalized: n(o),
                            ownerIdRaw: i,
                            ownerIdNormalized: n(i)
                        }
                    })));
                    return r.attachs && r.attachs.length > 0 && r.attachs[0] && (e.MsgType === $e.OA ? r.attach = r.attachs : (r.attach = r.attachs[0], e.MsgType !== $e.Sticker && r.attach && delete r.attach.catId)), {
                        fromId: n(o),
                        fromName: "",
                        attach: "{}",
                        attachData: r,
                        globalMsgId: e.GlbMsgId,
                        cliMsgId: e.CliMsgId,
                        msg: e.MsgContent,
                        ownerId: n(i),
                        ownerType: 0,
                        sequenseId: e.TimeStamp,
                        ts: e.TimeStamp,
                        ttl: e.TTL,
                        localPathRaw: e.LocalPath,
                        type: t,
                        userId: this.plainUserId
                    }
                }
            }
            class He {
                constructor(e, t, s, r) {
                    this.path = e, this.plainUserId = t, this.noiseUserId = s, this.logger = r, this._responsedByMe = new Set
                }
                async open() {}
                async close() {}
                async getAllConversations() {
                    const e = [],
                        t = await Qe.c(this.path);
                    for (const s of t)
                        if (s.endsWith(".db")) {
                            const t = "g" === s[0] ? 1 : 0,
                                r = t ? 6 : 0,
                                a = s.length - 3,
                                i = s.substring(r, a);
                            e.push({
                                ownerId: i,
                                ownerType: t
                            })
                        } return e
                }
                async getMetaDataConversations(e) {
                    const t = {},
                        s = Date.now();
                    for (const a of e) {
                        const e = await this.getBackupOfConversation(a);
                        if (e) try {
                            const r = await e.getMessages(s, 0, 1, a.ownerId),
                                i = await e.countMessages(s);
                            if (r.length > 0) {
                                t[a.ownerId] = {
                                    msg: r[0],
                                    numOfMsg: i
                                };
                                await e.hasResponsedByUser(this.plainUserId) && this._responsedByMe.add(a.ownerId)
                            }
                        } catch (r) {
                            if (this.logger.zsymb(18, "tAckvs", `read conv failure: ${r.message}`), 11 != r.errno && !r.message.includes("SQLITE_CORRUPT")) throw r
                        } finally {
                            e.close()
                        }
                    }
                    return t
                }
                getRespondedByMeSet() {
                    return this._responsedByMe
                }
                async getBackupOfConversation(e) {
                    return this._getNewBackupOfConversation(e)
                }
                async _getNewBackupOfConversation(e) {
                    const t = e.ownerId,
                        s = 1 === e.ownerType ? `group_${t}` : t,
                        r = Ve.join(this.path, `${s}.db`),
                        a = new Ze({
                            path: r,
                            noiseId: t,
                            noiseUserId: this.noiseUserId,
                            plainUserId: this.plainUserId,
                            logger: this.logger
                        });
                    return await a.open(), a
                }
            }
            const Ke = [20, 21, 25, 26, 29, 32, 34, 35, 45, 51, 52].map((e => `MsgType != ${e}`)).join(" AND "),
                Ye = "SELECT TimeStamp FROM ChatContent ORDER BY TimeStamp ASC LIMIT 1",
                We = `SELECT * FROM ChatContent WHERE TimeStamp <= (?) AND MsgStatus > 0 AND CliMsgId NOT NULL AND CliMsgId != (?) AND ${Ke} ORDER BY TimeStamp DESC LIMIT (?)`,
                Je = `SELECT count(*) as count FROM ChatContent WHERE TimeStamp <= (?) AND MsgStatus > 0 AND cliMsgId NOT NULL AND ${Ke}`,
                Xe = e => `SELECT count(*) as count FROM ChatContent WHERE SenderId = ${e} AND MsgStatus > 0 AND ${Ke}`;
            var et = s("cfFl");
            class tt extends Ge {
                constructor() {
                    super("format-1"), this._backup = null
                }
                async doRestoreConversations(e) {
                    Pe.instance().searchIndexQueue.pause();
                    const {
                        params: t,
                        abort: s
                    } = e, r = await this._getBackup(t.backupPath, t.plainUserId, t.noiseUserId);
                    let a = await r.getAllConversations();
                    const i = [],
                        o = [];
                    if (a.forEach((e => {
                            1 === e.ownerType ? i.push(e.ownerId) : o.push(e.ownerId)
                        })), await this.noiseIdStore.import({
                            gids: i,
                            uids: o
                        }), s.aborted) throw new Error("aborted");
                    const n = await r.getMetaDataConversations(a),
                        c = r.getRespondedByMeSet();
                    if (s.aborted) throw new Error("aborted");
                    a = a.filter((e => n[e.ownerId] && n[e.ownerId].numOfMsg > 0));
                    const l = a.map((e => n[e.ownerId].msg)),
                        d = await this.importer.insertMsgs(l),
                        u = {},
                        h = [],
                        g = [];
                    return a.forEach((e => {
                        const t = this.noiseIdStore.get(e.ownerId) || e.ownerId;
                        if (t) {
                            const s = 1 === e.ownerType,
                                r = {
                                    plainId: e.ownerId,
                                    noisedId: t,
                                    isGroup: s,
                                    respondedByMe: c.has(e.ownerId)
                                };
                            h.push(r), u[t] = r
                        } else g.push(e.ownerId)
                    })), d.success.forEach((e => {
                        u[e.toUid] && (u[e.toUid].lastMessage = e)
                    })), d.fail.forEach((e => {
                        u[e.toUid] && (u[e.toUid].lastMessage = e)
                    })), await Pe.instance().indexSearch(d.success, {
                        queueIndexSearch: t.meta.queueIndexSearch
                    }), h
                }
                doRestoreMessages(e) {
                    return Pe.instance().searchIndexQueue.pause(), se.reset(), se.unlock(), se.total.start(), new Promise((async t => {
                        const {
                            params: s,
                            checkpoint: r,
                            abort: a,
                            report: i
                        } = e, o = new Set;
                        if (!r || 1 !== r.format) throw new Error("invalid checkpoint");
                        let n = s.conversations.filter((e => !e.shouldIgnore)),
                            c = s.meta.viewingConversation;
                        const l = () => {
                            if (c) {
                                const t = (e = c, n.find((t => t.noiseId === e)));
                                if (this.logger.zsymb(0, "RHUv3X", `getViewingConversation ${c} ${JSON.stringify(t)}`), t) return t
                            }
                            var e;
                            return null
                        };
                        let d = r.priorities;

                        function u() {
                            if (d.length > 0) {
                                const e = l();
                                let t;
                                if (e && !o.has(e.plainId)) c = null, t = e;
                                else {
                                    const e = d.find((e => !o.has(e)));
                                    t = n.find((t => t.plainId === e))
                                }
                                return t
                            }
                            return n[0]
                        }
                        e.onParamsChange((e => {
                            e && e.meta && e.meta.viewingConversation && (this.logger.zsymb(0, "h24oWb", `viewing conversation changed to ${e.meta.viewingConversation}`), c = e.meta.viewingConversation)
                        }));
                        const h = () => Math.floor(r.syncedMessages / s.meta.numberOfMessages * 95),
                            g = await this._getBackup(s.backupPath, s.plainUserId, s.noiseUserId);
                        let m;
                        const p = () => {
                                r.syncedConversations = s.meta.numberOfConversations, i(100, r), se.total.end(), se.print(), o.clear(), t({
                                    trackInfo: se.getSnapShot()
                                })
                            },
                            b = () => {
                                if (d.length > n.length) {
                                    this.logger.zsymb(0, "CTruQc", "detect invalid priority conv " + (n.length - d.length));
                                    const e = new Set(n.map((e => e.plainId))),
                                        t = [];
                                    for (let s = 0; s < d.length; s++) e.has(d[s]) && t.push(d[s]);
                                    d = t, r.priorities = t
                                }
                            },
                            f = () => {
                                if (d.length <= 0 || a.aborted) return void this.logger.zsymb(0, "NOeSkB", "done restore: ", d.length, a.aborted);
                                const e = u();
                                e ? (o.add(e.plainId), m.push(e)) : b()
                            };
                        m = Object(et.queue)((async e => {
                            r.syncingConversation = e.noiseId, i(h(), r);
                            const t = new xe(this.session, g, e, r.currentSeq[e.plainId], a);
                            var o;
                            await t.execute(s.meta.queueIndexSearch, ((t, s, a, o, n) => {
                                r.syncedMessages += t, r.importedMessages += s, r.currentSeq[e.plainId] = a, 0 === r.maxSeq ? r.maxSeq = n : r.maxSeq = Math.max(r.maxSeq, n), 0 === r.minSeq ? r.minSeq = o : r.minSeq = Math.min(r.minSeq, o), i(h(), r)
                            })), delete r.currentSeq[e.plainId], t.msgInserted && (r.updatedConversation++, i(h(), r)), o = e, d = d.filter((e => e !== o.plainId)), r.priorities = d.filter((e => e !== o.plainId)), n = n.filter((e => e.noiseId !== o.noiseId)), f()
                        }), 5), b(), u() ? m.drain = p : p();
                        for (let e = 0; e <= 5; e++) f()
                    }))
                }
                async close() {
                    return this._backup && await this._backup.close(), super.close()
                }
                async _getBackup(e, t, s) {
                    return this._backup || (this._backup = new He(e, t, s, this.logger), await this._backup.open()), this._backup
                }
            }

            function st(e) {
                try {
                    if (0 === e) return new Ee;
                    if (1 === e) return new tt
                } catch (t) {
                    throw t
                }
                throw new Error(`Unsupported format: ${e}`)
            }
            var rt;
            Object(y.j)()(rt = Object(l.injectable)()(rt = class extends q.a {
                getType() {
                    return "RESTORE_CONVERSATIONS"
                }
                async execute(e) {
                    if (e.abort.aborted) return Promise.reject(new Error("Aborted"));
                    const t = st(e.params.format);
                    return console.error("[SYNC] RESTORE_CONVERSATIONS start format=", e.params.format), ye(e.params.shouldUseNewMediaDBFlowConfig), t.restoreConversations(e).then((t => (console.error("[SYNC] RESTORE_CONVERSATIONS done count=", null != t && t.length ? t.length : 0), t)))
                }
            }) || rt);
            var at;
            Object(y.j)()(at = Object(l.injectable)()(at = class extends q.a {
                getType() {
                    return "RESTORE_MESSAGES"
                }
                async execute(e) {
                    if (e.abort.aborted) return Promise.reject(new Error("Aborted"));
                    const t = st(e.params.format);
                    return console.error("[SYNC] RESTORE_MESSAGES start format=", e.params.format), ye(e.params.shouldUseNewMediaDBFlowConfig), t.restoreMessages(e).then((t => (console.error("[SYNC] RESTORE_MESSAGES done"), t)))
                }
            }) || at);
            var it;
            const ot = $znode.nativelibs.dbUtils();
            Object(y.j)()(it = Object(l.injectable)()(it = class extends q.a {
                getType() {
                    return "DECRYPT_BACKUP"
                }
                async execute(e) {
                    return e.abort.aborted ? Promise.reject(new Error("Aborted")) : this._decyptBackup(e)
                }
                async _decyptBackup(e) {
                    const {
                        params: t,
                        report: s
                    } = e;
                    console.error("[SYNC] _decyptBackup: format="+t.format+" input="+t.inputPath+" output="+t.outputPath+" convCount="+t.numberOfConversationsCount+" key="+(t.privateKey||""));
                    return await Object(Qe.b)(t.outputPath), 0 === t.format ? this._decryptBackupFormat0(t.inputPath, t.outputPath, t.privateKey) : this._decryptBackupFormat1(t.inputPath, t.outputPath, t.privateKey, t.numberOfConversationsCount, s)
                }
                async _decryptBackupFormat0(e, t, s) {
                    const {
                        result: r,
                        err_message: a
                    } = ot.decompressAndDecryptDb(e, t, s);
                    console.error("FORMAT0 decrypt result:", r, "err:", a, "input:", e, "output:", t, "key:", s);
                    return 0 === r ? t : Promise.reject({
                        error: r,
                        message: a
                    })
                }
                async _decryptBackupFormat1(e, t, s, r, a) {
                    t = await Object(Qe.a)(t);
                    try {
                        const dirList = await Qe.c(t);
                        console.error("[SYNC] decrypt output dir=", t, "entries=", null == dirList ? void 0 : dirList.length, "sample=", (dirList || []).slice(0, 10));
                    } catch (err) {
                        console.error("[SYNC] decrypt output dir inspect failed:", t, err && (err.message || err));
                    }
                    let i = 0;
                    const {
                        result: o,
                        inner_error: n,
                        error_message: c
                    } = ot.decompressAndDecryptDb_V2(e, t, s.toUpperCase(), (() => {
                        i++;
                        const e = Math.floor(Math.round(i / r * 100));
                        a(e)
                    }));
                    console.error("FORMAT1 decrypt V2 result:", o, "inner:", n, "msg:", c);
                    return 0 === o ? t : Promise.reject({
                        error: o,
                        inner_error: n,
                        message: c
                    })
                }
            }) || it);
            var nt = s("Jz/+"),
                ct = s("d/or"),
                lt = s("YEoC");
            l.ModuleContainer.resolve(p.a).addEventListener(nt.a.SessionChange, (async ({
                session: e
            }) => {
                if (null === e) return;
                const t = l.ModuleContainer.resolve(ct.a);
                await t.getAdapterTypeOfUserScopedDatabases(e.userId) == lt.a.IDB ? s.e(61).then(s.bind(null, "AYQT")) : s.e(62).then(s.bind(null, "PKoC"))
            }));
            s("/d1d"), s("Ceyj");
            var dt = s("pAGP"),
                ut = s("wH4e"),
                ht = s("QiE6"),
                gt = s("h1h5"),
                mt = s("KkZn"),
                pt = s("y4y9"),
                bt = s("q1L6"),
                ft = s("dm9D"),
                yt = s("RYIJ"),
                It = s("tdV2"),
                St = s("Q8nW"),
                vt = s("ZChn"),
                Mt = s("Pwr1");
            class wt {
                constructor(e) {
                    this.data = void 0, this.typeId = void 0, this.data = new Map, this.typeId = e
                }
                addItem(e, t) {
                    this.data.set(e, {
                        path: e,
                        id: t
                    })
                }
                removeItem(e) {
                    this.data.delete(e)
                }
                getItems() {
                    return Array.from(this.data.values())
                }
                getItem(e) {
                    return this.data.get(e)
                }
                hasItem(e) {
                    return this.data.has(e)
                }
                get size() {
                    return this.data.size
                }
                clear() {
                    this.data.clear()
                }
                getTypeId() {
                    return this.typeId
                }
            }
            var _t = s("IpzU");
            const Ct = "renderer" === __SCRIPT_TYPE__ ? $znode.path : s("ipWf");
            class Et {
                constructor() {
                    this.paths = void 0, this.isInititalized = void 0, this.paths = {}, this.isInititalized = !1
                }
                static getInstance() {
                    return this.instance || (this.instance = new Et), this.instance
                }
                async init(e) {
                    this.paths.zaloDownloadsDir = await _t.getUserZaloDownloadsDir(e), this.paths.zaloResourceDir = await _t.getMediaResourceDir(e), this.isInititalized = !0
                }
                getUserZaloDownloadsDir() {
                    return this.paths.zaloDownloadsDir
                }
                getZaloDownloadsVideoDir(e) {
                    return e.startsWith(ae.GROUPID_PREFIX) ? Ct.join(this.paths.zaloDownloadsDir, "video", "group", Ct.sep) : Ct.join(this.paths.zaloDownloadsDir, "video", Ct.sep)
                }
                getZaloDownloadsVoiceDir(e) {
                    return e.startsWith(ae.GROUPID_PREFIX) ? Ct.join(this.paths.zaloDownloadsDir, "voice", "group", Ct.sep) : Ct.join(this.paths.zaloDownloadsDir, "voice", Ct.sep)
                }
                getZaloDownloadsRichThumbDir(e) {
                    return e.startsWith(ae.GROUPID_PREFIX) ? Ct.join(this.paths.zaloDownloadsDir, "richThumb", "group", Ct.sep) : Ct.join(this.paths.zaloDownloadsDir, "richThumb", Ct.sep)
                }
                getZaloDownloadsPictureDir(e) {
                    let t = e;
                    return t.startsWith(ae.GROUPID_PREFIX) && (t = t.slice(1) + "_group"), Ct.join(this.paths.zaloDownloadsDir, "picture", t, Ct.sep)
                }
                getZaloDownloadsFileDir(e) {
                    let t = "";
                    return e.startsWith(ae.GROUPID_PREFIX) && (t = "group"), Ct.join(this.paths.zaloDownloadsDir, "file", t, Ct.sep)
                }
                getZaloDownloadsFileNoiseDir(e) {
                    let t = "";
                    return e.startsWith(ae.GROUPID_PREFIX) && (t = "group"), Ct.join(this.paths.zaloDownloadsDir, "fileNoise", t, Ct.sep)
                }
                getZaloDownloadsFileThumbDir() {
                    return Ct.join(this.paths.zaloDownloadsDir, "fileThumb", Ct.sep)
                }
                getZaloDownloadsZInstantDir() {
                    return Ct.join(this.paths.zaloDownloadsDir, "zinstant", Ct.sep)
                }
                getZaloDownloadFolderDir() {
                    return Ct.join(this.paths.zaloDownloadsDir, "folder", Ct.sep)
                }
                getZaloDownloadsResourceVideoDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "video", Ct.sep)
                }
                getZaloDownloadsResourceVoiceDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "voice", Ct.sep)
                }
                getZaloDownloadsResourceRichThumbDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "richThumb", Ct.sep)
                }
                getZaloDownloadsResourcePictureDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "picture", Ct.sep)
                }
                getZaloDownloadsResourceFileDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "file", Ct.sep)
                }
                getZaloDownloadsResourceFileNoiseDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "fileNoise", Ct.sep)
                }
                getZaloDownloadsResourceFileThumbDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "fileThumb", Ct.sep)
                }
                getZaloDownloadResourceFolderDir(e) {
                    return Ct.join(this.paths.zaloResourceDir, e, "folder", Ct.sep)
                }
            }
            Et.instance = void 0;
            var Tt = Et,
                Ot = s("AqnK"),
                Rt = s("QmNg"),
                Dt = s("tmLI"),
                kt = s("sANj"),
                Ft = s("f7u2"),
                zt = s("MbQE"),
                jt = s("iqN/"),
                Nt = s("Uzj0"),
                Lt = s("RoG4");
            class At extends gt.a {
                constructor(e) {
                    super(), this.convId = void 0, this.userId = void 0, this.types = void 0, this.data = void 0, this.status = void 0, this.getResourcePaths = async (e, t, s) => {
                        const r = [],
                            a = Tt.getInstance(),
                            i = Object(jt.a)();
                        if (!i) throw new Error("ResourceScanner: sessionUserId is not available");
                        const o = await Object(Ft.v)(this.convId, i);
                        switch (e) {
                            case ae.MSG_VOICE: {
                                const e = await Object(Ft.B)(i, this.convId);
                                r.push(...t.map((t => [Object(zt.b)(t, {
                                    fallbackName: () => $znode.path.join(e, t.msgId)
                                }), t.msgId])));
                                break
                            }
                            case ae.MSG_VIDEO: {
                                const e = await Object(Ft.z)(i, this.convId);
                                r.push(...t.map((t => [Object(zt.b)(t, {
                                    fallbackName: () => $znode.path.join(e, t.msgId)
                                }), t.msgId])));
                                break
                            }
                            case ae.MSG_CONTACT: {
                                const e = await Object(Ft.x)(i, this.convId);
                                t.forEach((t => {
                                    if (Object(ft.v)(t) && t.message.thumb) {
                                        const s = t.message.thumb;
                                        r.push([Object(zt.b)(t, {
                                            fallbackName: () => $znode.path.join(e, `z${t.msgId}_${Object(mt.d)(s)}${Object(mt.b)(s)||".jpg"}`),
                                            extension: () => Object(mt.b)(s) || ".jpg"
                                        }), t.msgId], [Object(zt.b)(t, {
                                            fallbackName: () => $znode.path.join(e, `z_${Object(mt.d)(s)}${Object(mt.b)(s)||".jpg"}`),
                                            extension: () => Object(mt.b)(s) || ".jpg"
                                        }), t.msgId])
                                    }
                                }));
                                break
                            }
                            case ae.MSG_PHOTO:
                                for (const e of t) {
                                    let t = !1;
                                    if (e.message) {
                                        const a = [e.message.oriUrl, e.message.hdUrl, e.message.normalUrl, e.message.thumbUrl];
                                        for (const n of a)
                                            if (n) {
                                                const a = Object(zt.f)({
                                                        cliMsgId: e.cliMsgId,
                                                        fromUid: e.fromUid,
                                                        toUid: e.toUid,
                                                        urlSrc: n
                                                    }, {
                                                        fallbackName: () => `z${e.msgId}_${Object(mt.e)(n)}`
                                                    }),
                                                    c = $znode.path.join(o, a);
                                                if (r.push([c, e.msgId]), !t && St.i(c) && (t = !0), s.includeImageGen) {
                                                    const t = Object(zt.f)({
                                                        cliMsgId: e.cliMsgId,
                                                        fromUid: e.fromUid,
                                                        toUid: e.toUid,
                                                        urlSrc: n
                                                    }, {
                                                        fallbackName: () => `z${e.msgId}_${Object(mt.e)(n,".jpg")}`,
                                                        extension: ".jpg"
                                                    });
                                                    r.push([$znode.path.join(o, t), e.msgId]);
                                                    for (let s of Object.values(Ot.o)) {
                                                        const t = await Object(Ft.g)(i),
                                                            a = await Rt.b.ZaloDownload_Photo(t, this.convId, Object(Rt.d)({
                                                                cliMsgId: e.cliMsgId,
                                                                fromUid: e.fromUid,
                                                                toUid: this.convId
                                                            }), s);
                                                        r.push([a, e.msgId])
                                                    }
                                                }
                                            }
                                    }!e.localPath || t && !s.includeImageGen || r.push([e.localPath, e.msgId])
                                }
                                break;
                            case ae.MSG_DOODLE:
                                for (const e of t) {
                                    const t = e.message.oriUrl,
                                        s = Object(zt.f)({
                                            cliMsgId: e.cliMsgId,
                                            fromUid: e.fromUid,
                                            toUid: e.toUid,
                                            urlSrc: t
                                        }, {
                                            fallbackName: () => `z${e.msgId}_${Object(mt.e)(t)}`
                                        }),
                                        a = $znode.path.join(o, s);
                                    r.push([a, e.msgId])
                                }
                                break;
                            case ae.MSG_FILE: {
                                const e = await Object(Ft.l)(this.convId, i),
                                    a = await Object(Ft.p)(this.convId, i),
                                    o = await Object(Ft.z)(i, this.convId);
                                for (const c of t) {
                                    var n;
                                    const t = Object(Mt.b)(c.message),
                                        l = Object(Mt.c)(c);
                                    if ((Object(Dt.n)(null == c || null === (n = c.message) || void 0 === n ? void 0 : n.params) || "").fType === kt.FileMsgType.Folder && s.zaloTempNewPos) {
                                        const e = await Object(Ft.a)(i, this.convId),
                                            s = Object(zt.b)(c, {
                                                fallbackName: () => $znode.path.join(e, t)
                                            });
                                        r.push([s, c.msgId])
                                    }
                                    r.push([Object(zt.b)(c, {
                                        fallbackName: () => $znode.path.join(e, t)
                                    }), c.msgId]), r.push([Object(zt.b)(c, {
                                        fallbackName: () => $znode.path.join(a, t)
                                    }), c.msgId]), bt.a.getGenType(l) && r.push([Object(zt.b)(c, {
                                        fallbackName: () => pt.fHelper.getPhotoThumbCache(c, t)
                                    }), c.msgId]), (c.subType === ae.MSG_SUBTYPE_MEDIA_VIDEO || l in It.a.VideoType) && r.push([Object(zt.b)(c, {
                                        fallbackName: () => $znode.path.join(o, c.msgId)
                                    }), c.msgId])
                                }
                                break
                            }
                            case ae.MSG_ZINSTANT:
                            case ae.MSG_ZINSTANT_V2:
                            case ae.MSG_ZINSTANT_OLD: {
                                const e = a.getZaloDownloadsZInstantDir();
                                await Nt.b.asyncForEach(t, (async t => {
                                    const s = Lt.h.create(new Lt.h.Adapters.MessageEntity(t)),
                                        a = await s.getZInstantData(),
                                        i = yt.a.getFileNameFromUrl(a.data_url);
                                    i && r.push([$znode.path.join(e, i), t.msgId])
                                }));
                                break
                            }
                        }
                        if (t.forEach((({
                                localPath: t,
                                msgId: s
                            }) => {
                                t && e !== ae.MSG_PHOTO && r.push([t, s])
                            })), r.length > 0) {
                            const t = this.data.get(e) || new wt(e);
                            r.forEach((([e, s]) => t.addItem(e, s))), this.data.set(e, t)
                        }
                    }, this.convId = e.convId, this.userId = e.userId, this.types = e.msgTypes, this.data = new Map, this.status = new Map
                }
                async onScanSuccess(e) {
                    this.status.set(e, "done"), this.types.every((e => "done" === this.status.get(e))) && await this.dispatchEvent("success", {
                        data: this.data,
                        key: this.convId
                    })
                }
                async onScanProgress(e, t, s) {
                    await this.dispatchEvent("progress", {
                        type: e,
                        key: this.convId,
                        data: this.data.get(e),
                        metadata: {
                            fromDttm: t,
                            toDttm: s
                        }
                    })
                }
                async onScanError(e) {
                    this.status.set(e, "failed"), await this.dispatchEvent("error", {
                        data: e,
                        key: this.convId
                    })
                }
                async scanMessages(e, t, s) {
                    return new Promise(((r, a) => {
                        const i = w.default.getInstance(),
                            [o, n] = t,
                            {
                                getConfig: c,
                                abort: l,
                                partitioning: d
                            } = s,
                            u = {
                                limit: c().batchSize,
                                partition: this.convId,
                                aborted: () => !(!l || !l.aborted)
                            };
                        (d ? i.Core.Message.index("msgType_sendDttm").getAll({
                            from: [e, String(o)],
                            to: [e, String(n)],
                            excludeTo: !0,
                            excludeFrom: !1
                        }, u) : i.Core.Message.index("userId_msgType_sendDttm").getAll({
                            from: [this.convId, e, String(o)],
                            to: [this.convId, e, String(n)],
                            excludeTo: !0,
                            excludeFrom: !1
                        }, u)).then((t => {
                            if (t.length > 0) {
                                const i = Number(t[t.length - 1].sendDttm);
                                this.getResourcePaths(e, t, c()).then((() => this.onScanProgress(e, o, i))).then((() => {
                                    setTimeout((() => {
                                        l && l.aborted ? a("aborted") : this.scanMessages(e, [i + 1, n], s).then(r, a)
                                    }), c().batchDelay)
                                })).catch(a)
                            } else this.onScanSuccess(e).then(r, a)
                        })).catch((t => {
                            vt.a.getInstance().zsymb(18, "v0BYWc", `fail scan for msgType=${e} convId=${this.convId}`, t), this.onScanError(e), a(t)
                        }))
                    }))
                }
                async run(e) {
                    const {
                        getConfig: t,
                        getRange: s,
                        abort: r,
                        partitioning: a
                    } = e, i = Tt.getInstance();
                    i.isInititalized || await i.init(this.userId);
                    const o = [];
                    for (const n of this.types) "running" !== this.status.get(n) && (this.status.set(n, "running"), o.push(this.scanMessages(n, s ? s(this.convId, n) : [0, Date.now()], {
                        getConfig: t,
                        abort: r,
                        partitioning: a
                    })));
                    await Promise.all(o)
                }
                flush() {
                    this.data.clear(), this.status.clear()
                }
                get key() {
                    return this.convId
                }
            }
            var Pt, Ut = At;
            Object(l.singleton)(ht.ResourceScannerExecutor)(Pt = Reflect.metadata("design:type", Function)(Pt = Reflect.metadata("design:paramtypes", [])(Pt = class extends gt.a {
                constructor() {
                    super(), this.scanners = void 0, this.currentChunkIndex = void 0, this.chunks = void 0, this.scanners = new Map, this.currentChunkIndex = -1, this.chunks = []
                }
                getScanner(e, t) {
                    let s = this.scanners.get(t);
                    return s || (s = new Ut({
                        convId: t,
                        userId: e,
                        msgTypes: ht.RESOURCE_LINKED_MSG_TYPES
                    }), this.scanners.set(t, s)), s
                }
                executeChunk(e) {
                    return new Promise(((t, s) => {
                        if (this.currentChunkIndex < 0 || this.currentChunkIndex >= this.chunks.length) return t();
                        const r = this.chunks[this.currentChunkIndex],
                            a = performance.now();
                        Promise.all(r.map((t => this.getScanner(e.userId, t).run({
                            getConfig: e.getConfig,
                            getRange: e.getRange,
                            abort: e.abort,
                            partitioning: e.partitioning
                        })))).then((() => {
                            this.dispatchEvent("batching", {
                                duration: performance.now() - a,
                                convIds: r
                            }), this.currentChunkIndex += 1, this.executeChunk(e).then(t, s)
                        })).catch((e => {
                            this.dispatchEvent("batching", {
                                duration: performance.now() - a,
                                convIds: r,
                                error: e
                            }), ht.ResourceScannerLog.getInstance().zsymb(18, "_AzviF", "execute fail at chunk", this.currentChunkIndex, r, e), s(e)
                        }))
                    }))
                }
                init(e, t) {
                    t.forEach((t => this.getScanner(e, t)))
                }
                async run(e) {
                    const {
                        convIds: t,
                        getConfig: s,
                        userId: r
                    } = e;
                    this.chunks = Object(Mt.a)(t, s().batchConv), this.currentChunkIndex = 0;
                    let i = e.partitioning;
                    if (!i) {
                        i = await l.ModuleContainer.resolve(dt.a).getAdapterTypeOfUserScopedDatabases(r) === ut.a.SQLite
                    }
                    await this.executeChunk(Object(a.a)(Object(a.a)({}, e), {}, {
                        partitioning: i
                    }))
                }
            }) || Pt) || Pt);
            var xt, Gt = class {
                constructor() {
                    this.startTs = void 0, this.lastTrackTs = void 0, this.startTs = -1, this.lastTrackTs = -1
                }
                start() {
                    this.startTs = performance.now(), this.lastTrackTs = this.startTs, vt.a.getInstance().zsymb(0, "BxytEh", "#start scan")
                }
                track(e, ...t) {
                    const s = performance.now(),
                        r = s - this.lastTrackTs;
                    this.lastTrackTs = s, vt.a.getInstance().zsymb(0, "LPue37", `#track ${e} duration=${r}`, ...t)
                }
                end(e, ...t) {
                    const s = performance.now() - this.startTs;
                    e ? vt.a.getInstance().zsymb(0, "Xib9cR", "#end scan status=success", `duration=${s}`, ...t) : vt.a.getInstance().zsymb(18, "JQ1HGk", "#end scan status=fail", `duration=${s}`, ...t)
                }
                cancel() {
                    const e = performance.now() - this.startTs;
                    vt.a.getInstance().zsymb(6, "5nfJbA", "#cancel scan", `duration=${e}`)
                }
            };
            Object(l.injectable)()(xt = Object(l.singleton)(ht.ResourceScannerManager)(xt = function(e, t) {
                return Object(l.inject)(ht.ResourceScannerExecutor)(e, void 0, 0)
            }(xt = Reflect.metadata("design:type", Function)(xt = Reflect.metadata("design:paramtypes", [void 0 === ht.ResourceScannerExecutor ? Object : ht.ResourceScannerExecutor])(xt = class extends gt.a {
                constructor(e) {
                    super(), this.executor = e, this.tracking = void 0, this.onConversationScanning = async e => {
                        await this.dispatchEvent("progress", e)
                    }, this.onConversationScanned = async e => {
                        const {
                            key: t,
                            data: s
                        } = e;
                        await this.dispatchEvent("success-item", [t, s])
                    }, this.onConversationsScanned = async e => {
                        await this.dispatchEvent("batching", e)
                    }, this.tracking = new Gt
                }
                async getAllConversations() {
                    const e = w.default.getInstance();
                    return (await e.Core.Conversation.getAll()).map((({
                        userId: e
                    }) => e)).filter(Boolean)
                }
                async run(e) {
                    const {
                        abort: t,
                        convIds: s,
                        getConfig: r,
                        getRange: a,
                        userId: i
                    } = e, o = new Map;
                    try {
                        this.tracking.start();
                        let e = s;
                        e || (e = await this.getAllConversations(), this.tracking.track("get conversations", e.length)), this.executor.init(i, e), this.executor.scanners.forEach((e => {
                            e.addEventListener("progress", this.onConversationScanning), e.addEventListener("success", this.onConversationScanned)
                        })), this.executor.addEventListener("batching", this.onConversationsScanned), await this.executor.run({
                            convIds: e,
                            getConfig: r,
                            getRange: a,
                            abort: t,
                            userId: i
                        }), this.executor.scanners.forEach(((e, t) => {
                            o.set(t, e.data)
                        })), await this.dispatchEvent("success", o), this.tracking.end(!0)
                    } catch (n) {
                        throw this.dispatchEvent("error", n), this.tracking.end(!1, n), n
                    } finally {
                        this.executor.scanners.forEach((e => {
                            e.removeEventListener("progress", this.onConversationScanning), e.removeEventListener("success", this.onConversationScanned)
                        })), this.executor.removeEventListener("batching", this.onConversationsScanned)
                    }
                    return o
                }
                flush() {
                    this.executor.scanners.forEach((e => {
                        e.flush()
                    }))
                }
            }) || xt) || xt) || xt) || xt);
            var $t = s("aY5P"),
                Bt = s("KTzm"),
                Qt = s("cMnt");
            class qt {
                constructor() {}
                static getInstance() {
                    return qt.instance || (qt.instance = new qt), qt.instance
                }
                saveState(e, t) {
                    const s = this.getState(e) || {
                            totalConversations: 0,
                            scannedConversations: 0,
                            isFinished: !1,
                            updatedAt: Date.now()
                        },
                        r = Object(a.a)(Object(a.a)(Object(a.a)({}, s), t), {}, {
                            updatedAt: Date.now()
                        });
                    localStorage.setItem(e, JSON.stringify(r))
                }
                getState(e) {
                    const t = localStorage.getItem(e);
                    if (!t) return null;
                    try {
                        return JSON.parse(t)
                    } catch {
                        return null
                    }
                }
            }
            qt.instance = void 0;
            class Vt {
                constructor(e, t) {
                    this.path = e, this.messageId = t
                }
            }
            const Zt = (e, t) => {
                const s = Tt.getInstance();
                return {
                    dir: s[`getZaloDownloads${e}Dir`](t),
                    resDir: s[`getZaloDownloadsResource${e}Dir`](t)
                }
            };
            class Ht {
                constructor(e, t) {
                    this.convId = void 0, this.cliMsgId = void 0, this.fromUid = void 0, this.toUid = void 0, this.msgId = void 0, this.message = void 0, this.localPath = void 0, this.subType = void 0, this.convId = t, this.cliMsgId = e.cliMsgId, this.fromUid = e.fromUid, this.toUid = e.toUid || t, this.msgId = e.msgId, this.message = e.message, this.localPath = e.localPath, this.subType = e.subType
                }
                buildResourceName() {
                    const e = Object(jt.a)();
                    return "0" == this.fromUid && e && (this.fromUid = e), `${this.cliMsgId}_${this.fromUid}_${this.toUid}`
                }
            }
            class Kt {
                constructor() {
                    this.supportedTypes = [ae.MSG_CONTACT]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path,
                        {
                            dir: i,
                            resDir: o
                        } = Zt("RichThumb", s);
                    return e.forEach((e => {
                        if (Object(ft.v)(e) && e.message.thumb) {
                            const t = new Ht(e, s),
                                n = e.message.thumb,
                                c = t.buildResourceName(),
                                l = Object(mt.b)(n) || ".jpg";
                            r.push(new Vt(a.join(i, `z${e.msgId}_${Object(mt.d)(n)}${l}`), e.msgId)), r.push(new Vt(a.join(i, `z_${Object(mt.d)(n)}${l}`), e.msgId)), r.push(new Vt(a.join(o, `${c}${l}`), e.msgId))
                        }
                    })), r
                }
            }
            class Yt {
                constructor() {
                    this.supportedTypes = [ae.MSG_VOICE, ae.MSG_VIDEO]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path;
                    for (const i of e) {
                        const e = new Ht(i, s),
                            t = i.msgType === ae.MSG_VOICE ? "Voice" : "Video",
                            {
                                dir: o,
                                resDir: n
                            } = Zt(t, s),
                            c = e.buildResourceName();
                        r.push(new Vt(a.join(o, i.msgId), i.msgId)), r.push(new Vt(a.join(n, c), i.msgId))
                    }
                    return r
                }
            }
            class Wt {
                constructor() {
                    this.supportedTypes = [ae.MSG_DOODLE]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path,
                        i = Tt.getInstance(),
                        o = i.getZaloDownloadsPictureDir(s),
                        n = i.getZaloDownloadsResourcePictureDir(s);
                    for (const c of e) {
                        const e = c.message.oriUrl,
                            t = new Ht(c, s).buildResourceName();
                        if (e) {
                            const s = a.join(o, `z${c.msgId}_${Object(mt.e)(e)}`);
                            r.push(new Vt(s, c.msgId)), r.push(new Vt(a.join(n, `${t}_${Object(mt.e)(e)}`), c.msgId))
                        }
                    }
                    return r
                }
            }
            class Jt {
                constructor() {
                    this.supportedTypes = [ae.MSG_PHOTO]
                }
                extractUrls(e) {
                    return [e.oriUrl, e.hdUrl, e.normalUrl, e.thumbUrl]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path,
                        i = Tt.getInstance(),
                        {
                            dir: o,
                            resDir: n
                        } = Zt("Picture", s);
                    for (const c of e) {
                        const e = new Ht(c, s),
                            l = e.buildResourceName();
                        let d = !1;
                        if (c.message) {
                            const l = this.extractUrls(c.message);
                            for (const u of l)
                                if (u && (await this.addPathsForUrl(r, u, c, e, o, n, t, i, s, a), !d)) {
                                    const e = a.join(o, `z${c.msgId}_${Object(mt.e)(u)}`);
                                    d = St.i(e)
                                }
                        }!c.localPath || d && !t.includeImageGen || r.push(new Vt(c.localPath, c.msgId)), r.push(new Vt(a.join(o, c.msgId), c.msgId)), r.push(new Vt(a.join(n, l), c.msgId))
                    }
                    return r
                }
                async addPathsForUrl(e, t, s, r, a, i, o, n, c, l) {
                    const d = l.join(a, `z${s.msgId}_${Object(mt.e)(t)}`),
                        u = r.buildResourceName();
                    if (e.push(new Vt(d, s.msgId)), e.push(new Vt(l.join(i, `${u}_${Object(mt.e)(t)}`), s.msgId)), o.includeImageGen) {
                        e.push(new Vt(l.join(a, `z${s.msgId}_${Object(mt.e)(t,".jpg")}`), s.msgId));
                        for (const t of Object.values(Ot.o)) {
                            const r = n.getUserZaloDownloadsDir(),
                                a = await Rt.b.ZaloDownload_Photo(r, c, Object(Rt.d)({
                                    cliMsgId: s.cliMsgId,
                                    fromUid: s.fromUid,
                                    toUid: c
                                }), t);
                            e.push(new Vt(a, s.msgId))
                        }
                    }
                }
            }
            class Xt {
                constructor() {
                    this.supportedTypes = [ae.MSG_FILE]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path,
                        i = Tt.getInstance(),
                        {
                            dir: o,
                            resDir: n
                        } = Zt("File", s),
                        {
                            dir: c,
                            resDir: l
                        } = Zt("FileNoise", s),
                        {
                            dir: d,
                            resDir: u
                        } = Zt("Video", s);
                    return e.forEach((e => {
                        var h;
                        const g = Object(Mt.b)(e.message),
                            m = Object(Mt.c)(e),
                            p = (Object(Dt.n)(null == e || null === (h = e.message) || void 0 === h ? void 0 : h.params) || "").fType === kt.FileMsgType.Folder,
                            b = new Ht(e, s).buildResourceName();
                        if (p && t.zaloTempNewPos) {
                            const t = i.getZaloDownloadFolderDir(),
                                o = i.getZaloDownloadResourceFolderDir(s),
                                n = a.join(t, g);
                            r.push(new Vt(n, e.msgId)), r.push(new Vt(a.join(o, b), e.msgId))
                        }
                        if (r.push(new Vt(a.join(o, g), e.msgId)), r.push(new Vt(a.join(n, b), e.msgId)), r.push(new Vt(a.join(c, g), e.msgId)), r.push(new Vt(a.join(l, b), e.msgId)), bt.a.getGenType(m)) {
                            const t = i.getZaloDownloadsResourceFileThumbDir(s);
                            r.push(new Vt(pt.fHelper.getPhotoThumbCache(e, g), e.msgId)), r.push(new Vt(a.join(t, b), e.msgId))
                        }(e.subType === ae.MSG_SUBTYPE_MEDIA_VIDEO || m in It.a.VideoType) && (r.push(new Vt(a.join(d, e.msgId), e.msgId)), r.push(new Vt(a.join(u, b), e.msgId)))
                    })), r
                }
            }
            class es {
                constructor() {
                    this.supportedTypes = [ae.MSG_ZINSTANT, ae.MSG_ZINSTANT_OLD]
                }
                async extract(e, t, s) {
                    const r = [],
                        a = $znode.path,
                        i = Tt.getInstance().getZaloDownloadsZInstantDir();
                    return await Promise.all(e.map((async e => {
                        try {
                            const t = Lt.h.create(new Lt.h.Adapters.MessageEntity(e)),
                                s = await t.getZInstantData(),
                                o = yt.a.getFileNameFromUrl(s.data_url);
                            o && r.push(new Vt(a.join(i, o), e.msgId))
                        } catch {}
                    }))), r
                }
            }
            class ts {
                constructor() {
                    this.strategies = new Map, this.registerStrategies()
                }
                registerStrategies() {
                    const e = [new Yt, new Kt, new Wt, new Jt, new Xt, new es];
                    for (const t of e)
                        for (const e of t.supportedTypes) this.strategies.set(e, t)
                }
                hasStrategy(e) {
                    return this.strategies.has(e)
                }
                async extractPaths(e, t, s, r) {
                    const a = this.strategies.get(e);
                    if (!a) return [];
                    const i = await a.extract(t, s, r);
                    if (e !== ae.MSG_PHOTO)
                        for (const o of t) o.localPath && i.push(new Vt(o.localPath, o.msgId));
                    return i
                }
            }
            var ss;
            Object(y.j)()(ss = Object(l.injectable)()(ss = function(e, t) {
                return Object(l.inject)(B)(e, void 0, 0)
            }(ss = Reflect.metadata("design:type", Function)(ss = Reflect.metadata("design:paramtypes", [Object])(ss = class extends q.a {
                constructor(e) {
                    super(), this.clientBroadcaster = void 0, this.eventEmitter = void 0, this.data = void 0, this.tracking = void 0, this.stateStorage = void 0, this.pathExtractorService = void 0, this.clientBroadcaster = $t.a.redundantResource, this.eventEmitter = e, this.data = new Map, this.tracking = new Gt, this.stateStorage = qt.getInstance(), this.pathExtractorService = new ts
                }
                getType() {
                    return "RESOURCE-SCANNER-MANAGER-V2"
                }
                async processMessages(e, t, s, r) {
                    const a = await this.pathExtractorService.extractPaths(e, t, s, r);
                    if (a.length > 0) {
                        const t = this.data.get(e) || new wt(e);
                        for (const e of a) t.addItem(e.path, e.messageId);
                        this.data.set(e, t)
                    }
                }
                async onScanProgress(e, t, s, r) {
                    vt.b.getInstance().zsymb(0, "juZcUk", `onScanProgress: ${JSON.stringify({convId:e,msgType:t,fromDttm:s,toDttm:r})}`), this.eventEmitter.emit(U, {
                        type: t,
                        key: e,
                        data: this.data.get(t),
                        metadata: {
                            fromDttm: s,
                            toDttm: r
                        }
                    })
                }
                async onScanConvSuccess(e) {
                    this.eventEmitter.emit(x, {
                        convId: e
                    })
                }
                async execute(e) {
                    return new Promise(((t, s) => {
                        const r = this.stateStorage.getState(Qt.b);
                        r && !r.isFinished && r.scannedConversations > 0 && vt.b.getInstance().zsymb(6, "wzJGcz", `Detected old scanning session interrupted at: ${r.scannedConversations}/${r.totalConversations}`), this.stateStorage.saveState(Qt.b, {
                            isFinished: !1,
                            scannedConversations: 0
                        });
                        let a = 0,
                            i = 0;
                        const o = [];
                        return this.tracking.start(), l.ModuleContainer.resolve(Bt.a).subscribe(this.clientBroadcaster, {
                            onProgress: async t => {
                                const s = (async s => {
                                    if (null === (s = t.data) || void 0 === s || !s.length) return;
                                    const r = t.data.reduce(((e, t) => (e[t.msgType] || (e[t.msgType] = []), e[t.msgType].push(t), e)), {}),
                                        a = Qt.a.filter((e => r[e])).map((async s => {
                                            await this.processMessages(s, r[s], e.params.getConfig, t.convId), await this.onScanProgress(t.convId, s, t.metadata.range[0], t.metadata.range[1])
                                        }));
                                    await Promise.all(a)
                                })();
                                o.push(s)
                            },
                            onConvSuccess: async e => {
                                const t = (async t => {
                                    vt.b.getInstance().zsymb(0, "O6Ist0", `onConvSuccess: ${JSON.stringify(e)}`), this.tracking.track("scan conv success", e.convId), null !== (t = e.metadata) && void 0 !== t && t.totalConversations && (a = e.metadata.totalConversations), i++, this.stateStorage.saveState(Qt.b, {
                                        totalConversations: a,
                                        scannedConversations: i,
                                        isFinished: !1,
                                        lastConvId: e.convId
                                    }), this.onScanConvSuccess(e.convId)
                                })();
                                o.push(t)
                            },
                            onError: async e => (this.stateStorage.saveState(Qt.b, {
                                isFinished: !1
                            }), this.tracking.end(!1, e), this.eventEmitter.emit(G, e), s(e)),
                            onComplete: async e => {
                                vt.b.getInstance().zsymb(0, "1qPr8y", `onComplete: ${JSON.stringify(e)}`), await Promise.allSettled(o), this.tracking.end(!0);
                                const {
                                    totalScannedConversation: r
                                } = e;
                                if (a > 0 && r < a) {
                                    const e = `Scan Fail: Missing data. Scanned ${r}/${a} conversations.`;
                                    return this.stateStorage.saveState(Qt.b, {
                                        isFinished: !1
                                    }), this.tracking.end(!1, e), this.eventEmitter.emit(G, new Error(e)), s(new Error(e))
                                }
                                return this.stateStorage.saveState(Qt.b, {
                                    totalConversations: a,
                                    scannedConversations: r,
                                    isFinished: !0
                                }), this.eventEmitter.emit($, e), t(e)
                            }
                        })
                    }))
                }
            }) || ss) || ss) || ss) || ss);
            var rs = s("Ff2n"),
                as = s("hGrR"),
                is = s("q8PC");
            const os = ["duration", "interupted"];
            var ns;
            Object(l.singleton)(as.ResourceCleanupStateStorage)(ns = Reflect.metadata("design:type", Function)(ns = Reflect.metadata("design:paramtypes", [])(ns = class extends is.a {
                constructor() {
                    super(as.RRCStorageName), this.progress = void 0, this.stage = void 0, this.progressLimit = void 0, this.trackingData = void 0, this.progress = {}, this.stage = as.ResourceCleanupStage.IDLE, this.progressLimit = null, this.trackingData = null
                }
                getLocalAsObject(e, t) {
                    const s = this.getItemForCurrentUser(e);
                    let r = null;
                    if (null !== s) try {
                        r = JSON.parse(s), "object" != typeof r && (r = null)
                    } catch {
                        r = null
                    }
                    return r || t
                }
                getLocalAsNumber(e, t = null) {
                    const s = this.getItemForCurrentUser(e);
                    if (s) {
                        const e = +s;
                        if (!isNaN(e)) return e
                    }
                    return t
                }
                init(e) {
                    super.init(e);
                    const t = this.getLocalAsNumber(as.RRCStorageKeys.STAGE);
                    null !== t && t in as.ResourceCleanupStage && (this.stage = t), this.progress = this.getLocalAsObject(as.RRCStorageKeys.PROGRESS, {});
                    const s = this.getLocalAsNumber(as.RRCStorageKeys.PROGRESS_LIMIT);
                    s && (this.progressLimit = s), this.trackingData = this.getLocalAsObject(as.RRCStorageKeys.TRACKING_DATA)
                }
                setStage(e) {
                    this.stage = e, this.setItemForCurrentUser(as.RRCStorageKeys.STAGE, String(e))
                }
                getStage() {
                    return this.stage
                }
                setProgress(e, t, s) {
                    this.progress[e] || (this.progress[e] = {}), this.progress[e][t] = s, this.setItemForCurrentUser(as.RRCStorageKeys.PROGRESS, JSON.stringify(this.progress))
                }
                clearProgress(e) {
                    delete this.progress[e], this.setItemForCurrentUser(as.RRCStorageKeys.PROGRESS, JSON.stringify(this.progress))
                }
                clearAllProgress() {
                    this.progress = {}, this.removeItemForCurrentUser(as.RRCStorageKeys.PROGRESS)
                }
                getProgress(e) {
                    return this.progress[e]
                }
                getProgressKeys() {
                    return Object.keys(this.progress)
                }
                setProgressKeys(e) {
                    this.progress = Object.fromEntries(e.map((e => [e, {}]))), this.setItemForCurrentUser(as.RRCStorageKeys.PROGRESS, JSON.stringify(this.progress))
                }
                setProgressLimit(e) {
                    this.progressLimit = e, this.setItemForCurrentUser(as.RRCStorageKeys.PROGRESS_LIMIT, String(e))
                }
                getProgressLimit() {
                    return this.progressLimit
                }
                clearProgressLimit() {
                    this.progressLimit = null, this.removeItemForCurrentUser(as.RRCStorageKeys.PROGRESS_LIMIT)
                }
                setTrackingData(e) {
                    const {
                        duration: t,
                        interupted: s
                    } = e, r = Object(rs.a)(e, os);
                    this.trackingData || (this.trackingData = {
                        startTs: Date.now(),
                        duration: {},
                        interupted: {}
                    }), t && Object.assign(this.trackingData.duration, t), s && Object.assign(this.trackingData.interupted, s), Object.assign(this.trackingData, r), this.setItemForCurrentUser(as.RRCStorageKeys.TRACKING_DATA, JSON.stringify(this.trackingData))
                }
                getTrackingData() {
                    return this.trackingData
                }
                clearTrackingData() {
                    this.trackingData = null, this.removeItemForCurrentUser(as.RRCStorageKeys.TRACKING_DATA)
                }
            }) || ns) || ns);
            var cs = s("ut+A");

            function ls(e, t) {
                return async (...s) => {
                    const r = performance.now();
                    try {
                        const a = await e(...s);
                        return t(performance.now() - r, !0), a
                    } catch (a) {
                        throw t(performance.now() - r, !1), a
                    }
                }
            }
            class ds {
                constructor() {
                    this.taskQueue = void 0, this.isExecuting = void 0, this.taskQueue = [], this.isExecuting = !1
                }
                dequeueTask() {
                    if (!this.isExecuting) {
                        const e = this.taskQueue.shift();
                        e && (this.isExecuting = !0, e().finally((() => {
                            this.isExecuting = !1, this.dequeueTask()
                        })))
                    }
                }
                exec(e, ...t) {
                    return new Promise(((s, r) => {
                        this.taskQueue.push((() => e(t).then(s, r))), this.dequeueTask()
                    }))
                }
            }
            var us = s("yVfy"),
                hs = s("7+QW"),
                gs = s("Fdeg"),
                ms = s("ZsEe"),
                ps = s("jB3n");
            class bs {
                constructor(e, t = 1) {
                    this.progress = void 0, this.prevProgress = void 0, this.config = void 0, this.emitter = void 0, this.thresholdToSignal = void 0, this.progress = 0, this.prevProgress = 0, this.config = e, this.emitter = new ps.a, this.thresholdToSignal = t
                }
                signal() {
                    this.progress - this.prevProgress >= this.thresholdToSignal && (this.prevProgress = this.progress, this.emitter.emit("change", Math.round(this.progress)))
                }
                reset() {
                    this.progress = 0, this.prevProgress = 0, this.signal()
                }
                onDoneStage(e) {
                    this.progress = Math.min(100, this.progress + this.config[e]), this.signal()
                }
                onUpdateStage(e, t) {
                    this.progress = Math.min(100, this.progress + this.config[e] * t), this.signal()
                }
                subscribeChange(e) {
                    this.emitter.on("change", e)
                }
                unsubscribeChange(e) {
                    this.emitter.off("change", e)
                }
                removeAllSubscribers() {
                    this.emitter.removeAllListeners("change")
                }
            }
            class fs {
                constructor() {
                    this.reporter = void 0, this.convCount = void 0, this.doneConvCount = void 0, this.convCount = 0, this.doneConvCount = 0, this.reporter = new bs({
                        takeSnapshot: 10,
                        scanMessages: 70,
                        deleteResources: 20,
                        scanMessagesRestructureMedia: 70
                    })
                }
                reset() {
                    this.convCount = 0, this.doneConvCount = 0, this.reporter.reset()
                }
                onDoneTakeSnapshot() {
                    this.reporter.onDoneStage("takeSnapshot")
                }
                onDoneGetConvList(e) {
                    this.convCount = e
                }
                onDoneDeleteResources() {
                    this.reporter.onDoneStage("deleteResources")
                }
                onDoneConvItems(e) {
                    this.doneConvCount += e, this.convCount && this.reporter.onUpdateStage("scanMessages", e / this.convCount)
                }
                onDoneScanMessages() {
                    this.reporter.onDoneStage("scanMessages")
                }
                onDoneScanMessagesRestructureMedia() {
                    this.reporter.onDoneStage("scanMessagesRestructureMedia")
                }
                subscribeChange(e) {
                    this.reporter.subscribeChange(e)
                }
                unscribeChange(e) {
                    this.reporter.unsubscribeChange(e)
                }
                removeAllSubscribers() {
                    this.reporter.removeAllSubscribers()
                }
            }
            var ys, Is = s("mP9p");
            Object(l.injectable)()(ys = Object(l.singleton)(cs.ResourceCleanupManager)(ys = function(e, t) {
                return Object(l.inject)(as.ResourceCleanupStateStorage)(e, void 0, 0)
            }(ys = Reflect.metadata("design:type", Function)(ys = Reflect.metadata("design:paramtypes", [void 0 === as.ResourceCleanupStateStorage ? Object : as.ResourceCleanupStateStorage])(ys = class {
                constructor(e) {
                    this.stateStorage = e, this.logger = void 0, this.taskID = void 0, this.markerExecutor = void 0, this.config = void 0, this.progressReporter = void 0, this.resourceManagerV2 = void 0, this.getScanMessagesRangeForConv = (e, t) => {
                        const s = this.stateStorage.getProgress(e),
                            r = this.stateStorage.getProgressLimit();
                        if (s && s[t] && r) {
                            const e = s[t] + 1;
                            return [e, Math.max(r, e)]
                        }
                        return [0, Date.now()]
                    }, this.onScanFromMessagesProgress = async e => {
                        const {
                            key: t,
                            data: s,
                            metadata: r,
                            type: a
                        } = e, i = [];
                        s && (s.getItems().forEach((e => {
                            i.push({
                                filePath: e.path,
                                id: e.id
                            })
                        })), s.clear());
                        try {
                            if (i.length > 0) {
                                const e = Tt.getInstance().getUserZaloDownloadsDir();
                                await this.markerExecutor.exec((() => $zFeatures.zwalker.updateFileStats(e, i)))
                            }
                            this.stateStorage.setProgress(t, a, r.toDttm)
                        } catch (o) {
                            throw this.Logger.zsymb(18, "c9jRiM", "updateFileStats fail", t, o), o
                        }
                    }, this.onScanFromMessagesDoneItem = async e => {
                        const [t] = e;
                        this.stateStorage.clearProgress(t)
                    }, this.onScanFromMessagesDoneItemV2 = async e => {
                        const {
                            convId: t
                        } = e;
                        this.stateStorage.clearProgress(t)
                    }, this.onScanFromMessagesDoneBatching = async e => {
                        let {
                            duration: t,
                            convIds: s
                        } = e;
                        const r = this.stateStorage.getTrackingData();
                        r && (t += r.duration[as.ResourceCleanupStage.SCAN_MESSAGES] || 0), this.stateStorage.setTrackingData({
                            duration: {
                                [as.ResourceCleanupStage.SCAN_MESSAGES]: t
                            }
                        }), this.progressReporter.onDoneConvItems(s.length)
                    }, this.taskID = new ms.a(0), this.markerExecutor = new ds, this.progressReporter = new fs
                }
                get Logger() {
                    return this.logger || (this.logger = l.ModuleContainer.resolve(g.ZLoggerFactory).createZLogger(ee.ZLoggerNametags.redundantResCleanup, ["cleanup-manager"])), this.logger
                }
                getTrackingFolderPaths(e) {
                    return Object.keys(hs.a).map((t => $znode.path.join(e, t)))
                }
                getIgnoreFolderPaths(e) {
                    return [$znode.path.join(e, "Cache", "**"), $znode.path.join(e, "zcloud", "**"), "**" + gs.b]
                }
                async takeResourceSnapshot() {
                    this.stateStorage.setStage(as.ResourceCleanupStage.TAKE_SNAPSHOT);
                    const e = Tt.getInstance().getUserZaloDownloadsDir();
                    return await $zFeatures.zwalker.collectDirectoryStats(e, this.getTrackingFolderPaths(e))
                }
                scanMessages(e) {
                    return this.stateStorage.setStage(as.ResourceCleanupStage.SCAN_MESSAGES_RESTRUCTURE_MEDIA), new Promise((async (t, s) => {
                        const r = l.ModuleContainer.resolve(B),
                            a = e => this.onScanFromMessagesProgress(e),
                            i = e => this.onScanFromMessagesDoneItemV2(e),
                            o = () => {
                                r.omit(U, a), r.omit(x, i), r.omit($, n), r.omit(G, c)
                            },
                            n = e => {
                                o(), t(e)
                            },
                            c = e => {
                                o(), s(e)
                            };
                        r.subscribe(U, a), r.subscribe(x, i), r.subscribe($, n), r.subscribe(G, c), this.resourceManagerV2 || (this.resourceManagerV2 = new Is.a({
                            getConfig: e.getConfig()
                        })), this.resourceManagerV2.run().catch((() => {}))
                    }))
                }
                scanFromMessages(e) {
                    return this.stateStorage.setStage(as.ResourceCleanupStage.SCAN_MESSAGES), new Promise(((t, s) => {
                        const r = l.ModuleContainer.resolve(ht.ResourceScannerManager);
                        r.addEventListener("progress", this.onScanFromMessagesProgress), r.addEventListener("success-item", this.onScanFromMessagesDoneItem), r.addEventListener("batching", this.onScanFromMessagesDoneBatching), r.run(e).then(t).catch(s).finally((() => {
                            r.removeEventListener("progress", this.onScanFromMessagesProgress), r.removeEventListener("success-item", this.onScanFromMessagesDoneItem), r.removeEventListener("batching", this.onScanFromMessagesDoneBatching)
                        }))
                    }))
                }
                async deleteResources(e) {
                    this.stateStorage.setStage(as.ResourceCleanupStage.DELETE_RESOURCES);
                    const t = Tt.getInstance().getUserZaloDownloadsDir();
                    return await $zFeatures.zwalker.deleteEmptyFolders(t), await $zFeatures.zwalker.deleteUnmarkedFiles(t, this.getIgnoreFolderPaths(t), this.getTrackingFolderPaths(t), e)
                }
                async calculateStatsAfterScanning() {
                    const e = Tt.getInstance().getUserZaloDownloadsDir(),
                        t = this.stateStorage.getTrackingData();
                    if (t && t.statsBefore && t.statsDetailBefore) {
                        const s = t.statsBefore,
                            r = t.statsDetailBefore,
                            a = [259200, 604800, 1209600],
                            i = await $zFeatures.zwalker.statUnmarkedFiles(e, this.getIgnoreFolderPaths(e), this.getTrackingFolderPaths(e), a),
                            o = Object.fromEntries(Object.entries(i.trackingFolderData).map((([e, {
                                size: t,
                                file_number: s
                            }]) => {
                                const a = $znode.path.basename(e);
                                return [a, {
                                    size: r[a].size - t,
                                    count: r[a].count - s
                                }]
                            }))),
                            n = a.map((e => i.trackingATime && i.trackingATime[e] ? {
                                count: i.trackingATime[e].file_number,
                                size: i.trackingATime[e].size
                            } : {
                                count: 0,
                                size: 0
                            }));
                        this.stateStorage.setTrackingData({
                            statsAfter: {
                                count: s.count - i.fileNumber,
                                size: s.size - i.size
                            },
                            statsDetailAfter: o,
                            statsAccessTime: n
                        }), this.Logger.zsymb(0, "o1V9kX", `stats unmarked files=${i.fileNumber} size=${i.size}`, n, o)
                    }
                }
                async getAllConversations() {
                    const e = w.default.getInstance();
                    return (await e.Core.Conversation.getAll()).map((({
                        userId: e
                    }) => e)).filter(Boolean)
                }
                checkInterrupted(e) {
                    const t = this.stateStorage.getTrackingData();
                    if (t) {
                        const s = (t.interupted[e] || 0) + 1;
                        this.stateStorage.setTrackingData({
                            interupted: {
                                [e]: s
                            }
                        }), this.Logger.zsymb(0, "OaLQhi", "check interrupted:", s), s && us.b.logCleanupInterrupted(e, t.duration[e])
                    }
                }
                mapConfig(e) {
                    return {
                        batchConv: e.batchConv,
                        batchDelay: e.batchDelay,
                        batchSize: e.batchSize,
                        includeImageGen: !!e.collectGeneratedJPG,
                        zaloTempNewPos: !!e.zaloTempNewPos
                    }
                }
                async executeSnapshotPhase(e) {
                    this.Logger.zsymb(0, "_VUI7u", "[RRC] start #1 take snapshot", e);
                    const t = await ls(this.takeResourceSnapshot.bind(this), ((t, s) => {
                            this.Logger.zsymb(0, "bOFcJr", "[RRC] done #1 take snapshot", e, t), this.stateStorage.setTrackingData({
                                duration: {
                                    [as.ResourceCleanupStage.TAKE_SNAPSHOT]: t
                                }
                            }), us.b.logCleanupPerf(as.ResourceCleanupStage.TAKE_SNAPSHOT, t, s)
                        }))(),
                        s = Object.fromEntries(Object.entries(t.trackingFolderData).map((([e, {
                            size: t,
                            file_number: s
                        }]) => [$znode.path.basename(e), {
                            size: t,
                            count: s
                        }])));
                    this.stateStorage.setTrackingData({
                        statsBefore: {
                            size: t.size,
                            count: t.fileNumber
                        },
                        statsDetailBefore: s
                    }), us.c.logSnapshotSuccessDetail(t.size, s), this.Logger.zsymb(0, "dHVWnc", `stats before files=${t.fileNumber} size=${t.size} detail=${JSON.stringify(s)}`)
                }
                async executeLegacyScanMesssagePhase(e, t) {
                    const {
                        userId: s,
                        abort: r,
                        config: a
                    } = t;
                    this.Logger.zsymb(0, "TyCvN-", "[RRC] start #2 scan messages", e);
                    let i = this.stateStorage.getProgressKeys();
                    i.length || (i = await this.getAllConversations(), this.stateStorage.setProgressKeys(i)), this.progressReporter.onDoneGetConvList(i.length), this.Logger.zsymb(0, "2VkOOo", "total conv:", i.length), await ls(this.scanFromMessages.bind(this, {
                        abort: r,
                        getConfig: () => this.mapConfig(a),
                        getRange: this.getScanMessagesRangeForConv,
                        convIds: i,
                        userId: s
                    }), ((t, s) => {
                        this.Logger.zsymb(0, "rX3Nar", "[RRC] done #2 scan messages", e, t), us.b.logCleanupPerf(as.ResourceCleanupStage.SCAN_MESSAGES, t, s)
                    }))()
                }
                async executeDeletionPhase(e, t) {
                    const {
                        config: s
                    } = t;
                    this.Logger.zsymb(0, "5Ao4Z7", "[RRC] start #3 delete resources", e);
                    const r = await ls(this.deleteResources.bind(this, !!s.deleteStatCache), ((t, s) => {
                            this.Logger.zsymb(0, "M-QYxs", "[RRC] done #3 delete resources", e, t), this.stateStorage.setTrackingData({
                                duration: {
                                    [as.ResourceCleanupStage.DELETE_RESOURCES]: t
                                }
                            }), us.b.logCleanupPerf(as.ResourceCleanupStage.DELETE_RESOURCES, t, s)
                        }))(),
                        a = Object.fromEntries(Object.entries(r.trackingFolderData).map((([e, {
                            size: t,
                            file_number: s
                        }]) => [$znode.path.basename(e), {
                            size: t,
                            count: s
                        }])));
                    this.stateStorage.setTrackingData({
                        statsAfter: {
                            size: r.size,
                            count: r.fileNumber
                        },
                        statsDetailAfter: a,
                        statsFailure: {
                            size: r.failedSize,
                            count: r.failedFileNumber
                        }
                    }), this.Logger.zsymb(0, "Fg-z7g", `stats after files=${r.fileNumber} size=${r.size} detail=${JSON.stringify(a)}`)
                }
                async run(e) {
                    const {
                        userId: t,
                        abort: s,
                        config: r,
                        maxTimeScan: a = Date.now()
                    } = e;
                    this.config = r, this.progressReporter.reset(), this.stateStorage.isInitialized() || this.stateStorage.init(t), await Tt.getInstance().init(t);
                    const i = this.taskID.next();
                    let o, n = this.stateStorage.getStage(),
                        c = this.stateStorage.getProgressLimit();
                    if (this.Logger.zsymb(0, "KQyRIM", "run task", i, n, c), this.checkInterrupted(n), c ? c < a - this.config.expiredCacheTime && (this.Logger.zsymb(0, "2Uls7H", "cache expired -> start from IDLE"), this.flush(), this.stateStorage.setProgressLimit(a), n = as.ResourceCleanupStage.IDLE) : this.stateStorage.setProgressLimit(a), this.config.enableUseScanMessagesV2 && (this.Logger.zsymb(0, "HQNLDn", "[RRC] start #0 scan messages restructure media", i), o = this.scanMessages({
                            abort: s,
                            getConfig: () => this.mapConfig(r)
                        })), n <= as.ResourceCleanupStage.TAKE_SNAPSHOT && await this.executeSnapshotPhase(i), this.progressReporter.onDoneTakeSnapshot(), this.config.enableUseScanMessagesV2) {
                        if (o) try {
                            await o, this.Logger.zsymb(0, "JlL9fz", "[RRC] scan messages restructure media success", i), this.progressReporter.onDoneScanMessagesRestructureMedia()
                        } catch (l) {
                            throw this.Logger.zsymb(18, "eL5eYO", "[RRC] scan messages restructure media error", i, l), l
                        }
                    } else n <= as.ResourceCleanupStage.SCAN_MESSAGES && await this.executeLegacyScanMesssagePhase(i, e), this.progressReporter.onDoneScanMessages();
                    return n <= as.ResourceCleanupStage.DELETE_RESOURCES && r.enableDelete ? await this.executeDeletionPhase(i, e) : await this.calculateStatsAfterScanning(), this.progressReporter.onDoneDeleteResources(), this.stateStorage.getTrackingData() || {
                        duration: {},
                        interupted: {},
                        startTs: 0
                    }
                }
                flush() {
                    l.ModuleContainer.resolve(ht.ResourceScannerManager).flush(), this.stateStorage.isInitialized() && (this.stateStorage.setStage(as.ResourceCleanupStage.IDLE), this.stateStorage.clearAllProgress(), this.stateStorage.clearProgressLimit(), this.stateStorage.clearTrackingData()), this.progressReporter.removeAllSubscribers()
                }
                subscribeProgress(e) {
                    this.progressReporter.subscribeChange(e)
                }
                unsubscribeProgress(e) {
                    this.progressReporter.unscribeChange(e)
                }
                setConfig(e) {
                    this.Logger.zsymb(0, "wEJpLg", "setConfig", e), this.config = e
                }
            }) || ys) || ys) || ys) || ys);
            var Ss;
            Object(y.j)()(Ss = Object(l.injectable)()(Ss = class extends q.a {
                constructor(...e) {
                    super(...e), this.logger = void 0
                }
                getType() {
                    return "REDUNDANT_RES_CLEANUP_START"
                }
                get Logger() {
                    return this.logger || (this.logger = l.ModuleContainer.resolve(g.ZLoggerFactory).createZLogger(ee.ZLoggerNametags.redundantResCleanup, ["worker-task"])), this.logger
                }
                onExecuteSuccess() {
                    const e = l.ModuleContainer.resolve(as.ResourceCleanupStateStorage).getTrackingData();
                    if (e) {
                        const {
                            statsBefore: t,
                            statsAfter: s,
                            statsDetailAfter: r,
                            statsDetailBefore: a,
                            duration: i
                        } = e;
                        t && s && r && a && (us.b.logCleanupSuccess(Math.max(t.size - s.size, 0), Nt.b.sum(Object.values(i))), us.b.logCleanupSuccessDetail(s.size, r))
                    }
                }
                onExecuteFail() {
                    const e = l.ModuleContainer.resolve(as.ResourceCleanupStateStorage),
                        t = e.getTrackingData(),
                        s = e.getStage();
                    us.b.logCleanupFail(s, t ? Nt.b.sum(Object.values(t.duration)) : 0)
                }
                async execute(e) {
                    const {
                        params: t,
                        abort: s,
                        report: r
                    } = e, {
                        config: a,
                        userId: i,
                        maxTimeScan: o,
                        sessionId: n,
                        forced: c
                    } = t;
                    let d = t.flush;
                    const u = l.ModuleContainer.resolve(cs.ResourceCleanupManager),
                        h = e => {
                            e % 5 == 0 && this.Logger.zsymb(0, "OpXPKl", "run progress", e), r(e)
                        };
                    e.onParamsChange((({
                        config: e,
                        flush: t
                    }) => {
                        "boolean" == typeof t && (d = t), "object" == typeof e && l.ModuleContainer.resolve(cs.ResourceCleanupManager).setConfig(e)
                    }));
                    try {
                        u.subscribeProgress(h);
                        const e = await u.run({
                            config: a,
                            userId: i,
                            maxTimeScan: o,
                            abort: s,
                            sessionId: n,
                            forced: c
                        });
                        return this.onExecuteSuccess(), e
                    } catch (g) {
                        throw this.onExecuteFail(), g
                    } finally {
                        d && u.flush(), u.unsubscribeProgress(h)
                    }
                }
            }) || Ss);
            s("69Uf"), s("Q/Ir"), s("rBRb");
            var vs, Ms = s("jbAT"),
                ws = s("zLd2"),
                _s = s("5txd");
            Object(l.injectable)()(vs = Object(l.singleton)(ws.c)(vs = function(e, t) {
                return Object(l.inject)(Ms.b)(e, void 0, 0)
            }(vs = function(e, t) {
                return Object(l.inject)(g.ZLoggerFactory)(e, void 0, 1)
            }(vs = Reflect.metadata("design:type", Function)(vs = Reflect.metadata("design:paramtypes", [void 0 === Ms.a ? Object : Ms.a, void 0 === g.ZLoggerFactory ? Object : g.ZLoggerFactory])(vs = class {
                constructor(e, t) {
                    this._utilsMediaAppService = void 0, this._logger = void 0, this._log = void 0, this._utilsMediaAppService = e, this._logger = t.createZLogger(ws.b, ["manage-utils-media-in-ui"]), this._log = Object(_s.b)(this._logger)
                }
                create(e) {
                    return this._utilsMediaAppService.create(e)
                }
                createOrUpdate(e) {
                    return this._utilsMediaAppService.createOrUpdate(e)
                }
                async createOrUpdateFromMedias(e) {
                    return this._utilsMediaAppService.createOrUpdateFromMedias(e)
                }
            }) || vs) || vs) || vs) || vs) || vs);
            s("37f1");
            var Cs, Es = s("X2RP"),
                Ts = s("ggcH"),
                Os = s("rvru"),
                Rs = s("rkiK"),
                Ds = s("Mk04"),
                ks = s("DOOx"),
                Fs = s("Yggq"),
                zs = s("HWGt");
            (new(Object(y.h)()(Cs = class extends zs.a {
                constructor(...e) {
                    super(...e), this.dbQos = null, this.signalOutOfMem = Object(Ds.a)((() => {
                        he.p.triggerEvent(he.o, [{
                            type: ks.b.OUT_OF_MEM,
                            src: ks.a.DB
                        }])
                    })), this.signalInvalidVersion = Object(Ds.a)((() => {
                        $zdb.notifyOpenDBFailed(!0, "VersionError")
                    })), this.signalMissingStore = Object(Ds.a)((() => {
                        $zdb.notifyOpenDBFailed(!0, "zkey-miss-object-store")
                    })), this.signalExceedingLimitInReopeningDBConnection = Object(Ds.a)((() => {
                        $zdb.notifyOpenDBFailed(!1)
                    })), this.signalDBClosedAbnormally = Object(Ds.a)((() => {
                        l.ModuleContainer.resolve(Ts.TDBCorruptionDetector).forceCheckDBCorruption()
                    }))
                }
                getDBQos() {
                    return null === this.dbQos && (this.dbQos = l.ModuleContainer.resolve(Os.a)), this.dbQos
                }
                handleQueryError(e) {
                    this.getDBQos().sendDBErrorQos(e)
                }
                handleOutOfMemError(e) {
                    this.signalOutOfMem()
                }
                handleInvalidVersionError() {
                    this.signalInvalidVersion()
                }
                handleMissingStoreError(e) {
                    const {
                        database: t,
                        missingTables: s,
                        adapterType: r
                    } = e;
                    this.signalMissingStore();
                    this.getDBQos().sendMissingTableQos(t, s)
                }
                handleExceedingLimitInReopeningDBConnection(e) {
                    this.signalExceedingLimitInReopeningDBConnection()
                }
                handleQueryExec(e) {
                    const {
                        database: t,
                        table: s,
                        method: r,
                        transaction: a,
                        startTime: i,
                        getEndTime: o
                    } = e;
                    Fs.a.has(t) || o().then((e => {
                        const o = {
                            method: r,
                            database: t,
                            table: s,
                            transaction: a
                        };
                        Rs.default.recordInfo(Rs.MetricName.query_resolution_time, {
                            startAt: i,
                            endAt: e,
                            info: o
                        })
                    })).catch((e => {}))
                }
                handleSuccessOpenDB(e) {
                    const {
                        startTime: t,
                        endTime: s,
                        fullname: r
                    } = e;
                    this.getDBQos().sendSuccessOpenDBDurationQos(r, t, s - t), Rs.default.recordInfo(Rs.MetricName.db_ready, {
                        startAt: t,
                        endAt: s,
                        info: {
                            dbName: r
                        }
                    })
                }
                handleConnectionClosedAbnormally() {
                    this.signalDBClosedAbnormally()
                }
                handleSchemaMigratedAbnormally() {}
                handleLongOpenDB(e) {
                    const {
                        startTime: t,
                        endTime: s,
                        fullname: r
                    } = e;
                    this.getDBQos().sendLongOpenRequestQos(r, t, s - t)
                }
                handleTimeConsumingQuery(e, t) {
                    this.getDBQos().sendTimeConsumingQueryQos(e, t)
                }
                handleWarning(e) {
                    const t = this.getDBQos();
                    if (e.type === Es.b.FAILED_MULTI) t.sendFailedMultiErrorQos(e)
                }
                onDispose() {
                    this.dispose()
                }
            }) || Cs)).start(), l.ModuleContainer.resolve(p.a).addEventListener(nt.a.SessionChange, (async ({
                session: e
            }) => {
                const t = ve.default.getInstance();
                null !== e ? t.init(e.userId, e.UIN) : t.resetSession()
            }));
            s("uiYo"), s("A+kQ");
            var js = s("gtFi"),
                Ns = s("XmYa"),
                Ls = s("hlqx"),
                As = s("666L");
            var Ps = s("4/eo");
            class Us {
                constructor(e = "persist") {
                    this._storage = void 0, this.name = void 0, this.name = e
                }
                get storage() {
                    return this._storage || (this._storage = this.init()), this._storage
                }
                get storageKey() {
                    return "mds-config"
                }
                get store() {
                    return $zelectron.getElectronStore({
                        name: this.storageKey
                    })
                }
                computeUserKey(e) {
                    return [Object(Ps.a)(), e].join("_")
                }
                setItemForCurrentUser(e, t) {
                    this.store.set(this.computeUserKey(e), t)
                }
                removeItemForCurrentUser(e) {
                    this.store.delete(this.computeUserKey(e))
                }
                getItemForCurrentUser(e) {
                    var t;
                    return null !== (t = this.store.get(this.computeUserKey(e))) && void 0 !== t ? t : null
                }
                init() {
                    const e = this.getItemForCurrentUser(this.storageKey);
                    return Object(Me.a)(e, {}) || {}
                }
                get(e) {
                    return this.storage[e]
                }
                set(e, t) {
                    this.storage[e] = t, this.persist()
                }
                clear(e) {
                    delete this.storage[e], this.persist()
                }
                clearAll() {
                    this._storage = {}, this.persist()
                }
                persist() {
                    this.setItemForCurrentUser(this.storageKey, JSON.stringify(this.storage, null, 2))
                }
            }
            var xs, Gs = s("MNHy");
            Object(y.j)()(xs = Object(l.injectable)()(xs = Reflect.metadata("design:type", Function)(xs = Reflect.metadata("design:paramtypes", [])(xs = class extends q.a {
                constructor() {
                    super(), this.autoScanController = void 0, this.autoScanController = this.initAutoScanController()
                }
                initManualScanController() {
                    const e = new Ls.a,
                        t = new As.a(e);
                    return new js.a(Ns.c.Manual, t)
                }
                initAutoScanController() {
                    const e = new Us,
                        t = new As.a(e);
                    return new js.a(Ns.c.Auto, t)
                }
                getType() {
                    return "MESSAGE_DB_SCANNER"
                }
                async execute(e) {
                    const {
                        abort: t,
                        report: s,
                        params: r
                    } = e, a = l.ModuleContainer.resolve(Bt.a), i = async e => {
                        var t, r;
                        null !== (t = e.metadata) && void 0 !== t && t.totalConversations && null !== (r = e.metadata) && void 0 !== r && r.scannedConversations && s(Math.round(e.metadata.scannedConversations / e.metadata.totalConversations * 100))
                    }, o = async () => {
                        s(100)
                    };
                    e.onParamsChange((({
                        scannerConfig: e
                    }) => {
                        this.autoScanController.setConfig(e)
                    }));
                    try {
                        if (r.mode === Ns.c.Auto) return a.emitter.addEventListener(Gs.a.ConvSuccess, i), a.emitter.addEventListener(Gs.a.Complete, o), await this.autoScanController.run({
                            abort: t,
                            config: r.scannerConfig
                        }), a.countListeners();
                        throw new Error("Manual scan is not supported")
                    } catch (n) {
                        throw n
                    } finally {
                        r.mode === Ns.c.Auto && (a.emitter.removeEventListener(Gs.a.ConvSuccess, i), a.emitter.removeEventListener(Gs.a.Complete, o))
                    }
                }
            }) || xs) || xs) || xs);
            s("Le17");
            var $s, Bs = s("MIkf"),
                Qs = s("VYav"),
                qs = s("mH7l"),
                Vs = s.n(qs),
                Zs = s("CCM8"),
                Hs = s("qmTA"),
                Ks = s("ce7i"),
                Ys = s("eEyI"),
                Ws = s("mTcm"),
                Js = s("3U5D");
            class Xs {
                constructor() {
                    this.logger = void 0, this.migrator = void 0, this.clientId = void 0, this.migrationQueue = void 0, this.secureLocalStorage = void 0, this.stats = void 0, this.onStarted = async e => {
                        this.getMigrationVersion() || (this.setMigrationVersion(e.version), this.stats.startedAt = Date.now(), this.persistStats(), Js.RestructureMediaQos.getInstance().logMigrateStarted())
                    }, this.onProgress = async e => {
                        e.data && 0 !== e.data.length && (await this.migrateBatch(e.data, e.convId), this.persistStats())
                    }, this.onConvSuccess = async () => {
                        this.stats.totalConversations++, this.persistStats()
                    }, this.onError = async e => {
                        this.logger.zsymb(18, "kY5nIA", "Migration error occurred", {
                            error: e,
                            currentStats: this.stats
                        })
                    }, this.onComplete = async e => {
                        try {
                            const t = this.getMigrationVersion();
                            this.removeMigrationVersion(), await this.migrationQueue.onIdle();
                            const s = t !== e.version,
                                r = this.stats.totalMessages < e.totalScannedMessage,
                                a = this.stats.totalConversations < e.totalScannedConversation;
                            if (s || r || a) return s ? (this.logger.zsymb(0, "LGKJ3o", "Migration version mismatches, skipping migration", {
                                version: t,
                                dataVersion: e.version
                            }), Js.RestructureMediaQos.getInstance().logMigrateMismatch("VERSION_MISMATCH")) : r ? (this.logger.zsymb(0, "CsKqYC", "Migration total messages mismatches, skipping migration", {
                                totalMessages: this.stats.totalMessages,
                                totalScannedMessage: e.totalScannedMessage
                            }), Js.RestructureMediaQos.getInstance().logMigrateMismatch("TOTAL_MESSAGE_MISMATCH")) : a && (this.logger.zsymb(0, "WQkE0u", "Migration total conversations mismatches, skipping migration", {
                                totalConversations: this.stats.totalConversations,
                                totalScannedConversation: e.totalScannedConversation
                            }), Js.RestructureMediaQos.getInstance().logMigrateMismatch("TOTAL_CONVERSATION_MISMATCH")), this.clearPersistedStats(), void(this.stats = this.createEmptyStats());
                            this.logger.zsymb(0, "erVBfi", "Migration completed successfully", {
                                finalStats: this.stats,
                                completionData: e
                            }), this.isAllowedApplyNewPath ? (Object(Zs.setPathStructureV2UserScope)(!0), await this.setCompletedStats("migrated"), Js.RestructureMediaQos.getInstance().logMigrateCompleted("PATH_VERSION_V2"), Js.RestructureMediaQos.getInstance().logApplyNewPathStructureSuccess("SCAN_MSG_MIGRATE"), this.logger.zsymb(0, "MD5sOo", "New path structure has been applied")) : (await this.setCompletedStats("not_migrated"), Js.RestructureMediaQos.getInstance().logMigrateCompleted("PATH_VERSION_V1")), this.clearPersistedStats(), this.stats = this.createEmptyStats(), this.migrationQueue.clear()
                        } catch (t) {
                            this.logger.zsymb(18, "XW5J0Y", "Error in onComplete", t), Js.RestructureMediaQos.getInstance().logApplyNewPathStructureFailed("SCAN_MSG_MIGRATE")
                        }
                    }, this.logger = Object(Qs.a)("migration-service"), this.migrator = new Bs.a, this.clientId = $t.a.migratePath, this.secureLocalStorage = ve.default.getInstance(), this.migrationQueue = new Vs.a({
                        concurrency: 40,
                        autoStart: !0
                    }), this.stats = this.createEmptyStats()
                }
                get isEnabled() {
                    return Ys.a.key("is_enable_migrate").value
                }
                get isAllowedApplyNewPath() {
                    return Ys.a.key("is_allow_set_new_path").value
                }
                async waitLocalStorageReady() {
                    this.secureLocalStorage.isReady || await this.secureLocalStorage.waitForReady()
                }
                createEmptyStats() {
                    return {
                        totalMessages: 0,
                        totalConversations: 0,
                        processedMessages: 0,
                        successfulMessages: 0,
                        failedMessages: 0,
                        totalFiles: 0,
                        successfulLinks: 0,
                        failedLinks: 0,
                        startedAt: 0
                    }
                }
                isValidStats(e) {
                    if (!e) return !1;
                    return "number" == typeof e.totalMessages && "number" == typeof e.totalConversations && "number" == typeof e.processedMessages && "number" == typeof e.successfulMessages && "number" == typeof e.failedMessages && "number" == typeof e.totalFiles && "number" == typeof e.successfulLinks && "number" == typeof e.failedLinks && "number" == typeof e.startAt
                }
                async setCompletedStats(e) {
                    await Object(Ws.c)({
                        pathStructureVersion: "migrated" === e ? 2 : 1,
                        stats: Object(a.a)(Object(a.a)({}, this.stats), {}, {
                            completedTs: Date.now()
                        }),
                        ts: Date.now()
                    })
                }
                async clearCompletedStats() {
                    await Object(Ws.a)()
                }
                restoreStatsFromStorage() {
                    try {
                        const e = this.secureLocalStorage.getItemForCurrentUser(Ks.e);
                        if (!e) return !1;
                        const t = JSON.parse(e);
                        return !!this.isValidStats(t) && (this.stats = t, !0)
                    } catch (e) {
                        return this.logger.zsymb(18, "sUQ2B5", "Failed to restore migration stats", e), !1
                    }
                }
                persistStats() {
                    try {
                        this.secureLocalStorage.setItemForCurrentUser(Ks.e, JSON.stringify(this.stats))
                    } catch (e) {
                        this.logger.zsymb(18, "7EXVKA", "Failed to persist migration stats", e)
                    }
                }
                clearPersistedStats() {
                    try {
                        this.secureLocalStorage.removeItemForCurrentUser(Ks.e)
                    } catch (e) {
                        this.logger.zsymb(18, "DUaGlj", "Failed to clear persisted migration stats", e)
                    }
                }
                dispose() {
                    this.unsubscribeFromMessageScanBroadcaster(), this.migrationQueue.clear(), this.migrationQueue.pause()
                }
                async startMigration() {
                    if (Object(Zs.isUsingPathStructureV2)()) return void this.logger.zsymb(0, "j5VPCz", "New path structure is already applied");
                    if (!this.isEnabled) return void this.logger.zsymb(0, "EjurpX", "Migration is disabled");
                    if (!(await Hs.a.getInstance().checkZaloPCDirHardlinkSupport()).supported) return void this.logger.zsymb(0, "jjEOcC", "Media folder does not support hardlink, skipping migration");
                    this.logger.zsymb(0, "8zkAlD", "Starting media path migration"), await this.waitLocalStorageReady(), await this.clearCompletedStats();
                    this.restoreStatsFromStorage() ? this.logger.zsymb(0, "UGPpmv", "Restored persisted migration stats", this.stats) : this.resetStats(), this.migrationQueue.start(), this.subscribeToMessageScanBroadcaster()
                }
                getStats() {
                    return Object(a.a)({}, this.stats)
                }
                getQueueStats() {
                    return {
                        size: this.migrationQueue.size,
                        pending: this.migrationQueue.pending,
                        isPaused: this.migrationQueue.isPaused
                    }
                }
                subscribeToMessageScanBroadcaster() {
                    this.logger.zsymb(0, "1PLBbB", "Subscribing to message scan broadcaster");
                    l.ModuleContainer.resolve(Bt.a).subscribe(this.clientId, {
                        onStarted: this.onStarted,
                        onProgress: this.onProgress,
                        onConvSuccess: this.onConvSuccess,
                        onError: this.onError,
                        onComplete: this.onComplete
                    })
                }
                unsubscribeFromMessageScanBroadcaster() {
                    this.logger.zsymb(0, "jTtpk6", "Unsubscribing from message scan broadcaster");
                    try {
                        l.ModuleContainer.resolve(Bt.a).unsubscribe(this.clientId)
                    } catch (e) {
                        this.logger.zsymb(18, "ikKUFq", "Error unsubscribing from broadcaster", e)
                    }
                }
                updateStats(e) {
                    this.stats.processedMessages++, e.success ? this.stats.successfulMessages++ : this.stats.failedMessages++, this.stats.totalFiles += e.totalFiles, this.stats.successfulLinks += e.successfulLinks, this.stats.failedLinks += e.totalFiles - e.successfulLinks
                }
                resetStats() {
                    this.stats = this.createEmptyStats(), this.persistStats()
                }
                async migrateBatch(e, t) {
                    this.stats.totalMessages += e.length;
                    const s = Date.now(),
                        r = [],
                        a = e.map((async e => this.migrateMessage(e, t))),
                        i = await Promise.allSettled(a);
                    for (const o of i) "fulfilled" === o.status && o.value && r.push(o.value);
                    return {
                        results: r,
                        stats: this.getStats(),
                        batchSize: e.length,
                        processingTime: Date.now() - s
                    }
                }
                async migrateMessage(e, t) {
                    return this.migrationQueue.add((async () => {
                        const s = await this.migrator.migrateMessage(e, t);
                        return s.success || Js.RestructureMediaQos.getInstance().logMigrateFailed(e.msgType), this.updateStats(s), s
                    }))
                }
                getMigrationVersion() {
                    return this.secureLocalStorage.getItemForCurrentUser(Ks.c) || ""
                }
                setMigrationVersion(e) {
                    this.secureLocalStorage.setItemForCurrentUser(Ks.c, e)
                }
                removeMigrationVersion() {
                    this.secureLocalStorage.removeItemForCurrentUser(Ks.c)
                }
            }
            Object(y.j)()($s = Object(l.injectable)()($s = Reflect.metadata("design:type", Function)($s = Reflect.metadata("design:paramtypes", [])($s = class extends q.a {
                constructor() {
                    super()
                }
                getType() {
                    return "MIGRATE_NPS_REGISTER"
                }
                async execute(e) {
                    const {
                        report: t
                    } = e, s = new Xs;
                    return await s.startMigration(), t(100), !0
                }
            }) || $s) || $s) || $s);
            var er = s("wNKw"),
                tr = s("Ikhw"),
                sr = s("g2tJ"),
                rr = s("C64z"),
                ar = s("Bgp9"),
                ir = s("emf8");
            const or = "renderer" === __SCRIPT_TYPE__ ? $znode.path : s("ipWf"),
                nr = "renderer" === __SCRIPT_TYPE__ ? $znode.fsPromise : s("cSWM").promises,
                cr = "renderer" === __SCRIPT_TYPE__ ? $znode.fastGlob : s("sRf1"),
                lr = "renderer" === __SCRIPT_TYPE__ ? $znode.fsExtra : s("g+8e"),
                dr = nr.stat,
                ur = nr.unlink,
                hr = lr.rename,
                gr = [tr.a.FOLDER, tr.a.FILE, tr.a.FILE_NOISE];
            class mr {
                constructor() {
                    this.logger = void 0, this.resourceLinkManager = void 0, this.logger = Object(Qs.a)("hardlinkRebuild"), this.resourceLinkManager = er.a.getInstance()
                }
                static getInstance() {
                    return this.instance || (this.instance = new mr), this.instance
                }
                async rebuildFileLinks(e = gr) {
                    const t = Date.now();
                    try {
                        if (!(await ir.a.getInstance().checkZaloPCDirHardlinkSupport()).supported) return void this.logger.zsymb(0, "jn4Ljc", "Hardlink is not supported");
                        const s = e.filter((e => gr.includes(e)));
                        if (0 === s.length) return void this.logger.zsymb(6, "ni2XJ8", "No allowed resource types provided. Only FOLDER, FILE, and FILE_NOISE are supported.");
                        this.logger.zsymb(0, "M0jANv", "Starting rebuild file links process", {
                            resourceTypes: s
                        });
                        for (const e of s) await this.processResourceType(e);
                        this.logger.zsymb(0, "fmfkg7", "Completed rebuild file links process"), Js.RestructureMediaQos.getInstance().logRebuildHardlinkSuccess(Date.now() - t)
                    } catch (s) {
                        throw this.logger.zsymb(18, "1HUE3v", "Error in rebuildFileLinks", s), Js.RestructureMediaQos.getInstance().logRebuildHardlinkFailed(Date.now() - t), s
                    }
                }
                async processResourceType(e) {
                    const t = Date.now();
                    try {
                        this.logger.zsymb(0, "_hhbNj", `Processing resource type: ${e}`);
                        const t = await this.getFolderPathsByResourceType(e);
                        if (!t || !t.oldFolder || !t.newFolderPattern) return void this.logger.zsymb(6, "Vwy5cM", `No folder paths configured for resource type: ${e}`);
                        const s = await this.pathExists(t.oldFolder);
                        s || this.logger.zsymb(6, "ZouNxl", `Old folder does not exist: ${t.oldFolder}`);
                        const r = s ? await this.buildMasterIndex(t.oldFolder, e) : this.createEmptyMasterIndex();
                        this.logger.zsymb(0, "mvC5li", `Master index size: ${r.total}`, {
                            resourceType: e,
                            oldFolder: t.oldFolder
                        });
                        const a = await this.processResourceEntries(e, t, r, {
                            dryRun: !1
                        });
                        this.logger.zsymb(0, "OVJLAQ", `Completed processing ${e}`, {
                            resourceType: e,
                            totalFiles: a.totalFiles,
                            alreadyLinked: a.alreadyLinked,
                            matched: a.matched,
                            successfulRelinks: a.successfulRelinks,
                            md5Comparisons: a.md5Comparisons,
                            spaceSaved: a.spaceSaved
                        }), r.byInode.clear(), r.bySize.clear(), r.total = 0
                    } catch (s) {
                        this.logger.zsymb(18, "pzzMyG", `Error processing resource type: ${e}`, s), Js.RestructureMediaQos.getInstance().logRebuildHardlinkResourceFailed(e, Date.now() - t)
                    }
                }
                async getFolderPathsByResourceType(e) {
                    const t = Object(jt.a)();
                    if (!t) throw new Error("User ID is required to get folder paths");
                    const s = await Object(Ft.e)(t);
                    switch (e) {
                        case tr.a.FILE:
                            return {
                                oldFolder: await Object(Ft.n)("", t), newFolderPattern: or.join(s, "*", "file", "*"), mediaRoot: s, resourceDirName: "file"
                            };
                        case tr.a.FILE_NOISE:
                            return {
                                oldFolder: await Object(Ft.r)("", t), newFolderPattern: or.join(s, "*", "fileNoise", "*"), mediaRoot: s, resourceDirName: "fileNoise"
                            };
                        case tr.a.FOLDER:
                            return {
                                oldFolder: await Object(Ft.c)(t), newFolderPattern: or.join(s, "*", "folder", "*"), mediaRoot: s, resourceDirName: "folder"
                            };
                        case tr.a.CACHE:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "Cache"), newFolderPattern: or.join(s, "*", "cache", "*"), mediaRoot: s, resourceDirName: "cache"
                            };
                        case tr.a.PHOTO:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "picture"), newFolderPattern: or.join(s, "*", "picture", "*"), mediaRoot: s, resourceDirName: "picture"
                            };
                        case tr.a.VOICE:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "voice"), newFolderPattern: or.join(s, "*", "voice", "*"), mediaRoot: s, resourceDirName: "voice"
                            };
                        case tr.a.VIDEO:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "video"), newFolderPattern: or.join(s, "*", "video", "*"), mediaRoot: s, resourceDirName: "video"
                            };
                        case tr.a.RICH_THUMB:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "richThumb"), newFolderPattern: or.join(s, "*", "richThumb", "*"), mediaRoot: s, resourceDirName: "richThumb"
                            };
                        case tr.a.FILE_THUMB:
                            return {
                                oldFolder: or.join(await Object(Ft.g)(t), "fileThumb"), newFolderPattern: or.join(s, "*", "fileThumb", "*"), mediaRoot: s, resourceDirName: "fileThumb"
                            };
                        default:
                            return this.logger.zsymb(6, "zGt2l-", `Unsupported resource type: ${e}. Only FOLDER, FILE, and FILE_NOISE are supported.`), null
                    }
                }
                async calculateChecksum(e) {
                    return $zFileManager.getMd5LargeFileObject(e)
                }
                async pathExists(e) {
                    return Object(rr.fileExistsSyncV2)(e)
                }
                createInodeKey(e) {
                    return Object(ar.a)({
                        dev: e.dev,
                        ino: e.inode
                    })
                }
                async tryGetStat(e) {
                    try {
                        return await nr.stat(e, {
                            bigint: !0
                        })
                    } catch {
                        return null
                    }
                }
                createEmptyMasterIndex() {
                    return {
                        byInode: new Map,
                        bySize: new Map,
                        total: 0
                    }
                }
                async buildMasterIndex(e, t) {
                    const s = new Map,
                        r = new Map;
                    let a = 0;
                    const i = or.join(e, "**", "*"),
                        o = await cr(i, {
                            objectMode: !0,
                            absolute: !0,
                            onlyFiles: !0,
                            suppressErrors: !0
                        });
                    for (const n of o) {
                        if (!n) continue;
                        const e = await this.tryGetStat(n.path);
                        if (!e || !Object(sr.a)(e)) continue;
                        const t = {
                                path: n.path,
                                size: e.size,
                                dev: e.dev,
                                inode: e.ino,
                                mtimeMs: e.mtimeMs,
                                birthtimeMs: e.birthtimeMs,
                                nlink: e.nlink
                            },
                            i = this.createInodeKey(t);
                        if (i && s.has(i)) continue;
                        i && s.set(i, t);
                        const o = r.get(Number(t.size));
                        o ? o.push(t) : r.set(Number(t.size), [t]), a++
                    }
                    return this.logger.zsymb(0, "HKg5kk", "Master index built", {
                        resourceType: t,
                        entryCount: a
                    }), {
                        byInode: s,
                        bySize: r,
                        total: a
                    }
                }
                async processResourceEntries(e, t, s, r) {
                    const a = {
                            totalFiles: 0,
                            alreadyLinked: 0,
                            matched: 0,
                            successfulRelinks: 0,
                            potentialRelinks: 0,
                            md5Comparisons: 0,
                            spaceSaved: 0
                        },
                        i = await cr(t.newFolderPattern, {
                            objectMode: !0,
                            absolute: !0,
                            onlyFiles: !0,
                            suppressErrors: !0
                        });
                    for (const o of i) {
                        if (!o) continue;
                        const t = await this.tryGetStat(o.path);
                        if (!t || !Object(sr.a)(t)) continue;
                        const i = {
                            path: o.path,
                            size: t.size,
                            dev: t.dev,
                            inode: t.ino,
                            mtimeMs: t.mtimeMs,
                            birthtimeMs: t.birthtimeMs,
                            nlink: t.nlink
                        };
                        a.totalFiles++;
                        const n = await this.handleResourceFile(i, s, e, r);
                        n.alreadyLinked && a.alreadyLinked++, n.candidateMatched && (a.matched++, a.potentialRelinks++), n.linked ? (a.successfulRelinks++, n.spaceSaved && (a.spaceSaved += n.spaceSaved)) : r.dryRun && n.candidateMatched && n.spaceSaved && (a.spaceSaved += n.spaceSaved), n.usedMd5 && a.md5Comparisons++
                    }
                    return a
                }
                preferByMetadata(e, t) {
                    const s = [];
                    for (const r of e) this.isMetadataClose(r, t) && s.push(r);
                    return s.length > 0 ? s : e
                }
                isMetadataClose(e, t) {
                    return "number" == typeof e.mtimeMs && "number" == typeof t.mtimeMs && Math.abs(e.mtimeMs - t.mtimeMs) <= 2e3 || "number" == typeof e.birthtimeMs && "number" == typeof t.birthtimeMs && Math.abs(e.birthtimeMs - t.birthtimeMs) <= 2e3
                }
                async handleResourceFile(e, t, s, r) {
                    const a = this.createInodeKey(e);
                    if (a && t.byInode.has(a)) return {
                        alreadyLinked: !0
                    };
                    const i = t.bySize.get(Number(e.size));
                    if (!i || 0 === i.length) return {};
                    let o = this.preferByMetadata(i, e);
                    if (o.length > 1) {
                        const t = await this.getPartialFingerprint(e);
                        if (t) {
                            e.partialHash = t;
                            const s = [];
                            for (const e of o) {
                                const r = await this.ensurePartialHash(e);
                                r && r === t && s.push(e)
                            }
                            s.length > 0 && (o = s)
                        }
                    }
                    const n = await this.ensureChecksumForFile(e);
                    if (!n) return {};
                    let c = null,
                        l = 0,
                        d = !1;
                    for (const g of o) {
                        if (await this.ensureMasterChecksum(g) === n) {
                            if (l++, c) {
                                d = !0;
                                break
                            }
                            c = g
                        }
                    }
                    if (0 === l || !c) return {
                        usedMd5: !0
                    };
                    if (d) return this.logger.zsymb(6, "OaldUG", "Multiple master candidates share identical checksum", {
                        filePath: e.path,
                        resourceType: s,
                        candidateCount: l
                    }), {
                        usedMd5: !0,
                        candidateMatched: !0
                    };
                    const u = c,
                        h = Number(e.size);
                    if (r.dryRun) return {
                        candidateMatched: !0,
                        usedMd5: !0,
                        linked: !1,
                        spaceSaved: h
                    };
                    return await this.relinkFile(u, e) ? {
                        candidateMatched: !0,
                        linked: !0,
                        spaceSaved: h,
                        usedMd5: !0
                    } : {
                        candidateMatched: !0,
                        usedMd5: !0,
                        linked: !1
                    }
                }
                async ensurePartialHash(e) {
                    if (e.partialHash) return e.partialHash;
                    const t = await this.getPartialFingerprint(e);
                    return t && (e.partialHash = t), t
                }
                async ensureChecksumForFile(e) {
                    if (e.checksum) return e.checksum;
                    try {
                        return e.checksum = await this.calculateChecksum(e.path), e.checksum
                    } catch (t) {
                        return this.logger.zsymb(18, "qOztbK", "Failed to calculate checksum for file", {
                            path: e.path,
                            error: t
                        }), null
                    }
                }
                async ensureMasterChecksum(e) {
                    if (e.checksum) return e.checksum;
                    try {
                        return e.checksum = await this.calculateChecksum(e.path), e.checksum
                    } catch (t) {
                        return this.logger.zsymb(18, "oVv6n-", "Failed to calculate checksum for master file", {
                            path: e.path,
                            error: t
                        }), null
                    }
                }
                async getPartialFingerprint(e) {
                    try {
                        return await $zFileManager.getPartialFingerprint(e.path)
                    } catch (t) {
                        return this.logger.zsymb(6, "S6pxID", "Failed to compute partial fingerprint", {
                            path: e.path,
                            error: t
                        }), null
                    }
                }
                async relinkFile(e, t) {
                    const s = t.path,
                        r = `${s}.${Date.now()}.bak`;
                    let a = !1;
                    try {
                        await hr(s, r), a = !0
                    } catch (o) {
                        return this.logger.zsymb(18, "-TGtI_", "Failed to backup file before relinking", {
                            duplicatePath: s,
                            error: o
                        }), !1
                    }
                    try {
                        const t = await this.resourceLinkManager.linkFile(e.path, s, {
                            fallbackStrategy: "copy"
                        });
                        if (t.success) {
                            var i;
                            this.logger.zsymb(0, "8pt8nC", "Created hardlink", {
                                source: e.path,
                                target: s
                            }), a && await ur(r).catch((e => this.logger.zsymb(6, "_oaEHc", "Failed to remove backup after relink", {
                                backupPath: r,
                                error: e
                            })));
                            const t = null !== (i = e.nlink) && void 0 !== i ? i : BigInt(1);
                            return e.nlink = t + BigInt(1), !0
                        }
                        return this.logger.zsymb(18, "SX7yth", "Failed to create hardlink", {
                            source: e.path,
                            target: s,
                            error: t.error,
                            reason: t.reason
                        }), a && await this.restoreBackup(r, s), !1
                    } catch (o) {
                        return this.logger.zsymb(18, "yUNZKC", "Unexpected error during relink", {
                            source: e.path,
                            target: s,
                            error: o
                        }), a && await this.restoreBackup(r, s), !1
                    }
                }
                async restoreBackup(e, t) {
                    try {
                        await dr(t).then((() => !0)).catch((() => !1)) && await ur(t).catch((() => {})), await hr(e, t)
                    } catch (s) {
                        this.logger.zsymb(18, "cVx5bj", "Failed to restore backup", {
                            backupPath: e,
                            targetPath: t,
                            error: s
                        })
                    }
                }
                async analyzePotentialSavings(e) {
                    try {
                        const t = e.filter((e => gr.includes(e)));
                        let s = 0,
                            r = 0,
                            a = 0;
                        for (const e of t) {
                            const t = await this.getFolderPathsByResourceType(e);
                            if (!t) continue;
                            const i = await this.pathExists(t.oldFolder) ? await this.buildMasterIndex(t.oldFolder, e) : this.createEmptyMasterIndex(),
                                o = await this.processResourceEntries(e, t, i, {
                                    dryRun: !0
                                });
                            s += i.total + o.totalFiles, r += o.potentialRelinks, a += o.spaceSaved, this.logger.zsymb(0, "00R738", "Analysis stats", {
                                resourceType: e,
                                totalFiles: o.totalFiles,
                                matched: o.matched,
                                potentialRelinks: o.potentialRelinks,
                                potentialSpaceSaved: o.spaceSaved
                            }), i.byInode.clear(), i.bySize.clear(), i.total = 0
                        }
                        return {
                            totalFiles: s,
                            duplicateGroups: r,
                            potentialSpaceSaved: a
                        }
                    } catch (t) {
                        throw this.logger.zsymb(18, "6gTfgv", "Error analyzing potential savings", t), t
                    }
                }
            }
            var pr;
            mr.instance = void 0;
            Object(y.j)()(pr = Object(l.injectable)()(pr = Reflect.metadata("design:type", Function)(pr = Reflect.metadata("design:paramtypes", [])(pr = class extends q.a {
                constructor() {
                    super()
                }
                getType() {
                    return "REBUILD_LINK"
                }
                async execute(e) {
                    const {
                        report: t,
                        params: s
                    } = e;
                    try {
                        return await mr.getInstance().rebuildFileLinks(s.types), t(100), !0
                    } catch (r) {
                        return !1
                    }
                }
            }) || pr) || pr) || pr);
            var br, fr, yr, Ir, Sr, vr = s("Y65e"),
                Mr = s("7yrd"),
                wr = s("mHfx"),
                _r = s("kAC1"),
                Cr = s("vfyE"),
                Er = s("zb/h");
            const Tr = "renderer" === __SCRIPT_TYPE__ ? $znode.path : s("ipWf"),
                Or = "renderer" === __SCRIPT_TYPE__ ? $znode.fsPromise : s("cSWM").promises;
            let Rr = (br = Object(_r.d)("clean-old-structure"), fr = Reflect.metadata("design:type", Function), yr = Reflect.metadata("design:paramtypes", [String, "undefined" == typeof AbortSignal ? Object : AbortSignal]), (Sr = class e {
                constructor() {
                    this.logger = void 0, this.progress = void 0, this.abortSignal = void 0, this.progressSaveInterval = 1e3, this.progressSaveCounter = 0, this.resourceDirExistenceCache = new Map, this.resourceInodeSet = void 0, this.mediaTypesToCleanup = [{
                        type: tr.a.VOICE,
                        folderName: "voice"
                    }, {
                        type: tr.a.VIDEO,
                        folderName: "video"
                    }, {
                        type: tr.a.RICH_THUMB,
                        folderName: "richThumb"
                    }, {
                        type: tr.a.FILE_THUMB,
                        folderName: "fileThumb"
                    }, {
                        type: tr.a.CACHE,
                        folderName: "Cache"
                    }, {
                        type: tr.a.PHOTO,
                        folderName: "picture"
                    }], this.logger = Object(Qs.a)("old-structure-cleanup-core"), this.progress = {
                        currentMediaType: null,
                        completedRootFolders: new Set,
                        currentFolder: null,
                        stats: {
                            totalFilesScanned: 0,
                            filesDeleted: 0,
                            filesSkipped: 0,
                            errors: 0
                        },
                        isComplete: !1,
                        statsBaseline: void 0,
                        currentRootId: void 0
                    }
                }
                static getInstance() {
                    return this.instance || (this.instance = new e), this.instance
                }
                async cleanupOldStructure(e, t) {
                    this.abortSignal = t;
                    const s = Date.now();
                    try {
                        var r;
                        if (this.logger.zsymb(0, "KGIxub", "Starting old structure cleanup", {
                                userId: e
                            }), await this.loadProgress(), null !== (r = this.abortSignal) && void 0 !== r && r.aborted) throw this.logger.zsymb(0, "QrLU6m", "Cleanup aborted before starting"), new Error("Cleanup aborted");
                        const t = await Object(Ft.e)(e),
                            i = await Object(Ft.g)(e);
                        if (!(await Object(Cr.a)(t))) return this.logger.zsymb(6, "-DVjN6", "Resource directory does not exist, skipping cleanup", {
                            resourceDir: t
                        }), this.progress.stats;
                        this.logger.zsymb(0, "GxeSVH", "Cleanup directories", {
                            downloadsDir: i,
                            resourceDir: t
                        }), await this.buildResourceInodeSet(t);
                        for (const s of this.mediaTypesToCleanup) {
                            var a;
                            if (null !== (a = this.abortSignal) && void 0 !== a && a.aborted) throw this.logger.zsymb(0, "BoRK4S", "Cleanup aborted during processing"), new Error("Cleanup aborted");
                            if (this.progress.isComplete) {
                                this.logger.zsymb(0, "S1RYME", "Cleanup already completed, skipping");
                                break
                            }
                            this.progress.currentMediaType = s.type, await this.scheduleProgressSave(!0), await this.processMediaType(e, s, t)
                        }
                        return this.progress.isComplete = !0, await this.scheduleProgressSave(!0), Js.RestructureMediaQos.getInstance().logOldStructureCleanupSuccess(Date.now() - s), this.logger.zsymb(0, "c2QvWY", "Old structure cleanup completed", this.progress.stats), this.progress.stats
                    } catch (i) {
                        throw this.logger.zsymb(18, "NxvH0e", "Error during old structure cleanup", i), Js.RestructureMediaQos.getInstance().logOldStructureCleanupFailed(Date.now() - s), i
                    } finally {
                        this.dispose()
                    }
                }
                async processMediaType(e, t, s) {
                    try {
                        const r = await Object(Ft.g)(e),
                            a = Tr.join(r, t.folderName);
                        if (this.logger.zsymb(0, "FtTki9", `Processing media type: ${t.folderName}`, {
                                mediaTypeDir: a
                            }), await this.ensureBaselineForRoot(a), this.progress.completedRootFolders.has(a)) return this.logger.zsymb(0, "AgTq9b", "Media type directory already completed, skipping", {
                            mediaTypeDir: a
                        }), void(await this.clearCurrentRootTracking(a));
                        if (!(await Object(Cr.a)(a))) return this.logger.zsymb(0, "H8s-Yb", "Media type directory does not exist, skipping", {
                            mediaTypeDir: a
                        }), void(await this.clearCurrentRootTracking(a));
                        await this.processDirectoryStreaming(a, s, r, t), this.progress.completedRootFolders.add(a), await this.clearCurrentRootTracking(a, !1), await this.scheduleProgressSave(!0), this.logger.zsymb(0, "lpuMQI", `Completed media type: ${t.folderName}`)
                    } catch (r) {
                        this.logger.zsymb(18, "jmYTP9", `Error processing media type ${t.folderName}`, r), this.progress.stats.errors++, await this.scheduleProgressSave(!0)
                    }
                }
                async processDirectoryStreaming(e, t, s, r, a = !0) {
                    var i;
                    if (null !== (i = this.abortSignal) && void 0 !== i && i.aborted) throw this.logger.zsymb(0, "LmJwuM", "Directory processing aborted"), new Error("Cleanup aborted");
                    try {
                        (a || this.progress.currentFolder !== e) && (this.progress.currentFolder = e, this.logger.zsymb(0, "i5WUQz", `Starting folder: ${e}`), await this.scheduleProgressSave(!1, 1));
                        const i = await Or.readdir(e, {
                            withFileTypes: !0
                        });
                        let c = 0;
                        for (const a of i) {
                            var o;
                            if (null !== (o = this.abortSignal) && void 0 !== o && o.aborted) throw this.logger.zsymb(0, "LYjrkY", "Directory scanning aborted"), new Error("Cleanup aborted");
                            const i = Tr.join(e, a.name);
                            try {
                                const o = await Or.stat(i);
                                if (Object(wr.a)(o)) await this.processDirectoryStreaming(i, t, s, r, !1);
                                else if (Object(sr.a)(o)) {
                                    c++;
                                    ".rescache" === a.name.toLowerCase() ? await this.deleteRescacheFile(i) : 1 === o.nlink ? (this.progress.stats.totalFilesScanned++, this.progress.stats.filesSkipped++) : await this.processFile(i, t), c % 5e3 == 0 && this.logger.zsymb(0, "vBkXn2", `Progress: ${c} files queued from ${e}`)
                                }
                            } catch (n) {
                                this.logger.zsymb(6, "rpjw9F", `Error accessing ${i}`, n)
                            }
                        }
                        await this.cleanupFolderIfEligible(e, a), this.progress.currentFolder = null, this.logger.zsymb(0, "ywvfHc", `Completed folder: ${e} (${c} files)`), await this.scheduleProgressSave(!1, 1)
                    } catch (n) {
                        throw this.logger.zsymb(18, "_93U0j", `Error processing directory ${e}`, n), n
                    }
                }
                async processFile(e, t) {
                    try {
                        var s;
                        if (null !== (s = this.abortSignal) && void 0 !== s && s.aborted) return;
                        this.progress.stats.totalFilesScanned++;
                        await this.checkHardlinkInResource(e, t) ? (await this.deleteFile(e), this.progress.stats.filesDeleted++) : this.progress.stats.filesSkipped++, await this.scheduleProgressSave()
                    } catch (r) {
                        this.logger.zsymb(18, "vvzZyd", `Error processing file: ${e}`, r), this.progress.stats.errors++
                    }
                }
                async checkHardlinkInResource(e, t) {
                    try {
                        if (this.resourceInodeSet && this.isMacOS()) {
                            const t = await Or.stat(e, {
                                bigint: !0
                            });
                            if (("bigint" == typeof t.nlink ? Number(t.nlink) : t.nlink) <= 1) return !1;
                            const s = Object(ar.a)(t);
                            return this.resourceInodeSet.has(s)
                        }
                        return (await Mr.a.detectHardlinksAsync(e, t)).some((e => {
                            const s = Tr.resolve(e).toLowerCase(),
                                r = Tr.resolve(t).toLowerCase();
                            return s.includes(r)
                        }))
                    } catch (s) {
                        const t = s.message;
                        return t.includes("The system cannot find the file specified") || t.includes("has only one link") || this.logger.zsymb(18, "tbsADm", `Error detecting hardlinks for ${e}`, s), !1
                    }
                }
                async deleteFile(e) {
                    try {
                        await Or.rm(e, {
                            force: !0,
                            recursive: !0,
                            maxRetries: 3
                        })
                    } catch (t) {
                        throw this.logger.zsymb(6, "kmKOpS", `Error deleting file: ${e}`, t), t
                    }
                }
                async deleteRescacheFile(e) {
                    this.progress.stats.totalFilesScanned++;
                    try {
                        await this.deleteFile(e), this.progress.stats.filesDeleted++
                    } catch (t) {
                        this.logger.zsymb(6, "DFcJtW", `Error deleting .rescache file: ${e}`, t), this.progress.stats.errors++
                    }
                    await this.scheduleProgressSave()
                }
                async cleanupFolderIfEligible(e, t) {
                    var s;
                    if (t || null !== (s = this.abortSignal) && void 0 !== s && s.aborted) return;
                    if (await this.isFolderEmpty(e)) try {
                        await Or.rm(e, {
                            recursive: !0,
                            force: !0,
                            maxRetries: 1
                        }), this.logger.zsymb(0, "JlNtEx", `Deleted empty folder after cleanup: ${e}`)
                    } catch (r) {
                        this.logger.zsymb(6, "6Lq28d", `Unable to delete folder after cleanup: ${e}`, r)
                    }
                }
                async isFolderEmpty(e) {
                    try {
                        return 0 === (await Or.readdir(e, {
                            withFileTypes: !0
                        })).length
                    } catch (t) {
                        return this.logger.zsymb(6, "3qO1vU", `Unable to inspect folder for cleanup: ${e}`, t), !1
                    }
                }
                async scheduleProgressSave(e = !1, t = 1) {
                    if (e) return this.progressSaveCounter = 0, void(await this.saveProgress());
                    t <= 0 || (this.progressSaveCounter += t, this.progressSaveCounter >= this.progressSaveInterval && (this.progressSaveCounter = 0, await this.saveProgress()))
                }
                async ensureBaselineForRoot(e) {
                    this.progress.currentRootId === e && this.progress.statsBaseline || (this.progress.currentRootId = e, this.progress.statsBaseline = Object(a.a)({}, this.progress.stats), await this.scheduleProgressSave(!0))
                }
                async clearCurrentRootTracking(e, t = !0) {
                    this.progress.currentRootId === e && (this.progress.currentRootId = void 0, this.progress.statsBaseline = void 0, t && await this.scheduleProgressSave(!0))
                }
                async saveProgress() {
                    try {
                        const e = ve.default.getInstance();
                        if (!e.isReady) return void this.logger.zsymb(6, "AkP_GA", "SecureLocalStorage not ready, skipping save");
                        const t = {
                            currentMediaType: this.progress.currentMediaType,
                            completedRootFolders: Array.from(this.progress.completedRootFolders),
                            currentFolder: this.progress.currentFolder,
                            stats: this.progress.stats,
                            isComplete: this.progress.isComplete,
                            statsBaseline: this.progress.statsBaseline,
                            currentRootId: this.progress.currentRootId
                        };
                        e.setItemForCurrentUser(Ks.a, JSON.stringify(t))
                    } catch (e) {
                        this.logger.zsymb(18, "fFOIq3", "Error saving cleanup progress", e)
                    }
                }
                async loadProgress() {
                    try {
                        const e = ve.default.getInstance();
                        if (!e.isReady) return void this.logger.zsymb(6, "8gIuyK", "SecureLocalStorage not ready, starting fresh");
                        const t = e.getItemForCurrentUser(Ks.a);
                        if (!t) return void this.logger.zsymb(0, "x7dPLI", "No saved progress found, starting fresh");
                        const s = JSON.parse(t),
                            r = s.completedRootFolders || [];
                        if (this.progress = {
                                currentMediaType: s.currentMediaType,
                                completedRootFolders: new Set(r),
                                currentFolder: s.currentFolder || null,
                                stats: s.stats || this.progress.stats,
                                isComplete: s.isComplete || !1,
                                statsBaseline: s.statsBaseline,
                                currentRootId: s.currentRootId
                            }, this.progress.currentRootId && this.progress.statsBaseline) {
                            this.progress.completedRootFolders.has(this.progress.currentRootId) ? (this.progress.currentRootId = void 0, this.progress.statsBaseline = void 0) : (this.logger.zsymb(0, "YaSDSz", `Restoring stats to baseline for interrupted root: ${this.progress.currentRootId}`, this.progress.statsBaseline), this.progress.stats = Object(a.a)({}, this.progress.statsBaseline))
                        }
                        this.logger.zsymb(0, "AuusDa", "Loaded cleanup progress", Object(a.a)(Object(a.a)({}, this.progress.stats), {}, {
                            completedRootFoldersCount: this.progress.completedRootFolders.size
                        }))
                    } catch (e) {
                        this.logger.zsymb(18, "mDOBpm", "Error loading cleanup progress", e)
                    }
                }
                async resetProgress() {
                    this.progress = {
                        currentMediaType: null,
                        completedRootFolders: new Set,
                        currentFolder: null,
                        stats: {
                            totalFilesScanned: 0,
                            filesDeleted: 0,
                            filesSkipped: 0,
                            errors: 0
                        },
                        isComplete: !1,
                        statsBaseline: void 0,
                        currentRootId: void 0
                    }, await this.scheduleProgressSave(!0), this.logger.zsymb(0, "bM4Bis", "Cleanup progress reset")
                }
                dispose() {
                    this.progress = {
                        currentMediaType: null,
                        completedRootFolders: new Set,
                        currentFolder: null,
                        stats: {
                            totalFilesScanned: 0,
                            filesDeleted: 0,
                            filesSkipped: 0,
                            errors: 0
                        },
                        isComplete: !1,
                        statsBaseline: void 0,
                        currentRootId: void 0
                    }, this.abortSignal = void 0, this.progressSaveCounter = 0, this.resourceDirExistenceCache.clear(), this.resourceInodeSet = void 0, this.logger.zsymb(0, "4LNJQV", "OldStructureCleanupCore disposed")
                }
                getStats() {
                    return Object(a.a)({}, this.progress.stats)
                }
                isMacOS() {
                    return !0
                }
                async buildResourceInodeSet(e) {
                    var t, s;
                    if (!this.isMacOS()) return void(this.resourceInodeSet = void 0);
                    if (null !== (t = this.abortSignal) && void 0 !== t && t.aborted) return void this.logger.zsymb(0, "4kFabU", "Skipping inode set build due to abort");
                    const r = await this.buildResourceInodeSetByStream(e);
                    null !== (s = this.abortSignal) && void 0 !== s && s.aborted || (r ? this.resourceInodeSet = r : await this.buildResourceInodeSetByFs(e))
                }
                async buildResourceInodeSetByStream(e) {
                    var t;
                    if ("renderer" !== __SCRIPT_TYPE__) return null;
                    if (null !== (t = this.abortSignal) && void 0 !== t && t.aborted) return null;
                    const s = new Set,
                        r = Date.now();
                    try {
                        return await new Promise(((t, a) => {
                            var i;
                            let o = !1;
                            const n = e => {
                                o || (o = !0, t(e))
                            };
                            Object(Er.a)({
                                rootDir: e,
                                includeStats: !0,
                                emitOnlyHardlinks: !1,
                                onlyFiles: !0
                            }, {
                                signal: null !== (i = this.abortSignal) && void 0 !== i ? i : void 0,
                                onChunk: e => {
                                    if (!o)
                                        for (const r of e)
                                            if (r.stat) try {
                                                const e = Object(ar.a)({
                                                    dev: BigInt(r.stat.dev),
                                                    ino: BigInt(r.stat.ino)
                                                });
                                                s.add(e)
                                            } catch (t) {
                                                this.logger.zsymb(6, "GqZoAq", "Unable to build inode key from stream entry", {
                                                    path: r.path,
                                                    error: t
                                                })
                                            }
                                },
                                onEnd: () => {
                                    var e;
                                    null !== (e = this.abortSignal) && void 0 !== e && e.aborted ? n(null) : (this.logger.zsymb(0, "16lKFR", "Built resource inode set via stream for macOS fast path", {
                                        count: s.size,
                                        durationMs: Date.now() - r
                                    }), n(s))
                                },
                                onError: e => {
                                    var t;
                                    null !== (t = this.abortSignal) && void 0 !== t && t.aborted ? n(null) : a(e)
                                }
                            }).catch((e => {
                                o || a(e)
                            }))
                        }))
                    } catch (a) {
                        return this.logger.zsymb(6, "RqPj4k", "Failed to build resource inode set via stream; falling back to fs scan", a), null
                    }
                }
                async buildResourceInodeSetByFs(e) {
                    const t = Date.now(),
                        s = new Set,
                        r = [e];
                    try {
                        for (; r.length > 0;) {
                            var a;
                            if (null !== (a = this.abortSignal) && void 0 !== a && a.aborted) return void this.logger.zsymb(0, "tfVoNm", "Aborting inode set build");
                            const e = r.pop();
                            let t;
                            try {
                                t = await Or.readdir(e, {
                                    withFileTypes: !0
                                })
                            } catch (i) {
                                this.logger.zsymb(6, "UQpZVw", `Unable to read directory while building inode set: ${e}`, i);
                                continue
                            }
                            for (const a of t) {
                                const t = Tr.join(e, a.name);
                                try {
                                    const e = await Or.stat(t, {
                                        bigint: !0
                                    });
                                    Object(wr.a)(e) ? r.push(t) : Object(sr.a)(e) && s.add(Object(ar.a)(e))
                                } catch (i) {
                                    this.logger.zsymb(6, "Nx-0f7", `Unable to stat entry while building inode set: ${t}`, i)
                                }
                            }
                        }
                        this.resourceInodeSet = s, this.logger.zsymb(0, "lKd4W9", "Built resource inode set for macOS fast path", {
                            count: s.size,
                            durationMs: Date.now() - t
                        })
                    } catch (i) {
                        this.logger.zsymb(6, "CbbmiK", "Failed to build resource inode set; falling back to default detection", i), this.resourceInodeSet = void 0
                    }
                }
            }).instance = void 0, Ir = Sr, Object(vr.a)(Ir.prototype, "cleanupOldStructure", [br, fr, yr], Object.getOwnPropertyDescriptor(Ir.prototype, "cleanupOldStructure"), Ir.prototype), Ir);
            var Dr;
            Object(y.j)()(Dr = Object(l.injectable)()(Dr = Reflect.metadata("design:type", Function)(Dr = Reflect.metadata("design:paramtypes", [])(Dr = class extends q.a {
                constructor() {
                    super()
                }
                getType() {
                    return "CLEANUP_OLD_STRUCTURE"
                }
                async execute(e) {
                    const {
                        params: t,
                        abort: s,
                        report: r
                    } = e, a = Rr.getInstance(), i = await a.cleanupOldStructure(t.userId, s);
                    return r(100), i
                }
            }) || Dr) || Dr) || Dr);
            var kr = s("BkRI"),
                Fr = s.n(kr),
                zr = s("B7OF");
            const jr = "renderer" === __SCRIPT_TYPE__ ? $znode.os : s("HWNe"),
                Nr = [ae.MSG_VIDEO, ae.MSG_CONTACT, ae.MSG_PHOTO, ae.MSG_PHOTO_2, ae.MSG_DOODLE, ae.MSG_FILE];
            class Lr {
                constructor() {
                    this.logger = void 0, this.migrator = void 0, this.migrationQueue = void 0, this.unsubscribe = void 0, this.subscribed = !1, this.logger = Object(Qs.a)("message-transfer-migration-service"), this.migrator = new Bs.a, this.migrationQueue = new Vs.a({
                        concurrency: jr.cpus().length || 4,
                        autoStart: !0
                    })
                }
                get isEnabled() {
                    return Ys.a.key("is_enable_migrate_for_msg_transfer").value && Ys.a.key("is_enable_migrate").value
                }
                async start() {
                    if (this.subscribed) return void this.logger.zsymb(0, "T_lRNU", "Message transfer migration already subscribed to events");
                    await this.waitLocalStorageReady();
                    await this.ensureEligibility() ? (this.subscribeToMessageTransferEvents(), this.logger.zsymb(0, "jETL9U", "Message transfer migration listener registered", {
                        eventType: we.a
                    })) : this.logger.zsymb(0, "YFa8B1", "Message transfer migration is not eligible")
                }
                stop() {
                    this.dispose()
                }
                dispose() {
                    this.unsubscribe && this.unsubscribe(), this.migrationQueue.clear(), this.subscribed = !1
                }
                async ensureEligibility() {
                    if (Object(Zs.isUsingPathStructureV2)()) return this.logger.zsymb(0, "z8QXoB", "New path structure is applied, force migration"), !0;
                    if (!this.isEnabled) return this.logger.zsymb(0, "-Sgj52", "Message transfer migration is disabled"), !1;
                    return !!(await Hs.a.getInstance().checkZaloPCDirHardlinkSupport()).supported || (this.logger.zsymb(0, "WcjaSh", "Media folder does not support hardlink, skipping transfer migration"), !1)
                }
                ensureMessageEligible(e) {
                    return !!this.isEnabled && Nr.includes(e.msgType)
                }
                subscribeToMessageTransferEvents() {
                    this.subscribed || (this.unsubscribe = Object(we.c)((e => {
                        this.logger.zsymb(0, "YQM7WU", "Received new messages", {
                            count: e.messages.length
                        }), this.handleNewMessages(e)
                    })), this.subscribed = !0)
                }
                async handleNewMessages(e) {
                    e.messages && 0 !== e.messages.length && await Object(zr.a)(e.messages.map((e => async () => {
                        this.ensureMessageEligible(e) && await this.queueMigration({
                            cliMsgId: e.cliMsgId,
                            msgId: e.msgId,
                            msgType: e.msgType,
                            toUid: e.toUid,
                            fromUid: e.fromUid,
                            message: Fr()(e.message)
                        }, e.toUid)
                    })), 10)
                }
                async queueMigration(e, t) {
                    const s = await this.migrator.getExistedResourcePaths(e, t);
                    if (0 === s.length) return;
                    const r = Date.now();
                    this.migrationQueue.add((async () => {
                        try {
                            0;
                            const a = [];
                            if (await Promise.allSettled(s.map((async e => {
                                    await Object(Cr.a)(e.newPath) || a.push(e)
                                }))), 0 === a.length) return void 0;
                            0, await this.migrator.migrateMessage(e, t, a), Js.RestructureMediaQos.getInstance().logMessageTransferMigrateSuccess(Date.now() - r)
                        } catch (a) {
                            this.logger.zsymb(18, "5z-8uK", "Message transfer migration failed", {
                                error: a,
                                messageId: e.msgId,
                                convId: t
                            }), Js.RestructureMediaQos.getInstance().logMessageTransferMigrateFailed("MIGRATE_FAILED", Date.now() - r)
                        }
                    })).catch((s => {
                        this.logger.zsymb(18, "8GhQCv", "Failed to enqueue message transfer migration", {
                            error: s,
                            messageId: e.msgId,
                            convId: t
                        }), Js.RestructureMediaQos.getInstance().logMessageTransferMigrateFailed("ENQUEUE_FAILED", Date.now() - r)
                    }))
                }
                async waitLocalStorageReady() {
                    const e = ve.default.getInstance();
                    e.isReady || await e.waitForReady()
                }
            }
            var Ar;
            Object(y.j)()(Ar = Reflect.metadata("design:type", Function)(Ar = Reflect.metadata("design:paramtypes", [])(Ar = class {
                constructor() {
                    this.logger = void 0, this.migrationServiceCore = void 0, this.logger = Object(Qs.a)("message-transfer-migration-service"), this.migrationServiceCore = new Lr
                }
                get isEnabled() {
                    return Ys.a.key("is_enable_migrate_for_msg_transfer").value && Ys.a.key("is_enable_migrate").value
                }
                async onStart() {
                    let e = this.isEnabled;
                    Ys.a.on((() => {
                        const t = this.isEnabled;
                        t !== e && (this.logger.zsymb(0, "FE1wTx", "Config changed", t), e = t, t ? this.startMigrationService() : this.stopMigrationService())
                    })), this.startMigrationService()
                }
                async startMigrationService() {
                    try {
                        if (!this.isEnabled) return void this.logger.zsymb(0, "lEuxo2", "Message transfer migration is disabled");
                        await this.migrationServiceCore.start()
                    } catch (e) {
                        this.logger.zsymb(18, "YipLAg", "Message transfer migration failed to start", e)
                    }
                }
                stopMigrationService() {
                    this.migrationServiceCore.stop()
                }
            }) || Ar) || Ar);
            var Pr, Ur = s("S8fy"),
                xr = s("9LCO"),
                Gr = s("YgrE");
            let $r = Object(y.j)()(Pr = class {
                constructor() {
                    this.moduleContainer = Ur.c.createChildContainer()
                }
                onStart() {
                    this.init()
                }
                get(e) {
                    return this.moduleContainer.resolve(e.token)
                }
                getAll(e) {
                    return !1 === this.moduleContainer.isRegistered(e.token, !0) ? [] : this.moduleContainer.resolveAll(e.token)
                }
                async init() {
                    try {
                        await l.ModuleContainer.resolve(Gr.a).load()
                    } catch (e) {
                        zconsole.zsymb(21, "8zaZ2o", ["ZInstantModuleLoader init error:", "Azx_l8"], e)
                    }
                }
            }) || Pr;
            l.ModuleContainer.registerSingleton(xr.a, $r);
            l.ModuleContainer.registerSingleton(Gr.a, class {
                async load() {
                    await this.loadLazyModule()
                }
                async loadLazyModule() {
                    await Promise.all([s.e(18), s.e(58)]).then(s.bind(null, "B3XN"))
                }
            });
            var Br = s("VHyX"),
                Qr = s("/oxa"),
                qr = s("iA9X"),
                Vr = s("t9fU"),
                Zr = s("GVt/"),
                Hr = s("u4F8"),
                Kr = s("dHiD");
            class Yr extends Zr.a {
                constructor() {
                    super(), this.minSendDttmMessage = Qr.a.getMinMsgMissQueue()
                }
                get config() {
                    return Qr.a.getCloudScanConfig()
                }
                get subscriptionTime() {
                    var e, t;
                    return (null === (e = this.config) || void 0 === e || null === (t = e.cloudInfo) || void 0 === t ? void 0 : t.subscription_time) || -1
                }
                push(e) {
                    var t;
                    if ((null == e ? void 0 : e.constructor) !== Array) return;
                    if (Vr.a.getInstance().isFreev2()) return;
                    if (this.subscriptionTime <= 0) return;
                    if (!e || e.length <= 0) return;
                    const s = e => Number(null != e ? e : NaN),
                        r = e.filter((e => {
                            if (!Hr.a.includes(e.msgType || 0)) return !1;
                            const t = s(e.sendDttm);
                            return e.status !== ae.MSG_FAIL && e.status !== ae.MSG_LOCAL && Number.isFinite(t) && t > this.subscriptionTime
                        }));
                    if (!r || r.length <= 0) return void Object(Br.c)("❌ No valid messages", r);
                    const a = r.reduce(((e, t) => s(e.sendDttm) <= s(t.sendDttm) ? e : t), r[0]),
                        i = s(a.sendDttm),
                        o = s(null === (t = this.minSendDttmMessage) || void 0 === t ? void 0 : t.sendDttm);
                    if ((!Number.isFinite(o) || !this.minSendDttmMessage || i < o) && (Kr.c.failed(Kr.b.PushEventLifecycle, Kr.a.DetectMissQueue, [a.cliMsgId || "", a.toUid || "", i + "", "detect_miss_queue"]), this.minSendDttmMessage = a, Object(qr.d)("⚠️ Update miss queue message", this.minSendDttmMessage), Qr.a.setMinMsgMissQueue(this.minSendDttmMessage), a.cliMsgId && a.toUid)) {
                        const e = Qr.a.getAuditQueueMsgInfo();
                        e.push({
                            cliMsgId: a.cliMsgId,
                            toUid: a.toUid,
                            fromUid: a.fromUid || "0",
                            sendDttm: parseInt(a.sendDttm || "0")
                        }), Qr.a.setAuditQueueMsgInfo(e)
                    }
                }
            }
            var Wr = s("bWos"),
                Jr = s("P6F4"),
                Xr = s("xiOp"),
                ea = s("UwMY");
            const ta = {
                local_file_size: 0,
                local_file_count: 0,
                local_file_size_min: 0,
                local_file_size_max: 0,
                local_voice_size: 0,
                local_voice_count: 0,
                local_voice_size_min: 0,
                local_voice_size_max: 0,
                local_photo_size: 0,
                local_photo_count: 0,
                local_photo_size_min: 0,
                local_photo_size_max: 0,
                local_video_size: 0,
                local_video_count: 0,
                local_video_size_min: 0,
                local_video_size_max: 0,
                cloud_file_size: 0,
                cloud_file_count: 0,
                cloud_file_size_min: 0,
                cloud_file_size_max: 0,
                cloud_voice_size: 0,
                cloud_voice_count: 0,
                cloud_voice_size_min: 0,
                cloud_voice_size_max: 0,
                cloud_photo_size: 0,
                cloud_photo_count: 0,
                cloud_photo_size_min: 0,
                cloud_photo_size_max: 0,
                cloud_video_size: 0,
                cloud_video_count: 0,
                cloud_video_size_min: 0,
                cloud_video_size_max: 0,
                cloud_local_file_size: 0,
                cloud_local_file_count: 0,
                cloud_local_file_size_min: 0,
                cloud_local_file_size_max: 0,
                cloud_local_voice_size: 0,
                cloud_local_voice_count: 0,
                cloud_local_voice_size_min: 0,
                cloud_local_voice_size_max: 0,
                cloud_local_photo_size: 0,
                cloud_local_photo_count: 0,
                cloud_local_photo_size_min: 0,
                cloud_local_photo_size_max: 0,
                cloud_local_video_size: 0,
                cloud_local_video_count: 0,
                cloud_local_video_size_min: 0,
                cloud_local_video_size_max: 0
            };
            class sa {
                constructor(e) {
                    this.cache = Object(a.a)({}, ta), e && (this.cache = Object(a.a)({}, e))
                }
                push(e, t = "local") {
                    if (!e) return;
                    let {
                        msgType: s,
                        type: r
                    } = e;
                    "image" === r && (r = "photo");
                    let a = r || "file";
                    if (!r) switch (s) {
                        case ae.MSG_FILE:
                            a = "file";
                            break;
                        case ae.MSG_DOODLE:
                        case ae.MSG_PHOTO:
                        case ae.MSG_PHOTO_2:
                            a = "photo";
                            break;
                        case ae.MSG_VIDEO:
                            a = "video";
                            break;
                        case ae.MSG_VOICE:
                            a = "voice";
                            break;
                        default:
                            return
                    }
                    let i = this.cache[`${t}_${a}_size`] || 0,
                        o = this.cache[`${t}_${a}_count`] || 0,
                        n = this.cache[`${t}_${a}_size_max`] || 0,
                        c = this.cache[`${t}_${a}_size_min`] || 0,
                        l = e.size || 0;
                    if ("local" === t) l = Wr.a.getFileSizeFromMessage(e);
                    i += l, o++, n = Math.max(n, l), c = c ? Math.min(c, l) : l, this.cache[`${t}_${a}_size`] = i, this.cache[`${t}_${a}_count`] = o, this.cache[`${t}_${a}_size_max`] = n, this.cache[`${t}_${a}_size_min`] = c
                }
                export () {
                    return this.cache
                }
                reset() {
                    this.cache = Object(a.a)({}, ta)
                }
            }
            let ra = 0;
            class aa {
                constructor() {
                    this.taskQueue = [], this.logTransformer = void 0, this.isRunning = !1, this.logConfig = {
                        firstLocalItem: null
                    }, this.processedConvs = [], this.exportResolver = null, this.logTransformer = new sa
                }
                get config() {
                    return Qr.a.getCloudScanConfig()
                }
                request(e) {
                    Object(Br.d)(`make request, data.range: ${e.range}, data.noiseId: ${e.noiseId}, data.offset: ${e.offset}, data.msgs.length: ${e.msgs.length}, data.convId: ${e.convId}`), this.taskQueue.push(e), this.run()
                }
                async run() {
                    if (this.isRunning) return;
                    if (this.taskQueue.length <= 0) return void this.reset();
                    this.isRunning = !0, Object(Br.d)("Run...", this.taskQueue.length);
                    let e = this.taskQueue.shift();
                    if (!e) return this.isRunning = !1, void this.run();
                    try {
                        let {
                            msgs: t,
                            convId: s,
                            range: r,
                            noiseId: a,
                            offset: i
                        } = e;
                        await this.executeTask(t, s, r, a, i)
                    } catch (t) {
                        throw Object(Br.c)("Runner: run", t), t
                    } finally {
                        this.isRunning = !1, this.run()
                    }
                }
                async executeTask(e, t, s, r = "", a = 0) {
                    if (!e || !t || e.length <= 0) return;
                    this.processedConvs.includes(t) || this.processedConvs.push(t);
                    const {
                        userId: i
                    } = this.config, [o = 0, n = 0] = s, c = await async function(e, t, s, r) {
                        try {
                            const a = w.default.getInstance().Core.ZCloud,
                                i = {
                                    from: [e, t, r],
                                    to: [e, s, Hr.i],
                                    excludeFrom: !1,
                                    excludeTo: !1
                                },
                                o = {
                                    index: ea.m.CONV_DATE,
                                    direction: ut.c.NEXT
                                };
                            return a.getAll(i, o)
                        } catch (a) {
                            throw a
                        }
                    }(t, o, n, r), l = ra++;
                    Object(Br.d)("execute", l, {
                        convId: t,
                        from: o,
                        to: n,
                        noiseId: r,
                        offset: a,
                        clouds: c,
                        msgs: e
                    });
                    let {
                        msgonly: d = [],
                        cloudonly: u = [],
                        both: h = []
                    } = await async function(e, t, s) {
                        const r = await async function(e) {
                            return new Map(e.map((e => [e.zKey, e])))
                        }(t), a = await async function(e, t) {
                            return new Set(e.map((e => ia(e, t))))
                        }(e, s), [i, o] = await async function(e, t, s) {
                            const r = [],
                                a = [];
                            for (const i of e) {
                                const e = ia(i, s);
                                t.has(e) ? a.push({
                                    msgInfo: i,
                                    cloudInfo: t.get(e)
                                }) : r.push(i)
                            }
                            return [r, a]
                        }(e, r, s), n = await async function(e, t) {
                            return e.filter((e => !t.has(e.zKey)))
                        }(t, a);
                        return {
                            msgonly: i,
                            cloudonly: n,
                            both: o
                        }
                    }(e, c, i);
                    d.length > 0 && (this.logMsgOnly(d), Yr.getInstance().push(d)), u.length > 0 && this.logCloudOnly(u), h.length > 0 && this.logBoth(h), Object(Br.d)("execute: analyse result", l, {
                        msgonly: d,
                        cloudonly: u,
                        both: h
                    })
                }
                async logMsgOnly(e) {
                    if ((null == e ? void 0 : e.length) > 0)
                        for (let u = 0; u < e.length; u++) {
                            let h = e[u];
                            if (h) {
                                var t, s, r, i;
                                const e = (null === (t = this.config) || void 0 === t || null === (s = t.uploadConfig) || void 0 === s ? void 0 : s.sendToMeId) || "";
                                if (e && (h.toUid === e || h.userId === e)) continue;
                                let u = await new Xr.a(h, null === (r = this.config) || void 0 === r ? void 0 : r.userId).runWithAsync(),
                                    g = 1 * h.sendDttm;
                                var o, n, c, l, d;
                                if (Date.now() - g > ((null === (i = this.config) || void 0 === i ? void 0 : i.uploadConfig.milestone) || 0))
                                    if (u) Object(Br.d)("✅ Msg này có vẻ UPLOAD được", null == h ? void 0 : h.msgId), Jr.a.add(Object(a.a)(Object(a.a)({}, h), {}, {
                                        sendDttm: g
                                    }), null === (o = this.config) || void 0 === o || null === (n = o.uploadConfig) || void 0 === n ? void 0 : n.enable, "bg");
                                    else Object(Br.d)("💀 Msg này có vẻ phù hợp upload nhưng không có localpath", null == h ? void 0 : h.msgId);
                                else Object(Br.d)("✅ Msg này có thể MIGRATE được", null == h ? void 0 : h.msgId), Jr.a.add(Object(a.a)(Object(a.a)({}, h), {}, {
                                    sendDttm: g
                                }), null === (c = this.config) || void 0 === c || null === (l = c.uploadConfig) || void 0 === l ? void 0 : l.enable, "bg");
                                if (u) this.logConfig.firstLocalItem ? h.sendDttm < this.logConfig.firstLocalItem.sendDttm && (this.logConfig.firstLocalItem = h) : this.logConfig.firstLocalItem = h, this.logTransformer.push(Object(a.a)(Object(a.a)({}, h), {}, {
                                    path: u,
                                    skipStatLocal: null === (d = this.config) || void 0 === d ? void 0 : d.skipStatLocal
                                }), "local")
                            }
                        }
                }
                logCloudOnly(e) {
                    (e || []).forEach((e => {
                        this.logTransformer.push(Object(a.a)(Object(a.a)({}, e), {}, {
                            msgType: Wr.a.normalizeMessageType(e.msgInfo.msgType),
                            size: e.mediaInfo.mediaSize || 0
                        }), "cloud")
                    }))
                }
                logBoth(e) {
                    for (let s = 0; s < e.length; s++) {
                        let {
                            msgInfo: r,
                            cloudInfo: i
                        } = e[s];
                        if (r) {
                            var t;
                            new Xr.a(r, null === (t = this.config) || void 0 === t ? void 0 : t.userId).run() ? this.logTransformer.push(Object(a.a)(Object(a.a)({}, r), {}, {
                                size: i.mediaInfo.mediaSize || 0
                            }), "cloud_local") : this.logTransformer.push(Object(a.a)(Object(a.a)({}, r), {}, {
                                msgType: r.msgType,
                                size: i.mediaInfo.mediaSize || 0
                            }), "cloud")
                        }
                    }
                }
                async export () {
                    return new Promise((e => {
                        this.exportResolver = e, this.taskQueue.length <= 0 && this.reset()
                    }))
                }
                reset() {
                    if (this.exportResolver) {
                        var e;
                        let t = null === (e = this.logTransformer) || void 0 === e ? void 0 : e.export();
                        t && this.exportResolver(t), this.exportResolver = null, this.logTransformer.reset()
                    }
                }
            }

            function ia(e, t) {
                if (!e) return "";
                let {
                    cliMsgId: s,
                    fromUid: r,
                    toUid: a
                } = e;
                return 0 == r && (r = t), 0 == a && (a = t), `${s}_${a}_${r}`
            }
            var oa;
            Object(y.j)()(oa = Object(Ur.e)()(oa = class extends q.a {
                getType() {
                    return "CLOUD_SCAN_V2"
                }
                async execute(e) {
                    return new Promise(((e, t) => {
                        Object(Br.d)("ScanTaskHandler listening...");
                        const s = new aa;
                        l.ModuleContainer.resolve(Bt.a).subscribe($t.a.cloud, {
                            onProgress: async e => {
                                Object(Br.d)("onProgress", e), Qr.a.setCloudScanState("processing");
                                const {
                                    data: t = [],
                                    convId: r,
                                    metadata: a,
                                    version: i
                                } = e;
                                Qr.a.getCloudScanVersion() !== i && (Object(Br.d)("onProgress: version changed", i), Qr.a.setScanLog(null)), Qr.a.setCloudScanVersion(i);
                                const {
                                    range: o
                                } = a || {
                                    batchSize: 0,
                                    range: [0, 0]
                                };
                                s.request({
                                    msgs: t,
                                    convId: r,
                                    range: o,
                                    noiseId: "",
                                    offset: 0
                                })
                            },
                            onConvSuccess: async e => {
                                Object(Br.d)("onConvSuccess", e)
                            },
                            onError: async e => {
                                Object(Br.c)("onError", e), Qr.a.setCloudScanState("error")
                            },
                            onComplete: async e => {
                                Object(Br.d)("onComplete", e);
                                let t = await s.export();
                                setTimeout((() => {
                                    Qr.a.setScanLog(t), Object(Br.d)("onComplete: export done", t), Qr.a.setCloudScanState("completed")
                                }), 500)
                            }
                        })
                    }))
                }
            }) || oa)
        },
        EXGp: function(e, t) {
            e.exports = require("../native/nativelibs")
        },
        O6t0: function(e, t, s) {
            "use strict";
            (function(e) {
                var t = s("c8uc"),
                    r = s("AsUd");
                e && e.env;
                const a = 12;
                let i = 0;

                function o() {
                    return new Promise(((e, s) => {
                        let r = t.a.isLoadedLib();
                        r && r.ok ? (c("load lib success!!"), $zdb.sendResponse({
                            act: a
                        }), $zdb.updateAvailableSqliteState(!0), n(!0, 91002, 0, 0, 3 == i ? 0 : 3, Date.now()), e(r)) : i > 0 ? (i--, c("load lib fail go retry " + i + " " + (r ? r.error : " _resp = null")), n(!1, 91002, 0, 0, 2, Date.now()), setTimeout((() => {
                            o()
                        }), 15e3)) : (c("load lib fail " + (r ? r.error : " _resp = null")), $zdb.updateAvailableSqliteState(!1), n(!1, 91002, 0, 0, 1, Date.now()), s(r))
                    }))
                }

                function n(e, t, s = 0, r, a, i) {
                    $zlogger.crossQosLog({
                        success: e,
                        commandId: t,
                        subCommandId: s,
                        duration: r,
                        errorCode: a,
                        startTime: i
                    })
                }

                function c(e) {
                    $zlogger.sendLog("info", "shared-worker: " + e)
                }! function() {
                    try {
                        const e = {
                                increaseSuccess: (e, t, s, r = [], a = {}) => {
                                    $zlogger.crossQosLog({
                                        success: !0,
                                        commandId: e,
                                        subCommandId: t,
                                        duration: s,
                                        params: r,
                                        options: a
                                    })
                                },
                                increaseFailed: (e, t, s, r, a, i = [], o = {}) => {
                                    $zlogger.crossQosLog({
                                        success: !1,
                                        commandId: e,
                                        subCommandId: t,
                                        duration: s,
                                        errorCode: r,
                                        startTime: a,
                                        params: i,
                                        options: o
                                    })
                                }
                            },
                            {
                                MetriczDriver: t
                            } = s("rkiK");
                        t.registerQosControl(e)
                    } catch (e) {}
                }(), $zsub.$zdb.processRequest(((e, s) => {
                    ! function(e) {
                        e && (e.constructor === Array ? e.forEach((e => {
                            t.a.receiverEvent(e)
                        })) : t.a.receiverEvent(e))
                    }(s)
                })), $zapp.onPing((() => Date.now())), $zlogger.onReceiveViewerkey(((e, t) => {
                    r.j("viewer_key", t)
                })), document.addEventListener("load", (() => {
                    o()
                })), c("[pid][shared-worker]" + e.pid), $zapp.registerProcess("dbtask", e.pid)
            }).call(this, s("8oxB"))
        },
        QiE6: function(e, t, s) {
            "use strict";
            var r = s("vyMb");
            s.d(t, "ResourceScannerManager", (function() {
                return r.a
            }));
            s("mP9p");
            var a = s("cMnt");
            s.d(t, "RESOURCE_LINKED_MSG_TYPES", (function() {
                return a.a
            }));
            var i = s("jWYK");
            s.o(i, "ResourceScannerExecutor") && s.d(t, "ResourceScannerExecutor", (function() {
                return i.ResourceScannerExecutor
            })), s.o(i, "ResourceScannerLog") && s.d(t, "ResourceScannerLog", (function() {
                return i.ResourceScannerLog
            }));
            var o = s("ZChn");
            s.d(t, "ResourceScannerLog", (function() {
                return o.a
            }));
            var n = s("39pz");
            s.d(t, "ResourceScannerExecutor", (function() {
                return n.a
            }))
        },
        ZChn: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return o
            })), s.d(t, "b", (function() {
                return n
            }));
            var r = s("jDHv"),
                a = s("h0S/"),
                i = s("Mgpg");
            class o {
                constructor() {}
                static getInstance() {
                    return this.instance || (this.instance = r.ModuleContainer.resolve(i.ZLoggerFactory).createZLogger(a.ZLoggerNametags.resourceUsageScanner, ["scan"])), this.instance
                }
            }
            o.instance = void 0;
            class n {
                constructor() {}
                static getInstance() {
                    return this.instance || (this.instance = r.ModuleContainer.resolve(i.ZLoggerFactory).createZLogger(a.ZLoggerNametags.resourceUsageScannerManagerV2, ["scan"])), this.instance
                }
            }
            n.instance = void 0
        },
        aEZA: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var r = s("jDHv");
            const a = Object(r.define)("resource-cleanup-manager")
        },
        c8uc: function(e, t, s) {
            "use strict";
            (function(e) {
                var r = s("VTBJ"),
                    a = s("ES/k"),
                    i = s("4prX"),
                    o = s("AtyM"),
                    n = s("Mgpg"),
                    c = s("jDHv"),
                    l = s("z0WU"),
                    d = s("bUXd"),
                    u = s("9aRL");
                e && e.env;
                let h = $zsqlite;
                const g = s("wBhU"),
                    m = c.ModuleContainer.resolve(n.ZLoggerFactory).createZLogger("dbtask", ["manager"]),
                    p = {
                        QUERY: 1,
                        ADD_DATA: 2,
                        PRE_DATA: 3,
                        INIT: 4,
                        GET_INDEX: 5,
                        ABORTED_SEARCH: 6,
                        MIGRATE: 7,
                        DEL_OLDDATA: 8,
                        DELETE_ALL: 9,
                        PRE_TAGGING: 10,
                        INIT_TAGGING: 11,
                        PROCESS_READY: 12,
                        QUERY_GLOBAL_MSG: 13,
                        CLOSE_DB: 14,
                        DELETE_DATA: 15,
                        PURE_QUERY: 18,
                        QUERY_GLOBAL_MSG_V3: 19,
                        COUNT_TOTAL: 20,
                        QUERY_GLOBAL_MSG_V4: 21,
                        QUERY_V2: 22,
                        DELETE_DATA_MULTI: 23
                    },
                    b = {
                        [p.QUERY]: function(e, t, s, i) {
                            return new Promise(((o, n) => {
                                let c;
                                N(e, i), c = t ? () => x(s, i) : () => !1;
                                let l = a.a.formatTextSearch(e);
                                if (l = l.trim(), !l) {
                                    let e = i.filter;
                                    if (!e || !0 !== e.searchForEmpty) return n("text is null")
                                }
                                let d = l.split(" ");
                                if ((i = i || {}).progress) {
                                    let e, t = !0,
                                        a = 0,
                                        l = i.configs && i.configs.limit ? i.configs.limit : V,
                                        u = i.configs && void 0 !== i.configs.offset ? i.configs.offset : 0,
                                        g = i.configs ? Object(r.a)({}, i.configs) : {},
                                        p = [];
                                    const b = () => {
                                            clearTimeout(e), e = null, o(p)
                                        },
                                        f = (o, u) => {
                                            if (c && c()) return n("aborted");
                                            h.query(d, i.filter, Object(r.a)(Object(r.a)({}, g), {}, {
                                                limit: u,
                                                offset: o
                                            }), c).then((r => {
                                                if (c && c()) return n("aborted");
                                                l -= u, r && r.constructor == Array ? (p.push.apply(p, r), r.length >= u && l > 0 ? (t ? (t = !1, a = p.length, P(p, s)) : e || (e = setTimeout((() => {
                                                    e = null, a < p.length && (a = p.length, P(p, s))
                                                }), 2e3)), f(o + r.length, Math.min(100, l))) : b()) : b()
                                            })).catch((e => {
                                                N("_doQuery", e), m.zsymb(18, "otDcuv", "_query fail", s, e), b()
                                            }))
                                        };
                                    f(u, Math.min(100, l))
                                } else i.configs && i.configs.limit || (i.configs = i.configs ? i.configs : {}, i.configs.limit = V, i.configs.offset = i.configs.offset || 0), o(h.query(d, i.filter, i.configs, c))
                            }))
                        },
                        [p.ADD_DATA]: function(e) {
                            return Promise.reject("not impl")
                        },
                        [p.INIT]: function(e) {
                            return new Promise(((t, s) => {
                                if (N(e), h.isLoadedLib(), !e.userId || !e.UIN) return s("invalid init data " + e.userId + " " + e.UIN);
                                if (Q) {
                                    if (Q.userId != e.userId || Q.UIN != e.UIN) return U("open new db last" + e.userId), Q = {
                                        userId: e.userId,
                                        UIN: e.UIN
                                    }, U("new Session!!"), f.length > 0 && (U("promote last task priority"), f.forEach((e => {
                                        e.priority = O
                                    }))), f.unshift({
                                        act: p.CLOSE_DB,
                                        taskId: y.next(),
                                        isAbort: !1,
                                        additions: null,
                                        priority: O,
                                        isMicro: !0
                                    }), f.unshift({
                                        act: p.INIT,
                                        data: e,
                                        taskId: y.next(),
                                        isAbort: !1,
                                        additions: null,
                                        priority: O,
                                        isMicro: !0
                                    }), t();
                                    if (!q) {
                                        U("open new db now!!!" + e.userId), q = {
                                            userId: e.userId,
                                            UIN: e.UIN
                                        };
                                        const s = Date.now(),
                                            r = o.a.now();
                                        return t(h.openDb(e.userId, e.UIN).catch((t => {
                                            throw g && e.onSentry && g.withScope((function(e) {
                                                e.setTag("db-error", "open"), e.setLevel("error"), g.captureException(t)
                                            })), i.default.increaseFailed(91002, 0, o.a.now() - r, 4, s), t.errno && i.default.increaseFailed(91002, 0, o.a.now() - r, 1e4 * t.errno + 4, s), t
                                        })))
                                    }
                                    return U("reopen the same with last" + e.userId), t()
                                }
                                Q = {
                                    userId: e.userId,
                                    UIN: e.UIN
                                }, U("open db first time now!!!" + e.userId), q = {
                                    userId: e.userId,
                                    UIN: e.UIN
                                }, t(h.openDb(e.userId, e.UIN, e.onSentry))
                            }))
                        },
                        [p.PRE_DATA]: function(e, t = !1, s, i, o) {
                            return N(e), new Promise(((t, n) => {
                                if (!e || 0 == e.length) return t();
                                if (e.length > w) {
                                    let t = e.slice(w, e.length);
                                    G({
                                        act: p.PRE_DATA,
                                        data: t,
                                        taskId: y.next(),
                                        isAbort: !1,
                                        additions: i,
                                        priority: o
                                    }), e = e.slice(0, w)
                                }
                                let c = [];
                                e.forEach((e => {
                                    if (!e || e.constructor !== Object) return;
                                    let t = a.a.getTextFromMessage(e.content);
                                    t && c.push(Object(r.a)(Object(r.a)({}, e), {}, {
                                        content: t
                                    }))
                                }));
                                const l = !1;
                                Date.now();
                                c.length ? h.writev2(c).then((() => {
                                    t()
                                })).catch((e => {
                                    m.zsymb(18, "pKPwc1", "predata fail", s, e), n(e)
                                })) : t()
                            }))
                        },
                        [p.GET_INDEX]: function(e) {
                            return Promise.reject("not impl")
                        },
                        [p.MIGRATE]: function(e) {
                            return Promise.reject("not impl")
                        },
                        [p.DEL_OLDDATA]: function() {
                            return Promise.reject("not impl")
                        },
                        [p.DELETE_ALL]: function() {
                            return Promise.reject("not impl")
                        },
                        [p.QUERY_GLOBAL_MSG]: function(e, t, s, r) {
                            return new Promise(((i, o) => {
                                let n;
                                n = t ? () => x(s, r) : () => !1;
                                let c = a.a.formatTextSearch(e);
                                if (c = c.trim(), !c) return o("text is null");
                                if (r || (r = {}), r.progress) {
                                    let e = 0;
                                    const t = [10, 20, 50],
                                        a = (e, t) => ({
                                            data: e,
                                            page: t
                                        }),
                                        l = (c, d, u, g = 30) => {
                                            if (n()) o("aborted");
                                            else if (u >= d.length) i(a(null, e));
                                            else {
                                                const m = Math.min(u + g, d.length);
                                                h.queryGlobalMessageV2(c, d.slice(u, m), r.configs, n).then((r => {
                                                    r ? (P(a(r, e), s), e >= 1 ? setTimeout((() => {
                                                        l(c, d, u + g, t[Math.min(++e, t.length - 1)])
                                                    }), 100) : l(c, d, u + g, t[Math.min(++e, t.length - 1)])) : i(a(null, e))
                                                })).catch((e => {
                                                    o(e)
                                                }))
                                            }
                                        };
                                    (() => {
                                        h.getConversationMatch(c, n).then((s => {
                                            if (s && s.length > 0) {
                                                s.sort(((e, t) => t.ts - e.ts));
                                                let e = [];
                                                s.forEach((t => {
                                                    e.push(t.convId)
                                                })), e.length > 0 && l(c, e, 0, t[0])
                                            } else i(a(null, e))
                                        })).catch((e => {
                                            o(e)
                                        }))
                                    })()
                                } else i(h.queryGlobalMessageV2(c, r.listConvs, r.configs, n))
                            }))
                        },
                        [p.CLOSE_DB]: function() {
                            return new Promise(((e, t) => {
                                q ? (U("go close db " + q.userId), e(h.closeDb(q.userId, q.UIN))) : t("close what?? curr: " + JSON.stringify(q)), q = null
                            }))
                        },
                        [p.DELETE_DATA]: function(e) {
                            return h.deleteRow(e.msgId)
                        },
                        [p.DELETE_DATA_MULTI]: function(e, t, s, r, a) {
                            const i = d.default.getTimeNow(),
                                n = o.a.now();
                            let c = Array.isArray(e) ? e : [e];
                            if (c.length > w) {
                                let e = c.slice(w, c.length);
                                G({
                                    act: p.DELETE_DATA_MULTI,
                                    data: e,
                                    taskId: y.next(),
                                    isAbort: !1,
                                    callback: !1,
                                    priority: a,
                                    additions: r
                                }), c = c.slice(0, w)
                            }
                            const l = c.map((e => e.msgId)),
                                u = (null == r ? void 0 : r.version) || "V1",
                                g = "V1" === u ? D.DEFAULT : D.V2;
                            return m.zsymb(0, "FcIWof", `_deleteDataMulti called, msgIds length: ${l.length}, version: ${u}`), H({
                                taskId: s,
                                additions: r,
                                startTime: n,
                                aggStartTime: i,
                                subCommandId: g,
                                extraParams: {
                                    batchLength: l.length
                                },
                                commandId: R,
                                successCode: k.SUCCESS,
                                failCode: k.FAIL_DEFAULT,
                                promise: h.deleteRows(l, u)
                            })
                        },
                        [p.PURE_QUERY]: function(e) {
                            return new Promise(((t, s) => {
                                h.pureQuery(e).then((r => {
                                    m.zsymb(0, "QfLxd3", "pure query done", e, r), r ? t(r) : s(null)
                                })).catch((t => {
                                    m.zsymb(18, "vKEF18", "pure query fail", e, t), s(t)
                                }))
                            }))
                        },
                        [p.QUERY_GLOBAL_MSG_V3]: function(e, t, s, r) {
                            return new Promise(((i, o) => {
                                let n;
                                n = t ? () => x(s, r) : () => !1;
                                let c = a.a.formatTextSearch(e);
                                if (c = c.trim(), !c) return o("text is null");
                                const l = (e, t) => ({
                                    data: e,
                                    page: t
                                });
                                j(`_queryGlobalMessageV3, kw: ${L(e)}`);
                                let d = r.configs.filter;
                                h.queryGlobalMessageV3(c, n, d, j).then((t => {
                                    j(`_queryGlobalMessageV3 SUCCESS, kw: ${L(e)}, total res: ${t?t.length:"empty"}`), t ? (P(l(t, 0), s), i(l(t, 0))) : i(l(null, 0))
                                })).catch((e => {
                                    j("_queryGlobalMessageV3 Error", JSON.stringify(e)), o(e)
                                }))
                            }))
                        },
                        [p.COUNT_TOTAL]: function() {
                            return new Promise(((e, t) => {
                                h.countTotal().then((s => {
                                    s ? e(s) : t(null)
                                })).catch((e => {
                                    m.zsymb(18, "Fqwg2S", "count total fail", e), t(e)
                                }))
                            }))
                        },
                        [p.QUERY_GLOBAL_MSG_V4]: function(e, t, s, r) {
                            return new Promise(((i, o) => {
                                let n;
                                n = t ? () => x(s, r) : () => !1;
                                let c = a.a.formatTextSearch(e);
                                if (c = c.trim(), !c) return o("text is null");
                                const l = (e, t) => ({
                                    data: e,
                                    page: t
                                });
                                j(`_queryGlobalMessageV3, kw: ${L(e)}`);
                                let d = r.configs.filter;
                                h.queryGlobalMessageV4(c, n, d, j).then((t => {
                                    j(`_queryGlobalMessageV3 SUCCESS, kw: ${L(e)}, total res: ${t?t.length:"empty"}`), t ? (P(l(t, 0), s), i(l(t, 0))) : i(l(null, 0))
                                })).catch((e => {
                                    j("_queryGlobalMessageV3 Error", JSON.stringify(e)), o(e)
                                }))
                            }))
                        },
                        [p.QUERY_V2]: function(e, t, s, i) {
                            return new Promise(((o, n) => {
                                let c;
                                N(e, i), c = t ? () => x(s, i) : () => !1;
                                let l = a.a.formatTextSearch(e);
                                if (l = l.trim(), !l) {
                                    let e = i.filter;
                                    if (!e || !0 !== e.searchForEmpty) return n("text is null")
                                }
                                let d = l.split(" ");
                                if ((i = i || {}).progress) {
                                    let e, t = !0,
                                        a = 0,
                                        l = i.configs && i.configs.limit ? i.configs.limit : V,
                                        u = i.configs && void 0 !== i.configs.offset ? i.configs.offset : 0,
                                        g = i.configs ? Object(r.a)({}, i.configs) : {},
                                        m = [];
                                    const p = () => {
                                            clearTimeout(e), e = null, o(m)
                                        },
                                        b = (o, u) => {
                                            if (c && c()) return n("aborted");
                                            h.queryV2(d, i.filter, Object(r.a)(Object(r.a)({}, g), {}, {
                                                limit: u,
                                                offset: o
                                            }), c).then((r => {
                                                if (c && c()) return n("aborted");
                                                l -= u, r && r.constructor == Array ? (m.push.apply(m, r), r.length >= u && l > 0 ? (t ? (t = !1, a = m.length, P(m, s)) : e || (e = setTimeout((() => {
                                                    e = null, a < m.length && (a = m.length, P(m, s))
                                                }), 2e3)), b(o + r.length, Math.min(100, l))) : p()) : p()
                                            })).catch((e => {
                                                N("_doQuery", e), p()
                                            }))
                                        };
                                    b(u, Math.min(100, l))
                                } else i.configs && i.configs.limit || (i.configs = i.configs ? i.configs : {}, i.configs.limit = V, i.configs.offset = i.configs.offset || 0), o(h.queryV2(d, i.filter, i.configs, c))
                            }))
                        }
                    };
                let f = [];
                const y = new class {
                    constructor() {
                        this.value = 1
                    }
                    next() {
                        return this.value++, this.value
                    }
                };
                let I = new Map;
                const S = 1,
                    v = 2;
                let M = 2;
                const w = 300;
                let _ = null;
                const C = "FLAG_SLEEP_TOO_LONG",
                    E = 6e4,
                    T = 6e4,
                    O = 100,
                    R = 152114,
                    D = {
                        DEFAULT: 0,
                        V2: 1
                    },
                    k = {
                        SUCCESS: 0,
                        FAIL_DEFAULT: 1
                    },
                    F = {
                        BEGIN: 2,
                        EXECUTE: 3,
                        COMMIT: 4,
                        ROLLBACK: 5
                    },
                    z = !0;

                function j(...e) {
                    z && m.zsymb(0, "44a45k", "[search]", ...e)
                }

                function N() {}
                const L = e => l.default.md5(e);
                var A = e => {
                    $zdb.sendResponse(e)
                };

                function P(e, t) {
                    A({
                        progress: !0,
                        taskId: t,
                        data: e
                    })
                }

                function U(e) {
                    A({
                        logErr: !0,
                        data: e
                    })
                }

                function x(e, t) {
                    return t && t.filter ? e != I.get("with-filter") : e != I.get("_")
                }

                function G(e) {
                    let t = !1;
                    [p.QUERY, p.QUERY_GLOBAL_MSG, p.QUERY_GLOBAL_MSG_V3, p.QUERY_GLOBAL_MSG_V4, p.QUERY_V2].indexOf(e.act) >= 0 && (e.additions && e.additions.filter ? I.set("with-filter", e.taskId) : I.set("_", e.taskId));
                    for (let a = 0; a < f.length; ++a)
                        if (s = f[a], r = e, s.priority >= r.priority) {
                            f.splice(a, 0, e), t = !0;
                            break
                        } var s, r;
                    t || f.push(e),
                        function() {
                            if (M == S) return;
                            M = S, B()
                        }()
                }

                function $(e, t, s) {
                    try {
                        e.callback && A({
                            success: s,
                            data: t,
                            taskId: e.taskId
                        }), s || U(" err " + e.act + " " + t)
                    } catch (r) {
                        U(" _afterAction " + e.act + ": " + r)
                    }
                    B()
                }

                function B() {
                    if (0 == f.length) return M = v, _ && clearTimeout(_), void(_ = setTimeout((() => {
                        U(C), _ = null
                    }), E));
                    _ && (clearTimeout(_), _ = null);
                    let e = f.pop();
                    if (!e || "object" != typeof e) return U("ev is not Object"), void B();
                    let t = b && e.act && b.hasOwnProperty(e.act) ? b[e.act] : null;
                    if ("function" == typeof t) {
                        let r = !1,
                            a = setTimeout((() => {
                                a = null, r = !0, U("timeout event " + e.act), $(e, "timeout!!", !1)
                            }), T);
                        const i = () => (a && (clearTimeout(a), a = null), r);
                        try {
                            t(e.data, e.isAbort, e.taskId, e.additions, e.priority).then((t => {
                                i() || $(e, t, !0)
                            })).catch((t => {
                                i() || $(e, t, !1)
                            }))
                        } catch (s) {
                            $(e, s, !1)
                        }
                    } else $(e, "not support this function " + e.act, !1)
                }
                let Q, q;
                const V = 1e4;

                function Z({
                    commandId: e,
                    startTime: t,
                    aggStartTime: s,
                    subCommandId: r = 0,
                    responseCode: a = 0,
                    extraParams: n = {}
                } = {}) {
                    if (!Object(u.a)(e) || !Object(u.a)(t) || !Object(u.a)(s)) return void m.zsymb(18, "d8D4ZO", "qosLog invalid params");
                    const c = Number((o.a.now() - t).toFixed(2));
                    m.zsymb(0, "wcxmSS", `qosLog cmd:${e} subCmd:${r} duration:${c} respCode:${a} aggStartTime:${s} extraParams:${JSON.stringify(n)}`), i.default.increaseFailed(e, r, c, a, s, [JSON.stringify(n)])
                }

                function H({
                    commandId: e,
                    subCommandId: t = 0,
                    promise: s,
                    startTime: a,
                    aggStartTime: i,
                    failCode: n,
                    successCode: c,
                    additions: l,
                    taskId: d,
                    extraParams: u = {}
                } = {}) {
                    return s.then((s => (P(Object(r.a)(Object(r.a)({}, l), {}, {
                        status: "success",
                        dbFinishedTime: Number((o.a.now() - a).toFixed(2)),
                        postMessageGapTime: a - l.currentBatchStartTime,
                        executionTime: Number((o.a.now() - l.currentBatchStartTime).toFixed(2))
                    }), d), Z({
                        commandId: e,
                        subCommandId: t,
                        startTime: a,
                        aggStartTime: i,
                        responseCode: c
                    }), s))).catch((s => {
                        const c = h.isDbTransactionStepError(s);
                        let g = n;
                        if (c) {
                            switch (s.step) {
                                case "BEGIN":
                                    g = F.BEGIN;
                                    break;
                                case "EXECUTE":
                                    g = F.EXECUTE;
                                    break;
                                case "COMMIT":
                                    g = F.COMMIT;
                                    break;
                                case "ROLLBACK":
                                    g = F.ROLLBACK;
                                    break;
                                default:
                                    g = n
                            }
                        }
                        throw P(Object(r.a)(Object(r.a)({}, l), {}, {
                            status: "fail",
                            dbFinishedTime: Number((o.a.now() - a).toFixed(2)),
                            postMessageGapTime: a - l.currentBatchStartTime,
                            executionTime: Number((o.a.now() - l.currentBatchStartTime).toFixed(2))
                        }), d), Z({
                            commandId: e,
                            subCommandId: t,
                            startTime: a,
                            aggStartTime: i,
                            responseCode: g,
                            extraParams: u
                        }), c && s.originalError ? s.originalError : s
                    }))
                }
                t.a = {
                    receiverEvent: function(e) {
                        switch (m.zsymb(0, "-k-78Y", "receiver ", e.act), e.act) {
                            case p.ABORTED_SEARCH:
                                I = new Map, f = f.filter((e => e.act != p.QUERY && e.act != p.QUERY_V2));
                                break;
                            case p.DELETE_ALL:
                                f = [], G(e);
                                break;
                            default:
                                G(e)
                        }
                    },
                    isLoadedLib: () => h.isLoadedLib()
                }
            }).call(this, s("8oxB"))
        },
        cMnt: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            })), s.d(t, "b", (function() {
                return i
            }));
            var r = s("1pet");
            const a = [r.MSG_PHOTO, r.MSG_VOICE, r.MSG_VIDEO, r.MSG_FILE, r.MSG_CONTACT, r.MSG_ZINSTANT, r.MSG_ZINSTANT_V2, r.MSG_ZINSTANT_OLD, r.MSG_DOODLE],
                i = "RESOURCE_SCANNER_STATE_STORAGE_KEY"
        },
        hGrR: function(e, t, s) {
            "use strict";
            var r = s("v0D5");
            s.d(t, "ResourceCleanupStateStorage", (function() {
                return r.a
            }));
            var a = s("Tlqd");
            s.d(t, "RRCStorageKeys", (function() {
                return a.a
            })), s.d(t, "RRCStorageName", (function() {
                return a.b
            })), s.d(t, "ResourceCleanupStage", (function() {
                return a.c
            }));
            s("rwwj")
        },
        jWYK: function(e, t) {},
        mP9p: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var r = s("3jnX");
            class a extends r.b {
                constructor(e) {
                    super({
                        type: "RESOURCE-SCANNER-MANAGER-V2",
                        params: e
                    })
                }
            }
        },
        qMb5: function(e, t) {},
        rwwj: function(e, t) {},
        "ut+A": function(e, t, s) {
            "use strict";
            var r = s("aEZA");
            s.d(t, "ResourceCleanupManager", (function() {
                return r.a
            }));
            s("qMb5")
        },
        v0D5: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var r = s("jDHv");
            const a = Object(r.define)("resource-cleanup-state-storage")
        },
        vyMb: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var r = s("jDHv");
            const a = Object(r.define)("resource-scanner-manager")
        }
    });
//# sourceMappingURL=sourcemaps/shared-worker.e08d0d44f38873747a6b.js.map