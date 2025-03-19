use rand::random_range;
use std::f64::consts::PI;
#[cfg(feature = "sensors")]
use std::fs::File;
#[cfg(feature = "sensors")]
use std::io::BufWriter;
#[cfg(feature = "sensors")]
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
use sysinfo::CpuRefreshKind;
use sysinfo::DiskRefreshKind;
use sysinfo::Disks;
use sysinfo::RefreshKind;

use log::{error, info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use aht::Aht;
use bmp::Bmp;
use device_manager::{DeviceManager, Status, Statuses};
use gps::Gps;
use imu::Imu;
use ina::Ina;
use wind::Wind;

use self::device_manager::Coords;

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

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
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
    #[cfg(feature = "sensors")]
    pub path: PathBuf,
    pub calib_path: &'a PathBuf,
    #[cfg(feature = "sensors")]
    pub data_link: PathBuf,
    pub read_period: Duration,
    i2s_status: &'a AtomicU8,
    umc_status: &'a AtomicU8,
    i2s_max: Arc<Mutex<i32>>,
    umc_max: Arc<Mutex<i32>>,
}

impl<'a> Reader<'a> {
    const PERIOD_MILLIS: u64 = 5000;

    pub fn new(
        #[cfg(feature = "sensors")] path: PathBuf,
        calib_path: &'a PathBuf,
        i2s_status: &'a AtomicU8,
        umc_status: &'a AtomicU8,
        i2s_max: Arc<Mutex<i32>>,
        umc_max: Arc<Mutex<i32>>,
    ) -> Self {
        #[cfg(feature = "sensors")]
        let data_link = path.join("current");
        Self {
            device_manager: DeviceManager::new(),
            #[cfg(feature = "sensors")]
            path,
            calib_path,
            #[cfg(feature = "sensors")]
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
        match err {
            gps::Error::NoData => {}
            _ => warn!("GPS data error: {err}"),
        }
    }

    fn handle_gps_init_error(&mut self, err: &gps::Error) {
        match err {
            gps::Error::Uart(uart_err) => {
                warn!("GPS init failed: {uart_err}");
                self.device_manager.statuses.gps = Status::Dc;
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
                self.device_manager.statuses.aht = Status::Dc;
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
                self.device_manager.statuses.bmp = Status::Dc;
            }
        }
    }

    pub fn read<'b>(
        &mut self,
        running: &'a AtomicBool,
        s: &'a Scope<'a, 'b>,
        ip: Option<(String, String, String)>,
    ) {
        let imu_data = Arc::new(Mutex::new((imu::Data::default(), Status::default())));
        thread::Builder::new()
            .name("imu".to_owned())
            .spawn_scoped(s, {
                let data = imu_data.clone();
                let bus = self.device_manager.settings.imu_bus;
                const PERIOD: Duration = Duration::from_millis(100);
                let path = self.calib_path.clone();
                move || {
                    const SAMPLES: usize = 10000 / PERIOD.as_millis() as usize;
                    let mut imu: Option<Imu<SAMPLES>> = None;
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
                            match Imu::new(bus, &path) {
                                Ok(mut device) => match device.calibrate(true) {
                                    Ok(()) => {
                                        info!("IMU device initialized");
                                        imu = Some(device);
                                        data.lock().1 = Status::NoData;
                                    }
                                    Err(err) => {
                                        warn!("{err}");
                                        data.lock().1 = Status::Dc;
                                    }
                                },
                                Err(err) => {
                                    warn!("IMU init: {err}");
                                    data.lock().1 = Status::Dc;
                                }
                            };
                        }

                        thread::sleep(PERIOD.saturating_sub(start.elapsed()));
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
                                    match err {
                                        wind::Error::NoData => {}
                                        _ => warn!("{err}"),
                                    }
                                    data.lock().1 = Status::NoData;
                                }
                            }
                        } else {
                            match Wind::new(settings.port, settings.baud_rate, settings.timeout) {
                                Ok(device) => {
                                    info!("Wind device initialized");
                                    wind = Some(device);
                                    data.lock().1 = Status::NoData;
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::Dc;
                                }
                            };
                        }

