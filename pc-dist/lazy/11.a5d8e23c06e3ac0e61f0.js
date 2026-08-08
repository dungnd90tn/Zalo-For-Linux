(this.webpackJsonp = this.webpackJsonp || []).push([
    [11], {
        "8/ir": function(t, e, a) {
            "use strict";
            var r = a("UiPd"),
                i = a("Gm1y"),
                n = a("1pet");
            class u {
                static async convertId(t, e, a) {
                    if (!t || "string" != typeof t) return "";
                    if ("me" == t) return r.default.getUidMe();
                    try {
                        return e ? await i.default.getGroupIdFromGlobalId(t) : await r.default.getUserIdFromGlobalId(t)
                    } catch (n) {
                        return ""
                    }
                }
                static async getGroupInfo(t, e) {
                    if (!t) return null;
                    try {
                        return await i.default.getGroupById(t)
                    } catch (a) {
                        return null
                    }
                }
                static async getProfileFriend(t, e) {
                    try {
                        return await r.default.getProfileFriendById(t, n.SRC_GET_PROFILE.FETCH_MINI_INFO)
                    } catch (a) {
                        return null
                    }
                }
                static async getDName(t, e, a = (() => {}), n = (() => {}), d) {
                    let l = e ? i.default.getDName(t) : r.default.getDName(t);
                    if (!l) {
                        const a = e ? await u.getGroupInfo(t, d) : await u.getProfileFriend(t, d);
                        a && a.displayName && (l = a.displayName)
                    }
                    l ? a(l) : n("not found")
                }
                static getUidMe() {
                    return r.default.getUidMe()
                }
            }
            e.a = u
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/11.a5d8e23c06e3ac0e61f0.js.map