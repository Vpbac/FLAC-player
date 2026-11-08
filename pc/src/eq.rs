//! Parametric Equalizer (Peace EQ / EqualizerAPO / AutoEQ compatible)
//! 64-bit floating-point RBJ Audio EQ Cookbook biquad filter cascade.

use std::f64::consts::PI;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterType {
    #[default]
    Peaking,   // PK / PEA
    LowShelf,  // LSC / LS
    HighShelf, // HSC / HS
    LowPass,   // LP / LPQ
    HighPass,  // HP / HPQ
    Notch,     // NO
    BandPass,  // BP
}

impl FilterType {
    pub fn code(self) -> &'static str {
        match self {
            FilterType::Peaking => "PK",
            FilterType::LowShelf => "LSC",
            FilterType::HighShelf => "HSC",
            FilterType::LowPass => "LPQ",
            FilterType::HighPass => "HPQ",
            FilterType::Notch => "NO",
            FilterType::BandPass => "BP",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FilterType::Peaking => "PK (Peaking)",
            FilterType::LowShelf => "LSC (Low Shelf)",
            FilterType::HighShelf => "HSC (High Shelf)",
            FilterType::LowPass => "LP (Low Pass)",
            FilterType::HighPass => "HP (High Pass)",
            FilterType::Notch => "NO (Notch)",
            FilterType::BandPass => "BP (Band Pass)",
        }
    }

    pub fn from_token(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "PK" | "PEA" | "PEQ" | "BELL" => Some(FilterType::Peaking),
            "LSC" | "LS" | "LSH" | "LS6" | "LS12" => Some(FilterType::LowShelf),
            "HSC" | "HS" | "HSH" | "HS6" | "HS12" => Some(FilterType::HighShelf),
            "LP" | "LPQ" | "LP6" | "LP12" => Some(FilterType::LowPass),
            "HP" | "HPQ" | "HP6" | "HP12" => Some(FilterType::HighPass),
            "NO" | "NOTCH" => Some(FilterType::Notch),
            "BP" => Some(FilterType::BandPass),
            _ => None,
        }
    }

    pub const ALL: &'static [FilterType] = &[
        FilterType::Peaking,
        FilterType::LowShelf,
        FilterType::HighShelf,
        FilterType::LowPass,
        FilterType::HighPass,
        FilterType::Notch,
        FilterType::BandPass,
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub enabled: bool,
    pub filter_type: FilterType,
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            enabled: true,
            filter_type: FilterType::Peaking,
            freq: 1000.0,
            gain_db: 0.0,
            q: 1.41,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqConfig {
    pub enabled: bool,
    pub name: String,
    pub preamp_db: f64,
    pub bands: Vec<EqBand>,
}

impl Default for EqConfig {
    fn default() -> Self {
        let freqs = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
        let bands = freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| EqBand {
                enabled: true,
                filter_type: if i == 0 {
                    FilterType::LowShelf
                } else if i == freqs.len() - 1 {
                    FilterType::HighShelf
                } else {
                    FilterType::Peaking
                },
                freq: f,
                gain_db: 0.0,
                q: if i == 0 || i == freqs.len() - 1 { 0.70 } else { 1.41 },
            })
            .collect();
        Self {
            enabled: false,
            name: "Flat (Mặc định)".into(),
            preamp_db: 0.0,
            bands,
        }
    }
}

