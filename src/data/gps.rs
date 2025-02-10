use std::io::BufRead;
use std::num::ParseIntError;
use std::time::Duration;

use chrono::NaiveDateTime;
use log::info;
use rppal::uart::{Parity, Uart};
use serde::{Deserialize, Serialize};

use super::Device;

#[derive(Debug)]
pub struct Gps {
    device: Uart,
}

fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

impl Gps {
    pub fn new(port: &str, baud_rate: u32, timeout: Duration) -> Result<Self, Error> {
        let mut uart = Uart::with_path(port, 9600, Parity::None, 8, 1)?;
        uart.set_read_mode(0, timeout)?;
        let msg = "b5620600140001000000d008000000c201000700070000000000c496b56206000100010822";
        let msg2 = "B56206090D0000000000FFFF0000000000001731BF";
        let bytes = decode_hex(msg).unwrap();
        let bytes2 = decode_hex(msg2).unwrap();
        uart.write(&bytes).unwrap();
        uart.write(&bytes2).unwrap();
        //uart.set_baud_rate(baud_rate).unwrap();
        Ok(Self { device: uart })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    longitude: f64,
    latitude: f64,
    altitude: f32,
    timestamp: NaiveDateTime,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("UART error")]
    Uart(#[from] rppal::uart::Error),
    #[error("NMEA error")]
    Nmea(#[from] nmea::Error<'static>),
    #[error("Data overflow")]
    DataOverflow,
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("No data")]
    NoData,
    #[error("Invalid NMEA string")]
    InvalidNmeaString,
}

impl Device for Gps {
    type Data = Data;
    type Error = Error;
    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        let mut buf = [0u8; 8192];

        let bytes = self.device.read(&mut buf)?;
        if bytes == 1024 && self.device.input_len()? > 0 {
            return Err(Error::DataOverflow);
        }

        let lines = buf.lines();

        //eprintln!("{lines:?}");
        //info!("{lines:?}");

        let gga = lines.filter(|l| {
            if let Ok(l) = l {
                //eprintln!("{l}");
                l.starts_with("$GPGGA")
            } else {
                false
            }
        });

        //eprintln!("{gga:?}");
        let gga = gga.last();

        //eprintln!("{gga:?}");

        let Some(Ok(line)) = gga else {
            return Err(Error::NoData);
        };

        let Ok(data) = nmea::parse_str(line.as_str()) else {
            return Err(Error::InvalidNmeaString);
        };

        match data {
            nmea::ParseResult::GGA(d) => {
                let Some(longitude) = d.longitude else {
                    return Err(Error::NoData);
                };
                let Some(latitude) = d.latitude else {
                    return Err(Error::NoData);
                };
                let Some(timestamp) = d.fix_time else {
                    return Err(Error::NoData);
                };
                let Some(altitude) = d.altitude else {
                    return Err(Error::NoData);
                };
                let timestamp = NaiveDateTime::new(chrono::Utc::now().date_naive(), timestamp);
                Ok(Self::Data {
                    longitude,
                    latitude,
                    altitude,
                    timestamp,
                })
            }
            _ => Err(Error::InvalidNmeaString),
        }
    }
}
