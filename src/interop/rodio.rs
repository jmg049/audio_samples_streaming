use std::time::Duration;

use audio_samples::AudioSamples;
use rodio::Source;

use crate::traits::AudioStream;

/// Adapts any [`AudioStream<Sample = f32>`] into a [`rodio::Source`].
///
/// Internally buffers one chunk at a time (interleaved) and feeds rodio's mixer
/// sample-by-sample. The chunk size is determined by the pre-allocated `buffer`
/// passed at construction — use a size appropriate for your latency requirements
/// (e.g. 1024–4096 frames).
pub struct RodioSource<S: AudioStream<Sample = f32>> {
    stream: S,
    buffer: AudioSamples<'static, f32>,
    interleaved: Vec<f32>,
    pos: usize,
    exhausted: bool,
}

impl<S: AudioStream<Sample = f32>> RodioSource<S> {
    /// Create a rodio source from any `AudioStream<Sample = f32>`.
    ///
    /// `buffer` is a pre-allocated `AudioSamples` that determines the chunk size
    /// and audio format (sample rate, channel count).
    pub fn new(stream: S, buffer: AudioSamples<'static, f32>) -> Self {
        Self {
            stream,
            buffer,
            interleaved: Vec::new(),
            pos: 0,
            exhausted: false,
        }
    }

    fn refill(&mut self) {
        match self.stream.fill_chunk(&mut self.buffer) {
            Ok(Some(frames)) => {
                let channels = self.buffer.num_channels().get() as usize;
                self.interleaved.clear();
                self.interleaved.resize(frames * channels, 0.0);

                if self.buffer.is_mono() {
                    let mono = self.buffer.as_mono().unwrap();
                    for (i, &s) in mono.iter().take(frames).enumerate() {
                        self.interleaved[i] = s;
                    }
                } else {
                    let arr = self.buffer.as_multi_channel().unwrap();
                    let nch = self.buffer.num_channels().get() as usize;
                    for ch in 0..nch {
                        for frame in 0..frames {
                            self.interleaved[frame * channels + ch] = arr[[ch, frame]];
                        }
                    }
                }

                self.pos = 0;
            }
            _ => {
                self.exhausted = true;
            }
        }
    }
}

impl<S: AudioStream<Sample = f32>> Iterator for RodioSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.exhausted {
            return None;
        }

        if self.pos >= self.interleaved.len() {
            self.refill();
            if self.exhausted {
                return None;
            }
        }

        let sample = self.interleaved[self.pos];
        self.pos += 1;
        Some(sample)
    }
}

impl<S: AudioStream<Sample = f32>> Source for RodioSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.interleaved.len() / self.buffer.num_channels().get() as usize)
    }

    fn channels(&self) -> std::num::NonZero<u16> {
        std::num::NonZero::new(self.buffer.num_channels().get() as u16).expect("channels > 0")
    }

    fn sample_rate(&self) -> std::num::NonZero<u32> {
        self.buffer.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
