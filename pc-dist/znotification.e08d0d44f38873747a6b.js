__SCRIPT_TYPE__ = "renderer",
    function(e) {
        function t(t) {
            for (var o, i, l = t[0], c = t[1], d = t[2], f = 0, s = []; f < l.length; f++) i = l[f], Object.prototype.hasOwnProperty.call(r, i) && r[i] && s.push(r[i][0]), r[i] = 0;
            for (o in c) Object.prototype.hasOwnProperty.call(c, o) && (e[o] = c[o]);
            for (u && u(t); s.length;) s.shift()();
            return a.push.apply(a, d || []), n()
        }

        function n() {
            for (var e, t = 0; t < a.length; t++) {
                for (var n = a[t], o = !0, l = 1; l < n.length; l++) {
                    var c = n[l];
                    0 !== r[c] && (o = !1)
                }
                o && (a.splice(t--, 1), e = i(i.s = n[0]))
            }
            return e
        }
        var o = {},
            r = {
                42: 0
            },
            a = [];

        function i(t) {
            if (o[t]) return o[t].exports;
            var n = o[t] = {
                i: t,
                l: !1,
                exports: {}
            };
            return e[t].call(n.exports, n, n.exports, i), n.l = !0, n.exports
        }
        i.e = function(e) {
            var t = [],
                n = r[e];
            if (0 !== n)
                if (n) t.push(n[2]);
                else {
                    var o = new Promise((function(t, o) {
                        n = r[e] = [t, o]
                    }));
                    t.push(n[2] = o);
                    var a, l = document.createElement("script");
                    l.charset = "utf-8", l.timeout = 120, i.nc && l.setAttribute("nonce", i.nc), l.src = function(e) {
                        return i.p + "lazy/" + ({
                            23: "countries",
                            26: "lang-en",
                            27: "lang-vi"
                        } [e] || e) + "." + {
                            12: "7f974bf19beb8f117b97",
                            17: "8a9bbb19b4814c8ea0c9",
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
                            59: "87a8a857fdef256298f6",
                            60: "4db464ec8d1dbe548c78",
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
                    a = function(t) {
                        l.onerror = l.onload = null, clearTimeout(d);
                        var n = r[e];
                        if (0 !== n) {
                            if (n) {
                                var o = t && ("load" === t.type ? "missing" : t.type),
                                    a = t && t.target && t.target.src;
                                c.message = "Loading chunk " + e + " failed.\n(" + o + ": " + a + ")", c.name = "ChunkLoadError", c.type = o, c.request = a, n[1](c)
                            }
                            r[e] = void 0
                        }
                    };
                    var d = setTimeout((function() {
                        a({
                            type: "timeout",
                            target: l
                        })
                    }), 12e4);
                    l.onerror = l.onload = a, document.head.appendChild(l)
                } return Promise.all(t)
        }, i.m = e, i.c = o, i.d = function(e, t, n) {
            i.o(e, t) || Object.defineProperty(e, t, {
                enumerable: !0,
                get: n
            })
        }, i.r = function(e) {
            "undefined" != typeof Symbol && Symbol.toStringTag && Object.defineProperty(e, Symbol.toStringTag, {
                value: "Module"
            }), Object.defineProperty(e, "__esModule", {
                value: !0
            })
        }, i.t = function(e, t) {
            if (1 & t && (e = i(e)), 8 & t) return e;
            if (4 & t && "object" == typeof e && e && e.__esModule) return e;
            var n = Object.create(null);
            if (i.r(n), Object.defineProperty(n, "default", {
                    enumerable: !0,
                    value: e
                }), 2 & t && "string" != typeof e)
                for (var o in e) i.d(n, o, function(t) {
                    return e[t]
                }.bind(null, o));
            return n
        }, i.n = function(e) {
            var t = e && e.__esModule ? function() {
                return e.default
            } : function() {
                return e
            };
            return i.d(t, "a", t), t
        }, i.o = function(e, t) {
            return Object.prototype.hasOwnProperty.call(e, t)
        }, i.p = "", i.oe = function(e) {
            throw e
        };
        var l = this.webpackJsonp = this.webpackJsonp || [],
            c = l.push.bind(l);
        l.push = t, l = l.slice();
        for (var d = 0; d < l.length; d++) t(l[d]);
        var u = c;
        a.push([9, 0, 4, 5, 1, 2, 3]), n()
    }({
        "3mqh": function(e, t, n) {},
        9: function(e, t, n) {
            e.exports = n("M75v")
        },
        B8cF: function(e, t, n) {
            "use strict";
            n.r(t);
            n("v5OU")
        },
        EXGp: function(e, t) {
            e.exports = require("../native/nativelibs")
        },
        I9mj: function(e, t, n) {},
        M75v: function(e, t, n) {
            (function(e) {
                var t = n("3tO9");
                n("I9mj"), n("B8cF"), n("33YB");
                const {
                    THEME: o
                } = n("TmDO"), r = 1e4;
                var a, i, l, c, d, u, f, s, b = !1;

                function p(e) {
                    return document.getElementById(e)
                }

                function m() {
                    ! function(e) {
                        $zlogger.sendLog("error", e)
                    }("[pid][notification]" + e.pid), $zapp.registerProcess("notification", e.pid), $zsub.$zapp.onRequestShowNoti(((e, p) => {
                        (async function() {
                            try {
                                const e = await $zconfig.getThemeConfig();
                                (null == e ? void 0 : e.theme) === o.LIGHT && n("3mqh"), (null == e ? void 0 : e.theme) === o.DARK && n("drlN");
                                const t = document.body;
                                t.classList.toggle(o.DARK, (null == e ? void 0 : e.theme) === o.DARK), t.classList.toggle(o.LIGHT, (null == e ? void 0 : e.theme) === o.LIGHT)
                            } catch (e) {}
                        })().then(),
                            function(e) {
                                let n = (a = t({}, e)).body || "";
                                n && (n = n.replace(/\r|\n/g, " ")), i.innerText = a.title, c.style.backgroundImage = `url(${a.icon})`, 1 == a.avaType ? c.style.borderRadius = "50%" : c.style.borderRadius = "0", n ? (d.innerText = n, null != d.className && -1 !== d.className.indexOf(" empty") && (d.className = d.className.replace(" empty", ""))) : null != d.className && -1 === d.className.indexOf(" empty") && (d.className += " empty"), l.innerText = a.titlePrefix, a.titlePrefix ? l.style.visibility = "visible" : l.style.visibility = "hidden";
                                let o = "var(--layer-background)";
                                a.additionInfos && a.additionInfos.label && a.additionInfos.label.color ? (o = a.additionInfos.label.color, f.style.backgroundColor = o, f.style.display = "block") : f.style.display = "none", u.style.height = null, s && clearTimeout(s), !0 === a.additionInfos.noAutoClose || (s = setTimeout((() => {
                                    s = null, !b && $zwindow.hide()
                                }), r)), $zwindow.showInactive()
                            }(p)
                    })), u = p("zbg"), i = p("zname"), c = p("zavatar"), d = p("zbody"), f = p("zlabel"), l = p("zname-prefix");
                    let m = p("closebtn");
                    m.style.backgroundImage = "url(close)", m.onclick = e => {
                        s && (clearTimeout(s), s = null), !b && $zwindow.hide(), e.preventDefault(), e.stopPropagation()
                    }, u.onclick = () => {
                        let e = t({}, a);
                        s && (clearTimeout(s), s = null), !b && $zwindow.hide(), $zapp.notifyClickNoti(e)
                    }, $zapp.notifyNotiReady()
                }
                document.addEventListener("DOMContentLoaded", (() => {
                    m()
                }))
            }).call(this, n("8oxB"))
        },
        drlN: function(e, t, n) {}
    });
//# sourceMappingURL=sourcemaps/znotification.e08d0d44f38873747a6b.js.map