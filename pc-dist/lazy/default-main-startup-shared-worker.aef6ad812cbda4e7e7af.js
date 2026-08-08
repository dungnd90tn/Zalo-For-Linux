(this.webpackJsonp = this.webpackJsonp || []).push([
    [15], {
        "/d1d": function(e, t, s) {
            "use strict";
            var i = s("jDHv"),
                r = s("t5Xg"),
                a = s("Yi2m"),
                n = s("rCQs"),
                o = s("7odm");
            let d = function(e) {
                return e.MigrationOldFolder = "MigrationOldFolder", e
            }({});
            const l = {
                [d.MigrationOldFolder]: 0
            };
            var c;
            let m = Object(n.b)(r.a)(c = Reflect.metadata("design:type", Function)(c = Reflect.metadata("design:paramtypes", [])(c = class {
                constructor() {}
                async log(e) {
                    this.logAction999(e)
                }
                async logAction999(e) {
                    a.default.logActionInfoV2(a.FeaturesInfo.ZaloTempNewPos, l.MigrationOldFolder, e)
                }
            }) || c) || c) || c;
            i.ModuleContainer.registerSingleton(r.a, m);
            var u = s("/W/i"),
                g = s("IpzU"),
                h = s("PoHQ"),
                p = s("cD7Y"),
                f = s("fc/q");
            const M = 152725;
            class _ {
                static getInstance() {
                    return this.instance || (this.instance = new _), this.instance
                }
                increaseFailed(e) {
                    i.ModuleContainer.resolve(f.b).log({
                        success: !1,
                        commandId: e.cmd,
                        subCommandId: e.subCmd,
                        duration: e.duration,
                        errorCode: e.code,
                        startTime: 0,
                        params: []
                    })
                }
                increaseSuccess(e) {
                    i.ModuleContainer.resolve(f.b).log({
                        success: !0,
                        commandId: e.cmd,
                        subCommandId: e.subCmd,
                        duration: e.duration,
                        errorCode: e.code,
                        startTime: 0,
                        params: []
                    })
                }
                markOldPathDoesExist(e) {
                    this.increaseFailed({
                        cmd: M,
                        subCmd: 1,
                        duration: Date.now() - e,
                        code: 0
                    })
                }
                markMovePathFailured(e, t) {
                    this.increaseFailed({
                        cmd: M,
                        subCmd: 2,
                        duration: Date.now() - t,
                        code: e
                    })
                }
                markMovePathSuccess(e) {
                    this.increaseSuccess({
                        cmd: M,
                        subCmd: 0,
                        duration: Date.now() - e,
                        code: 0
                    })
                }
            }
            _.instance = void 0;
            var b = s("poQk"),
                y = s("hpRf");
            let I, v, O;
            I = $znode.fs, v = $znode.fsExtra, O = $znode.path;
            const D = new y.a("zalo-temp-new-pos-al"),
                T = (e, t) => {
                    let s = D.getItemForCurrentUser("data");
                    if ("string" == typeof s && (s = JSON.parse(s), s && s[o.d])) {
                        let i = s[o.d].file_success_count,
                            r = s[o.d].migration_size_gb,
                            a = s[o.d].file_fail_count;
                        t ? (i++, r += e) : a++, s[o.d] = {
                            file_success_count: i,
                            migration_size_gb: r,
                            file_fail_count: a
                        }
                    }
                    let i = s && s[o.d] ? s[o.d] : null;
                    i || (i = {
                        file_success_count: t ? 1 : 0,
                        migration_size_gb: t ? e : 0,
                        file_fail_count: t ? 0 : 1
                    });
                    const r = {
                        [o.b]: u.a.zalo_temp_new_pos.enable,
                        [o.a]: a.default.getDeviceID(),
                        [o.c]: Date.now(),
                        [o.d]: i
                    };
                    D.setItemForCurrentUser("has_new_log", "1"), D.setItemForCurrentUser("data", JSON.stringify(r))
                };
            var w;
            let E;
            E = $znode.fsExtra;
            Object(n.b)(p.a)(w = class {
                isEnabled() {
                    return u.a.zalo_temp_new_pos.enable
                }
                isEnableLog() {
                    return u.a.zalo_temp_new_pos.enable_log
                }
                async getZaloTempPath(e) {
                    if (this.isEnabled()) {
                        const t = h.p.getSessionUserId();
                        return await Object(g.getDefaultZaloDownloadsFolderDir)(t, e)
                    }
                    return await Object(g.getZaloTempDir)()
                }
                getZaloTempPathSync(e) {
                    if (this.isEnabled()) {
                        const t = h.p.getSessionUserId();
                        return Object(g.getDefaultZaloDownloadsFolderDirSync)(t, e)
                    }
                    return Object(g.getZaloTempDirSync)()
                }
                async moveZaloTempToNewLocation(e, t) {
                    const s = await b.b.execRaw("getInfoZipFile", [e], {
                        priority: b.a.HIGH
                    });
                    return this.isEnabled() ? E.existsSync(t) ? (this.isEnableLog() && (D.setItemForCurrentUser("has_new_log", "1"), D.setItemForCurrentUser("data", JSON.stringify({
                        [o.b]: u.a.zalo_temp_new_pos.enable,
                        [o.e]: "true",
                        [o.a]: a.default.getDeviceID(),
                        [o.c]: Date.now()
                    }))), Promise.resolve(!0)) : await ((e, t) => new Promise((async (s, i) => {
                        const r = e + "~",
                            a = Date.now();
                        v.ensureDirSync(O.dirname(t)), v.existsSync(e) || (_.getInstance().markOldPathDoesExist(a), i(new Error("Source path does not exist")));
                        const n = await b.b.execRaw("getInfoZipFile", [e], {
                            priority: b.a.HIGH
                        });
                        try {
                            v.renameSync(e, t), v.existsSync(r) && v.removeSync(r), _.getInstance().markMovePathSuccess(a), T(n.totalUncompressedSize, !0), s(!0)
                        } catch (o) {
                            "EXDEV" !== o.code && (_.getInstance().markMovePathFailured(o.code, a), T(n.totalUncompressedSize, !1), i(o)), v.copySync(e, t, {
                                overwrite: !1,
                                preserveTimestamps: !0,
                                errorOnExist: !0
                            }), v.removeSync(e), v.existsSync(r) && v.removeSync(r), _.getInstance().markMovePathSuccess(a), T(n.totalUncompressedSize, !0), s(!0)
                        }
                    })))(e, t) : (T(s.totalUncompressedSize, !1), Promise.resolve(!1))
                }
            })
        },
        "37f1": function(e, t, s) {
            "use strict";
            var i = s("VTBJ"),
                r = s("v6qY"),
                a = s("bUXd"),
                n = s("1pet"),
                o = s("NDmK"),
                d = s("AjFa"),
                l = s("W13p"),
                c = s("5R0e"),
                m = s("jDHv"),
                u = s("EYv5"),
                g = s("Mlp7"),
                h = s("Mgpg"),
                p = s("3dEI"),
                f = s("fhp4"),
                M = s("4ePI"),
                _ = (s("4pKu"), s("K4Ns")),
                b = s("fsN4"),
                y = s("XS0u"),
                I = s("1erv"),
                v = s("wH4e"),
                O = s("zLd2"),
                D = s("KPNQ"),
                T = s("H8Z7"),
                w = s("8tev"),
                E = s("dm9D"),
                S = s("vAzq"),
                C = s("AtyM"),
                U = s("mlG1"),
                j = s("5txd"),
                F = s("3ael"),
                A = s("SrDO"),
                N = s("jbAT"),
                P = s("ma9L");
            const R = m.ModuleContainer.resolve(h.ZLoggerFactory).createZLogger("media-manager"),
                k = () => m.ModuleContainer.resolve(N.b);

            function L(e, t = !0) {
                const s = m.ModuleContainer.resolve(A.b).getMediaMapper(e);
                if (!s && t) throw Error(`[getMediaMapper]: can't get mediaMapper has mediaType: ${e}!`);
                return s
            }
            async function B(e) {
                try {
                    const t = [],
                        s = [],
                        r = [];
                    for (let o = 0; o < e.length; o++) {
                        const a = e[o],
                            d = a.msgType,
                            l = Number(a.sendDttm);
                        switch (d) {
                            case n.MSG_PHOTO:
                            case n.MSG_PHOTO_2:
                            case n.MSG_DOODLE:
                                if (!Object(F.a)(a) && !Object(F.d)(a) && !Object(F.c)(a)) {
                                    const e = a.message.thumb || a.message.oriUrl,
                                        t = d == n.MSG_DOODLE ? Object(i.a)(Object(i.a)({}, a.message), {}, {
                                            thumbUrl: e
                                        }) : a.message;
                                    s.push({
                                        sendDttm: l,
                                        msgId: a.msgId,
                                        userId: a.toUid,
                                        message: t,
                                        fromUid: a.fromUid,
                                        subType: n.MSG_SUBTYPE_PHOTO,
                                        type: "image",
                                        ttl: a.ttl,
                                        cliMsgId: a.cliMsgId
                                    })
                                }
                                break;
                            case n.MSG_FILE:
                                t.push({
                                    sendDttm: l,
                                    msgId: a.msgId,
                                    userId: a.toUid,
                                    message: a.message,
                                    fromUid: a.fromUid,
                                    type: "file",
                                    ttl: a.ttl,
                                    cliMsgId: a.cliMsgId
                                });
                                break;
                            case n.MSG_CONTACT:
                                "recommened.link" === a.message.action && r.push({
                                    sendDttm: l,
                                    msgId: a.msgId,
                                    userId: a.toUid,
                                    message: a.message,
                                    fromUid: a.fromUid,
                                    type: "link",
                                    ttl: a.ttl,
                                    cliMsgId: a.cliMsgId
                                });
                                break;
                            case n.MSG_VIDEO:
                                s.push({
                                    sendDttm: l,
                                    msgId: a.msgId,
                                    userId: a.toUid,
                                    message: a.message,
                                    fromUid: a.fromUid,
                                    subType: n.MSG_SUBTYPE_MEDIA_VIDEO,
                                    type: "image",
                                    ttl: a.ttl,
                                    cliMsgId: a.cliMsgId
                                })
                        }
                    }
                    let a = [];
                    return s.length > 0 && a.push(b.default.getInstance().Core.Image.insertMulti(s, {
                        replace: !1
                    })), r.length > 0 && a.push(b.default.getInstance().Core.Link.insertMulti(r, {
                        replace: !1
                    })), t.length > 0 && a.push(b.default.getInstance().Core.File.insertMulti(t, {
                        replace: !1
                    })), await Promise.all(a)
                } catch (s) {
                    var t;
                    const e = [null == s || null === (t = s.toString) || void 0 === t ? void 0 : t.call(s), null == s ? void 0 : s.stack].join("\n");
                    R.zsymb(21, "AV_Lyc", ["[_addMediasWhenTransferMessageOldFlow] - error: {}", "RzE6NX"], e)
                }
            }
            async function z(e, t) {
                try {
                    if (!e || !e.length) return;
                    t = t || `${r.c.UNKNOWN}${r.d.FROM_MSG}`;
                    const s = [],
                        a = [],
                        o = [],
                        d = L("image"),
                        l = L("file"),
                        c = L("link");
                    e.forEach((e => {
                        const r = e.msgType;
                        switch (r) {
                            case n.MSG_PHOTO:
                            case n.MSG_PHOTO_2:
                            case n.MSG_DOODLE:
                                if (r === n.MSG_DOODLE) {
                                    let r = e.message;
                                    if (e.message) {
                                        const t = e.message.thumb || e.message.oriUrl;
                                        r = Object(i.a)(Object(i.a)({}, e.message), {}, {
                                            thumbUrl: t
                                        })
                                    }
                                    s.push(d.toDomainFromTMessage(Object(i.a)(Object(i.a)({}, e), {}, {
                                        message: r
                                    }), n.MSG_SUBTYPE_MEDIA_DOODLE, t))
                                } else Object(F.a)(e) || Object(F.d)(e) || Object(F.c)(e) || s.push(d.toDomainFromTMessage(e, n.MSG_SUBTYPE_PHOTO, t));
                                break;
                            case n.MSG_FILE:
                                a.push(l.toDomainFromMessage(e, t));
                                break;
                            case n.MSG_CONTACT:
                                "recommened.link" === e.message.action && o.push(c.toDomainFromMessage(e, t));
                                break;
                            case n.MSG_VIDEO:
                                s.push(d.toDomainFromTMessage(e, n.MSG_SUBTYPE_MEDIA_VIDEO, t))
                        }
                    }));
                    let m = [];
                    return s.length > 0 && m.push(Object(P.a)(b.default.getInstance().Media.Image, s).then((e => {
                        if (e.success.length) {
                            const t = e.success.map((e => L("image").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                            k().createOrUpdateFromMedias(t)
                        }
                    }))), a.length > 0 && m.push(Object(P.a)(b.default.getInstance().Media.File, a).then((e => {
                        if (e.success.length) {
                            const t = e.success.map((e => L("file").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                            k().createOrUpdateFromMedias(t)
                        }
                    }))), o.length > 0 && m.push(Object(P.a)(b.default.getInstance().Media.Link, o).then((e => {
                        if (e.success.length) {
                            const t = e.success.map((e => L("link").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                            k().createOrUpdateFromMedias(t)
                        }
                    }))), await Promise.all(m)
                } catch (a) {
                    var s;
                    const e = [null == a || null === (s = a.toString) || void 0 === s ? void 0 : s.call(a), null == a ? void 0 : a.stack].join("\n");
                    R.zsymb(21, "xWvBk6", ["[_addMediasFromMessages] - error: {}", "J_iGYE"], e)
                }
            }
            var x;
            Object(m.injectable)()(x = Object(m.singleton)(u.a)(x = function(e, t) {
                return Object(m.inject)(O.c)(e, void 0, 0)
            }(x = function(e, t) {
                return Object(m.inject)(p.b)(e, void 0, 1)
            }(x = function(e, t) {
                return Object(m.inject)(f.e)(e, void 0, 2)
            }(x = function(e, t) {
                return Object(m.inject)(D.b)(e, void 0, 3)
            }(x = function(e, t) {
                return Object(m.inject)(h.ZLoggerFactory)(e, void 0, 4)
            }(x = Reflect.metadata("design:type", Function)(x = Reflect.metadata("design:paramtypes", [void 0 === O.IUtilsMediaManager ? Object : O.IUtilsMediaManager, void 0 === p.IMediaRepositoryFactory ? Object : p.IMediaRepositoryFactory, void 0 === f.b ? Object : f.b, void 0 === D.IMediaPrimaryKeyConvertor ? Object : D.IMediaPrimaryKeyConvertor, void 0 === h.ZLoggerFactory ? Object : h.ZLoggerFactory])(x = class {
                constructor(e, t, s, i, r) {
                    this._utilsMediaManager = void 0, this._mediaMapperFactory = void 0, this._mediaRepositoryFactory = void 0, this._mediaPrimaryKeyConvertor = void 0, this._logger = void 0, this._utilsMediaManager = e, this._mediaRepositoryFactory = t, this._mediaMapperFactory = s, this._mediaPrimaryKeyConvertor = i, this._logger = r.createZLogger("media-manager", [""])
                }
                filter(e, t) {
                    return Object(I.a)() ? M.a.filterMedia(e, t) : g.a.filter(e, t)
                }
                getMediaRepository(e, t = !0) {
                    const s = this._mediaRepositoryFactory.getMediaRepository(e);
                    if (!s && t) throw Error(`[getMediaRepository]: can't get mediaRepository has mediaType: ${e}!`);
                    return s
                }
                getMediaMapper(e, t = !0) {
                    return L(e, t)
                }
                async _isMigrationDone() {
                    return (await m.ModuleContainer.resolve(T.c).getMediaMigrationState()).stateName === w.b.DONE
                }
                async isExistedMedia(e, t) {
                    try {
                        if (!e) throw Error("mediaType is invalid!");
                        if (!t) throw Error("mediaIdObj is invalid!");
                        const s = this.getMediaRepository(e);
                        return await s.isExistedMedia({
                            newId: t.newId || "",
                            oldId: t.oldId || ""
                        })
                    } catch (i) {
                        var s;
                        const e = [null == i || null === (s = i.toString) || void 0 === s ? void 0 : s.call(i), null == i ? void 0 : i.stack].join("\n");
                        return this._logger.zsymb(21, "h5xqoJ", ["[isExistedMedia] - error: {}", "X80ChP"], e), !1
                    }
                }
                async addMedias(e, t, s = `${r.c.UNKNOWN}${r.d.UNKNOWN}`) {
                    try {
                        if (!e) throw Error("mediaType is undefined!");
                        if (!t || !t.length) return;
                        const i = this.getMediaRepository(e),
                            r = this.getMediaMapper(e),
                            a = t.map((t => (t.src = s, "image" === e && (t.subType = n.MSG_SUBTYPE_MEDIA_DOODLE), r.toDomainFromDTO(t)))),
                            o = (await i.insertMulti(a)).success.map((t => this.getMediaMapper(e).toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(t)));
                        await this._utilsMediaManager.createOrUpdateFromMedias(o)
                    } catch (a) {
                        var i;
                        const e = [null == a || null === (i = a.toString) || void 0 === i ? void 0 : i.call(a), null == a ? void 0 : a.stack].join("\n");
                        this._logger.zsymb(21, "tLmjqE", ["[addMedias] - error: {}", "ZfQ5VZ"], e)
                    }
                }
                addMediaToConvOnly(e, t = `${r.c.UNKNOWN}${r.d.UNKNOWN}`) {
                    var s, i, a;
                    if (!Object(I.a)()) return g.a.addMediaToConversationOnly(e);
                    let n = [];
                    if (null != e && null !== (s = e.image) && void 0 !== s && s.length) {
                        const s = this.getMediaRepository("image"),
                            i = this.getMediaMapper("image");
                        n.push(null == s ? void 0 : s.insertMulti(e.image.map((e => (e.src = t, i.toDomainFromDTO(e))))).then((e => {
                            if (e.success.length) {
                                const t = e.success.map((e => this.getMediaMapper("image").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                                this._utilsMediaManager.createOrUpdateFromMedias(t)
                            }
                        })))
                    }
                    if (null != e && null !== (i = e.link) && void 0 !== i && i.length) {
                        const s = this.getMediaRepository("link"),
                            i = this.getMediaMapper("link");
                        n.push(null == s ? void 0 : s.insertMulti(e.link.map((e => {
                            e.src = t;
                            return i.toDomainFromDTO(e)
                        }))).then((e => {
                            if (e.success.length) {
                                const t = e.success.map((e => this.getMediaMapper("link").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                                this._utilsMediaManager.createOrUpdateFromMedias(t)
                            }
                        })))
                    }
                    if (null != e && null !== (a = e.file) && void 0 !== a && a.length) {
                        const s = this.getMediaRepository("file"),
                            i = this.getMediaMapper("file");
                        n.push(s.insertMulti(e.file.map((e => (e.src = t, i.toDomainFromDTO(e))))).then((e => {
                            if (e.success.length) {
                                const t = e.success.map((e => this.getMediaMapper("file").toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                                this._utilsMediaManager.createOrUpdateFromMedias(t)
                            }
                        })))
                    }
                    return Promise.all(n)
                }
                async deleteMediasById(e, t, s) {
                    try {
                        if (!s) throw Error("mediaIdObjs is undefined!");
                        const o = Array.isArray(s) ? [...s] : [s];
                        for (const e of o) {
                            const {
                                newId: s,
                                oldId: i
                            } = e;
                            if (!s && i) {
                                const s = await this.getMediaFieldsByMsgId(t, i);
                                e.newId = s ? `${s.cliMsgId}_${s.fromUid}_${s.toUid}` : ""
                            }
                        }
                        const u = this.getMediaRepository(e, !1),
                            g = [];
                        for (const e of o) g.push(Object(i.a)(Object(i.a)({}, e), {}, {
                            deleteTo: await this._isMigrationDone() ? _.a.NEW : _.a.UNKNOWN
                        }));
                        if (u) await u.deleteMultiMedias(g);
                        else {
                            var r, a, n;
                            const e = [];
                            e.push(null === (r = m.ModuleContainer.resolve(d.b)) || void 0 === r ? void 0 : r.deleteMultiMedias(g), null === (a = m.ModuleContainer.resolve(l.b)) || void 0 === a ? void 0 : a.deleteMultiMedias(g), null === (n = m.ModuleContainer.resolve(c.b)) || void 0 === n ? void 0 : n.deleteMultiMedias(g)), await Promise.all(e)
                        }
                    } catch (u) {
                        var o;
                        const e = [null == u || null === (o = u.toString) || void 0 === o ? void 0 : o.call(u), null == u ? void 0 : u.stack].join("\n");
                        this._logger.zsymb(21, "q3WAxa", ["[deleteMediasById] - error: {}", "bnCacj"], e)
                    }
                }
                async getValidMediasOfConv(e, t, s, r, a, o) {
                    try {
                        if (!t) return Promise.resolve([]);
                        const d = C.a.now(),
                            l = {
                                cliMsgId: n.MessageConstants.MAX_MSG_ID,
                                sendDttm: parseInt(n.MessageConstants.MAX_SENDDTTM),
                                msgId: r || n.MessageConstants.MAX_MSG_ID
                            };
                        if (null != o && o.lastItemOpts) l.cliMsgId = o.lastItemOpts.cliMsgId, l.sendDttm = o.lastItemOpts.sendDttm;
                        else if (r) {
                            const e = await this.getMediaFieldsByMsgId(t, r);
                            e && (l.cliMsgId = e.cliMsgId, l.sendDttm = e.sendDttm)
                        }
                        const c = g.a.getLastDeleteConv(t, e),
                            m = this.getMediaRepository(e);
                        let u = "image" === e ? 50 : s;
                        const h = await m.getMediasOfConv(t, l, u, Object(i.a)(Object(i.a)({}, a), {}, {
                            deletePointOfConv: c
                        }));
                        if (!h.length) return [];
                        const p = this.getMediaMapper(e);
                        let f = await Promise.all(h.map((e => p.toDTO(e))));
                        return this._trackDBPerformance({
                            executionTime: C.a.now() - d,
                            actionName: "getByConv",
                            mediaType: e,
                            resultSize: f.length
                        }), "image" === e && (f = Object(j.c)(f)), f
                    } catch (l) {
                        var d;
                        const e = [null == l || null === (d = l.toString) || void 0 === d ? void 0 : d.call(l), null == l ? void 0 : l.stack].join("\n");
                        return this._logger.zsymb(21, "7mfaJy", ["[getValidMediasOfConv] - error: {}", "S04UxK"], e), Promise.resolve([])
                    }
                }
                async getLastMediasOfConvWithoutCorrectData(e, t, s, i = 100, r) {
                    if (!Object(I.a)()) return g.a.getLastMediaFromConversation(t, e, i, s);
                    try {
                        if (!t) return Promise.resolve([null, Error("convId is undefined!")]);
                        const a = {
                            cliMsgId: n.MessageConstants.MAX_MSG_ID,
                            sendDttm: parseInt(n.MessageConstants.MAX_SENDDTTM),
                            msgId: s || o.default.load_media.use_max_id || n.MessageConstants.MAX_MSG_ID
                        };
                        if (null != r && r.lastItemOpts) a.cliMsgId = r.lastItemOpts.cliMsgId, a.sendDttm = r.lastItemOpts.sendDttm;
                        else if (s) {
                            const e = await this.getMediaFieldsByMsgId(t, s);
                            e && (a.cliMsgId = e.cliMsgId, a.sendDttm = e.sendDttm)
                        }
                        const d = this.getMediaRepository(e),
                            l = await d.getLastMediasOfConv(t, a, i);
                        return [await Promise.all(l.map((t => this.getMediaMapper(e).toDTO(t)))), null]
                    } catch (d) {
                        var a;
                        const e = [null == d || null === (a = d.toString) || void 0 === a ? void 0 : a.call(d), null == d ? void 0 : d.stack].join("\n");
                        return this._logger.zsymb(21, "BAMdLL", ["[getLastMediasOfConvWithoutCorrectData] - error: {}", "T8awn1"], e), [null, d]
                    }
                }
                async updateMedia(e, t, s, r, a) {
                    if (!Object(I.a)()) {
                        const t = Object(i.a)(Object(i.a)({}, r), {}, {
                            msgId: s
                        });
                        return g.a.updateMedia(t, e, a)
                    }
                    return this._updateMedia(e, t, s, r, a)
                }
                async countMediaOfConv(e, t, s) {
                    try {
                        t || Promise.resolve({
                            ok: !0,
                            result: 0,
                            error: null
                        });
                        const {
                            from: r,
                            to: a
                        } = s;
                        r.cliMsgId = r.cliMsgId ? r.cliMsgId : "", r.sendDttm = r.sendDttm ? r.sendDttm : 0, r.msgId = r.msgId ? r.msgId : "", a.cliMsgId = a.cliMsgId ? a.cliMsgId : n.MessageConstants.MAX_MSG_ID, a.sendDttm = a.sendDttm ? a.sendDttm : parseInt(n.MessageConstants.MAX_SENDDTTM), a.msgId = a.msgId ? a.msgId : n.MessageConstants.MAX_MSG_ID;
                        const o = this.getMediaRepository(e);
                        return {
                            ok: !0,
                            result: await o.countMediaOfConv(t, {
                                from: Object(i.a)({}, r),
                                to: Object(i.a)({}, a)
                            }),
                            error: null
                        }
                    } catch (a) {
                        var r;
                        const e = [null == a || null === (r = a.toString) || void 0 === r ? void 0 : r.call(a), null == a ? void 0 : a.stack].join("\n");
                        return this._logger.zsymb(21, "u2r1ch", ["[countMediaOfConv] - error: {}", "vOiVMU"], e), {
                            ok: !1,
                            result: 0,
                            error: a
                        }
                    }
                }
                async getMediaFieldsByMsgId(e, t) {
                    if (!t) return Promise.resolve(void 0);
                    const s = await b.default.getInstance().Core.Message.get(t, {
                        partition: e
                    });
                    return s ? {
                        cliMsgId: "number" == typeof s.cliMsgId ? s.cliMsgId.toString() : s.cliMsgId,
                        fromUid: s.fromUid,
                        toUid: s.toUid,
                        sendDttm: "string" == typeof s.sendDttm ? parseInt(s.sendDttm) : s.sendDttm
                    } : Promise.resolve(void 0)
                }
                async getAllValidPhotoAndVideosOfConv(e) {
                    if (!Object(I.a)()) return g.a.getAllMediaFromConv(e);
                    let t = !0,
                        s = {
                            cliMsgId: n.MessageConstants.MAX_MSG_ID,
                            sendDttm: parseInt(n.MessageConstants.MAX_SENDDTTM)
                        },
                        i = n.MessageConstants.MAX_MSG_ID,
                        r = "",
                        a = [],
                        o = [];
                    for (; t;) {
                        const d = await this.getValidMediasOfConv("image", e, 50, i, {}, {
                            lastItemOpts: {
                                cliMsgId: s.cliMsgId,
                                sendDttm: s.sendDttm
                            }
                        });
                        t = d.length >= 50;
                        let l = s,
                            c = r,
                            m = i;
                        if (d.length) {
                            if (d.forEach((e => {
                                    e && e.subType == n.MSG_SUBTYPE_MEDIA_VIDEO ? a.push(e) : !e || e.subType != n.MSG_SUBTYPE_PHOTO && e.subType !== n.MSG_SUBTYPE_MEDIA_DOODLE || o.push(e)
                                })), l = {
                                    cliMsgId: d[d.length - 1].cliMsgId,
                                    sendDttm: d[d.length - 1].sendDttm
                                }, m = d[d.length - 1].msgId, c = d[d.length - 1].mediaId, m && i && m == i || c && r && c == r) break;
                            s = l, r = c, i = m
                        }
                    }
                    return {
                        photos: o,
                        videos: a
                    }
                }
                async getAllValidPhotoAndVideosOfConvV2(e) {
                    if (!Object(I.a)()) return g.a.getAllMediaFromConv(e);
                    let t = [],
                        s = [];
                    const r = C.a.now();
                    let a = !1,
                        o = {
                            cliMsgId: n.MessageConstants.MAX_MSG_ID,
                            sendDttm: parseInt(n.MessageConstants.MAX_SENDDTTM)
                        };
                    do {
                        const r = await this.getMediaRepository("image").getLastMediasOfConvInNewDB(e, o, 50),
                            d = [],
                            l = r.map((e => e.cliMsgId)),
                            c = {
                                index: "cliMsgIdIndex",
                                partition: e
                            },
                            m = b.default.getInstance(),
                            u = (await m.Core.Message.getMultiIfExists(l, c)).reduce(((e, t) => (t && (e[`${t.cliMsgId}_${t.fromUid}_${t.toUid}`] = t), e)), {}),
                            g = [];
                        for (const e of r) g.push(new Promise((async t => {
                            try {
                                var s, r;
                                const t = Object(i.a)(Object(i.a)({}, e), {}, {
                                        msgId: null !== (s = null === (r = u[e.mediaId]) || void 0 === r ? void 0 : r.msgId) && void 0 !== s ? s : ""
                                    }),
                                    a = await this.getMediaMapper("image").toDTO(t, !1);
                                d.push(a)
                            } catch (a) {}
                            t()
                        })));
                        if (await Promise.all(g), d.length) {
                            d.forEach((e => {
                                e.subType == n.MSG_SUBTYPE_MEDIA_VIDEO ? s.push(e) : e.subType != n.MSG_SUBTYPE_PHOTO && e.subType != n.MSG_SUBTYPE_MEDIA_DOODLE || t.push(e)
                            }));
                            let e = {
                                cliMsgId: d[d.length - 1].cliMsgId,
                                sendDttm: d[d.length - 1].sendDttm
                            };
                            if (e.cliMsgId && o.cliMsgId == e.cliMsgId || e.sendDttm && o.sendDttm == e.sendDttm) break;
                            o = e
                        }
                        a = d.length >= 50
                    } while (a);
                    a = !1;
                    let d = n.MessageConstants.MAX_MSG_ID;
                    do {
                        const i = await this.getMediaRepository("image").getLastMediasOfConvInOldDB(e, {
                                msgId: d
                            }, 50),
                            r = await Promise.all(i.map((e => this.getMediaMapper("image").toDTO(e, !1))));
                        if (r.length) {
                            r.forEach((e => {
                                e.subType == n.MSG_SUBTYPE_MEDIA_VIDEO ? s.push(e) : e.subType != n.MSG_SUBTYPE_PHOTO && e.subType != n.MSG_SUBTYPE_MEDIA_DOODLE || t.push(e)
                            }));
                            let e = r[r.length - 1].msgId;
                            if (e && d == e) break;
                            d = e
                        }
                        a = r.length >= 50
                    } while (a);
                    return this._trackDBPerformance({
                        executionTime: C.a.now() - r,
                        actionName: "getAllByConv",
                        mediaType: "all",
                        resultSize: t.length + s.length
                    }), {
                        photos: t,
                        videos: s
                    }
                }
                async getImageMessagesForPhotoViewer(e, t, s, i, r, a) {
                    if (!Object(I.a)()) return y.default.getImagesForPhotoViewer({
                        userId: e
                    }, t, s, i, r, a);
                    try {
                        var o;
                        if (!e || !t) throw Error("convId or lastMsgId is undefined!");
                        const l = await this.getMediaFieldsByMsgId(e, t);
                        return await (null === (o = m.ModuleContainer.resolve(d.b)) || void 0 === o ? void 0 : o.getImageMessagesForPhotoViewer(e, s, {
                            sendDttm: (null == l ? void 0 : l.sendDttm) || parseInt(n.MessageConstants.MAX_SENDDTTM),
                            cliMsgId: (null == l ? void 0 : l.cliMsgId) || n.MessageConstants.MAX_MSG_ID,
                            msgId: t
                        }, i, r, a))
                    } catch (c) {
                        var l;
                        const e = [null == c || null === (l = c.toString) || void 0 === l ? void 0 : l.call(c), null == c ? void 0 : c.stack].join("\n");
                        return this._logger.zsymb(21, "ii0Vkm", ["[getImageMessagesForPhotoViewer] - error: {}", "_5G0Jw"], e), []
                    }
                }
                async getLastestAddedFiles(e, t = 40) {
                    if (!Object(I.a)()) return y.default.getLatestFiles(e, t);
                    try {
                        if (!e) {
                            e = Date.now();
                            let t = a.default.getTimeNow();
                            e = e > t ? e : t
                        }
                        const s = n.MessageConstants.MAX_CONVERSATION_ID,
                            i = parseInt(n.MessageConstants.MAX_MSG_ID),
                            r = m.ModuleContainer.resolve(l.b);
                        let o = await r.getLastestAddedFiles({
                            convId: s,
                            sendDttm: e,
                            cliMsgId: i
                        }, t);
                        if (o.length > 0) {
                            o = o.filter((e => !!e && !U.d.isDeleteMessage(e.convId, e)));
                            const e = this.getMediaMapper("file");
                            return await Promise.all(o.map((t => e.toDTO(t))))
                        }
                        return []
                    } catch (i) {
                        var s;
                        const e = [null == i || null === (s = i.toString) || void 0 === s ? void 0 : s.call(i), null == i ? void 0 : i.stack].join("\n");
                        return this._logger.zsymb(21, "wJhjke", ["[getLastestAddedFiles] - error: {}", "Qg9Jcl"], e), Promise.resolve([])
                    }
                }
                async getMultiMedias(e, t, s) {
                    if (!Object(I.a)()) return y.default.getMediaByIds(e, s);
                    try {
                        if (!s || s.length < 0) throw Error("msgIds is undefined or empty!");
                        const i = C.a.now(),
                            r = [],
                            a = s.map((e => this.getMediaFieldsByMsgId(t, e).then((async t => {
                                r.push({
                                    newId: t ? `${t.cliMsgId}_${t.fromUid}_${t.toUid}` : "",
                                    oldId: e,
                                    getFrom: await this._isMigrationDone() ? _.b.NEW : _.b.UNKNOWN
                                })
                            }))));
                        if (await Promise.all(a), !r.length) return [];
                        const n = (await this.getMediaRepository(e).getMultiMedias(r)).filter((e => !!e && !U.d.isDeleteMessage(t, e)));
                        return n.length > 0 ? (this._trackDBPerformance({
                            executionTime: C.a.now() - i,
                            actionName: "getByIds",
                            mediaType: e,
                            resultSize: n.length
                        }), await Promise.all(n.map((t => this.getMediaMapper(e).toDTO(t))))) : []
                    } catch (r) {
                        var i;
                        const e = [null == r || null === (i = r.toString) || void 0 === i ? void 0 : i.call(r), null == r ? void 0 : r.stack].join("\n");
                        return this._logger.zsymb(21, "BKMKyz", ["[getMultiMedias] - error: {}", "JSrZp4"], e), []
                    }
                }
                async getMediaById(e, t, s) {
                    try {
                        if (!s) throw Error("mediaId is undefined!");
                        const i = await this.getMediaFieldsByMsgId(t, s),
                            r = this.getMediaRepository(e),
                            a = await r.getMedia({
                                newId: i ? `${i.cliMsgId}_${i.fromUid}_${i.toUid}` : "",
                                oldId: s
                            }, void 0, await this._isMigrationDone() ? _.b.NEW : _.b.UNKNOWN);
                        return a ? await this.getMediaMapper(e).toDTO(a) : void 0
                    } catch (r) {
                        var i;
                        const e = [null == r || null === (i = r.toString) || void 0 === i ? void 0 : i.call(r), null == r ? void 0 : r.stack].join("\n");
                        return this._logger.zsymb(21, "RkrTYf", ["[getMediaById] - error: {}", "qyEH4V"], e), Promise.resolve(void 0)
                    }
                }
                async _updateMedia(e, t, s, i, r) {
                    try {
                        if (!t) throw Error("chatID isn't valid!");
                        if (!s) throw Error("mediaId isn't valid!");
                        if (!i) throw Error("mediaValue isn't valid!");
                        const r = this.getMediaRepository(e),
                            a = await this._mediaPrimaryKeyConvertor.toMediaPKFromMessagePK(t, s),
                            n = this.getMediaMapper(e).toNewEAttsFromDTOAtts(i);
                        return await r.updateMedia({
                            newId: a,
                            oldId: s
                        }, {
                            attributes: Object.keys(n),
                            value: n,
                            ignoreNotFound: !0
                        }, await this._isMigrationDone() ? _.c.NEW : _.c.UNKNOWN)
                    } catch (n) {
                        var a;
                        const e = [null == n || null === (a = n.toString) || void 0 === a ? void 0 : a.call(n), null == n ? void 0 : n.stack].join("\n");
                        return this._logger.zsymb(21, "ILB6Hf", ["[_updateMedia] - error: {}", "mxEfsB"], e), !1
                    }
                }
                async _addMediasWhenTransferMessageOldFlow(e) {
                    return B(e)
                }
                async _addMediasFromMessages(e, t) {
                    return z(e, t)
                }
                async _addMediasAtExportImportFlowOldFlow(e) {
                    if (!e || !e.length) return;
                    const t = [],
                        s = [],
                        r = [];
                    e.forEach((e => {
                        if (e.msgType === n.MSG_FILE) s.push({
                            msgId: e.msgId,
                            userId: e.toUid,
                            message: e.message,
                            sendDttm: e.sendDttm,
                            localPath: e.localPath,
                            fromUid: e.fromUid,
                            type: "file",
                            ttl: e.ttl
                        });
                        else if (e.msgType === n.MSG_CONTACT) "recommened.link" === e.message.action && r.push({
                            msgId: e.msgId,
                            userId: e.toUid,
                            message: e.message,
                            sendDttm: e.sendDttm,
                            fromUid: e.fromUid,
                            type: "link",
                            ttl: e.ttl
                        });
                        else if (e.msgType !== n.MSG_PHOTO && e.msgType !== n.MSG_PHOTO_2 && e.msgType !== n.MSG_DOODLE || Object(E.c)(e) || Object(E.H)(e) || Object(E.C)(e)) e.msgType === n.MSG_VIDEO && t.push({
                            msgId: e.msgId,
                            userId: e.toUid,
                            message: e.message,
                            sendDttm: e.sendDttm,
                            fromUid: e.fromUid,
                            type: "image",
                            subType: n.MSG_SUBTYPE_MEDIA_VIDEO,
                            ttl: e.ttl
                        });
                        else {
                            const s = {
                                msgId: e.msgId,
                                userId: e.toUid,
                                message: e.message,
                                sendDttm: e.sendDttm,
                                fromUid: e.fromUid,
                                type: "image",
                                subType: n.MSG_SUBTYPE_PHOTO,
                                ttl: e.ttl
                            };
                            if (e.msgType === n.MSG_DOODLE) {
                                const e = s.message.thumb || s.message.oriUrl;
                                s.subType = n.MSG_SUBTYPE_MEDIA_DOODLE, s.message = Object(i.a)(Object(i.a)({}, s.message), {}, {
                                    thumbUrl: e
                                })
                            }
                            t.push(s)
                        }
                    })), s.length && y.default.addFilesToConversation(s), r.length && y.default.addLinksToConversation(r), t.length && y.default.addOrUpdateImagesToConversation(t)
                }
                async _addMediasFromMessagesWhenBackupOldFlow(e) {
                    if (!e || !e.length) return;
                    const t = [],
                        s = [],
                        r = [];
                    e.forEach((e => {
                        if (e.msgType === n.MSG_FILE) s.push({
                            msgId: e.msgId,
                            userId: e.toUid,
                            message: e.message,
                            sendDttm: e.sendDttm,
                            localPath: e.localPath,
                            fromUid: e.fromUid,
                            ttl: e.ttl
                        });
                        else if (e.msgType === n.MSG_CONTACT && "recommened.link" === e.message.action) r.push({
                            msgId: e.msgId,
                            userId: e.toUid,
                            message: e.message,
                            sendDttm: e.sendDttm,
                            fromUid: e.fromUid,
                            ttl: e.ttl
                        });
                        else if (!(e.msgType !== n.MSG_PHOTO && e.msgType !== n.MSG_PHOTO_2 && e.msgType !== n.MSG_DOODLE || Object(E.c)(e) || Object(E.H)(e) || Object(E.C)(e))) {
                            let s = e.message;
                            if (e.msgType === n.MSG_DOODLE) {
                                const t = e.message.thumb || e.message.oriUrl;
                                s = Object(i.a)(Object(i.a)({}, e.message), {}, {
                                    thumbUrl: t
                                })
                            }
                            t.push({
                                msgId: e.msgId,
                                userId: e.toUid,
                                message: s,
                                sendDttm: e.sendDttm,
                                fromUid: e.fromUid,
                                ttl: e.ttl
                            })
                        }
                    })), s.length && y.default.addFilesToConversation(s), r.length && y.default.addLinksToConversation(r), t.length && y.default.addOrUpdateImagesToConversation(t)
                }
                async _emptyOldMediaStore(e, t) {
                    const s = {
                            from: [t, 0, ""],
                            to: [t, n.MessageConstants.MAX_MSG_ID.length, n.MessageConstants.MAX_MSG_ID],
                            excludeFrom: !1,
                            excludeTo: !1
                        },
                        i = {
                            index: "userId_sendDttm_msgId",
                            direction: v.c.PREV
                        },
                        r = await e.getAllKey(s, i);
                    null != r && r.length && await e.deleteMulti(r)
                }
                async _emptyNewMediaStore(e, t) {
                    const s = {
                            from: [t, 0, ""],
                            to: [t, parseInt(n.MessageConstants.MAX_SENDDTTM), n.MessageConstants.MAX_MSG_ID],
                            excludeFrom: !1,
                            excludeTo: !1
                        },
                        i = {
                            index: "convId_sendDttm_cliMsgId",
                            direction: v.c.PREV
                        },
                        r = await e.getAllKey(s, i);
                    null != r && r.length && await e.deleteMulti(r)
                }
                async updateFile(e, t, s, i) {
                    return Object(I.a)() ? this._updateMedia("file", e, t, s, i) : !!(await y.default.updateFile(t, s, i))
                }
                getMediaFromConversation(e, t, s, i, r) {
                    if (!Object(I.a)()) return g.a.getMediaFromConversation({
                        userId: t
                    }, e, s, i, r);
                    const a = e.substring(0, e.length - 1);
                    return this.getValidMediasOfConv(a, t, s, i, r)
                }
                async deleteMediaWhenDeleteMsg(e, t, s) {
                    if (!Object(I.a)()) return void y.default.deleteMediaItem(t);
                    let i = "";
                    if (null != s && s.mediaIdKeys) {
                        const {
                            cliMsgId: e,
                            fromUid: t,
                            convId: r
                        } = s.mediaIdKeys;
                        i = `${e}_${t}_${r}`
                    } else i = await this._mediaPrimaryKeyConvertor.toMediaPKFromMessagePK(e, t);
                    this.deleteMediasById("", e, {
                        newId: i,
                        oldId: t
                    })
                }
                async deleteMediaItems(e, t, s, i) {
                    if (!s) return [];
                    if (!Object(I.a)()) return g.a.deleteMediaItem(s, e);
                    let r = [];
                    r = null != i && i.mediaIdKeysArr && i.mediaIdKeysArr.length ? i.mediaIdKeysArr.map((e => `${e.cliMsgId}_${e.fromUid}_${e.convId}`)) : await Promise.all(s.map((e => this._mediaPrimaryKeyConvertor.toMediaPKFromMessagePK(t, e))));
                    const a = s.map(((e, t) => ({
                        oldId: e,
                        newId: r[t]
                    })));
                    return this.deleteMediasById(e, t, a)
                }
                async addLinksToConversation(e) {
                    Object(I.a)() ? this.addMedias("link", e) : y.default.addLinksToConversation(e)
                }
                async getFilesFromConversation(e, t, s) {
                    return Object(I.a)() ? this.getValidMediasOfConv("file", e, t, s, null) : y.default.getFilesFromConversation({
                        userId: e
                    }, t, s)
                }
                async addMediasWhenTransferMessage(e, t, s) {
                    return async function(e, t, s) {
                        return s ? z(e, t) : B(e)
                    }(e, t, s)
                }
                async addMediasAtExportImportFlow(e, t) {
                    return Object(I.a)() ? this._addMediasFromMessages(e, t) : this._addMediasAtExportImportFlowOldFlow(e)
                }
                async addMediasFromMessagesWhenBackup(e, t) {
                    return Object(I.a)() ? this._addMediasFromMessages(e, t) : this._addMediasFromMessagesWhenBackupOldFlow(e)
                }
                async deleteImageByMsgId(e, t, s) {
                    if (!Object(I.a)()) return y.default.deleteImageByMsgId(t);
                    let i = "";
                    return i = null != s && s.mediaIdKeys ? `${s.mediaIdKeys.cliMsgId}_${s.mediaIdKeys.fromUid}_${s.mediaIdKeys.convId}` : await this._mediaPrimaryKeyConvertor.toMediaPKFromMessagePK(e, t), await this.deleteMediasById("image", e, {
                        newId: i,
                        oldId: t
                    }), !0
                }
                async doAddMediaToConversation(e, t) {
                    return Object(I.a)() ? this.addMedias(e, t) : g.a.doAddMediaToConversation(t, e)
                }
                async updateMsgIdForRelativeMedia(e, t, s) {
                    if (Object(I.a)()) {
                        const i = (t, s, i) => {
                            this.getMediaById(t, e, i).then((r => {
                                r && (r.msgId = s, this.deleteMediasById(t, e, {
                                    newId: void 0,
                                    oldId: i
                                }).then((() => {
                                    this.addMedias(t, [r])
                                })))
                            }))
                        };
                        i("file", t, s), i("image", t, s), i("link", t, s)
                    } else {
                        const e = (e, t, s) => {
                                e.get(s).then((i => {
                                    i && (i.msgId = t, e.delete(s).then((() => {
                                        e.insert(i)
                                    })))
                                }))
                            },
                            i = b.default.getInstance();
                        e(i.Core.File, t, s), e(i.Core.Image, t, s), e(i.Core.Link, t, s)
                    }
                }
                async emptyMediaStores(e, t) {
                    e && t && t.length && (Object(I.a)() ? t.forEach((t => {
                        "file" === t ? this._emptyNewMediaStore(b.default.getInstance().Media.File, e) : "image" === t ? this._emptyNewMediaStore(b.default.getInstance().Media.Image, e) : "link" === t && this._emptyNewMediaStore(b.default.getInstance().Media.Link, e)
                    })) : t.forEach((t => {
                        "file" === t ? this._emptyOldMediaStore(b.default.getInstance().Core.File, e) : "image" === t ? this._emptyOldMediaStore(b.default.getInstance().Core.Image, e) : "link" === t && this._emptyOldMediaStore(b.default.getInstance().Core.Link, e)
                    })))
                }
                async addMediasFromOldDB(e, t) {
                    if (!t || !t.length) return [];
                    const s = this.getMediaRepository(e),
                        i = g.a.getAllDeleteConv();
                    let r = t,
                        a = t.map((e => e.msgId));
                    if (i) {
                        const s = t.reduce(((t, s) => {
                            var r;
                            const {
                                userId: a
                            } = s;
                            if (null !== (r = i[a]) && void 0 !== r && r[e]) {
                                const r = s.msgId,
                                    n = i[a][e].lastId;
                                n && r && n < r && t.oldMediaEntitiesShouldAddToNewDB.push(s)
                            } else t.oldMediaEntitiesShouldAddToNewDB.push(s);
                            return t.oldMediaIdsShouldDeleteFromOldDB.push(s.msgId), t
                        }), {
                            oldMediaEntitiesShouldAddToNewDB: [],
                            oldMediaIdsShouldDeleteFromOldDB: []
                        });
                        r = s.oldMediaEntitiesShouldAddToNewDB, a = s.oldMediaIdsShouldDeleteFromOldDB
                    }
                    const n = await s.correctMediasInOldDB(r, {
                            saveBack: !1
                        }),
                        o = this.getMediaMapper(e),
                        d = n.map((e => o.toDomainFromOldDomain(e))),
                        l = await s.insertMulti(d);
                    if (l.success.length) {
                        const e = l.success.map((e => o.toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e)));
                        return this._utilsMediaManager.createOrUpdateFromMedias(e), a
                    }
                    return []
                }
                async getMediasFromOldDB(e, t, s) {
                    if (await this._isMigrationDone()) return [];
                    const i = {
                            from: "",
                            to: s,
                            excludeFrom: !0,
                            excludeTo: !0
                        },
                        r = {
                            index: "primary",
                            direction: v.c.PREV,
                            limit: t
                        };
                    return this.getMediaRepository(e).getOldDBTable().getAll(i, r)
                }
                async deleteMediasFromOldDB(e, t) {
                    const s = this.getMediaRepository(e);
                    return (await s.getOldDBTable().deleteMulti(t)).success
                }
                async deleteMediasFromOldDBByRange(e, t, s) {
                    const i = {
                            from: "",
                            to: s,
                            excludeFrom: !0,
                            excludeTo: !0
                        },
                        r = {
                            index: "primary",
                            direction: v.c.PREV,
                            limit: t
                        },
                        a = this.getMediaRepository(e),
                        n = await a.getOldDBTable().getAllKey(i, r);
                    return (await a.getOldDBTable().deleteMulti(n)).success
                }
                async countTotalMediaInOldDB(e) {
                    try {
                        return this.getMediaRepository(e).getOldDBTable().count()
                    } catch (s) {
                        var t;
                        const e = [null == s || null === (t = s.toString) || void 0 === t ? void 0 : t.call(s), null == s ? void 0 : s.stack].join("\n");
                        return this._logger.zsymb(21, "dAVxgb", ["[countTotalMediaInOldDB] - error: {}", "hOPVfW"], e), 0
                    }
                }
                async _testAddMedias(e, t = 50) {
                    let s = [];
                    const i = 2 * Date.now() + Math.round(1e5 * Math.random()) + 1e3,
                        r = 5 * Date.now() + Math.round(1e7 * Math.random()) + 1e3,
                        a = ["0", "123456123111111456", "12345111111345", "111111111111111", "12345111111167890", "2121212121112222121", "1231231231232222123", "22222222222222222", "2111111111111222111", "322222222222222222", "12341234212341123", "9753434346787646", "1234123412341234", "987651235545454545", "123123455432123432", "21212123443434545", "5454544531321323", "98909087567564545645"],
                        o = ["4037840159631073270", ...JSON.parse('["101598415","124139871","147333052","169079931","194945127","200868372","225822710","276214855","355712826","361879215","910825795994501468","g100588026","g112969450","g1149397433596350988","g1352476194718464059","g147262698","g149205131","g158719108","g158754949","g160924468","g164283274","g175572981","g194334627","g230291933","g2426404535463764819","g245533229","g25193586","g257364624","g263690118","g285979907","g285992315","g28757702","g288999008","g289367881","g293290840","g301757910","g302249930","g302577276","g309222362","g312505929","g317894641","g318028102","g320951866","g321466298","g322706780","g325920125","g325933426","g328827910","g331912501","g34509241","g47431271","g5355130437069108654","g55402697","g6911969691454201107","g85951308"]')];
                    switch (e) {
                        case "image":
                            for (let e = 0; e < t; e++) {
                                const t = a[Math.round(Math.random() * (a.length - 1))],
                                    d = o[Math.round(Math.random() * (o.length - 1))],
                                    l = i + e,
                                    c = r + e;
                                s.push({
                                    userId: d,
                                    cliMsgId: l,
                                    fromUid: t,
                                    msgId: c.toString(),
                                    message: {
                                        hdUrl: "https://b-f62-zpg-r.zdn.vn/8236607293085145247/0d5e94a8fc3726697f26.jpg?e2esession=QIqxr2vdpiBkZh0yizDjKzJIO2LACOAaSb+KBDlUkbwlil6lySe5/p1RncUJJLfTJj1Co/AkPZAOh6ln",
                                        oriUrl: "https://b-f62-zpg-r.zdn.vn/8236607293085145247/0d5e94a8fc3726697f26.jpg?e2esession=QIqxr2vdpiBkZh0yizDjKzJIO2LACOAaSb+KBDlUkbwlil6lySe5/p1RncUJJLfTJj1Co/AkPZAOh6ln",
                                        params: '{"width":800,"height":800,"hd":"https://b-f62-zpg-r.zdn.vn/8236607293085145247/0d5e94a8fc3726697f26.jpg?e2esession=QIqxr2vdpiBkZh0yizDjKzJIO2LACOAaSb+KBDlUkbwlil6lySe5/p1RncUJJLfTJj1Co/AkPZAOh6ln","rawThumbUrl":"https://b-f62-zpg-r.zdn.vn/8236607293085145247/0d5e94a8fc3726697f26.jpg"}',
                                        thumbUrl: "https://f62-zpg-r.zdn.vn/1518777612491889561/5571c987a1187b462209.jpg?e2esession=YBl+XgC1U90bnMk4yPcU519rhnRW4KoItoeNXLpRsl/secNpaT3Nxkal57Nj3UZhy2rUFgxhf8Pcjwah",
                                        title: void 0
                                    },
                                    type: "image",
                                    isExpired: !1,
                                    isExpiredAll: !1,
                                    subType: Math.round(Math.random()) ? n.MSG_SUBTYPE_PHOTO : n.MSG_SUBTYPE_MEDIA_VIDEO,
                                    sendDttm: Date.now() + e,
                                    ttl: 0,
                                    localPath: null,
                                    previewThumb: void 0,
                                    updateTime: Date.now(),
                                    width: 0,
                                    height: 0
                                })
                            }
                            break;
                        case "file":
                            for (let e = 0; e < t; e++) {
                                const t = a[Math.round(Math.random() * (a.length - 1))],
                                    n = o[Math.round(Math.random() * (o.length - 1))],
                                    d = i + e,
                                    l = r + e;
                                s.push({
                                    userId: n,
                                    cliMsgId: d,
                                    fromUid: t,
                                    msgId: l.toString(),
                                    message: {
                                        href: "https://f27-group-zfr.zdn.vn/7f8b522a54e0babee3f1/117090926800068678?e2esession=RSHmz0Kj28vN9gMz6oFilBjFxVFqisUPnj3XpjlolXXpTo3bwJ0hqlUl5uR6Sr/wtHrtcyXqXj/mLYl3",
                                        params: '{"fileSize":8178,"checksum":"78c2c03eeab27abbdf28b0fe25088e30","checksumSha":"","fileExt":"xlsx","fdata":"{}","fType":1}',
                                        title: "program_matrix_laP5chSikftFYc_LmERud.xlsx"
                                    },
                                    type: "file",
                                    fileType: -1,
                                    extType: "",
                                    sendDttm: Date.now() + e,
                                    ttl: 0,
                                    updateTime: Date.now(),
                                    localPath: null,
                                    folderPath: null,
                                    previewThumb: "",
                                    dimension: null,
                                    downloadError: !1,
                                    isExpired: !1,
                                    isExpiredAll: !1
                                })
                            }
                            break;
                        case "link":
                            for (let e = 0; e < t; e++) {
                                const t = a[Math.round(Math.random() * (a.length - 1))],
                                    n = o[Math.round(Math.random() * (o.length - 1))],
                                    d = i + e,
                                    l = r + e;
                                s.push({
                                    userId: n,
                                    cliMsgId: d,
                                    fromUid: t,
                                    msgId: l.toString(),
                                    message: {
                                        action: "recommened.link",
                                        childnumber: 0,
                                        description: "http://tinhte.vn",
                                        href: "http://tinhte.vn",
                                        oriUrl: "http://tinhte.vn",
                                        params: '{"mediaTitle":"http://tinhte.vn","artist":"","src":"tinhte.vn","stream_icon":"","streamUrl":"","type":0,"subType":3}',
                                        thumb: "",
                                        thumbUrl: "",
                                        title: "tinhte.vn",
                                        type: ""
                                    },
                                    type: "link",
                                    sendDttm: Date.now() + e,
                                    ttl: 0,
                                    updateTime: Date.now(),
                                    previewThumb: ""
                                })
                            }
                    }
                    const d = this._getMediaDB(e, !1);
                    await (null == d ? void 0 : d.insertMulti(s))
                }
                _getMediaDB(e, t) {
                    const s = b.default.getInstance();
                    switch (e) {
                        case "image":
                            return t ? s.Media.Image : s.Core.Image;
                        case "file":
                            return t ? s.Media.File : s.Core.File;
                        case "link":
                            return t ? s.Media.Link : s.Core.Link
                    }
                }
                _trackDBPerformance(e) {
                    Object(S.b)().logInfo(S.a.LoadMedia, {
                        duration: e.executionTime,
                        action_name: e.actionName,
                        media_type: e.mediaType,
                        result_size: e.resultSize
                    })
                }
            }) || x) || x) || x) || x) || x) || x) || x) || x)
        },
        "3dEI": function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("media-repository-factory")
        },
        "4ePI": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return f
            }));
            var i = s("VTBJ"),
                r = s("NDmK"),
                a = s("fsN4"),
                n = s("GbHB"),
                o = s("1pet"),
                d = s("K4Ns"),
                l = s("wH4e"),
                c = s("8Nax"),
                m = s("z0WU"),
                u = s("XS0u"),
                g = s("8tev"),
                h = s("yWVE"),
                p = s("ma9L");
            class f {
                constructor(e, t, s, i, r, n) {
                    this._dbInstance = void 0, this._mediaMigrationManager = void 0, this._dalInstance = a.default.getInstance(), this.dbTable = void 0, this.logger = void 0, this.mediaType = void 0, this.dbTable = this._dalInstance[e][t], this._dbInstance = this._dalInstance[e], this.mediaType = s, this._mediaMigrationManager = r, this.logger = n.createZLogger(`${e}-${t}-repository`, [i])
                }
                async insert(e, t) {
                    if (!e) throw Error("[insert]: item is undefined!");
                    if (!e.cliMsgId || !e.fromUid || !e.convId) throw Error(`[insert]: item doesn't have valid key_from_to: ${e.cliMsgId}_${e.fromUid}_${e.convId}`);
                    return e.mediaId = `${e.cliMsgId}_${e.fromUid}_${e.convId}`, !!(await this.dbTable.insert(e, t))
                }
                insertMulti(e, t) {
                    return Object(p.a)(this.dbTable, e, t)
                }
                async update(e, t) {
                    if (!f.isNewMediaIdFormat(e)) throw Error("[update]: mediaId doesn't have new media id format!");
                    return !!(await this.dbTable.update(e, t))
                }
                async updateMedia(e, t, s = d.c.UNKNOWN) {
                    if (!e) throw Error("[updateMedia]: mediaIdObj is undefined!");
                    const {
                        newId: r,
                        oldId: a
                    } = e;
                    if ((s === d.c.NEW || await this._isMigrationDone()) && r) return !!(await this.dbTable.update(r, t));
                    if (s === d.c.OLD && a) {
                        let e;
                        if (t) {
                            const s = this.getMediaMapper().toOldEAttsFromNewEAtts(t.value || {});
                            return e = Object(i.a)(Object(i.a)({}, t), {}, {
                                attributes: Object.keys(s),
                                value: s
                            }), !!(await this.getOldDBTable().update(a, e))
                        }
                        throw Error("options is undefined!")
                    }
                    try {
                        if (r) {
                            if (await this.dbTable.get(r)) return !!(await this.dbTable.update(r, t))
                        }
                        if (a) {
                            let e;
                            if (t) {
                                const s = this.getMediaMapper().toOldEAttsFromNewEAtts(t.value || {});
                                return e = Object(i.a)(Object(i.a)({}, t), {}, {
                                    attributes: Object.keys(s),
                                    value: s
                                }), !!(await this.getOldDBTable().update(a, e))
                            }
                            throw Error("options is undefined!")
                        }
                        throw Error(`${r} or ${a} isn't valid!`)
                    } catch (n) {
                        throw Error(`[updateMedia] - err: ${n.message}`)
                    }
                }
                async updateMulti(e) {
                    return await this.dbTable.updateMulti(e)
                }
                async updateMultiMedias(e) {
                    throw Error("This updateMultiMedias isn't implemented!")
                }
                delete(e, t) {
                    if (!f.isNewMediaIdFormat(e)) throw Error("[delete]: mediaId doesn't have new media id format!");
                    return this.dbTable.delete(e, t)
                }
                deleteMulti(e, t) {
                    if (!e || !e.length) throw Error("[deleteMulti]: mediaIds is undefined or empty!");
                    if (e.some((e => !f.isNewMediaIdFormat(e)))) throw Error("[deleteMulti]: mediaIds contains an id which doesn't have new media id format!");
                    return this.dbTable.deleteMulti(e, t)
                }
                async deleteMedia(e, t, s = d.a.UNKNOWN) {
                    if (!e) throw Error("[deleteMedia]: mediaObj is undefined!");
                    const {
                        newId: i,
                        oldId: r
                    } = e;
                    if ((s === d.a.NEW || await this._isMigrationDone()) && i) return this.dbTable.delete(i);
                    if (s === d.a.OLD && r) return this.getOldDBTable().delete(r, t);
                    {
                        const e = [];
                        i && e.push(this.dbTable.delete(i, t)), r && e.push(this.getOldDBTable().delete(r, t));
                        return (await Promise.allSettled(e)).every((e => "fulfilled" === e.status && e.value))
                    }
                }
                async deleteMultiMedias(e, t) {
                    if (!e || !e.length) throw Error("[deleteMultiMedias]: mediaIdObjs is undefined or empty!");
                    const s = {
                            success: [],
                            fail: []
                        },
                        i = [];
                    for (const {
                            newId: r,
                            oldId: a,
                            deleteTo: n
                        }
                        of e) {
                        const e = (n === d.a.NEW ? r : n === d.a.OLD ? a : r || a || "") || "",
                            o = async () => {
                                try {
                                    await this.deleteMedia({
                                        newId: r,
                                        oldId: a
                                    }, t, n) && s.success.push(e)
                                } catch (o) {
                                    var i;
                                    const t = [null == o || null === (i = o.toString) || void 0 === i ? void 0 : i.call(o), null == o ? void 0 : o.stack].join("\n");
                                    this.logger.zsymb(18, "m8o8Sp", t), s.fail.push(e)
                                }
                            };
                        i.push(o)
                    }
                    return await Promise.allSettled(i.map((e => e()))), s
                }
                async get(e, t) {
                    if (!f.isNewMediaIdFormat(e)) throw Error("[get]: mediaId doesn't have new media id format!");
                    return this.dbTable.get(e, t)
                }
                async getMulti(e, t) {
                    const {
                        newIdFormats: s,
                        oldIdFormats: i
                    } = e.reduce(((e, t) => (f.isNewMediaIdFormat(t) && e.newIdFormats.push(t), e)), {
                        newIdFormats: [],
                        oldIdFormats: []
                    });
                    let r = [];
                    if (s.length) {
                        const e = await this.dbTable.getMulti(s, t);
                        e.length && r.push(...e)
                    }
                    return r
                }
                async getMedia(e, t, s = d.b.UNKNOWN) {
                    if (!e) throw Error("[getMedia]: mediaIdObj params is undefined!");
                    const {
                        newId: i,
                        oldId: r
                    } = e;
                    if ((s === d.b.NEW || await this._isMigrationDone()) && i) return this.dbTable.get(i, t);
                    if (s === d.b.OLD && r) {
                        const e = await this.getOldDBTable().get(r, t);
                        if (e) return this.getMediaMapper().toDomainFromOldDomain(e)
                    } else {
                        if (i) {
                            const e = await this.dbTable.get(i, t);
                            if (e) return e
                        }
                        if (r) {
                            const e = await this.getOldDBTable().get(r, t);
                            if (e) return this.getMediaMapper().toDomainFromOldDomain(e)
                        }
                    }
                }
                async getMultiMedias(e, t) {
                    if (!e || !e.length) throw Error("[getMultiMedias]: mediaIdObjs is undefined or empty!");
                    let s = [];
                    const i = [];
                    for (const {
                            newId: r,
                            oldId: a,
                            getFrom: n
                        }
                        of e) i.push(this.getMedia({
                        newId: r,
                        oldId: a
                    }, t, "number" == typeof n ? n : d.b.UNKNOWN));
                    return s = (await Promise.allSettled(i)).map((e => "fulfilled" === e.status ? e.value : void 0)), s
                }
                getAll(e, t) {
                    if (!e) throw Error("[getAll]: keyRange is undefined!");
                    return this.dbTable.getAll(e, t)
                }
                async getAllInOldDB(e, t) {
                    if (await this._isMigrationDone()) return [];
                    if (!e) throw Error("[getAllInOldDB]: keyRange is undefined!");
                    const s = await this.getOldDBTable().getAll(e, t);
                    return null != s && s.length ? s.map((e => this.getMediaMapper().toDomainFromOldDomain(e))) : []
                }
                static isNewMediaIdFormat(e) {
                    if (!e) return !1;
                    const t = e.split("_");
                    return 3 === t.length && t.every(Boolean)
                }
                static filterMedia(e, t) {
                    if (!(t && (t.member || t.date && (t.date.start || t.date.end) || t.name || t.ext))) return !0;
                    if (!t.member || e.fromUid && e.fromUid === t.member) {
                        let i, r, a;
                        if (t.name) {
                            if (e.message.params && e.message.href) try {
                                let t = JSON.parse(e.message.params);
                                i = t.mediaTitle ? m.default.simpleStripVietnamese(t.mediaTitle) : m.default.simpleStripVietnamese(e.message.title), r = t.src
                            } catch (s) {
                                return m.default.logCoreError(s), !1
                            }
                            a = m.default.simpleStripVietnamese(t.name)
                        }
                        if (!t.name || a.length <= i.length && m.default.searchContent(i, a) || m.default.searchContent(r, a)) {
                            let i = !1;
                            if (t.ext && (i = o.fileExt[t.ext].some((t => {
                                    if (!e.message.title) {
                                        const t = n.a.decrypt(e.message, u.default.UIN, !1);
                                        try {
                                            e.message = JSON.parse(t)
                                        } catch (s) {
                                            e.message = {
                                                title: ""
                                            }
                                        }
                                    }
                                    return e.message.title.endsWith(t)
                                }))), !t.ext || i) return null == t.date.start && null == t.date.end || (e.sendDttm >= t.date.start ? e.sendDttm <= t.date.end : 0)
                        }
                    }
                    return !1
                }
                async isExistedMedia(e) {
                    let t = !1;
                    if (e) {
                        const {
                            newId: s,
                            oldId: i
                        } = e;
                        s && (t = !!(await this.dbTable.get(s))), i && !t && (t = !!(await this.getOldDBTable().get(i)))
                    }
                    return t
                }
                async getLastMediasOfConv(e, t, s = 100) {
                    if (!e || !t) throw Error(`[getLastMediasOfConv]: convId: ${e}, lastItemOptions:${JSON.stringify(t)} isn't valid!`);
                    const {
                        sendDttm: i,
                        cliMsgId: r,
                        msgId: a
                    } = t, n = await this.getLastMediasOfConvInNewDB(e, {
                        sendDttm: i,
                        cliMsgId: r
                    }, s);
                    return n.length < s && !(await this._isMigrationDone()) && n.push(...await this.getLastMediasOfConvInOldDB(e, {
                        msgId: a
                    }, s - n.length)), n
                }
                async countMediaOfConv(e, t) {
                    let s = 0;
                    if (!(e && t && t.from && t.to)) throw Error(`[countMediaOfConv]: convId: ${e}, options: ${JSON.stringify(t)} isn't valid!`);
                    const {
                        from: i,
                        to: r
                    } = t;
                    return s = await this.countMediaOfConvInNewDB(e, {
                        from: {
                            sendDttm: i.sendDttm,
                            cliMsgId: i.cliMsgId
                        },
                        to: {
                            sendDttm: r.sendDttm,
                            cliMsgId: r.cliMsgId
                        }
                    }), await this._isMigrationDone() || (s += await this.countMediaOfConvInOldDB(e, {
                        from: {
                            msgId: i.msgId
                        },
                        to: {
                            msgId: r.msgId
                        }
                    })), s
                }
                countMediaOfConvInNewDB(e, t) {
                    const {
                        from: s,
                        to: i
                    } = t, r = {
                        from: [e, s.sendDttm, s.cliMsgId],
                        to: [e, i.sendDttm, i.cliMsgId],
                        excludeFrom: !1,
                        excludeTo: !1
                    };
                    return this.dbTable.count(r, {
                        index: "convId_sendDttm_cliMsgId"
                    })
                }
                async countMediaOfConvInOldDB(e, t) {
                    if (await this._isMigrationDone()) return 0;
                    const {
                        from: s,
                        to: i
                    } = t, r = {
                        from: [e, s.msgId.length, s.msgId],
                        to: [e, i.msgId.length, i.msgId],
                        excludeFrom: !1,
                        excludeTo: !1
                    };
                    return this.getOldDBTable().count(r, {
                        index: "userId_sendDttm_msgId"
                    })
                }
                async getMediasOfConv(e, t, s, i) {
                    if (!e || !t || !s) return Promise.resolve([]);
                    let a = [];
                    if (r.default.load_media.optimize_mode && (a = await this._isMigrationDone() ? await this.getMediasOfConvOptimizeMode(e, t, s, i) : await this.getMediasOfConvOptimizeModeInMixedDB(e, t, s, i)), null != i && i.deletePointOfConv) {
                        const {
                            lastId: e,
                            lastSendDttm: t,
                            lastCliMsgId: s
                        } = i.deletePointOfConv;
                        return a.filter((i => !(e && i.msgId && i.msgId <= e) && (!(t && i.sendDttm <= t) && !(s && i.cliMsgId <= s))))
                    }
                    return a
                }
                async getMediasOfConvOptimizeModeInMixedDB(e, t, s, i) {
                    let r = !1,
                        a = !1;
                    i && i.date && (null !== i.date.start || null !== i.date.end) && (r = !0);
                    const n = [],
                        {
                            sendDttm: o,
                            cliMsgId: d
                        } = await this.getSendDttmOfLatestMediaInOldDB(e),
                        {
                            sendDttm: u,
                            cliMsgId: g,
                            msgId: p
                        } = t,
                        M = {
                            from: [e, 0, ""],
                            to: [e, u, g],
                            excludeFrom: !0,
                            excludeTo: !0
                        },
                        _ = {
                            index: "convId_sendDttm_cliMsgId",
                            limit: s,
                            direction: l.c.PREV,
                            useLGKeyRange: !0,
                            predicate: e => {
                                if (!e) return !1;
                                if (!m.default.mapHasItem(e.content)) return !1;
                                const t = c.a.instance.filterDBMessage(e),
                                    s = Object(h.e)(e);
                                if (!t || !s) return !1;
                                let r = f.filterMedia({
                                    message: e.content,
                                    fromUid: e.fromUid,
                                    sendDttm: e.sendDttm
                                }, i);
                                return 0 === r && (a = !0), !!r && (e.sendDttm > o && e.cliMsgId > d || (n.push(e), !1))
                            },
                            aborted: r ? () => a : void 0
                        };
                    try {
                        let t = await this.dbTable.getAll(M, _);
                        if (t.length < s) {
                            const r = await this.getMediasOfConvOptimizeModeInOldDB(e, {
                                    msgId: p
                                }, s - t.length, i),
                                a = [];
                            r.length > 0 ? n.length > 0 ? (r.forEach((e => {
                                const t = n[0];
                                t && e.sendDttm <= t.sendDttm && e.cliMsgId <= t.cliMsgId ? (a.push(t), n.shift()) : a.push(e)
                            })), n.length > 0 && a.push(...n), t.push(...a.slice(0, s - t.length))) : t.push(...r) : n.length > 0 && t.push(...n)
                        }
                        return t
                    } catch (b) {
                        const t = [e, this.mediaType, s, u, g, !!i].join("_");
                        throw this.logger.zsymb(21, "irxkIG", ["[getMediasOfConvOptimizeModeInMixedDB] errInfo: {}", "WnbJy6"], t), b
                    }
                }
                async getSendDttmOfLatestMediaInOldDB(e) {
                    const t = {
                            from: [e, 0, ""],
                            to: [e, o.MessageConstants.MAX_SENDDTTM, o.MessageConstants.MAX_MSG_ID],
                            excludeFrom: !1,
                            excludeTo: !1
                        },
                        s = {
                            index: "userId_sendDttm_msgId",
                            limit: 1,
                            direction: l.c.PREV
                        },
                        i = await this.getOldDBTable().getAll(t, s);
                    if (!i || !i[0]) return {
                        sendDttm: 0,
                        cliMsgId: "0"
                    };
                    return {
                        sendDttm: "string" == typeof i[0].sendDttm ? parseInt(i[0].sendDttm) : i[0].sendDttm,
                        cliMsgId: "" + (i[0].cliMsgId || 0)
                    }
                }
                async getMediasOfConvOptimizeMode(e, t, s, i) {
                    if (!e || !t || !s) throw Error(`[getMediasOfConvOptimizeMode]: convId: ${e}, lastItemOptions: ${JSON.stringify(t)}, limit: ${s} isn't valid!`);
                    const {
                        sendDttm: r,
                        cliMsgId: a,
                        msgId: n
                    } = t, o = await this.getMediasOfConvOptimizeModeInNewDB(e, {
                        sendDttm: r,
                        cliMsgId: a
                    }, s, i);
                    return o.length < s && !(await this._isMigrationDone()) && o.push(...await this.getMediasOfConvOptimizeModeInOldDB(e, {
                        msgId: n
                    }, s - o.length, i)), o
                }
                getMediasOfConvOptimizeModeInNewDB(e, t, s, i) {
                    let r = !1,
                        a = !1;
                    i && i.date && (null !== i.date.start || null !== i.date.end) && (r = !0);
                    const {
                        sendDttm: n,
                        cliMsgId: o
                    } = t, d = {
                        from: [e, 0, ""],
                        to: [e, n, o],
                        excludeFrom: !0,
                        excludeTo: !0
                    }, u = {
                        index: "convId_sendDttm_cliMsgId",
                        limit: s,
                        direction: l.c.PREV,
                        predicate: e => {
                            if (!e) return !1;
                            if (!m.default.mapHasItem(e.content)) return !1;
                            const t = c.a.instance.filterDBMessage(e),
                                s = Object(h.e)(e);
                            if (!t || !s) return !1;
                            let r = f.filterMedia({
                                message: e.content,
                                fromUid: e.fromUid,
                                sendDttm: e.sendDttm
                            }, i);
                            return 0 === r && (a = !0), !!r
                        },
                        aborted: r ? () => a : void 0
                    };
                    try {
                        return this.dbTable.getAll(d, u)
                    } catch (g) {
                        const t = [e, this.mediaType, s, n, o, !!i].join("_");
                        throw this.logger.zsymb(21, "0ZLW4S", ["[getMediasOfConvOptimizeModeInNewDB] errInfo: {}", "lBUVJu"], t), g
                    }
                }
                async getMediasOfConvOptimizeModeInOldDB(e, t, s, i) {
                    if (await this._isMigrationDone()) return [];
                    let r = !1,
                        a = !1;
                    i && i.date && (null != i.date.start || null != i.date.end) && (r = !0);
                    const {
                        msgId: n
                    } = t, o = {
                        from: [e, 0, ""],
                        to: [e, n.length, n],
                        excludeFrom: !0,
                        excludeTo: !0
                    }, d = {
                        index: "userId_sendDttm_msgId",
                        limit: s,
                        direction: l.c.PREV,
                        useLGKeyRange: !0,
                        predicate: e => {
                            if (e) {
                                if (e.msgId && e.msgId.includes("_")) return !1;
                                if (!m.default.mapHasItem(e.message)) return !1
                            }
                            let t = c.a.instance.filterDBMessage(e),
                                s = Object(h.e)(e);
                            if (!t || !s) return !1;
                            let r = f.filterMedia(e, i);
                            return 0 === r && (a = !0), !!r
                        },
                        aborted: r ? () => a : void 0
                    };
                    try {
                        const e = await this.getOldDBTable().getAll(o, d);
                        return (await this.correctMediasInOldDB(e)).map((e => this.getMediaMapper().toDomainFromOldDomain(e)))
                    } catch (u) {
                        const t = [e, this.mediaType, s, n, !!i].join("_");
                        throw this.logger.zsymb(21, "Rj04j7", ["[getMediasOfConvOptimizeModeInOldDB] errInfo: {}", "NFyoCu"], t), u
                    }
                }
                getLastMediasOfConvInNewDB(e, t, s) {
                    const {
                        sendDttm: i,
                        cliMsgId: r
                    } = t, a = {
                        from: [e, 0, ""],
                        to: [e, i, r],
                        excludeFrom: !1,
                        excludeTo: !1
                    }, n = {
                        index: "convId_sendDttm_cliMsgId",
                        limit: s,
                        direction: l.c.PREV
                    };
                    return this.dbTable.getAll(a, n)
                }
                async getLastMediasOfConvInOldDB(e, t, s) {
                    if (await this._isMigrationDone()) return [];
                    const {
                        msgId: r
                    } = t, a = {
                        from: [e, 0, ""],
                        to: [e, r.length, r],
                        excludeFrom: !1,
                        excludeTo: !1
                    }, n = {
                        index: "userId_sendDttm_msgId",
                        limit: s,
                        direction: l.c.PREV
                    }, o = await this.getOldDBTable().getAll(a, n);
                    return o.length > 0 ? o.map((e => Object(i.a)(Object(i.a)({}, this.getMediaMapper().toDomainFromOldDomain(e)), {}, {
                        msgId: e.msgId
                    }))) : []
                }
                correctMediasInOldDB(e, t = {
                    saveBack: !0
                }) {
                    return e && e.length ? new Promise((s => {
                        let i = [],
                            r = {},
                            a = [];
                        if (e.forEach((e => {
                                if (e && e.msgId && e.sendDttm) {
                                    const t = null == e.fromUid;
                                    e.type && e.cliMsgId && !t ? a.push(e) : (i.push(e.msgId), r[e.msgId] = e)
                                }
                            })), !i.length) return s(a);
                        {
                            let n = "";
                            const d = (e, t) => {
                                    let s = !0;
                                    switch (t.msgType) {
                                        case o.MSG_PHOTO:
                                        case o.MSG_PHOTO_2:
                                            n = "image", e.subType = o.MSG_SUBTYPE_PHOTO;
                                            break;
                                        case o.MSG_VIDEO:
                                            n = "image", e.subType = o.MSG_SUBTYPE_MEDIA_VIDEO;
                                            break;
                                        case o.MSG_FILE:
                                            n = "file";
                                            break;
                                        case o.MSG_CONTACT:
                                            "object" == typeof t.message && "recommened.link" === t.message.action && (n = "link")
                                    }
                                    e.cliMsgId = t.cliMsgId, e.type = n;
                                    return "" !== t.fromUid && null != t.fromUid ? (s = !0, e.fromUid = t.fromUid + "") : s = !1, {
                                        isSuccess: s,
                                        data: s ? e : null
                                    }
                                },
                                l = e[0].userId;
                            u.default.getConvMessagesByIdsInQueue(i, l).then((e => {
                                const i = [],
                                    n = [];
                                if (e) {
                                    for (let s in e)
                                        if (e[s]) {
                                            const t = r[s];
                                            delete r[s];
                                            const {
                                                isSuccess: a,
                                                data: o
                                            } = d(t, e[s]);
                                            a && o ? i.push(o) : n.push(s)
                                        } const t = Object.getOwnPropertyNames(r);
                                    t.length && t.forEach((e => {
                                        n.push(e)
                                    }))
                                }
                                let o = a.concat(i);
                                return o.sort(((e, t) => parseInt(t.sendDttm) - parseInt(e.sendDttm))), i.length && t.saveBack && this.deleteAndInsertInOldDB(i), n.length && this.getOldDBTable().deleteMulti(n), s(o)
                            })).catch((e => s(a)))
                        }
                    })) : Promise.resolve([])
                }
                async deleteAndInsertInOldDB(e) {
                    if (!e || !e.length) throw Error(`[deleteAndInsertInOldDB]: oldItems: ${JSON.stringify(e)} isn't valid!`);
                    return this.getOldDBTable().insertMulti(e, {
                        replace: !0
                    })
                }
                async _isMigrationDone() {
                    return (await this._mediaMigrationManager.getMediaMigrationState()).stateName === g.b.DONE
                }
            }
        },
        "4pKu": function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("utils-media-repository")
        },
        "5R0e": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            })), s.d(t, "b", (function() {
                return n
            }));
            var i = s("4ePI"),
                r = s("jDHv");
            class a extends i.a {
                constructor(e, t, s, i, r, a) {
                    super(e, t, s, i, r, a)
                }
            }
            const n = Object(r.define)("link-media-repository")
        },
        "666L": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = function(e) {
                return e.PROGRESS = "progress", e.VERSION = "version", e.TOTAL_ITEMS = "totalItems", e.TOTAL_MESSAGES = "totalMessages", e.START_AT = "startAt", e
            }(i || {});
            class r {
                constructor(e) {
                    this._progress = void 0, this.adapter = void 0, this.initialized = !1, this.adapter = e
                }
                get progress() {
                    return this._progress || (this._progress = this.init()), this._progress
                }
                getProgressKeys() {
                    return Object.keys(this.progress)
                }
                setProgressKeys(e) {
                    this._progress = Object.fromEntries(e.map((e => [e, 0]))), this.adapter.set(i.PROGRESS, this.progress)
                }
                setTotalProgressKeys(e) {
                    this.adapter.set(i.TOTAL_ITEMS, e)
                }
                getTotalProgressKeys() {
                    return this.adapter.get(i.TOTAL_ITEMS)
                }
                setTotalMessage(e) {
                    this.adapter.set(i.TOTAL_MESSAGES, e)
                }
                getTotalMessage() {
                    return this.adapter.get(i.TOTAL_MESSAGES) || 0
                }
                getProgressData(e) {
                    return this.progress[e]
                }
                setProgressData(e, t) {
                    this.progress[e] = t, this.adapter.set(i.PROGRESS, this.progress)
                }
                clearAllProgressData() {
                    this._progress = {}, this.adapter.clearAll()
                }
                clearProgressData(e) {
                    delete this.progress[e], this.adapter.set(i.PROGRESS, this.progress)
                }
                setVersion(e) {
                    this.adapter.set(i.VERSION, e)
                }
                getVersion() {
                    return this.adapter.get(i.VERSION)
                }
                clearVersion() {
                    this.adapter.clear(i.VERSION)
                }
                setStartAt(e) {
                    this.adapter.set(i.START_AT, e)
                }
                getStartAt() {
                    return this.adapter.get(i.START_AT)
                }
                clearStartAt() {
                    this.adapter.clear(i.START_AT)
                }
                persistStorage() {
                    this.adapter.persist()
                }
                init() {
                    return this.adapter.init(), this.adapter.get(i.PROGRESS) || {}
                }
            }
        },
        "7+QW": function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            })), s.d(t, "b", (function() {
                return r
            }));
            const i = {
                    Cache: 0,
                    file: 1,
                    fileNoise: 2,
                    fileThumb: 3,
                    picture: 4,
                    richThumb: 5,
                    video: 6,
                    voice: 7,
                    zinstant: 8,
                    zcloud: 9,
                    resource: 10
                },
                r = {
                    file: ["file", "fileNoise"],
                    photo: ["picture"],
                    video: ["video"],
                    voice: ["voice"],
                    other: ["fileThumb", "richThumb", "zinstant"]
                }
        },
        "7odm": function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return i
            })), s.d(t, "e", (function() {
                return r
            })), s.d(t, "d", (function() {
                return a
            })), s.d(t, "a", (function() {
                return n
            })), s.d(t, "c", (function() {
                return o
            }));
            const i = "zalo_temp_new_pos_enable_new_folder_to_media",
                r = "zalo_temp_new_pos_swap_path_success",
                a = "zalo_temp_new_pos_migration_old_folder_detailed",
                n = "zalo_temp_new_pos_device_id",
                o = "zalo_temp_new_pos_log_time_stamp"
        },
        "8tev": function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return i
            })), s.d(t, "d", (function() {
                return r
            })), s.d(t, "e", (function() {
                return a
            })), s.d(t, "a", (function() {
                return n
            })), s.d(t, "c", (function() {
                return o
            }));
            const i = {
                    DONE: "DONE",
                    NOT_DONE: "NOT_DONE",
                    UNKNOWN: "UNKNOWN"
                },
                r = {
                    GET_FROM_OLD_DB: "GET_FROM_OLD_DB",
                    ADD_TO_NEW_DB: "ADD_TO_NEW_DB",
                    DELETE_FROM_OLD_DB: "DELETE_FROM_OLD_DB"
                },
                a = {
                    DONE: "DONE",
                    FAILED: "FAILED"
                },
                n = {
                    NEW: "NEW",
                    LOAD_FROM_LOCAL: "LOAD_FROM_LOCAL",
                    RUNNING: "RUNNING",
                    DONE: "DONE",
                    FAILED: "FAILED"
                },
                o = {
                    PERSISTED_JOB_DESC_SUMMARIES_KEY: "persisted_job_desc_summaries",
                    MSG_ID_CURSOR_KEY: "msg_id_cursor",
                    MIGRATION_STATE_KEY: "migration_state"
                }
        },
        "A+kQ": function(e, t, s) {
            "use strict";
            var i, r = s("jDHv"),
                a = s("6ivO"),
                n = s("Zxlm"),
                o = s("Uzj0");
            o.j.timeInMs("5s"), o.j.timeInMs("100ms");
            const d = {
                enable: !1,
                start_after: o.j.timeInMs("1m"),
                gap_time: o.j.timeInMs("500ms"),
                batch_size: 50,
                bypass_legacy_error: !1,
                enable_detail_log: !0
            };
            let l = Object(r.singleton)()(i = Reflect.metadata("design:type", Function)(i = Reflect.metadata("design:paramtypes", [])(i = class extends a.AdvancedRemoteConfigs {
                constructor() {
                    super(n.a, d, {
                        validator: new a.ObjectValidator({
                            enable: a.Rule.field().boolean(),
                            start_after: a.Rule.field().number().min(0),
                            gap_time: a.Rule.field().number().min(0),
                            batch_size: a.Rule.field().number().min(1),
                            bypass_legacy_error: a.Rule.field().boolean(),
                            enable_detail_log: a.Rule.field().boolean()
                        })
                    })
                }
                nestedKey(e) {
                    return "enable" != e && super.nestedKey(e)
                }
            }) || i) || i) || i;
            r.ModuleContainer.registerSingleton(n.b, l)
        },
        AjFa: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return l
            })), s.d(t, "b", (function() {
                return c
            }));
            var i = s("4ePI"),
                r = s("jDHv"),
                a = s("wH4e"),
                n = s("z0WU"),
                o = s("1pet"),
                d = s("XS0u");
            class l extends i.a {
                constructor(e, t, s, i, r, a) {
                    super(e, t, s, i, r, a)
                }
                async getImageMessagesForPhotoViewer(e, t, s, i, r, a) {
                    const n = await this.getMsgIdsOfImageMediaForPhotoViewer(e, s, t, i, r, a);
                    if (null == n || !n.length) return [];
                    const o = await Promise.all(n.map((e => this.getMediaMapper().toDTO(e))));
                    return d.default._fillExifBeforeShow(o), o
                }
                async getMsgIdsOfImageMediaForPhotoViewer(e, t, s, i, r, a) {
                    const d = i && r;
                    let l = !1;
                    const c = () => l,
                        m = e => {
                            if (!(a && (a.member || a.date && (a.date.start || a.date.end) || a.name || a.ext))) return !0;
                            if (!a.member || e.fromUid && e.fromUid === a.member) {
                                let s, i, r;
                                if (a.name) {
                                    if (e.message.params && e.message.href) try {
                                        let t = JSON.parse(e.message.params);
                                        s = t.mediaTitle ? n.default.simpleStripVietnamese(t.mediaTitle) : n.default.simpleStripVietnamese(e.message.title), i = t.src
                                    } catch (t) {
                                        return this.logger.zsymb(18, "Mm-93w", t), !1
                                    }
                                    r = n.default.simpleStripVietnamese(a.name)
                                }
                                if (!a.name || n.default.searchContent(s, r) || n.default.searchContent(i, r)) {
                                    let t = !1;
                                    if (a.ext && (t = o.fileExt[a.ext].some((t => e.message.title.endsWith(t)))), !a.ext || t) return !a.date.start || !a.date.end || (a.date.end < e.sendDttm ? e.sendDttm < a.date.start + 864e5 : (l = !0, !1))
                                }
                            }
                            return !1
                        };
                    let u = [];
                    i && u.push(this.getImageMediasForward(e, t, s, d, m, c)), r && u.push(this.getImageMediasBackward(e, t, s, d, m, c));
                    try {
                        const e = await Promise.all(u);
                        if (2 === e.length) {
                            let t = e[1].reverse();
                            return t.pop(), t = t.concat(e[1]), t
                        }
                        return i ? e[0] : e[0].reverse()
                    } catch (g) {
                        return this.logger.zsymb(18, "MVbdpl", g), []
                    }
                }
                async getImageMediasForward(e, t, s, i, r, n) {
                    let d = [];
                    const {
                        cliMsgId: l,
                        sendDttm: c,
                        msgId: m
                    } = t;
                    if (!(await this._isMigrationDone())) {
                        const t = {
                                from: [e, m.length, m],
                                to: [e, o.MessageConstants.MAX_MSG_ID.length, o.MessageConstants.MAX_MSG_ID],
                                excludeFrom: !i,
                                excludeTo: !i
                            },
                            l = {
                                limit: s,
                                index: "userId_sendDttm_msgId",
                                direction: a.c.NEXT,
                                predicate: e => r({
                                    fromUid: e.fromUid,
                                    message: e.message,
                                    sendDttm: "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm
                                }),
                                aborted: n,
                                useLGKeyRange: !0
                            };
                        d = await this.getAllInOldDB(t, l)
                    }
                    if (d.length < s) {
                        const t = {
                                from: [e, c, l],
                                to: [e, parseInt(o.MessageConstants.MAX_SENDDTTM), o.MessageConstants.MAX_MSG_ID],
                                excludeFrom: !i,
                                excludeTo: !i
                            },
                            m = {
                                limit: s - d.length,
                                index: "convId_sendDttm_cliMsgId",
                                direction: a.c.NEXT,
                                predicate: e => r({
                                    fromUid: e.fromUid,
                                    message: e.content,
                                    sendDttm: e.sendDttm
                                }),
                                aborted: n,
                                useLGKeyRange: !0
                            };
                        d.push(...await this.getAll(t, m))
                    }
                    return d
                }
                async getImageMediasBackward(e, t, s, i, r, n) {
                    let o = [];
                    const {
                        cliMsgId: d,
                        sendDttm: l,
                        msgId: c
                    } = t, m = {
                        from: [e, 0, ""],
                        to: [e, l, d],
                        excludeFrom: !i,
                        excludeTo: !i
                    }, u = {
                        limit: s,
                        index: "convId_sendDttm_cliMsgId",
                        direction: a.c.PREV,
                        predicate: e => r({
                            fromUid: e.fromUid,
                            message: e.content,
                            sendDttm: e.sendDttm
                        }),
                        aborted: n,
                        useLGKeyRange: !0
                    };
                    if (o = await this.getAll(m, u), o.length < s && !(await this._isMigrationDone())) {
                        const t = {
                                from: [e, 0, ""],
                                to: [e, c.length, c],
                                excludeFrom: !i,
                                excludeTo: !i
                            },
                            d = {
                                limit: o ? s - o.length : s,
                                index: "userId_sendDttm_msgId",
                                direction: a.c.PREV,
                                predicate: e => r({
                                    fromUid: e.fromUid,
                                    message: e.message,
                                    sendDttm: "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm
                                }),
                                aborted: n,
                                useLGKeyRange: !0
                            };
                        o.push(...await this.getAllInOldDB(t, d))
                    }
                    return o
                }
            }
            const c = Object(r.define)("image-media-repository")
        },
        Ceyj: function(e, t, s) {
            "use strict";
            var i = s("VTBJ"),
                r = s("fsN4");

            function a(e, t) {
                return e.reduce(((e, s, i, r) => (e[t(s, i, r) ? 0 : 1].push(s), e)), [
                    [],
                    []
                ])
            }
            var n, o = s("D8f9"),
                d = s("1pet"),
                l = s("jDHv"),
                c = s("fqRP"),
                m = s("Mgpg");
            const u = d.MessageConstants.MAX_MSG_ID;
            Object(l.singleton)(c.a)(n = Reflect.metadata("design:type", Function)(n = Reflect.metadata("design:paramtypes", [])(n = class {
                constructor() {
                    this._logger = void 0, this.dispose = () => {}, this.correctTTLItemEntity = e => {
                        function t(e, t) {
                            return null == e ? t : e
                        }
                        const s = Object(i.a)({}, e);
                        return s.beginTime = t(s.beginTime, 0), s.cliMsgId = t(s.cliMsgId, ""), s.expireOn = t(s.expireOn, Date.now()), s.fromUid = t(s.fromUid, ""), s.mediaType = t(s.mediaType, ""), s.msgId = t(s.msgId, ""), s.sendDttm = t(s.sendDttm, "") + "", s.serverTime = t(s.serverTime, "") + "", s.toUid = t(s.toUid, ""), s.ttl = t(s.ttl, 0), s.ttlType = t(s.ttlType, ""), s
                    }, this._safePut = async e => {
                        const t = [],
                            s = [],
                            i = r.default.getInstance();
                        for (const r of e) try {
                            const e = this.correctTTLItemEntity(r);
                            await i.MsgInfo.TTLItem.insert(e, {
                                replace: !0
                            }), t.push(r)
                        } catch (a) {
                            s.push([r, a])
                        }
                        return [t, s]
                    }, this.putMsgs = async e => {
                        let t = e.map((e => p(e)));
                        const [s, i] = a(t, (e => e.ok)), r = s.map((e => e.value));
                        let n = [],
                            o = [];
                        [o, n] = await this._safePut(r);
                        const d = i.map((e => e.error));
                        return n.lastItem || d.length ? {
                            ok: !1,
                            error: {
                                invalidItems: d,
                                errorItems: n
                            }
                        } : {
                            ok: !0,
                            value: o
                        }
                    }, this.deletes = e => r.default.getInstance().MsgInfo.TTLItem.deleteMulti(e), this.getExpireItemsBefore = async (e, t) => {
                        const s = t ? {
                                from: [t.expireOn, t.toUid, t.msgId, t.ttlType],
                                to: [e, d.MessageConstants.MAX_CONVERSATION_ID, u, o.b],
                                excludeFrom: !1,
                                excludeTo: !0
                            } : {
                                to: [e, d.MessageConstants.MAX_CONVERSATION_ID, u, o.b],
                                excludeTo: !1
                            },
                            i = {
                                limit: 50,
                                index: "expireOn_toUid_pk",
                                useLGKeyRange: !0
                            },
                            a = r.default.getInstance();
                        return await a.MsgInfo.TTLItem.getAll(s, i)
                    }, this.getYoungestExpiredItem = async () => {
                        const e = {
                                to: [Number.MAX_VALUE, d.MessageConstants.MAX_CONVERSATION_ID, u, o.b],
                                excludeTo: !0
                            },
                            t = r.default.getInstance();
                        return (await t.MsgInfo.TTLItem.getAll(e, {
                            limit: 1,
                            index: "expireOn_toUid_pk",
                            useLGKeyRange: !0
                        }))[0]
                    }, this.getMappedMsgsByConvIdFromTTLItems = async (e, t) => {
                        let s;
                        return t && (s = await t.getMappedMessagesByConvIdAsync(e)), s
                    }, this._logger = l.ModuleContainer.resolve(m.ZLoggerFactory).createZLogger("utils", ["ttl", "destructor", "ttl"])
                }
            }) || n) || n);
            const g = e => "number" == typeof e ? String(e) : e,
                h = [o.a.Message, o.a.Quote],
                p = (e = {}) => {
                    const t = Object(i.a)({}, e);
                    return t.cliMsgId = g(t.cliMsgId), t.cliMsgId ? (t.fromUid = g(t.fromUid), t.fromUid ? (t.toUid = g(t.toUid), t.toUid ? (t.msgId = g(t.msgId), s = t.ttlType, h.includes(s) ? (t.expireOn = +t.expireOn, "number" != typeof t.expireOn ? {
                        ok: !1,
                        error: e
                    } : {
                        ok: !0,
                        value: t
                    }) : {
                        ok: !1,
                        error: e
                    }) : {
                        ok: !1,
                        error: e
                    }) : {
                        ok: !1,
                        error: e
                    }) : {
                        ok: !1,
                        error: e
                    };
                    var s
                }
        },
        ClHk: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return d
            }));
            var i, r = s("VTBJ"),
                a = s("jDHv"),
                n = s("hI9i"),
                o = s("rCQs");
            const d = Object(a.define)("shared-tasks-monitor");
            Object(o.b)(d)(i = class {
                constructor() {
                    this.tasks = new Map, this.taskIds = [], this.name = "shared-tasks-monitor", this.key = "id"
                }
                insertTask(e, t) {
                    this.taskIds.length > 20 && (this.taskIds = this.taskIds.slice(1)), this.taskIds = [...this.taskIds, e], this.tasks.set(e, {
                        id: e,
                        type: t,
                        startTime: Date.now(),
                        endTime: 0,
                        progress: 0,
                        status: "running"
                    }), Object(n.i)(this.name, "all")
                }
                updateTaskProgress(e, t) {
                    const s = this.tasks.get(e);
                    s && (this.tasks.set(e, Object(r.a)(Object(r.a)({}, s), {}, {
                        progress: t
                    })), Object(n.h)(this.name, e))
                }
                updateTaskStatus(e, t) {
                    const s = this.tasks.get(e);
                    s && (this.tasks.set(e, Object(r.a)(Object(r.a)({}, s), {}, {
                        status: t
                    })), Object(n.h)(this.name, e))
                }
                init() {}
                getItem(e, t) {
                    return this.tasks.get(e.key)
                }
                getList() {
                    return this.taskIds
                }
                onGetItemFailure() {}
                onGetListFailure() {}
            })
        },
        Hbak: function(e, t, s) {
            "use strict";
            var i, r = s("jDHv"),
                a = s("8/YW"),
                n = s("dOPt"),
                o = s("ClHk"),
                d = s("Mgpg"),
                l = s("Ydol"),
                c = s("Q7+W");
            Object(a.j)()(i = Object(r.singleton)(n.a)(i = Object(r.injectable)()(i = function(e, t) {
                return Object(r.inject)(d.ZLoggerFactory)(e, void 0, 0)
            }(i = Reflect.metadata("design:type", Function)(i = Reflect.metadata("design:paramtypes", [void 0 === d.ZLoggerFactory ? Object : d.ZLoggerFactory])(i = class {
                constructor(e) {
                    this.handlers = new Map, this.runningMap = new Map, this.logger = void 0, this.monitor = void 0, this.logger = e.createZLogger("core", ["shared-worker-runner"]), this.monitor = r.ModuleContainer.resolveIfExist(o.a)
                }
                onStart() {
                    this.logger.zsymb(3, "iBC5RJ", ["start", "p6UnuI"]), $zsharedWorker.start(), $zsub.$zsharedWorker.onRequestRunTask((async (e, t) => {
                        var s;
                        const i = this.handlers.get(t.type);
                        if (!i) return;
                        null === (s = this.monitor) || void 0 === s || s.insertTask(t.id, t.type), this.logger.zsymb(3, "4xQd0w", ["{} id: {}", "vIvErH"], t.type, t.id);
                        const r = new AbortController,
                            a = t.id,
                            n = {
                                abort: r,
                                updateParams: () => {}
                            };
                        this.runningMap.set(a, n), this.logger.zsymb(3, "9xGumb", ["{} id: {} start", "ZnGFjH"], t.type, t.id);
                        try {
                            const e = (e, s) => {
                                var i;
                                $zsharedWorker.updateProgress({
                                    id: a,
                                    progress: e,
                                    checkpoint: s
                                }), null === (i = this.monitor) || void 0 === i || i.updateTaskProgress(t.id, e)
                            };
                            await i.execute({
                                abort: r.signal,
                                params: t.params,
                                checkpoint: t.checkpoint,
                                onParamsChange: e => {
                                    n.updateParams = e
                                },
                                report: e
                            }).then((e => {
                                var s;
                                r.signal.aborted || (null === (s = this.monitor) || void 0 === s || s.updateTaskStatus(t.id, "finished"), $zsharedWorker.responseResult({
                                    id: a,
                                    result: e
                                }), this.logger.zsymb(3, "WGZMts", ["{} id: {} finished", "yXVQTl"], t.type, t.id))
                            })).catch((e => {
                                var s;
                                if (r.signal.aborted) return;
                                null === (s = this.monitor) || void 0 === s || s.updateTaskStatus(t.id, "error");
                                const i = "number" == typeof e.error ? e.error : -999;
                                $zsharedWorker.responseResult({
                                    id: a,
                                    error: {
                                        message: e.message,
                                        error_code: i
                                    }
                                }), this.logger.zsymb(0, "vnxiI9", (() => [`task ${t.type} id: ${t.id} failed`, {
                                    error: e
                                }]))
                            }))
                        } catch (d) {
                            var o;
                            null === (o = this.monitor) || void 0 === o || o.updateTaskStatus(t.id, "error"), $zsharedWorker.responseResult({
                                id: a,
                                error: {
                                    message: d.message
                                }
                            }), this.logger.zsymb(0, "33DAKL", (() => [`task ${t.type} id: ${t.id} failed`, {
                                error: d
                            }]))
                        }
                        this.runningMap.delete(a)
                    })), $zsub.$zsharedWorker.onRequestAbortTask(((e, t) => {
                        const s = this.runningMap.get(t.id);
                        var i;
                        s && (null === (i = this.monitor) || void 0 === i || i.updateTaskStatus(t.id, "error"), s.abort.abort());
                        this.logger.zsymb(3, "Rp-OKi", ["abort task {} id: {}", "q-JI4I"], t.type, t.id)
                    })), $zsub.$zsharedWorker.onRequestUpdateTaskParams(((e, t) => {
                        const s = this.runningMap.get(t.id);
                        if (s) return s.updateParams(t.params), void this.logger.zsymb(3, "yoV3gV", ["{} id: {} - updated params", "ddx2j3"], t.type, t.id);
                        this.logger.zsymb(18, "6XqXdC", `${t.type} id: ${t.id} - update params failed. task not found`)
                    }));
                    const e = () => {
                        $zFileManager.cleanupLogFileDescriptors(), $zsub.$zsharedWorker.removeListenUnloadZLog(e)
                    };
                    $zsub.$zsharedWorker.onUnloadZLog(e), "shared-worker" === __ZaBUNDLENAME__ && this.startCheckAlive(), this.syncRemoteConfigToSharedWorker()
                }
                registerHandler(e, t) {
                    this.logger.zsymb(3, "BN6yWL", ["register handler: {}", "KzoiHA"], e), this.handlers.set(e, t), $zsharedWorker.register(e)
                }
                startCheckAlive() {
                    this.logger.zsymb(3, "wqB8ds", ["start check alive", "aYRRkn"]), setInterval((() => {
                        this.logger.zsymb(3, "HITIPj", ["sending ping", "T-qTLr"]), $zsharedWorker.ping()
                    }), 6e4)
                }
                syncRemoteConfigToSharedWorker() {
                    try {
                        if ("shared-worker" === __ZaBUNDLENAME__) return;
                        l.default.on("server-info-config", (e => {
                            $zsharedWorker.updateRemoteConfig({
                                config: e
                            })
                        })), l.default.on("authen-info-config", (e => {
                            $zsharedWorker.updateRemoteConfig({
                                config: e
                            })
                        })), $zsharedWorker.updateRemoteConfig({
                            config: r.ModuleContainer.resolve(c.a).getAllCache()
                        })
                    } catch (e) {
                        this.logger.zsymb(21, "ebIYDM", ["syncRemoteConfigToSharedWorker failed", "4MwTtE"], e)
                    }
                }
            }) || i) || i) || i) || i) || i)
        },
        Hy6w: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("VTBJ");
            class r {
                constructor(e) {
                    this.label = e, this.data = new Map, this.id = 0, this.locked = !1
                }
                get nextId() {
                    return ++this.id
                }
                get info() {
                    return `${this.label}: ${this.time}`
                }
                get time() {
                    let e = 0;
                    return this.data.forEach((t => e += t.time)), e
                }
                lock() {
                    this.locked = !0
                }
                unlock() {
                    this.locked = !1
                }
                getTracker() {
                    const e = this.nextId;
                    return {
                        start: () => this.start(e),
                        end: () => this.end(e),
                        do: (t, ...s) => {
                            this.start(e);
                            const i = t(...s);
                            return this.end(e), i
                        },
                        doAsync: (t, ...s) => (this.start(e), t(...s).then((t => (this.end(e), t)))),
                        reset: () => {
                            this.data.delete(e)
                        }
                    }
                }
                start(e) {
                    if (this.locked) return;
                    const t = this.data.get(e);
                    if (t) {
                        let s = t.start;
                        t.counting <= 0 && (s = Date.now()), this.data.set(e, Object(i.a)(Object(i.a)({}, t), {}, {
                            counting: t.counting + 1,
                            start: s
                        }))
                    } else this.data.set(e, {
                        counting: 1,
                        time: 0,
                        start: Date.now()
                    })
                }
                end(e) {
                    if (this.locked) return;
                    const t = this.data.get(e);
                    if (t) {
                        let s = t.start,
                            r = t.time + Date.now() - t.start;
                        t.counting && t.counting > 1 && (s = Date.now()), this.data.set(e, Object(i.a)(Object(i.a)({}, t), {}, {
                            time: r,
                            start: s,
                            counting: t.counting - 1
                        }))
                    }
                }
            }
        },
        "J/8C": function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("link-media-mapper")
        },
        K4Ns: function(e, t, s) {
            "use strict";
            s.d(t, "c", (function() {
                return i
            })), s.d(t, "b", (function() {
                return r
            })), s.d(t, "a", (function() {
                return a
            }));
            let i = function(e) {
                    return e[e.OLD = 0] = "OLD", e[e.NEW = 1] = "NEW", e[e.UNKNOWN = 2] = "UNKNOWN", e
                }({}),
                r = function(e) {
                    return e[e.OLD = 0] = "OLD", e[e.NEW = 1] = "NEW", e[e.UNKNOWN = 2] = "UNKNOWN", e
                }({}),
                a = function(e) {
                    return e[e.OLD = 0] = "OLD", e[e.NEW = 1] = "NEW", e[e.UNKNOWN = 2] = "UNKNOWN", e
                }({})
        },
        KPNQ: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("media-primary-key-convertor")
        },
        KTzm: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("message-scan-broadcaster")
        },
        KszJ: function(e, t, s) {
            "use strict";
            var i, r = s("jDHv"),
                a = s("8/YW"),
                n = s("Xy/4"),
                o = s("Mgpg"),
                d = s("/n5c");
            Object(a.j)()(i = Object(a.h)()(i = Object(r.injectable)()(i = Object(r.singleton)(n.a)(i = function(e, t) {
                return Object(r.inject)(o.ZLoggerFactory)(e, void 0, 0)
            }(i = Reflect.metadata("design:type", Function)(i = Reflect.metadata("design:paramtypes", [void 0 === o.ZLoggerFactory ? Object : o.ZLoggerFactory])(i = class {
                constructor(e) {
                    this.idCounter = 0, this.runningTasks = new Map, this.logger = void 0, this.logger = e.createZLogger("shared-worker", ["client"])
                }
                onStart() {
                    $zsub.$zsharedWorker.onReceiveUpdateProgress(((e, t) => {
                        const s = this.runningTasks.get(t.id);
                        s ? s.notifyProgressChange(t.progress, t.checkpoint) : $zsharedWorker.abortTask({
                            id: t.id,
                            type: t.type
                        })
                    })), $zsub.$zsharedWorker.onReceiveResult(((e, t) => {
                        const s = this.runningTasks.get(t.id);
                        s ? (this.logger.zsymb(3, "cY2sJs", ["resolve:{} id: {} success:{}", "BZeHyB"], s.type, t.id, !t.error), t.error ? s.notifyError(t.error) : s.notifyResult(t.result, t.checkpoint), this.runningTasks.delete(t.id)) : this.logger.zsymb(18, "VxqG8A", (() => ["not found resolver for task", {
                            id: t.id
                        }]))
                    }));
                    const e = async () => {
                        await r.ModuleContainer.resolve(d.a).tryFlushAll(), await $zFileManager.cleanupLogFileDescriptors(), $zsub.$zwindow.removeListenUnloadZLog(e)
                    };
                    $zsub.$zwindow.onUnloadZLog(e)
                }
                onDispose() {
                    $zsharedWorker.dispose()
                }
                run(e) {
                    const t = (this.idCounter++).toString();
                    e.id = t, this.runningTasks.set(t, e), this.logger.zsymb(3, "jDyeiD", ["run:{} id: {}", "KuV9t2"], e.type, t), $zsharedWorker.runTask(e.toDescriptor())
                }
                abort(e) {
                    const t = this.runningTasks.get(e);
                    t && (this.runningTasks.delete(e), $zsharedWorker.abortTask({
                        id: e,
                        type: t.type
                    }))
                }
                notifyChangeParams(e, t) {
                    const s = this.runningTasks.get(e);
                    if (!s) throw new Error(`not found task with id: ${e}`);
                    const i = {
                        id: e,
                        type: s.type,
                        params: t
                    };
                    $zsharedWorker.updateTaskParams(i)
                }
            }) || i) || i) || i) || i) || i) || i)
        },
        Le17: function(e, t, s) {
            "use strict";
            var i = s("MNHy"),
                r = s("jDHv"),
                a = s("KTzm"),
                n = s("Oy95"),
                o = s("kAC1");
            class d {
                constructor() {
                    this.logger = void 0, this._listeners = {}, this.addEventListener = (e, t, s = 0) => {
                        var i;
                        const r = null !== (i = this._listeners[e]) && void 0 !== i ? i : [];
                        return r.push({
                            listener: t,
                            priority: s
                        }), r.sort(((e, t) => t.priority - e.priority)), this._listeners[e] = r, () => this.removeEventListener(e, t)
                    }, this.removeEventListener = (e, t) => {
                        var s;
                        this._listeners[e] = null !== (s = this._listeners[e]) && void 0 !== s ? s : [];
                        const i = this._listeners[e].findIndex((e => e.listener === t)); - 1 !== i && this._listeners[e].splice(i, 1)
                    }, this.dispatchEvent = async (e, t) => {
                        var s;
                        const i = null !== (s = this._listeners[e]) && void 0 !== s ? s : [];
                        if (i.length)
                            for (const {
                                    listener: r
                                }
                                of i) await Object(o.a)((() => r(t)), 6e4).catch((e => {
                                this.logger.zsymb(21, "COAZ5y", ["Error while executing listener for event: {}", "9Xx4W7"], e)
                            }))
                    }, this.logger = Object(n.a)("priority-event-emitter")
                }
                listenerCount(e) {
                    var t, s;
                    return null !== (t = null === (s = this._listeners[e]) || void 0 === s ? void 0 : s.length) && void 0 !== t ? t : 0
                }
                removeAllListeners() {
                    this._listeners = {}
                }
            }
            var l, c = s("aY5P"),
                m = s("igA5"),
                u = s("XmYa");
            Object(r.singleton)(a.a)(l = Reflect.metadata("design:type", Function)(l = Reflect.metadata("design:paramtypes", [])(l = class {
                constructor() {
                    this.emitter = new d, this.clients = new Map, this.initCountListeners()
                }
                countListeners() {
                    return this.clients.size
                }
                subscribe(e, t) {
                    if (this.clients.has(e)) throw new Error("Client already subscribed");
                    t.onStarted && this.listenStarted(e, t.onStarted), this.listenProgress(e, t.onProgress), this.listenConvSuccess(e, t.onConvSuccess), this.listenError(e, t.onError), this.listenComplete(e, t.onComplete), this.clients.set(e, t), this.setCountListeners(this.clients.size)
                }
                unsubscribe(e) {
                    const t = this.clients.get(e);
                    t && (this.emitter.removeEventListener(i.a.OnProgress, t.onProgress), this.emitter.removeEventListener(i.a.ConvSuccess, t.onConvSuccess), this.emitter.removeEventListener(i.a.Error, t.onError), this.emitter.removeEventListener(i.a.Complete, t.onComplete), this.clients.delete(e), this.setCountListeners(this.clients.size))
                }
                listenStarted(e, t) {
                    return this.emitter.addEventListener(i.a.Started, t, c.b[e])
                }
                listenProgress(e, t) {
                    return this.emitter.addEventListener(i.a.OnProgress, t, c.b[e])
                }
                listenConvSuccess(e, t) {
                    return this.emitter.addEventListener(i.a.ConvSuccess, t, c.b[e])
                }
                listenComplete(e, t) {
                    return this.emitter.addEventListener(i.a.Complete, t, c.b[e])
                }
                listenError(e, t) {
                    return this.emitter.addEventListener(i.a.Error, t, c.b[e])
                }
                async broadcastStarted(e) {
                    await this.emitter.dispatchEvent(i.a.Started, e)
                }
                async broadcastMessages(e) {
                    await this.emitter.dispatchEvent(i.a.OnProgress, e)
                }
                async completeBroadcastConv(e) {
                    await this.emitter.dispatchEvent(i.a.ConvSuccess, e)
                }
                async broadcastError(e) {
                    await this.emitter.dispatchEvent(i.a.Error, e)
                }
                async completeBroadcast(e) {
                    await this.emitter.dispatchEvent(i.a.Complete, e)
                }
                setCountListeners(e) {
                    m.default.getInstance().setItemForCurrentUser(u.a, e.toString())
                }
                async initCountListeners() {
                    const e = m.default.getInstance();
                    await e.waitForReady(), this.setCountListeners(this.clients.size)
                }
            }) || l) || l)
        },
        MLPV: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("image-media-mapper")
        },
        MNHy: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            }));
            let i = function(e) {
                return e.Started = "Started", e.OnProgress = "OnProgress", e.ConvSuccess = "ConvSuccess", e.Error = "Error", e.Complete = "Complete", e
            }({})
        },
        Oy95: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return a
            }));
            var i = s("jDHv"),
                r = s("Mgpg");
            const a = e => {
                return i.ModuleContainer.resolve(r.ZLoggerFactory).createZLogger(["message-db-scanner"], [e])
            }
        },
        "Q/Ir": function(e, t, s) {
            "use strict";
            var i = s("jDHv"),
                r = s("8/YW"),
                a = s("Mgpg"),
                n = s("+eUS"),
                o = s("wH4e"),
                d = s("YrRS");
            const l = "render" !== __ZaBUNDLENAME__ && "WEB" !== __ZaBUNDLENAME__ && "shared-worker" !== __ZaBUNDLENAME__ && "main" !== __ZaBUNDLENAME__;
            setTimeout((async function() {
                if (l) return;
                const e = i.ModuleContainer.resolve(a.ZLoggerFactory).createZLogger("bootstrap", ["shared"]);
                e.zsymb(3, "oUNHRY", ["running application bootstrap", "dbOBtm"]);
                let t = (() => {
                    switch (__ZaBUNDLENAME__) {
                        case "WEB":
                            return o.i.Browser;
                        case "render":
                            return o.i.Host;
                        case "shared-worker":
                            return o.i.Client;
                        case "main":
                            return o.i.Background;
                        default:
                            return o.i.Unknown
                    }
                })();
                try {
                    const s = i.ModuleContainer.resolve(r.a);
                    await s.start(), Object(n.a)(t), t === o.i.Browser && Object(d.b)(), "shared-worker" === __ZaBUNDLENAME__ && await s.init(), e.zsymb(3, "TFr5xv", ["application bootstrap success", "m3PIaI"])
                } catch (s) {
                    e.zsymb(0, "vE51HJ", (() => ["application bootstrap fail", {
                        reason: s
                    }]))
                }
            }), 0)
        },
        RgQf: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("file-media-mapper")
        },
        SrDO: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("media-mapper-factory")
        },
        Tlqd: function(e, t, s) {
            "use strict";
            s.d(t, "c", (function() {
                return i
            })), s.d(t, "a", (function() {
                return r
            })), s.d(t, "b", (function() {
                return a
            }));
            let i = function(e) {
                return e[e.IDLE = 0] = "IDLE", e[e.SCAN_MESSAGES_RESTRUCTURE_MEDIA = 1] = "SCAN_MESSAGES_RESTRUCTURE_MEDIA", e[e.TAKE_SNAPSHOT = 2] = "TAKE_SNAPSHOT", e[e.SCAN_MESSAGES = 3] = "SCAN_MESSAGES", e[e.DELETE_RESOURCES = 4] = "DELETE_RESOURCES", e
            }({});
            const r = {
                    STAGE: "rrc_stage",
                    PROGRESS: "rrc_progress",
                    PROGRESS_LIMIT: "rrc_progress_lim",
                    LAST_STATUS: "rrc_last_st",
                    FORCE_RUN: "rrc_force_run",
                    TRACKING_DATA: "rrc_tracking_data"
                },
                a = "resource-config"
        },
        W13p: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return n
            })), s.d(t, "b", (function() {
                return o
            }));
            var i = s("4ePI"),
                r = s("jDHv"),
                a = s("wH4e");
            class n extends i.a {
                constructor(e, t, s, i, r, a) {
                    super(e, t, s, i, r, a)
                }
                async getLastestAddedFiles(e, t) {
                    if (!e || "number" != typeof t) throw Error(`[getLastestAddedFiles]: ${JSON.stringify(e)} or ${t} isn't valid!`);
                    const s = await this.getLastestAddedFilesInNewDB(e, t);
                    return s.length < t && !(await this._isMigrationDone()) && s.push(...await this.getLastestAddedFilesInOldDB({
                        sendDttm: e.sendDttm
                    }, t - s.length)), s
                }
                async getLastestAddedFilesInNewDB(e, t) {
                    const {
                        convId: s,
                        sendDttm: i,
                        cliMsgId: r
                    } = e;
                    return this.dbTable.getAll({
                        from: ["", 0, 0],
                        to: [s, i, r],
                        excludeFrom: !0,
                        excludeTo: !0
                    }, {
                        index: "convId_sendDttm_cliMsgId",
                        limit: t,
                        direction: a.c.PREV
                    })
                }
                async getLastestAddedFilesInOldDB(e, t) {
                    if (await this._isMigrationDone()) return [];
                    const s = await this.getOldDBTable().getAll({
                        from: 0,
                        to: e.sendDttm,
                        excludeFrom: !0,
                        excludeTo: !0
                    }, {
                        index: "sendDttm",
                        limit: t,
                        direction: a.c.PREV
                    });
                    return s.length > 0 ? s.map((e => this.getMediaMapper().toDomainFromOldDomain(e))) : []
                }
            }
            const o = Object(r.define)("file-media-repository")
        },
        YgrE: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("zinstant-module-loader")
        },
        aY5P: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            })), s.d(t, "b", (function() {
                return r
            }));
            let i = function(e) {
                return e.dummy = "dummy", e.dummy2 = "dummy2", e.cloud = "cloud", e.redundantResource = "redundantResource", e.migratePath = "migratePath", e
            }({});
            const r = {
                [i.dummy]: 0,
                [i.dummy2]: 1,
                [i.cloud]: 2,
                [i.redundantResource]: 3,
                [i.migratePath]: 4
            }
        },
        fhp4: function(e, t, s) {
            "use strict";
            s.d(t, "c", (function() {
                return i.b
            })), s.d(t, "a", (function() {
                return r.a
            })), s.d(t, "d", (function() {
                return a.b
            })), s.d(t, "b", (function() {
                return n.IMediaMapperFactory
            })), s.d(t, "e", (function() {
                return n.b
            }));
            var i = s("MLPV"),
                r = s("RgQf"),
                a = s("J/8C"),
                n = s("SrDO")
        },
        gtFi: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return A
            }));
            var i = s("VTBJ"),
                r = s("Y65e"),
                a = s("jDHv"),
                n = s("XmYa"),
                o = s("Oy95"),
                d = s("kcif"),
                l = s.n(d),
                c = s("fsN4"),
                m = s("1pet"),
                u = s("h1h5"),
                g = s("kAC1");
            s("JHeE");
            class h extends u.a {
                constructor(e) {
                    super(), this.logger = void 0, this.convId = void 0, this.logger = Object(o.a)("scanner"), this.convId = e
                }
                async run(e) {
                    try {
                        var t;
                        const s = (null === (t = e.getScannerRange) || void 0 === t ? void 0 : t.call(e, this.convId)) || [0, Date.now()];
                        await this.scanData(s, e)
                    } catch (s) {
                        throw this.logger.zsymb(21, "dh8dNA", ["Error while scanning resource db: {}", "RrNyDo"], s), s
                    }
                }
                async recursiveScanData(e, t) {
                    var s;
                    const i = c.default.getInstance(),
                        [r, a] = e,
                        o = await i.Core.Message.getAll({
                            from: [this.convId, String(r), m.MessageConstants.MIN_MSG_ID],
                            to: [this.convId, String(a), m.MessageConstants.MAX_MSG_ID],
                            excludeTo: !1,
                            excludeFrom: !0
                        }, {
                            limit: t.getScannerConfig().batchSize,
                            partition: this.convId,
                            index: "userId_sendDttm_msgId",
                            filter: {
                                OR: n.d.map((e => ({
                                    msgType: e
                                })))
                            }
                        });
                    if (null !== (s = t.abort) && void 0 !== s && s.aborted) return this.logger.zsymb(6, "JxCeTx", "abort signal received in recursiveScanData", this.convId), {
                        success: !1
                    };
                    if (o.length > 0) {
                        var d;
                        const e = o.filter((e => Object(g.c)(e))),
                            s = Number(e[0].sendDttm),
                            i = Number(e[e.length - 1].sendDttm);
                        return await this.dispatchEvent("progress", {
                            data: e,
                            convId: this.convId,
                            metadata: {
                                batchSize: t.getScannerConfig().batchSize,
                                range: [s, i]
                            }
                        }), await (l = t.getScannerConfig().batchDelay, new Promise((e => setTimeout(e, l)))), null !== (d = t.abort) && void 0 !== d && d.aborted ? (this.logger.zsymb(6, "6j_AEX", "abort signal received in recursiveScanData", this.convId), {
                            success: !1
                        }) : await this.recursiveScanData([i, a], t)
                    }
                    var l;
                    return {
                        success: !0
                    }
                }
                async scanData(e, t) {
                    try {
                        var s;
                        if (!(await this.recursiveScanData(e, t)).success || null !== (s = t.abort) && void 0 !== s && s.aborted) return void this.logger.zsymb(0, "Qaizd7", "abort signal received in scanData", this.convId);
                        await this.dispatchEvent("conv-success", {
                            convId: this.convId,
                            metadata: {
                                range: e
                            }
                        })
                    } catch (i) {
                        throw await this.dispatchEvent("error", {
                            convId: this.convId,
                            message: i.message
                        }), i
                    }
                }
            }
            const p = e => new Promise((t => setTimeout(t, e)));
            class f extends u.a {
                constructor() {
                    super(), this.logger = void 0, this.scanners = void 0, this.runningScanners = void 0, this.totalScanners = void 0, this.completedScanners = void 0, this.onProgress = async e => {
                        e.data.length > 0 && await this.dispatchEvent("progress", e)
                    }, this.onSuccess = async e => {
                        await this.dispatchEvent("conv-success", {
                            convId: e.convId,
                            metadata: e.metadata
                        }), this.completedScanners.add(e.convId), this.isAllScannerDone() && await this.dispatchEvent("completed", {})
                    }, this.onError = async e => {
                        this.logger.zsymb(18, "G4J_OA", "got error:", e), await this.dispatchEvent("error", e)
                    }, this.logger = Object(o.a)("message-db-scanner-manager"), this.scanners = new Map, this.runningScanners = new Set, this.totalScanners = new Set, this.completedScanners = new Set
                }
                getScanner(e) {
                    let t = this.scanners.get(e);
                    return t || (t = new h(e), this.scanners.set(e, t)), t
                }
                isAllScannerDone() {
                    return this.completedScanners.size === this.totalScanners.size
                }
                async executeChunk(e, t, s) {
                    var i;
                    if (null !== (i = e.abort) && void 0 !== i && i.aborted) return void this.logger.zsymb(6, "HmwBfo", "abort signal received in executeChunk, aborting");
                    if (s < 0 || s >= t.length) return;
                    const r = t[s];
                    try {
                        var a;
                        if (await Promise.all(r.map((async t => {
                                for (; this.runningScanners.has(t);) this.logger.zsymb(0, "S4DA4r", "conv is running", t), await p(10);
                                this.runningScanners.add(t);
                                try {
                                    const s = this.getScanner(t);
                                    s.addEventListener("progress", this.onProgress), s.addEventListener("conv-success", (async e => {
                                        await this.onSuccess(e), s.removeAllListeners()
                                    })), s.addEventListener("error", this.onError), await s.run({
                                        getScannerConfig: e.getScannerConfig,
                                        abort: e.abort,
                                        getScannerRange: e.getScannerRange
                                    })
                                } finally {
                                    this.runningScanners.delete(t)
                                }
                            }))), null !== (a = e.abort) && void 0 !== a && a.aborted) return void this.logger.zsymb(0, "bWt1Qz", "abort signal received between chunks, aborting");
                        await this.executeChunk(e, t, s + 1)
                    } catch (n) {
                        throw this.logger.zsymb(18, "UWcdXF", "execute fail at chunk", s, r, n), n
                    }
                }
                async run(e) {
                    const {
                        getScannerConfig: t
                    } = e, s = e.convIds;
                    this.logger.zsymb(0, "tlWY8U", "total conversation: ", s.length), s.forEach((e => this.totalScanners.add(e)));
                    const i = l()(s, t().batchConv);
                    await this.executeChunk(e, i, 0), this.logger.zsymb(0, "uCLAyS", "all done")
                }
                dispose() {
                    this.logger.zsymb(0, "H5ytID", "dispose scanner manager"), this.removeAllListeners(), this.scanners.forEach((e => {
                        e.removeAllListeners()
                    })), this.scanners.clear(), this.totalScanners.clear(), this.completedScanners.clear()
                }
            }
            var M = s("KTzm"),
                _ = s("4b23"),
                b = s("fc/q");
            const y = {
                    CMD: 152750
                },
                I = {
                    CMD: 152751
                },
                v = {
                    CMD: 152752
                },
                O = {
                    CMD: 152753
                },
                D = {
                    CMD: 152754
                },
                T = 36e5,
                w = 24 * T,
                E = [3 * T, 12 * T, 1 * w, 3 * w, 5 * w];
            class S {
                static getInstance() {
                    return this.instance || (this.instance = new S), this.instance
                }
                increaseFailed(e) {
                    a.ModuleContainer.resolve(b.b).log({
                        success: !1,
                        commandId: e.cmd,
                        subCommandId: e.subCmd,
                        duration: e.duration,
                        errorCode: e.code,
                        startTime: 0,
                        params: []
                    })
                }
                getTierCode(e, t) {
                    for (let s = 0; s < t.length; s++)
                        if (e < t[s]) return s;
                    return t.length
                }
                markScannerSuccess(e, t, s) {
                    const i = this.getTierCode(e, E);
                    this.increaseFailed({
                        cmd: y.CMD,
                        subCmd: 0,
                        duration: e,
                        code: i
                    });
                    const r = this.getTierCode(t, [1e5, 3e5, 5e5, 1e6, 2e6]);
                    this.increaseFailed({
                        cmd: D.CMD,
                        subCmd: 0,
                        duration: 0,
                        code: r
                    });
                    const a = this.getTierCode(s, [1e3, 5e3, 1e4, 2e4]);
                    this.increaseFailed({
                        cmd: O.CMD,
                        subCmd: 0,
                        duration: 0,
                        code: a
                    })
                }
                markScannerError(e) {
                    const t = this.getTierCode(e, E);
                    this.increaseFailed({
                        cmd: I.CMD,
                        subCmd: 0,
                        duration: e,
                        code: t
                    })
                }
                markScannerStarted() {
                    this.increaseFailed({
                        cmd: v.CMD,
                        subCmd: 0,
                        duration: 0,
                        code: 0
                    })
                }
            }
            var C, U, j, F;
            S.instance = void 0;
            let A = (C = Object(g.d)("run-scan-message"), U = Reflect.metadata("design:type", Function), j = Reflect.metadata("design:paramtypes", [Object]), F = class extends u.a {
                constructor(e, t) {
                    super(), this.mode = e, this.state = t, this.scannerManager = void 0, this.logger = void 0, this._version = void 0, this.totalItems = 0, this.config = void 0, this.isRunning = !1, this.onSuccess = async e => {
                        const t = this.state.getProgressKeys().length,
                            s = this.state.getTotalProgressKeys(),
                            r = s - t;
                        this.logger.zsymb(0, "NVtpYS", "Scan completed for conv: ", e.convId, e.metadata.range, this.version, r, s);
                        const o = Object(i.a)(Object(i.a)({}, e), {}, {
                            version: this.version,
                            metadata: Object(i.a)(Object(i.a)({}, e.metadata), {}, {
                                totalConversations: s,
                                scannedConversations: r
                            })
                        });
                        this.mode === n.c.Auto ? await a.ModuleContainer.resolve(M.a).completeBroadcastConv(o) : await this.dispatchEvent("conv-success", o), this.state.clearProgressData(e.convId)
                    }, this.onError = async e => {
                        this.logger.zsymb(18, "5Z8lbH", "Scan error:", e.message);
                        const t = Object(i.a)(Object(i.a)({}, e), {}, {
                            version: this.version
                        });
                        this.mode === n.c.Auto ? await a.ModuleContainer.resolve(M.a).broadcastError(t) : await this.dispatchEvent("error", t), this.trackScannerError(), this.totalItems = 0
                    }, this.onProgress = async e => {
                        var t, s, r, o;
                        this.totalItems += null !== (t = null === (s = e.data) || void 0 === s ? void 0 : s.length) && void 0 !== t ? t : 0;
                        const d = Object(i.a)(Object(i.a)({}, e), {}, {
                            version: this.version
                        });
                        this.mode === n.c.Auto ? await a.ModuleContainer.resolve(M.a).broadcastMessages(d) : await this.dispatchEvent("progress", d), this.trackTotalMessages(null !== (r = null === (o = e.data) || void 0 === o ? void 0 : o.length) && void 0 !== r ? r : 0), this.state.setProgressData(e.convId, e.metadata.range[1])
                    }, this.onCompleted = async () => {
                        this.logger.zsymb(0, "Pon8Wl", "Scan completed for all conversations"), await a.ModuleContainer.resolve(M.a).completeBroadcast({
                            version: this.version,
                            totalScannedMessage: this.state.getTotalMessage(),
                            totalScannedConversation: this.state.getTotalProgressKeys()
                        }), this.trackScannerComplete(), this.dispose()
                    }, this.getScanMessagesRangeForConv = e => {
                        var t;
                        return [null !== (t = this.state.getProgressData(e)) && void 0 !== t ? t : 0, Date.now()]
                    }, this.scannerManager = new f, this.logger = Object(o.a)("message-db-scan-controller"), this.logger.zsymb(0, "qpKQtW", "Create message db scan controller with mode: ", e)
                }
                get version() {
                    return this._version || (this._version = this.state.getVersion() || Object(_.b)()), this._version
                }
                setConfig(e) {
                    this.logger.zsymb(0, "9QgLrG", "Set scanner config: ", e), this.config = e
                }
                dispose() {
                    this.totalItems = 0, this.removeAllListeners(), this._version = void 0, this.state.clearVersion(), this.state.clearAllProgressData(), this.state.clearStartAt()
                }
                async run(e) {
                    if (this.isRunning) throw new Error("Scan is already running");
                    try {
                        this.isRunning = !0, this.config = e.config;
                        const t = 0 === a.ModuleContainer.resolve(M.a).countListeners();
                        if (this.mode === n.c.Auto && t) return void this.logger.zsymb(0, "ZjDsgT", "No listeners, skip scanning");
                        this.scannerManager.addEventListener("progress", this.onProgress), this.scannerManager.addEventListener("conv-success", this.onSuccess), this.scannerManager.addEventListener("error", this.onError), this.scannerManager.addEventListener("completed", this.onCompleted);
                        let s = this.state.getProgressKeys();
                        0 === s.length && (s = await Object(g.b)(), this.state.setProgressKeys(s), this.state.setTotalProgressKeys(s.length)), this.state.setVersion(this.version), this.logger.zsymb(0, "FfblOE", "Start scan with clients: ", a.ModuleContainer.resolve(M.a).countListeners()), this.state.getStartAt() || (this.state.setStartAt(Date.now()), await a.ModuleContainer.resolve(M.a).broadcastStarted({
                            version: this.version
                        })), await this.scannerManager.run(Object(i.a)(Object(i.a)({}, e), {}, {
                            convIds: s,
                            getScannerRange: this.getScanMessagesRangeForConv,
                            getScannerConfig: () => this.config ? this.config : e.config
                        }))
                    } catch (t) {
                        throw this.onError({
                            message: null == t ? void 0 : t.message,
                            convId: ""
                        }), this.logger.zsymb(21, "2mLXTC", ["Error when start scan {}", "61czj-"], t), t
                    } finally {
                        this.isRunning = !1, this.scannerManager.dispose()
                    }
                }
                trackScannerComplete() {
                    this.logger.zsymb(0, "uUzJl9", "Total items scanned in this run: ", this.totalItems), this.logger.zsymb(0, "RrkWfd", "Total conversations scanned: ", this.state.getTotalProgressKeys()), this.logger.zsymb(0, "U524oQ", "Total messages scanned: ", this.state.getTotalMessage());
                    const e = Date.now() - this.state.getStartAt();
                    S.getInstance().markScannerSuccess(e, this.state.getTotalMessage(), this.state.getTotalProgressKeys())
                }
                trackScannerError() {
                    const e = Date.now() - this.state.getStartAt();
                    S.getInstance().markScannerError(e)
                }
                trackTotalMessages(e) {
                    try {
                        this.state.setTotalMessage(this.state.getTotalMessage() + e)
                    } catch (t) {
                        this.logger.zsymb(18, "_5Ll07", "Error when track total messages", t)
                    }
                }
            }, Object(r.a)(F.prototype, "run", [C, U, j], Object.getOwnPropertyDescriptor(F.prototype, "run"), F.prototype), F)
        },
        h1h5: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            }));
            class i {
                constructor() {
                    this._listeners = {}
                }
                addEventListener(e, t) {
                    var s;
                    const i = null !== (s = this._listeners[e]) && void 0 !== s ? s : [];
                    return i.push(t), this._listeners[e] = i, this
                }
                removeEventListener(e, t) {
                    var s;
                    this._listeners[e] = null !== (s = this._listeners[e]) && void 0 !== s ? s : [];
                    const i = this._listeners[e].indexOf(t);
                    return -1 === i || this._listeners[e].splice(i, 1), this
                }
                async dispatchEvent(e, t, s = !1) {
                    var i;
                    const r = null !== (i = this._listeners[e]) && void 0 !== i ? i : [];
                    if (r.length)
                        if (s)
                            for (const a of r) await a(t);
                        else await Promise.all(r.map((e => e(t))))
                }
                removeAllListeners() {
                    this._listeners = {}
                }
            }
        },
        hlqx: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            }));
            class i {
                constructor() {
                    this.storage = void 0, this.name = void 0, this.storage = {}, this.name = "InMemoryStorage"
                }
                init() {
                    return this.storage
                }
                get(e) {
                    return this.storage[e]
                }
                set(e, t) {
                    this.storage[e] = t
                }
                clear(e) {
                    delete this.storage[e]
                }
                clearAll() {
                    this.storage = {}
                }
                persist() {}
            }
        },
        hpRf: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            }));
            class i {
                constructor(e) {
                    this.name = void 0, this.name = e
                }
                setItemForCurrentUser(e, t) {
                    this.store.set(e, t)
                }
                removeItemForCurrentUser(e) {
                    this.store.delete(e)
                }
                getItemForCurrentUser(e) {
                    var t;
                    return null !== (t = this.store.get(e)) && void 0 !== t ? t : null
                }
                clear() {
                    this.store.clear()
                }
                get store() {
                    return $zelectron.getElectronStore({
                        name: this.name
                    })
                }
            }
        },
        jB3n: function(e, t, s) {
            "use strict";
            var i = s("OgkW");
            t.a = class {
                constructor() {
                    this._eventEmitter = new i.EventEmitter
                }
                addListener(e, t) {
                    this.eventEmitter.addListener(e, t)
                }
                emit(e, ...t) {
                    return this.eventEmitter.emit(e, ...t)
                }
                on(e, t) {
                    this.eventEmitter.on(e, t)
                }
                once(e, t) {
                    this.eventEmitter.once(e, t)
                }
                off(e, t) {
                    this.eventEmitter.off(e, t)
                }
                removeAllListeners(e) {
                    this.eventEmitter.removeAllListeners(e)
                }
                removeListener(e, t) {
                    this.eventEmitter.removeListener(e, t)
                }
                get eventEmitter() {
                    return this._eventEmitter
                }
            }
        },
        jbAT: function(e, t, s) {
            "use strict";
            var i = s("mvkY");
            s.d(t, "a", (function() {
                return i.IUtilsMediaAppService
            })), s.d(t, "b", (function() {
                return i.c
            }))
        },
        kAC1: function(e, t, s) {
            "use strict";
            s.d(t, "c", (function() {
                return o
            })), s.d(t, "d", (function() {
                return l
            })), s.d(t, "a", (function() {
                return c
            })), s.d(t, "b", (function() {
                return m
            }));
            var i = s("XmYa"),
                r = s("Oy95"),
                a = s("fsN4");
            const n = Object(r.a)("execution-timer"),
                o = e => i.d.includes(e.msgType),
                d = (e, t) => {
                    n.zsymb(6, "_oHWp_", ` function ${e} takes ${t}ms to complete`)
                },
                l = e => (t, s, i) => {
                    const r = i.value;
                    return i.value = async function(...t) {
                        const i = e || s,
                            a = performance.now();
                        try {
                            const e = await r.apply(this, t),
                                s = performance.now();
                            return d(i, s - a), e
                        } catch (n) {
                            const e = performance.now();
                            throw d(i, e - a), n
                        }
                    }, i
                };
            async function c(e, t) {
                const s = new Promise(((e, s) => {
                    setTimeout((() => {
                        s(new Error(`Function execution timed out after ${t}ms`))
                    }), t)
                }));
                return Promise.race([e(), s])
            }
            const m = async () => {
                const e = a.default.getInstance(),
                    t = new Set;
                let s = "";
                for (;;) {
                    var i;
                    const r = await e.Core.Conversation.getAll({
                        from: [s],
                        to: ["g9999999999999999999"],
                        excludeFrom: !0,
                        excludeTo: !1
                    });
                    if (0 === r.length) break;
                    for (const {
                            userId: e
                        }
                        of r) e && t.add(e);
                    const a = null === (i = r[r.length - 1]) || void 0 === i ? void 0 : i.userId;
                    if (!a || a === s) break;
                    s = a
                }
                return Array.from(t)
            }
        },
        ma9L: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return n
            }));
            var i = s("jDHv"),
                r = s("Mgpg");
            const a = i.ModuleContainer.resolve(r.ZLoggerFactory).createZLogger("env-isolated-utils");

            function n(e, t, s) {
                if (!t || !t.length) throw Error("[insertMulti]: items is undefined or empty!");
                const i = [];
                return t.forEach((e => {
                    e.cliMsgId && e.fromUid && e.convId ? (e.mediaId = `${e.cliMsgId}_${e.fromUid}_${e.convId}`, i.push(e)) : a.zsymb(21, "P6ak6S", ["[insertMulti]: media doesn't have valid key_from_to: {}_{}_{}", "FDA9C1"], e.cliMsgId, e.fromUid, e.convId)
                })), e.insertMulti(i, s)
            }
        },
        mvkY: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return r
            })), s.d(t, "c", (function() {
                return a
            }));
            var i = s("jDHv");
            const r = "utils-media-app-service",
                a = Object(i.define)(r)
        },
        nhJq: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return a
            })), s.d(t, "a", (function() {
                return n
            })), s.d(t, "c", (function() {
                return d
            }));
            let i = $znode.rimraf,
                r = $znode.fs;
            async function a(e) {
                if (await o(e)) return new Promise(((t, s) => {
                    i(e, (e => {
                        e ? s(e) : t()
                    }))
                }))
            }
            async function n(e) {
                return e.endsWith("/") || (e += "/"), await o(e) ? e : new Promise(((t, s) => {
                    r.mkdir(e, (i => {
                        i ? s(i) : t(e)
                    }))
                }))
            }

            function o(e) {
                return new Promise((t => {
                    r.stat(e, (e => {
                        t(!e)
                    }))
                }))
            }

            function d(e) {
                return new Promise(((t, s) => {
                    r.readdir(e, ((e, i) => {
                        e ? s(e) : t(i)
                    }))
                }))
            }
        },
        oqw1: function(e, t, s) {
            "use strict";
            s.r(t);
            var i, r = s("jDHv"),
                a = s("xM06"),
                n = s("x9oK"),
                o = s("drXQ"),
                d = s("Hw41");
            let l = r.ModuleContainer.injectable()(i = class {
                async createAdapter(e, t) {
                    return o.a.factory(e, t)
                }
                async getExistedPartitionKeys(e) {
                    const t = r.ModuleContainer.resolve(d.b),
                        s = await t.getPort(),
                        i = {
                            typeID: a.j,
                            data: {
                                type: a.p,
                                data: {
                                    fullname: e
                                }
                            }
                        };
                    return s.invokeMessage(i)
                }
            }) || i;
            r.ModuleContainer.registerSingleton(n.c, l)
        },
        q8PC: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return i
            }));
            class i {
                constructor(e) {
                    this.userId = void 0, this.name = void 0, this.name = e, this.userId = null
                }
                isInitialized() {
                    return null !== this.userId
                }
                init(e) {
                    this.userId = e
                }
                computeUserKey(e) {
                    if (null === this.userId) throw new Error("Session not found");
                    return `${this.userId}_${e}`
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
                get store() {
                    return $zelectron.getElectronStore({
                        name: this.name
                    })
                }
            }
        },
        qUG6: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("3jnX");
            class r extends i.b {
                constructor(e) {
                    super({
                        type: "UPDATE_CONVERSATION_PREVIEW",
                        params: e
                    })
                }
            }
        },
        rBRb: function(e, t, s) {
            "use strict";
            var i, r = s("jDHv"),
                a = s("fsN4"),
                n = s("KPNQ");
            let o = Object(r.injectable)()(i = class {
                async toMediaPKFromMessagePK(e, t) {
                    if (!t) return "";
                    const s = await a.default.getInstance().Core.Message.get(t, {
                        partition: e
                    });
                    return s && (s.cliMsgId || s.fromUid || s.toUid) ? `${s.cliMsgId}_${s.fromUid}_${s.toUid}` : ""
                }
                async toMessagePKFromMediaPK(e) {
                    const [t, s, i] = this._getThreePartsFromMediaPK(e);
                    if (!t || !s || !i) return "";
                    const r = await this._getMessagesByCliMsgIdRange(t, i);
                    let a = "";
                    return r.forEach((e => {
                        e.fromUid === s && e.toUid === i && (a = e.msgId)
                    })), a
                }
                _getMessagesByCliMsgIdRange(e, t) {
                    const s = {
                            from: e,
                            to: e,
                            excludeFrom: !1,
                            excludeTo: !1
                        },
                        i = {
                            index: "cliMsgIdIndex",
                            partition: t
                        };
                    return a.default.getInstance().Core.Message.getAll(s, i)
                }
                _getThreePartsFromMediaPK(e) {
                    if ("string" != typeof e || !e) return [];
                    const [t, s, i] = e.split("_");
                    return [t, s, i]
                }
            }) || i;
            r.ModuleContainer.register(n.b, o);
            const d = Object(r.define)("utils-media-mapper");
            var l, c = s("RgQf"),
                m = s("MLPV"),
                u = s("J/8C");
            let g = Object(r.injectable)()(l = class {
                toUtilsMediaDTOFromDomain(e) {
                    return {
                        id: e.id,
                        convId: e.convId,
                        mediaType: e.mediaType,
                        senderIds: e.senderIds,
                        fileTypes: e.fileTypes
                    }
                }
            }) || l;
            r.ModuleContainer.register(d, g);
            var h, p = s("VTBJ"),
                f = s("v6qY"),
                M = s("IZCB");

            function _(e) {
                try {
                    const t = JSON.parse(e),
                        {
                            fileExt: s,
                            fType: i
                        } = t;
                    return "zip" === s && 2 === i
                } catch (t) {
                    return !1
                }
            }

            function b(e) {
                try {
                    if (_(e)) return "folder";
                    const t = JSON.parse(e);
                    if (!t) return "";
                    const {
                        fileExt: s
                    } = t;
                    return s.toLowerCase()
                } catch (t) {
                    return ""
                }
            }

            function y(e) {
                try {
                    const t = JSON.parse(e);
                    if (!t) return {
                        width: null,
                        height: null
                    };
                    const {
                        width: s,
                        height: i
                    } = t;
                    return {
                        width: s,
                        height: i
                    }
                } catch (t) {
                    return {
                        width: null,
                        height: null
                    }
                }
            }
            let I = Object(r.injectable)()(h = class {
                toDomainFromOldDomain(e, t = `${f.c.UNKNOWN}${f.d.FROM_OLD_DB}`) {
                    if (!e || !e.message) throw Error("This oldImageEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This oldImageEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const s = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        i = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${i}_${e.userId}`,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: i,
                        content: {
                            title: e.message.title || null,
                            thumbUrl: e.message.thumbUrl || "",
                            hdUrl: e.message.hdUrl || "",
                            normalUrl: e.message.normalUrl || "",
                            oriUrl: e.message.oriUrl,
                            params: e.message.params,
                            duration: e.message.duration || null
                        },
                        type: "number" == typeof e.subType ? e.subType : -1,
                        src: t,
                        sendDttm: s,
                        ttl: e.ttl,
                        localPath: e.localPath || "",
                        previewThumb: e.previewThumb || "",
                        modifiedTime: e.updateTime || s,
                        metadata: Object(p.a)(Object(p.a)({}, y(e.message.params)), {}, {
                            vOrient: M.a.None
                        })
                    }
                }
                toDomainFromDTO(e) {
                    if (!e || !e.message) throw Error("This imageDTO isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This imageDTO doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const t = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        s = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: e.mediaId,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: s,
                        content: {
                            title: e.message.title || null,
                            thumbUrl: e.message.thumbUrl || "",
                            hdUrl: e.message.hdUrl || "",
                            normalUrl: e.message.normalUrl || "",
                            oriUrl: e.message.oriUrl,
                            params: e.message.params,
                            duration: e.message.duration || null
                        },
                        type: "number" == typeof e.subType ? e.subType : -1,
                        sendDttm: t,
                        src: e.src,
                        ttl: "number" == typeof e.ttl ? e.ttl : 0,
                        localPath: e.localPath || "",
                        previewThumb: e.previewThumb || "",
                        modifiedTime: "number" == typeof e.updateTime ? e.updateTime : t,
                        metadata: Object(p.a)(Object(p.a)({}, y(e.message.params)), {}, {
                            vOrient: e.vOrient || M.a.None
                        })
                    }
                }
                toDomainFromTMessage(e, t, s = `${f.c.UNKNOWN}${f.d.FROM_MSG}`) {
                    if (!e || !e.message) throw Error("This messageEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.toUid) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                toUid: e.toUid
                            },
                            s = `This messageEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const i = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        r = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${r}_${e.toUid}`,
                        convId: e.toUid,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: r,
                        content: {
                            title: e.message.title || null,
                            thumbUrl: e.message.thumbUrl || "",
                            hdUrl: e.message.hdUrl || "",
                            normalUrl: e.message.normalUrl || "",
                            oriUrl: e.message.oriUrl,
                            params: e.message.params,
                            duration: e.message.duration || null
                        },
                        src: s,
                        type: t,
                        sendDttm: i,
                        ttl: e.ttl || 0,
                        localPath: e.localPath || "",
                        previewThumb: e.previewThumb || "",
                        modifiedTime: i,
                        metadata: Object(p.a)(Object(p.a)({}, y(e.message.params)), {}, {
                            vOrient: M.a.None
                        })
                    }
                }
                async toDTO(e, t = !0) {
                    if (!e || !e.mediaId) throw Error("This imageEntity isn't valid!");
                    const s = e.msgId ? e.msgId : t ? await r.ModuleContainer.resolve(n.b).toMessagePKFromMediaPK(e.mediaId) : "";
                    return {
                        mediaId: e.mediaId,
                        msgId: s,
                        cliMsgId: e.cliMsgId,
                        fromUid: e.fromUid,
                        userId: e.convId,
                        message: {
                            title: e.content.title,
                            description: "",
                            childnumber: 0,
                            action: "",
                            params: e.content.params,
                            type: "",
                            thumbUrl: e.content.thumbUrl,
                            oriUrl: e.content.oriUrl,
                            hdUrl: e.content.hdUrl,
                            normalUrl: e.content.normalUrl,
                            duration: e.content.duration || null,
                            thumb: "",
                            href: ""
                        },
                        sendDttm: e.sendDttm,
                        src: e.src,
                        ttl: e.ttl,
                        type: "image",
                        subType: e.type,
                        id: 0,
                        localPath: e.localPath,
                        previewThumb: e.previewThumb,
                        updateTime: e.modifiedTime,
                        width: e.metadata.width,
                        height: e.metadata.height,
                        vOrient: e.metadata.vOrient
                    }
                }
                toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e) {
                    return {
                        convId: e.convId,
                        fromUid: e.fromUid,
                        mediaType: "image",
                        content: {
                            params: e.content.params
                        }
                    }
                }
                toOldEAttsFromNewEAtts(e) {
                    if (!e || "object" != typeof e) throw Error("newEAtts is undefined or not valid!");
                    const t = {};
                    var s, i;
                    (e.hasOwnProperty("message") && "object" == typeof e.content && (t.message = {
                        title: e.content.title,
                        description: "",
                        childnumber: 0,
                        action: "",
                        params: e.content.params,
                        type: "",
                        thumbUrl: e.content.thumbUrl,
                        oriUrl: e.content.oriUrl,
                        hdUrl: e.content.hdUrl,
                        normalUrl: e.content.normalUrl,
                        duration: e.content.duration || null,
                        thumb: "",
                        href: ""
                    }), e.hasOwnProperty("subType") && (t.subType = e.type), e.hasOwnProperty("sendDttm") && (t.sendDttm = "string" == typeof e.sendDttm ? +e.sendDttm : e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("localPath") && (t.localPath = e.localPath || ""), e.hasOwnProperty("previewThumb") && (t.previewThumb = e.previewThumb || ""), e.hasOwnProperty("updateTime") && (t.updateTime = e.modifiedTime || Date.now()), e.hasOwnProperty("metadata")) && (t.width = null === (s = e.metadata) || void 0 === s ? void 0 : s.width, t.height = null === (i = e.metadata) || void 0 === i ? void 0 : i.height);
                    return t
                }
                toNewEAttsFromDTOAtts(e) {
                    if (!e || "object" != typeof e) throw Error("dtoAtts is undefined or not valid!");
                    const t = {};
                    return e.hasOwnProperty("message") && "object" == typeof e.message && (t.content = {
                        title: e.message.title || null,
                        thumbUrl: e.message.thumbUrl || "",
                        hdUrl: e.message.hdUrl || "",
                        normalUrl: e.message.normalUrl || "",
                        oriUrl: e.message.oriUrl,
                        params: e.message.params,
                        duration: e.message.duration || null
                    }), e.hasOwnProperty("subType") && (t.type = e.subType), e.hasOwnProperty("sendDttm") && (t.sendDttm = "string" == typeof e.sendDttm ? +e.sendDttm : e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("localPath") && (t.localPath = e.localPath || ""), e.hasOwnProperty("previewThumb") && (t.previewThumb = e.previewThumb || ""), e.hasOwnProperty("updateTime") && (t.modifiedTime = e.updateTime || Date.now()), e.hasOwnProperty("width") && e.hasOwnProperty("height") && e.hasOwnProperty("vOrient") && (t.metadata = {
                        width: e.width,
                        height: e.height,
                        vOrient: e.vOrient || 0
                    }), t
                }
            }) || h;
            var v;
            r.ModuleContainer.register(m.b, I);
            let O = Object(r.injectable)()(v = class {
                toDomainFromOldDomain(e, t = `${f.c.UNKNOWN}${f.d.FROM_OLD_DB}`) {
                    if (!e || !e.message) throw Error("This oldFileEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This oldFileEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const s = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        i = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${i}_${e.userId}`,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: i,
                        content: {
                            title: e.message.title,
                            href: e.message.href,
                            params: e.message.params,
                            thumb: e.message.thumb || ""
                        },
                        type: _(e.message.params) ? f.a.FOLDER : f.a.FILE,
                        src: t,
                        extType: b(e.message.params),
                        sendDttm: s,
                        ttl: "number" == typeof e.ttl ? e.ttl : 0,
                        modifiedTime: e.updateTime || s,
                        localPath: e.localPath || "",
                        folderPath: e.folderPath || "",
                        thumbMetadata: e.dimension ? {
                            width: e.dimension.width,
                            height: e.dimension.height,
                            type: e.dimension.type,
                            orientation: Object(p.a)({}, e.dimension.orientation),
                            bigRes: e.dimension.bigRes
                        } : null
                    }
                }
                toDomainFromDTO(e) {
                    if (!e || !e.message) throw Error("This fileDTO isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This fileDTO doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const t = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        s = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: e.mediaId,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: s,
                        content: {
                            title: e.message.title,
                            href: e.message.href,
                            params: e.message.params,
                            thumb: e.message.thumb || ""
                        },
                        type: e.fileType || _(e.message.params) ? f.a.FOLDER : f.a.FILE,
                        src: e.src,
                        extType: e.extType || b(e.message.params),
                        sendDttm: t,
                        ttl: "number" == typeof e.ttl ? e.ttl : 0,
                        modifiedTime: e.updateTime || t,
                        localPath: e.localPath || "",
                        folderPath: e.folderPath || "",
                        thumbMetadata: e.dimension ? {
                            width: e.dimension.width,
                            height: e.dimension.height,
                            type: e.dimension.type,
                            orientation: Object(p.a)({}, e.dimension.orientation),
                            bigRes: e.dimension.bigRes
                        } : null
                    }
                }
                async toDTO(e) {
                    if (!e || !e.mediaId || !e.content) throw Error("This fileEntity isn't valid!");
                    const t = e.msgId ? e.msgId : await r.ModuleContainer.resolve(n.b).toMessagePKFromMediaPK(e.mediaId);
                    return {
                        mediaId: e.mediaId,
                        msgId: t,
                        cliMsgId: e.cliMsgId,
                        fromUid: e.fromUid,
                        userId: e.convId,
                        message: {
                            title: e.content.title,
                            href: e.content.href,
                            params: e.content.params,
                            thumb: e.content.thumb,
                            childnumber: 0,
                            action: "",
                            description: "",
                            type: "",
                            thumbUrl: "",
                            oriUrl: ""
                        },
                        sendDttm: e.sendDttm,
                        ttl: e.ttl,
                        src: e.src,
                        type: "file",
                        fileType: e.type,
                        extType: e.extType,
                        id: 0,
                        updateTime: e.modifiedTime,
                        localPath: e.localPath || "",
                        folderPath: e.folderPath || "",
                        downloadError: !1,
                        dimension: "object" == typeof e.thumbMetadata ? e.thumbMetadata : null,
                        previewThumb: ""
                    }
                }
                toDomainFromMessage(e, t = `${f.c.UNKNOWN}${f.d.FROM_MSG}`) {
                    if (!e || !e.msgId || !e.message) throw Error("This messageEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.toUid) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                toUid: e.toUid
                            },
                            s = `This messageEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const s = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        i = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${i}_${e.toUid}`,
                        convId: e.toUid,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: i,
                        content: {
                            title: e.message.title,
                            href: e.message.href,
                            params: e.message.params,
                            thumb: e.message.thumb || ""
                        },
                        type: _(e.message.params) ? f.a.FOLDER : f.a.FILE,
                        src: t,
                        extType: b(e.message.params),
                        sendDttm: s,
                        ttl: e.ttl || 0,
                        modifiedTime: s,
                        localPath: e.localPath || "",
                        folderPath: e.folderPath || "",
                        thumbMetadata: null
                    }
                }
                toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e) {
                    return {
                        convId: e.convId,
                        fromUid: e.fromUid,
                        mediaType: "file",
                        content: {
                            params: e.content.params
                        }
                    }
                }
                toOldEAttsFromNewEAtts(e) {
                    if (!e || "object" != typeof e) throw Error("newEAtts isn't valid!");
                    const t = {};
                    return e.hasOwnProperty("message") && "object" == typeof e.content && (t.message = {
                        title: e.content.title,
                        href: e.content.href,
                        params: e.content.params,
                        thumb: e.content.thumb,
                        childnumber: 0,
                        action: "",
                        description: "",
                        type: "",
                        thumbUrl: "",
                        oriUrl: ""
                    }), e.hasOwnProperty("sendDttm") && (t.sendDttm = e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("localPath") && (t.localPath = e.localPath || ""), e.hasOwnProperty("folderPath") && (t.folderPath = e.folderPath || ""), e.hasOwnProperty("updateTime") && (t.updateTime = e.modifiedTime || Date.now()), e.hasOwnProperty("dimension") && (t.dimension = e.thumbMetadata ? {
                        width: e.thumbMetadata.width,
                        height: e.thumbMetadata.height,
                        type: e.thumbMetadata.type,
                        orientation: e.thumbMetadata.orientation,
                        bigRes: e.thumbMetadata.bigRes
                    } : null), t
                }
                toNewEAttsFromDTOAtts(e) {
                    if (!e || "object" != typeof e) throw Error("dtoAtts isn't valid!");
                    const t = {};
                    return e.hasOwnProperty("message") && "object" == typeof e.message && (t.content = {
                        title: e.message.title,
                        href: e.message.href,
                        params: e.message.params,
                        thumb: e.message.thumb || ""
                    }), e.hasOwnProperty("fileType") && (t.type = e.fileType), e.hasOwnProperty("extType") && (t.extType = e.extType || ""), e.hasOwnProperty("sendDttm") && (t.sendDttm = "string" == typeof e.sendDttm ? +e.sendDttm : e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("localPath") && (t.localPath = e.localPath || ""), e.hasOwnProperty("folderPath") && (t.folderPath = e.folderPath || ""), e.hasOwnProperty("updateTime") && (t.modifiedTime = e.updateTime || Date.now()), e.hasOwnProperty("dimension") && (t.thumbMetadata = e.dimension ? {
                        width: e.dimension.width,
                        height: e.dimension.height,
                        type: e.dimension.type,
                        orientation: e.dimension.orientation,
                        bigRes: e.dimension.bigRes
                    } : null), t
                }
            }) || v;
            var D;
            r.ModuleContainer.register(c.a, O);
            let T = Object(r.injectable)()(D = class {
                toDomainFromOldDomain(e, t = `${f.c.UNKNOWN}${f.d.FROM_OLD_DB}`) {
                    if (!e || !e.message) throw Error("This oldLinkEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This oldLinkEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const s = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        i = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${i}_${e.userId}`,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: i,
                        content: {
                            title: "object" == typeof e.message.title ? e.message.title.title || "" : e.message.title,
                            params: e.message.params || "",
                            href: e.message.href || "",
                            thumb: e.message.thumb || "",
                            description: e.message.description || "",
                            type: e.message.type || ""
                        },
                        type: -1,
                        src: t,
                        sendDttm: s,
                        ttl: e.ttl,
                        modifiedTime: e.updateTime || s,
                        parsedInfo: null
                    }
                }
                toDomainFromDTO(e) {
                    if (!e || !e.message) throw Error("This linkDTO isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.userId) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                userId: e.userId
                            },
                            s = `This linkDTO doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const t = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        s = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: e.mediaId,
                        convId: e.userId,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: s,
                        content: {
                            title: "object" == typeof e.message.title ? e.message.title.title : e.message.title || "",
                            params: e.message.params || "",
                            href: e.message.href || "",
                            thumb: e.message.thumb || "",
                            description: e.message.description || "",
                            type: e.message.type
                        },
                        src: e.src,
                        type: "number" == typeof e.linkType ? e.linkType : -1,
                        sendDttm: t,
                        ttl: "number" == typeof e.ttl ? e.ttl : 0,
                        modifiedTime: e.updateTime || t,
                        parsedInfo: null
                    }
                }
                async toDTO(e) {
                    if (!e || !e.mediaId) throw Error("This linkEntity isn't valid!");
                    const t = e.msgId ? e.msgId : await r.ModuleContainer.resolve(n.b).toMessagePKFromMediaPK(e.mediaId);
                    return {
                        mediaId: e.mediaId,
                        msgId: t,
                        cliMsgId: parseInt(e.cliMsgId),
                        fromUid: e.fromUid,
                        userId: e.convId,
                        message: {
                            title: e.content.title,
                            params: e.content.params,
                            href: e.content.href,
                            thumb: e.content.thumb,
                            description: e.content.description,
                            type: e.content.type,
                            action: "",
                            childnumber: 0,
                            oriUrl: "",
                            thumbUrl: ""
                        },
                        sendDttm: e.sendDttm,
                        type: "link",
                        ttl: e.ttl,
                        src: e.src,
                        linkType: e.type,
                        id: 0,
                        updateTime: e.modifiedTime,
                        previewThumb: "",
                        parsedInfo: "object" == typeof e.parsedInfo ? Object(p.a)({}, e.parsedInfo) : null
                    }
                }
                toDomainFromMessage(e, t = `${f.c.UNKNOWN}${f.d.FROM_MSG}`) {
                    if (!e) throw Error("This messageEntity isn't valid!");
                    if (!e.cliMsgId || "0" != e.fromUid && !e.fromUid || !e.toUid) {
                        0;
                        const t = {
                                cliMsgId: e.cliMsgId,
                                fromUid: e.fromUid,
                                toUid: e.toUid
                            },
                            s = `This messageEntity doesn't have key_from_to valid: ${JSON.stringify(t)}`;
                        throw Error(s)
                    }
                    const s = "string" == typeof e.sendDttm ? parseInt(e.sendDttm) : e.sendDttm,
                        i = "number" == typeof e.fromUid ? e.fromUid.toString() : e.fromUid;
                    return {
                        mediaId: `${e.cliMsgId}_${i}_${e.toUid}`,
                        convId: e.toUid,
                        cliMsgId: "number" == typeof e.cliMsgId ? e.cliMsgId.toString() : e.cliMsgId,
                        fromUid: i,
                        content: {
                            title: "object" === e.message.title ? e.message.title.title : e.message.title || "",
                            params: e.message.params || "",
                            href: e.message.href || "",
                            thumb: e.message.thumb || "",
                            description: e.message.description || "",
                            type: e.message.type
                        },
                        type: -1,
                        src: t,
                        sendDttm: s,
                        ttl: e.ttl || 0,
                        modifiedTime: s,
                        parsedInfo: null
                    }
                }
                toMediaToCreateOrUpdateUtilsMediaDTOFromDomain(e) {
                    return {
                        convId: e.convId,
                        fromUid: e.fromUid,
                        mediaType: "link",
                        content: {
                            params: e.content.params
                        }
                    }
                }
                toOldEAttsFromNewEAtts(e) {
                    if (!e || "object" != typeof e) throw Error("newEAtts isn't valid!");
                    const t = {};
                    return e.hasOwnProperty("content") && "object" == typeof e.content && (t.message = {
                        title: e.content.title,
                        params: e.content.params,
                        href: e.content.href,
                        thumb: e.content.thumb,
                        description: e.content.description,
                        type: e.content.type,
                        action: "",
                        childnumber: 0,
                        oriUrl: "",
                        thumbUrl: ""
                    }), e.hasOwnProperty("sendDttm") && (t.sendDttm = e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("updateTime") && (t.updateTime = e.modifiedTime || Date.now()), t
                }
                toNewEAttsFromDTOAtts(e) {
                    if (!e || "object" != typeof e) throw Error("dtoAtts isn't valid!");
                    const t = {};
                    var s;
                    e.hasOwnProperty("message") && "object" == typeof e.message && (t.content = {
                        title: "object" == typeof e.message.title ? (null === (s = e.message.title) || void 0 === s ? void 0 : s.title) || "" : e.message.title || "",
                        params: e.message.params || "",
                        href: e.message.href || "",
                        thumb: e.message.thumb || "",
                        description: e.message.description || "",
                        type: e.message.type || ""
                    });
                    return e.hasOwnProperty("sendDttm") && (t.sendDttm = "string" == typeof e.sendDttm ? +e.sendDttm : e.sendDttm), e.hasOwnProperty("ttl") && (t.ttl = e.ttl || 0), e.hasOwnProperty("linkType") && (t.type = e.linkType), e.hasOwnProperty("updateTime") && (t.modifiedTime = e.updateTime || Date.now()), e.hasOwnProperty("parsedInfo") && (t.parsedInfo = e.parsedInfo ? {
                        protocol: e.parsedInfo.protocol || "",
                        host: e.parsedInfo.domain || ""
                    } : null), t
                }
            }) || D;
            r.ModuleContainer.register(u.b, T);
            var w, E = s("AjFa"),
                S = s("Mgpg"),
                C = s("H8Z7");
            Object(r.injectable)()(w = Object(r.singleton)(E.b)(w = function(e, t) {
                return Object(r.inject)(m.b)(e, void 0, 0)
            }(w = function(e, t) {
                return Object(r.inject)(C.c)(e, void 0, 1)
            }(w = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 2)
            }(w = Reflect.metadata("design:type", Function)(w = Reflect.metadata("design:paramtypes", [void 0 === m.IImageMediaMapper ? Object : m.IImageMediaMapper, void 0 === C.IMediaMigrationManager ? Object : C.IMediaMigrationManager, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(w = class extends E.a {
                constructor(e, t, s) {
                    super("Media", "Image", "image", "crud-image-media", t, s), this.imageMediaMapper = void 0, this.oldDBTable = void 0, this.imageMediaMapper = e, this.oldDBTable = this._dalInstance.Core.Image
                }
                getOldDBTable() {
                    return this.oldDBTable || (this.oldDBTable = a.default.getInstance().Core.Image), this.oldDBTable
                }
                getMediaMapper() {
                    return this.imageMediaMapper
                }
                deleteMatch(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                getAllKey(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                put(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                runTransaction(e, t, s) {
                    throw Error("Currently, this method isn't supported!")
                }
            }) || w) || w) || w) || w) || w) || w);
            var U, j = s("W13p");
            Object(r.injectable)()(U = Object(r.singleton)(j.b)(U = function(e, t) {
                return Object(r.inject)(c.a)(e, void 0, 0)
            }(U = function(e, t) {
                return Object(r.inject)(C.c)(e, void 0, 1)
            }(U = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 2)
            }(U = Reflect.metadata("design:type", Function)(U = Reflect.metadata("design:paramtypes", [void 0 === c.IFileMediaMapper ? Object : c.IFileMediaMapper, void 0 === C.IMediaMigrationManager ? Object : C.IMediaMigrationManager, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(U = class extends j.a {
                constructor(e, t, s) {
                    super("Media", "File", "file", "crud-file-media", t, s), this.fileMediaMapper = void 0, this.oldDBTable = void 0, this.fileMediaMapper = e, this.oldDBTable = this._dalInstance.Core.File
                }
                getOldDBTable() {
                    return this.oldDBTable || (this.oldDBTable = a.default.getInstance().Core.File), this.oldDBTable
                }
                getMediaMapper() {
                    return this.fileMediaMapper
                }
                deleteMatch(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                getAllKey(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                put(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                runTransaction(e, t, s) {
                    throw Error("Currently, this method isn't supported!")
                }
            }) || U) || U) || U) || U) || U) || U);
            var F, A = s("5R0e");
            Object(r.injectable)()(F = Object(r.singleton)(A.b)(F = function(e, t) {
                return Object(r.inject)(u.b)(e, void 0, 0)
            }(F = function(e, t) {
                return Object(r.inject)(C.c)(e, void 0, 1)
            }(F = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 2)
            }(F = Reflect.metadata("design:type", Function)(F = Reflect.metadata("design:paramtypes", [void 0 === u.ILinkMediaMapper ? Object : u.ILinkMediaMapper, void 0 === C.IMediaMigrationManager ? Object : C.IMediaMigrationManager, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(F = class extends A.a {
                constructor(e, t, s) {
                    super("Media", "Link", "link", "crud-link-media", t, s), this.linkMediaMapper = void 0, this.oldDBTable = void 0, this.linkMediaMapper = e, this.oldDBTable = this._dalInstance.Core.Link
                }
                getOldDBTable() {
                    return this.oldDBTable || (this.oldDBTable = a.default.getInstance().Core.Link), this.oldDBTable
                }
                getMediaMapper() {
                    return this.linkMediaMapper
                }
                deleteMatch(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                getAllKey(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                put(e, t) {
                    throw Error("Currently, this method isn't supported!")
                }
                runTransaction(e, t, s) {
                    throw Error("Currently, this method isn't supported!")
                }
            }) || F) || F) || F) || F) || F) || F);
            var N, P = s("hWjG"),
                R = s("4pKu"),
                k = s("ndDP");
            class L {
                constructor(e) {
                    this._lruCache = void 0, this._lruCache = new k.default(e)
                }
                get(e) {
                    return this._lruCache.get(e)
                }
                getAll(e = "ASC") {
                    const t = "ASC" === e ? this._lruCache.entriesAscending() : this._lruCache.entriesDescending();
                    return Object.values(t)
                }
                getAllKey(e = "ASC") {
                    const t = "ASC" === e ? this._lruCache.entriesAscending() : this._lruCache.entriesDescending();
                    return Object.keys(t)
                }
                set(e, t) {
                    this._lruCache.set(e, t)
                }
                delete(e) {
                    return this._lruCache.delete(e)
                }
                clear() {
                    this._lruCache.clear()
                }
            }
            Object(r.injectable)()(N = Object(r.singleton)(R.b)(N = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 0)
            }(N = Reflect.metadata("design:type", Function)(N = Reflect.metadata("design:paramtypes", [void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(N = class extends P.b {
                constructor(e) {
                    super("Media", "UtilsMedia", "utils-media-repo", e, "id", f.b), this._cache = new L({
                        maxSize: f.b
                    })
                }
                async isExisted(e) {
                    if (!e) return !1;
                    const t = this._cache.get(e);
                    if (!t) {
                        const t = await this._dbCollection.get(e);
                        return t && this._cache.set(e, t), !!t
                    }
                    return !!t
                }
                async update(e, t) {
                    if (await this._dbCollection.update(e, t)) {
                        const t = await this._dbCollection.get(e);
                        if (t) return this._cache.set(e, t), !0
                    }
                    return !1
                }
                async updateReturnEntity(e, t) {
                    if (await this._dbCollection.update(e, t)) {
                        const t = await this._dbCollection.get(e);
                        if (t) return this._cache.set(e, t), t
                    }
                    throw Error(`[update]: cannot update record with id: ${e}`)
                }
                async updateMulti(e) {
                    const t = await this._dbCollection.updateMulti(e);
                    return t.success.forEach((e => this._cache.set(e.id, e))), t
                }
                getAllSenderIds(e, t = !0) {
                    if (!e) throw Error(`[getAllSenderIds]: id: ${e} isn't valid!`);
                    return this._getItemListByField(e, "senderIds", t)
                }
                getAllFileTypes(e, t) {
                    if (!e) throw Error(`[getAllFileTypes]: id: ${e} isn't valid!`);
                    return this._getItemListByField(e, "fileTypes", t)
                }
                saveSender(e, t) {
                    if (!e) throw Error(`[saveSender]: id: ${e} isn't valid!`);
                    return this._saveValueOnField(e, "senderIds", t)
                }
                saveFileType(e, t) {
                    if (!e) throw Error(`[saveFileType]: id: ${e} isn't valid!`);
                    return this._saveValueOnField(e, "fileTypes", t)
                }
                saveSenders(e, t) {
                    if (!e) throw Error(`[saveSenders]: id: ${e} isn't valid!`);
                    return this._saveValuesOnField(e, "senderIds", t)
                }
                saveFileTypes(e, t) {
                    if (!e) throw Error(`[saveSenders]: id: ${e} isn't valid!`);
                    return this._saveValuesOnField(e, "fileTypes", t)
                }
                deleteSender(e, t) {
                    if (!e) throw Error(`[deleteSender]: id: ${e} isn't valid!`);
                    return this._deleteValueOnField(e, "senderIds", t)
                }
                deleteFileType(e, t) {
                    if (!e) throw Error(`[deleteFileType]: id: ${e} isn't valid!`);
                    return this._deleteValueOnField(e, "fileTypes", t)
                }
                deleteSenders(e, t) {
                    if (!e) throw Error(`[deleteSenders]: id: ${e} isn't valid!`);
                    return this._deleteValuesByField(e, "senderIds", t)
                }
                deleteFileTypes(e, t) {
                    if (!e) throw Error(`[deleteFileTypes]: id: ${e} isn't valid!`);
                    return this._deleteValuesByField(e, "fileTypes", t)
                }
                async _saveValueOnField(e, t, s) {
                    const i = this._cache.get(e) || await this._dbCollection.get(e);
                    if (i) {
                        i[t].some((e => e === s)) || (i[t].push(s), await this.update(e, {
                            value: {
                                [t]: i[t]
                            },
                            attributes: [t]
                        }))
                    }
                    return !0
                }
                async _saveValuesOnField(e, t, s) {
                    const i = this._cache.get(e) || await this._dbCollection.get(e);
                    if (i) {
                        const r = s.filter((e => !i[t].includes(e)));
                        r.length && (i[t].push(...r), await this.update(e, {
                            value: {
                                [t]: i[t]
                            },
                            attributes: [t]
                        }))
                    }
                    return !0
                }
                async _deleteValueOnField(e, t, s) {
                    const i = this._cache.get(e) || await this._dbCollection.get(e);
                    if (i) {
                        const r = i[t].indexOf(s); - 1 !== r && (i[t].splice(r, 1), await this.update(e, {
                            value: {
                                [t]: i[t]
                            },
                            attributes: [t]
                        }))
                    }
                    return !0
                }
                async _deleteValuesByField(e, t, s) {
                    const i = this._cache.get(e) || await this._dbCollection.get(e);
                    if (i) {
                        const r = i[t].filter((e => !s.includes(e)));
                        r.length < i[t].length && (i[t] = r, await this.update(e, {
                            value: {
                                [t]: r
                            },
                            attributes: [t]
                        }))
                    }
                    return !0
                }
                async _getItemListByField(e, t, s = !0) {
                    var i;
                    const r = this._cache.get(e);
                    var a;
                    if (s) return r && null !== (a = r[t]) && void 0 !== a && a.length ? r[t].slice() : [];
                    const n = await this._dbCollection.get(e);
                    return n && !r && this._cache.set(e, Object(p.a)({}, n)), n && null !== (i = n[t]) && void 0 !== i && i.length ? n[t].slice() : []
                }
            }) || N) || N) || N) || N);
            var B, z = s("SrDO"),
                x = s("fhp4");
            Object(r.injectable)()(B = Object(r.singleton)(z.b)(B = class {
                getMediaMapper(e) {
                    switch (e) {
                        case "image":
                            return r.ModuleContainer.resolve(x.c);
                        case "file":
                            return r.ModuleContainer.resolve(x.a);
                        case "link":
                            return r.ModuleContainer.resolve(x.d);
                        default:
                            return
                    }
                }
            }) || B);
            var $, J = s("3dEI");
            Object(r.injectable)()($ = Object(r.singleton)(J.b)($ = class {
                getMediaRepository(e) {
                    switch (e) {
                        case "image":
                            return r.ModuleContainer.resolve(E.b);
                        case "link":
                            return r.ModuleContainer.resolve(A.b);
                        case "file":
                            return r.ModuleContainer.resolve(j.b);
                        default:
                            return
                    }
                }
            }) || $);
            const G = "utils-media-domain-service",
                K = Object(r.define)(G);
            var W, V = s("5txd");
            Object(r.injectable)()(W = Object(r.singleton)(K)(W = function(e, t) {
                return Object(r.inject)(R.b)(e, void 0, 0)
            }(W = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 1)
            }(W = Reflect.metadata("design:type", Function)(W = Reflect.metadata("design:paramtypes", [void 0 === R.IUtilsMediaRepository ? Object : R.IUtilsMediaRepository, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(W = class {
                constructor(e, t) {
                    this._log = void 0, this._logger = void 0, this._utilsMediaRepository = void 0, this._utilsMediaRepository = e, this._logger = t.createZLogger(`${G}`, ["mn-utils-media-de"]), this._log = Object(V.b)(this._logger)
                }
                async create(e) {
                    if (!e) throw Error(`utilsMedia(${e}) isn't existed!`);
                    if (!e.convId || !e.mediaType) throw Error(`cannot create utilsMedia with convId(${e.convId}) and mediaType(${e.mediaType})`);
                    if (await this._utilsMediaRepository.isExisted(`${e.convId}_${e.mediaType}`)) throw this._log("[create]", `existed utilsMedia have convId(${e.convId}) and mediaType(${e.mediaType})`, "info"), Error(`existed utilsMedia have convId(${e.convId}) and mediaType(${e.mediaType})`);
                    return {
                        id: `${e.convId}_${e.mediaType}`,
                        convId: e.convId,
                        mediaType: e.mediaType,
                        senderIds: e.senderIds || [],
                        fileTypes: e.fileTypes || []
                    }
                }
                addSenders(e, t) {
                    if (!e) throw Error("[addFileTypes] utilsMediaEntity isn't existed!");
                    if (!t || !t.length) return Object(p.a)({}, e);
                    return Object(p.a)(Object(p.a)({}, e), {}, {
                        senderIds: t.reduce(((e, t) => (e.includes(t) || e.push(t), e)), [...e.senderIds] || !1)
                    })
                }
                addFileTypes(e, t) {
                    if (!e) throw Error("[addFileTypes] utilsMediaEntity isn't existed!");
                    if (!t || !t.length) return Object(p.a)({}, e);
                    const s = Object(p.a)(Object(p.a)({}, e), {}, {
                        fileTypes: t.reduce(((e, t) => (e.includes(t) || e.push(t), e)), [...e.fileTypes] || !1)
                    });
                    return s
                }
            }) || W) || W) || W) || W) || W);
            var H, Z = s("mvkY"),
                Y = s("jbAT");
            Object(r.injectable)()(H = Object(r.singleton)(Y.b)(H = function(e, t) {
                return Object(r.inject)(K)(e, void 0, 0)
            }(H = function(e, t) {
                return Object(r.inject)(R.b)(e, void 0, 1)
            }(H = function(e, t) {
                return Object(r.inject)(d)(e, void 0, 2)
            }(H = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 3)
            }(H = Reflect.metadata("design:type", Function)(H = Reflect.metadata("design:paramtypes", [Object, void 0 === R.IUtilsMediaRepository ? Object : R.IUtilsMediaRepository, Object, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(H = class {
                constructor(e, t, s, i) {
                    this._utilsMediaDomainService = void 0, this._utilsMediaRepository = void 0, this._mapper = void 0, this._logger = void 0, this._log = void 0, this._utilsMediaDomainService = e, this._utilsMediaRepository = t, this._mapper = s, this._logger = i.createZLogger(Z.b, ["manage-utils-media-in-app"]), this._log = Object(V.b)(this._logger)
                }
                async create(e) {
                    try {
                        const t = await this._utilsMediaDomainService.create(e);
                        return this._mapper.toUtilsMediaDTOFromDomain(await this._utilsMediaRepository.insert(t))
                    } catch (t) {
                        return void this._log("[create]", t.message)
                    }
                }
                async createOrUpdate(e) {
                    try {
                        const t = `${e.convId}_${e.mediaType}`;
                        let s = await this._utilsMediaRepository.get(t);
                        if (s) {
                            const {
                                senderIds: i,
                                fileTypes: r
                            } = e;
                            let a = s.senderIds,
                                n = s.fileTypes;
                            if (i && i.length && (a = this._utilsMediaDomainService.addSenders(s, i).senderIds), r && r.length && (n = this._utilsMediaDomainService.addFileTypes(s, r).fileTypes), a.length !== s.senderIds.length || n.length !== s.fileTypes.length) {
                                const e = await this._utilsMediaRepository.updateReturnEntity(t, {
                                    value: {
                                        senderIds: a,
                                        fileTypes: n
                                    },
                                    attributes: ["senderIds", "fileTypes"]
                                });
                                return this._mapper.toUtilsMediaDTOFromDomain(e)
                            }
                            return this._mapper.toUtilsMediaDTOFromDomain(s)
                        }
                        return this.create(e)
                    } catch (t) {
                        return void this._log("[createOrUpdate]", t.message)
                    }
                }
                async deleteUtilsMediasByConvId(e) {
                    try {
                        if (!e) return [];
                        const t = Object.values(f.e).map((t => `${e}_${t}`));
                        return (await this._utilsMediaRepository.deleteMulti(t)).success
                    } catch (t) {
                        return this._log("[deleteUtilsMediasByConvId]", t.message), []
                    }
                }
                async createOrUpdateFromMedias(e) {
                    try {
                        if (!e || !e.length) return [];
                        const t = new Map;
                        e.forEach((e => {
                            let s = t.get(e.convId);
                            if ("image" === e.mediaType || "link" === e.mediaType)
                                if (s) {
                                    -1 === s.senderIds.indexOf(e.fromUid) && (s.senderIds.push(e.fromUid), t.set(e.convId, s))
                                } else s = {
                                    convId: e.convId,
                                    mediaType: e.mediaType,
                                    senderIds: [e.fromUid],
                                    fileTypes: []
                                }, t.set(e.convId, s);
                            else if ("file" === e.mediaType) {
                                const i = this._getFileType(e.content.params);
                                if (s) {
                                    -1 === s.fileTypes.indexOf(i) && (s.fileTypes.push(i), t.set(e.convId, s))
                                } else s = {
                                    convId: e.convId,
                                    mediaType: e.mediaType,
                                    senderIds: [e.fromUid],
                                    fileTypes: [i]
                                }, t.set(e.convId, s)
                            }
                        }));
                        const s = Array.from(t.values());
                        return (await Promise.all(s.map((e => this.createOrUpdate(e))))).filter(Boolean)
                    } catch (t) {
                        return this._log("[createOrUpdateFromMedias]", t.message), []
                    }
                }
                _getFileType(e) {
                    try {
                        const t = JSON.parse(e);
                        if (!t) throw Error(`paramsObj is ${t}!`);
                        const {
                            fileExt: s,
                            fType: i
                        } = t;
                        return "zip" === s && 2 === i ? "folder" : s.toLowerCase()
                    } catch (t) {
                        return this._log("[_getFileType]", t.message), ""
                    }
                }
            }) || H) || H) || H) || H) || H) || H) || H);
            var X = s("igA5");
            const q = Object(r.define)("media-migration-state-persist");
            var Q, ee = s("8tev");
            Object(r.injectable)()(Q = Object(r.singleton)(q)(Q = class {
                constructor() {
                    this._localStorage = X.default.getInstance()
                }
                async saveMigrationState(e) {
                    e && await this._localStorage.setItemForCurrentUserAsync(ee.c.MIGRATION_STATE_KEY, JSON.stringify(e))
                }
                async getMigrationState() {
                    try {
                        const e = await this._localStorage.getItemForCurrentUserAsync(ee.c.MIGRATION_STATE_KEY);
                        return e ? JSON.parse(e) : {
                            stateName: ee.b.UNKNOWN,
                            mediaTableNamesToMigrate: []
                        }
                    } catch (e) {
                        return {
                            stateName: ee.b.UNKNOWN,
                            mediaTableNamesToMigrate: []
                        }
                    }
                }
                async saveJobDescSummaries(e) {
                    Array.isArray(e) && await this._localStorage.setItemForCurrentUserAsync(ee.c.PERSISTED_JOB_DESC_SUMMARIES_KEY, JSON.stringify(e))
                }
                async getSavedJobDescSummaries() {
                    try {
                        const e = await this._localStorage.getItemForCurrentUserAsync(ee.c.PERSISTED_JOB_DESC_SUMMARIES_KEY);
                        if (!e) return [];
                        return JSON.parse(e)
                    } catch (e) {
                        return []
                    }
                }
                async clearPersistedJobDescSummaries() {
                    await this._localStorage.removeItemForCurrentUserAsync(ee.c.PERSISTED_JOB_DESC_SUMMARIES_KEY)
                }
            }) || Q);
            var te = s("8/YW"),
                se = s("mH7l"),
                ie = s.n(se);
            const re = e => "GET_FROM_OLD_DB" === e.name,
                ae = e => "ADD_TO_NEW_DB" === e.name,
                ne = e => "DELETE_FROM_OLD_DB" === e.name;
            var oe = s("1pet"),
                de = s("ZsEe"),
                le = s("EYv5"),
                ce = s("NDmK");

            function me() {
                return ce.default.media_migration_db.should_stop_migration
            }

            function ue() {
                return ce.default.media_migration_db.max_running_concurrency_job
            }

            function ge() {
                return ce.default.media_migration_db.delay_time_per_job
            }

            function he() {
                return ce.default.media_migration_db.delay_time_each_k_jobs
            }

            function pe() {
                return ce.default.media_migration_db.media_limit_per_job
            }

            function fe() {
                return ce.default.media_migration_db.min_retry_after
            }

            function Me() {
                return ce.default.media_migration_db.max_retry_after
            }
            var _e = s("1erv");
            const be = {
                MAX_RETRY: 0,
                INVALID_CONFIG_JITTER: 1,
                INVALID_CONFIG_MIN: 2,
                TIMEOUT: 3,
                RETRY_ON_ERROR_CONDITION_FAIL: 4
            };
            class ye extends Error {
                constructor(e) {
                    super(`ExpBackoff error:${e}`), this.code = e
                }
            }

            function Ie({
                step: e,
                nTry: t,
                jitter: s,
                min: i,
                max: r
            }) {
                let a = i * Math.pow(e, t);
                if (s) {
                    const e = Math.random(),
                        t = Math.floor(e * s * a);
                    a = 1 & Math.floor(10 * e) ? a + t : a - t
                }
                return 0 | Math.min(a, r)
            }

            function ve(e, t) {
                return async function(...s) {
                    for await (const {
                            sleep: r,
                            duration: a,
                            nTry: n
                        }
                        of
                        function*(e) {
                            var t, s, i, r, a;
                            const n = null !== (t = null == e ? void 0 : e.retries) && void 0 !== t ? t : 3,
                                o = null !== (s = null == e ? void 0 : e.min) && void 0 !== s ? s : 100,
                                d = null !== (i = null == e ? void 0 : e.max) && void 0 !== i ? i : 1e4,
                                l = null !== (r = null == e ? void 0 : e.step) && void 0 !== r ? r : 2,
                                c = null !== (a = null == e ? void 0 : e.jitter) && void 0 !== a ? a : 0;
                            if (o <= 0) throw new ye(be.INVALID_CONFIG_MIN);
                            if (c < 0 || c > 1) throw new ye(be.INVALID_CONFIG_JITTER);
                            let m = 0;
                            for (; m <= n;) {
                                const e = Ie({
                                    nTry: m,
                                    step: l,
                                    jitter: c,
                                    min: o,
                                    max: d
                                });
                                yield {
                                    nTry: m,
                                    duration: e,
                                    sleep: () => new Promise((t => {
                                        setTimeout(t, e)
                                    }))
                                }, m++
                            }
                        }(t)) try {
                        const i = await e(...s);
                        return null != t && t.afterRetry && (null == t || t.afterRetry({
                            nTry: n,
                            duration: a,
                            sleep: r,
                            success: !0,
                            shouldRetry: !1
                        })), i
                    } catch (i) {
                        const e = null != t && t.shouldRetryOnError ? null == t ? void 0 : t.shouldRetryOnError : () => Promise.resolve(!0),
                            o = await e({
                                error: i,
                                currentState: {
                                    nTry: n,
                                    duration: a,
                                    sleep: r
                                },
                                input: s
                            });
                        if (null != t && t.afterRetry && (null == t || t.afterRetry({
                                nTry: n,
                                duration: a,
                                sleep: r,
                                shouldRetry: o,
                                success: !1
                            })), !o) throw new ye(be.RETRY_ON_ERROR_CONDITION_FAIL);
                        await r()
                    }
                    throw new ye(be.MAX_RETRY)
                }
            }
            var Oe;
            const De = new de.a,
                Te = 999999,
                we = 1 / 0;
            Object(te.d)()(Oe = Object(r.injectable)()(Oe = Object(r.singleton)(C.c)(Oe = function(e, t) {
                return Object(r.inject)(q)(e, void 0, 0)
            }(Oe = function(e, t) {
                return Object(r.inject)(te.a)(e, void 0, 1)
            }(Oe = function(e, t) {
                return Object(r.inject)(S.ZLoggerFactory)(e, void 0, 2)
            }(Oe = Reflect.metadata("design:type", Function)(Oe = Reflect.metadata("design:paramtypes", [Object, void 0 === te.a ? Object : te.a, void 0 === S.ZLoggerFactory ? Object : S.ZLoggerFactory])(Oe = class {
                constructor(e, t, s) {
                    this._isLoadData = !1, this._forceStop = !1, this._mediaMigrationStatePersist = void 0, this._maxWorkerPoolNum = ue(), this._worker = new ie.a({
                        concurrency: this._maxWorkerPoolNum,
                        autoStart: !1
                    }), this._delayTimePerJob = ge(), this._delayTimeEachKJobs = he(), this._mediaNumLimitPerJob = pe(), this._minRetryAfter = fe(), this._maxRetryAfter = Me(), this._currentJobDescs = [], this._currentJobHolders = [], this._currentMsgIdCursor = oe.MessageConstants.MAX_MSG_ID, this._mediaMigrationState = void 0, this._failedGetJob = !1, this._failedAddJob = !1, this._failedDeleteJob = !1, this._hasDBError = !1, this._logger = void 0, this._application = void 0, this._mediaMigrationStatePersist = e, this._onApplicationIdle = this._onApplicationIdle.bind(this), this._onApplicationActive = this._onApplicationActive.bind(this), this._onApplicationExit = this._onApplicationExit.bind(this), this._application = t, this._logger = s.createZLogger(C.b, ["manage-media-migration"])
                }
                onApplicationReady(e) {
                    this._listenEvents(), this._isLoadData || this._loadData().then((() => {
                        this._runMigration()
                    })), this.toggleDBError(this._getTestMediaMigrationFlag("[mm]:db_error")), this.toggleGetJobFailed(this._getTestMediaMigrationFlag("[mm]:get_failed")), this.toggleAddJobFailed(this._getTestMediaMigrationFlag("[mm]:add_failed")), this.toggleDeleteJobFailed(this._getTestMediaMigrationFlag("[mm]:delete_failed"))
                }
                async getMediaMigrationState() {
                    if (!Object(_e.a)() || me()) return {
                        stateName: ee.b.UNKNOWN,
                        mediaTableNamesToMigrate: []
                    };
                    if (!this._mediaMigrationState) {
                        const {
                            stateName: e,
                            remainingTableNames: t
                        } = await this._statsMediaMigration();
                        this._mediaMigrationState = {
                            stateName: e,
                            mediaTableNamesToMigrate: t
                        }
                    }
                    return this._mediaMigrationState
                }
                pauseMigration(e) {
                    this._logger.zsymb(3, "7Iu9Nv", ["[mm] pause migration - reason: {}", "OAYEPd"], e), this.flushHoldingJobs(), this._forceStop = !0, this._worker.pause()
                }
                async resumeMigration(e) {
                    this._logger.zsymb(3, "EgwLBP", ["[mm] resume migration - reason: {}", "5JhI3E"], e), this._forceStop = !1, Object(_e.a)() && !me() && (this._isLoadData || await this._loadData(), this._currentJobDescs.forEach((e => {
                        if (e.state === ee.a.FAILED) {
                            e.state = ee.a.NEW;
                            const t = this._createJob(e);
                            if (t) {
                                const s = this._scheduleJob(e.id, t);
                                this._worker.add(s, {
                                    priority: Te
                                })
                            }
                        }
                    })), this._worker.size > 0 ? this._worker.isPaused && this._worker.start() : this._runMigration())
                }
                flushHoldingJobs() {
                    this._currentJobHolders.forEach((e => {
                        clearTimeout(e.timeoutId), e.resolver()
                    })), this._currentJobHolders = []
                }
                toggleGetJobFailed(e) {
                    this._failedGetJob = e
                }
                toggleAddJobFailed(e) {
                    this._failedAddJob = e
                }
                toggleDeleteJobFailed(e) {
                    this._failedDeleteJob = e
                }
                toggleDBError(e) {
                    this._hasDBError = e
                }
                _createJob(e) {
                    const {
                        id: t,
                        currentStep: s
                    } = e;
                    return this._updateStateOfCurrentJobByJobId(t, ee.a.RUNNING), re(s) ? this._createJobFromGetFromOldDBCurrentStep(t, s) : ae(s) ? this._createJobFromAddToNewDBCurrentStep(t, s) : ne(s) ? this._createJobFromDeleteFromOldDBCurrentStep(t, s) : void 0
                }
                _createJobFromGetFromOldDBCurrentStep(e, t) {
                    const s = Object(p.a)({}, t),
                        {
                            mediaTableName: i,
                            amount: r,
                            lastMsgId: a
                        } = s.dataToRun;
                    return async () => {
                        try {
                            await this._withRetry((async () => {
                                const t = this._createStep(s);
                                if (!t) return void this._logger.zsymb(21, "sigVGj", ["[mm] getFromOldDBStep is undefined!", "NcUV9l"]);
                                let n = await t();
                                if (n.state === ee.e.FAILED) throw n.data.jobId = e, n.data.jobType = "GET_FROM_OLD_DB", n.data;
                                const {
                                    data: o
                                } = n;
                                if (this._logger.zsymb(3, "4whHLy", ["[mm] getFromOldDBStep - DONE - jobId: {}, {}", "VclcPO"], e, o.length), o.length >= r ? this._currentMsgIdCursor = o[o.length - 1].msgId : this._mediaMigrationState && this._mediaMigrationState.mediaTableNamesToMigrate.length && (this._mediaMigrationState.mediaTableNamesToMigrate.shift(), this._currentMsgIdCursor = oe.MessageConstants.MAX_MSG_ID), this._runMigration(), o.length) {
                                    const t = {
                                        name: ee.d.ADD_TO_NEW_DB,
                                        dataToRun: {
                                            mediaTableName: i,
                                            amount: r,
                                            lastMsgId: a,
                                            data: o
                                        }
                                    };
                                    this._updateCurrentStepByJobId(e, t);
                                    const s = this._createJobFromAddToNewDBCurrentStep(e, t);
                                    await s()
                                }
                            }))()
                        } catch (t) {
                            this._logger.zsymb(3, "qPcmDQ", ["[mm] throw error from get from old DB step - jobId: {}, {}", "YLlAgb"], e, null == t ? void 0 : t.message)
                        }
                    }
                }
                _createJobFromAddToNewDBCurrentStep(e, t) {
                    const s = Object(p.a)({}, t),
                        {
                            mediaTableName: i,
                            amount: r,
                            lastMsgId: a
                        } = s.dataToRun;
                    return async () => {
                        try {
                            await this._withRetry((async () => {
                                await this._persistCurrentJobDescs();
                                const s = this._createStep(t);
                                if (!s) return void this._logger.zsymb(18, "qxvWwo", "[mm] addToNewDBStep is undefined!");
                                const n = await s();
                                if (n.state === ee.e.FAILED) throw n.data.jobId = e, n.data.jobType = "ADD_TO_NEW_DB", n.data;
                                const {
                                    data: o
                                } = n;
                                if (this._logger.zsymb(3, "xC24Ml", ["[mm] addToNewDBStep - jobId: {}, {}", "hCbC7F"], e, o.length), o.length) {
                                    const t = {
                                        name: ee.d.DELETE_FROM_OLD_DB,
                                        dataToRun: {
                                            mediaTableName: i,
                                            amount: r,
                                            lastMsgId: a,
                                            data: o
                                        }
                                    };
                                    this._updateCurrentStepByJobId(e, t);
                                    const s = this._createJobFromDeleteFromOldDBCurrentStep(e, t);
                                    await s()
                                }
                            }))()
                        } catch (s) {
                            this._logger.zsymb(3, "hgbwIT", ["[mm] throw error from add to new DB step - jobId: {}, {}", "_b_-ks"], e, null == s ? void 0 : s.message)
                        }
                    }
                }
                _createJobFromDeleteFromOldDBCurrentStep(e, t) {
                    const s = Object(p.a)({}, t);
                    return async () => {
                        try {
                            await this._withRetry((async () => {
                                await this._persistCurrentJobDescs();
                                const t = this._createStep(s);
                                if (!t) return void this._logger.zsymb(21, "CvlEDz", ["[mm] deleteFromOldDBStep is undefined!", "AnYPgZ"]);
                                const i = await t();
                                if (i.state === ee.e.FAILED) throw i.data.jobId = e, i.data.jobType = "DELETE_FROM_OLD_DB", i.data;
                                this._logger.zsymb(3, "w_c1WD", ["[mm] deleteFromOldDBStep - DONE - jobId: {}", "EB3MVF"], e), this._currentJobDescs = this._currentJobDescs.filter((t => t.id !== e)), this._currentJobHolders = this._currentJobHolders.filter((t => t.jobId !== e)), this._persistCurrentJobDescs()
                            }))()
                        } catch (t) {
                            this._logger.zsymb(3, "NV6mfq", ["[mm] throw error from delete from old DB step - jobId: {} - err: {}", "dx5bPM"], e, null == t ? void 0 : t.message)
                        }
                    }
                }
                _updateCurrentStepByJobId(e, t) {
                    const s = this._currentJobDescs.find((t => t.id === e));
                    s && (s.currentStep = t)
                }
                _updateStateOfCurrentJobByJobId(e, t) {
                    const s = this._currentJobDescs.find((t => t.id === e));
                    s && (s.state = t)
                }
                async _persistCurrentJobDescs() {
                    await this._mediaMigrationStatePersist.saveJobDescSummaries(this._currentJobDescs.reduce(((e, t) => (t.currentStep.name !== ee.d.GET_FROM_OLD_DB && e.push(this._toJobDescSummary(t)), e)), []))
                }
                _createStep(e) {
                    return re(e) ? async () => {
                        try {
                            if (this._hasDBError) this._throwDBError();
                            else if (this._failedGetJob) throw Error("[SIMULATE]: Failed to get job from old DB!");
                            const {
                                mediaTableName: t,
                                amount: s,
                                lastMsgId: i
                            } = e.dataToRun, a = t, n = r.ModuleContainer.resolve(le.a), o = await n.getMediasFromOldDB(a, s, i);
                            return {
                                state: ee.e.DONE,
                                data: o
                            }
                        } catch (t) {
                            return this._logger.zsymb(21, "xkaTcg", ["[mm] get from old db step err: {}", "MGXT_n"], null == t ? void 0 : t.message), {
                                state: ee.e.FAILED,
                                data: t
                            }
                        }
                    }: ae(e) ? async () => {
                        try {
                            if (this._hasDBError) this._throwDBError();
                            else if (this._failedAddJob) throw Error("[SIMULATE]: Failed to add job to new DB!");
                            let {
                                mediaTableName: t,
                                data: s,
                                lastMsgId: i,
                                amount: a
                            } = e.dataToRun;
                            const n = t,
                                o = r.ModuleContainer.resolve(le.a);
                            let d = s;
                            d || (d = await o.getMediasFromOldDB(n, a, i));
                            const l = await o.addMediasFromOldDB(n, d);
                            return {
                                state: ee.e.DONE,
                                data: l
                            }
                        } catch (t) {
                            return this._logger.zsymb(21, "CbOHrY", ["[mm] add to new db step err: {}", "n11DlX"], null == t ? void 0 : t.message), {
                                state: ee.e.FAILED,
                                data: t
                            }
                        }
                    }: ne(e) ? async () => {
                        try {
                            if (this._hasDBError) this._throwDBError();
                            else if (this._failedDeleteJob) throw Error("[SIMULATE]: Failed to delete job from old DB!");
                            const {
                                mediaTableName: t,
                                data: s,
                                lastMsgId: i,
                                amount: a
                            } = e.dataToRun, n = t, o = r.ModuleContainer.resolve(le.a);
                            return s ? {
                                state: ee.e.DONE,
                                data: await o.deleteMediasFromOldDB(n, s)
                            } : {
                                state: ee.e.DONE,
                                data: await o.deleteMediasFromOldDBByRange(n, a, i)
                            }
                        } catch (t) {
                            return this._logger.zsymb(21, "W3eCo1", ["[mm] delete from old db step err: {}", "AZ4o9s"], null == t ? void 0 : t.message), {
                                state: ee.e.FAILED,
                                data: t
                            }
                        }
                    }: void 0
                }
                async _loadData() {
                    if (me() || !Object(_e.a)()) return;
                    const e = await this.getMediaMigrationState();
                    this._logger.zsymb(3, "eKiHTp", ["[mm] MEDIA MIGRATION STATE after load data: {}", "nyIrFu"], e.stateName), this._mediaMigrationState = e;
                    const t = await this._mediaMigrationStatePersist.getSavedJobDescSummaries();
                    t.length > 0 && t.forEach((e => {
                        const t = this._toJobDesc(e);
                        this._currentJobDescs.push(t);
                        const s = this._createJob(t);
                        if (s) {
                            const e = this._scheduleJob(t.id, s);
                            this._worker.add(e, {
                                priority: Te
                            })
                        }
                    })), this._isLoadData = !0
                }
                async _statsMediaMigration() {
                    const e = r.ModuleContainer.resolve(le.a),
                        t = await Promise.all([e.countTotalMediaInOldDB("image"), e.countTotalMediaInOldDB("file"), e.countTotalMediaInOldDB("link")]);
                    let s = ee.b.NOT_DONE,
                        i = [];
                    return s = t.reduce(((e, t, s) => (0 === s && t > 0 ? i.push("image") : 1 === s && t > 0 ? i.push("file") : 2 === s && t > 0 && i.push("link"), e + t)), 0) > 0 ? ee.b.NOT_DONE : ee.b.DONE, {
                        stateName: s,
                        remainingTableNames: i
                    }
                }
                async _runMigration() {
                    if (this._logger.zsymb(0, "EYvfMF", "[mm] ====================RUN MIGRATION===================="), !Object(_e.a)()) return void this._logger.zsymb(0, "TNywOB", "[mm] ==========STOP MIGRATION: not use new media DB flow (cfsv)==========");
                    if (me()) return void this._logger.zsymb(0, "4M3ykc", "[mm] ==========STOP MIGRATION: stop migration (cfsv)==========");
                    if (this._forceStop) return void this._logger.zsymb(0, "0iZVaH", "[mm] ==========STOP MIGRATION: force stop (in app)==========");
                    if (!this._isLoadData && (await this._loadData(), !this._isLoadData)) return;
                    if (this._mediaMigrationState.stateName === ee.b.DONE) return void this._logger.zsymb(0, "rAJDh2", "[mm] ==========STOP MIGRATION: migration done==========");
                    const e = this._mediaMigrationState.mediaTableNamesToMigrate.length > 0 ? this._mediaMigrationState.mediaTableNamesToMigrate[0] : "";
                    if (!e) {
                        if (!this._currentJobDescs.length) return this._logger.zsymb(0, "j3U7yT", "[mm] ==========STOP MIGRATION: no media table to migrate=========="), this._stopListenEvents(), void(await this._mediaMigrationStatePersist.clearPersistedJobDescSummaries());
                        this._logger.zsymb(0, "_J9T0I", "[mm] ==========STOP MIGRATION: no media table to migrate, but still have running jobs==========")
                    }
                    if (this._logger.zsymb(3, "3ijgq6", ["[mm] worker pending: ", "T1-smP"], this._worker.pending), this._logger.zsymb(3, "MPi6_2", ["[mm] worker size: ", "t-ACpd"], this._worker.size), e) {
                        if (De.value % this._maxWorkerPoolNum == 0 && (await this._waitIn(this._delayTimeEachKJobs), this._forceStop || !Object(_e.a)() || me())) return;
                        const t = {
                                id: De.next(),
                                state: ee.a.NEW,
                                currentStep: {
                                    name: ee.d.GET_FROM_OLD_DB,
                                    dataToRun: {
                                        mediaTableName: e,
                                        amount: this._mediaNumLimitPerJob,
                                        lastMsgId: this._currentMsgIdCursor
                                    }
                                }
                            },
                            s = this._createJob(t);
                        if (s) {
                            this._currentJobDescs.push(t);
                            const e = this._scheduleJob(t.id, s);
                            this._worker.add(e)
                        }
                    } else if (!this._currentJobDescs.length) return;
                    this._worker.isPaused && this._worker.start()
                }
                _listenEvents() {
                    this._application.addEventListener(te.b.Idle, this._onApplicationIdle), this._application.addEventListener(te.b.Active, this._onApplicationActive), this._application.addEventListener(te.b.Exit, this._onApplicationExit)
                }
                _stopListenEvents() {
                    this._application.removeEventListener(te.b.Idle, this._onApplicationIdle), this._application.removeEventListener(te.b.Active, this._onApplicationActive), this._application.removeEventListener(te.b.Exit, this._onApplicationExit)
                }
                _onApplicationIdle() {
                    this._setupMigrationConfig({
                        maxWorkerPoolNum: ue(),
                        mediaNumLimitPerJob: pe(),
                        delayTimePerJob: ge(),
                        delayTimeEachKJobs: he(),
                        minRetryAfter: fe(),
                        maxRetryAfter: Me()
                    })
                }
                _onApplicationActive() {
                    this._setupMigrationConfig({
                        maxWorkerPoolNum: ue(),
                        mediaNumLimitPerJob: pe(),
                        delayTimePerJob: ge(),
                        delayTimeEachKJobs: he(),
                        minRetryAfter: fe(),
                        maxRetryAfter: Me()
                    })
                }
                _onApplicationExit() {
                    this.pauseMigration("EXIT_APP")
                }
                _toJobDescSummary(e) {
                    const {
                        currentStep: t
                    } = e;
                    return {
                        currentStep: {
                            name: t.name,
                            summaryData: {
                                mediaTableName: t.dataToRun.mediaTableName,
                                amount: t.dataToRun.amount,
                                lastMsgId: t.dataToRun.lastMsgId
                            }
                        }
                    }
                }
                _toJobDesc(e) {
                    let {
                        currentStep: t
                    } = e;
                    return {
                        id: De.next(),
                        state: ee.a.NEW,
                        currentStep: {
                            name: t.name,
                            dataToRun: Object(p.a)(Object(p.a)({}, t.summaryData), {}, {
                                data: void 0
                            })
                        }
                    }
                }
                _setupMigrationConfig(e) {
                    "number" == typeof e.maxWorkerPoolNum && (this._worker.concurrency = e.maxWorkerPoolNum), "number" == typeof e.mediaNumLimitPerJob && (this._mediaNumLimitPerJob = e.mediaNumLimitPerJob), "number" == typeof e.delayTimePerJob && (this._delayTimePerJob = e.delayTimePerJob), "number" == typeof e.delayTimeEachKJobs && (this._delayTimeEachKJobs = e.delayTimeEachKJobs), "number" == typeof e.minRetryAfter && (this._minRetryAfter = e.minRetryAfter), "number" == typeof e.maxRetryAfter && (this._maxRetryAfter = e.maxRetryAfter)
                }
                _scheduleJob(e, t) {
                    return async () => new Promise((t => {
                        const s = setTimeout((() => {
                            this._currentJobHolders = this._currentJobHolders.filter((t => t.jobId !== e)), t()
                        }), 1e3 * this._delayTimePerJob);
                        this._currentJobHolders.push({
                            jobId: e,
                            timeoutId: s,
                            resolver: t
                        })
                    })).then((() => t()))
                }
                _withRetry(e) {
                    return ve(e, {
                        retries: we,
                        min: 1e3 * this._minRetryAfter,
                        max: 1e3 * this._maxRetryAfter,
                        shouldRetryOnError: async ({
                            error: e,
                            currentState: t
                        }) => this._forceStop || me() || !Object(_e.a)() ? (this._updateStateOfCurrentJobByJobId(e.jobId, ee.a.FAILED), !1) : "UnknownError" === e.name || "QuotaExceededError" === e.name || 22 === e.code ? (this._updateStateOfCurrentJobByJobId(e.jobId, ee.a.FAILED), this.pauseMigration("DB_ERROR"), !1) : (this._logger.zsymb(21, "RSDYhe", ["[mm]: Retry - jobId: {} - jobType: {} - error: {}", "RR75Cd"], e.jobId, e.jobType, e.message), !0)
                    })
                }
                async _waitIn(e) {
                    return this._logger.zsymb(3, "cEl11q", ["[mm]: waiting in {} seconds", "O3mRYq"], e), new Promise((t => {
                        setTimeout(t, 1e3 * e)
                    }))
                }
                _getTestMediaMigrationFlag(e) {
                    const t = X.default.getInstance().getItemForCurrentUser(e);
                    try {
                        return !!t && JSON.parse(t)
                    } catch (s) {
                        return !1
                    }
                }
                _throwDBError() {
                    const e = new Error("[SIMULATE]: DB is corrupted or quota is exceeded!");
                    throw e.name = "UnknownError", e.code = 22, e
                }
            }) || Oe) || Oe) || Oe) || Oe) || Oe) || Oe) || Oe)
        },
        t5Xg: function(e, t, s) {
            "use strict";
            s.d(t, "a", (function() {
                return r
            }));
            var i = s("jDHv");
            const r = Object(i.define)("zalo-temp-new-pos-action-log")
        },
        yVfy: function(e, t, s) {
            "use strict";
            s.d(t, "b", (function() {
                return i
            })), s.d(t, "c", (function() {
                return r
            })), s.d(t, "a", (function() {
                return a
            }));
            var i = {};
            s.r(i), s.d(i, "logCleanupSuccess", (function() {
                return g
            })), s.d(i, "logCleanupFail", (function() {
                return h
            })), s.d(i, "logCleanupPerf", (function() {
                return p
            })), s.d(i, "logCleanupSuccessDetail", (function() {
                return f
            })), s.d(i, "logCleanupInterrupted", (function() {
                return M
            })), s.d(i, "logCleanupResetRun", (function() {
                return _
            }));
            var r = {};
            s.r(r), s.d(r, "logSnapshotSuccessDetail", (function() {
                return b
            }));
            var a = {};
            s.r(a), s.d(a, "startScanReport", (function() {
                return E
            })), s.d(a, "completeCleanupReport", (function() {
                return S
            }));
            var n = s("4prX");

            function o(e, t, s = 1) {
                let i = 0;
                for (let r = 0; r < t.length; r++) {
                    if (e < t[r] * s) {
                        i = r;
                        break
                    }
                    if (r === t.length - 1) {
                        i = r + 1;
                        break
                    }
                }
                return i
            }
            var d = s("7+QW"),
                l = s("Tlqd");
            const c = [100, 500, 1024, 2048, 5120, 10240, 20480],
                m = [5, 10, 30, 60, 300, 600, 1440],
                u = {
                    [l.c.TAKE_SNAPSHOT]: 0,
                    [l.c.SCAN_MESSAGES]: 1,
                    [l.c.DELETE_RESOURCES]: 2
                };

            function g(e, t) {
                n.default.increaseFailed(152500, 0, t, o(e, c, 1e6), 0), n.default.increaseFailed(152505, 0, 0, o(t, m, 6e4), 0)
            }

            function h(e, t) {
                const s = u[e];
                "number" == typeof s && (n.default.increaseFailed(152501, 0, t, s, 0), n.default.increaseFailed(152505, 1, 0, o(t, m, 6e4), 0))
            }

            function p(e, t, s) {
                const i = u[e];
                "number" == typeof i && n.default.increaseFailed(152502, i, t, s ? 0 : 1, 0)
            }

            function f(e, t) {
                Object.entries(t).forEach((([t, {
                    size: s
                }]) => {
                    const i = d.a[t];
                    "number" == typeof i && n.default.increaseFailed(152504, i, Math.round(e / 1e6), o(s, c, 1e6), 0)
                }))
            }

            function M(e, t) {
                n.default.increaseFailed(152506, 0, t, e, 0)
            }

            function _(e) {
                n.default.increaseFailed(152507, 0, 0, "forced" === e ? 1 : 0, 0)
            }

            function b(e, t) {
                Object.entries(t).forEach((([t, {
                    size: s
                }]) => {
                    const i = d.a[t];
                    "number" == typeof i && n.default.increaseFailed(152503, i, Math.round(e / 1e6), o(s, c, 1e6), 0)
                }))
            }
            var y = s("Uzj0"),
                I = s("IpzU"),
                v = s("Yi2m"),
                O = s("bUXd");

            function D(e) {
                return Number((e / 1e9).toFixed(2))
            }
            async function T(e) {
                try {
                    const t = await Object(I.getZaloDir)();
                    e(D((await $znode.diskSpace(t)).free))
                } catch {
                    e(-1)
                }
            }

            function w(e, t) {
                return Math.max(e - t, 0)
            }

            function E(e) {
                const {
                    sessionId: t,
                    triggerType: s
                } = e;
                T((e => {
                    v.default.logActionInfoV2(v.FeaturesInfo.RedundantResourceCleanup, "ACOR_start_scan", {
                        device_id: v.default.getDeviceID(),
                        timestamp: O.default.getTimeNow(),
                        clean_up_session_id: t,
                        trigger_event: s,
                        free_space_at_start_scan_gb: e
                    })
                }))
            }

            function S(e) {
                const {
                    sessionId: t,
                    triggerType: s,
                    interrupt: i,
                    duration: r,
                    statsAfter: a,
                    statsBefore: n,
                    statsFailure: o,
                    statsDetailAfter: c,
                    statsDetailBefore: m,
                    statsAccessTime: u
                } = e, g = {
                    files_count: 0,
                    photos_count: 0,
                    videos_count: 0,
                    voices_count: 0,
                    other_count: 0,
                    files_size_gb: 0,
                    photos_size_gb: 0,
                    videos_size_gb: 0,
                    voices_size_gb: 0,
                    other_size_gb: 0
                };
                for (const l in m)
                    if (c[l]) {
                        const e = w(m[l].count, c[l].count),
                            t = D(w(m[l].size, c[l].size));
                        if (d.b.file.includes(l)) {
                            g.files_count += e, g.files_size_gb += t;
                            continue
                        }
                        if (d.b.photo.includes(l)) {
                            g.photos_count += e, g.photos_size_gb += t;
                            continue
                        }
                        if (d.b.video.includes(l)) {
                            g.videos_count += e, g.videos_size_gb += t;
                            continue
                        }
                        if (d.b.voice.includes(l)) {
                            g.voices_count += e, g.voices_size_gb += t;
                            continue
                        }
                        if (d.b.other.includes(l)) {
                            g.other_count += e, g.other_size_gb += t;
                            continue
                        }
                    } T((d => {
                    ! function(e) {
                        const {
                            sessionId: t,
                            triggerType: s,
                            duration: i,
                            interrupt: r
                        } = e;
                        v.default.logActionInfoV2(v.FeaturesInfo.RedundantResourceCleanup, "ACOR_scan_completed", {
                            device_id: v.default.getDeviceID(),
                            timestamp: O.default.getTimeNow(),
                            clean_up_session_id: t,
                            trigger_event: s,
                            scan_duration_s: Math.round((i[l.c.SCAN_MESSAGES] || 0) / 1e3),
                            snapshot_duration_s: Math.round((i[l.c.TAKE_SNAPSHOT] || 0) / 1e3),
                            interupted_scan: r[l.c.SCAN_MESSAGES] || 0
                        })
                    }(e), u ? v.default.logActionInfoV2(v.FeaturesInfo.RedundantResourceCleanup, "ACOR_just_scan_result", {
                        device_id: v.default.getDeviceID(),
                        timestamp: O.default.getTimeNow(),
                        just_scan_no_cleanup: 1,
                        clean_up_session_id: t,
                        trigger_event: s,
                        total_cleanup_duration_s: Math.round(y.b.sum(Object.values(r)) / 1e3),
                        free_space_at_clean_up_completed_gb: d,
                        orphan_resources_details: g,
                        orphan_resource_last_active_0_3d_count: u[0].count,
                        orphan_resource_last_active_3_7d_count: u[1].count,
                        orphan_resource_last_active_7_14d_count: u[2].count,
                        "orphan_resource_last_active_14d+_count": w(w(n.count, a.count), y.b.sumBy((e => e.count), u)),
                        orphan_resource_last_active_0_3d_size_gb: D(u[0].size),
                        orphan_resource_last_active_3_7d_size_gb: D(u[1].size),
                        orphan_resource_last_active_7_14d_size_gb: D(u[2].size),
                        "orphan_resource_last_active_14d+_size_gb": D(w(w(n.size, a.size), y.b.sumBy((e => e.size), u)))
                    }) : v.default.logActionInfoV2(v.FeaturesInfo.RedundantResourceCleanup, "ACOR_cleanup_completed", {
                        device_id: v.default.getDeviceID(),
                        timestamp: O.default.getTimeNow(),
                        clean_up_session_id: t,
                        trigger_event: s,
                        total_cleanup_duration_s: Math.round(y.b.sum(Object.values(r)) / 1e3),
                        interrupted_delete: i[l.c.DELETE_RESOURCES] || 0,
                        free_space_at_clean_up_completed_gb: d,
                        orphan_resources_deleted_count: w(n.count, a.count),
                        orphan_resources_deleted_size_gb: D(w(n.size, a.size)),
                        orphan_resources_remained_count: o ? o.count : 0,
                        orphan_resources_remained_size_gb: o ? D(o.size) : 0,
                        orphan_resources_details: g
                    })
                }))
            }
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/default-main-startup-shared-worker.aef6ad812cbda4e7e7af.js.map