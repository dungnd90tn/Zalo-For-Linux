#!/usr/bin/env python3
"""Build the Linux `zjxl` native addon.

Upstream ships zjxl only as Mach-O / PE binaries, so on Linux
`native/nativelibs/zjxl/index.js` returns `{error: 'not support'}` and every
JPEG XL message photo fails to display.  That did not matter while the port ran
Electron 22 (Chromium 108 still had a JXL decoder behind `--enable-features=JXL`,
which main.js switches on).  Chromium removed JPEG XL in 110, so on Electron 43
neither decoder exists and chat photos degrade to a download placeholder.

This script builds a drop-in replacement exposing the same seven entry points the
bundles call (see `native/nativelibs/zjxl/index.js` and the `Q3fx` wrapper module
in main-dist/main.js):

    moduleReady()                         -> bool, synchronous
    getJxlInfo({buffer}, cb)              -> {width, height, ...}
    jxlToJpeg({buffer, quality, outputWidth, outputHeight}, cb)   -> JPEG bytes
    jxlDecompressMulti({localPath|buffer, quality, sizes?}, cb)   -> [{data, ...}]
    bitmapToJxl({buffer, width, height}, cb)                      -> JXL bytes
    resizeJxl({buffer, width, height}, cb)                        -> JXL bytes
    resizeJxlLimit({buffer, width, height, limit}, cb)            -> JXL bytes

Every callback is `(error, data, status_code)` with status 1 = success, matching
the `SUCCESS_STATUS` enum in the bundles.

libjxl and libjpeg-turbo are linked dynamically and their shared objects are
copied next to the addon with an $ORIGIN rpath -- the same self-contained layout
upstream uses for its macOS dylibs -- so the result does not depend on the host
distro shipping a matching libjxl.  (Static linking is not an option: Ubuntu's
libjpeg.a is built without -fPIC.)  Node dlopens addons RTLD_LOCAL, so these
copies stay out of the global symbol namespace Chromium resolves against.

Usage:
    python3 generate-jxl-addon.py

Needs the libjxl and libjpeg-turbo headers.  Either install them:
    sudo apt install libjxl-dev libjpeg-turbo8-dev
or, without root, extract the .deb files and point the script at them:
    ZALO_JXL_SYSROOT=/path/to/sysroot python3 generate-jxl-addon.py
"""

import os, json, shutil, subprocess, sys, tempfile, glob

REPO = os.path.dirname(os.path.abspath(__file__))
DEST = os.path.join(REPO, "native", "nativelibs", "zjxl", "build", "linux_x64")

# Electron 43.3.0 embeds Node 24; keep this in step with ELECTRON_VERSION in start.sh.
ELECTRON_TARGET = "43.3.0"
NODE_ADDON_API = "8.9.1"
NODE_GYP = "12.4.0"

# A fixed /tmp path is world-writable and guessable, so another local user could
# pre-seed the sources we are about to compile.  mkdtemp gives us a private dir.
BUILD_DIR = tempfile.mkdtemp(prefix="zjxl-build-")

SYSROOT = os.environ.get("ZALO_JXL_SYSROOT", "")
INC_DIRS, LIB_DIRS = [], []
if SYSROOT:
    INC_DIRS.append(os.path.join(SYSROOT, "usr/include"))
    LIB_DIRS.append(os.path.join(SYSROOT, "usr/lib/x86_64-linux-gnu"))
INC_DIRS.append("/usr/include")
LIB_DIRS.append("/usr/lib/x86_64-linux-gnu")

# libjpeg-turbo splits jconfig.h into the multiarch include dir, and jpeglib.h
# pulls it in by bare name -- so both have to be on the include path.
INC_DIRS = INC_DIRS + [os.path.join(d, "x86_64-linux-gnu") for d in list(INC_DIRS)]


def find_first(paths, rel):
    for base in paths:
        p = os.path.join(base, rel)
        if os.path.exists(p):
            return p
    return None


# Preflight: node-gyp's output for a missing header is long and unhelpful, so say it plainly.
missing = []
if not find_first(INC_DIRS, "jxl/decode.h"):
    missing.append("libjxl-dev (jxl/decode.h)")
if not find_first(INC_DIRS, "jpeglib.h"):
    missing.append("libjpeg-turbo8-dev (jpeglib.h)")
if missing:
    print("ERROR: missing build dependencies: " + ", ".join(missing), file=sys.stderr)
    print("  sudo apt install libjxl-dev libjpeg-turbo8-dev", file=sys.stderr)
    print("  (or set ZALO_JXL_SYSROOT to a directory with the extracted .debs)", file=sys.stderr)
    sys.exit(1)

jxl_inc = os.path.dirname(os.path.dirname(find_first(INC_DIRS, "jxl/decode.h")))
jpeg_inc = os.path.dirname(find_first(INC_DIRS, "jpeglib.h"))


