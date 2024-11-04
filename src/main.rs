//#![allow(unused)]
mod audio;
mod data;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use alsa::pcm::Format;
use crossbeam_channel::unbounded;
use flexi_logger::{with_thread, FileSpec, Logger};
use log::{info, warn};
use rppal::gpio::Gpio;

use self::audio::CaptureDevice;
use self::audio::CaptureDeviceError;

const AUDIO_FILE_DURATION: Duration = Duration::from_secs(10);

fn handle_capture_device_error(err: &CaptureDeviceError) {
    match err {
        audio::CaptureDeviceError::FormatUnimplemented(e) => unimplemented!("{e}"),
        audio::CaptureDeviceError::AlsaError(e) => {
            warn!("Alsa error: {e}");
            thread::sleep(Duration::from_secs(1));
        }
    }
}

fn main() {
    Logger::try_with_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().directory("../log"))
        .print_message()
        .create_symlink("../log/current")
        .format(with_thread)
        .start()
        .unwrap();

    let running = Arc::new(AtomicBool::new(true));
    // set Ctrl-C interrupt handler to set the 'running' atomic bool to false
    {
        let running = running.clone();
        ctrlc::set_handler(move || {
            running.store(false, Ordering::Relaxed);
        })
        .expect("Error setting Ctrl-C handler");
    }

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
    //let andros_status = Arc::new(AtomicU8::new(0));
    //let andros_thread = {
    //    let running = running.clone();
    //    let status = andros_status.clone();
    //    let rx = rx.clone();
    //    thread::spawn(move || {
    //        let andros = CaptureDevice::new(
    //            "hw:CARD=ANDROSi2s,DEV=1",
    //            4,
    //            192_000,
    //            Format::s32(),
    //            "../data/i2s/",
    //            running.clone(),
    //            status,
    //            rx,
    //        );
    //        while running.load(Ordering::Relaxed) {
    //            match andros.read(AUDIO_FILE_DURATION) {
    //                Ok(()) => {}
    //                Err(err) => handle_capture_device_error(&err),
    //            };
    //        }
    //    })
    //};

    // Create the UMC microphone capture thread
    //let umc_status = Arc::new(AtomicU8::new(0));
    //let umc_thread = {
    //    let running = running.clone();
    //    let status = umc_status.clone();
    //    let rx = rx.clone();
    //    thread::spawn(move || {
    //        let umc = CaptureDevice::new(
    //            "hw:CARD=U192k,DEV=0",
    //            2,
    //            48_000,
    //            Format::s32(),
    //            "../data/umc/",
    //            running.clone(),
    //            status,
    //            rx,
    //        );
    //        while running.load(Ordering::Relaxed) {
    //            match umc.read(AUDIO_FILE_DURATION) {
    //                Ok(()) => {}
    //                Err(err) => handle_capture_device_error(&err),
    //            };
    //        }
    //    })
    //};

    let data_thread = {
        let running = running.clone();
        thread::spawn(move || {
            let mut reader = data::Reader::new();
            reader.read(&running);
        })
    };

    while running.load(Ordering::Relaxed) {
        let start = Instant::now();
        //println!("Andros I2S status: {}", andros_status.load(Ordering::Relaxed));
        //println!("UMC status: {}", umc_status.load(Ordering::Relaxed));
        thread::sleep(Duration::from_secs(2).saturating_sub(start.elapsed()));
    }
    //andros_thread.join().unwrap();
    //umc_thread.join().unwrap();
    data_thread.join().unwrap();
}
