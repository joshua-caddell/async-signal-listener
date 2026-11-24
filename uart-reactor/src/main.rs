use mio::{Events, Interest, Poll, Token};
use mio_serial::{SerialPortBuilderExt, SerialStream};
use signal_sample::{Sample, Samples, WaveType};
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BAUD_RATE: u32 = 115_200;
const FRAME_LEN: usize = 24;
const NANOS_PER_SEC: u128 = 1_000_000_000;

struct Port {
    stream: SerialStream,
    samples: Samples,
    buf: [u8; FRAME_LEN],
    filled: usize,
}

fn main() {
    let devices: Vec<String> = (0..5).map(|i| format!("/dev/ttyAMA{i}")).collect();

    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(devices.len());
    let mut ports: Vec<Port> = Vec::with_capacity(devices.len());

    for (i, path) in devices.iter().enumerate() {
        println!("Opening {path} at {BAUD_RATE},8N1");

        let mut stream = mio_serial::new(path.as_str(), BAUD_RATE).open_native_async().unwrap();
        poll.registry().register(&mut stream, Token(i), Interest::READABLE).unwrap();

        ports.push(Port {
            stream,
            samples: Samples::new(path.clone(), WaveType::SINE),
            buf: [0; FRAME_LEN],
            filled: 0,
        });
    }

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in events.iter() {
            let port = &mut ports[event.token().0];

            // Drain until WouldBlock (edge-triggered).
            loop {
                match port.stream.read(&mut port.buf[port.filled..]) {
                    Ok(0) => {
                        eprintln!("[{}] EOF", port.samples.device);
                        break;
                    }
                    Ok(n) => {
                        port.filled += n;
                        if port.filled < FRAME_LEN {
                            continue;
                        }
                        port.filled = 0;

                        let Ok(rx_time) = SystemTime::now().duration_since(UNIX_EPOCH) else {
                            continue;
                        };
                        let value = f64::from_le_bytes(port.buf[..8].try_into().unwrap());
                        let tx_ns = u128::from_le_bytes(port.buf[8..].try_into().unwrap());
                        let Ok(secs) = u64::try_from(tx_ns / NANOS_PER_SEC) else {
                            eprintln!("[{}] bad tx timestamp (misaligned?)", port.samples.device);
                            continue;
                        };
                        let tx_time = Duration::new(secs, (tx_ns % NANOS_PER_SEC) as u32);

                       // println!("{} - {} - {:?} - {:?}", port.samples.device, value, tx_time, rx_time);
                        let sample = Sample::new(value, tx_time, rx_time);
                        Samples::add_sample_inner(&mut port.samples.samples, sample, Samples::MAX_SAMPLES);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        eprintln!("[{}] read error: {e}", port.samples.device);
                        break;
                    }
                }
            }
        }

        if ports.iter().all(|p| p.samples.samples.len() >= Samples::MAX_SAMPLES) {
            break;
        }
    }

    for p in &ports {
        let path = format!(
            "samples_{}.csv",
            p.samples.device.trim_start_matches('/').replace('/', "_")
        );
        if let Err(e) = write_csv(&path, &p.samples) {
            eprintln!("failed to write {path}: {e}");
        }
    }
}

fn write_csv(path: &str, s: &Samples) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "VALUE,RX TIME,TX TIME")?;
    for s in s.samples.iter() {
        writeln!(w, "{},{},{}", s.value, s.rx_time.as_nanos(), s.tx_time.as_nanos())?;
    }
    w.flush()
}