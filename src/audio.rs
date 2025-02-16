#[cfg(feature = "audio")]
use std::fs::File;
use std::io;
#[cfg(feature = "audio")]
use std::io::BufWriter;
#[cfg(feature = "audio")]
use std::io::Write;
#[cfg(feature = "audio")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use alsa::{
    pcm::{Access, Format, HwParams, PCM},
    Direction, Error, ValueOr,
};
#[cfg(feature = "audio")]
use hound::{SampleFormat, WavSpec, WavWriter};
use log::info;
use parking_lot::Mutex;

#[cfg(feature = "audio")]
const AUDIO_FILE_DURATION: Duration = Duration::from_secs(10);

#[cfg(feature = "audio")]
struct AudioWriter {
    wav_writer: WavWriter<BufWriter<File>>,
    clock_writer: BufWriter<File>,
    wav_file: String,
    output_dir: PathBuf,
    sample: usize,
    file_start: Instant,
    clock: Instant,
    wav_spec: WavSpec,
}

#[cfg(feature = "audio")]
impl AudioWriter {
    fn new(
        output_dir: PathBuf,
        clock_dir: PathBuf,
        wav_spec: hound::WavSpec,
    ) -> Result<Self, CaptureDeviceError> {
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let path = output_dir.join(format!("{nanos}.wav"));
        let writer = WavWriter::create(path.clone(), wav_spec)?;

        let clock_path = clock_dir.join(format!("{nanos}.csv"));
        let mut clock_writer = BufWriter::new(File::create(clock_path)?);
        writeln!(clock_writer, "time,file,sample")?;

        Ok(Self {
            wav_file: path.file_name().unwrap().to_str().unwrap().to_string(),
            wav_writer: writer,
            clock_writer,
            output_dir,
            sample: 0,
            file_start: Instant::now(),
            clock: Instant::now(),
            wav_spec,
        })
    }

    fn write_clock(&mut self) -> Result<(), CaptureDeviceError> {
        self.clock = self.clock.checked_add(Duration::from_secs(1)).unwrap();
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        writeln!(self.clock_writer, "{nanos},{},{}", self.sample, self.wav_file)?;
        self.clock_writer.flush()?;
        Ok(())
    }

    fn write_wav(mut self) -> Result<Self, CaptureDeviceError> {
        self.file_start = self.file_start.checked_add(AUDIO_FILE_DURATION).unwrap();
        self.wav_writer.finalize()?;
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let path = self.output_dir.join(format!("{nanos}.wav"));
        self.wav_writer = WavWriter::create(path.clone(), self.wav_spec)?;
        self.sample = 0;
        Ok(self)
    }

    fn write_sample(&mut self, sample: i32) -> Result<(), CaptureDeviceError> {
        self.wav_writer.write_sample(sample)?;
        Ok(())
    }

