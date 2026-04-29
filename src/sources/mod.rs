#[cfg(feature = "device")]
pub mod device;
#[cfg(feature = "flac")]
pub mod flac;
#[cfg(feature = "wav")]
pub mod wav;

#[cfg(feature = "device")]
pub use device::DeviceCapture;
#[cfg(feature = "flac")]
pub use flac::FlacFileStream;
#[cfg(feature = "wav")]
pub use wav::WavFileStream;
