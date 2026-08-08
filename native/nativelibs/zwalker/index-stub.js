'use strict';
// Linux stub - no native .node files shipped for Linux in this build
const noop = () => Promise.resolve([]);
const noopSync = () => [];

module.exports.scanDirectory          = noop;
module.exports.updateReferenceMessageId = noop;
module.exports.deleteHomelessFiles    = noop;
module.exports.statUnmarkedFiles      = noop;
module.exports.deleteEmptyFolders     = noop;
