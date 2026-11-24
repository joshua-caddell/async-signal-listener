use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Duration;
use signal_sample::{Samples, WaveType};
use tokio_signal::{SignalListener, SpiModeFlags};

const SPI_SPEED_HZ: u32 = 9_000_000;
const BAUD_RATE: u32 = 115_200;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let devices = vec![
        ("/dev/ttyAMA0".to_string(), BAUD_RATE),
        ("/dev/ttyAMA1".to_string(), BAUD_RATE),
        ("/dev/ttyAMA2".to_string(), BAUD_RATE),
        ("/dev/ttyAMA3".to_string(), BAUD_RATE),
        ("/dev/ttyAMA4".to_string(), BAUD_RATE),
    ];

    let mut signal_listener = SignalListener::new();

    for d in devices {
        let samples = Samples::new(d.0.clone(), WaveType::SINE);
        if d.0.clone().contains("spi") {
            if let Err(e) = signal_listener.add_spi(d.0.clone(), SPI_SPEED_HZ,
                                                    SpiModeFlags::SPI_MODE_0, d.1, samples).await {
                eprintln!("{}", e);
                return Err(e);
            }
        } else {
            if let Err(e) = signal_listener.add_serial(d.0.clone(), d.1, samples) {
                eprintln!("{}", e);
                return Err(e);
            }
        }

    }

    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        println!("DEVICE\t\tVALUE\t\tRX INTERVAL\tTX INTERVAL\tLATENCY");

        let mut complete: bool  = true;

        for samples in &signal_listener.handlers {
            let guard = samples.lock().await;

            complete = complete && guard.samples.len() == Samples::MAX_SAMPLES;

            let mut latency = 0.0;
            let mut rx_interval = 0.0;
            let mut tx_interval = 0.0;

            let mut value: f64 = 0.0;
            if let Some(sample) = guard.samples.back() {
                value = sample.value;
                latency = sample.elapsed.as_secs_f64() * 1000.0;

                if guard.samples.len() > 2 {
                    let prev_sample = guard.samples[guard.samples.len() - 2];
                    rx_interval = (sample.rx_time.as_secs_f64() - prev_sample.rx_time.as_secs_f64()) * 1000.0;
                    tx_interval = (sample.tx_time.as_secs_f64() - prev_sample.tx_time.as_secs_f64()) * 1000.0;
                }
            }


            println!("{}\t{}\t{}\t{}\t{}", guard.device, format!("{:.9}", value),
                     format!("{:.9}", rx_interval),
                     format!("{:.9}", tx_interval),
                     format!("{:.9}", latency));
        }

        if complete {
            break;
        }

        print!("\x1B[{}A", signal_listener.handlers.len() + 1);
    }

    for samples in &signal_listener.handlers {
        let guard = samples.lock().await;
        let path = format!("samples_{}.csv",
            guard.device.trim_start_matches('/').replace('/', "_")
        );

        if let Err(e) = write_csv(&path, &guard) {
            eprintln!("failed to write {}: {}", path, e);
        }
    }

    Ok(())
    //Ok(signal_listener.wait().await)
}

fn write_csv(path: &str, s: &Samples) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "VALUE,RX TIME,TX TIME")?;
    for s in s.samples.iter() {
        writeln!(w, "{},{},{}", s.value, s.rx_time.as_nanos(), s.tx_time.as_nanos())?;
    }
    w.flush()
}
