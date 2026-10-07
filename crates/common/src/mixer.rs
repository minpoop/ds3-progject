//! The pure part of the mashup's sound playback: clips (stereo, 16-bit, 44.1 kHz) and a small mixer that adds
//! overlapping voices. The Windows device layer (waveOut) lives in the hook DLL; everything here runs and is tested
//! on any OS.
use std::sync::Arc;

pub const RATE: u32 = 44_100;

/// A sound ready to mix: interleaved stereo, signed 16-bit, at [`RATE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub frames: Vec<i16>,
}

impl Clip {
    pub fn len_frames(&self) -> usize {
        self.frames.len() / 2
    }

    pub fn seconds(&self) -> f32 {
        self.len_frames() as f32 / RATE as f32
    }

    /// Convert decoded audio (any channel count and rate) into a clip: linear-interpolation resample, mono is
    /// duplicated to both ears, more than two channels keep the first two.
    pub fn from_pcm(channels: u16, rate: u32, samples: &[i16]) -> Clip {
        let ch = channels.max(1) as usize;
        let frames_in = samples.len() / ch;
        if frames_in == 0 || rate == 0 {
            return Clip { frames: Vec::new() };
        }
        let get = |frame: usize, c: usize| -> f32 { samples[frame * ch + c.min(ch - 1)] as f32 };
        let frames_out = ((frames_in as u64 * RATE as u64) / rate as u64).max(1) as usize;
        let mut out = Vec::with_capacity(frames_out * 2);
        for i in 0..frames_out {
            let pos = i as f64 * rate as f64 / RATE as f64;
            let a = (pos.floor() as usize).min(frames_in - 1);
            let b = (a + 1).min(frames_in - 1);
            let t = (pos - a as f64) as f32;
            for c in 0..2 {
                let v = get(a, c) * (1.0 - t) + get(b, c) * t;
                out.push(v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16);
            }
        }
        Clip { frames: out }
    }

    /// A short sine beep with a fade in/out (used by the in-game audio check).
    pub fn tone(freq: f32, ms: u32, volume: f32) -> Clip {
        let n = (RATE as u64 * ms as u64 / 1000) as usize;
        let fade = (RATE as usize / 100).min(n / 2).max(1); // 10 ms
        let mut frames = Vec::with_capacity(n * 2);
        for i in 0..n {
            let env = (i.min(n - 1 - i).min(fade) as f32) / fade as f32;
            let v = (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin() * volume * env;
            let s = (v * i16::MAX as f32) as i16;
            frames.push(s);
            frames.push(s);
        }
        Clip { frames }
    }
}

struct Voice {
    clip: Arc<Clip>,
    pos: usize, // in frames
    gain: f32,
}

#[derive(Default)]
pub struct Mixer {
    voices: Vec<Voice>,
}

impl Mixer {
    /// More than this many overlapping voices and the oldest is dropped (a runaway trigger must not pile up).
    pub const MAX_VOICES: usize = 24;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn play(&mut self, clip: Arc<Clip>, gain: f32) {
        if clip.len_frames() == 0 {
            return;
        }
        if self.voices.len() >= Self::MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice { clip, pos: 0, gain: gain.clamp(0.0, 4.0) });
    }

    pub fn active(&self) -> usize {
        self.voices.len()
    }

    /// Fill `out` (interleaved stereo) with the next stretch of every voice added together, clipped to 16 bits.
    /// Finished voices are removed. Silence when nothing plays.
    pub fn mix(&mut self, out: &mut [i16]) {
        let frames = out.len() / 2;
        let mut acc = vec![0i32; frames * 2];
        for v in &mut self.voices {
            let avail = v.clip.len_frames() - v.pos;
            let n = avail.min(frames);
            for i in 0..n * 2 {
                acc[i] += (v.clip.frames[v.pos * 2 + i] as f32 * v.gain) as i32;
            }
            v.pos += n;
        }
        self.voices.retain(|v| v.pos < v.clip.len_frames());
        for (o, a) in out.iter_mut().zip(acc) {
            *o = a.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_duplicated_and_the_rate_is_converted() {
        let mono: Vec<i16> = (0..22_050).map(|i| (i % 100) as i16 * 100).collect();
        let c = Clip::from_pcm(1, 22_050, &mono);
        assert_eq!(c.len_frames(), 44_100);
        assert!(c.frames.chunks_exact(2).all(|f| f[0] == f[1]));
        assert!((c.seconds() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn stereo_at_the_target_rate_is_unchanged_and_extra_channels_are_dropped() {
        let st: Vec<i16> = vec![1, -1, 2, -2, 3, -3, 4, -4];
        assert_eq!(Clip::from_pcm(2, RATE, &st).frames, st);
        let six: Vec<i16> = (0..12).collect();
        let c = Clip::from_pcm(6, RATE, &six);
        assert_eq!(c.frames, vec![0, 1, 6, 7]);
        assert!(Clip::from_pcm(2, 0, &st).frames.is_empty());
        assert!(Clip::from_pcm(2, RATE, &[]).frames.is_empty());
    }

    #[test]
    fn downsampling_a_constant_stays_constant() {
        let c = Clip::from_pcm(1, 96_000, &vec![1234i16; 9_600]);
        assert!(c.frames.iter().all(|&s| s == 1234));
        assert_eq!(c.len_frames(), 4_410);
    }

    #[test]
    fn voices_add_and_finish() {
        let a = Arc::new(Clip { frames: vec![100; 8] }); // 4 frames
        let b = Arc::new(Clip { frames: vec![50; 4] }); // 2 frames
        let mut m = Mixer::new();
        m.play(a, 1.0);
        m.play(b, 1.0);
        let mut out = [0i16; 6]; // 3 frames
        m.mix(&mut out);
        assert_eq!(out, [150, 150, 150, 150, 100, 100]);
        assert_eq!(m.active(), 1);
        let mut out2 = [7i16; 6];
        m.mix(&mut out2);
        assert_eq!(out2, [100, 100, 0, 0, 0, 0]);
        assert_eq!(m.active(), 0);
        let mut silence = [9i16; 4];
        m.mix(&mut silence);
        assert_eq!(silence, [0, 0, 0, 0]);
    }

    #[test]
    fn sums_are_clipped_not_wrapped_and_gain_applies() {
        let loud = Arc::new(Clip { frames: vec![30_000; 2] });
        let mut m = Mixer::new();
        m.play(loud.clone(), 1.0);
        m.play(loud.clone(), 1.0);
        let mut out = [0i16; 2];
        m.mix(&mut out);
        assert_eq!(out, [i16::MAX, i16::MAX]);
        m.play(loud, 0.5);
        m.mix(&mut out);
        assert_eq!(out, [15_000, 15_000]);
    }

    #[test]
    fn a_runaway_trigger_cannot_pile_up_voices() {
        let c = Arc::new(Clip { frames: vec![1; 2_000] });
        let mut m = Mixer::new();
        for _ in 0..1_000 {
            m.play(c.clone(), 1.0);
        }
        assert_eq!(m.active(), Mixer::MAX_VOICES);
        m.play(Arc::new(Clip { frames: Vec::new() }), 1.0);
        assert_eq!(m.active(), Mixer::MAX_VOICES, "an empty clip is ignored");
    }

    #[test]
    fn the_beep_has_the_right_length_and_fades() {
        let t = Clip::tone(440.0, 200, 0.5);
        assert_eq!(t.len_frames(), 8_820);
        assert_eq!(t.frames[0], 0);
        assert_eq!(*t.frames.last().unwrap(), 0);
        let peak = t.frames.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 15_000 && peak < 17_000, "{peak}");
    }
}
