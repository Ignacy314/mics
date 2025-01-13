use core::fmt;
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use super::Device;

pub struct Bmp {
    device: bmp280::Bmp280,
}

impl Debug for Bmp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[derive(Debug)]
        struct Bmp {}
        fmt::Debug::fmt(&Bmp {}, f)
    }
}

impl Bmp {
    pub fn new() -> Result<Self, Error> {
        let bmp = bmp280::Bmp280Builder::new()
            .ground_pressure(101_325.0)
            .build()?;
        Ok(Self { device: bmp })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    temperature: f32,
    pressure: f32,
    altitude: f32,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("BMP280 error")]
    Bmp(#[from] bmp280::Error),
}

impl Device for Bmp {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        let temperature = self.device.temperature_celsius()?;
        let pressure = self.device.pressure_kpa()? * 10.0;
        let altitude = self.device.altitude_m_relative(101_325.0)?;
        Ok(Self::Data { temperature, pressure, altitude })
    }
}
