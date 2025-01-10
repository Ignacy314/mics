//#![allow(unused)]
mod audio;
mod data;

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

use alsa::pcm::Format;
use crossbeam_channel::unbounded;
use flexi_logger::{with_thread, FileSpec, Logger};
use log::{info, warn};
use rppal::gpio::Gpio;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::Signals;

use self::audio::CaptureDevice;
use self::audio::CaptureDeviceError;

const AUDIO_FILE_DURATION: Duration = Duration::from_secs(10);

fn handle_capture_device_error(err: &CaptureDeviceError) {
    warn!("{err}");
    thread::sleep(Duration::from_secs(1));
}

#[allow(clippy::too_many_lines)]
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

    let ip: Option<String> = {
        let path = andros_dir.join("ip");
        let open = File::open(path);
        if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => {
                    Some(buf)
                }
                Err(e) => {
                    warn!("Failed to read ip from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open ip file: {}", open.unwrap_err());
            None
        }
    };

    let log_dir = &andros_dir.join("log");
    let data_dir = &andros_dir.join("data");

    for dir in ["i2s", "umc", "data"] {
        let path = data_dir.join(dir);
        if !path.exists() {
            std::fs::create_dir(path)
                .unwrap_or_else(|e| warn!("Failed to create {dir} data directory: {e}"));
        }
    }

    //std::fs::create_dir(data_dir.clone())
    //    .unwrap_or_else(|e| warn!("Failed to create data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("i2s"))
    //    .unwrap_or_else(|e| warn!("Failed to create i2s data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("umc"))
    //    .unwrap_or_else(|e| warn!("Failed to create umc data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("data"))
    //    .unwrap_or_else(|e| warn!("Failed to create sensor data directory: {e}"));

    Logger::try_with_env()
        .unwrap()
        .log_to_file(FileSpec::default().directory(log_dir))
        .log_to_stderr()
        .print_message()
        .create_symlink(log_dir.join("current"))
        .format(with_thread)
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

        let gpio = Gpio::new().unwrap();
        let mut pps_pin = gpio.get(13).unwrap().into_input_pulldown();

        let (tx, rx) = unbounded();

        pps_pin
            .set_async_interrupt(
                rppal::gpio::Trigger::RisingEdge,
                Some(Duration::from_millis(5)),
                move |_| {
                    let now = chrono::Utc::now();
                    info!("PPS at UTC {now}");
                    let nanos = now.timestamp_nanos_opt().unwrap();
                    tx.send(nanos).unwrap();
                },
            )
            .unwrap();

        // Create the Andros I2S microphone capture thread
        thread::Builder::new()
            .name("i2s".to_owned())
            .spawn_scoped(s, {
                let rx = rx.clone();
                move || {
                    let i2s = CaptureDevice::new(
                        "hw:CARD=ANDROSi2s,DEV=1",
                        4,
                        192_000,
                        Format::s32(),
                        data_dir.join("i2s"),
                        running,
                        i2s_status,
                        rx,
                    );
                    while running.load(Ordering::Relaxed) {
                        match i2s.read(AUDIO_FILE_DURATION) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err),
                        };
                    }
                }
            })
            .unwrap();

        // Create the UMC microphone capture thread
        thread::Builder::new()
            .name("umc".to_owned())
            .spawn_scoped(s, {
                let rx = rx.clone();
                move || {
                    let umc = CaptureDevice::new(
                        "hw:CARD=U192k,DEV=0",
                        2,
                        48_000,
                        Format::s32(),
                        data_dir.join("umc"),
                        running,
                        umc_status,
                        rx,
                    );
                    while running.load(Ordering::Relaxed) {
                        match umc.read(AUDIO_FILE_DURATION) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err),
                        };
                    }
                }
            })
            .unwrap();

        let mut reader = data::Reader::new(data_dir.join("data"), data_dir, i2s_status, umc_status);
        reader.read(running, s, ip);
    });
    info!("Exited properly");
}
