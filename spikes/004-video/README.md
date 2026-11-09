# Video spike

A standalone Rust crate for testing dynamic FFmpeg decode, frame pacing and
offscreen wgpu conversion. Each input gets its own decoder and textures.
Hardware decode is attempted on the host platform, with software fallback.
The RGBA texture is held across pause and the first frame is read back for
verification. There is no window or audio track.

## Build

Use a shared FFmpeg build with libavcodec, libavformat, libavutil and libswscale.
On macOS, use the FFmpeg 8.0.1 source and install pkg-config, libvpx and libtheora
as build prerequisites. The following configuration builds the libraries and
fixture tools used for the spike. See the report for distribution considerations.

```sh
./configure --prefix="$FFMPEG_DIR" \
  --enable-shared --disable-static --disable-gpl --disable-nonfree --disable-version3 \
  --disable-autodetect --enable-videotoolbox --enable-libvpx --enable-libtheora \
  --extra-cflags="$(pkg-config --cflags theoraenc)" \
  --extra-ldflags="$(pkg-config --libs-only-L theoraenc)" \
  --disable-doc --disable-debug --disable-ffplay --disable-everything \
  --enable-protocol=file --enable-demuxer=mov,matroska,ogg \
  --enable-muxer=mp4,mov,matroska,webm,ogg,null --enable-parser=h264,hevc,vp9 \
  --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,vp9_videotoolbox \
  --enable-decoder=h264,hevc,vp9,libvpx_vp9,prores,theora,rawvideo,wrapped_avframe \
  --enable-encoder=h264_videotoolbox,hevc_videotoolbox,libvpx_vp9,libtheora,prores_ks,wrapped_avframe \
  --enable-filter=testsrc2,color,format,scale,setparams --enable-indev=lavfi
make -j8
make install
```

From the native repository root, with `FFMPEG_DIR` exported and pointing to
that prefix, build the spike. On macOS, set `LIBCLANG_PATH` to Xcode's
`Toolchains/XcodeDefault.xctoolchain/usr/lib` directory if bindgen cannot
locate libclang. On Windows and Linux, use shared FFmpeg built with D3D11VA
or VAAPI support respectively, and make the runtime libraries discoverable.

```sh
cargo build --manifest-path spikes/004-video/Cargo.toml --release --locked
```

## Generate fixtures

Use a temporary directory for the generated media. These commands run from
that directory with the spike FFmpeg first on `PATH`.

```sh
ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=30 -t 4 \
  -c:v h264_videotoolbox -allow_sw 1 -b:v 12M -g 30 -colorspace bt709 \
  -color_range tv -f mp4 h264-1080p30.mp4
ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=60 -t 4 \
  -c:v h264_videotoolbox -allow_sw 1 -b:v 12M -g 60 -colorspace bt709 \
  -color_range tv -f mp4 h264-1080p60.m4v
ffmpeg -f lavfi -i testsrc2=size=3840x2160:rate=30 -t 4 \
  -c:v h264_videotoolbox -allow_sw 1 -b:v 12M -g 30 -colorspace bt709 \
  -color_range tv -f mp4 h264-4k30.mp4
ffmpeg -f lavfi -i 'testsrc2=size=1920x1080:rate=30,format=p010le' -t 4 \
  -c:v hevc_videotoolbox -allow_sw 1 -profile:v main10 -b:v 8M -g 30 \
  -colorspace bt2020nc -color_range tv hevc-main10.mov
ffmpeg -f lavfi -i 'testsrc2=size=640x360:rate=30:alpha=80,format=yuva444p10le' \
  -t 2 -c:v prores_ks -profile:v 4 prores-alpha.mov
ffmpeg -f lavfi -i 'testsrc2=size=640x360:rate=30:alpha=80,format=yuva420p' \
  -t 2 -c:v libvpx-vp9 -deadline realtime -cpu-used 6 -auto-alt-ref 0 vp9-alpha.webm
ffmpeg -f lavfi -i testsrc2=size=640x360:rate=30 -t 2 -c:v libtheora theora.ogv
```

## Measure

```sh
kairo-spike-video h264-1080p30.mp4 --looping --seconds 1800 --csv soak.csv
kairo-spike-video h264-1080p30.mp4 h264-1080p30.mp4 h264-1080p30.mp4 --looping --seconds 10
kairo-spike-video h264-1080p30.mp4 --looping --start 0.5 --end 2.75 --rate 2 \
  --pause-at 1.2 --pause-for 0.5 --seek-at 2.1 --seek-to 1.273 --seconds 6
kairo-spike-video h264-1080p30.mp4 --start 0.5 --end 1.5 --rate 0.25 --seconds 6
kairo-spike-video vp9-alpha.webm --expect-alpha
kairo-spike-video prores-alpha.mov --expect-alpha
```

Use `--software` to force software decode for comparison. The CSV reports
completion-to-completion intervals, upload plus GPU wait, time to prepare the
next frame, hardware download for the current frame, wrap count and process RSS.
The reported CPU percentage is process user plus system time divided by elapsed
wall time. RSS is sampled every five seconds. The p99 summaries round up to
0.1 ms. The first frame and the first after a seek or pause reset the frame
interval sample.
