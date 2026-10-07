//! Mixing the layers of one Space Marine 2 sound event (a shot is often a crack, a thump and a tail) into one clip.
use crate::wem::Pcm;

/// A mix is never longer than this; a longer one is cut and fades out.
pub const MAX_SECONDS: u32 = 8;
/// The fade-out that hides the cut.
const FADE_MS: u32 = 20;
/// The loudest sample a mix may have, as a fraction of full scale. A louder mix is turned down as a whole to this; a
/// quieter one is left as it is (a mix is never made louder).
const CEILING: f64 = 0.95;

/// Add the layers up into one clip. They all start at time zero and the clip is as long as the longest one. Layers of
/// different sample rates are brought to the highest rate by linear interpolation; a mono layer is played on both
/// sides when any layer is stereo (more than two channels keep the first two, as the game mixer does). A clip longer
/// than [`MAX_SECONDS`] is cut there with a 20 ms fade-out. If the sum is louder than 0.95 of full scale the whole
/// mix is turned down until its peak is exactly that. `None` when there is nothing to mix (no layer, or only empty ones).
pub fn mix_layers(layers: &[Pcm]) -> Option<Pcm> {
    let layers: Vec<&Pcm> = layers.iter().filter(|l| l.sample_rate > 0 && l.channels > 0 && l.samples.len() >= l.channels as usize).collect();
    let rate = layers.iter().map(|l| l.sample_rate).max()?;
    let channels = layers.iter().map(|l| l.channels).max()?.min(2) as usize;

    let mut sum: Vec<i32> = Vec::new();
    for layer in &layers {
        let frames = frames_at(layer, rate, channels);
        if sum.len() < frames.len() {
            sum.resize(frames.len(), 0);
        }
        for (s, f) in sum.iter_mut().zip(&frames) {
            *s += f;
        }
    }

    let max_frames = rate as usize * MAX_SECONDS as usize;
    if sum.len() > max_frames * channels {
        sum.truncate(max_frames * channels);
        let fade = (rate as usize * FADE_MS as usize / 1000).clamp(1, max_frames);
        for k in 0..fade {
            let gain = 1.0 - (k + 1) as f64 / fade as f64; // the last frame ends at zero
            for c in 0..channels {
                let at = (max_frames - fade + k) * channels + c;
                sum[at] = (sum[at] as f64 * gain).round() as i32;
            }
        }
    }

    let peak = sum.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f64;
    let limit = CEILING * i16::MAX as f64;
    let scale = if peak > limit { limit / peak } else { 1.0 };
    let samples = sum.iter().map(|&s| (s as f64 * scale).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16).collect();
    Some(Pcm { channels: channels as u16, sample_rate: rate, samples })
}

