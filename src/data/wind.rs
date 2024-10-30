use std::thread;
//#![allow(unused)]
use std::time::{Duration, Instant};

use rppal::uart::{Parity, Uart};
use serde::{Deserialize, Serialize};

use super::Device;

#[derive(Debug)]
pub struct Wind {
    device: Uart,
}

impl Wind {
    const QUERY: [u8; 8] = [0x01, 0x03, 0x00, 0x00, 0x00, 0x26, 0xC4, 0x10];

    pub fn new(port: &str, baud_rate: u32, timeout: Duration) -> Result<Self, Error> {
        let mut uart = Uart::with_path(port, baud_rate, Parity::None, 8, 1)?;
        uart.set_read_mode(0, timeout)?;
        uart.set_write_mode(true)?;
        Ok(Self { device: uart })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    dir: u16,
    speed: f32,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("No data")]
    NoData,
    #[error("UART error")]
    Uart(#[from] rppal::uart::Error),
}

impl Device for Wind {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        const TIMEOUT: Duration = Duration::from_millis(1000);

        self.device.flush(rppal::uart::Queue::Both)?;
        self.device.write(&Self::QUERY)?;
        let start = Instant::now();
        thread::sleep(Duration::from_millis(200));
        let mut elapsed = start.elapsed();
        //while self.device.input_len()? < 81 && elapsed < TIMEOUT {
        //    elapsed = start.elapsed();
        //}
        //eprintln!("{}", self.device.input_len()?);
        //if elapsed >= TIMEOUT {
        //    return Err(Error::NoData);
        //}
        let mut buf = [0u8; 81];
        let n_bytes = self.device.read(&mut buf)?;
        eprintln!("{n_bytes}");
        let dir = u16::from_be_bytes(buf[5..7].try_into().unwrap());
        let speed = f32::from_be_bytes(buf[7..11].try_into().unwrap());

        Ok(Data { dir, speed })
    }
}