                        thread::sleep(period.saturating_sub(start.elapsed()));
                    }
                }
            })
            .unwrap();

        let ina_data = Arc::new(Mutex::new((ina::Data::default(), Status::default())));
        thread::Builder::new()
            .name("ina".to_owned())
            .spawn_scoped(s, {
                let data = ina_data.clone();
                let period = Duration::from_millis(100);
                move || {
                    let mut ina: Option<Ina> = None;
                    while running.load(Ordering::Relaxed) {
                        let start = Instant::now();

                        if let Some(ina) = ina.as_mut() {
                            match ina.get_data() {
                                Ok(d) => {
                                    *data.lock() = (d, Status::Ok);
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::NoData;
                                }
                            }
                        } else {
                            match Ina::new() {
                                Ok(device) => {
                                    info!("INA device initialized");
                                    ina = Some(device);
                                    data.lock().1 = Status::NoData;
                                }
                                Err(err) => {
                                    warn!("{err}");
                                    data.lock().1 = Status::Dc;
                                }
                            };
                        }

                        thread::sleep(period.saturating_sub(start.elapsed()));
                    }
                }
            })
            .unwrap();

        let mut disks = Disks::new_with_refreshed_list();
        let mut disk = disks
            .list_mut()
            .iter_mut()
            .find(|d| d.mount_point() == Path::new("/"));

        let mut system = sysinfo::System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
        );

        let mut components = sysinfo::Components::new_with_refreshed_list();

        let (client, (ip, mac, post)) = if let Some((ip, mac, post)) = ip {
            (Some(reqwest::blocking::Client::new()), (ip, mac, post))
        } else {
            (None, (String::new(), String::new(), String::new()))
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

        let mut sum_lon = 0f64;
        let mut sum_lat = 0f64;
        let mut coord_count = 0;

        //let mut rng = rng();

        self.device_manager.statuses.mac = mac.clone();

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

            if let Some(guard) = ina_data.try_lock_for(Duration::from_millis(50)) {
                let (ina_data, ina_status) = *guard;
                drop(guard);
                if ina_status == Status::Ok {
                    data.ina = Some(ina_data);
                }
                self.device_manager.statuses.ina = ina_status;
            } else {
                self.device_manager.statuses.ina = Status::NoData;
            }

            if let Some(gps) = self.device_manager.gps.as_mut() {
                match gps.get_data() {
                    Ok(d) => {
                        self.device_manager.statuses.gps = Status::Ok;
                        sum_lon += d.longitude;
                        sum_lat += d.latitude;
                        coord_count += 1;
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

            if rand::random_range(0u32..10) != 0 && data.gps.is_some() {
                self.device_manager.statuses.drone_detected = true;
                const R_EARTH: f64 = 6365.0;
                let avg_lon = if coord_count > 0 {
                    sum_lon / coord_count as f64
                } else {
                    0.0
                };
                let avg_lat = if coord_count > 0 {
                    sum_lat / coord_count as f64
                } else {
                    0.0
                };
                let dx = random_range(-0.3f64..0.3f64);
                let dy = random_range(-0.3f64..0.3f64);
                let new_lat = avg_lat + (dy / R_EARTH) * (180.0 / PI);
                let new_lon =
                    avg_lon + (dx / R_EARTH) * (180.0 / PI) / (new_lat * PI / 180.0).cos();
                self.device_manager.statuses.drone_coords = Some(Coords {
                    lon: new_lon,
                    lat: new_lat,
                    target_id: 1,
                });
            } else {
                self.device_manager.statuses.drone_detected = false;
                self.device_manager.statuses.drone_coords = None;
            }

            self.device_manager.statuses.i2s =
                self.i2s_status.fetch_and(0, Ordering::Relaxed).into();
            self.device_manager.statuses.umc =
                self.umc_status.fetch_and(0, Ordering::Relaxed).into();

            if let Some(mut guard) = self.i2s_max.try_lock_for(Duration::from_millis(50)) {
                self.device_manager.statuses.max_i2s = (*guard as f32 / 1e6) as i32;
                *guard = i32::MIN;
            } else {
                self.device_manager.statuses.max_i2s = i32::MIN;
            }

            if let Some(mut guard) = self.umc_max.try_lock_for(Duration::from_millis(50)) {
                self.device_manager.statuses.max_umc = (*guard as f32 / 1e6) as i32;
                *guard = i32::MIN;
            } else {
                self.device_manager.statuses.max_umc = i32::MIN;
            }

            if let Some(disk) = disk.as_mut() {
                disk.refresh_specifics(DiskRefreshKind::nothing().with_storage());
                let free = disk.available_space() as f32 / (1024.0 * 1024.0 * 1024.0);
                self.device_manager.statuses.free = free;
            }

            system.refresh_cpu_usage();
            components.refresh(false);

            self.device_manager.statuses.cpu_usage = system.global_cpu_usage();
            self.device_manager.statuses.temp = components
                .iter()
                .filter_map(|c| c.temperature())
                .max_by(|a, b| a.total_cmp(b));

            #[derive(Serialize, Debug)]
            struct FakeData {
                mac: String,
                datetime: i64,
                cpu_temp: f32,
                accel: [f32; 3],
                magn: [f32; 3],
                gyro: [f32; 3],
                angle: f32,
                mag_magnitude: f32,
                temp: f32,
                humidity: f32,
                latitude: f64,
                longitude: f64,
                gps_timestamp: Option<String>,
                wind_dir: u16,
                wind_speed: f32,
                radius: u32,
            }

            //let fake_lat_lon = [
            //    (52.47834, 16.93098),
            //    (52.47751, 16.92642),
            //    (52.47671, 16.92221),
            //];

            //let fake_lat = [52.4775535, 52.4786645, 52.4797755];
            //let fake_lon = [16.9273625, 16.9284735, 16.9295845];

            //let rand_lat_lon = fake_lat_lon
            //    .choose(&mut rng)
            //    .map(|(lat, lon)| {
            //        (lat + random_range(0.0..0.0000099), lon + random_range(0.0..0.0000099))
            //    })
            //    .unwrap();

            let rand_lat_lon = match ip.chars().rev().nth(1).unwrap() {
                '4' => (
                    52.47834 + random_range(0.0..0.0000099),
                    16.93098 + random_range(0.0..0.0000099),
                ),
                '5' => {
                    self.device_manager.statuses.drone_detected = true;
                    self.device_manager.statuses.drone_coords = Some(Coords {
                        lat: 52.478982 + random_range(0.0..0.0000099),
                        lon: 16.919339 + random_range(0.0..0.0000099),
                        target_id: 0,
                    });
                    (
                        52.47751 + random_range(0.0..0.0000099),
                        16.92642 + random_range(0.0..0.0000099),
                    )
                }
                '6' => {
                    self.device_manager.statuses.drone_detected = true;
                    self.device_manager.statuses.drone_coords = Some(Coords {
                        lat: 52.478982 + random_range(0.0..0.0000099),
                        lon: 16.919339 + random_range(0.0..0.0000099),
                        target_id: 0,
                    });
                    (
                        52.47671 + random_range(0.0..0.0000099),
                        16.92221 + random_range(0.0..0.0000099),
                    )
                }
                _ => (0.0, 0.0),
            };

            let fake_data = FakeData {
                mac: mac.clone(),
                datetime: chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                cpu_temp: self.device_manager.statuses.temp.unwrap_or(0.0),
                accel: data.imu.map_or([0.0, 0.0, 0.0], |d| d.acc),
                magn: data.imu.map_or([0.0, 0.0, 0.0], |d| d.mag),
                gyro: data.imu.map_or([0.0, 0.0, 0.0], |d| d.gyro),
                angle: data.imu.map_or(0.0, |d| d.angle),
                mag_magnitude: 0.0,
                temp: data.aht.map_or(0.0, |d| d.temperature),
                humidity: data.aht.map_or(0.0, |d| d.humidity),
                latitude: rand_lat_lon.0,
                longitude: rand_lat_lon.1,
                //latitude: *fake_lat.choose(&mut rng).unwrap(),
                //longitude: *fake_lon.choose(&mut rng).unwrap(),
                //latitude: data.gps.as_ref().map_or(0.0, |d| d.latitude),
                //longitude: data.gps.as_ref().map_or(0.0, |d| d.longitude),
                gps_timestamp: data.gps.clone().map(|d| d.timestamp.to_rfc3339()),
                wind_dir: data.wind.map_or(0, |d| d.dir),
                wind_speed: data.wind.map_or(0.0, |d| d.speed),
                radius: 9000,
            };

            match ip.chars().rev().nth(1).unwrap() {
                '4' | '5' | '6' => {
                    if let Some(gps) = data.gps.as_mut() {
                        gps.latitude = rand_lat_lon.0;
                        gps.longitude = rand_lat_lon.1;
                    } else {
                        data.gps = Some(gps::Data {
                            latitude: rand_lat_lon.0,
                            longitude: rand_lat_lon.1,
                            timestamp: chrono::Utc::now(),
                            altitude: 0.0,
                        })
                    }
                }
                _ => {}
            }

            #[derive(Serialize, Debug)]
            struct FakeDetection {
                tid: String,
                target: String,
                latitude: f64,
                longitude: f64,
                showtime: String,
                mast: String,
                status: String,
            }

            //let rand_lat_lon = fake_lat_lon
            //    .choose(&mut rng)
            //    .map(|(lat, lon)| {
            //        (lat + random_range(0.0..0.0000099), lon + random_range(0.0..0.0000099))
            //    })
            //    .unwrap();
            let target = format!("target{}", random_range(0u8..10));

            let fake_detection =
                self.device_manager
                    .statuses
                    .drone_coords
                    .map(|coords| FakeDetection {
                        tid: target.clone(),
                        target,
                        //latitude: 52.478982 + random_range(0.0..0.0000099),
                        //longitude: 16.919339 + random_range(0.0..0.0000099),
                        latitude: coords.lat,
                        longitude: coords.lon,
                        //latitude: *fake_lat.choose(&mut rng).unwrap(),
                        //longitude: *fake_lon.choose(&mut rng).unwrap(),
                        showtime: chrono::Utc::now().to_rfc3339(),
                        mast: mac.clone(),
                        status: "hostile".to_string(),
                    });

            #[derive(Serialize, Debug)]
            struct JsonData<'a> {
                statuses: Statuses<'a>,
                data: Data,
            }
            let json_data = JsonData {
                statuses: self.device_manager.statuses.clone(),
                data: data.clone(),
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
                match serde_json::to_string(&json_data) {
                    Ok(str) => {
                        let msg = format!("{ip} {mac} {str}");
                        match client.post(&post).body(msg).send() {
                            Ok(_) => {}
                            Err(err) => {
                                warn!("Failed to make POST request: {err}");
                            }
                        }
                        match client
                            .post("http://192.168.71.12:8095/andros/api/kafka/json/publish/node")
                            .body(str)
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
                match serde_json::to_string(&fake_data) {
                    Ok(str) => {
                        match client
                            .post("http://192.168.71.12:8095/andros/api/kafka/json/publish/node")
                            .body(str)
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
                if let Some(fake_detection) = fake_detection {
                    match serde_json::to_string(&fake_detection) {
                        Ok(str) => {
                            match client
                            .post("http://192.168.71.12:8095/andros/api/kafka/json/publish/target")
                            .body(str)
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
            }

            thread::sleep(self.read_period.saturating_sub(start.elapsed()));
        }
    }
}
