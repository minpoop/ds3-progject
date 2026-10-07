//! `wemtool <in.wem> <out.wav>`: convert one Space Marine 2 sound file (a copy the player made) to a plain `.wav`.
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        anyhow::bail!("usage: wemtool <in.wem> <out.wav>");
    };
    let bytes = std::fs::read(input).with_context(|| format!("reading {input}"))?;
    let pcm = ashen_sm2::wem::decode(&bytes)?;
    std::fs::write(output, ashen_sm2::wem::wav_bytes(&pcm)).with_context(|| format!("writing {output}"))?;
    println!("{output}: {} ch, {} Hz, {:.3} s, peak {}", pcm.channels, pcm.sample_rate, pcm.seconds(), pcm.peak());
    Ok(())
}
