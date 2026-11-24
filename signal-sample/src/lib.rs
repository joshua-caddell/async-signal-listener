extern crate core;

use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_signal::{SignalEventHandler, SignalStream};

#[derive(Default, PartialEq)]
pub enum WaveType {
    #[default]
    UNKNOWN,
    SINE,
    SQUARE,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct Sample {
    pub value: f64,
    #[allow(unused)]
    pub tx_time: Duration,
    #[allow(unused)]
    pub rx_time: Duration,
    pub elapsed: Duration,
}

impl PartialEq for Sample {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Sample {}

impl PartialOrd for Sample {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl Ord for Sample {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.partial_cmp(&other.value).unwrap_or(std::cmp::Ordering::Equal)
    }
}

#[derive(Default)]
pub struct Samples {
    pub device: String,
    pub wave_type: WaveType,
    pub samples: VecDeque<Sample>,
    crests: VecDeque<Sample>,
    troughs: VecDeque<Sample>,
    pub wavelength: Duration,
    pub frequency: f32,
    pub amplitude: f32,
}

impl Sample {
    pub fn new(value: f64, tx_time: Duration, rx_time: Duration) -> Self {
        Sample {
            value,
            tx_time,
            rx_time, 
            elapsed: rx_time.checked_sub(tx_time).unwrap_or(Duration::new(0, 0)),
        }
    }
}

impl Samples {

    pub const MAX_SAMPLES: usize = 100_000;
    const MAX_CRESTS_TROUGHS: usize = 10_000;

    pub fn new(device: String, wave_type: WaveType) -> Self {
        Samples {
            device,
            wave_type,
            samples: VecDeque::with_capacity(Self::MAX_SAMPLES),
            crests: VecDeque::with_capacity(Self::MAX_CRESTS_TROUGHS),
            troughs: VecDeque::with_capacity(Self::MAX_CRESTS_TROUGHS),
            wavelength: Duration::ZERO,
            frequency: 0.0,
            amplitude: 0.0
        }
    }
    pub fn add_sample(&mut self, value: f64, tx_time: Duration, rx_time: Duration) {
        let sample = Sample::new(value, tx_time, rx_time);

        if self.wave_type == WaveType::SINE && self.samples.len() > 1 {
            let prev_sample_index = self.samples.len() - 1;
            let prev_sample = &self.samples[prev_sample_index];
            let prev_prev_sample = &self.samples[prev_sample_index - 1];

            if self.wave_type == WaveType::SINE || self.wave_type == WaveType::UNKNOWN {
                if Self::is_crest(&sample, prev_sample, prev_prev_sample) {
                    Self::add_sample_inner(&mut self.crests, sample, Self::MAX_CRESTS_TROUGHS);
                    self.calculate_wavelength_frequency();
                } else if Self::is_trough(&sample, prev_sample, prev_prev_sample) {
                    Self::add_sample_inner(&mut self.troughs, sample, Self::MAX_CRESTS_TROUGHS);
                }
            }
        } else if self.wave_type == WaveType::SQUARE && self.samples.len() > 0 {
            let prev_sample_index = self.samples.len() - 1;
            let prev_sample = &self.samples[prev_sample_index];

            if sample > *prev_sample {
                Self::add_sample_inner(&mut self.crests, sample, Self::MAX_CRESTS_TROUGHS);
                self.calculate_wavelength_frequency();
            } else if sample < *prev_sample {
                Self::add_sample_inner(&mut self.troughs, sample, Self::MAX_CRESTS_TROUGHS);
            }
        }

        Self::add_sample_inner(&mut self.samples, sample, Self::MAX_SAMPLES);
    }

    fn is_crest(curr: &Sample, prev: &Sample, prev_prev: &Sample) -> bool {
        prev_prev < prev && curr < prev
    }

    fn is_trough(curr: &Sample, prev: &Sample, prev_prev: &Sample) -> bool {
        prev_prev > prev && curr > prev
    }

    pub fn add_sample_inner(samples: &mut VecDeque<Sample>, sample: Sample, max_size: usize) {
        if samples.len() == max_size {
            samples.pop_front();
        }

        samples.push_back(sample)
    }

    fn calculate_wavelength_frequency(&mut self) {
        if self.crests.len() < 2 {
            return;
        }

        let first_sample = self.crests.front().unwrap();
        let curr_sample = self.crests.back().unwrap();

        let abs = curr_sample.value.abs() as f32;

        if abs > self.amplitude {
            self.amplitude = abs;
        }

        if let Some(duration) = curr_sample.rx_time.checked_sub(first_sample.rx_time) {

            let num_intervals: u32 = (self.crests.len() - 1) as u32;

            if let Some(wl) = duration.checked_div(num_intervals) {
                self.wavelength = wl;
                self.frequency = 1.0 / self.wavelength.as_secs_f32();
            }
        }
    }
}

#[async_trait]
impl SignalEventHandler for Samples {
    async fn handle(&mut self, stream: &mut dyn SignalStream) -> bool {
        let mut buf = vec![0u8; 24];

        // first 8 bytes of packet are the wave value. last 16 is timestamp in nanos
        if let Err(e) = stream.read(&mut buf).await {
            eprintln!("{}", e);
            return true;
        }

        let rx_time: Duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let value = f64::from_le_bytes(buf[..8].try_into().unwrap());

        let tx_ns =  u128::from_le_bytes(buf[8..].try_into().unwrap());
        if let Err(_) = u64::try_from(tx_ns / 1_000_000_000u128) {
            self.samples.clear(); 
            return true
        };

        let tx_time =  Duration::from_nanos_u128(tx_ns);

        let sample= Sample::new(value, tx_time, rx_time);
        Self::add_sample_inner(&mut self.samples, sample, Self::MAX_SAMPLES);

        true
    }
}