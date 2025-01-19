import sounddevice as sd
import adafruit_ahtx0
import serial
import pynmea2
from datetime import datetime
import time
import numpy as np
import threading
import multiprocessing
from multiprocessing import Queue
import os
import queue
import sys
from mpu9250_jmdev.registers import *
from mpu9250_jmdev.mpu_9250 import MPU9250
import board
import temporenc
import struct
import RPi.GPIO as GPIO

SAMPLERATE = 192000
UMC_SAMPLERATE = 48000
BITS_PER_SAMPLE = 32
BYTES_PER_SAMPLE = BITS_PER_SAMPLE // 8
I2S_CHANNELS = 4
I2S_BUF_SIZE = SAMPLERATE * I2S_CHANNELS * BYTES_PER_SAMPLE
UMC_CHANNELS = 2
UMC_BUF_SIZE = SAMPLERATE * UMC_CHANNELS * BYTES_PER_SAMPLE


def do_every(period, run_event, f, *args):
    def g_tick():
        t = time.time()
        while True:
            t += period
            yield max(t - time.time(), 0)

    g = g_tick()
    while not run_event.is_set():
        time.sleep(next(g))
        f(*args)


class Audio:
    def __init__(self, device, channels, samplerate, bits) -> None:
        self.device = device
        self.channels = channels
        self.samplerate = samplerate
        self.dtype = f"int{bits}"

    def stream(self, q, run_event):
        def callback(indata, frames, _time, status):
            if status:
                print(status, file=sys.stderr)
            q.put(bytes(indata))
            global callback_time
            callback_time = time.time_ns()
            #print(f"{self.device}: {indata.nbytes}")

        s = sd.InputStream(
            blocksize=0,
            device=self.device,
            channels=self.channels,
            samplerate=self.samplerate,
            dtype=self.dtype,
            callback=callback,
        )
        with s:
            callback_time = time.time_ns()
            while True:
                if run_event.is_set():
                    return 0
                if time.time_ns() - callback_time > int(1e9):
                    return 1


    def read(self, q, run_event) -> None:
        while not run_event.is_set():
            try:
                while True:
                    ret = self.stream(q, run_event)
                    if ret == 0:
                        return

            except KeyboardInterrupt:
                pass
            except Exception as e:
                pass
                #print(e)


