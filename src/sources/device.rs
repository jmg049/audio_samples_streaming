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
use crate::traits::AudioStream;

/// An [`AudioStream`] that captures audio from an input device (microphone, line-in).
///
/// Audio is captured on a dedicated cpal callback thread and written into a lock-free
/// ring buffer. [`fill_chunk`](AudioStream::fill_chunk) reads from the consumer side,
/// blocking on a condvar rather than spinning.
///
/// The cpal callback thread requests `SCHED_FIFO` priority on Linux on its first
/// invocation. This silently fails without `CAP_SYS_NICE` or rtkit — configure
/// `/etc/security/limits.d/audio.conf` (`@audio - rtprio 95`) for production use.
pub struct DeviceCapture {
    _stream: cpal::Stream,
    consumer: rtrb::Consumer<f32>,
    channels: u16,
    sample_rate: u32,
    scratch: Vec<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
}

impl DeviceCapture {
    /// Open the system default input device.
    ///
    /// `target_latency` requests a device callback period as a duration:
    /// - `None`                              — let the driver choose (typically 10–20 ms)
    /// - `Some(Duration::from_millis(5))`    — typical professional target
    /// - `Some(Duration::from_millis(2))`    — low-latency; requires JACK/PipeWire and RT scheduling
    ///
    /// The actual period is rounded to the nearest integer frame count at the device's
    /// sample rate. The ring buffer is sized to 8× the period.
    pub fn from_default_input(target_latency: Option<Duration>) -> Result<Self, StreamingError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| StreamingError::Parameter("no default input device".into()))?;
        Self::from_device(&device, target_latency)
    }

    /// Open a specific cpal device for capture.
    pub fn from_device(
        device: &cpal::Device,
        target_latency: Option<Duration>,
    ) -> Result<Self, StreamingError> {
        let supported = device
            .default_input_config()
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
            SampleFormat::F32 => build_f32_input(
                device,
                &config,
                producer,
                Arc::clone(&notifier),
                Arc::clone(&xruns),
            )?,
            SampleFormat::I16 => build_i16_input(
                device,
                &config,
                producer,
                Arc::clone(&notifier),
                Arc::clone(&xruns),
            )?,
            SampleFormat::U8 => build_u8_input(
                device,
                &config,
                producer,
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

        let scratch_cap = period_frames.unwrap_or(1024) as usize * channels as usize;
        let scratch = Vec::with_capacity(scratch_cap);
        Ok(Self {
            _stream: stream,
            consumer,
            channels,
            sample_rate,
            scratch,
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

    /// Number of capture overflows since the stream was opened.
    ///
    /// An overflow means the ring buffer was full when the callback fired and samples
    /// were dropped. Increase `period_frames` or reduce processing load to eliminate xruns.
    pub fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Relaxed)
    }
}

impl AudioStream for DeviceCapture {
    type Sample = f32;
    type Error = StreamingError;

    fn fill_chunk(
        &mut self,
        buffer: &mut AudioSamples<'static, f32>,
    ) -> Result<Option<usize>, StreamingError> {
        let frames = buffer.samples_per_channel().get();
        let channels = self.channels as usize;
        let samples_needed = frames * channels;

        loop {
            if self.consumer.slots() >= samples_needed {
                break;
            }
            self.notifier.wait();
        }

        let chunk = self
            .consumer
            .read_chunk(samples_needed)
            .map_err(|_| StreamingError::Parameter("ring buffer read failed".into()))?;
        let (s1, s2) = chunk.as_slices();

        if channels == 1 {
            let mono = buffer
                .as_mono_mut()
                .ok_or_else(|| StreamingError::Parameter("expected mono buffer".into()))?
                .as_slice_mut();
            mono[..s1.len()].copy_from_slice(s1);
            mono[s1.len()..].copy_from_slice(s2);
        } else {
            let multi = buffer
                .as_multi_channel_mut()
                .ok_or_else(|| StreamingError::Parameter("expected multi-channel buffer".into()))?;
            let flat = multi
                .as_slice_mut()
                .ok_or_else(|| StreamingError::Parameter("buffer not contiguous".into()))?;

            let interleaved: &[f32] = if s2.is_empty() {
                s1
            } else {
                self.scratch.clear();
                self.scratch.extend_from_slice(s1);
                self.scratch.extend_from_slice(s2);
                &self.scratch
            };

            // SAFETY: samples_needed > 0 (frames * channels, both non-zero).
            let flat_nes = unsafe { NonEmptySlice::new_unchecked_mut(flat) };
            let channels_nz = unsafe { NonZeroU32::new_unchecked(self.channels as u32) };
            let interleaved_nes = unsafe { NonEmptySlice::new_unchecked(interleaved) };
            simd_conversions::deinterleave_multi(interleaved_nes, flat_nes, channels_nz)
                .map_err(StreamingError::AudioSamples)?;
        }

        chunk.commit_all();
        Ok(Some(frames))
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

fn build_f32_input(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: rtrb::Producer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    let rt_once = Arc::new(Once::new());
    device
        .build_input_stream(
            config,
            move |data: &[f32], _| {
                try_set_rt_priority(&rt_once);
                let available = producer.slots();
                if available < data.len() {
                    xruns.fetch_add(1, Ordering::Relaxed);
                }
                let n = available.min(data.len());
                if n > 0 {
                    producer
                        .write_chunk_uninit(n)
                        .expect("slots checked above")
                        .fill_from_iter(data[..n].iter().copied());
                }
                notifier.notify();
            },
            |e| eprintln!("capture error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}

fn build_i16_input(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: rtrb::Producer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    use cpal::Sample;
    let rt_once = Arc::new(Once::new());
    device
        .build_input_stream(
            config,
            move |data: &[i16], _| {
                try_set_rt_priority(&rt_once);
                let n = producer.slots().min(data.len());
                if n < data.len() {
                    xruns.fetch_add(1, Ordering::Relaxed);
                }
                if n > 0 {
                    producer
                        .write_chunk_uninit(n)
                        .expect("slots checked above")
                        .fill_from_iter(data[..n].iter().map(|&s| s.to_float_sample()));
                    notifier.notify();
                }
            },
            |e| eprintln!("capture error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}

fn build_u8_input(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: rtrb::Producer<f32>,
    notifier: Arc<AudioNotifier>,
    xruns: Arc<AtomicU32>,
) -> Result<cpal::Stream, StreamingError> {
    use cpal::Sample;
    let rt_once = Arc::new(Once::new());
    device
        .build_input_stream(
            config,
            move |data: &[u8], _| {
                try_set_rt_priority(&rt_once);
                let n = producer.slots().min(data.len());
                if n < data.len() {
                    xruns.fetch_add(1, Ordering::Relaxed);
                }
                if n > 0 {
                    producer
                        .write_chunk_uninit(n)
                        .expect("slots checked above")
                        .fill_from_iter(data[..n].iter().map(|&s| s.to_float_sample()));
                    notifier.notify();
                }
            },
            |e| eprintln!("capture error: {e}"),
            None,
        )
        .map_err(|e| StreamingError::Parameter(e.to_string()))
}
