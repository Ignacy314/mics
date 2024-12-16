use std::io::BufRead;
use std::time::Duration;

use chrono::NaiveTime;
use rppal::uart::{Parity, Uart};
use serde::{Deserialize, Serialize};

use super::Device;

#[derive(Debug)]
pub struct Gps {
    device: Uart,
}

impl Gps {
    pub fn new(port: &str, baud_rate: u32, timeout: Duration) -> Result<Self, Error> {
        let mut uart = Uart::with_path(port, baud_rate, Parity::None, 8, 1)?;
        uart.set_read_mode(0, timeout)?;
        Ok(Self { device: uart })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    longitude: f64,
    latitude: f64,
    altitude: f32,
    timestamp: NaiveTime,
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
