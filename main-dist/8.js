exports.ids = [8], exports.modules = {
    WDks: function(t, e, r) {
        "use strict";
        r.r(e), r.d(e, "getDrivesInfo", (function() {
            return n
        }));
        var s = r("lUm7");
        const n = async () => {
            const t = await s.a.diskInfo();
            return new Proxy(t, {
                get(t, e) {
                    var r;
                    return t["string" == typeof e ? (r = e).endsWith(":\\") ? r.toUpperCase() : /^[A-Z]$/i.test(r) ? r.toUpperCase() + ":\\" : r : e]
                }
            })
        }
    }
};