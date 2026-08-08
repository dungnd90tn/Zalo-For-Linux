(this.webpackJsonp = this.webpackJsonp || []).push([
    [64], {
        "Ub+V": function(t, e, n) {
            "use strict";
            n.r(e), n.d(e, "default", (function() {
                return a
            }));
            var c = n("q1tI"),
                s = n.n(c),
                u = (n("9Tg7"), n("euSg")),
                o = n("TmDO"),
                r = n("xPHB");
            const i = 5e3;

            function a({
                _window: t = window
            }) {
                const e = Object(r.b)();
                return s.a.useEffect((() => {
                    const e = t.document.body;
                    return e && (e.classList.add(o.THEME.DARK), Object(u.g)()), () => {
                        e && (e.classList.remove(o.THEME.DARK), setTimeout((() => {
                            Object(u.g)()
                        }), i))
                    }
                }), []), s.a.useEffect((() => {
                    const n = t.document.head;
                    return n && e && Object(u.j)(o.THEME.DARK), () => {
                        n && e && Object(u.j)(o.THEME.LIGHT)
                    }
                }), [e]), null
            }
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/64.5b054957a1310ace92d2.js.map