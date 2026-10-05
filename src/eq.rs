//! Winamp-style 10-band equalizer, preamp and balance, applied in a wrapper around librespot's sink.

use std::sync::Mutex;

use librespot::playback::{
    SAMPLE_RATE,
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};

/// Winamp's band centres (Hz).
pub const BANDS: [f64; 10] = [60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0];
/// Slider range of Winamp's EQ window (+12 dB to -12 dB).
pub const MAX_DB: f32 = 12.0;
// ponytail: one Q for every band, close to Winamp's octave-wide look; per-band Q if it sounds off.
const Q: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EqState {
    pub on: bool,
    /// dB, -MAX_DB..=MAX_DB.
    pub preamp: f32,
    /// dB per band, -MAX_DB..=MAX_DB.
    pub bands: [f32; 10],
    /// -1 (left) ..= 1 (right).
    pub balance: f32,
}

impl EqState {
    pub fn from_json(v: &serde_json::Value) -> Self {
        let db = |v: &serde_json::Value| v.as_f64().map_or(0.0, |d| (d as f32).clamp(-MAX_DB, MAX_DB));
        let mut bands = [0.0; 10];
        for (i, b) in bands.iter_mut().enumerate() {
            *b = db(&v["bands"][i]);
        }
        EqState {
            on: v["on"].as_bool().unwrap_or(false),
            preamp: db(&v["preamp"]),
            bands,
            balance: v["balance"].as_f64().map_or(0.0, |b| (b as f32).clamp(-1.0, 1.0)),
        }
    }

    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!({ "on": self.on, "preamp": self.preamp, "bands": self.bands, "balance": self.balance })
    }
}

/// Current settings and a version the sink compares to pick up changes.
static CURRENT: Mutex<(EqState, u64)> = Mutex::new((EqState { on: false, preamp: 0.0, bands: [0.0; 10], balance: 0.0 }, 0));

/// Takes effect on the next audio packet of the running player.
pub fn set(state: EqState) {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if cur.0 != state {
        *cur = (state, cur.1 + 1);
    }
}

/// RBJ peaking filter coefficients, normalised: [b0, b1, b2, a1, a2].
fn peaking(freq: f64, gain_db: f64) -> [f64; 5] {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * freq / SAMPLE_RATE as f64;
    let alpha = w0.sin() / (2.0 * Q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha / a;
    [(1.0 + alpha * a) / a0, -2.0 * cos / a0, (1.0 - alpha * a) / a0, -2.0 * cos / a0, (1.0 - alpha / a) / a0]
}

/// Wraps the real sink; samples are interleaved stereo f64 at librespot's SAMPLE_RATE.
pub struct EqSink {
    inner: Box<dyn Sink>,
    version: u64,
    state: EqState,
    preamp: f64,
    gains: [f64; 2],
    coeffs: Vec<[f64; 5]>,
    /// Filter memory per channel and active band (transposed direct form II).
    z: [Vec<[f64; 2]>; 2],
}

impl EqSink {
    pub fn new(inner: Box<dyn Sink>) -> Self {
        let mut sink = EqSink { inner, version: u64::MAX, state: EqState::default(), preamp: 1.0, gains: [1.0; 2], coeffs: Vec::new(), z: [Vec::new(), Vec::new()] };
        sink.sync();
        sink
    }

    fn sync(&mut self) {
        let (state, version) = *CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        if version != self.version {
            self.version = version;
            self.apply(state);
        }
    }

    fn apply(&mut self, state: EqState) {
        self.state = state;
        let b = state.balance as f64;
        self.gains = [(1.0 - b).min(1.0), (1.0 + b).min(1.0)];
        self.preamp = if state.on { 10f64.powf(state.preamp as f64 / 20.0) } else { 1.0 };
        self.coeffs = match state.on {
            true => BANDS.iter().zip(state.bands).filter(|(_, g)| *g != 0.0).map(|(f, g)| peaking(*f, g as f64)).collect(),
            false => Vec::new(),
        };
        // Keep the memory when the band count stays, so dragging a slider doesn't click.
        for z in &mut self.z {
            z.resize(self.coeffs.len(), [0.0; 2]);
        }
    }

    fn process(&mut self, samples: &mut [f64]) {
        if self.coeffs.is_empty() && self.preamp == 1.0 && self.gains == [1.0; 2] {
            return;
        }
        for frame in samples.chunks_exact_mut(2) {
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = *s * self.preamp;
                for (c, z) in self.coeffs.iter().zip(self.z[ch].iter_mut()) {
                    let y = c[0] * x + z[0];
                    z[0] = c[1] * x - c[3] * y + z[1];
                    z[1] = c[2] * x - c[4] * y;
                    x = y;
                }
                *s = (x * self.gains[ch]).clamp(-1.0, 1.0);
            }
        }
    }
}

impl Sink for EqSink {
    fn start(&mut self) -> SinkResult<()> {
        self.inner.start()
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.inner.stop()
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        match packet {
            AudioPacket::Samples(mut samples) => {
                self.sync();
                self.process(&mut samples);
                self.inner.write(AudioPacket::Samples(samples), converter)
            }
            raw => self.inner.write(raw, converter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Null;
    impl Sink for Null {
        fn write(&mut self, _: AudioPacket, _: &mut Converter) -> SinkResult<()> {
            Ok(())
        }
    }

    fn sink(state: EqState) -> EqSink {
        let mut s = EqSink::new(Box::new(Null));
        s.apply(state);
        s
    }

    /// Peak amplitude of a steady sine at `freq` after the filters settle.
    fn response(state: EqState, freq: f64) -> f64 {
        let mut s = sink(state);
        let n = SAMPLE_RATE as usize;
        let mut buf: Vec<f64> = (0..n).flat_map(|i| {
            let v = 0.25 * (2.0 * std::f64::consts::PI * freq * i as f64 / SAMPLE_RATE as f64).sin();
            [v, v]
        }).collect();
        s.process(&mut buf);
        buf[n..].iter().step_by(2).fold(0.0, |m: f64, v| m.max(v.abs())) / 0.25
    }

    #[test]
    fn boosts_its_band_and_leaves_others() {
        let mut state = EqState { on: true, ..Default::default() };
        state.bands[4] = 12.0; // 1 kHz
        let db = |g: f64| 20.0 * g.log10();
        assert!((db(response(state, 1000.0)) - 12.0).abs() < 0.5);
        assert!(db(response(state, 16000.0)).abs() < 1.0);
        // Off means bypass, whatever the sliders say.
        let mut off = sink(EqState { on: false, ..state });
        let mut buf = vec![0.3, -0.7, 0.1, 0.9];
        off.process(&mut buf);
        assert_eq!(buf, vec![0.3, -0.7, 0.1, 0.9]);
    }

    #[test]
    fn balance_attenuates_one_side() {
        let mut s = sink(EqState { balance: 0.5, ..Default::default() });
        let mut buf = vec![0.5, 0.5];
        s.process(&mut buf);
        assert_eq!(buf, vec![0.25, 0.5]);
    }

    #[test]
    fn json_roundtrip_clamps() {
        let mut state = EqState { on: true, preamp: -3.0, balance: -0.2, ..Default::default() };
        state.bands[9] = 7.5;
        assert_eq!(EqState::from_json(&state.to_json()), state);
        let wild = EqState::from_json(&serde_json::json!({ "preamp": 99, "bands": [-40], "balance": 5 }));
        assert_eq!((wild.preamp, wild.bands[0], wild.balance), (MAX_DB, -MAX_DB, 1.0));
    }
}
