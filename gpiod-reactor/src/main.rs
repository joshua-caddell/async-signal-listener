use gpiod::{Chip, EdgeDetect, Options};
use signal_sample::{Sample, Samples, WaveType};
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Read, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SPI_SPEED_HZ: u32 = 9_000_000;
const FRAME_LEN: usize = 24;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let devices: HashMap<u32, String> = HashMap::from([
        (27, "/dev/spidev0.0".to_string()),
        (22, "/dev/spidev1.0".to_string()),
        (23, "/dev/spidev2.0".to_string()),
        (24, "/dev/spidev3.0".to_string()),
        (25, "/dev/spidev5.0".to_string()),
    ]);

    let mut sample_map: HashMap<u32, (Spidev, Samples)> = HashMap::new();
    for (&pin, dev) in &devices {
        sample_map.insert(
            pin,
            (create_spi(dev.clone())?, Samples::new(dev.clone(), WaveType::SINE)),
        );
    }

    let chip = Chip::new("/dev/gpiochip0")?;
    let pins: Vec<u32> = devices.keys().copied().collect();
    let mut lines = chip.request_lines(
        Options::input(pins.as_slice())
            .edge(EdgeDetect::Rising)
            .consumer("gpiod-reactor"),
    )?;

    // ESP32 holds READY high until the frame is clocked out, so any line that is
    // already high was raised before we requested it and will never produce a
    // rising edge. Clock out and discard that stale frame so READY drops and
    // future edges arrive.
    let initial = lines.get_values(vec![false; pins.len()])?;
    for (idx, high) in initial.iter().enumerate() {
        if *high {
            println!("Pin {} already high at startup, dropping stale frame", pins[idx]);
            if let Some((spidev, _)) = sample_map.get_mut(&pins[idx]) {
                let mut discard = [0_u8; FRAME_LEN];
                if let Err(e) = spidev.read_exact(&mut discard) {
                    eprintln!("Error draining SPI on pin {}: {}", pins[idx], e);
                }
            }
        }
    }

    loop {
        let ev = match lines.read_event() {
            Ok(ev) => {
                //println!("Event on pin {}", ev.line);
                ev
            },
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                println!("Interrupted");
                continue
            },
            Err(e) => {
                eprintln!("Error reading event: {}", e);
                return Err(e.into())
            },
        };

        if let Some((spidev, samples)) = sample_map.get_mut(&pins[ev.line as usize]) {
            handle_pin_event(spidev, samples);
        }

        if sample_map.values().all(|(_, s)| s.samples.len() == Samples::MAX_SAMPLES) {
            break;
        }
    }

    for (_, samples) in sample_map.values() {
        let path = format!(
            "samples_{}.csv",
            samples.device.trim_start_matches('/').replace('/', "_")
        );
        if let Err(e) = write_csv(&path, samples) {
            eprintln!("failed to write {}: {}", path, e);
        }
    }

    Ok(())
}


// Synchronous application business logic routing
fn handle_pin_event(spidev: &mut Spidev, samples: &mut Samples) {
    let mut buf = [0_u8; FRAME_LEN];
    if let Err(e) = spidev.read_exact(&mut buf) {
        eprintln!("Error reading SPI data: {}", e);
        return;
    }

    let rx_time: Duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let value = f64::from_le_bytes(buf[..8].try_into().unwrap());
    let tx_time = Duration::from_nanos_u128(u128::from_le_bytes(buf[8..].try_into().unwrap()));

    let sample= Sample::new(value, tx_time, rx_time);
    Samples::add_sample_inner(&mut samples.samples, sample, Samples::MAX_SAMPLES);
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

fn write_csv(path: &str, s: &Samples) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "VALUE,RX TIME,TX TIME")?;
    for s in s.samples.iter() {
        writeln!(w, "{},{},{}", s.value, s.rx_time.as_nanos(), s.tx_time.as_nanos())?;
    }
    w.flush()
}
