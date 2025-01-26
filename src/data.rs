use parking_lot::Mutex;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::thread::Scope;
use std::time::{Duration, Instant};
use sysinfo::DiskRefreshKind;
use sysinfo::Disks;

use ::serde::{Deserialize, Serialize};
use log::{error, info, warn};

use self::aht::Aht;
use self::bmp::Bmp;
use self::device_manager::{DeviceManager, Status, Statuses};
use self::gps::Gps;
use self::imu::Imu;
use self::wind::Wind;

pub mod aht;
pub mod bmp;
pub mod device_manager;
pub mod gps;
pub mod imu;
pub mod ina;
pub mod wind;

pub trait Device {
    type Data;
    type Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Data {
    gps: Option<gps::Data>,
    aht: Option<aht::Data>,
    wind: Option<wind::Data>,
    imu: Option<imu::Data>,
    bmp: Option<bmp::Data>,
    ina: Option<ina::Data>,
}

pub struct Reader<'a> {
    pub device_manager: DeviceManager<'a>,
    pub path: PathBuf,
    pub calib_path: &'a PathBuf,
    pub data_link: PathBuf,
    pub read_period: Duration,
    i2s_status: &'a AtomicU8,
    umc_status: &'a AtomicU8,
    i2s_max: Arc<Mutex<i32>>,
    umc_max: Arc<Mutex<i32>>,
}

impl<'a> Reader<'a> {
    const PERIOD_MILLIS: u64 = 5000;

    pub fn new<P: Into<PathBuf>>(
        path: P,
        calib_path: &'a PathBuf,
        i2s_status: &'a AtomicU8,
        umc_status: &'a AtomicU8,
        i2s_max: Arc<Mutex<i32>>,
        umc_max: Arc<Mutex<i32>>,
    ) -> Self {
        let path: PathBuf = path.into();
        let data_link = path.join("current");
        Self {
            device_manager: DeviceManager::new(),
            path,
            calib_path,
            data_link,
            read_period: Duration::from_millis(Self::PERIOD_MILLIS),
            i2s_status,
            umc_status,
            i2s_max,
            umc_max,
        }
    }

    fn handle_gps_data_error(&mut self, err: &gps::Error) {
        self.device_manager.statuses.gps = Status::NoData;
        warn!("GPS data error: {err}");
    }

    fn handle_gps_init_error(&mut self, err: &gps::Error) {
        match err {
            gps::Error::Uart(uart_err) => {
                warn!("GPS init failed: {uart_err}");
                self.device_manager.statuses.gps = Status::Disconnected;
            }
            _ => unreachable!(),
        }
    }

    fn handle_aht_data_error(&mut self, err: &aht::Error) {
        self.device_manager.statuses.aht = Status::NoData;
        error!("AHT10 data error: {err}");
    }

    fn handle_aht_init_error(&mut self, err: &aht::Error) {
        match err {
            aht::Error::I2c(i2c_err) => {
                warn!("AHT10 init failed: {i2c_err}");
                self.device_manager.statuses.aht = Status::Disconnected;
            }
            aht::Error::Aht(_) => unreachable!(),
        }
    }

    fn handle_bmp_data_error(&mut self, err: &bmp::Error) {
        self.device_manager.statuses.bmp = Status::NoData;
        error!("BMP280 data error: {err}");
    }

    fn handle_bmp_init_error(&mut self, err: &bmp::Error) {
        match err {
            bmp::Error::Bmp(err) => {
                warn!("BMP280 init failed: {err}");
                self.device_manager.statuses.bmp = Status::Disconnected;
            }
        }
    }

    fn handle_ina_data_error(&mut self, err: &ina::Error) {
        self.device_manager.statuses.ina = Status::NoData;
        error!("INA219 data error: {err}");
    }

    fn handle_ina_init_error(&mut self, err: &ina::Error) {
        self.device_manager.statuses.ina = Status::Disconnected;
        warn!("INA219 init failed: {err}");
    }