class IMU:
    def __init__(self) -> None:
        # Initialize MPU9250
        self.mpu = MPU9250(
            address_ak=AK8963_ADDRESS,
            address_mpu_master=MPU9050_ADDRESS_68,
            address_mpu_slave=None,
            bus=1,
            gfs=GFS_1000,
            afs=AFS_8G,
            mfs=AK8963_BIT_16,
            mode=AK8963_MODE_C100HZ,
        )
        self.mpu.configure()

        # Calibration Variables
        self.COEFFS_FILE = "mag_coeffs"
        if os.path.isfile(self.COEFFS_FILE):
            self.mag_coeffs = np.fromfile(self.COEFFS_FILE)
            print(f"IMU: coeffs from file: {self.mag_coeffs}")
            if len(self.mag_coeffs) != 3:
                self.mag_coeffs = np.zeros(3)
        else:
            self.mag_coeffs = np.zeros(3)
        # assuming North is in Y direction initially
        self.north_vector = np.array([0, 1, 0])
        self.gyro_data = []
        self.mag_data = []
        self.last_calibration_time = time.time()

    def detect_rotation(self, threshold=360, time_limit=10):
        total_angle = 0
        start_time = self.gyro_data[0][1]
        for i in range(1, len(self.gyro_data)):
            angle_diff = self.gyro_data[i][0] * (
                self.gyro_data[i][1] - self.gyro_data[i - 1][1]
            )
            total_angle += angle_diff
            if self.gyro_data[i][1] - start_time > time_limit:
                return False, total_angle
            if abs(total_angle) >= threshold:
                return True, total_angle
        return False, total_angle

    def update_calibration(self):
        if len(self.mag_data) == 0:
            return
        # Extract magnetometer readings
        mags = np.array([data[0] for data in self.mag_data])
        x, y, z = mags[:, 0], mags[:, 1], mags[:, 2]
        x_centered = x - np.mean(x)
        y_centered = y - np.mean(y)

        # Use a fit method to calculate calibration coefficients
        coeffs = np.array([np.mean(x_centered), np.mean(y_centered), 0])

        # Normalize coefficients
        mag_max = np.linalg.norm(coeffs[:2])
        if mag_max != 0:
            self.north_vector = np.array(coeffs[:2]) / mag_max
        else:
            self.north_vector = np.array([1, 0])
        # Store mean magnetometer readings as calibration offsets
        self.mag_coeffs = np.mean(mags, axis=0)
        self.mag_coeffs.tofile(self.COEFFS_FILE)

        self.last_calibration_time = time.time()

        # Print calibration message
        # print("---------------------------Calibration conducted------------------------------------------------")

    def get_data(self, current_time):
        accel = np.float64(self.mpu.readAccelerometerMaster())
        magn = np.float64(self.mpu.readMagnetometerMaster())
        gyro = np.float64(self.mpu.readGyroscopeMaster())
        angle_relative_to_north, mag_magnitude = self.calculate_angle_and_magnitude(
            magn
        )

        self.gyro_data.append((gyro[2], current_time))
        self.mag_data.append((np.array(magn), current_time))

        if len(self.gyro_data) > 1:
            rotation_detected, total_angle = self.detect_rotation()
            if rotation_detected:
                self.update_calibration()
                self.gyro_data.clear()
                self.mag_data.clear()
            elif current_time - self.gyro_data[0][1] > 10:
                # Clear data if it took more than 10 seconds
                self.gyro_data.clear()
                self.mag_data.clear()

        return accel, magn, gyro, angle_relative_to_north, mag_magnitude

    def calculate_angle_and_magnitude(self, magn):
        magn = np.array(magn) - self.mag_coeffs  # Apply calibration offsets
        mag_magnitude = np.linalg.norm(magn)
        angle_relative_to_north = (
            np.degrees(np.arctan2(magn[1], magn[2]))
            - np.degrees(np.arctan2(self.north_vector[1], self.north_vector[0]))
        ) % 360
        return angle_relative_to_north, mag_magnitude


class Wind:
    def __init__(self, port="/dev/ttyAMA2"):
        self.port = port
        self.ser = serial.Serial(port, baudrate=9600, timeout=1)
        self.dir = None
        self.speed = None
        self.query = bytes.fromhex("01 03 00 00 00 26 C4 10")

    def update(self):
        try:
            while self.ser.inWaiting() > 0:
                #print(1)
                self.ser.read(5000)
            self.ser.write(self.query)
            start = time.time_ns()
            now = start
            while self.ser.inWaiting() < 81 and now - start < int(1e9):
                now = time.time_ns()
                #print(2)
            if now - start >= int(1e9):
                self.dir = None
                self.speed = None
            else:
                while self.ser.inWaiting() > 0:
                    #print(3)
                    response = self.ser.read(81)
                    wind_str_b = response[8:6:-1]+response[10:8:-1]
                    self.dir = response[5:7]
                    self.speed = wind_str_b
                    #self.dir = int.from_bytes(response[5:7])
                    #self.speed = np.float32(struct.unpack("f", wind_str_b)[0])
        except Exception as e:
            print(f"Error reading wind data: {e}")

    def get_data(self):
        return self.dir, self.speed

    def close(self):
        self.ser.close()

class GPS:
    def __init__(self, port="/dev/ttyAMA0"):
        self.port = port
        self.ser = serial.Serial(port, baudrate=9600, timeout=1)
        self.latitude = None
        self.longitude = None
        self.timestamp = None

    def update(self):
        try:
            data = self.ser.readline().decode("ascii", errors="replace")
            if data.startswith("$GPGGA"):
                try:
                    msg = pynmea2.parse(data)
                    if msg.gps_qual > 0:  # Ensure there's a valid fix
                        self.latitude = np.float64(msg.latitude)
                        self.longitude = np.float64(msg.longitude)
                        self.timestamp = msg.timestamp
                    else:
                        self.latitude = None
                        self.longitude = None
                        self.timestamp = None
                except pynmea2.ParseError as e:
                    print(f"Parse error: {e}")
        except Exception as e:
            print(f"Error reading GPS data: {e}")

    def get_data(self):
        return self.latitude, self.longitude, self.timestamp

    def close(self):
        self.ser.close()


