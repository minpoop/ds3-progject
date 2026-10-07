//! Checks the Wwise Vorbis converter against real Space Marine 2 `.wem` files, which are never part of this repository.
//! Set `ASHEN_SM2_WEM_DIR` to a folder of `.wem` files (the "sound samples" a player's own PC produced) to run it.
//! Optionally set `ASHEN_SM2_REF_OGG_DIR` to a folder of `<same name>.ogg` files made by the reference tool (ww2ogg with
//! the aoTuV 603 codebooks): the rebuilt setup header and every audio packet must then be bit-identical.
use ashen_sm2::wem;
use ashen_sm2::wwvorbis;
use std::path::{Path, PathBuf};

fn wems(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "wem")).collect();
    v.sort();
    v
}

/// Split an Ogg file into its packets (handles packets that continue over page boundaries).
fn ogg_packets(b: &[u8]) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut p = 0;
    while p + 27 <= b.len() {
        assert_eq!(&b[p..p + 4], b"OggS", "page at {p}");
        let nseg = b[p + 26] as usize;
        let table = &b[p + 27..p + 27 + nseg];
        let mut at = p + 27 + nseg;
        for &seg in table {
            cur.extend_from_slice(&b[at..at + seg as usize]);
            at += seg as usize;
            if seg < 255 {
                packets.push(std::mem::take(&mut cur));
            }
        }
        p = at;
    }
    packets
}

#[test]
fn real_wems_convert_and_match_the_reference_tool() {
    let Some(dir) = std::env::var_os("ASHEN_SM2_WEM_DIR") else {
        eprintln!("ASHEN_SM2_WEM_DIR not set: skipping (needs real Space Marine 2 sound files)");
        return;
    };
    let refdir = std::env::var_os("ASHEN_SM2_REF_OGG_DIR").map(PathBuf::from);
    let files = wems(Path::new(&dir));
    assert!(!files.is_empty(), "no .wem files in {dir:?}");
    let mut compared = 0;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(f).unwrap();
        let stream = wwvorbis::to_vorbis_packets(&bytes).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let pcm = wwvorbis::decode(&bytes).unwrap_or_else(|e| panic!("{name}: decode: {e:#}"));
        assert_eq!(pcm.samples.len(), stream.info.sample_count as usize * stream.info.channels as usize, "{name}: sample count");
        assert!(pcm.peak() > 100, "{name}: silent output");
        eprintln!(
            "{name:70} {} ch {} Hz {:6.3} s  peak {:5}  packets {}",
            pcm.channels,
            pcm.sample_rate,
            pcm.seconds(),
            pcm.peak(),
            stream.audio.len()
        );
        if let Some(rd) = &refdir {
            let ogg = rd.join(name.replace(".wem", ".ogg"));
            if !ogg.exists() {
                continue;
            }
            let packets = ogg_packets(&std::fs::read(&ogg).unwrap());
            assert!(packets.len() >= 3 + stream.audio.len(), "{name}: reference has {} packets, mine {}", packets.len(), 3 + stream.audio.len());
            assert_eq!(packets[0][..7], stream.ident[..7], "{name}: identification header start");
            // channels, rate and block sizes of the identification header
            assert_eq!(packets[0][11..16], stream.ident[11..16], "{name}: channels/rate");
            assert_eq!(packets[0][28], stream.ident[28], "{name}: block sizes");
            assert_eq!(packets[2], stream.setup, "{name}: setup header differs from the reference");
            for (i, a) in stream.audio.iter().enumerate() {
                assert_eq!(&packets[3 + i], a, "{name}: audio packet {i} differs from the reference");
            }
            compared += 1;
        }
    }
    if refdir.is_some() {
        assert!(compared > 0, "reference folder given but no matching .ogg names");
        eprintln!("{compared} files bit-identical to the reference tool");
    }
    let _ = wem::codec_name(0xFFFF);
}
