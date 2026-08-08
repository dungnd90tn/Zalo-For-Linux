(this.webpackJsonp = this.webpackJsonp || []).push([
    [48], {
        IODd: function(e, t, o) {
            "use strict";
            o.r(t);
            var r = o("VTBJ"),
                n = o("q1tI"),
                a = o.n(n),
                s = o("7iK/"),
                i = o("dFSO"),
                c = o("6Be5");
            const l = Object(n.memo)((e => {
                const {
                    warningBytes: t = Object(i.a)("600MB"),
                    dangerBytes: o = Object(i.a)("1GB")
                } = e || {}, [l, b] = Object(n.useState)({
                    showTooltip: !0
                }), {
                    memoryInBytes: g,
                    startMonitor: d,
                    stopMonitor: p
                } = Object(s.a)();
                let h = 0;
                Object(n.useEffect)((() => (d(), () => {
                    p()
                })), []);
                const m = Object(n.useMemo)((() => g > t ? g > o ? "danger" : "warning" : "normal"), [g]),
                    u = () => {
                        l.showTooltip && (h++, h > 2 && b((e => Object(r.a)(Object(r.a)({}, e), {}, {
                            showTooltip: !1
                        }))))
                    },
                    w = () => a.a.createElement("div", {
                        style: {
                            position: "absolute",
                            top: -32,
                            left: 0,
                            height: 32,
                            width: 64,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center"
                        },
                        onMouseOver: u
                    }, a.a.createElement("span", {
                        style: {
                            padding: "1px 6px",
                            background: "danger" === m ? "rgb(255 88 45 / 70%)" : "warning" === m ? "rgb(234 190 143 / 80%)" : "rgb(17 30 99 / 30%)",
                            borderRadius: 4,
                            border: "1px solid",
                            borderColor: "danger" === m ? "rgb(255 208 186 / 50%)" : "warning" === m ? "rgb(206 206 206 / 80%)" : "rgb(23 123 212 / 50%)",
                            color: "danger" === m ? "#9c0000" : "warning" === m ? "#c25a00" : "var(--text-on-color)",
                            fontSize: 11,
                            fontWeight: "normal" === m ? 400 : 500,
                            transition: "all .3s"
                        }
                    }, Object(i.d)(g)));
                return l.showTooltip ? a.a.createElement(c.a, {
                    arrow: !0,
                    placement: "right-center",
                    title: "Memory of RENDER process",
                    positionMode: "anchor"
                }, w()) : w()
            }));
            t.default = l
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/48.bcc959683f11d7790fd2.js.map