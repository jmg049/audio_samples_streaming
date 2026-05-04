<div align="center">

# AudioSamples Streaming

## Chunk-based audio streaming in Rust using [`audio_samples`](https://docs.rs/audio_samples)

<img src="logo.png" title="AudioSamples Logo -- Ferrous' Mustachioed Cousin From East Berlin, Eisenhaltig" width="200"/>

[![Crates.io][crate-img]][crate] [![Docs.rs][docs-img]][docs] [![License: MIT][license-img]][license]
</div>

---

Chunk-based audio streaming for Rust. Reads and writes [`AudioSamples`](https://docs.rs/audio_samples) buffers through a common trait interface; WAV files, FLAC files, hardware devices, rodio, and async runtimes all speak the same language.

## The AudioSamples Ecosystem

```text
audio_samples
├── core types, signal generation, DSP, analysis
├── audio_samples_io
│   └── file I/O: read and write WAV and FLAC
├── audio_samples_python
│   └── python bindings via PyO3
├── audio_samples_streaming
│   └── streaming pipeline, device I/O, rodio, async
└── audio_samples_ml
    └── STT and TTS integrations
```

| Crate | What it provides | Start here if... |
|---|---|---|
| [`audio_samples`](https://crates.io/crates/audio_samples) | `AudioSamples<T>`, core types, signal generation, DSP, analysis | You need in-memory audio representations or signal processing primitives |
| [`audio_samples_io`](https://crates.io/crates/audio_samples_io) | Read and write WAV and FLAC files | You need to load or save audio files with minimal setup |
| [`audio_samples_python`](https://crates.io/crates/audio_samples_python) | Python bindings via PyO3 and NumPy interop | You want to use the library from Python or integrate with Python workflows |
| [`audio_samples_streaming`](https://crates.io/crates/audio_samples_streaming) | Chunk-based streaming, real-time device I/O, async pipelines | You need real-time audio processing or streaming pipelines |
| [`audio_samples_ml`](https://crates.io/crates/audio_samples_ml) | Speech-to-text (STT) and text-to-speech (TTS) integrations | You want to integrate transcription or synthesis into your audio pipeline |

## Core concepts

Two traits drive everything:

- **`AudioStream`** — pull source. `fill_chunk(&mut buffer)` returns `Ok(Some(n))` per chunk, `Ok(None)` when exhausted.
- **`AudioSink`** — push destination. `write_chunk(&buffer)`, then `finalize()` when done.

A pre-allocated `AudioSamples` buffer is passed to every call. No per-chunk allocation.

## Sources and sinks

| Type | Direction | Feature |
|------|-----------|---------|
| `WavFileStream` | source | `wav` |
| `WavFileSink` | sink | `wav` |
| `FlacFileStream` | source | `flac` |
| `DeviceCapture` | source | `device` |
| `DevicePlayback` | sink | `device` |
| `RodioSource` | source adapter | `rodio` |
| `AsyncAudioStream` | stream adapter | `async` |

## Quick start

**WAV to WAV** — read a WAV file chunk by chunk and write it out, optionally processing each chunk in between:

```rust
use audio_samples::AudioSamples;
use audio_samples_streaming::{WavFileStream, WavFileSink, pipeline};
use std::num::{NonZeroU32, NonZeroUsize};

let mut source = WavFileStream::<_, f32>::open("input.wav")?;
let mut sink   = WavFileSink::<_, f32>::create("output.wav", source.num_channels(), source.sample_rate())?;

let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
    NonZeroU32::new(source.num_channels() as u32).unwrap(),
    NonZeroUsize::new(1024).unwrap(),
    NonZeroU32::new(source.sample_rate()).unwrap(),
);

pipeline::run(&mut source, &mut sink, &mut buffer)?;
sink.finalize()?;
```

**Choosing output bit depth** — use `create_typed` when you need 24-bit or 32-bit integer output:

```rust
use audio_samples_streaming::WavFileSink;
use audio_samples_io::ValidatedSampleType;

let sink = WavFileSink::<_, f32>::create_typed("out.wav", 2, 48000, ValidatedSampleType::I24)?;
```

**Device playback** — stream a WAV file to speakers with a 10 ms target period:

```rust
use audio_samples_streaming::{WavFileStream, DevicePlayback, pipeline};
use std::time::Duration;

let mut source = WavFileStream::<_, f32>::open("input.wav")?;
let mut sink   = DevicePlayback::from_default_output(Some(Duration::from_millis(10)))?;

let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
    NonZeroU32::new(source.num_channels() as u32).unwrap(),
    NonZeroUsize::new(1024).unwrap(),
    NonZeroU32::new(source.sample_rate()).unwrap(),
);

pipeline::run(&mut source, &mut sink, &mut buffer)?;
sink.finalize()?;
```

**Device capture** — record the microphone to a WAV file using device-reported format:

```rust
use audio_samples_streaming::{DeviceCapture, WavFileSink, AudioStream, AudioSink};
use std::time::Duration;

let mut source = DeviceCapture::from_default_input(Some(Duration::from_millis(10)))?;
let mut sink   = WavFileSink::<_, f32>::create("capture.wav", source.channels(), source.sample_rate())?;

let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
    NonZeroU32::new(source.channels() as u32).unwrap(),
    NonZeroUsize::new(1024).unwrap(),
    NonZeroU32::new(source.sample_rate()).unwrap(),
);

while let Some(_) = source.fill_chunk(&mut buffer)? {
    sink.write_chunk(&buffer)?;
}
sink.finalize()?;
```

## Device I/O and low-latency use

Both `DeviceCapture` and `DevicePlayback` are built for low-latency work:

- **Lock-free data path** — rtrb SPSC ring buffer between the cpal callback and the application thread. No locks cross the real-time boundary.
- **Condvar notification** — the application thread sleeps on a condvar; the RT callback wakes it immediately. No spin-sleep.
- **Period size control** — pass `Some(Duration)` to request a target callback period. `None` uses the driver default. Shorter periods mean lower latency but require more stable scheduling.
- **RT thread priority** — on Linux, the callback thread attempts `SCHED_FIFO` priority on its first invocation. This silently fails without `CAP_SYS_NICE` or rtkit access. For production, add the audio group to `/etc/security/limits.d/audio.conf`:

  ```
  @audio - rtprio 95
  @audio - memlock unlimited
  ```

- **Xrun counting** — `.xruns()` returns the cumulative count of capture overflows and playback underruns. Non-zero means lost audio; increase the period or reduce processing load to eliminate them.

### Typical period targets

| `target_latency` | Suitability |
|-----------------|------------|
| `None` | General use, file playback |
| `from_millis(10)` | Applications, monitoring |
| `from_millis(5)` | Live processing, plugin hosts |
| `from_millis(2)` | Professional; requires RT scheduling and JACK/PipeWire |

## FLAC

`FlacFileStream` decodes the full file into memory at construction, then streams from that buffer with no per-chunk allocation. This is appropriate for most file sizes; a 5-minute stereo 24-bit/48 kHz file is roughly 170 MB decoded as f32.

```rust
use audio_samples_streaming::FlacFileStream;

let mut source = FlacFileStream::<f32>::open("input.flac")?;
println!("{} Hz  {} ch  {} frames", source.sample_rate(), source.channels(), source.total_frames());
```

## Features

```toml
[dependencies]
audio_samples_streaming = { version = "...", features = ["wav", "device"] }
```

| Feature | Enables |
|---------|---------|
| `wav` | `WavFileStream`, `WavFileSink` |
| `flac` | `FlacFileStream` |
| `device` | `DeviceCapture`, `DevicePlayback` via cpal |
| `rodio` | `RodioSource` adapter |
| `async` | `AsyncAudioStream` adapter |

No features are enabled by default.

## Examples

Run any example with `cargo run --example <name> --features <features>`:

| Example | Features | Description |
|---------|----------|-------------|
| `file_to_file` | `wav` | WAV → WAV via `pipeline::run` |
| `manual_loop` | `wav` | Manual chunk loop with processing hooks |
| `device_playback` | `device,wav` | Stream WAV to speakers |
| `device_capture` | `device,wav` | Record microphone to WAV |
| `device_passthrough` | `device` | Real-time mic → speakers |
| `rodio_source` | `rodio,wav` | Stream WAV via rodio |
| `async_stream` | `async,wav` | WAV as async stream |

## Contributing

Contributions are welcome. Please submit a pull request and see
[CONTRIBUTING.md](CONTRIBUTING.md) for guidance.

## Citing

If you use AudioSamples in research, please cite:

```bibtex
@inproceedings{geraghty2026audio,
  author    = {Geraghty, Jack and Golpayegani, Fatemeh and Hines, Andrew},
  title     = {Audio Made Simple: A Modern Framework for Audio Processing},
  booktitle = {ACM Multimedia Systems Conference 2026 (MMSys '26)},
  year      = {2026},
  month     = apr,
  publisher = {ACM},
  address   = {Hong Kong, Hong Kong},
  doi       = {10.1145/3793853.3799811},
  note      = {Accepted for publication}
}
```

[crate]: https://crates.io/crates/audio_samples_streaming
[crate-img]: https://img.shields.io/crates/v/audio_samples_streaming?style=for-the-badge&color=009E73&label=crates.io

[docs]: https://docs.rs/audio_samples_streaming
[docs-img]: https://img.shields.io/badge/docs.rs-online-009E73?style=for-the-badge&labelColor=gray

[license-img]: https://img.shields.io/crates/l/audio_samples_streaming?style=for-the-badge&label=license&labelColor=gray
[license]: https://github.com/jmg049/audio_samples_streaming/blob/main/LICENSE
