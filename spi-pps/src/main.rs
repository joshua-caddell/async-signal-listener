use gpiod::{Chip, Options};
use spidev::{SpiModeFlags, Spidev, SpidevOptions, SpidevTransfer};
use std::fs::File;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SPI_SPEED_HZ: u32 = 9_000_000;
const PPS_PIN: u32 = 26;

struct TimeStamp {
    time: u128,
    correction: i64,
}

impl TimeStamp {
    fn new(time: u128) -> Self {
        Self { time, correction: 0 }
    }
}
struct DeviceTimes {
    timestamps: Vec<TimeStamp>,
    device: String,
    spi: Spidev,
}

impl DeviceTimes {
    pub fn new(device: String, spi: Spidev) -> Self {
        DeviceTimes {
            timestamps: vec![],
            device,
            spi,
        }
    }

    pub fn add_timestamp(&mut self, timestamp: TimeStamp) {
        self.timestamps.push(timestamp);

        if self.timestamps.len() > 100_000 {
            self.timestamps.remove(0);
        }
    }
}
fn create_spi(device: String) -> io::Result<Spidev> {
    let mut spi = Spidev::open(device)?;
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_SPEED_HZ)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)?;
    Ok(spi)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let devices = vec![
        "/dev/spidev0.0".to_string(),
        "/dev/spidev1.0".to_string(),
        "/dev/spidev2.0".to_string(),
        "/dev/spidev3.0".to_string(),
        "/dev/spidev5.0".to_string(),
    ];

    let mut device_times = vec![];

    for d in &devices {
        device_times.push(DeviceTimes::new(d.clone(), create_spi(d.clone())?));
    }

    let mut tx_buf = [0_u8; 16];
    let mut rx_buf = [0_u8; 16];

    let chip = Chip::new("gpiochip0")?;
    let opts = Options::output([PPS_PIN]).values([false]);

    let outputs = chip.request_lines(opts)?;

    let csv_path = std::env::current_dir()?.join("corrections.csv");
    let mut csv = File::create(&csv_path)?;
    println!("writing {}", csv_path.display());

    println!("DEVICE\t\t\tOFFSET US");

    loop {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let nanos_into_current_second = now.subsec_nanos();
        let time_to_next_second = Duration::from_nanos(1_000_000_000 - nanos_into_current_second as u64);

        thread::sleep(time_to_next_second);

        outputs.set_values([true])?;

        let exact_unix_nano = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        thread::sleep(Duration::from_millis(100));
        outputs.set_values([false])?;

        tx_buf.copy_from_slice(&exact_unix_nano.to_le_bytes());

        for i in 0..device_times.len() {
            let dt = &mut device_times[i];
            let mut transfer = SpidevTransfer::read_write(&tx_buf, &mut rx_buf);
            dt.spi.transfer(&mut transfer)?;
            let correction = i64::from_le_bytes(rx_buf[0..8].try_into()?);

            if let Some(t) = dt.timestamps.last_mut() {
                if correction != i64::MIN && rx_buf[0..8] != [0xFF; 8] {
                    t.correction = correction;
                    writeln!(csv, "{},{},{}", dt.device, t.time, correction)?;
                }
            }

            dt.add_timestamp(TimeStamp::new(exact_unix_nano));
            println!("{:<15}\t{:>12.6}\x1B[K", devices[i], correction);
        }

        print!("\x1B[{}A", devices.len());
    }
}
