use async_trait::async_trait;
use spidev::{Spidev, SpidevOptions};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_gpiod::{Chip, EdgeDetect, Input, Lines, Options};
use tokio_serial::SerialPortBuilderExt;


pub use spidev::SpiModeFlags;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[async_trait]
pub trait SignalEventHandler: Send + 'static {
    async fn handle(&mut self, stream: &mut dyn SignalStream) -> bool;
}

#[async_trait]
pub trait SignalStream: Send {
    async fn read(&mut self, buf: &mut [u8]) -> Result<()>;
}

pub struct SerialStream {
    #[allow(unused)]
    pub device: String,
    pub(crate) inner: tokio_serial::SerialStream
}

pub struct SpiStream {
    pub device: String, 
    pub(crate) inner: Spidev,
}

pub struct SignalListener<H: SignalEventHandler> {
    handles: Vec<JoinHandle<()>>,
    pub handlers: Vec<Arc<Mutex<H>>>,
}

impl<H: SignalEventHandler> SignalListener<H> {

    pub fn new() -> Self {
        SignalListener {
            handles: vec![],
            handlers: vec![],
        }
    }

    pub fn add_serial(&mut self, device: String, baud_rate: u32, handler: H) -> Result<()> {
        let mtx_handler = self.create_handler(handler);

        let mut stream = match SerialStream::new(&device, baud_rate) {
            Ok(stream) => stream,
            Err(e) => return Err(e)
        };

        self.handles.push(tokio::spawn(async move{
            loop {
                match stream.inner.readable().await {
                    Ok(_) => {
                        let mut guard = mtx_handler.lock().await;
                        if !guard.handle(&mut stream).await {
                            break;
                        }
                    },
                    Err(e) => eprintln!("{}", e),
                }
            }
        }));

        Ok(())
    }

    pub async fn add_spi(&mut self, device: String, speed_hz: u32, mode: SpiModeFlags, ready_pin: u32, handler: H) -> Result<()> {
        let mtx_handler = self.create_handler(handler);

        let mut spi = Spidev::open(&device)?;
        let options =
            SpidevOptions::new().bits_per_word(8).max_speed_hz(speed_hz).mode(mode).build();
        spi.configure(&options)?;

        let chip = Chip::new("gpiochip0").await?;
        let opts = Options::input([ready_pin]).edge(EdgeDetect::Rising);
        let mut pin: Lines<Input> = chip.request_lines(opts).await?;

        let mut stream = match SpiStream::new(&device, spi) {
            Ok(stream) => stream,
            Err(e) => return Err(e)
        };

        self.handles.push(tokio::spawn(async move{
            let mut pending = match pin.get_values([false; 1]).await {
                Ok([level]) => level,
                Err(e) => {
                    eprintln!("error reading ready pin: {}", e);
                    false
                }
            };

            loop {
                if pending {
                    let mut guard = mtx_handler.lock().await;
                    if !guard.handle(&mut stream).await {
                        break;
                    }
                }

                pending = match pin.read_event().await {
                    Ok(_) => true,
                    Err(e) => {
                        eprintln!("{}", e);
                        false
                    },
                }
            }
        }));

        Ok(())
    }

    #[allow(unused)]
    pub async fn wait(self) {
        for handle in self.handles {
            handle.await.ok();
        }
    }

    fn create_handler(&mut self, handler: H) -> Arc<Mutex<H>> {
        let handler = Arc::new(Mutex::new(handler));
        self.handlers.push(Arc::clone(&handler));
        handler
    }
}

impl SerialStream {
    pub fn new(device: &String, baud_rate: u32) -> Result<Self> {

        let serial_port = match tokio_serial::new(device, baud_rate).open_native_async() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error opening port: {}", e);
                return Err(e.into());
            }
        };

        Ok(SerialStream {
            device: device.clone(),
            inner: serial_port
        })
    }
}

#[async_trait]
impl SignalStream for SerialStream {

    async fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        if let Err(e) = self.inner.read_exact(buf).await {
            return Err(e.into());
        };
        Ok(())
    }
}

impl SpiStream {
    pub fn new(device: &String, inner: Spidev) -> Result<Self> {
        Ok (SpiStream {
            device: device.clone(),
            inner,
        })
    }
}

#[async_trait]
impl SignalStream for SpiStream {

    async fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        use std::io::Read;

        if let Err(e) = self.inner.read(buf) {
            return Err(e.into());
        };
        Ok(())
    }
}
