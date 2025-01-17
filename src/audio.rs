use hound::{SampleFormat, WavWriter};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alsa::{
    pcm::{Access, Format, HwParams, PCM},
    Direction, Error, ValueOr,
};

#[derive(thiserror::Error, Debug)]
pub enum CaptureDeviceError {
    #[error("Format unimplemented: {0}")]
    FormatUnimplemented(Format),
    #[error("Alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("Hound error: {0}")]
    Hound(#[from] hound::Error),
}

pub struct CaptureDevice<'a> {
    device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    output_dir: PathBuf,
    running: &'a AtomicBool,
    status: &'a AtomicU8,
    pps: Arc<Mutex<(bool, i64)>>,
}

#[allow(clippy::too_many_arguments)]
impl<'a> CaptureDevice<'a> {
    pub fn new<P: Into<PathBuf>>(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        output_dir: P,
        running: &'a AtomicBool,
        status: &'a AtomicU8,
        pps: Arc<Mutex<(bool, i64)>>,
    ) -> Self {
        Self {
            device_name: device_name.to_owned(),
            channels,
            samplerate,
            format,
            output_dir: output_dir.into(),
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
        #[allow(clippy::cast_possible_wrap)]
        const PREFIX: i32 = 0xeeee_eeeeu32 as i32;

        let pcm = self.init_device()?;
        let io = match &self.format {
            Format::S32LE | Format::S32BE => pcm.io_i32()?,
            default => return Err(CaptureDeviceError::FormatUnimplemented(*default)),
        };

        let mut buf = [0i32; 1024];
        let wav_spec = hound::WavSpec {
            #[allow(clippy::cast_possible_truncation)]
            channels: self.channels as u16,
            sample_rate: self.samplerate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Int,
        };

        let mut nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let mut path = self.output_dir.join(format!("{nanos}.wav"));
        let mut writer = WavWriter::create(path, wav_spec)?;
        let mut start = Instant::now();
        let mut last_read = Instant::now();
        while self.running.load(Ordering::Relaxed) {
            {
                let mut pps = self.pps.lock();
                if pps.0 {
                    pps.0 = false;
                    let low: i32 = (pps.1 & 0xffff_ffff) as i32;
                    let high: i32 = (pps.1 >> 32) as i32;
                    drop(pps);
                    writer.write_sample(PREFIX)?;
                    writer.write_sample(PREFIX)?;
                    writer.write_sample(high)?;
                    writer.write_sample(low)?;
                }
            }
            //if let Ok(s) = io.readi(&mut buf) {
            //    let n = s * wav_spec.channels as usize;
            //    let mut zeros = 0;
            //    for &sample in &buf[0..n] {
            //        if sample.trailing_zeros() >= 28 {
            //            zeros += 1;
            //        }
            //        writer.write_sample(sample)?;
            //    }
            //    if zeros < n {
            //        last_read = Instant::now();
            //    }
            //}
            if io.readi(&mut buf)? * wav_spec.channels as usize == buf.len() {
                let mut zeros = 0;
                let mut samples = buf.len();
                for sample in buf {
                    if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                        zeros += 1;
                    }
                    writer.write_sample(sample)?;
                }
                if zeros < samples {
                    last_read = Instant::now();
                }
            }
            if start.elapsed() >= file_duration {
                start = start.checked_add(file_duration).unwrap();
                writer.finalize()?;
                nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
                path = self.output_dir.join(format!("{nanos}.wav"));
                writer = WavWriter::create(path, wav_spec)?;
            }
            if last_read.elapsed().as_secs() >= 2 {
                self.status.store(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }
}
