//! Sound output for the mashup's own sounds: a small mixer thread feeding the Windows waveOut device. It never
//! touches the game's own audio engine, so it cannot disturb it; if no device can be opened it says so and stays off.
use ashen_common::{
    logging::Logger,
    mixer::{Clip, Mixer, RATE},
};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;
use windows_sys::Win32::Media::Audio::{waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite, CALLBACK_NULL, HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE};

const BUFFERS: usize = 4;
const BUFFER_FRAMES: usize = (RATE as usize) / 50; // 20 ms

pub(super) enum Msg {
    Play(Arc<Clip>, f32),
    Loop(u32, Arc<Clip>, f32),
    Stop(u32),
}

/// Handle to the mixer thread. Cheap to clone and call from any thread.
#[derive(Clone)]
pub struct Audio {
    tx: Sender<Msg>,
}

impl Audio {
    pub fn play(&self, clip: Arc<Clip>, gain: f32) {
        let _ = self.tx.send(Msg::Play(clip, gain));
    }

    /// Play `clip` over and over until [`Audio::stop`] is called with the same key.
    pub fn play_loop(&self, key: u32, clip: Arc<Clip>, gain: f32) {
        let _ = self.tx.send(Msg::Loop(key, clip, gain));
    }

    pub fn stop(&self, key: u32) {
        let _ = self.tx.send(Msg::Stop(key));
    }

    /// A handle that is not connected to any sound device: the messages can be read from the returned receiver.
    #[cfg(test)]
    pub(super) fn test_pair() -> (Audio, Receiver<Msg>) {
        let (tx, rx) = channel();
        (Audio { tx }, rx)
    }
}

/// Open the default output device and start mixing. `None` (with the reason logged) if that is not possible.
pub fn start(log: Arc<Logger>) -> Option<Audio> {
    let (tx, rx) = channel();
    let (ready_tx, ready_rx) = channel::<Result<(), String>>();
    let log2 = log.clone();
    let spawned = std::thread::Builder::new().name("ashen-audio".into()).spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(rx, ready_tx, log2.clone())));
        if r.is_err() {
            log2.log("audio: the mixer thread crashed; sounds are off");
        }
    });
    if spawned.is_err() {
        log.log("audio: could not start the mixer thread");
        return None;
    }
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Some(Audio { tx }),
        Ok(Err(why)) => {
            log.log(&format!("audio: off ({why})"));
            None
        }
        Err(_) => {
            log.log("audio: the output device did not answer within 5 s; sounds are off");
            None
        }
    }
}

fn run(rx: Receiver<Msg>, ready: Sender<Result<(), String>>, log: Arc<Logger>) {
    unsafe {
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut dev: HWAVEOUT = core::ptr::null_mut();
        let rc = waveOutOpen(&mut dev, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL);
        if rc != 0 {
            let _ = ready.send(Err(format!("waveOutOpen failed with code {rc}")));
            return;
        }
        let mut bufs: Vec<Vec<i16>> = (0..BUFFERS).map(|_| vec![0i16; BUFFER_FRAMES * 2]).collect();
        let mut hdrs: Vec<WAVEHDR> = Vec::with_capacity(BUFFERS);
        for b in bufs.iter_mut() {
            let mut h: WAVEHDR = core::mem::zeroed();
            h.lpData = b.as_mut_ptr() as *mut u8;
            h.dwBufferLength = (b.len() * 2) as u32;
            let rc = waveOutPrepareHeader(dev, &mut h, core::mem::size_of::<WAVEHDR>() as u32);
            if rc != 0 {
                let _ = ready.send(Err(format!("waveOutPrepareHeader failed with code {rc}")));
                waveOutClose(dev);
                return;
            }
            h.dwFlags |= WHDR_DONE; // free to fill
            hdrs.push(h);
        }
        log.log("audio: output device opened (44.1 kHz stereo, 4 x 20 ms buffers)");
        let _ = ready.send(Ok(()));

        let mut mixer = Mixer::new();
        'run: loop {
            loop {
                match rx.try_recv() {
                    Ok(Msg::Play(clip, gain)) => mixer.play(clip, gain),
                    Ok(Msg::Loop(key, clip, gain)) => mixer.play_loop(key, clip, gain),
                    Ok(Msg::Stop(key)) => mixer.stop(key),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break 'run,
                }
            }
            let mut wrote = false;
            for i in 0..BUFFERS {
                if hdrs[i].dwFlags & WHDR_DONE != 0 {
                    mixer.mix(&mut bufs[i]);
                    hdrs[i].dwFlags &= !WHDR_DONE;
                    let rc = waveOutWrite(dev, &mut hdrs[i], core::mem::size_of::<WAVEHDR>() as u32);
                    if rc != 0 {
                        log.log(&format!("audio: waveOutWrite failed with code {rc}; sounds are off"));
                        break 'run;
                    }
                    wrote = true;
                }
            }
            if !wrote {
                std::thread::sleep(Duration::from_millis(4));
            }
        }
        waveOutReset(dev);
        for h in hdrs.iter_mut() {
            waveOutUnprepareHeader(dev, h, core::mem::size_of::<WAVEHDR>() as u32);
        }
        waveOutClose(dev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn starting_never_hangs_or_crashes_with_or_without_a_device() {
        let dir = std::env::temp_dir().join(format!("ashen-audio-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Arc::new(Logger::open(&dir.join("log.txt"), "t"));
        let t0 = Instant::now();
        let a = start(log.clone());
        assert!(t0.elapsed() < Duration::from_secs(8), "start() must answer within its own timeout");
        match a {
            Some(a) => {
                a.play(Arc::new(Clip::tone(440.0, 60, 0.05)), 1.0);
                std::thread::sleep(Duration::from_millis(300));
            }
            None => {
                let text = std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default();
                assert!(text.contains("audio:"), "a failure is explained in the log: {text:?}");
            }
        }
    }
}