def find_shared(stem):
    """Absolute path of a real lib<stem>.so*, not the -dev symlink.

    The -dev package's bare `libjxl.so` symlink points at the runtime package's
    versioned file, which lives outside an extracted sysroot -- so `-ljxl` finds
    a dangling link and fails.  Linking the versioned file by path works and
    still records the right SONAME.
    """
    for base in LIB_DIRS + ["/lib/x86_64-linux-gnu"]:
        for cand in sorted(glob.glob(os.path.join(base, f"lib{stem}.so*"))):
            real = os.path.realpath(cand)
            if os.path.isfile(real):
                return real
    return None


libjxl_so = find_shared("jxl")
libjxlthreads_so = find_shared("jxl_threads")
# Ubuntu's libjpeg.a is built without -fPIC, so it cannot be linked into a shared
# object.  Link libjpeg dynamically instead and ship it beside the addon.
libjpeg_so = find_shared("jpeg")
if not libjxl_so or not libjxlthreads_so or not libjpeg_so:
    print("ERROR: libjxl / libjxl_threads / libjpeg shared objects not found.", file=sys.stderr)
    sys.exit(1)

pkg = {
    "name": "zjxl-linux",
    "version": "1.0.0",
    "private": True,
    "scripts": {"build": "node-gyp configure build"},
    "gypfile": True,
    "dependencies": {"node-addon-api": NODE_ADDON_API, "node-gyp": NODE_GYP},
}
with open(f"{BUILD_DIR}/package.json", "w") as f:
    json.dump(pkg, f, indent=2)

gyp = {
    "targets": [{
        "target_name": "jxl",
        "sources": ["src/jxl_addon.cc"],
        "include_dirs": [
            "<(module_root_dir)/node_modules/node-addon-api",
            jxl_inc,
            jpeg_inc,
            os.path.join(jpeg_inc, "x86_64-linux-gnu"),
        ],
        "dependencies": ["<!(node -p \"require('node-addon-api').gyp\")"],
        "cflags_cc": ["-std=c++17", "-fexceptions", "-fvisibility=hidden", "-O2"],
        "cflags_cc!": ["-fno-exceptions"],
        "defines": ["NAPI_CPP_EXCEPTIONS"],
        "libraries": [
            libjxl_so,
            libjxlthreads_so,
            libjpeg_so,
            "-Wl,--exclude-libs,ALL",
            # DT_RPATH, not DT_RUNPATH: only the old tag is inherited when
            # resolving a dependency's own dependencies, and libjxl.so pulls in
            # libjxl_cms / libhwy / brotli, which we also ship in this directory.
            # With DT_RUNPATH those transitive loads fall back to the system
            # copies and break on any distro without a matching libjxl.
            "-Wl,--disable-new-dtags",
            "-Wl,-rpath,'$$ORIGIN'",
        ],
    }]
}
with open(f"{BUILD_DIR}/binding.gyp", "w") as f:
    json.dump(gyp, f, indent=2)

os.makedirs(f"{BUILD_DIR}/src", exist_ok=True)

