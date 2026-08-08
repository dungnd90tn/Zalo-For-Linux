(this.webpackJsonp = this.webpackJsonp || []).push([
    [57], {
        C71O: function(e, t, n) {
            "use strict";
            n.r(t);
            var s = n("jDHv"),
                a = n("6ivO"),
                i = n("fubQ"),
                o = n("9LCO"),
                r = n("CjKu"),
                c = n("Uzj0");
            const d = new a.ObjectValidator({
                    enable_zinstant: a.Rule.field().oneOf([0, 1]),
                    limit_file_size_save: a.Rule.field().number().min(0),
                    limit_num_file_save: a.Rule.field().number().min(0),
                    limit_length_act_multi: a.Rule.field().number().min(0),
                    bannedActions: a.Rule.field().array("string"),
                    bannedTemplateUsingAction: a.Rule.field().array("string"),
                    transformActions: a.Rule.field().object({}),
                    reply_zinstant_v2: a.Rule.field().schema({
                        enabled: a.Rule.field().oneOf([0, 1]),
                        attach_zinstant_data: a.Rule.field().oneOf([0, 1])
                    })
                }),
                l = {
                    enable_zinstant: 1,
                    limit_file_size_save: 1e3,
                    limit_num_file_save: 100,
                    limit_length_act_multi: 2,
                    bannedActions: [],
                    bannedTemplateUsingAction: [],
                    transformActions: {},
                    reply_zinstant_v2: {
                        enabled: 1,
                        attach_zinstant_data: 0
                    }
                };
            class g extends a.AdvancedRemoteConfigs {
                constructor() {
                    super("zinstant_config", l, {
                        validator: d
                    }), this.addEventListener("update", (() => {
                        c.c.Macrotask.push((() => this.syncZInstantPluginConfig()))
                    })), c.c.Macrotask.push((() => this.syncZInstantPluginConfig()))
                }
                syncZInstantPluginConfig() {
                    s.ModuleContainer.resolve(o.a).get(i.a).useZinstantConfig({
                        bannedActions: this.nestedKey("bannedActions"),
                        transformActions: this.nestedKey("transformActions"),
                        bannedTemplateUsingAction: this.nestedKey("bannedTemplateUsingAction")
                    })
                }
            }
            s.ModuleContainer.registerSingleton(r.a, g);
            var m = n("AH6j"),
                u = n("Uz0v");
            class b extends m.b {}
            s.ModuleContainer.registerSingleton(u.a, b);
            var v, h = n("4Lqt"),
                f = n("9NA1"),
                p = n("nSW/");
            class y {
                constructor() {}
                get debugTools() {
                    return s.ModuleContainer.resolve(f.d).getAll(p.a)
                }
                async process(e) {
                    const [t] = e;
                    if (this.debugTools.some((e => e.enabled))) {
                        const e = await Promise.all(t.deliverProcessData.allmsgs.map((e => this.intercept(e))));
                        t.deliverProcessData.allmsgs = e, t.deliverProcessData.pageMsgs = t.deliverProcessData.pageMsgs.map((t => e.find((e => e.msgId === t.msgId)) || t)), t.messages = t.messages.map((t => e.find((e => e.msgId === t.msgId)) || t))
                    }
                    return e
                }
                async intercept(e) {
                    let t = e;
                    for (const n of this.debugTools.filter((e => e.enabled))) t = await n.simulate(t);
                    return t
                }
            }
            let E = Object(s.injectable)()(v = Reflect.metadata("design:type", Function)(v = Reflect.metadata("design:paramtypes", [])(v = class {
                constructor() {}
                async process(e) {
                    return (new y).process(e)
                }
                async intercept(e) {
                    return e
                }
            }) || v) || v) || v;
            s.ModuleContainer.registerSingleton(h.a, E);
            n("4i5M"), n("zzdu");
            var j, O = n("Mgpg"),
                I = n("CWQo"),
                M = n("dVU6"),
                _ = n("hOCS");
            let z = Object(s.singleton)()(j = Object(s.injectable)()(j = function(e, t) {
                return Object(s.inject)(f.b)(e, void 0, 0)
            }(j = function(e, t) {
                return Object(s.inject)(O.ZLoggerFactory)(e, void 0, 1)
            }(j = Reflect.metadata("design:type", Function)(j = Reflect.metadata("design:paramtypes", [void 0 === f.b ? Object : f.b, void 0 === O.ZLoggerFactory ? Object : O.ZLoggerFactory])(j = class {
                constructor(e, t) {
                    this.zinstantEventEmitter = e, this.logger = void 0, this.onReceiveNewMessage = e => {
                        c.c.Macrotask.push((() => {
                            e.incomingMessageDTOs.forEach((e => this.processMessageDTO(e)))
                        }))
                    }, this.logger = t.createZLogger(O.ZLoggerNametags.zinstant, ["auto-download-template", "@zpc-764"])
                }
                async onInit() {
                    this.zinstantEventEmitter.addEventListener(f.b.EventNames.INCOMING_MESSAGE, this.onReceiveNewMessage)
                }
                async processMessageDTO(e) {
                    try {
                        const t = _.a.create(new _.a.Adapters.IncomingMessageDTO(e));
                        if (t.isZInstant()) {
                            const n = await t.getZInstantData(),
                                s = {
                                    url: n.data_url,
                                    checksum: n.checksum,
                                    msgId: e.msgId
                                };
                            M.a.download({
                                url: n.data_url,
                                checksum: n.checksum,
                                msgId: e.msgId
                            }, !1), this.logger.zsymb(3, "5neI1h", ["Auto downloading zinstant template, msgId={}, data={}", "X0n1pW"], e.msgId, s)
                        }
                    } catch (t) {
                        this.logger.zsymb(21, "da4Shd", ["Failed to process message DTO for auto-download:", "gEOEma"], t)
                    }
                }
            }) || j) || j) || j) || j) || j) || j;
            s.ModuleContainer.register(I.a, z);
            var C, w = n("XITH"),
                Z = n("/Bne"),
                L = n("Z+rT");
            let R = Object(s.singleton)()(C = Object(s.injectable)()(C = function(e, t) {
                return Object(s.inject)(f.b)(e, void 0, 0)
            }(C = Reflect.metadata("design:type", Function)(C = Reflect.metadata("design:paramtypes", [void 0 === f.b ? Object : f.b])(C = class {
                constructor(e) {
                    this.zinstantEventEmitter = e, this.zinstantv1E2eeService = void 0, this.onIncomingMessage = e => {
                        c.c.Macrotask.push((() => {
                            const t = e.incomingMessageDTOs.filter((e => this.zinstantv1E2eeService.isZInstant(Object(w.a)(e.msgType))));
                            if (0 === t.length) return;
                            const n = t.map((e => Z.a.transformMessageFromServer(e, e.idTo)));
                            setTimeout((() => {
                                n.forEach((async e => {
                                    try {
                                        await this.zinstantv1E2eeService.decrypt(e)
                                    } catch (t) {
                                        zconsole.zsymb(18, "UDDq7W", "Auto decrypt E2EE message failed", t)
                                    }
                                }))
                            }), 5e3)
                        }))
                    }, this.zinstantv1E2eeService = new L.a
                }
                async onInit() {
                    this.zinstantEventEmitter.addEventListener(f.b.EventNames.INCOMING_MESSAGE, this.onIncomingMessage)
                }
            }) || C) || C) || C) || C) || C;
            s.ModuleContainer.register(I.a, R);
            var A, D = n("8/YW"),
                T = n("AyM1");
            let F = Object(s.singleton)()(A = Object(s.injectable)()(A = function(e, t) {
                return Object(s.inject)(f.b)(e, void 0, 0)
            }(A = function(e, t) {
                return Object(s.inject)(O.ZLoggerFactory)(e, void 0, 1)
            }(A = Reflect.metadata("design:type", Function)(A = Reflect.metadata("design:paramtypes", [void 0 === f.b ? Object : f.b, void 0 === O.ZLoggerFactory ? Object : O.ZLoggerFactory])(A = class {
                constructor(e, t) {
                    this.eventEmitter = e, this.logger = void 0, this.onDeleteMessage = e => {
                        const t = s.ModuleContainer.resolve(D.a).getUserID();
                        T.a.removeFileFromMsg(e.deleteMessageEntity, t), this.logger.zsymb(3, "CV710D", ["Sync delete resources for deleted messages, msgId={}", "Vd40iX"], e.deleteMessageEntity.msgId)
                    }, this.logger = t.createZLogger(O.ZLoggerNametags.zinstant, ["sync-delete-resource", "@zpc-764"])
                }
                async onInit() {
                    this.eventEmitter.addEventListener(f.b.EventNames.DELETE_MESSAGE, this.onDeleteMessage)
                }
            }) || A) || A) || A) || A) || A) || A;
            s.ModuleContainer.register(f.a, F);
            var S, N = n("Cbzp");
            let k = Object(s.singleton)()(S = Object(s.injectable)()(S = function(e, t) {
                return Object(s.inject)(f.b)(e, void 0, 0)
            }(S = function(e, t) {
                return Object(s.inject)(O.ZLoggerFactory)(e, void 0, 1)
            }(S = Reflect.metadata("design:type", Function)(S = Reflect.metadata("design:paramtypes", [void 0 === f.b ? Object : f.b, void 0 === O.ZLoggerFactory ? Object : O.ZLoggerFactory])(S = class {
                constructor(e, t) {
                    this.eventEmitter = e, this.logger = void 0, this.onChildWindowCreated = e => {
                        Object(N.defineZInstantInWindow)(e.windowContainer), this.logger.zsymb(3, "gdoBhh", ["Defined ZInstant in child window, windowId={}", "oZlLRa"], e.windowId)
                    }, this.logger = t.createZLogger(O.ZLoggerNametags.zinstant, ["multi-window", "@zpc-764"])
                }
                async onInit() {
                    this.eventEmitter.addEventListener(f.b.EventNames.CHILD_WINDOW_CREATED, this.onChildWindowCreated)
                }
            }) || S) || S) || S) || S) || S) || S;
            s.ModuleContainer.register(f.a, k)
        }
    }
]);
//# sourceMappingURL=../sourcemaps/lazy/57.f9a0a5fd5806de87341c.js.map