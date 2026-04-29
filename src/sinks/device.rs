use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use audio_samples::AudioSamples;
use audio_samples::simd_conversions;
use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use non_empty_slice::NonEmptySlice;

use crate::error::StreamingError;
use crate::notify::AudioNotifier;
use crate::traits::AudioSink;

/// An [`AudioSink`] that plays audio through an output device (speaker, headphones).
///
/// Audio is written into a lock-free ring buffer by [`write_chunk`](AudioSink::write_chunk).
/// The cpal callback drains that buffer on a dedicated thread, notifying the non-RT side
/// via condvar when space opens up. Underrun produces silence rather than stalling.
///
/// The cpal callback thread requests `SCHED_FIFO` priority on Linux on its first
/// invocation. This silently fails without `CAP_SYS_NICE` or rtkit — configure
/// `/etc/security/limits.d/audio.conf` (`@audio - rtprio 95`) for production use.
pub struct DevicePlayback {
    _stream: cpal::Stream,
    producer: rtrb::Producer<f32>,
    channels: u16,
    sample_rate: u32,
    interleaved: Vec<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
}

impl DevicePlayback {
    /// Open the system default output device.
    ///
    /// `target_latency` requests a device callback period as a duration:
    /// - `None`                              — let the driver choose (typically 10–20 ms)
    /// - `Some(Duration::from_millis(5))`    — typical professional target
    /// - `Some(Duration::from_millis(2))`    — low-latency; requires JACK/PipeWire and RT scheduling
    ///
    /// The actual period is rounded to the nearest integer frame count at the device's
    /// sample rate. The ring buffer is sized to 8× the period.
    pub fn from_default_output(target_latency: Option<Duration>) -> Result<Self, StreamingError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| StreamingError::Parameter("no default output device".into()))?;
        Self::from_device(&device, target_latency)
    }

    /// Open a specific cpal device for playback.
    pub fn from_device(
        device: &cpal::Device,
        target_latency: Option<Duration>,
    ) -> Result<Self, StreamingError> {
        let supported = device
            .default_output_config()
            .map_err(|e| StreamingError::Parameter(e.to_string()))?;

        let channels = supported.channels();
        let sample_rate = supported.sample_rate();
        let period_frames: Option<u32> =
            target_latency.map(|d| (d.as_secs_f64() * sample_rate as f64).round() as u32);
        let ring_capacity = 8 * period_frames.unwrap_or(1024) as usize * channels as usize;
        let (producer, consumer) = rtrb::RingBuffer::new(ring_capacity);

        let config = cpal::StreamConfig {
            channels,
            sample_rate: supported.sample_rate(),
            buffer_size: match period_frames {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };

        let notifier = Arc::new(AudioNotifier::new());
        let xruns = Arc::new(AtomicU32::new(0));

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_f32_output(
                device,
                &config,
                consumer,
                Arc::clone(&notifier),
                Arc::clone(&xruns),
            )?,
            SampleFormat::I16 => build_i16_output(
                device,
                &config,
                consumer,
                Arc::clone(&notifier),
                Arc::clone(&xruns),
            )?,
            SampleFormat::U8 => build_u8_output(
                device,
                &config,
                consumer,
                Arc::clone(&notifier),
                Arc::clone(&xruns),
            )?,
            fmt => {
                return Err(StreamingError::Unsupported(format!(
                    "sample format {fmt:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| StreamingError::Parameter(e.to_string()))?;

        let interleaved_cap = period_frames.unwrap_or(1024) as usize * channels as usize;
        let interleaved = Vec::with_capacity(interleaved_cap);
        Ok(Self {
            _stream: stream,
            producer,
            channels,
            sample_rate,
            interleaved,
            notifier,
            xruns,
        })
    }

    /// Sample rate reported by the device. Use this to allocate `AudioSamples` buffers.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count reported by the device. Use this to allocate `AudioSamples` buffers.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of playback underruns since the stream was opened.
    ///
    /// An underrun means the ring buffer was empty when the callback fired and silence
    /// was played. Increase `period_frames` or reduce processing load to eliminate xruns.
    pub fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Relaxed)
    }
}

impl AudioSink for DevicePlayback {
    type Sample = f32;
    type Error = StreamingError;

    fn write_chunk(&mut self, chunk: &AudioSamples<'static, f32>) -> Result<(), StreamingError> {
        let channels = self.channels as usize;
        let frames = chunk.samples_per_channel().get();
        let samples = frames * channels;

        self.interleaved.clear();
        self.interleaved.resize(samples, 0.0);

        if chunk.is_mono() {
            let mono = chunk.as_mono().expect("is_mono but as_mono returned None");
            for (frame, &s) in mono.iter().take(frames).enumerate() {
                for ch in 0..channels {
                    self.interleaved[frame * channels + ch] = s;
                }
            }
        } else {
            let flat = chunk
                .as_multi_channel()
                .expect("multi-channel but as_multi_channel returned None")
                .as_slice()
                .ok_or_else(|| StreamingError::Parameter("source buffer not contiguous".into()))?;

            // SAFETY: samples > 0 (frames * channels, both non-zero).
            let flat_nes = unsafe { NonEmptySlice::new_unchecked(flat) };
            let out_nes = unsafe { NonEmptySlice::new_unchecked_mut(&mut self.interleaved) };
            let channels_nz = unsafe { NonZeroU32::new_unchecked(self.channels as u32) };

            simd_conversions::interleave_multi(flat_nes, out_nes, channels_nz)
                .map_err(StreamingError::AudioSamples)?;
        }

        loop {
            if self.producer.slots() >= samples {
                break;
            }
            self.notifier.wait();
        }

        self.producer
            .write_chunk_uninit(samples)
            .expect("slots verified above")
            .fill_from_iter(self.interleaved.iter().copied());

        Ok(())
    }

    fn flush(&mut self) -> Result<(), StreamingError> {
        let capacity = self.producer.buffer().capacity();
        loop {
            if self.producer.slots() >= capacity {
                break;
            }
            self.notifier.wait();
        }
        Ok(())
    }

    fn finalize(mut self) -> Result<(), StreamingError> {
        self.flush()
    }
}

/// Attempt to elevate the calling thread to `SCHED_FIFO` priority on Linux.
/// Silently no-ops if the process lacks `CAP_SYS_NICE` or rtkit access.
#[cfg(target_os = "linux")]
fn try_set_rt_priority(once: &Once) {
    once.call_once(|| unsafe {
        let param = libc::sched_param { sched_priority: 80 };
        libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param);
    });
}

#[cfg(not(target_os = "linux"))]
fn try_set_rt_priority(_once: &Once) {}

fn build_f32_output(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    let rt_once = Arc::new(Once::new());
    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                try_set_rt_priority(&rt_once);
                match consumer.read_chunk(data.len()) {
                    Ok(chunk) => {
                        let (s1, s2) = chunk.as_slices();
                        let mid = s1.len();
                        data[..mid].copy_from_slice(s1);
                        data[mid..].copy_from_slice(s2);
                        chunk.commit_all();
                    }
                    Err(_) => {
                        xruns.fetch_add(1, Ordering::Relaxed);
                        data.fill(0.0);
                    }
                }
                notifier.notify();
            },
            |e| eprintln!("playback error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}

fn build_i16_output(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    use cpal::Sample;
    let rt_once = Arc::new(Once::new());
    device
        .build_output_stream(
            config,
            move |data: &mut [i16], _| {
                try_set_rt_priority(&rt_once);
                match consumer.read_chunk(data.len()) {
                    Ok(chunk) => {
                        let (s1, s2) = chunk.as_slices();
                        for (dst, &src) in data.iter_mut().zip(s1.iter().chain(s2.iter())) {
                            *dst = src.to_sample::<i16>();
                        }
                        chunk.commit_all();
                    }
                    Err(_) => {
                        xruns.fetch_add(1, Ordering::Relaxed);
                        data.fill(0i16);
                    }
                }
                notifier.notify();
            },
            |e| eprintln!("playback error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}

fn build_u8_output(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    use cpal::Sample;
    let rt_once = Arc::new(Once::new());
    device
        .build_output_stream(
            config,
            move |data: &mut [u8], _| {
                try_set_rt_priority(&rt_once);
                match consumer.read_chunk(data.len()) {
                    Ok(chunk) => {
                        let (s1, s2) = chunk.as_slices();
                        for (dst, &src) in data.iter_mut().zip(s1.iter().chain(s2.iter())) {
                            *dst = src.to_sample::<u8>();
                        }
                        chunk.commit_all();
                    }
                    Err(_) => {
                        xruns.fetch_add(1, Ordering::Relaxed);
                        data.fill(128u8);
                    }
                }
                notifier.notify();
            },
            |e| eprintln!("playback error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}
