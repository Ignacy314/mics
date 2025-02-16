use std::cmp::Ordering;
use std::thread;

use circular_buffer::CircularBuffer;
use ina219::address::Address;
use ina219::calibration::UnCalibrated;
use ina219::SyncIna219;
use serde::{Deserialize, Serialize};

use super::Device;

const DATA_SECONDS: usize = 150;
const VOL_SAMPLES: usize = DATA_SECONDS * 10;
const VOTERS: usize = 10;
const PARTS: usize = 10;

pub struct Ina {
    device: SyncIna219<rppal::i2c::I2c, UnCalibrated>,
    voltage: CircularBuffer<VOL_SAMPLES, u32>,
    bat_status: CircularBuffer<VOTERS, i8>,
    prev_charge: Charge,
}

impl Ina {
    pub fn new() -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::new()?;
        let ina = SyncIna219::new(i2c, Address::from_byte(0x40)?)?;
        Ok(Self {
            device: ina,
            voltage: CircularBuffer::new(),
            bat_status: CircularBuffer::new(),
            prev_charge: Charge::default(),
        })
    }

    fn charging(&self) -> Ordering {
        const SIZE: usize = VOL_SAMPLES / PARTS;

        let mean_1 = self.voltage.iter().take(SIZE).sum::<u32>() as f32 / SIZE as f32;
        let mean_2 = self.voltage.iter().skip(VOL_SAMPLES - SIZE).take(SIZE).sum::<u32>() as f32 / SIZE as f32;

        mean_2.total_cmp(&mean_1)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Charge {
    Unknown,
    Charging(u16),
    Discharging(u16),
    CriticalError,
    CriticalDischarge,
}

impl Default for Charge {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    pub bus_voltage: u16,
    shunt_voltage: i32,
    current: u16,
    power: f32,
    charge: Charge,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I2C error")]
    I2C(#[from] rppal::i2c::Error),
    #[error("INA address error")]
    InaAddress(#[from] ina219::address::OutOfRange),
    #[error("INA init error")]
    InaInit(#[from] ina219::errors::InitializationError<rppal::i2c::I2c, rppal::i2c::Error>),
    #[error("INA config read error")]
    InaConfigRead(#[from] ina219::errors::ConfigurationReadError<rppal::i2c::Error>),
    #[error("INA measurement error")]
    InaMeasurement(#[from] ina219::errors::MeasurementError<rppal::i2c::Error>),
    #[error("INA bus voltage read error")]
    InaBusRead(#[from] ina219::errors::BusVoltageReadError<rppal::i2c::Error>),
    #[error("INA shunt voltage read error")]
    InaShuntRead(#[from] ina219::errors::ShuntVoltageReadError<rppal::i2c::Error>),
}

impl Device for Ina {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        if let Some(time) = self.device.configuration()?.conversion_time() {
            thread::sleep(time);
        }

        let bus_voltage = (self.device.bus_voltage()?).voltage_mv();
        let shunt_voltage = (self.device.shunt_voltage()?).shunt_voltage_uv();
        let current = (self.device.current_raw()?).0 * 10;
        let power = shunt_voltage.unsigned_abs() as f32 / 100.0;

        self.voltage.push_back(u32::from(bus_voltage));
        if self.voltage.is_full() {
            self.bat_status.push_back(match self.charging() {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            });
        }
        let sum = self.bat_status.iter().sum::<i8>();
        let charge = if !self.voltage.is_full() {
            Charge::Unknown
        } else if bus_voltage >= 15000 {
            Charge::CriticalError
        } else if bus_voltage <= 10000 {
            Charge::CriticalDischarge
        } else if bus_voltage >= 13000 || sum > 0 {
            let percentage = ((bus_voltage - 10500) / 43).clamp(0, 100);
            Charge::Charging(percentage)
        } else if sum < 0 {
            let percentage = ((bus_voltage - 10500) / 24).clamp(0, 100);
            Charge::Discharging(percentage)
        } else {
            self.prev_charge
        };

        self.prev_charge = charge;

        Ok(Self::Data {
            bus_voltage,
            shunt_voltage,
            current,
            power,
            charge,
        })
    }
}