impl EqConfig {
    /// Parse EqualizerAPO / Peace / AutoEQ ParametricEQ.txt format
    pub fn from_autoeq_text(name: &str, text: &str) -> Self {
        let mut preamp_db = 0.0;
        let mut bands = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let lower = line.to_lowercase();
            if lower.starts_with("preamp:") {
                let rest = line[7..].trim().trim_end_matches("dB").trim_end_matches("db").trim();
                if let Ok(v) = rest.parse::<f64>() {
                    preamp_db = v.clamp(-24.0, 12.0);
                }
                continue;
            }
            if lower.starts_with("filter") {
                if let Some(colon_idx) = line.find(':') {
                    let body = line[colon_idx + 1..].trim();
                    let tokens: Vec<&str> = body.split_whitespace().collect();
                    if tokens.is_empty() {
                        continue;
                    }
                    let mut idx = 0;
                    let mut enabled = true;
                    if tokens[idx].eq_ignore_ascii_case("ON") {
                        enabled = true;
                        idx += 1;
                    } else if tokens[idx].eq_ignore_ascii_case("OFF") {
                        enabled = false;
                        idx += 1;
                    }
                    if idx >= tokens.len() {
                        continue;
                    }
                    let Some(filter_type) = FilterType::from_token(tokens[idx]) else {
                        continue;
                    };
                    idx += 1;

                    let mut freq = 1000.0;
                    let mut gain_db = 0.0;
                    let mut q = match filter_type {
                        FilterType::LowShelf | FilterType::HighShelf | FilterType::LowPass | FilterType::HighPass => 0.707,
                        _ => 1.41,
                    };

                    while idx < tokens.len() {
                        let key = tokens[idx].to_uppercase();
                        if key == "FC" && idx + 1 < tokens.len() {
                            let val_str = tokens[idx + 1].trim_end_matches("Hz").trim_end_matches("hz").replace(',', ".");
                            if let Ok(v) = val_str.parse::<f64>() {
                                freq = v.clamp(20.0, 20000.0);
                            }
                            idx += 2;
                            if idx < tokens.len() && tokens[idx].eq_ignore_ascii_case("Hz") {
                                idx += 1;
                            }
                        } else if key == "GAIN" && idx + 1 < tokens.len() {
                            let val_str = tokens[idx + 1].trim_end_matches("dB").trim_end_matches("db").replace(',', ".");
                            if let Ok(v) = val_str.parse::<f64>() {
                                gain_db = v.clamp(-24.0, 24.0);
                            }
                            idx += 2;
                            if idx < tokens.len() && tokens[idx].eq_ignore_ascii_case("dB") {
                                idx += 1;
                            }
                        } else if key == "Q" && idx + 1 < tokens.len() {
                            let val_str = tokens[idx + 1].replace(',', ".");
                            if let Ok(v) = val_str.parse::<f64>() {
                                q = v.clamp(0.1, 18.0);
                            }
                            idx += 2;
                        } else {
                            idx += 1;
                        }
                    }

                    bands.push(EqBand {
                        enabled,
                        filter_type,
                        freq,
                        gain_db,
                        q,
                    });
                }
            }
        }

        if bands.is_empty() {
            return Self::default();
        }

