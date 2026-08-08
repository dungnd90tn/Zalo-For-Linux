(this.webpackJsonp = this.webpackJsonp || []).push([
    [51], {
        TRf6: function(a, e, t) {
            "use strict";
            t.r(e), t.d(e, "default", (function() {
                return N
            }));
            var s = t("+pQO"),
                r = t("gwig"),
                n = t("Ilka"),
                o = t("lb3U"),
                c = t("XaxS"),
                i = t("Cbzp"),
                C = t("rgLe"),
                _ = t("8/ir"),
                d = t("AyM1"),
                A = t("Uzj0");
            const I = [o.a.CHECK_GROUP_HAS_AVATAR, o.a.CHECK_IS_MY_UID, o.a.CHECK_GROUP_HAS_NAME],
                E = [o.a.CHECK_GROUP_HAS_AVATAR, o.a.CHECK_GROUP_HAS_NAME],
                f = -1,
                l = 0,
                u = 1;
            class N extends i.ZNSHandleCMD {
                handle(a) {
                    I.includes(a.type) ? _.a.convertId(a.data, E.includes(a.type), c.a.ZNS_CONDITION).then((e => {
                        a.data = e, this.doHandle(a)
                    })).catch((e => {
                        null == a || a.error(e)
                    })) : this.doHandle(a)
                }
                doHandle(a) {
                    const e = a.data,
                        t = a.success,
                        i = a.error;
                    switch (a.type) {
                        case o.a.CHECK_PLATFORM: {
                            const a = "pc";
                            let s = e;
                            if ("string" == typeof s) {
                                const [a, e] = A.g.safeParseJson(s);
                                a || (s = e)
                            }
                            if (s && s.constructor == Array) return s.includes(a) ? void t(u) : void t(l);
                            t(f);
                            break
                        }
                        case o.a.CHECK_GROUP_HAS_AVATAR:
                            _.a.getGroupInfo(e, c.a.ZNS_CONDITION).then((a => {
                                t(a ? a.avatar ? u : l : f)
                            })).catch((() => {
                                t(f)
                            }));
                            break;
                        case o.a.CHECK_GROUP_HAS_NAME:
                            _.a.getGroupInfo(e, c.a.ZNS_CONDITION).then((a => {
                                t(a ? 0 == a.subType ? l : u : f)
                            })).catch((() => {
                                t(f)
                            }));
                            break;
                        case o.a.CHECK_IS_MY_UID:
                            t(e == _.a.getUidMe() ? u : l);
                            break;
                        case o.a.CHECK_DATETIME: {
                            let a = e;
                            if ("string" == typeof e) {
                                const [e, t] = A.g.safeParseJson(a);
                                e || (a = t)
                            }
                            if (!a || a.constructor !== Array) return void t(f);
                            if (d.a.inThisDateTimeWithValue(a)) return void t(u);
                            t(l);
                            break
                        }
                        case o.a.CHECK_NETWORK_CONNECTION:
                            t(r.b.getStateNetwork() == r.a.CONNECTED ? u : l);
                            break;
                        case o.a.CHECK_BIZ_INFO_CHAT_VISIBILITY:
                            d.a.safeRun((() => {
                                var r, n;
                                const o = C.c.getBusinessInfoChat(null === (r = s.a.getMessageInfo(a.key)) || void 0 === r || null === (n = r.message) || void 0 === n ? void 0 : n.toUid, e);
                                t(o.visible ? u : l)
                            }), (a => i(a)));
                            break;
                        case o.a.CHECK_ALLOW_CLOSE_ZINSTANT:
                            try {
                                t(e && Object(n.E)(e) ? u : l)
                            } catch (i) {
                                t(l)
                            }
                            break;
                        case o.a.CHECK_VERSION:
                            try {
                                let a = e;
                                if ("string" == typeof e) {
                                    const [e, t] = A.g.safeParseJson(a);
                                    e || (a = t)
                                }
                                if (!a || "number" != typeof a["DARWIN".toLowerCase()] || "string" != typeof a.action) throw new Error("data is empty");
                                const s = d.a.validVersionOnPlatform(a.action, a["DARWIN".toLowerCase()]);
                                t(s ? u : l)
                            } catch (i) {
                                t(l)
                            }
                            break;
                        default:
                            t(f)
                    }
                }
            }
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/51.1829ced05f1c18e93c0a.js.map