    fn inc_sample(&mut self, s: usize) {
        self.sample += s;
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CaptureDeviceError {
    //#[error("Format unimplemented: {0}")]
    //FormatUnimplemented(Format),
    #[error("Alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("Hound error: {0}")]
    Hound(#[from] hound::Error),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Audio error: {0}")]
    Other(String),
}

pub struct CaptureDevice<'a> {
    device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    #[cfg(feature = "audio")]
    output_dir: PathBuf,
    #[cfg(feature = "audio")]
    clock_dir: PathBuf,
    running: &'a AtomicBool,
    status: &'a AtomicU8,
    max_read: Arc<Mutex<i32>>,
}

#[allow(clippy::too_many_arguments)]
impl<'a> CaptureDevice<'a> {
    pub fn new(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        #[cfg(feature = "audio")] output_dir: PathBuf,
        #[cfg(feature = "audio")] clock_dir: PathBuf,
        running: &'a AtomicBool,
        status: &'a AtomicU8,
        max_read: Arc<Mutex<i32>>,
    ) -> Self {
        Self {
            device_name: device_name.to_owned(),
            channels,
            samplerate,
            format,
            #[cfg(feature = "audio")]
            output_dir,
            #[cfg(feature = "audio")]
            clock_dir,
            running,
            status,
            max_read,
        }
    }

    fn init_device(&self) -> Result<PCM, Error> {
        let pcm = PCM::new(&self.device_name, Direction::Capture, true)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(self.channels)?;
            hwp.set_rate(self.samplerate, ValueOr::Nearest)?;
            hwp.set_format(self.format)?;
            hwp.set_access(Access::RWInterleaved)?;
            let buf_size = hwp.get_buffer_size_max()?;
            hwp.set_buffer_size(buf_size)?;
            pcm.hw_params(&hwp)?;
        }
        //let rate = {
        //    let hwp = pcm.hw_params_current()?;
        //    let swp = pcm.sw_params_current()?;
        //    let buf_size = hwp.get_buffer_size()?;
        //    swp.set_start_threshold(v)
        //};
        pcm.prepare()?;
        pcm.start()?;
        Ok(pcm)
    }

    pub fn read(&self) -> Result<(), CaptureDeviceError> {
        let pcm = self.init_device()?;

        let mut mmap = pcm.direct_mmap_capture::<i32>();

        let mut io = if mmap.is_err() {
            Some(pcm.io_i32()?)
        } else {
            None
        };

        //let io = match &self.format {
        //    Format::S32LE | Format::S32BE => pcm.io_i32()?,
        //    default => return Err(CaptureDeviceError::FormatUnimplemented(*default)),
        //};

        //let mut buf = [0i32; 1024 * 128];
        #[cfg(feature = "audio")]
        let wav_spec = hound::WavSpec {
            channels: self.channels as u16,
            sample_rate: self.samplerate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Int,
        };

        #[cfg(feature = "audio")]
        let mut writer =
            AudioWriter::new(self.output_dir.clone(), self.clock_dir.clone(), wav_spec)?;

        let mut last_read = Instant::now();
        info!("start audio read");
        while self.running.load(Ordering::Relaxed) {
            let start = Instant::now();
            #[cfg(feature = "audio")]
            if writer.clock.elapsed() >= Duration::from_secs(1) {
                writer.write_clock()?;
            }

            if let Ok(ref mut mmap) = mmap {
                let s = mmap.avail();
                if s > 0 {
                    let n = s * self.channels as i64;
                    let mut max_sample = i32::MIN;
                    let mut zeros = 0;
                    for sample in mmap.iter() {
                        if sample.abs() > max_sample {
                            max_sample = sample;
                        }
                        if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                            zeros += 1;
                        }
                        #[cfg(feature = "audio")]
                        writer.write_sample(sample)?;
                    }
                    mmap.commit(s);
                    if zeros < n {
                        last_read = Instant::now();
                    }
                    #[cfg(feature = "audio")]
                    writer.inc_sample(s as usize);
                    let mut saved_max = self.max_read.lock();
                    *saved_max = saved_max.max(max_sample);
                }
                use alsa::pcm::State;
                match mmap.status().state() {
                    State::Running => {} // All fine
                    State::Prepared => {
                        info!("Starting audio output stream");
                        pcm.start()?;
                    }
                    State::XRun => {
                        info!("Underrun in audio output stream!");
                        pcm.prepare()?;
                    }
                    State::Suspended => {
                        println!("Resuming audio output stream");
                        pcm.resume()?;
                    }
                    n => {
                        return Err(CaptureDeviceError::Other(format!(
                            "Unexpected pcm state {:?}",
                            n
                        )))
                    }
                };
            } else if let Some(ref mut io) = io {
                let s = match pcm.avail_update() {
                    Ok(n) => n,
                    Err(e) => {
                        info!("Recovering from {}", e);
                        pcm.recover(e.errno() as std::os::raw::c_int, true)?;
                        pcm.avail_update()?
                    }
                } as usize;

                if s > 0 {
                    io.mmap(s, |buf| {
                        let n = buf.len();
                        let mut max_sample = i32::MIN;
                        let mut zeros = 0;
                        for sample in buf {
                            if sample.abs() > max_sample {
                                max_sample = *sample;
                            }
                            if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                                zeros += 1;
                            }
                            #[cfg(feature = "audio")]
                            writer.write_sample(*sample).unwrap();
                        }
                        if zeros < n {
                            last_read = Instant::now();
                        }
                        #[cfg(feature = "audio")]
                        writer.inc_sample(s as usize);
                        let mut saved_max = self.max_read.lock();
                        *saved_max = saved_max.max(max_sample);
                        n / self.channels as usize
                    })?;
                };
            };

            //match io.readi(&mut buf) {
            //    Ok(s) => {
            //        let n = s * self.channels as usize;
            //        let mut max_sample = i32::MIN;
            //        let mut zeros = 0;
            //        for sample in &buf[0..n] {
            //            if sample.abs() > max_sample {
            //                max_sample = *sample;
            //            }
            //            if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
            //                zeros += 1;
            //            }
            //            #[cfg(feature = "audio")]
            //            writer.write_sample(*sample)?;
            //        }
            //        if zeros < n {
            //            last_read = Instant::now();
            //        }
            //        #[cfg(feature = "audio")]
            //        writer.inc_sample(s);
            //        let mut saved_max = self.max_read.lock();
            //        *saved_max = saved_max.max(max_sample);
            //    }
            //    Err(err) => {
            //        if err.errno() != 11 {
            //            info!("ALSA try recover from: {err}");
            //            pcm.try_recover(err, false)?;
            //        }
            //    }
            //}

            if last_read.elapsed().as_secs() >= 2 {
                self.status.store(1, Ordering::Relaxed);
            }

            #[cfg(feature = "audio")]
            if writer.file_start.elapsed() >= AUDIO_FILE_DURATION {
                writer = writer.write_wav()?;
            }

            thread::sleep(Duration::from_millis(1).saturating_sub(start.elapsed()));
        }

        Ok(())
    }
}