    #[allow(clippy::too_many_lines)]
    pub fn read<'b>(
        &mut self,
        running: &'a AtomicBool,
        s: &'a Scope<'a, 'b>,
        ip: Option<(String, String)>,
    ) {
        let imu_data = Arc::new(Mutex::new((imu::Data::default(), Status::default())));
        thread::Builder::new()
            .name("imu".to_owned())
            .spawn_scoped(s, {
                let data = imu_data.clone();
                let bus = self.device_manager.settings.imu_bus;
                let period = Duration::from_millis(100);
                let path = self.calib_path.clone();
                move || {
                    let samples: usize = 10000 / period.as_millis() as usize;
                    let mut imu: Option<Imu> = None;
                    while running.load(Ordering::Relaxed) {
                        let start = Instant::now();

                        if let Some(imu) = imu.as_mut() {
                            match imu.get_data() {
                                Ok(d) => {
                                    *data.lock() = (d, Status::Ok);
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::NoData;
                                }
                            }
                        } else {
                            match Imu::new(bus, samples, &path) {
                                Ok(mut device) => match device.calibrate(true) {
                                    Ok(()) => {
                                        info! {"IMU device initialized"};
                                        imu = Some(device);
                                        data.lock().1 = Status::NoData;
                                    }
                                    Err(err) => {
                                        warn!("{err}");
                                        data.lock().1 = Status::Disconnected;
                                    }
                                },
                                Err(err) => {
                                    warn!("IMU init: {err}");
                                    data.lock().1 = Status::Disconnected;
                                }
                            };
                        }

                        thread::sleep(period.saturating_sub(start.elapsed()));
                    }
                }
            })
            .unwrap();

        let wind_data = Arc::new(Mutex::new((wind::Data::default(), Status::default())));
        thread::Builder::new()
            .name("wind".to_owned())
            .spawn_scoped(s, {
                let data = wind_data.clone();
                let settings = self.device_manager.settings.wind;
                let period = Duration::from_millis(1000);
                move || {
                    let mut wind: Option<Wind> = None;
                    while running.load(Ordering::Relaxed) {
                        let start = Instant::now();

                        if let Some(wind) = wind.as_mut() {
                            match wind.get_data() {
                                Ok(d) => {
                                    *data.lock() = (d, Status::Ok);
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::NoData;
                                }
                            }
                        } else {
                            match Wind::new(settings.port, settings.baud_rate, settings.timeout) {
                                Ok(device) => {
                                    info! {"Wind device initialized"};
                                    wind = Some(device);
                                    data.lock().1 = Status::NoData;
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::Disconnected;
                                }
                            };
                        }

                        thread::sleep(period.saturating_sub(start.elapsed()));
                    }
                }
            })
            .unwrap();

        let mut disks = Disks::new_with_refreshed_list();
        //for disk in disks.list() {
        //    info!("disk: {:?}", disk.mount_point());
        //    info!("disk mount is / : {}", disk.mount_point() == Path::new("/"));
        //}
        let mut disk = disks
            .list_mut()
            .iter_mut()
            .find(|d| d.mount_point() == Path::new("/"));
        //info!("option disk: {disk:?}");

        let (client, (ip, mac)) = if let Some((ip, mac)) = ip {
            (Some(reqwest::blocking::Client::new()), (ip, mac))
        } else {
            (None, (String::new(), String::new()))
        };

        #[cfg(all(feature = "audio", feature = "sensors"))]
        {
            self.device_manager.statuses.writing = "audio,sensors";
        }
        #[cfg(all(feature = "audio", not(feature = "sensors")))]
        {
            self.device_manager.statuses.writing = "audio";
        }
        #[cfg(all(feature = "sensors", not(feature = "audio")))]
        {
            self.device_manager.statuses.writing = "sensors";
        }

        //let client = reqwest::blocking::Client::new();
        while running.load(Ordering::Relaxed) {
            let start = Instant::now();

            let mut data = Data::default();

            if let Some(guard) = imu_data.try_lock_for(Duration::from_millis(50)) {
                let (imu_data, imu_status) = *guard;
                drop(guard);
                if imu_status == Status::Ok {
                    data.imu = Some(imu_data);
                }
                self.device_manager.statuses.imu = imu_status;
            } else {
                self.device_manager.statuses.imu = Status::NoData;
            }

            if let Some(guard) = wind_data.try_lock_for(Duration::from_millis(50)) {
                let (wind_data, wind_status) = *guard;
                drop(guard);
                if wind_status == Status::Ok {
                    data.wind = Some(wind_data);
                }
                self.device_manager.statuses.wind = wind_status;
            } else {
                self.device_manager.statuses.wind = Status::NoData;
            }

            if let Some(gps) = self.device_manager.gps.as_mut() {
                match gps.get_data() {
                    Ok(d) => {
                        self.device_manager.statuses.gps = Status::Ok;
                        data.gps = Some(d);
                    }
                    Err(e) => {
                        self.handle_gps_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_gps() {
                    Ok(()) => {
                        info!("GPS device initialized");
                    }
                    Err(e) => {
                        self.handle_gps_init_error(&e);
                    }
                }
            }

            if let Some(aht) = self.device_manager.aht.as_mut() {
                match aht.get_data() {
                    Ok(d) => {
                        self.device_manager.statuses.aht = Status::Ok;
                        data.aht = Some(d);
                    }
                    Err(e) => {
                        self.handle_aht_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_aht() {
                    Ok(()) => {
                        info!("AHT10 device initialized");
                    }
                    Err(e) => {
                        self.handle_aht_init_error(&e);
                    }
                }
            }

            if let Some(bmp) = self.device_manager.bmp.as_mut() {
                match bmp.get_data() {
                    Ok(d) => {
                        self.device_manager.statuses.bmp = Status::Ok;
                        data.bmp = Some(d);
                    }
                    Err(e) => {
                        self.handle_bmp_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_bmp() {
                    Ok(()) => {
                        info!("BMP280 device initialized");
                    }
                    Err(e) => {
                        self.handle_bmp_init_error(&e);
                    }
                }
            }

            if let Some(ina) = self.device_manager.ina.as_mut() {
                match ina.get_data() {
                    Ok(d) => {
                        self.device_manager.statuses.ina = Status::Ok;
                        data.ina = Some(d);
                    }
                    Err(e) => {
                        self.handle_ina_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_ina() {
                    Ok(()) => {
                        info!("INA219 device initialized");
                    }
                    Err(e) => {
                        self.handle_ina_init_error(&e);
                    }
                }
            }

            //self.device_manager.statuses.i2s = self.i2s_status.load(Ordering::Relaxed).into();
            //self.device_manager.statuses.umc = self.umc_status.load(Ordering::Relaxed).into();
            self.device_manager.statuses.i2s =
                self.i2s_status.fetch_and(0, Ordering::Relaxed).into();
            self.device_manager.statuses.umc =
                self.umc_status.fetch_and(0, Ordering::Relaxed).into();

            if let Some(mut guard) = self.i2s_max.try_lock_for(Duration::from_millis(50)) {
                self.device_manager.statuses.max_i2s = *guard / 1_000_000;
                *guard = i32::MIN;
            } else {
                self.device_manager.statuses.max_i2s = i32::MIN;
            }

            if let Some(mut guard) = self.umc_max.try_lock_for(Duration::from_millis(50)) {
                self.device_manager.statuses.max_umc = *guard / 1_000_000;
                *guard = i32::MIN;
            } else {
                self.device_manager.statuses.max_umc = i32::MIN;
            }

            if let Some(disk) = disk.as_mut() {
                disk.refresh_specifics(DiskRefreshKind::nothing().with_storage());
                #[allow(clippy::cast_precision_loss)]
                let free = disk.available_space() as f32 / (1024.0 * 1024.0 * 1024.0);
                self.device_manager.statuses.free = free;
            }

            #[allow(clippy::items_after_statements)]
            #[derive(Serialize, Debug)]
            struct JsonData<'a> {
                statuses: Statuses<'a>,
                data: Data,
            }
            let json_data = JsonData {
                statuses: self.device_manager.statuses,
                data,
            };

            #[cfg(feature = "sensors")]
            {
                let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
                let path = self.path.join(format!("{nanos}.json"));
                match File::create(&path) {
                    Ok(file) => {
                        let mut writer = BufWriter::new(file);
                        match serde_json::to_writer(&mut writer, &json_data) {
                            Ok(()) => {
                                match writer.write(b"\n") {
                                    Ok(_) => {}
                                    Err(err) => {
                                        error!("Failed to write new line to data file: {err}");
                                    }
                                }
                                if self.data_link.exists() {
                                    match std::fs::remove_file(&self.data_link) {
                                        Ok(()) => {}
                                        Err(err) => {
                                            error!("Failed to remove previous data symlink: {err}");
                                        }
                                    }
                                }
                                match std::os::unix::fs::symlink(&path, &self.data_link) {
                                    Ok(()) => {}
                                    Err(err) => {
                                        error!("Failed to create data symlink: {err}");
                                    }
                                };
                            }
                            Err(e) => {
                                warn!("Failed to serialize data to json: {e}");
                            }
                        };
                    }
                    Err(e) => {
                        warn!("Failed to create data file: {e}");
                    }
                };
            }

            if let Some(client) = client.as_ref() {
                //info!("POST: {:?}", &json_data);
                match serde_json::to_string(&json_data) {
                    Ok(str) => {
                        let msg = format!("{ip} {mac} {str}");
                        match client
                            .post("http://mlynarczyk.edu.pl:8080/andros/publish")
                            .body(msg)
                            .send()
                        {
                            Ok(_) => {}
                            Err(err) => {
                                warn!("Failed to make POST request: {err}");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to serialize data to json: {e}");
                    }
                }
            }

            thread::sleep(self.read_period.saturating_sub(start.elapsed()));
        }

        //imu_thread.join().unwrap();
        //wind_thread.join().unwrap();
    }
}