src = r'''
#include <napi.h>
#include <jxl/decode.h>
#include <jxl/decode_cxx.h>
#include <jxl/encode.h>
#include <jxl/encode_cxx.h>
#include <jxl/thread_parallel_runner.h>
#include <jxl/thread_parallel_runner_cxx.h>
#include <jpeglib.h>

#include <csetjmp>
#include <cstring>
#include <fstream>
#include <memory>
#include <string>
#include <vector>

// Mirrors the status enum the bundles compare against (`SUCCESS_STATUS = 1`).
static const int kFailure = 0;
static const int kSuccess = 1;

struct Image {
    std::vector<uint8_t> rgb;   // tightly packed RGB8
    uint32_t width = 0;
    uint32_t height = 0;
};

// ---------------------------------------------------------------- JXL decode

// Alpha is composited over white rather than dropped: JPEG has no alpha, and
// dropping it turns transparent regions into whatever garbage the encoder left
// in those channels.
static bool DecodeJxl(const uint8_t* data, size_t size, Image& out, std::string& err) {
    auto dec = JxlDecoderMake(nullptr);
    if (!dec) { err = "JxlDecoderCreate failed"; return false; }

    auto runner = JxlThreadParallelRunnerMake(nullptr, JxlThreadParallelRunnerDefaultNumWorkerThreads());
    if (JXL_DEC_SUCCESS != JxlDecoderSetParallelRunner(dec.get(), JxlThreadParallelRunner, runner.get())) {
        err = "JxlDecoderSetParallelRunner failed"; return false;
    }
    if (JXL_DEC_SUCCESS != JxlDecoderSubscribeEvents(dec.get(), JXL_DEC_BASIC_INFO | JXL_DEC_FULL_IMAGE)) {
        err = "JxlDecoderSubscribeEvents failed"; return false;
    }
    if (JXL_DEC_SUCCESS != JxlDecoderSetInput(dec.get(), data, size)) {
        err = "JxlDecoderSetInput failed"; return false;
    }
    JxlDecoderCloseInput(dec.get());

    JxlBasicInfo info;
    bool has_alpha = false;
    std::vector<uint8_t> raw;
    JxlPixelFormat fmt = {3, JXL_TYPE_UINT8, JXL_NATIVE_ENDIAN, 0};

    for (;;) {
        JxlDecoderStatus st = JxlDecoderProcessInput(dec.get());
        if (st == JXL_DEC_ERROR) { err = "JXL decode error"; return false; }
        if (st == JXL_DEC_NEED_MORE_INPUT) { err = "Truncated JXL input"; return false; }

        if (st == JXL_DEC_BASIC_INFO) {
            if (JXL_DEC_SUCCESS != JxlDecoderGetBasicInfo(dec.get(), &info)) {
                err = "JxlDecoderGetBasicInfo failed"; return false;
            }
            if (info.xsize == 0 || info.ysize == 0) { err = "Zero-sized JXL image"; return false; }
            // Guard against a hostile header claiming a size that would overflow
            // the allocation maths below (4 channels * w * h).
            const uint64_t px = (uint64_t)info.xsize * (uint64_t)info.ysize;
            if (px > (uint64_t)200000000) { err = "JXL image too large"; return false; }
            has_alpha = info.alpha_bits > 0;
            fmt.num_channels = has_alpha ? 4 : 3;
            out.width = info.xsize;
            out.height = info.ysize;
            continue;
        }

        if (st == JXL_DEC_NEED_IMAGE_OUT_BUFFER) {
            size_t need = 0;
            if (JXL_DEC_SUCCESS != JxlDecoderImageOutBufferSize(dec.get(), &fmt, &need)) {
                err = "JxlDecoderImageOutBufferSize failed"; return false;
            }
            raw.resize(need);
            if (JXL_DEC_SUCCESS != JxlDecoderSetImageOutBuffer(dec.get(), &fmt, raw.data(), raw.size())) {
                err = "JxlDecoderSetImageOutBuffer failed"; return false;
            }
            continue;
        }

        if (st == JXL_DEC_FULL_IMAGE) continue;
        if (st == JXL_DEC_SUCCESS) break;
    }

    if (raw.empty()) { err = "JXL produced no pixels"; return false; }

    const size_t n = (size_t)out.width * out.height;
    out.rgb.resize(n * 3);
    if (has_alpha) {
        for (size_t i = 0; i < n; ++i) {
            const uint32_t a = raw[i * 4 + 3];
            for (int c = 0; c < 3; ++c) {
                const uint32_t v = raw[i * 4 + c];
                out.rgb[i * 3 + c] = (uint8_t)((v * a + 255u * (255u - a) + 127u) / 255u);
            }
        }
    } else {
        std::memcpy(out.rgb.data(), raw.data(), n * 3);
    }
    return true;
}

// ---------------------------------------------------------------- resampling

// Box filter when shrinking (what every caller here does -- thumbnails), bilinear
// when growing. Separable, two passes, RGB8 in and out.
static void ResizeRgb(const Image& in, uint32_t dw, uint32_t dh, Image& out) {
    out.width = dw; out.height = dh;
    out.rgb.assign((size_t)dw * dh * 3, 0);
    if (in.width == 0 || in.height == 0 || dw == 0 || dh == 0) return;
    if (dw == in.width && dh == in.height) { out.rgb = in.rgb; return; }

    // horizontal pass into a dw x in.height intermediate
    std::vector<uint8_t> mid((size_t)dw * in.height * 3);
    const double xr = (double)in.width / dw;
    for (uint32_t y = 0; y < in.height; ++y) {
        const uint8_t* srow = &in.rgb[(size_t)y * in.width * 3];
        uint8_t* drow = &mid[(size_t)y * dw * 3];
        for (uint32_t x = 0; x < dw; ++x) {
            if (xr > 1.0) {
                uint32_t x0 = (uint32_t)(x * xr);
                uint32_t x1 = (uint32_t)((x + 1) * xr);
                if (x1 <= x0) x1 = x0 + 1;
                if (x1 > in.width) x1 = in.width;
                uint32_t acc[3] = {0, 0, 0};
                for (uint32_t sx = x0; sx < x1; ++sx)
                    for (int c = 0; c < 3; ++c) acc[c] += srow[sx * 3 + c];
                const uint32_t cnt = x1 - x0;
                for (int c = 0; c < 3; ++c) drow[x * 3 + c] = (uint8_t)(acc[c] / cnt);
            } else {
                const double fx = (x + 0.5) * xr - 0.5;
                int x0 = (int)(fx < 0 ? 0 : fx);
                int x1 = x0 + 1;
                if (x1 > (int)in.width - 1) x1 = in.width - 1;
                const double t = fx - x0 < 0 ? 0 : fx - x0;
                for (int c = 0; c < 3; ++c)
                    drow[x * 3 + c] = (uint8_t)(srow[x0 * 3 + c] * (1 - t) + srow[x1 * 3 + c] * t + 0.5);
            }
        }
    }

    // vertical pass
    const double yr = (double)in.height / dh;
    for (uint32_t y = 0; y < dh; ++y) {
        uint8_t* drow = &out.rgb[(size_t)y * dw * 3];
        if (yr > 1.0) {
            uint32_t y0 = (uint32_t)(y * yr);
            uint32_t y1 = (uint32_t)((y + 1) * yr);
            if (y1 <= y0) y1 = y0 + 1;
            if (y1 > in.height) y1 = in.height;
            for (uint32_t x = 0; x < dw; ++x) {
                uint32_t acc[3] = {0, 0, 0};
                for (uint32_t sy = y0; sy < y1; ++sy)
                    for (int c = 0; c < 3; ++c) acc[c] += mid[((size_t)sy * dw + x) * 3 + c];
                const uint32_t cnt = y1 - y0;
                for (int c = 0; c < 3; ++c) drow[x * 3 + c] = (uint8_t)(acc[c] / cnt);
            }
        } else {
            const double fy = (y + 0.5) * yr - 0.5;
            int y0 = (int)(fy < 0 ? 0 : fy);
            int y1 = y0 + 1;
            if (y1 > (int)in.height - 1) y1 = in.height - 1;
            const double t = fy - y0 < 0 ? 0 : fy - y0;
            for (uint32_t x = 0; x < dw; ++x)
                for (int c = 0; c < 3; ++c)
                    drow[x * 3 + c] = (uint8_t)(mid[((size_t)y0 * dw + x) * 3 + c] * (1 - t) +
                                                mid[((size_t)y1 * dw + x) * 3 + c] * t + 0.5);
        }
    }
}

// Callers pass -1 for "unconstrained" (index.js normalises 0/undefined to -1).
static void TargetSize(uint32_t sw, uint32_t sh, int ow, int oh, uint32_t& dw, uint32_t& dh) {
    if (ow <= 0 && oh <= 0) { dw = sw; dh = sh; return; }
    if (ow <= 0) { dh = (uint32_t)oh; dw = (uint32_t)((double)sw * oh / sh + 0.5); }
    else if (oh <= 0) { dw = (uint32_t)ow; dh = (uint32_t)((double)sh * ow / sw + 0.5); }
    else { dw = (uint32_t)ow; dh = (uint32_t)oh; }
    if (dw == 0) dw = 1;
    if (dh == 0) dh = 1;
}

// --------------------------------------------------------------- JPEG encode

struct JpegErrMgr {
    struct jpeg_error_mgr pub;
    jmp_buf jump;
    char msg[JMSG_LENGTH_MAX];
};

static void JpegErrExit(j_common_ptr cinfo) {
    JpegErrMgr* e = (JpegErrMgr*)cinfo->err;
    (*cinfo->err->format_message)(cinfo, e->msg);
    longjmp(e->jump, 1);
}

static bool EncodeJpeg(const Image& img, int quality, std::vector<uint8_t>& out, std::string& err) {
    if (img.width == 0 || img.height == 0) { err = "Empty image"; return false; }
    if (quality < 1) quality = 1;
    if (quality > 100) quality = 100;

    struct jpeg_compress_struct cinfo;
    JpegErrMgr jerr;
    unsigned char* buf = nullptr;
    unsigned long buflen = 0;

    cinfo.err = jpeg_std_error(&jerr.pub);
    jerr.pub.error_exit = JpegErrExit;
    if (setjmp(jerr.jump)) {
        jpeg_destroy_compress(&cinfo);
        if (buf) free(buf);
        err = std::string("libjpeg: ") + jerr.msg;
        return false;
    }

    jpeg_create_compress(&cinfo);
    jpeg_mem_dest(&cinfo, &buf, &buflen);
    cinfo.image_width = img.width;
    cinfo.image_height = img.height;
    cinfo.input_components = 3;
    cinfo.in_color_space = JCS_RGB;
    jpeg_set_defaults(&cinfo);
    jpeg_set_quality(&cinfo, quality, TRUE);
    jpeg_start_compress(&cinfo, TRUE);
    while (cinfo.next_scanline < cinfo.image_height) {
        JSAMPROW row = (JSAMPROW)&img.rgb[(size_t)cinfo.next_scanline * img.width * 3];
        jpeg_write_scanlines(&cinfo, &row, 1);
    }
    jpeg_finish_compress(&cinfo);
    out.assign(buf, buf + buflen);
    jpeg_destroy_compress(&cinfo);
    free(buf);
    return true;
}

// ---------------------------------------------------------------- JXL encode

static bool EncodeJxlRgba(const uint8_t* rgba, uint32_t w, uint32_t h, float distance,
                          std::vector<uint8_t>& out, std::string& err) {
    auto enc = JxlEncoderMake(nullptr);
    if (!enc) { err = "JxlEncoderCreate failed"; return false; }
    auto runner = JxlThreadParallelRunnerMake(nullptr, JxlThreadParallelRunnerDefaultNumWorkerThreads());
    if (JXL_ENC_SUCCESS != JxlEncoderSetParallelRunner(enc.get(), JxlThreadParallelRunner, runner.get())) {
        err = "JxlEncoderSetParallelRunner failed"; return false;
    }

    JxlBasicInfo info;
    JxlEncoderInitBasicInfo(&info);
    info.xsize = w;
    info.ysize = h;
    info.bits_per_sample = 8;
    info.num_color_channels = 3;
    info.num_extra_channels = 1;
    info.alpha_bits = 8;
    info.uses_original_profile = JXL_FALSE;
    if (JXL_ENC_SUCCESS != JxlEncoderSetBasicInfo(enc.get(), &info)) {
        err = "JxlEncoderSetBasicInfo failed"; return false;
    }

    JxlColorEncoding color = {};
    JxlColorEncodingSetToSRGB(&color, JXL_FALSE);
    if (JXL_ENC_SUCCESS != JxlEncoderSetColorEncoding(enc.get(), &color)) {
        err = "JxlEncoderSetColorEncoding failed"; return false;
    }

    JxlEncoderFrameSettings* fs = JxlEncoderFrameSettingsCreate(enc.get(), nullptr);
    JxlEncoderSetFrameDistance(fs, distance);

    JxlPixelFormat fmt = {4, JXL_TYPE_UINT8, JXL_NATIVE_ENDIAN, 0};
    if (JXL_ENC_SUCCESS != JxlEncoderAddImageFrame(fs, &fmt, rgba, (size_t)w * h * 4)) {
        err = "JxlEncoderAddImageFrame failed"; return false;
    }
    JxlEncoderCloseInput(enc.get());

    out.resize(65536);
    uint8_t* next = out.data();
    size_t avail = out.size();
    for (;;) {
        JxlEncoderStatus st = JxlEncoderProcessOutput(enc.get(), &next, &avail);
        if (st == JXL_ENC_NEED_MORE_OUTPUT) {
            const size_t used = next - out.data();
            out.resize(out.size() * 2);
            next = out.data() + used;
            avail = out.size() - used;
            continue;
        }
        if (st == JXL_ENC_ERROR) { err = "JxlEncoderProcessOutput failed"; return false; }
        out.resize(next - out.data());
        return true;
    }
}

// ------------------------------------------------------------- input helpers

static bool ReadFileBytes(const std::string& path, std::vector<uint8_t>& out, std::string& err) {
    std::ifstream f(path, std::ios::binary | std::ios::ate);
    if (!f) { err = "Cannot open " + path; return false; }
    const std::streamsize n = f.tellg();
    if (n <= 0) { err = "Empty file " + path; return false; }
    f.seekg(0, std::ios::beg);
    out.resize((size_t)n);
    if (!f.read((char*)out.data(), n)) { err = "Cannot read " + path; return false; }
    return true;
}

static bool GetBuffer(const Napi::Object& opts, const char* key, std::vector<uint8_t>& out) {
    if (!opts.Has(key)) return false;
    Napi::Value v = opts.Get(key);
    if (v.IsBuffer()) {
        auto b = v.As<Napi::Buffer<uint8_t>>();
        out.assign(b.Data(), b.Data() + b.Length());
        return true;
    }
    if (v.IsTypedArray()) {
        auto t = v.As<Napi::TypedArray>();
        const uint8_t* p = (const uint8_t*)t.ArrayBuffer().Data() + t.ByteOffset();
        out.assign(p, p + t.ByteLength());
        return true;
    }
    return false;
}

static int GetInt(const Napi::Object& o, const char* key, int dflt) {
    if (!o.Has(key)) return dflt;
    Napi::Value v = o.Get(key);
    if (!v.IsNumber()) return dflt;
    return v.As<Napi::Number>().Int32Value();
}

// ------------------------------------------------------------- async workers

// One entry of a jxlDecompressMulti result.
struct Out {
    std::vector<uint8_t> data;
    uint32_t width = 0;
    uint32_t height = 0;
};

class JobWorker : public Napi::AsyncWorker {
public:
    JobWorker(Napi::Function cb) : Napi::AsyncWorker(cb) {}

    void OnOK() override {
        Napi::Env env = Env();
        Napi::HandleScope scope(env);
        Callback().Call({env.Null(), Result(env), Napi::Number::New(env, kSuccess)});
    }

    void OnError(const Napi::Error& e) override {
        Napi::Env env = Env();
        Napi::HandleScope scope(env);
        Callback().Call({e.Value(), env.Null(), Napi::Number::New(env, kFailure)});
    }

    virtual Napi::Value Result(Napi::Env env) = 0;
};

// Shared payload: raw JXL bytes plus the requested output sizes.
struct DecodeJob {
    std::vector<uint8_t> input;
    int quality = 90;
    std::vector<std::pair<int, int>> sizes;   // (outputWidth, outputHeight), -1 = free
};

class DecodeWorker : public JobWorker {
public:
    DecodeWorker(Napi::Function cb, DecodeJob job, bool multi)
        : JobWorker(cb), job_(std::move(job)), multi_(multi) {}

    void Execute() override {
        Image src;
        std::string err;
        if (!DecodeJxl(job_.input.data(), job_.input.size(), src, err)) { SetError(err); return; }

        for (const auto& s : job_.sizes) {
            uint32_t dw, dh;
            TargetSize(src.width, src.height, s.first, s.second, dw, dh);
            Out o;
            if (dw == src.width && dh == src.height) {
                if (!EncodeJpeg(src, job_.quality, o.data, err)) { SetError(err); return; }
            } else {
                Image scaled;
                ResizeRgb(src, dw, dh, scaled);
                if (!EncodeJpeg(scaled, job_.quality, o.data, err)) { SetError(err); return; }
            }
            o.width = dw;
            o.height = dh;
            outs_.push_back(std::move(o));
        }
    }

    Napi::Value Result(Napi::Env env) override {
        if (!multi_) {
            const Out& o = outs_[0];
            return Napi::Buffer<uint8_t>::Copy(env, o.data.data(), o.data.size());
        }
        Napi::Array arr = Napi::Array::New(env, outs_.size());
        for (size_t i = 0; i < outs_.size(); ++i) {
            Napi::Object e = Napi::Object::New(env);
            e.Set("data", Napi::Buffer<uint8_t>::Copy(env, outs_[i].data.data(), outs_[i].data.size()));
            e.Set("width", Napi::Number::New(env, outs_[i].width));
            e.Set("height", Napi::Number::New(env, outs_[i].height));
            arr.Set((uint32_t)i, e);
        }
        return arr;
    }

private:
    DecodeJob job_;
    bool multi_;
    std::vector<Out> outs_;
};

class InfoWorker : public JobWorker {
public:
    InfoWorker(Napi::Function cb, std::vector<uint8_t> input)
        : JobWorker(cb), input_(std::move(input)) {}

    void Execute() override {
        std::string err;
        Image img;
        if (!DecodeJxl(input_.data(), input_.size(), img, err)) { SetError(err); return; }
        w_ = img.width; h_ = img.height;
    }

    Napi::Value Result(Napi::Env env) override {
        Napi::Object o = Napi::Object::New(env);
        o.Set("width", Napi::Number::New(env, w_));
        o.Set("height", Napi::Number::New(env, h_));
        o.Set("hasAnimation", Napi::Boolean::New(env, false));
        return o;
    }

private:
    std::vector<uint8_t> input_;
    uint32_t w_ = 0, h_ = 0;
};

class EncodeWorker : public JobWorker {
public:
    EncodeWorker(Napi::Function cb, std::vector<uint8_t> rgba, uint32_t w, uint32_t h,
                 int limit, bool from_jxl, int dw, int dh)
        : JobWorker(cb), in_(std::move(rgba)), w_(w), h_(h), limit_(limit),
          from_jxl_(from_jxl), dw_(dw), dh_(dh) {}

    void Execute() override {
        std::string err;
        Image img;

        if (from_jxl_) {
            if (!DecodeJxl(in_.data(), in_.size(), img, err)) { SetError(err); return; }
            uint32_t tw, th;
            TargetSize(img.width, img.height, dw_, dh_, tw, th);
            if (tw != img.width || th != img.height) {
                Image scaled;
                ResizeRgb(img, tw, th, scaled);
                img = std::move(scaled);
            }
        } else {
            if (in_.size() < (size_t)w_ * h_ * 4) { SetError("bitmap buffer too small"); return; }
            img.width = w_;
            img.height = h_;
            img.rgb.resize((size_t)w_ * h_ * 3);
            for (size_t i = 0, n = (size_t)w_ * h_; i < n; ++i) {
                const uint32_t a = in_[i * 4 + 3];
                for (int c = 0; c < 3; ++c) {
                    const uint32_t v = in_[i * 4 + c];
                    img.rgb[i * 3 + c] = (uint8_t)((v * a + 255u * (255u - a) + 127u) / 255u);
                }
            }
        }

        // EncodeJxlRgba wants RGBA; re-expand with an opaque alpha channel.
        std::vector<uint8_t> rgba((size_t)img.width * img.height * 4);
        for (size_t i = 0, n = (size_t)img.width * img.height; i < n; ++i) {
            rgba[i * 4 + 0] = img.rgb[i * 3 + 0];
            rgba[i * 4 + 1] = img.rgb[i * 3 + 1];
            rgba[i * 4 + 2] = img.rgb[i * 3 + 2];
            rgba[i * 4 + 3] = 255;
        }

        float distance = 1.0f;
        if (!EncodeJxlRgba(rgba.data(), img.width, img.height, distance, out_, err)) { SetError(err); return; }

        // resizeJxlLimit: shrink quality until the result fits `limit` bytes.
        for (int i = 0; limit_ > 0 && (int)out_.size() > limit_ && i < 6; ++i) {
            distance += 1.5f;
            std::vector<uint8_t> retry;
            if (!EncodeJxlRgba(rgba.data(), img.width, img.height, distance, retry, err)) break;
            out_.swap(retry);
        }
    }

    Napi::Value Result(Napi::Env env) override {
        return Napi::Buffer<uint8_t>::Copy(env, out_.data(), out_.size());
    }

private:
    std::vector<uint8_t> in_;
    uint32_t w_, h_;
    int limit_;
    bool from_jxl_;
    int dw_, dh_;
    std::vector<uint8_t> out_;
};

// ------------------------------------------------------------------ bindings

// Every export takes (optionsObject, callback) and reports through the callback,
// so an argument mistake must not throw synchronously into the bundle's promise
// wrapper -- it reports failure the same way a decode error would.
static Napi::Value Fail(Napi::Env env, Napi::Function cb, const std::string& msg) {
    cb.Call({Napi::String::New(env, msg), env.Null(), Napi::Number::New(env, kFailure)});
    return env.Undefined();
}

static bool Prologue(const Napi::CallbackInfo& info, Napi::Object& opts, Napi::Function& cb) {
    if (info.Length() < 2 || !info[0].IsObject() || !info[1].IsFunction()) return false;
    opts = info[0].As<Napi::Object>();
    cb = info[1].As<Napi::Function>();
    return true;
}

static Napi::Value JxlToJpeg(const Napi::CallbackInfo& info) {
    Napi::Env env = info.Env();
    Napi::Object opts; Napi::Function cb;
    if (!Prologue(info, opts, cb)) {
        Napi::TypeError::New(env, "jxlToJpeg(options, callback)").ThrowAsJavaScriptException();
        return env.Undefined();
    }
    DecodeJob job;
    if (!GetBuffer(opts, "buffer", job.input)) return Fail(env, cb, "jxlToJpeg: missing buffer");
    job.quality = GetInt(opts, "quality", 90);
    job.sizes.push_back({GetInt(opts, "outputWidth", -1), GetInt(opts, "outputHeight", -1)});
    (new DecodeWorker(cb, std::move(job), false))->Queue();
    return env.Undefined();
}

static Napi::Value JxlDecompressMulti(const Napi::CallbackInfo& info) {
    Napi::Env env = info.Env();
    Napi::Object opts; Napi::Function cb;
    if (!Prologue(info, opts, cb)) {
        Napi::TypeError::New(env, "jxlDecompressMulti(options, callback)").ThrowAsJavaScriptException();
        return env.Undefined();
    }

    DecodeJob job;
    if (!GetBuffer(opts, "buffer", job.input)) {
        if (!opts.Has("localPath") || !opts.Get("localPath").IsString())
            return Fail(env, cb, "jxlDecompressMulti: need buffer or localPath");
        std::string err;
        if (!ReadFileBytes(opts.Get("localPath").As<Napi::String>().Utf8Value(), job.input, err))
            return Fail(env, cb, err);
    }
    job.quality = GetInt(opts, "quality", 90);

    // `quality` arrives as 0..1 from the renderer (it passes 0.9) but libjpeg
    // wants 1..100; accept either.
    if (opts.Has("quality") && opts.Get("quality").IsNumber()) {
        const double q = opts.Get("quality").As<Napi::Number>().DoubleValue();
        job.quality = (q > 0.0 && q <= 1.0) ? (int)(q * 100.0 + 0.5) : (int)q;
    }

    // A caller may ask for several sizes at once; that is what "Multi" means.
    // With no list we return one entry, which is what decodeLocalImage reads.
    if (opts.Has("sizes") && opts.Get("sizes").IsArray()) {
        Napi::Array a = opts.Get("sizes").As<Napi::Array>();
        for (uint32_t i = 0; i < a.Length(); ++i) {
            Napi::Value v = a.Get(i);
            if (!v.IsObject()) continue;
            Napi::Object s = v.As<Napi::Object>();
            job.sizes.push_back({GetInt(s, "outputWidth", GetInt(s, "width", -1)),
                                 GetInt(s, "outputHeight", GetInt(s, "height", -1))});
        }
    }
    if (job.sizes.empty())
        job.sizes.push_back({GetInt(opts, "outputWidth", -1), GetInt(opts, "outputHeight", -1)});

    (new DecodeWorker(cb, std::move(job), true))->Queue();
    return env.Undefined();
}

static Napi::Value GetJxlInfo(const Napi::CallbackInfo& info) {
    Napi::Env env = info.Env();
    Napi::Object opts; Napi::Function cb;
    if (!Prologue(info, opts, cb)) {
        Napi::TypeError::New(env, "getJxlInfo(options, callback)").ThrowAsJavaScriptException();
        return env.Undefined();
    }
    std::vector<uint8_t> buf;
    if (!GetBuffer(opts, "buffer", buf)) return Fail(env, cb, "getJxlInfo: missing buffer");
    (new InfoWorker(cb, std::move(buf)))->Queue();
    return env.Undefined();
}

static Napi::Value BitmapToJxl(const Napi::CallbackInfo& info) {
    Napi::Env env = info.Env();
    Napi::Object opts; Napi::Function cb;
    if (!Prologue(info, opts, cb)) {
        Napi::TypeError::New(env, "bitmapToJxl(options, callback)").ThrowAsJavaScriptException();
        return env.Undefined();
    }
    std::vector<uint8_t> buf;
    if (!GetBuffer(opts, "buffer", buf)) return Fail(env, cb, "bitmapToJxl: missing buffer");
    const int w = GetInt(opts, "width", 0), h = GetInt(opts, "height", 0);
    if (w <= 0 || h <= 0) return Fail(env, cb, "bitmapToJxl: bad dimensions");
    (new EncodeWorker(cb, std::move(buf), (uint32_t)w, (uint32_t)h, 0, false, -1, -1))->Queue();
    return env.Undefined();
}

static Napi::Value ResizeJxlImpl(const Napi::CallbackInfo& info, bool with_limit) {
    Napi::Env env = info.Env();
    Napi::Object opts; Napi::Function cb;
    if (!Prologue(info, opts, cb)) {
        Napi::TypeError::New(env, "resizeJxl(options, callback)").ThrowAsJavaScriptException();
        return env.Undefined();
    }
    std::vector<uint8_t> buf;
    if (!GetBuffer(opts, "buffer", buf)) return Fail(env, cb, "resizeJxl: missing buffer");
    const int w = GetInt(opts, "width", -1), h = GetInt(opts, "height", -1);
    const int limit = with_limit ? GetInt(opts, "limit", 0) : 0;
    (new EncodeWorker(cb, std::move(buf), 0, 0, limit, true, w, h))->Queue();
    return env.Undefined();
}

static Napi::Value ResizeJxl(const Napi::CallbackInfo& info) { return ResizeJxlImpl(info, false); }
static Napi::Value ResizeJxlLimit(const Napi::CallbackInfo& info) { return ResizeJxlImpl(info, true); }

static Napi::Value ModuleReady(const Napi::CallbackInfo& info) {
    return Napi::Boolean::New(info.Env(), true);
}

static Napi::Object Init(Napi::Env env, Napi::Object exports) {
    exports.Set("jxlToJpeg", Napi::Function::New(env, JxlToJpeg));
    exports.Set("jxlDecompressMulti", Napi::Function::New(env, JxlDecompressMulti));
    exports.Set("getJxlInfo", Napi::Function::New(env, GetJxlInfo));
    exports.Set("bitmapToJxl", Napi::Function::New(env, BitmapToJxl));
    exports.Set("resizeJxl", Napi::Function::New(env, ResizeJxl));
    exports.Set("resizeJxlLimit", Napi::Function::New(env, ResizeJxlLimit));
    exports.Set("moduleReady", Napi::Function::New(env, ModuleReady));
    return exports;
}

NODE_API_MODULE(jxl, Init)
'''