        Self {
            enabled: true,
            name: name.to_string(),
            preamp_db,
            bands,
        }
    }

    pub fn to_autoeq_text(&self) -> String {
        let mut out = format!("Preamp: {:.1} dB\n", self.preamp_db);
        for (i, b) in self.bands.iter().enumerate() {
            let state = if b.enabled { "ON" } else { "OFF" };
            out.push_str(&format!(
                "Filter {}: {} {} Fc {:.0} Hz Gain {:.1} dB Q {:.2}\n",
                i + 1,
                state,
                b.filter_type.code(),
                b.freq,
                b.gain_db,
                b.q
            ));
        }
        out
    }

    /// Evaluate combined frequency response (in dB) at `freq_hz` for graph plotting
    pub fn response_db(&self, freq_hz: f64, sample_rate: f64) -> f64 {
        let mut total_db = self.preamp_db;
        for b in &self.bands {
            if !b.enabled {
                continue;
            }
            let coeffs = BiquadCoeffs::design(b, sample_rate);
            total_db += coeffs.magnitude_db(freq_hz, sample_rate);
        }
        total_db
    }

    pub fn builtin_presets() -> Vec<EqConfig> {
        vec![
            Self::default(),
            Self::from_autoeq_text(
                "AutoEQ: Sony WH-1000XM4 (Harman)",
                "Preamp: -6.3 dB
Filter 1: ON LSC Fc 105 Hz Gain 3.1 dB Q 0.70
Filter 2: ON PK Fc 205 Hz Gain -5.2 dB Q 1.15
Filter 3: ON PK Fc 890 Hz Gain 2.4 dB Q 1.40
Filter 4: ON PK Fc 2450 Hz Gain 4.8 dB Q 1.80
Filter 5: ON PK Fc 4300 Hz Gain 3.9 dB Q 2.10
Filter 6: ON HSC Fc 10000 Hz Gain 4.5 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "AutoEQ: Sennheiser HD600 (Harman)",
                "Preamp: -5.8 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.6 dB Q 0.70
Filter 2: ON PK Fc 210 Hz Gain -1.8 dB Q 1.20
Filter 3: ON PK Fc 3300 Hz Gain -2.1 dB Q 2.20
Filter 4: ON HSC Fc 10000 Hz Gain 2.0 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "AutoEQ: AirPods Pro 2 (Harman)",
                "Preamp: -4.2 dB
Filter 1: ON LSC Fc 105 Hz Gain 2.2 dB Q 0.70
Filter 2: ON PK Fc 350 Hz Gain -1.6 dB Q 1.30
Filter 3: ON PK Fc 1850 Hz Gain 3.1 dB Q 1.90
Filter 4: ON PK Fc 5500 Hz Gain -2.4 dB Q 2.50
Filter 5: ON HSC Fc 10500 Hz Gain 3.2 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "AutoEQ: Moondrop Aria / Blessing (IEF)",
                "Preamp: -4.8 dB
Filter 1: ON LSC Fc 105 Hz Gain 3.8 dB Q 0.70
Filter 2: ON PK Fc 280 Hz Gain -1.4 dB Q 1.20
Filter 3: ON PK Fc 3100 Hz Gain -2.2 dB Q 2.00
Filter 4: ON PK Fc 6200 Hz Gain 2.5 dB Q 2.80
Filter 5: ON HSC Fc 11000 Hz Gain 3.6 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "AutoEQ: Sony IER-M9 / XBA-N3",
                "Preamp: -5.0 dB
Filter 1: ON LSC Fc 105 Hz Gain 2.5 dB Q 0.70
Filter 2: ON PK Fc 230 Hz Gain -2.8 dB Q 1.10
Filter 3: ON PK Fc 3000 Hz Gain 3.5 dB Q 1.70
Filter 4: ON HSC Fc 10000 Hz Gain 4.0 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "Harman Bass Boost (+6.5dB Sub-Bass)",
                "Preamp: -6.5 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.71
Filter 2: ON PK Fc 220 Hz Gain -1.5 dB Q 1.40
Filter 3: ON PK Fc 3200 Hz Gain 2.0 dB Q 1.60
Filter 4: ON HSC Fc 10000 Hz Gain 2.5 dB Q 0.70",
            ),
            Self::from_autoeq_text(
                "Audiophile Vocal & Air (Chi tiết)",
                "Preamp: -4.0 dB
Filter 1: ON LSC Fc 90 Hz Gain 1.8 dB Q 0.70
Filter 2: ON PK Fc 250 Hz Gain -1.5 dB Q 1.41
Filter 3: ON PK Fc 1500 Hz Gain 2.2 dB Q 1.30
Filter 4: ON PK Fc 3500 Hz Gain 2.8 dB Q 1.80
Filter 5: ON HSC Fc 11000 Hz Gain 3.5 dB Q 0.70",
            ),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    pub fn design(band: &EqBand, sample_rate: f64) -> Self {
        let fs = sample_rate.max(8000.0);
        let f0 = band.freq.clamp(10.0, fs * 0.49);
        let q = band.q.clamp(0.1, 18.0);
        let db = band.gain_db.clamp(-24.0, 24.0);

        let a = 10.0_f64.powf(db / 40.0);
        let w0 = 2.0 * PI * f0 / fs;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match band.filter_type {
            FilterType::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w0,
                1.0 - alpha / a,
            ),
            FilterType::LowShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
            FilterType::HighShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
            FilterType::LowPass => (
                (1.0 - cos_w0) * 0.5,
                1.0 - cos_w0,
                (1.0 - cos_w0) * 0.5,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            FilterType::HighPass => (
                (1.0 + cos_w0) * 0.5,
                -(1.0 + cos_w0),
                (1.0 + cos_w0) * 0.5,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            FilterType::Notch => (
                1.0,
                -2.0 * cos_w0,
                1.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            FilterType::BandPass => (
                alpha,
                0.0,
                -alpha,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    pub fn magnitude_db(&self, freq_hz: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * freq_hz / sample_rate.max(8000.0);
        let cos_w = w.cos();
        let sin_w = w.sin();
        let cos_2w = (2.0 * w).cos();
        let sin_2w = (2.0 * w).sin();

        let num_r = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_i = -(self.b1 * sin_w + self.b2 * sin_2w);
        let den_r = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_i = -(self.a1 * sin_w + self.a2 * sin_2w);

        let num_mag2 = num_r * num_r + num_i * num_i;
        let den_mag2 = (den_r * den_r + den_i * den_i).max(1e-18);
        10.0 * (num_mag2 / den_mag2).log10()
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, x: f64, c: &BiquadCoeffs) -> f64 {
        // Direct Form II Transposed for numerical stability
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

pub struct EqProcessor {
    config: EqConfig,
    sample_rate: u32,
    channels: usize,
    preamp_gain: f64,
    coeffs: Vec<BiquadCoeffs>,
    states: Vec<Vec<BiquadState>>, // [band_idx][channel_idx]
}

impl Default for EqProcessor {
    fn default() -> Self {
        Self::new(EqConfig::default(), 44100, 2)
    }
}

impl EqProcessor {
    pub fn new(config: EqConfig, sample_rate: u32, channels: usize) -> Self {
        let mut proc = Self {
            config,
            sample_rate: sample_rate.max(8000),
            channels: channels.max(1),
            preamp_gain: 1.0,
            coeffs: Vec::new(),
            states: Vec::new(),
        };
        proc.rebuild_filters(true);
        proc
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled && !self.coeffs.is_empty()
    }

    pub fn update_config(&mut self, config: EqConfig) {
        let bands_count_changed = self.config.bands.len() != config.bands.len();
        self.config = config;
        self.rebuild_filters(bands_count_changed);
    }

    pub fn set_format(&mut self, sample_rate: u32, channels: usize) {
        if self.sample_rate != sample_rate || self.channels != channels {
            self.sample_rate = sample_rate.max(8000);
            self.channels = channels.max(1);
            self.rebuild_filters(true);
        }
    }

    pub fn reset_state(&mut self) {
        for band_states in &mut self.states {
            for s in band_states {
                *s = BiquadState::default();
            }
        }
    }

    fn rebuild_filters(&mut self, reset_states: bool) {
        self.preamp_gain = 10.0_f64.powf(self.config.preamp_db / 20.0);
        self.coeffs.clear();
        for b in &self.config.bands {
            if b.enabled {
                self.coeffs.push(BiquadCoeffs::design(b, self.sample_rate as f64));
            }
        }
        if reset_states || self.states.len() != self.coeffs.len() {
            self.states = vec![vec![BiquadState::default(); self.channels]; self.coeffs.len()];
        }
    }

    #[inline]
    pub fn process_sample(&mut self, sample_i32: i32, channel: usize) -> i32 {
        if !self.config.enabled {
            return sample_i32;
        }
        let ch = channel.min(self.channels.saturating_sub(1));
        let mut x = (sample_i32 as f64 / 2_147_483_648.0) * self.preamp_gain;
        for (b_idx, c) in self.coeffs.iter().enumerate() {
            x = self.states[b_idx][ch].process(x, c);
        }
        // Soft knee limiting near 0 dBFS to avoid harsh digital clipping
        if x > 0.98 {
            x = 0.98 + 0.02 * ((x - 0.98) / 0.02).tanh();
        } else if x < -0.98 {
            x = -0.98 - 0.02 * ((-x - 0.98) / 0.02).tanh();
        }
        (x * 2_147_483_648.0).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}
