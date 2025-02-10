use std::io::BufRead;
use std::time::Duration;

use chrono::NaiveDateTime;
use rppal::uart::{Parity, Uart};
use serde::{Deserialize, Serialize};

use super::Device;

#[derive(Debug)]
pub struct Gps {
    device: Uart,
}

//const B115200: [u8; 37] = [
//    0xb5, 0x62, 0x06, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0xd0, 0x08, 0x00, 0x00, 0x00, 0xc2,
//    0x01, 0x00, 0x07, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc4, 0x96, 0xb5, 0x62, 0x06, 0x00,
//    0x01, 0x00, 0x01, 0x08, 0x22,
//];
//
//const B9600: [u8; 37] = [
//    0xb5, 0x62, 0x06, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0xd0, 0x08, 0x00, 0x00, 0x80, 0x25,
//    0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa6, 0xcd, 0xb5, 0x62, 0x06, 0x00,
//    0x01, 0x00, 0x01, 0x08, 0x22,
//];
//
//const BAUD_RATES: [u32; 4] = [4800, 9600, 38400, 115200];

//fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
//    (0..s.len())
//        .step_by(2)
//        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
//        .collect()
//}

impl Gps {
    pub fn new(port: &str, baud_rate: u32, timeout: Duration) -> Result<Self, Error> {
        let mut uart = Uart::with_path(port, baud_rate, Parity::None, 8, 1)?;
        uart.set_read_mode(0, timeout)?;
        //match baud_rate {
        //    9_600 => {
        //        for br in BAUD_RATES {
        //            uart.set_baud_rate(br).unwrap();
        //            uart.write(&B9600).unwrap();
        //            thread::sleep(Duration::from_millis(100));
        //        }
        //        uart.set_baud_rate(9_600).unwrap();
        //    }
        //    115_200 => {
        //        for br in BAUD_RATES {
        //            uart.set_baud_rate(br).unwrap();
        //            uart.write(&B115200).unwrap();
        //            thread::sleep(Duration::from_millis(100));
        //        }
        //        uart.set_baud_rate(115_200).unwrap();
        //    }
        //    _ => {
        //        warn!("unsupported GPS baud rate; defaulting to 9600");
        //        for br in BAUD_RATES {
        //            uart.set_baud_rate(br).unwrap();
        //            uart.write(&B9600).unwrap();
        //            thread::sleep(Duration::from_millis(100));
        //        }
        //        uart.set_baud_rate(9_600).unwrap();
        //    }
        //}
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