with open(f"{BUILD_DIR}/src/jxl_addon.cc", "w") as f:
    f.write(src)

build_sh = f"""
set -e
cd "{BUILD_DIR}"
npm install --no-audit --no-fund node-addon-api@{NODE_ADDON_API} node-gyp@{NODE_GYP} >/dev/null
# --no-install pins us to the node-gyp just installed instead of letting npx fetch latest.
npx --no-install node-gyp configure --target={ELECTRON_TARGET} --arch=x64 \
    --dist-url=https://www.electronjs.org/headers build >/dev/null
"""

print(f"[*] Building zjxl for Electron {ELECTRON_TARGET} in {BUILD_DIR} ...")
rc = subprocess.call(["bash", "-c", build_sh])
if rc != 0:
    print("ERROR: build failed; leaving %s in place for inspection." % BUILD_DIR, file=sys.stderr)
    sys.exit(1)

built = f"{BUILD_DIR}/build/Release/jxl.node"
if not os.path.exists(built):
    print("ERROR: %s was not produced." % built, file=sys.stderr)
    sys.exit(1)

os.makedirs(DEST, exist_ok=True)
shutil.copy2(built, os.path.join(DEST, "jxl.node"))

# Ship libjxl next to the addon so the result does not depend on the host distro
# having a matching libjxl -- the same thing upstream does with its macOS dylibs.
BUNDLE = [
    "libjxl.so.0.11", "libjxl_cms.so.0.11", "libjxl_threads.so.0.11",
    "libhwy.so.1", "libbrotlidec.so.1", "libbrotlienc.so.1",
    "libbrotlicommon.so.1", "liblcms2.so.2", "libjpeg.so.8",
]
copied = []
for name in BUNDLE:
    src_lib = None
    for base in LIB_DIRS + ["/lib/x86_64-linux-gnu"]:
        cand = os.path.join(base, name)
        if os.path.exists(cand):
            src_lib = os.path.realpath(cand)
            break
    if not src_lib:
        print(f"WARNING: {name} not found; the addon may fail to load.", file=sys.stderr)
        continue
    shutil.copy2(src_lib, os.path.join(DEST, name))
    copied.append(name)

shutil.rmtree(BUILD_DIR, ignore_errors=True)
print(f"[*] Wrote {DEST}/jxl.node")
print(f"[*] Bundled: {', '.join(copied)}")
