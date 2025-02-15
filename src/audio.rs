use hound::{SampleFormat, WavWriter};
use log::{info, warn};
use parking_lot::Mutex;
use std::fs::File;
use std::io::Write;
use std::io::{self, BufWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
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
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

pub struct CaptureDevice<'a> {
    device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    output_dir: PathBuf,
    clock_dir: PathBuf,
    running: &'a AtomicBool,
    status: &'a AtomicU8,
    max_read: Arc<Mutex<i32>>,
}

#[allow(clippy::too_many_arguments)]
impl<'a> CaptureDevice<'a> {
    pub fn new<P: Into<PathBuf>>(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        output_dir: P,
        clock_dir: P,
        running: &'a AtomicBool,
        status: &'a AtomicU8,
        max_read: Arc<Mutex<i32>>,
    ) -> Self {
        Self {
            device_name: device_name.to_owned(),
            channels,
            samplerate,
            format,
            output_dir: output_dir.into(),
            clock_dir: clock_dir.into(),
            running,
            status,
            max_read,
        }
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

        let mut buf = [0i32; 1024];
        let wav_spec = hound::WavSpec {
            channels: self.channels as u16,
            sample_rate: self.samplerate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Int,
        };

        #[cfg(feature = "audio")]
        let mut nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        #[cfg(feature = "audio")]
        let mut path = self.output_dir.join(format!("{nanos}.wav"));
        #[cfg(feature = "audio")]
        let mut writer = WavWriter::create(path.clone(), wav_spec)?;
        #[cfg(feature = "audio")]
        let clock_path = self.clock_dir.join(format!("{nanos}.csv"));
        #[cfg(feature = "audio")]
        let mut clock_writer = BufWriter::new(File::create(clock_path)?);
        #[cfg(feature = "audio")]
        writeln!(clock_writer, "time,file,sample")?;

        let mut start = Instant::now();
        let mut last_read = Instant::now();
        let mut clock = Instant::now();
        let mut sample = 0;
        info!("start audio read");
        while self.running.load(Ordering::Relaxed) {
            #[cfg(feature = "audio")]
            if clock.elapsed() >= Duration::from_secs(1) {
                let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
                clock = clock.checked_add(Duration::from_secs(1)).unwrap();
                writeln!(
                    clock_writer,
                    "{nanos},{sample},{}",
                    path.file_name().unwrap().to_string_lossy()
                )?;
                clock_writer.flush()?;
            }
            //if let Ok(s) = io.readi(&mut buf) {
            //    let n = s * wav_spec.channels as usize;
            //    let mut max_sample = i32::MIN;
            //    let mut zeros = 0;
            //    for &sample in &buf[0..n] {
            //        if sample.abs() > max_sample {
            //            max_sample = sample;
            //        }
            //        if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
            //            zeros += 1;
            //        }
            //        #[cfg(feature = "audio")]
            //        writer.write_sample(sample)?;
            //    }
            //    sample += s;
            //    let mut saved_max = self.max_read.lock();
            //    *saved_max = saved_max.max(max_sample);
            //    if zeros < n {
            //        last_read = Instant::now();
            //    }
            //}
            //if io.readi(&mut buf)? * wav_spec.channels as usize == buf.len() {
            //let s = io.readi(&mut buf)?;

            match io.readi(&mut buf) {
                Ok(s) => {
                    let n = s * wav_spec.channels as usize;
                    let mut max_sample = i32::MIN;
                    let mut zeros = 0;
                    //let samples = buf.len();
                    for sample in &buf[0..n] {
                        if sample.abs() > max_sample {
                            max_sample = *sample;
                        }
                        if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                            zeros += 1;
                        }
                        #[cfg(feature = "audio")]
                        writer.write_sample(*sample)?;
                    }
                    sample += s;
                    let mut saved_max = self.max_read.lock();
                    *saved_max = saved_max.max(max_sample);
                    if zeros < n {
                        last_read = Instant::now();
                    }
                }
                Err(err) => {
                    pcm.try_recover(err, false)?;
                    //if err.errno() != 11 {
                    //    return Err(CaptureDeviceError::Alsa(err));
                    //}
                }
            }
            //}
            if start.elapsed() >= file_duration {
                start = start.checked_add(file_duration).unwrap();
                #[cfg(feature = "audio")]
                {
                    writer.finalize()?;
                    nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
                    path = self.output_dir.join(format!("{nanos}.wav"));
                    writer = WavWriter::create(path.clone(), wav_spec)?;
                    sample = 0;
                }
            }
            if last_read.elapsed().as_secs() >= 2 {
                self.status.store(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }
}
