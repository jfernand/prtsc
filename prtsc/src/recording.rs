//! Captures a PipeWire video stream (negotiated via [`crate::screencast`]),
//! encodes it with `openh264`, and muxes it to MP4 - all on one dedicated
//! thread, since PipeWire's mainloop is blocking, not async.

use std::cell::RefCell;
use std::fs::File;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use mp4::{AvcConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig};
use openh264::OpenH264API;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::{BgraSliceU8, RgbaSliceU8, YUVBuffer};
use pipewire as pw;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Pod, Value, object, property};
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::StreamFlags;

/// MP4 track id for our (only, video) track.
const TRACK_ID: u32 = 1;
/// MP4 timescale: units per second used for sample timestamps/durations.
const TIMESCALE: u32 = 1000;

/// Which interleaved pixel layout PipeWire negotiated - both map directly
/// onto an `openh264` slice-wrapper type, so no manual RGB/BGR channel
/// swapping is needed, just picking the matching wrapper at encode time.
#[derive(Clone, Copy)]
enum PixelLayout {
    Bgra,
    Rgba,
}

/// Mutable state shared between the `param_changed` and `process`
/// callbacks. Both run on the PipeWire mainloop thread, so a plain
/// `RefCell` (no `Mutex`) is enough.
struct EncodeState {
    writer: Mp4Writer<File>,
    encoder: Encoder,
    layout: Option<PixelLayout>,
    size: (usize, usize),
    track_added: bool,
    start: Option<Instant>,
    error: Option<String>,
}

/// Runs the capture+encode+mux loop until interrupted (Ctrl-C/SIGTERM),
/// blocking the calling thread. Meant to be driven via
/// `tokio::task::spawn_blocking` from async code.
pub fn record(fd: OwnedFd, node_id: u32, size: (i32, i32), output: &Path) -> Result<(), String> {
    pw::init();

    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(|err| err.to_string())?;

    let weak = main_loop.downgrade();
    let _sig_int = main_loop
        .loop_()
        .add_signal_local(pw::loop_::Signal::INT, move || {
            if let Some(main_loop) = weak.upgrade() {
                main_loop.quit();
            }
        });
    let weak = main_loop.downgrade();
    let _sig_term = main_loop
        .loop_()
        .add_signal_local(pw::loop_::Signal::TERM, move || {
            if let Some(main_loop) = weak.upgrade() {
                main_loop.quit();
            }
        });

    let context = pw::context::ContextRc::new(&main_loop, None).map_err(|err| err.to_string())?;
    let core = context
        .connect_fd_rc(fd, None)
        .map_err(|err| err.to_string())?;

    let file = File::create(output).map_err(|err| err.to_string())?;
    let mp4_config = Mp4Config {
        major_brand: str::parse("isom").unwrap(),
        minor_version: 512,
        compatible_brands: vec![
            str::parse("isom").unwrap(),
            str::parse("iso2").unwrap(),
            str::parse("avc1").unwrap(),
            str::parse("mp41").unwrap(),
        ],
        timescale: TIMESCALE,
    };
    let writer = Mp4Writer::write_start(file, &mp4_config).map_err(|err| err.to_string())?;

    let api = OpenH264API::from_source();
    let encoder_config = EncoderConfig::new();
    let encoder = Encoder::with_api_config(api, encoder_config).map_err(|err| err.to_string())?;

    let state = Rc::new(RefCell::new(EncodeState {
        writer,
        encoder,
        layout: None,
        size: (size.0.max(0) as usize, size.1.max(0) as usize),
        track_added: false,
        start: None,
        error: None,
    }));

    let stream = pw::stream::StreamBox::new(
        &core,
        "prtsc-record",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|err| err.to_string())?;

    let param_state = state.clone();
    let process_state = state.clone();
    let _listener = stream
        .add_local_listener_with_user_data(VideoInfoRaw::default())
        .param_changed(move |_stream, format, id, param| {
            let Some(param) = param else { return };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = pw::spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                return;
            }
            if format.parse(param).is_err() {
                return;
            }
            let layout = match format.format() {
                VideoFormat::BGRA | VideoFormat::BGRx => PixelLayout::Bgra,
                VideoFormat::RGBA | VideoFormat::RGBx => PixelLayout::Rgba,
                _ => return,
            };
            let mut state = param_state.borrow_mut();
            state.layout = Some(layout);
            state.size = (format.size().width as usize, format.size().height as usize);
        })
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let stride = data.chunk().stride().max(0) as usize;
            let Some(bytes) = data.data() else { return };

            let mut state = process_state.borrow_mut();
            if state.error.is_some() {
                return;
            }
            let Some(layout) = state.layout else { return };
            if let Err(err) = encode_frame(&mut state, layout, stride, bytes) {
                state.error = Some(err);
            }
        })
        .register()
        .map_err(|err| err.to_string())?;

    let values = enum_format_pod(size)?;
    let format_pod = Pod::from_bytes(&values).ok_or("failed to build format pod")?;
    let mut params = [format_pod];
    stream
        .connect(
            Direction::Input,
            Some(node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|err| err.to_string())?;

    main_loop.run();

    // Both callbacks above hold their own `Rc` clone of `state` for as long
    // as `_listener` is alive; drop it (and `stream`, which owns it) first
    // so `Rc::into_inner` below actually sees a unique reference.
    drop(_listener);
    drop(stream);

    let state = Rc::into_inner(state)
        .ok_or("encoder state still referenced after main loop exited")?
        .into_inner();
    if let Some(err) = state.error {
        return Err(err);
    }
    let mut writer = state.writer;
    writer.write_end().map_err(|err| err.to_string())?;
    Ok(())
}