class AHT:
    def __init__(self) -> None:
        self.sensor = adafruit_ahtx0.AHTx0(board.I2C())

    def get_data(self):
        return np.float64(self.sensor.temperature), np.float64(
            self.sensor.relative_humidity
        )


class IDGen:
    def __init__(self) -> None:
        self.ids = 0

    def gen_next(self) -> bytes:
        self.ids += 1
        return self.ids.to_bytes().rjust(4, b"\xff")


class DataReader:
    def __init__(self) -> None:
        self.imu = IMU()
        self.aht = AHT()
        self.gps = GPS()
        self.wind = Wind()

        self.id_gen = IDGen()

        self.DATETIME_ID = self.id_gen.gen_next()
        self.CPU_TEMP_ID = self.id_gen.gen_next()
        self.ACCEL_ID = self.id_gen.gen_next()
        self.MAGN_ID = self.id_gen.gen_next()
        self.GYRO_ID = self.id_gen.gen_next()
        self.ANGLE_ID = self.id_gen.gen_next()
        self.MAG_MAGNITUDE_ID = self.id_gen.gen_next()
        self.TEMP_ID = self.id_gen.gen_next()
        self.HUMIDITY_ID = self.id_gen.gen_next()
        self.LATITUDE_ID = self.id_gen.gen_next()
        self.LONGITUDE_ID = self.id_gen.gen_next()
        self.GPS_TIMESTAMP_ID = self.id_gen.gen_next()
        self.WIND_DIR_ID = self.id_gen.gen_next()
        self.WIND_SPEED_ID = self.id_gen.gen_next()


    def _inner_read(self, q):
        now = datetime.now()
        current_time = time.time()
        data = bytearray()

        # read temp
        with open("/sys/class/thermal/thermal_zone0/temp", 'r') as f:
            cpu_temp = np.float32(f.read()) / np.float32(1000.0)

        # Get data from MPU9250
        imu_reset = False
        try:
            accel, magn, gyro, angle_relative_to_north, mag_magnitude = self.imu.get_data(
                current_time
            )
        except Exception as e:
            print(f"IMU: {e}")
            imu_reset = True

        # Get data from AHT
        aht_reset = False
        try:
            temperature, humidity = self.aht.get_data()
        except Exception as e:
            #print(f"AHT: {e}")
            aht_reset = True
            #self.aht.reset()
            pass

        # Get data from GPS
        self.gps.update()
        latitude, longitude, timestamp = self.gps.get_data()

        # Get wind data
        self.wind.update()
        wind_dir, wind_speed = self.wind.get_data()

        # Write data to bytearray
        data.extend(self.DATETIME_ID)
        data.extend(temporenc.packb(now))

        data.extend(self.CPU_TEMP_ID)
        data.extend(bytes(cpu_temp))

        if not imu_reset:
            data.extend(self.ACCEL_ID)
            data.extend(bytes(accel))
            data.extend(self.MAGN_ID)
            data.extend(bytes(magn))
            data.extend(self.GYRO_ID)
            data.extend(bytes(gyro))
            data.extend(self.ANGLE_ID)
            data.extend(bytes(angle_relative_to_north))
            data.extend(self.MAG_MAGNITUDE_ID)
            data.extend(bytes(mag_magnitude))
        else:
            try:
                self.imu = IMU()
            except Exception as e:
                print(f"NEW IMU: {e}")
                pass

        if not aht_reset:
            data.extend(self.TEMP_ID)
            data.extend(bytes(temperature))
            data.extend(self.HUMIDITY_ID)
            data.extend(bytes(humidity))
        else:
            try:
                self.aht = AHT()
            except Exception as e:
                #print(f"NEW AHT: {e}")
                pass

        if latitude is not None and longitude is not None and timestamp is not None:
            data.extend(self.LATITUDE_ID)
            data.extend(bytes(latitude))
            data.extend(self.LONGITUDE_ID)
            data.extend(bytes(longitude))
            data.extend(self.GPS_TIMESTAMP_ID)
            data.extend(temporenc.packb(timestamp))

        if wind_dir is not None and wind_speed is not None:
            data.extend(self.WIND_DIR_ID)
            #data.extend(bytes(wind_dir))
            data.extend(wind_dir)
            data.extend(self.WIND_SPEED_ID)
            #data.extend(bytes(wind_speed))
            data.extend(wind_speed)

        # Write data to queue
        q.put(data)

    def read(self, q, run_event) -> None:
        try:
            do_every(0.2, run_event, self._inner_read, q)
        except KeyboardInterrupt:
            pass
        except Exception as e:
            print(f"DataReader.read: {e}")
            pass

