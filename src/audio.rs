use crossbeam_channel::Receiver;
use hound::{SampleFormat, WavWriter};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alsa::{
    pcm::{Access, Format, HwParams, PCM},
    Direction, Error, ValueOr,
};

#[derive(Debug)]
pub enum CaptureDeviceError {
    FormatUnimplemented(Format),
    AlsaError(alsa::Error),
}

impl std::fmt::Display for CaptureDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureDeviceError::FormatUnimplemented(format) => {
                write!(f, "Unimplemented sample format: {format}")
            }
            CaptureDeviceError::AlsaError(err) => {
                write!(f, "Audio Device Error: {err}")
            }
        }
    }
}

impl std::error::Error for CaptureDeviceError {}

impl From<Error> for CaptureDeviceError {
    fn from(value: Error) -> Self {
        Self::AlsaError(value)
    }
}

pub struct CaptureDevice {
    device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    output_dir: String,
    running: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
    pps: Receiver<i64>,
}

#[allow(clippy::too_many_arguments)]
impl CaptureDevice {
    pub fn new(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        output_dir: &str,
        running: Arc<AtomicBool>,
        status: Arc<AtomicU8>,
        pps: Receiver<i64>,
    ) -> Self {
        Self {
            device_name: device_name.to_owned(),
            channels,
            samplerate,
            format,
            output_dir: output_dir.to_owned(),
            running,
            status,
            pps,
        }
    }

    #[allow(unused)]
    pub fn set_device_name(&mut self, device_name: &str) {
        device_name.clone_into(&mut self.device_name);
    }

    fn init_device(&self) -> Result<PCM, Error> {
        let pcm = PCM::new(&self.device_name, Direction::Capture, false)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(self.channels)?;
            hwp.set_rate(self.samplerate, ValueOr::Nearest)?;
            hwp.set_format(self.format)?;
            hwp.set_access(Access::RWInterleaved)?;
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare()?;
        pcm.start()?;
        Ok(pcm)
    }

    pub fn read(&self, file_duration: Duration) -> Result<(), CaptureDeviceError> {
        let pcm = self.init_device()?;
        let io = match &self.format {
            Format::S32LE | Format::S32BE => pcm.io_i32()?,
            default => return Err(CaptureDeviceError::FormatUnimplemented(*default)),
        };

        let mut buf = [0i32; 8192];
        let wav_spec = hound::WavSpec {
            #[allow(clippy::cast_possible_truncation)]
            channels: self.channels as u16,
            sample_rate: self.samplerate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Int,
        };

        let mut nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let mut path = format!("{}/{nanos}.wav", self.output_dir);
        let mut writer = WavWriter::create(path, wav_spec).unwrap();
        let mut start = Instant::now();
        let mut last_read = Instant::now();
        while self.running.load(Ordering::Relaxed) {
            if let Ok(nanos) = self.pps.try_recv() {
                let low: i32 = (nanos & 0xffff_ffff) as i32;
                let high: i32 = (nanos >> 32) as i32;
                #[allow(clippy::cast_possible_wrap)]
                let prefix = 0xeeee_eeeeu32 as i32;
                writer.write_sample(prefix).unwrap();
                writer.write_sample(prefix).unwrap();
                writer.write_sample(high).unwrap();
                writer.write_sample(low).unwrap();
            }
            if io.readi(&mut buf)? * wav_spec.channels as usize == buf.len() {
                last_read = Instant::now();
                for sample in buf {
                    writer.write_sample(sample).unwrap();
                }
            }
            if start.elapsed() >= file_duration {
                start = start.checked_add(file_duration).unwrap();
                writer.finalize().unwrap();
                nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
                path = format!("{}/{nanos}.wav", self.output_dir);
                writer = WavWriter::create(path, wav_spec).unwrap();
            }
            if last_read.elapsed().as_secs() >= 2 {
                self.status.store(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }
}