/// Builds an SPA `EnumFormat` pod offering the pixel layouts `openh264` can
/// consume directly (`BgraSliceU8`/`RgbaSliceU8`), fixed at the portal's
/// reported stream size - PipeWire negotiates down to one of these.
fn enum_format_pod(size: (i32, i32)) -> Result<Vec<u8>, String> {
    let obj = object!(
        SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        property!(FormatProperties::MediaType, Id, MediaType::Video),
        property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
        ),
        property!(
            FormatProperties::VideoSize,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: size.0.max(1) as u32,
                height: size.1.max(1) as u32,
            }
        ),
    );
    let values = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(obj))
        .map_err(|err| format!("failed to serialize format pod: {err:?}"))?
        .0
        .into_inner();
    Ok(values)
}

/// Converts one raw frame to YUV420, encodes it, and - lazily, once the
/// first frame's SPS/PPS are known - starts the MP4 video track and writes
/// the sample.
fn encode_frame(
    state: &mut EncodeState,
    layout: PixelLayout,
    stride: usize,
    bytes: &[u8],
) -> Result<(), String> {
    let (width, height) = state.size;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let packed = repack_rows(bytes, stride, width * 4, height);

    let yuv = match layout {
        PixelLayout::Bgra => {
            YUVBuffer::from_bgra8_source(BgraSliceU8::new(&packed, (width, height)))
        }
        PixelLayout::Rgba => {
            YUVBuffer::from_rgba8_source(RgbaSliceU8::new(&packed, (width, height)))
        }
    };

    let bitstream = state.encoder.encode(&yuv).map_err(|err| err.to_string())?;

    let mut sps = None;
    let mut pps = None;
    let mut sample_bytes = Vec::new();
    for l in 0..bitstream.num_layers() {
        let Some(layer) = bitstream.layer(l) else {
            continue;
        };
        for n in 0..layer.nal_count() {
            let Some(nal) = layer.nal_unit(n) else {
                continue;
            };
            let Some((nal_type, payload)) = split_annex_b_nal(nal) else {
                continue;
            };
            match nal_type {
                7 => sps = Some(payload.to_vec()),
                8 => pps = Some(payload.to_vec()),
                _ => {
                    sample_bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                    sample_bytes.extend_from_slice(payload);
                }
            }
        }
    }

    if !state.track_added {
        let (Some(sps), Some(pps)) = (sps, pps) else {
            // No parameter sets yet (shouldn't happen on the first frame,
            // but nothing to mux until they arrive).
            return Ok(());
        };
        let avc_config = AvcConfig {
            width: width as u16,
            height: height as u16,
            seq_param_set: sps,
            pic_param_set: pps,
        };
        state
            .writer
            .add_track(&TrackConfig::from(avc_config))
            .map_err(|err| err.to_string())?;
        state.track_added = true;
        state.start = Some(Instant::now());
    }

    if sample_bytes.is_empty() {
        return Ok(());
    }
    let start = *state.start.get_or_insert_with(Instant::now);
    let start_time = start.elapsed().as_millis() as u64;
    let is_sync = bitstream.frame_type() == openh264::encoder::FrameType::IDR;
    state
        .writer
        .write_sample(
            TRACK_ID,
            &Mp4Sample {
                start_time,
                duration: 0,
                rendering_offset: 0,
                is_sync,
                bytes: sample_bytes.into(),
            },
        )
        .map_err(|err| err.to_string())?;

    Ok(())
}