/// One layer as interleaved samples with `channels` channels at `rate` Hz.
fn frames_at(layer: &Pcm, rate: u32, channels: usize) -> Vec<i32> {
    let ch = layer.channels as usize;
    let frames_in = layer.samples.len() / ch;
    // channel c of the output comes from channel c of the layer; a mono layer feeds every channel
    let sample = |frame: usize, c: usize| layer.samples[frame * ch + c.min(ch - 1)];
    let mut out = Vec::new();
    if layer.sample_rate == rate {
        out.reserve(frames_in * channels);
        for f in 0..frames_in {
            out.extend((0..channels).map(|c| sample(f, c) as i32));
        }
        return out;
    }
    let frames_out = ((frames_in as u64 * rate as u64 + layer.sample_rate as u64 / 2) / layer.sample_rate as u64).max(1) as usize;
    out.reserve(frames_out * channels);
    for i in 0..frames_out {
        let pos = i as f64 * layer.sample_rate as f64 / rate as f64;
        let a = (pos.floor() as usize).min(frames_in - 1);
        let b = (a + 1).min(frames_in - 1);
        let t = pos - a as f64;
        out.extend((0..channels).map(|c| (sample(a, c) as f64 * (1.0 - t) + sample(b, c) as f64 * t).round() as i32));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(channels: u16, rate: u32, samples: &[i16]) -> Pcm {
        Pcm { channels, sample_rate: rate, samples: samples.to_vec() }
    }

    #[test]
    fn layers_are_added_sample_by_sample() {
        let mix = mix_layers(&[pcm(1, 44100, &[100, 200, -300]), pcm(1, 44100, &[1, 2, 3]), pcm(1, 44100, &[1000, 0, 0])]).unwrap();
        assert_eq!((mix.channels, mix.sample_rate), (1, 44100));
        assert_eq!(mix.samples, vec![1101, 202, -297]);
        // one layer comes out as it went in
        assert_eq!(mix_layers(&[pcm(2, 48000, &[5, -5, 6, -6])]).unwrap().samples, vec![5, -5, 6, -6]);
    }

    #[test]
    fn a_mono_layer_is_played_on_both_sides_next_to_a_stereo_one() {
        let mix = mix_layers(&[pcm(1, 44100, &[100, 200]), pcm(2, 44100, &[1, 2, 3, 4])]).unwrap();
        assert_eq!(mix.channels, 2);
        assert_eq!(mix.samples, vec![101, 102, 203, 204]);
        // the order of the layers does not matter
        assert_eq!(mix_layers(&[pcm(2, 44100, &[1, 2, 3, 4]), pcm(1, 44100, &[100, 200])]).unwrap().samples, mix.samples);
        // more than two channels keep the first two
        let six = mix_layers(&[pcm(6, 44100, &[1, 2, 3, 4, 5, 6])]).unwrap();
        assert_eq!((six.channels, six.samples), (2, vec![1, 2]));
    }

    #[test]
    fn the_mix_is_as_long_as_the_longest_layer() {
        let mix = mix_layers(&[pcm(1, 44100, &[10, 10]), pcm(1, 44100, &[1, 1, 1, 1, 1])]).unwrap();
        assert_eq!(mix.samples, vec![11, 11, 1, 1, 1]);
        // layers that carry no whole frame, or no rate, count as empty
        let mix = mix_layers(&[pcm(2, 44100, &[7]), pcm(1, 0, &[9, 9]), pcm(1, 44100, &[4])]).unwrap();
        assert_eq!(mix.samples, vec![4]);
    }

    #[test]
    fn a_slower_layer_is_brought_to_the_highest_rate() {
        // 100 Hz [0, 100] has two frames; at 200 Hz it has four, interpolated linearly (the last value is held)
        let mix = mix_layers(&[pcm(1, 100, &[0, 100]), pcm(1, 200, &[0, 0, 0, 0, 0])]).unwrap();
        assert_eq!(mix.sample_rate, 200);
        assert_eq!(mix.samples, vec![0, 50, 100, 100, 0]);
        // and the other way round, with stereo
        let mix = mix_layers(&[pcm(2, 200, &[10, -10]), pcm(2, 100, &[0, 0, 100, -100])]).unwrap();
        assert_eq!(mix.sample_rate, 200);
        assert_eq!(mix.samples, vec![10, -10, 50, -50, 100, -100, 100, -100]);
        assert!((mix.seconds() - 0.02).abs() < 1e-9, "the length in seconds is kept");
    }

    #[test]
    fn a_loud_mix_is_turned_down_as_a_whole_and_a_quiet_one_is_not_turned_up() {
        let mix = mix_layers(&[pcm(1, 44100, &[20000, -20000, 10000]), pcm(1, 44100, &[20000, -10000, 0])]).unwrap();
        // 0.95 of full scale is 31128.65: the loudest sample lands on it and the others keep their ratios
        assert_eq!(mix.peak(), 31129);
        assert_eq!(mix.samples[0], 31129);
        assert!((mix.samples[1] as f64 / mix.samples[0] as f64 + 30000.0 / 40000.0).abs() < 1e-3, "{:?}", mix.samples);
        assert!((mix.samples[2] as f64 / mix.samples[0] as f64 - 10000.0 / 40000.0).abs() < 1e-3, "{:?}", mix.samples);
        // a negative peak counts, and so does the most negative sample there is
        assert_eq!(mix_layers(&[pcm(1, 44100, &[i16::MIN, 100])]).unwrap().samples, vec![-31129, 95]);
        // quiet stays quiet, and anything at or under the ceiling is untouched
        assert_eq!(mix_layers(&[pcm(1, 44100, &[100, -50])]).unwrap().samples, vec![100, -50]);
        assert_eq!(mix_layers(&[pcm(1, 44100, &[31000, -31000])]).unwrap().samples, vec![31000, -31000]);
    }

    #[test]
    fn a_long_mix_is_cut_at_eight_seconds_with_a_short_fade() {
        // 1000 Hz: 8 s are 8000 frames and the 20 ms fade is 20 frames
        let long = pcm(1, 1000, &vec![1000; 9000]);
        let mix = mix_layers(&[long, pcm(1, 1000, &[1; 10])]).unwrap();
        assert_eq!(mix.samples.len(), 8000);
        assert_eq!(mix.samples[0], 1001, "the layers are still added");
        assert_eq!(mix.samples[7979], 1000);
        assert_eq!(mix.samples[7980], 950);
        assert_eq!(mix.samples[7990], 450);
        assert_eq!(mix.samples[7999], 0, "the cut ends in silence");
        assert!((mix.seconds() - 8.0).abs() < 1e-9);

        // exactly eight seconds is not cut, and has no fade
        let exact = mix_layers(&[pcm(1, 1000, &vec![1000; 8000])]).unwrap();
        assert_eq!((exact.samples.len(), exact.samples[7999]), (8000, 1000));

        // stereo is cut and faded on both sides; the fade is measured at the real rate (44.1 kHz: 882 frames)
        let st = mix_layers(&[pcm(2, 44100, &vec![2000; 44100 * 2 * 9])]).unwrap();
        assert_eq!(st.samples.len(), 44100 * 8 * 2);
        assert_eq!(&st.samples[st.samples.len() - 2..], &[0, 0]);
        assert_eq!(st.samples[(44100 * 8 - 883) * 2], 2000);
        assert!(st.samples[(44100 * 8 - 882) * 2] < 2000);
    }

    #[test]
    fn the_cut_comes_before_the_loudness_is_measured() {
        // the loud part lies after the cut, so the part that is kept must not be turned down because of it
        let mut loud_tail = vec![1000i16; 8000];
        loud_tail.extend(vec![30000i16; 1000]);
        let mix = mix_layers(&[pcm(1, 1000, &loud_tail), pcm(1, 1000, &vec![2000i16; 9000])]).unwrap();
        assert_eq!(mix.samples.len(), 8000);
        assert_eq!((mix.samples[0], mix.peak()), (3000, 3000));
    }

    #[test]
    fn nothing_to_mix_gives_nothing() {
        assert!(mix_layers(&[]).is_none());
        assert!(mix_layers(&[pcm(1, 44100, &[])]).is_none());
        assert!(mix_layers(&[pcm(2, 44100, &[]), pcm(1, 0, &[1, 2, 3])]).is_none());
    }
}
