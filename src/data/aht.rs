use core::fmt;
use std::fmt::Debug;

use aht10::AHT10;
use rppal::hal::Delay;
use rppal::i2c::I2c;
use serde::{Deserialize, Serialize};

use super::Device;

pub struct Aht {
    device: AHT10<I2c, Delay>,
}

impl Debug for Aht {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[derive(Debug)]
        struct Aht {}
        fmt::Debug::fmt(&Aht {}, f)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I2c error")]
    I2c(#[from] rppal::i2c::Error),
    #[error("AHT10 error")]
    Aht(aht10::Error<rppal::i2c::Error>),
}

impl From<aht10::Error<rppal::i2c::Error>> for Error {
    fn from(value: aht10::Error<rppal::i2c::Error>) -> Self {
        Error::Aht(value)
    }
}

impl Aht {
    pub fn new(bus: u8) -> Result<Self, Error> {
        let i2c = I2c::with_bus(bus)?;
        let delay = Delay::new();
        let aht = AHT10::new(i2c, delay)?;
        Ok(Self { device: aht })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Data {
    pub humidity: f32,
    pub temperature: f32,
}

impl Device for Aht {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        let data = self.device.read()?;
        Ok(Self::Data {
            humidity: data.0.rh(),
            temperature: data.1.celsius(),
        })
    }
}