/// Strips a leading Annex-B start code (`00 00 01` or `00 00 00 01`) off
/// `nal`, returning its NAL unit type and the remaining payload.
fn split_annex_b_nal(nal: &[u8]) -> Option<(u8, &[u8])> {
    let payload = if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        return None;
    };
    let nal_type = payload.first()? & 0x1F;
    Some((nal_type, payload))
}

/// Copies `height` rows of `row_bytes` bytes each out of a possibly-padded
/// `src` buffer (row pitch `stride`) into a tightly packed `Vec<u8>` -
/// `openh264`'s slice wrappers require exactly `width * height * 4` bytes
/// with no per-row padding.
fn repack_rows(src: &[u8], stride: usize, row_bytes: usize, height: usize) -> Vec<u8> {
    if stride == row_bytes {
        return src[..row_bytes * height].to_vec();
    }
    let mut out = Vec::with_capacity(row_bytes * height);
    for row in 0..height {
        let start = row * stride;
        out.extend_from_slice(&src[start..start + row_bytes]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises `encode_frame` directly with synthetic frames - bypassing
    /// PipeWire/the portal entirely - to verify the NAL parsing, SPS/PPS
    /// extraction, and MP4 muxing logic in isolation. Confirms the result
    /// by reading it back with the `mp4` crate's own reader.
    #[test]
    fn encodes_synthetic_frames_to_a_readable_mp4() {
        const WIDTH: usize = 64;
        const HEIGHT: usize = 64;
        const FRAME_COUNT: usize = 5;

        let dir = std::env::temp_dir();
        let path = dir.join(format!("prtsc-test-{}.mp4", std::process::id()));

        let file = File::create(&path).expect("create temp file");
        let mp4_config = Mp4Config {
            major_brand: str::parse("isom").unwrap(),
            minor_version: 512,
            compatible_brands: vec![str::parse("isom").unwrap(), str::parse("avc1").unwrap()],
            timescale: TIMESCALE,
        };
        let writer = Mp4Writer::write_start(file, &mp4_config).expect("write_start");

        let api = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, EncoderConfig::new()).expect("build encoder");

        let mut state = EncodeState {
            writer,
            encoder,
            layout: Some(PixelLayout::Bgra),
            size: (WIDTH, HEIGHT),
            track_added: false,
            start: None,
            error: None,
        };

        for frame in 0..FRAME_COUNT {
            // A shifting solid color per frame - not a realistic screen
            // capture, but enough non-trivial pixel data to exercise the
            // encoder rather than a degenerate all-zero buffer.
            let shade = (frame * 40) as u8;
            let pixels = vec![shade; WIDTH * HEIGHT * 4];
            encode_frame(&mut state, PixelLayout::Bgra, WIDTH * 4, &pixels).expect("encode_frame");
        }

        assert!(state.track_added, "no track was added - SPS/PPS never seen");
        state.writer.write_end().expect("write_end");

        let file = std::fs::File::open(&path).expect("reopen mp4");
        let size = file.metadata().expect("metadata").len();
        let mp4 = mp4::Mp4Reader::read_header(std::io::BufReader::new(file), size)
            .expect("mp4 file failed to parse as valid MP4");

        assert_eq!(mp4.tracks().len(), 1, "expected exactly one track");
        let track = mp4.tracks().values().next().unwrap();
        assert_eq!(track.track_type().unwrap(), mp4::TrackType::Video);
        assert!(
            mp4.sample_count(1).unwrap() >= 1,
            "expected at least one sample to have been written"
        );

        let _ = std::fs::remove_file(&path);
    }
}