class pga:
    def __init__(self, data, clk, fsync):
        self.dataPin = gpiozero.OutputDevice(pin = data)
        self.clkPin = gpiozero.OutputDevice(pin = clk)
        self.fsyncPin = gpiozero.OutputDevice(pin = fsync)
        self.fsyncPin.on()
        self.clkPin.off()

    def send16(self, n):
        self.clkPin.off()
        self.dataPin.value=0
        time.sleep(0.05)
        self.fsyncPin.off()
        #time.sleep(0.005)
        mask = 1 << 15
        for i in range(0, 16):
            self.dataPin.value = bool(n & mask)
            #time.sleep(0.000050)
            self.clkPin.on()
            #time.sleep(0.0001)
            self.clkPin.off()
            #time.sleep(0.00005)
            #self.dataPin.value=0
            #time.sleep(0.00005)
            mask = mask >> 1
        self.dataPin.off()
        #time.sleep(0.05)
        self.fsyncPin.on()

def buffer_thread(
    i2s_q: Queue,
    # i2s_file: TextIOWrapper,
    umc_q: Queue,
    # umc_file: TextIOWrapper,
    data_q: Queue,
    # data_file: TextIOWrapper,
    run_event,
    pps: Queue,
):
    try:
        i2s = bytearray()
        umc = bytearray()
        data = bytearray()
        # last = time.time_ns()
        last_file = time.time_ns()
        path = "./data"
        # i2s_file = open(f"data/i2s_{last_file}", "wb")
        # umc_file = open(f"data/umc_{last_file}", "wb")
        # data_file = open(f"data/data_{last_file}", "wb")
        while not run_event.is_set():
            try:
                b = pps.get_nowait()
                # pps.task_done()
                print(f"pps i2s buffer size: {len(i2s)}")
                i2s.extend(b)
                umc.extend(b)
                data.extend(b)
            except queue.Empty:
                pass

            try:
                i2s.extend(i2s_q.get_nowait())
                # i2s_q.task_done()
            except queue.Empty:
                pass

            try:
                umc.extend(umc_q.get_nowait())
                # umc_q.task_done()
            except queue.Empty:
                pass

            try:
                data.extend(data_q.get_nowait())
                # data_q.task_done()
            except queue.Empty:
                pass

            now = time.time_ns()
            diff = now - last_file
            if diff >= int(1e10):
                last_file += int(1e10)

                with open(f"{path}/i2s_{now}", "wb") as f:
                    #print(f"writing {len(i2s)} bytes to i2s")
                    f.write(i2s)
                with open(f"{path}/umc_{now}", "wb") as f:
                    #print(f"writing {len(umc)} bytes to umc")
                    f.write(umc)
                with open(f"{path}/data_{now}", "wb") as f:
                    #print(f"writing {len(data)} bytes to data")
                    f.write(data)
                #
                # i2s_file.close()
                # umc_file.close()
                # data_file.close()

                i2s = bytearray()
                umc = bytearray()
                data = bytearray()

                # i2s_file = open(f"data/i2s_{last_file}", "wb")
                # umc_file = open(f"data/umc_{last_file}", "wb")
                # data_file = open(f"data/data_{last_file}", "wb")

        while not pps.empty():
            b = pps.get()
            # pps.task_done()
            i2s.extend(b)
            umc.extend(b)
            data.extend(b)

        while not i2s_q.empty():
            i2s.extend(i2s_q.get())
            # i2s_q.task_done()
        # i2s_file.write(i2s)
        print(f"leftover I2S bytes: {len(i2s)}")

        while not umc_q.empty():
            umc.extend(umc_q.get())
            # umc_q.task_done()
        # umc_file.write(umc)
        print(f"leftover UMC bytes: {len(umc)}")

        while not data_q.empty():
            data.extend(data_q.get())
            # data_q.task_done()
        # data_file.write(data)
        print(f"leftover data bytes: {len(data)}")

        pps.close()
        i2s_q.close()
        umc_q.close()
        data_q.close()

        with open(f"{path}/i2s_{now}", "wb") as f:
            f.write(i2s)
        with open(f"{path}/umc_{now}", "wb") as f:
            f.write(umc)
        with open(f"{path}/data_{now}", "wb") as f:
            f.write(data)
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(e)


