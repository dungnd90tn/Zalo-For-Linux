(this.webpackJsonp = this.webpackJsonp || []).push([
    [52], {
        "2mlY": function(a, l, d) {
            "use strict";
            d.r(l), d.d(l, "ZinstantNetworkProviderIml", (function() {
                return n
            }));
            var o = d("fBUP"),
                r = d("X4fA"),
                t = d("z0WU"),
                e = d("Cbzp");
            class n extends e.ZinstantNetworkProvider {
                get(a, l) {
                    var d;
                    let e = a.url + "?" + o.default._getCommonParams() + `&params=${encodeURIComponent(t.default.encodeAES(JSON.stringify({imei:r.a.getZaloClientID()})))}`;
                    o.default._get(e, null !== (d = a.params) && void 0 !== d ? d : null, null, 0, 0, 0, null, {
                        headers: null == a ? void 0 : a.headers
                    }).then((a => {
                        var d, o, r, e, n;
                        if (200 !== (null == a ? void 0 : a.status) || !a.data.data) throw {
                            data: (null == a || null === (d = a.data) || void 0 === d ? void 0 : d.data) || null,
                            error_code: (null == a || null === (o = a.data) || void 0 === o ? void 0 : o.error_code) || (null == a || null === (r = a.data) || void 0 === r ? void 0 : r.error) || a.status,
                            error_message: (null == a || null === (e = a.data) || void 0 === e ? void 0 : e.error_message) || (null == a || null === (n = a.data) || void 0 === n ? void 0 : n.errorMsg) || a.statusText
                        };
                        {
                            var u, i, s, v, c;
                            const d = JSON.parse(t.default.decodeAES(a.data.data));
                            a.data = d;
                            const o = JSON.stringify(null !== (u = a.data.data) && void 0 !== u ? u : {});
                            l(o, null !== (i = null === (s = a.data) || void 0 === s ? void 0 : s.error_code) && void 0 !== i ? i : 0, null !== (v = null === (c = a.data) || void 0 === c ? void 0 : c.error_message) && void 0 !== v ? v : "")
                        }
                    })).catch((a => {
                        var d, o, r;
                        l(null !== (d = null == a ? void 0 : a.data) && void 0 !== d ? d : "", null !== (o = null == a ? void 0 : a.error_code) && void 0 !== o ? o : -1, null !== (r = null == a ? void 0 : a.error_message) && void 0 !== r ? r : "Unknow")
                    }))
                }
                post(a, l) {
                    var d;
                    let r = a.url + "?" + o.default._getCommonParams();
                    const e = t.default.encodeAES(JSON.stringify(null !== (d = a.params) && void 0 !== d ? d : {}));
                    o.default._post(r, o.default._constructUrlParams({
                        params: e
                    }), null, null, 0).then((a => {
                        var d, o, r, e, n;
                        if (200 !== (null == a ? void 0 : a.status) || !a.data.data) throw {
                            data: (null == a || null === (d = a.data) || void 0 === d ? void 0 : d.data) || null,
                            error_code: (null == a || null === (o = a.data) || void 0 === o ? void 0 : o.error_code) || (null == a || null === (r = a.data) || void 0 === r ? void 0 : r.error) || a.status,
                            error_message: (null == a || null === (e = a.data) || void 0 === e ? void 0 : e.error_message) || (null == a || null === (n = a.data) || void 0 === n ? void 0 : n.errorMsg) || a.statusText
                        };
                        {
                            var u, i, s, v, c;
                            const d = JSON.parse(t.default.decodeAES(a.data.data));
                            a.data = d;
                            const o = JSON.stringify(null !== (u = a.data.data) && void 0 !== u ? u : {});
                            l(o, null !== (i = null === (s = a.data) || void 0 === s ? void 0 : s.error_code) && void 0 !== i ? i : 0, null !== (v = null === (c = a.data) || void 0 === c ? void 0 : c.error_message) && void 0 !== v ? v : "")
                        }
                    })).catch((a => {
                        var d, o, r;
                        l(null !== (d = null == a ? void 0 : a.data) && void 0 !== d ? d : "", null !== (o = null == a ? void 0 : a.error_code) && void 0 !== o ? o : -1, null !== (r = null == a ? void 0 : a.error_message) && void 0 !== r ? r : "Unknow")
                    }))
                }
            }
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/52.ecbfa0dd62bfd6631313.js.map