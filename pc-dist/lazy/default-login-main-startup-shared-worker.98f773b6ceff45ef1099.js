(this.webpackJsonp = this.webpackJsonp || []).push([
    [6], {
        "2xv9": function(t, e, a) {
            "use strict";
            a.d(e, "a", (function() {
                return r
            }));
            var s = a("wudS"),
                i = a("JTyQ");
            const r = () => {
                const t = Object(s.f)(),
                    e = {
                        shared: i.c
                    };
                return t.forEach((t => {
                    e[t] = i.c
                })), e
            }
        },
        nUpV: function(t, e, a) {
            "use strict";
            var s, i = a("jDHv"),
                r = a("75pU"),
                d = a.n(r),
                n = a("YEoC"),
                h = a("JTyQ"),
                c = a("d/or"),
                o = a("2xv9"),
                p = a("NTiX"),
                u = a("fzd6");
            let S = i.ModuleContainer.injectable()(s = function(t, e) {
                return Object(i.inject)(p.b)(t, void 0, 0)
            }(s = Reflect.metadata("design:type", Function)(s = Reflect.metadata("design:paramtypes", [void 0 === p.IDatabaseStateStorage ? Object : p.IDatabaseStateStorage])(s = class {
                constructor(t) {
                    this.databaseStateStorage = t, this._userId = null, this.states = null, this.getAdapterTypeOfSharedDatabases = d()(this.getAdapterTypeOfSharedDatabases.bind(this)), this.getAdapterTypeOfUserScopedDatabases = d()(this.getAdapterTypeOfUserScopedDatabases.bind(this))
                }
                reset() {
                    this.states = null, this.getAdapterTypeOfSharedDatabases = d()(this.getAdapterTypeOfSharedDatabases.bind(this)), this.getAdapterTypeOfUserScopedDatabases = d()(this.getAdapterTypeOfUserScopedDatabases.bind(this))
                }
                async getStates() {
                    if (null !== this.states) return this.states;
                    try {
                        let t = await this.databaseStateStorage.getObjectAsync(h.a);
                        null === t && (t = Object(o.a)(), await this.databaseStateStorage.setObjectAsync(h.a, t)), this.states = t
                    } catch (t) {
                        throw new Error("Invalid adapter type states")
                    }
                    return this.states
                }
                async saveStates(t) {
                    this.states = t, await this.databaseStateStorage.setObjectAsync(h.a, t)
                }
                async getAdapterTypeOfSharedDatabases() {
                    const t = await this.getStates();
                    let e = !1;
                    return void 0 === t.shared && (t.shared = h.b, e = !0), await this.migrateFullDone() && t.shared === n.a.IDB && (t.shared = n.a.SQLite, e = !0), await u.a.upgradeRequired() && t.shared === n.a.IDB && (t.shared = n.a.SQLite, e = !0), e && await this.saveStates(t), t.shared
                }
                async setAdapterTypeOfSharedDatabases(t) {
                    const e = await this.getStates();
                    let a = !1;
                    e.shared !== t && (e.shared = t, a = !0), a && await this.saveStates(e)
                }
                async getAdapterTypeOfUserScopedDatabases(t) {
                    const e = await this.getStates();
                    let a = !1;
                    return void 0 === e[t] && (e[t] = h.b, a = !0), await this.migrateFullDone() && e[t] === n.a.IDB && (e[t] = n.a.SQLite, a = !0), await u.a.upgradeRequired() && e[t] === n.a.IDB && (e[t] = n.a.SQLite, a = !0), a && await this.saveStates(e), e[t]
                }
                async migrateFullDone() {
                    return $zFeatures.migrateDB.isMigrateFullDone()
                }
                async setAdapterTypeOfUserScopedDatabases(t, e) {
                    const a = await this.getStates();
                    let s = !1;
                    a[t] !== e && (a[t] = e, s = !0), s && await this.saveStates(a)
                }
                async getInUseAdaperTypes() {
                    const t = await this.getStates(),
                        e = [];
                    if (void 0 !== t.shared) {
                        const a = t.shared;
                        e.push(a)
                    }
                    if (null !== this._userId && void 0 !== t[this._userId]) {
                        const a = t[this._userId];
                        e.includes(a) || e.push(a)
                    }
                    return e
                }
                init(t) {
                    this._userId = t, this.reset()
                }
            }) || s) || s) || s) || s;
            i.ModuleContainer.registerSingleton(c.a, S)
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/default-login-main-startup-shared-worker.98f773b6ceff45ef1099.js.map