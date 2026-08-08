'use strict';
// Linux stub - zcall native binary is macOS only
const noop = () => {};
const noopPromise = () => Promise.resolve({});

const stub = {
    MainApp: () => ({
        check:               noop,
        authenication:       noopPromise,
        setCallback:         noop,
        setConfigData:       noopPromise,
        makeCall:            noop,
        incomingCall:        noop,
        stop:                noop,
        mute:                noop,
        stopCapture:         noop,
        holdAudio:           noop,
        getCallInfo:         () => ({}),
        getJsonStats406:     () => '{}',
        getListDevices:      () => [],
        getEventMessage:     () => null,
        getVideoFrame:       noop,
        getVideoFrameLocal:  noop,
        changeAudioDevice:   noop,
        setAudioVolume:      noop,
        changeVideoDevice:   noop,
        setAgc:              noop,
        startDesktopCapture: noop,
        stopDesktopCapture:  noop,
        changeMinMaxMobileBitrate: noop,
        getExtendData:       () => '{}',
        getActiveAudioCodecs: () => [],
        bindGetPeerId:       noop,
    })
};
module.exports = stub;
