# Synthetic media decoder fixtures

`photo.png`, `photo.jpg`, `photo.webp`, and `photo.jxl` encode the same original 96×64 RGB test pattern (blue/green bands). They contain no account, conversation, external image, or credential data.

Generated locally with FFmpeg using a P6 RGB input, one frame, and respectively the `png`, `mjpeg`, `libwebp`, and `libjxl` encoders. Tests exercise the application decoders; FFmpeg is not a runtime dependency. The native GUI fixture uses the decoded JPEG XL pixels and the source-derived raw message fixtures in `../messaging/media.json`.

Production previews support PNG/JPEG/WebP and a bounded static JPEG XL profile. JXL ancillary container metadata is discarded without expansion; embedded ICC, preview/animation headers, and more than one extra channel are rejected. A source URL is never copied from an account into these fixtures.
