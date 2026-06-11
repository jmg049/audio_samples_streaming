use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomData;
use std::path::Path;

use audio_samples::AudioSamples;
use audio_samples::traits::StandardSample;
use audio_samples_io::ReadSeek;
use audio_samples_io::error::AudioIOError;
use audio_samples_io::wav::StreamedWavFile;

use crate::traits::AudioStream;

/// An [`AudioStream`] that reads frames from a WAV file on demand.
///
/// Wraps [`StreamedWavFile`] from `audio_samples_io`. The file is parsed on construction;
/// audio data is read one chunk at a time via [`fill_chunk`](AudioStream::fill_chunk).
pub struct WavFileStream<R: ReadSeek, T: StandardSample> {
    inner: StreamedWavFile<R>,
    _marker: PhantomData<T>,
}

impl<R: ReadSeek, T: StandardSample> WavFileStream<R, T> {
    /// Create a stream from any `Read + Seek` source.
    pub fn new(reader: R) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavFile::new(reader)?,
            _marker: PhantomData,
        })
    }

    /// Sample rate of the underlying file in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    /// Number of channels in the underlying file.
    pub fn num_channels(&self) -> u16 {
        self.inner.num_channels()
    }

    /// Seek to a specific frame position within the file.
    pub fn seek_to_frame(&mut self, frame: usize) -> Result<(), AudioIOError> {
        self.inner.seek_to_frame(frame)
    }

    /// Reset to the beginning of the audio data.
    pub fn reset(&mut self) -> Result<(), AudioIOError> {
        self.inner.reset()
    }

    /// Total frames in the file.
    pub fn total_frames(&self) -> usize {
        self.inner.total_frames()
    }

    /// Remaining frames from the current position.
    pub fn remaining_frames(&self) -> usize {
        self.inner.remaining_frames()
    }
}

impl<T: StandardSample + 'static> WavFileStream<BufReader<File>, T> {
    /// Open a WAV file by path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AudioIOError> {
        let reader = BufReader::new(File::open(path)?);
        Self::new(reader)
    }
}

impl<R: ReadSeek, T: StandardSample + 'static> AudioStream for WavFileStream<R, T> {
    type Sample = T;
    type Error = AudioIOError;

    fn fill_chunk(
        &mut self,
        buffer: &mut AudioSamples<'static, Self::Sample>,
    ) -> Result<Option<usize>, Self::Error> {
        if self.inner.remaining_frames() == 0 {
            return Ok(None);
        }

        let frame_count = buffer.samples_per_channel();
        let n = self.inner.read_frames_into(buffer, frame_count)?;

        if n == 0 { Ok(None) } else { Ok(Some(n)) }
    }
}
