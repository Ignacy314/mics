//#![allow(unused)]
mod audio;
mod data;

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;

use alsa::pcm::Format;
#[cfg(feature = "audio")]
use crossbeam_channel::unbounded;
use flexi_logger::{with_thread, FileSpec, Logger};
#[cfg(feature = "audio")]
use hound::SampleFormat;
use log::{info, warn};
use parking_lot::Mutex;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::Signals;

use audio::CaptureDevice;
use audio::CaptureDeviceError;

#[cfg(feature = "audio")]
use self::audio::{AudioWriter, SEND_BUF_SIZE};

fn handle_capture_device_error(err: &CaptureDeviceError, status: &AtomicU8) {
    warn!("{err}");
    status.store(2, Ordering::Relaxed);
    //if !err.to_string().contains("(32)") {
    //    thread::sleep(Duration::from_millis(200));
    //}
}

fn main() {
    let home = match std::env::var("HOME") {
        Ok(var) => var,
        Err(err) => {
            warn!("Failed to load $HOME environmental variable: {err}\nChoosing current directory as working directory.");
            ".".to_owned()
        }
    };

    let andros_dir = format!("{home}/andros");
    let andros_dir = Path::new(&andros_dir);
    let andros_dir = match std::fs::create_dir_all(andros_dir) {
        Ok(()) => andros_dir,
        Err(err) => {
            warn!("Failed to create andros directory: {err}\nWriting data and logs to current directory.");
            Path::new(".")
        }
    };

    let ip: Option<(String, String)> = {
        let path = andros_dir.join("ip");
        let open = File::open(path);
        let ip = if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => Some(buf),
                Err(e) => {
                    warn!("Failed to read ip from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open ip file: {}", open.unwrap_err());
            None
        };

        let path = andros_dir.join("mac");
        let open = File::open(path);
        let mac = if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => Some(buf[(buf.len() - 6)..].to_string()),
                Err(e) => {
                    warn!("Failed to read mac from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open mac file: {}", open.unwrap_err());
            None
        };

        if let Some(ip) = ip {
            mac.map(|mac| (ip, mac))
        } else {
            None
        }
    };

    let log_dir = &andros_dir.join("log");
    if !log_dir.exists() {
        std::fs::create_dir(log_dir).unwrap_or_else(|e| {
            warn!("Failed to create {} data directory: {e}", log_dir.display())
        });
    }
    let data_dir = &andros_dir.join("data");
    if !data_dir.exists() {
        std::fs::create_dir(data_dir).unwrap_or_else(|e| {
            warn!("Failed to create {} data directory: {e}", data_dir.display())
        });
    }

    for dir in ["i2s", "umc", "data", "clock_umc", "clock_i2s"] {
        let path = data_dir.join(dir);
        if !path.exists() {
            std::fs::create_dir(path)
                .unwrap_or_else(|e| warn!("Failed to create {dir} data directory: {e}"));
        }
    }

    Logger::try_with_env_or_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().directory(log_dir))
        .duplicate_to_stderr(flexi_logger::Duplicate::All)
        .create_symlink(log_dir.join("current"))
        .format(with_thread)
        .use_utc()
        .start()
        .unwrap();

    let running = &AtomicBool::new(true);
    let i2s_status = &AtomicU8::new(0);
    let umc_status = &AtomicU8::new(0);

    thread::scope(|s| {
        let mut signals = Signals::new([SIGINT]).unwrap();
        s.spawn(move || {
            for sig in signals.forever() {
                if sig == signal_hook::consts::SIGINT {
                    running.store(false, Ordering::Relaxed);
                    println!();
                    break;
                }
            }
        });

        let i2s_max = Arc::new(Mutex::new(0i32));
        let umc_max = Arc::new(Mutex::new(0i32));

        // Create the Andros I2S microphone capture thread
        #[cfg(feature = "audio")]
        let (i2s_s, i2s_r) = unbounded::<[i32; SEND_BUF_SIZE]>();
        thread::Builder::new()
            .stack_size(1024 * 1024 * 32)
            .name("i2s".to_owned())
            .spawn_scoped(s, {
                let i2s_max = i2s_max.clone();
                move || {
                    let i2s = CaptureDevice::new(
                        "hw:CARD=ANDROSi2s,DEV=1",
                        4,
                        192_000,
                        Format::s32(),
                        running,
                        i2s_status,
                        i2s_max,
                    );
                    while running.load(Ordering::Relaxed) {
                        match i2s.read(
                            #[cfg(feature = "audio")]
                            i2s_s.clone(),
                        ) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, i2s_status),
                        };
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        thread::Builder::new()
            .name("i2s_processor".to_owned())
            .spawn_scoped(s, {
                move || {
                    let wav_spec = hound::WavSpec {
                        channels: 4,
                        sample_rate: 192000,
                        bits_per_sample: 32,
                        sample_format: SampleFormat::Int,
                    };
                    let output_dir = data_dir.join("i2s");
                    let clock_dir = data_dir.join("clock_i2s");
                    let mut writer =
                        AudioWriter::new(output_dir, clock_dir, wav_spec, i2s_r).unwrap();
                    while running.load(Ordering::Relaxed) {
                        match writer.receive() {
                            Ok(()) => {}
                            Err(err) => {
                                handle_capture_device_error(&err, umc_status);
                            }
                        };
                        if writer.time_to_write() {
                            writer = writer.write_wav().unwrap();
                        }
                    }
                }
            })
            .unwrap();

        // Create the UMC microphone capture thread
        #[cfg(feature = "audio")]
        let (umc_s, umc_r) = unbounded::<[i32; SEND_BUF_SIZE]>();
        thread::Builder::new()
            .stack_size(1024 * 1024 * 32)
            .name("umc".to_owned())
            .spawn_scoped(s, {
                let umc_max = umc_max.clone();
                move || {
                    let umc = CaptureDevice::new(
                        "hw:CARD=U192k,DEV=0",
                        2,
                        48_000,
                        Format::s32(),
                        running,
                        umc_status,
                        umc_max,
                    );
                    while running.load(Ordering::Relaxed) {
                        match umc.read(
                            #[cfg(feature = "audio")]
                            umc_s.clone(),
                        ) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, umc_status),
                        };
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        thread::Builder::new()
            .name("umc_processor".to_owned())
            .spawn_scoped(s, {
                move || {
                    let wav_spec = hound::WavSpec {
                        channels: 2,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: SampleFormat::Int,
                    };
                    let output_dir = data_dir.join("umc");
                    let clock_dir = data_dir.join("clock_umc");
                    let mut writer =
                        AudioWriter::new(output_dir, clock_dir, wav_spec, umc_r).unwrap();
                    while running.load(Ordering::Relaxed) {
                        match writer.receive() {
                            Ok(()) => {}
                            Err(err) => {
                                handle_capture_device_error(&err, umc_status);
                            }
                        };
                        if writer.time_to_write() {
                            writer = writer.write_wav().unwrap();
                        }
                    }
                }
            })
            .unwrap();

        let mut reader = data::Reader::new(
            #[cfg(feature = "sensors")]
            data_dir.join("data"),
            data_dir,
            i2s_status,
            umc_status,
            i2s_max,
            umc_max,
        );
        reader.read(running, s, ip);
    });
    info!("Exited properly");
}