if __name__ == "__main__":
    GPIO.setmode(GPIO.BCM)
    GPIO.setup(13, GPIO.IN, pull_up_down=GPIO.PUD_DOWN)

    pps = Queue()

    def interrupt(channel):
        if channel == 13:
            now = time.time_ns()
            b = now.to_bytes(8)
            pps.put(b.rjust(16, b'\xee'))

    GPIO.add_event_detect(13, GPIO.RISING, callback=interrupt, bouncetime=1)

    run_event = multiprocessing.Event()
    # run_event.set()

    i2s_q = Queue()
    i2s_reader = Audio("ANDROSi2s", I2S_CHANNELS, SAMPLERATE, BITS_PER_SAMPLE)
    i2s_thread = multiprocessing.Process(
        target=i2s_reader.read,
        args=(
            i2s_q,
            run_event,
        ),
    )

    umc_q = Queue()
    umc_reader = Audio("UMC202HD 192k", UMC_CHANNELS, UMC_SAMPLERATE, BITS_PER_SAMPLE)
    umc_thread = multiprocessing.Process(
        target=umc_reader.read,
        args=(
            umc_q,
            run_event,
        ),
    )

    data_q = Queue()
    data_reader = DataReader()
    data_thread = multiprocessing.Process(
        target=data_reader.read,
        args=(
            data_q,
            run_event,
        ),
    )

    # i2s_file = open("data/i2s", "wb")
    # umc_file = open("data/umc", "wb")
    # data_file = open("data/data", "wb")

    buffer = multiprocessing.Process(
        target=buffer_thread,
        args=(
            i2s_q,
            # i2s_file,
            umc_q,
            # umc_file,
            data_q,
            # data_file,
            run_event,
            pps,
        ),
    )

    buffer.start()
    i2s_thread.start()
    umc_thread.start()
    data_thread.start()

    try:
        while True:
            time.sleep(0.1)
    except KeyboardInterrupt:
        print("Stopping")
        threading.Thread(target=run_event.set).start()
        # run_event.set()
        print("Joining queues")
        pps.join_thread()
        i2s_q.join_thread()
        umc_q.join_thread()
        data_q.join_thread()
        # joins = [
        #     threading.Thread(target=pps.join),
        #     threading.Thread(target=i2s_q.join),
        #     threading.Thread(target=umc_q.join),
        #     threading.Thread(target=data_q.join),
        # ]
        # for j in joins:
        #     j.start()
        print("Queues joined")
        print("Joining threads")
        data_thread.join()
        i2s_thread.join()
        umc_thread.join()
        buffer.join()
        # for j in joins:
        #     j.join()
        print("Threads joined")
        exit(0)

    # print("Closing files")
    # i2s_file.close()
    # umc_file.close()
    # data_file.close()
    # print("Files closed")
