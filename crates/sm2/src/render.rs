//! Putting one sound of a Wwise event into the mix: its volume (already summed over the containers above it), its pitch
//! offset and its start delay are applied to the decoded audio. Pure sample arithmetic.
use crate::wem::Pcm;

/// Apply a linear `gain`, a pitch offset in cents (a positive offset plays the sound faster and higher, as in Wwise) and a
/// start delay in milliseconds. The sample rate and channel count stay as they are.
pub fn apply_voice(pcm: &Pcm, gain: f32, pitch_cents: f32, delay_ms: u32) -> Pcm {
    let ch = pcm.channels.max(1) as usize;
    let frames_in = pcm.samples.len() / ch;
    let mut samples: Vec<i16>;
    if pitch_cents.abs() > 1.0 && frames_in > 1 {
        // playing `ratio` times faster = reading the source `ratio` frames at a time
        let ratio = 2f64.powf(pitch_cents as f64 / 1200.0);
        let frames_out = ((frames_in as f64 / ratio).floor() as usize).max(1);
        samples = Vec::with_capacity(frames_out * ch);
        for i in 0..frames_out {
            let pos = i as f64 * ratio;
            let a = (pos.floor() as usize).min(frames_in - 1);
            let b = (a + 1).min(frames_in - 1);
            let t = (pos - a as f64) as f32;
            for c in 0..ch {
                let v = pcm.samples[a * ch + c] as f32 * (1.0 - t) + pcm.samples[b * ch + c] as f32 * t;
                samples.push(v.round() as i16);
            }
        }
    } else {
        samples = pcm.samples.clone();
    }
    if (gain - 1.0).abs() > 1e-6 {
        for s in &mut samples {
            *s = (*s as f32 * gain).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }
    if delay_ms > 0 {
        let pad = (pcm.sample_rate as u64 * delay_ms as u64 / 1000) as usize * ch;
        let mut padded = vec![0i16; pad];
        padded.extend_from_slice(&samples);
        samples = padded;
    }
    Pcm { channels: pcm.channels, sample_rate: pcm.sample_rate, samples }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(rate: u32, samples: Vec<i16>) -> Pcm {
        Pcm { channels: 1, sample_rate: rate, samples }
    }

    #[test]
    fn identity_when_nothing_is_asked() {
        let p = mono(48_000, vec![1, -2, 3, 1000]);
        assert_eq!(apply_voice(&p, 1.0, 0.0, 0).samples, p.samples);
    }

    #[test]
    fn gain_scales_and_clips() {
        let p = mono(48_000, vec![1000, -1000, 30_000]);
        assert_eq!(apply_voice(&p, 0.5, 0.0, 0).samples, vec![500, -500, 15_000]);
        assert_eq!(apply_voice(&p, 2.0, 0.0, 0).samples, vec![2000, -2000, i16::MAX]);
    }

    #[test]
    fn an_octave_up_halves_the_length_and_an_octave_down_doubles_it() {
        let p = mono(48_000, (0..4800).map(|i| (i % 100) as i16).collect());
        assert_eq!(apply_voice(&p, 1.0, 1200.0, 0).samples.len(), 2400);
        let down = apply_voice(&p, 1.0, -1200.0, 0).samples.len();
        assert!((down as i64 - 9600).abs() <= 2, "{down}");
        // a constant stays constant
        let c = mono(48_000, vec![700; 1000]);
        assert!(apply_voice(&c, 1.0, 300.0, 0).samples.iter().all(|&s| s == 700));
    }

    #[test]
    fn a_delay_is_silence_in_front_for_every_channel() {
        let st = Pcm { channels: 2, sample_rate: 1000, samples: vec![5, 6, 7, 8] };
        let d = apply_voice(&st, 1.0, 0.0, 10); // 10 ms at 1 kHz = 10 frames = 20 samples
        assert_eq!(d.samples.len(), 24);
        assert!(d.samples[..20].iter().all(|&s| s == 0));
        assert_eq!(&d.samples[20..], &[5, 6, 7, 8]);
    }
}
