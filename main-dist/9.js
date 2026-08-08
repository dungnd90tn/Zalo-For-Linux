exports.ids = [9], exports.modules = {
    "3U5D": function(E, _, R) {
        "use strict";
        R.r(_), R.d(_, "RestructureMediaQos", (function() {
            return A
        }));
        var e = R("jDHv"),
            S = R("fc/q");
        const C = {
            HARDLINK_FULL_SUPPORT: {
                CMD: 152720
            },
            HARDLINK_PARTIAL_SUPPORT: {
                CMD: 152721
            },
            HARDLINK_MINIMAL_SUPPORT: {
                CMD: 152722
            },
            HARDLINK_NO_SUPPORT: {
                CMD: 152723
            },
            MEDIA_DRIVE_NOT_SUPPORT_HARDLINK: {
                CMD: 152724
            },
            IE_MIGRATION_SUCCESS: {
                CMD: 152755
            },
            FILE_LINKING_FAILED: {
                CMD: 152756,
                ERROR_CODE: {
                    PATH_EMPTY: 1,
                    NOT_IN_MEDIA_FOLDER: 2,
                    MISSING_TRIPLE_ID: 3,
                    INVALID_RESOURCE_TYPE: 4,
                    INVALID_IMAGE_QUALITY: 5,
                    LINKING_FAILED: 6,
                    UNSUPPORTED_FILESYSTEM: 7
                }
            },
            MIGRATE_MESSAGE_COMMANDS: {
                STARTED: {
                    CMD: 152757
                },
                COMPLETED: {
                    CMD: 152758,
                    ERROR_CODE: {
                        PATH_VERSION_V1: 1,
                        PATH_VERSION_V2: 2
                    }
                },
                FAILED: {
                    CMD: 152759
                },
                MISMATCH_CHECK: {
                    CMD: 152760,
                    ERROR_CODE: {
                        VERSION_MISMATCH: 1,
                        TOTAL_MESSAGE_MISMATCH: 2,
                        TOTAL_CONVERSATION_MISMATCH: 3
                    }
                }
            },
            MESSAGE_PATH_MIGRATOR: {
                CMD: 152761,
                ERROR_CODE: {
                    MIGRATE_MESSAGE_FAILED: 1,
                    MIGRATE_PATH_FAILED: 2
                }
            },
            APPLY_NEW_PATH_STRUCTURE_SUCCESS: {
                CMD: 152762,
                ERROR_CODE: {
                    STARTUP_MIGRATE: 1,
                    SCAN_MSG_MIGRATE: 2
                }
            },
            APPLY_NEW_PATH_STRUCTURE_FAILED: {
                CMD: 152763,
                ERROR_CODE: {
                    STARTUP_MIGRATE: 1,
                    SCAN_MSG_MIGRATE: 2
                }
            },
            OLD_STRUCTURE_CLEANUP_SUCCESS: {
                CMD: 152764
            },
            OLD_STRUCTURE_CLEANUP_FAILED: {
                CMD: 152765
            },
            REBUILD_HARDLINK_SUCCESS: {
                CMD: 152766
            },
            REBUILD_HARDLINK_FAILED: {
                CMD: 152767
            },
            REBUILD_HARDLINK_RESOURCE_FAILED: {
                CMD: 152768
            },
            MESSAGE_TRANSFER_MIGRATE_SUCCESS: {
                CMD: 152769
            },
            MESSAGE_TRANSFER_MIGRATE_FAILED: {
                CMD: 152770,
                ERROR_CODE: {
                    MIGRATE_FAILED: 1,
                    ENQUEUE_FAILED: 2
                }
            },
            PATH_VERSION_V1_STATUS: {
                CMD: 152771,
                ERROR_CODE: {
                    NO_NTFS_SUPPORTED: 0,
                    NTFS_SUPPORTED: 1
                }
            },
            PATH_VERSION_V2_STATUS: {
                CMD: 152772,
                ERROR_CODE: {
                    NO_NTFS_SUPPORTED: 0,
                    NTFS_SUPPORTED: 1
                }
            },
            GEN_RESOURCE_NAME_FAILED: {
                CMD: 152773,
                ERROR_CODE: {
                    INVALID_CLIENT_ID: 1,
                    INVALID_TRIPLE_ID: 2
                }
            }
        };
        class A {
            static getInstance() {
                return this.instance || (this.instance = new A), this.instance
            }
            increaseFailed(E) {
                e.ModuleContainer.resolve(S.QoS).log({
                    success: !1,
                    commandId: E.cmd,
                    subCommandId: E.subCmd,
                    duration: E.duration,
                    errorCode: E.code,
                    startTime: 0,
                    params: []
                })
            }
            logImportExportMigrateSuccess(E) {
                const _ = Math.floor(E / 5e4);
                this.increaseFailed({
                    cmd: C.IE_MIGRATION_SUCCESS.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: _
                })
            }
            logFileLinkingFailed(E) {
                this.increaseFailed({
                    cmd: C.FILE_LINKING_FAILED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.FILE_LINKING_FAILED.ERROR_CODE[E]
                })
            }
            logMigrateMismatch(E) {
                this.increaseFailed({
                    cmd: C.MIGRATE_MESSAGE_COMMANDS.MISMATCH_CHECK.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.MIGRATE_MESSAGE_COMMANDS.MISMATCH_CHECK.ERROR_CODE[E]
                })
            }
            logMigrateStarted() {
                this.increaseFailed({
                    cmd: C.MIGRATE_MESSAGE_COMMANDS.STARTED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: 0
                })
            }
            logMigrateCompleted(E) {
                this.increaseFailed({
                    cmd: C.MIGRATE_MESSAGE_COMMANDS.COMPLETED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.MIGRATE_MESSAGE_COMMANDS.COMPLETED.ERROR_CODE[E]
                })
            }
            logMigrateFailed(E) {
                this.increaseFailed({
                    cmd: C.MIGRATE_MESSAGE_COMMANDS.FAILED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: E
                })
            }
            logMessagePathMigratorFailed(E) {
                this.increaseFailed({
                    cmd: C.MESSAGE_PATH_MIGRATOR.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.MESSAGE_PATH_MIGRATOR.ERROR_CODE[E]
                })
            }
            logApplyNewPathStructureSuccess(E) {
                this.increaseFailed({
                    cmd: C.APPLY_NEW_PATH_STRUCTURE_SUCCESS.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.APPLY_NEW_PATH_STRUCTURE_SUCCESS.ERROR_CODE[E]
                })
            }
            logApplyNewPathStructureFailed(E) {
                this.increaseFailed({
                    cmd: C.APPLY_NEW_PATH_STRUCTURE_FAILED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.APPLY_NEW_PATH_STRUCTURE_FAILED.ERROR_CODE[E]
                })
            }
            logOldStructureCleanupSuccess(E) {
                this.increaseFailed({
                    cmd: C.OLD_STRUCTURE_CLEANUP_SUCCESS.CMD,
                    subCmd: 0,
                    duration: E,
                    code: 0
                })
            }
            logOldStructureCleanupFailed(E) {
                this.increaseFailed({
                    cmd: C.OLD_STRUCTURE_CLEANUP_FAILED.CMD,
                    subCmd: 0,
                    duration: E,
                    code: 0
                })
            }
            logRebuildHardlinkSuccess(E) {
                this.increaseFailed({
                    cmd: C.REBUILD_HARDLINK_SUCCESS.CMD,
                    subCmd: 0,
                    duration: E,
                    code: 0
                })
            }
            logRebuildHardlinkFailed(E) {
                this.increaseFailed({
                    cmd: C.REBUILD_HARDLINK_FAILED.CMD,
                    subCmd: 0,
                    duration: E,
                    code: 0
                })
            }
            logRebuildHardlinkResourceFailed(E, _) {
                this.increaseFailed({
                    cmd: C.REBUILD_HARDLINK_RESOURCE_FAILED.CMD,
                    subCmd: 0,
                    duration: _,
                    code: E
                })
            }
            logMessageTransferMigrateSuccess(E) {
                this.increaseFailed({
                    cmd: C.MESSAGE_TRANSFER_MIGRATE_SUCCESS.CMD,
                    subCmd: 0,
                    duration: E,
                    code: 0
                })
            }
            logMessageTransferMigrateFailed(E, _) {
                this.increaseFailed({
                    cmd: C.MESSAGE_TRANSFER_MIGRATE_FAILED.CMD,
                    subCmd: 0,
                    duration: _,
                    code: C.MESSAGE_TRANSFER_MIGRATE_FAILED.ERROR_CODE[E]
                })
            }
            logGenResourceNameFailed(E) {
                this.increaseFailed({
                    cmd: C.GEN_RESOURCE_NAME_FAILED.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.GEN_RESOURCE_NAME_FAILED.ERROR_CODE[E]
                })
            }
            markHardlinkSupport(E) {
                this.increaseFailed({
                    cmd: C.HARDLINK_FULL_SUPPORT.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: E
                })
            }
            logPathVersionV1Status(E) {
                this.increaseFailed({
                    cmd: C.PATH_VERSION_V1_STATUS.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.PATH_VERSION_V1_STATUS.ERROR_CODE[E]
                })
            }
            logPathVersionV2Status(E) {
                this.increaseFailed({
                    cmd: C.PATH_VERSION_V2_STATUS.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: C.PATH_VERSION_V2_STATUS.ERROR_CODE[E]
                })
            }
            logPathVersion(E, _) {
                E ? this.logPathVersionV2Status(_ ? "NTFS_SUPPORTED" : "NO_NTFS_SUPPORTED") : this.logPathVersionV1Status(_ ? "NTFS_SUPPORTED" : "NO_NTFS_SUPPORTED")
            }
            markHardlinkPartialSupport(E, _) {
                this.increaseFailed({
                    cmd: C.HARDLINK_PARTIAL_SUPPORT.CMD,
                    subCmd: _,
                    duration: 0,
                    code: E
                })
            }
            markHardlinkMinimalSupport(E) {
                this.increaseFailed({
                    cmd: C.HARDLINK_MINIMAL_SUPPORT.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: E
                })
            }
            markHardlinkNoSupport(E) {
                this.increaseFailed({
                    cmd: C.HARDLINK_NO_SUPPORT.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: E
                })
            }
            markMediaDriveNotSupportHardlink(E) {
                this.increaseFailed({
                    cmd: C.MEDIA_DRIVE_NOT_SUPPORT_HARDLINK.CMD,
                    subCmd: 0,
                    duration: 0,
                    code: E ? 1 : 0
                })
            }
        }
        A.instance = void 0
    }
};