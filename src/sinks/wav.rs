use std::fs::File;
use std::io::BufWriter;
use std::marker::PhantomData;
use std::path::Path;

use audio_samples::AudioSamples;
use audio_samples::traits::StandardSample;
use audio_samples_io::WriteSeek;
use audio_samples_io::error::AudioIOError;
use audio_samples_io::traits::AudioStreamWrite;
use audio_samples_io::traits::AudioStreamWriter;
use audio_samples_io::wav::StreamedWavWriter;

use crate::traits::AudioSink;

/// An [`AudioSink`] that encodes frames into a WAV file incrementally.
///
/// Wraps [`StreamedWavWriter`] from `audio_samples_io`. Headers are written on
/// construction and finalised when [`finalize`](AudioSink::finalize) is called.
/// Always call `finalize` — the WAV header size fields are only correct after it runs.
pub struct WavFileSink<W: WriteSeek, T: StandardSample> {
    inner: StreamedWavWriter<W>,
    _marker: PhantomData<T>,
}

impl<W: WriteSeek, T: StandardSample> WavFileSink<W, T> {
    /// Create a sink that encodes to 16-bit PCM WAV (CD quality, lossless, widest compatibility).
    pub fn new_i16(writer: W, channels: u16, sample_rate: u32) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavWriter::new_i16(writer, channels, sample_rate)?,
            _marker: PhantomData,
        })
    }

    /// Create a sink that encodes to 24-bit PCM WAV (studio standard, 144 dB dynamic range).
    pub fn new_i24(writer: W, channels: u16, sample_rate: u32) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavWriter::new_i24(writer, channels, sample_rate)?,
            _marker: PhantomData,
        })
    }

    /// Create a sink that encodes to 32-bit integer PCM WAV.
    pub fn new_i32(writer: W, channels: u16, sample_rate: u32) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavWriter::new_i32(writer, channels, sample_rate)?,
            _marker: PhantomData,
        })
    }

    /// Create a sink that encodes to 32-bit float WAV (IEEE 754, native internal format).
    pub fn new_f32(writer: W, channels: u16, sample_rate: u32) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavWriter::new_f32(writer, channels, sample_rate)?,
            _marker: PhantomData,
        })
    }

    /// Create a sink that encodes to 64-bit float WAV (maximum precision, larger files).
    pub fn new_f64(writer: W, channels: u16, sample_rate: u32) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedWavWriter::new_f64(writer, channels, sample_rate)?,
            _marker: PhantomData,
        })
    }

    /// Total frames written so far.
    pub fn frames_written(&self) -> usize {
        self.inner.frames_written()
    }
}

impl<T: StandardSample + 'static> WavFileSink<BufWriter<File>, T> {
    /// Create a 32-bit float WAV file at the given path.
    ///
    /// Convenience wrapper around [`new_f32`](Self::new_f32). The output file always uses
    /// f32 encoding; the source sample type `T` must implement `ConvertTo<f32>`.
    pub fn create<P: AsRef<Path>>(
        path: P,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, AudioIOError>
    where
        T: audio_samples::ConvertTo<f32>,
    {
        let writer = BufWriter::new(File::create(path)?);
        Self::new_f32(writer, channels, sample_rate)
    }

    /// Create a WAV file at the given path with an explicit output bit depth.
    ///
    /// Use this when you need a specific encoding (e.g. 24-bit for studio delivery):
    ///
    /// ```no_run
    /// use audio_samples_streaming::WavFileSink;
    /// use audio_samples_io::ValidatedSampleType;
    ///
    /// let sink = WavFileSink::<_, f32>::create_typed("out.wav", 2, 48000, ValidatedSampleType::I24)?;
    /// # Ok::<(), audio_samples_io::error::AudioIOError>(())
    /// ```
    pub fn create_typed<P: AsRef<Path>>(
        path: P,
        channels: u16,
        sample_rate: u32,
        sample_type: audio_samples_io::ValidatedSampleType,
    ) -> Result<Self, AudioIOError> {
        use audio_samples_io::ValidatedSampleType;

        let writer = BufWriter::new(File::create(path)?);
        let inner = match sample_type {
            // 8-bit output is widened to 16-bit, matching audio_samples_io's
            // own dispatch for u8 streaming writes.
            ValidatedSampleType::U8 | ValidatedSampleType::I16 => {
                StreamedWavWriter::new_i16(writer, channels, sample_rate)?
            }
            ValidatedSampleType::I24 => StreamedWavWriter::new_i24(writer, channels, sample_rate)?,
            ValidatedSampleType::I32 => StreamedWavWriter::new_i32(writer, channels, sample_rate)?,
            ValidatedSampleType::F32 => StreamedWavWriter::new_f32(writer, channels, sample_rate)?,
            ValidatedSampleType::F64 => StreamedWavWriter::new_f64(writer, channels, sample_rate)?,
        };
        Ok(Self {
            inner,
            _marker: PhantomData,
        })
    }
}

impl<W: WriteSeek, T: StandardSample + 'static> AudioSink for WavFileSink<W, T> {
    type Sample = T;
    type Error = AudioIOError;

    fn write_chunk(
        &mut self,
        chunk: &AudioSamples<'static, Self::Sample>,
    ) -> Result<(), Self::Error> {
        self.inner.write_frames(chunk)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        AudioStreamWriter::flush(&mut self.inner)
    }

    fn finalize(mut self) -> Result<(), Self::Error> {
        AudioStreamWriter::finalize(&mut self.inner)
    }
}
