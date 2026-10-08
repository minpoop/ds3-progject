//! Looking for geometry in a binary file whose layout is not known yet. Weapon models of Space Marine 2 sit in `.tpl_data`
//! files whose structure is described by the `.tpl` next to them; until that description is understood, these heuristics
//! find the vertex and index buffers by what they look like: a long run of equally spaced records whose first three
//! numbers are small and change smoothly from one record to the next (positions), and a long run of 16- or 32-bit numbers
//! that come in triples of nearby, different values (triangles). Pure functions on byte slices: tested on any OS with
//! made-up data, and used by `ashenmarine-setup sm2-mesh-probe` to write a report about the player's own files.

/// A place in the file that looks like an array of vertex positions.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexRun {
    /// how the first three numbers of each record are stored
    pub kind: PosKind,
    pub offset: usize,
    /// bytes from the start of one record to the start of the next
    pub stride: usize,
    pub count: usize,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// median distance between neighbouring records divided by the size of the box around the run (small = smooth = a mesh)
    pub smoothness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosKind {
    /// three IEEE floats
    F32,
    /// three half floats
    F16,
    /// three signed 16-bit integers (normalised or scaled by something stored elsewhere)
    I16,
}

impl PosKind {
    pub fn name(self) -> &'static str {
        match self {
            PosKind::F32 => "3 x float32",
            PosKind::F16 => "3 x float16",
            PosKind::I16 => "3 x int16",
        }
    }
    fn width(self) -> usize {
        match self {
            PosKind::F32 => 4,
            _ => 2,
        }
    }
}

/// A place that looks like a triangle index list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRun {
    pub offset: usize,
    /// 16 or 32 bits per index
    pub bits: u8,
    pub triangles: usize,
    pub min_index: u32,
    pub max_index: u32,
}

const F32_STRIDES: [usize; 14] = [12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 64];
const SMALL_STRIDES: [usize; 13] = [6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 36, 40, 48];
const MIN_RUN: usize = 48;
const MAX_REPORTED: usize = 12;

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0f32 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let frac = (h & 0x03FF) as f32;
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

fn read3(kind: PosKind, d: &[u8], at: usize) -> Option<[f32; 3]> {
    let w = kind.width();
    let b = d.get(at..at + 3 * w)?;
    Some(match kind {
        PosKind::F32 => [0, 1, 2].map(|i| f32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])),
        PosKind::F16 => [0, 1, 2].map(|i| f16_to_f32(u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]))),
        PosKind::I16 => [0, 1, 2].map(|i| f32::from(i16::from_le_bytes([b[i * 2], b[i * 2 + 1]]))),
    })
}

fn plausible(kind: PosKind, p: &[f32; 3]) -> bool {
    match kind {
        PosKind::F32 | PosKind::F16 => p.iter().all(|v| v.is_finite() && v.abs() <= 1000.0 && (*v == 0.0 || v.abs() >= 1e-6)),
        PosKind::I16 => true,
    }
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The 10th and 90th percentile of `v` (sorted in place).
fn p10_p90(v: &mut [f32]) -> (f32, f32) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f32| v[(((v.len() - 1) as f32) * q).round() as usize];
    (at(0.10), at(0.90))
}

/// Judge a finished run of consecutive records: long enough, spread out in all three directions, and smooth.
fn judge(kind: PosKind, stride: usize, start: usize, pts: &[[f32; 3]]) -> Option<VertexRun> {
    // zero records at either end (two or more in a row) are padding between buffers, not vertices
    let zero = |p: &[f32; 3]| p.iter().all(|v| *v == 0.0);
    let lead = pts.iter().take_while(|p| zero(p)).count();
    let trail = pts.iter().rev().take_while(|p| zero(p)).count();
    if lead + trail >= pts.len() {
        return None;
    }
    let (lead, trail) = (if lead >= 2 { lead } else { 0 }, if trail >= 2 { trail } else { 0 });
    let mut lo = lead;
    let mut hi = pts.len() - trail;
    if hi.saturating_sub(lo) < MIN_RUN {
        return None;
    }
    // the typical step inside the run; records at either end that sit much further from their neighbour than that are
    // stray numbers that happened to look like positions
    let mut steps: Vec<f32> = pts[lo..hi].windows(2).map(|w| dist(&w[0], &w[1])).collect();
    steps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = steps[steps.len() / 2];
    let far = (8.0 * median).max(1e-6);
    while hi - lo > MIN_RUN && dist(&pts[lo], &pts[lo + 1]) > far {
        lo += 1;
    }
    while hi - lo > MIN_RUN && dist(&pts[hi - 1], &pts[hi - 2]) > far {
        hi -= 1;
    }
    let pts_in = &pts[lo..hi];
    if pts_in.len() < MIN_RUN {
        return None;
    }
    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in pts_in {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let diag = dist(&min, &max);
    let floor = if kind == PosKind::I16 { 200.0 } else { 0.02 };
    // a position array spreads in every direction (a constant column is a different kind of data)
    if diag <= floor || (0..3).any(|k| max[k] - min[k] < 1e-3 * diag) {
        return None;
    }
    let mut steps: Vec<f32> = pts_in.windows(2).map(|w| dist(&w[0], &w[1])).collect();
    steps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = steps[steps.len() / 2];
    // how far apart typical positions are, ignoring a few outliers: independent random numbers step about this far, a mesh far less
    let spread: f32 = (0..3)
        .map(|k| {
            let mut axis: Vec<f32> = pts_in.iter().map(|p| p[k]).collect();
            let (a, b) = p10_p90(&mut axis);
            (b - a).powi(2)
        })
        .sum::<f32>()
        .sqrt();
    if spread <= 0.0 || median / spread > 0.15 {
        return None;
    }
    Some(VertexRun { kind, offset: start + lo * stride, stride, count: pts_in.len(), min, max, smoothness: median / spread })
}

/// Maximal runs of records at `stride` (starting at `phase`) that are plausible and no further than `step_limit` from
/// their predecessor.
fn runs_at(kind: PosKind, d: &[u8], stride: usize, phase: usize, step_limit: f32, out: &mut Vec<VertexRun>) {
    let mut start = 0usize;
    let mut pts: Vec<[f32; 3]> = Vec::new();
    let flush = |start: usize, pts: &mut Vec<[f32; 3]>, out: &mut Vec<VertexRun>| {
        if pts.len() >= MIN_RUN {
            // a stretch of identical records is padding or a fill value: cut the run there and judge the pieces
            let mut seg = 0usize;
            let mut i = 0usize;
            while i < pts.len() {
                let mut j = i;
                while j + 1 < pts.len() && pts[j + 1] == pts[i] {
                    j += 1;
                }
                if j - i + 1 >= 4 {
                    if i - seg >= MIN_RUN {
                        out.extend(judge(kind, stride, start + seg * stride, &pts[seg..i]));
                    }
                    seg = j + 1;
                }
                i = j + 1;
            }
            if pts.len() - seg >= MIN_RUN {
                out.extend(judge(kind, stride, start + seg * stride, &pts[seg..]));
            }
        }
        pts.clear();
    };
    let mut at = phase;
    while at + 3 * kind.width() <= d.len() {
        match read3(kind, d, at) {
            Some(p) if plausible(kind, &p) => {
                if pts.last().is_some_and(|prev| dist(prev, &p) > step_limit) {
                    flush(start, &mut pts, out);
                }
                if pts.is_empty() {
                    start = at;
                }
                pts.push(p);
            }
            _ => flush(start, &mut pts, out),
        }
        at += stride;
    }
    flush(start, &mut pts, out);
}

/// Vertex position arrays of one kind, best first: the longest and smoothest runs, overlapping ones dropped.
pub fn find_vertex_runs(d: &[u8], kind: PosKind) -> Vec<VertexRun> {
    let strides: &[usize] = if kind == PosKind::F32 { &F32_STRIDES } else { &SMALL_STRIDES };
    let mut all = Vec::new();
    for &stride in strides {
        let align = kind.width();
        let mut phase = 0;
        while phase < stride {
            if kind == PosKind::I16 {
                // integers have no natural scale; a mesh jumps across its whole width between rows, so only the smoothness
                // check below tells it from other numbers
                runs_at(kind, d, stride, phase, 45000.0, &mut all);
            } else {
                // metres, then centimetres
                runs_at(kind, d, stride, phase, 0.5, &mut all);
                runs_at(kind, d, stride, phase, 25.0, &mut all);
            }
            phase += align;
        }
    }
    // smooth (a mesh) and long first
    let score = |r: &VertexRun| r.count as f32 / (1.0 + 20.0 * r.smoothness);
    all.sort_by(|a, b| score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<VertexRun> = Vec::new();
    for r in all {
        let end = r.offset + r.count * r.stride;
        // the same bytes found again with another step limit or a multiple of the stride: keep only the best
        if kept.iter().any(|k| r.offset < k.offset + k.count * k.stride && k.offset < end) {
            continue;
        }
        kept.push(r);
        if kept.len() >= MAX_REPORTED {
            break;
        }
    }
    kept.sort_by_key(|r| r.offset);
    kept
}

/// Triangle index lists: runs of triples of different, nearby indices.
pub fn find_index_runs(d: &[u8]) -> Vec<IndexRun> {
    let mut out = Vec::new();
    for bits in [16u8, 32] {
        let w = usize::from(bits / 8);
        for phase in (0..w * 3).step_by(w) {
            // a triple is plausible when its three indices differ and lie close together (meshes are ordered for the cache)
            let near: u32 = if bits == 16 { 2048 } else { 1 << 16 };
            let limit: u32 = if bits == 16 { 0xFFFF } else { 1 << 24 };
            let get = |at: usize| -> Option<u32> {
                let b = d.get(at..at + w)?;
                Some(if w == 2 { u32::from(u16::from_le_bytes([b[0], b[1]])) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) })
            };
            let mut start: Option<usize> = None;
            let mut tris = 0usize;
            let (mut lo, mut hi) = (u32::MAX, 0u32);
            let close = |start: &mut Option<usize>, tris: &mut usize, lo: &mut u32, hi: &mut u32, out: &mut Vec<IndexRun>| {
                if let Some(s) = *start {
                    // 90 triangles is about the smallest thing worth looking at; a long run of tiny indices is not a mesh either
                    if *tris >= 90 && *hi >= 30 {
                        out.push(IndexRun { offset: s, bits, triangles: *tris, min_index: *lo, max_index: *hi });
                    }
                }
                *start = None;
                *tris = 0;
                *lo = u32::MAX;
                *hi = 0;
            };
            let mut at = phase;
            while at + 3 * w <= d.len() {
                let (a, b, c) = (get(at).unwrap_or(0), get(at + w).unwrap_or(0), get(at + 2 * w).unwrap_or(0));
                let ok = a != b && b != c && a != c && a < limit && b < limit && c < limit && a.abs_diff(b) <= near && b.abs_diff(c) <= near && a.abs_diff(c) <= near;
                if ok {
                    if start.is_none() {
                        start = Some(at);
                    }
                    tris += 1;
                    lo = lo.min(a).min(b).min(c);
                    hi = hi.max(a).max(b).max(c);
                    at += 3 * w;
                } else {
                    close(&mut start, &mut tris, &mut lo, &mut hi, &mut out);
                    at += w;
                }
            }
            close(&mut start, &mut tris, &mut lo, &mut hi, &mut out);
        }
    }
    out.sort_by(|a, b| b.triangles.cmp(&a.triangles));
    // overlapping finds of the same bytes (other phase / width): keep the longest
    let mut kept: Vec<IndexRun> = Vec::new();
    for r in out {
        let end = r.offset + r.triangles * 3 * usize::from(r.bits / 8);
        if kept.iter().any(|k| r.offset < k.offset + k.triangles * 3 * usize::from(k.bits / 8) && k.offset < end) {
            continue;
        }
        kept.push(r);
        if kept.len() >= MAX_REPORTED {
            break;
        }
    }
    kept.sort_by_key(|r| r.offset);
    kept
}

/// Which vertex runs could belong to an index run: enough records to hold every index it uses.
pub fn pair_up<'a>(indices: &IndexRun, verts: &'a [VertexRun]) -> Vec<&'a VertexRun> {
    verts.iter().filter(|v| v.count as u64 > u64::from(indices.max_index) && (v.count as u64) < u64::from(indices.max_index) * 3 + 64).collect()
}

/// Shannon entropy (bits per byte) of `blocks` equal slices of the data: about 8 means compressed or encrypted, much less
/// means plain structures.
pub fn entropy_blocks(d: &[u8], blocks: usize) -> Vec<f32> {
    if d.is_empty() || blocks == 0 {
        return Vec::new();
    }
    let size = d.len().div_ceil(blocks);
    d.chunks(size)
        .map(|c| {
            let mut counts = [0u32; 256];
            for b in c {
                counts[usize::from(*b)] += 1;
            }
            let n = c.len() as f32;
            -counts.iter().filter(|&&k| k > 0).map(|&k| (k as f32 / n) * (k as f32 / n).log2()).sum::<f32>()
        })
        .collect()
}

/// Printable ASCII strings of at least `min_len` characters with their offsets.
pub fn ascii_strings(d: &[u8], min_len: usize, max: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in d.iter().enumerate() {
        let printable = (0x20..0x7F).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min_len {
                    out.push((s, String::from_utf8_lossy(&d[s..i]).to_string()));
                    if out.len() >= max {
                        return out;
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if d.len() - s >= min_len && out.len() < max {
            out.push((s, String::from_utf8_lossy(&d[s..]).to_string()));
        }
    }
    out
}

/// Classic hex dump lines: offset, 16 bytes in hex, then the printable characters.
pub fn hexdump(d: &[u8], from: usize, len: usize) -> Vec<String> {
    let end = d.len().min(from.saturating_add(len));
    let mut out = Vec::new();
    let mut at = from;
    while at < end {
        let row = &d[at..end.min(at + 16)];
        let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        let text: String = row.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
        out.push(format!("{at:08x}  {:<47}  {text}", hex.join(" ")));
        at += 16;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| (self.next() >> 24) as u8).collect()
        }
    }

    /// A wavy 40 x 40 sheet, one metre wide: positions and triangle indices.
    fn sheet() -> (Vec<[f32; 3]>, Vec<u16>) {
        let n = 40usize;
        let mut v = Vec::new();
        for y in 0..n {
            for x in 0..n {
                v.push([0.01 + x as f32 * 0.025, 0.01 + y as f32 * 0.025, 0.05 * ((x + y) as f32 * 0.4 + 0.3).sin()]);
            }
        }
        let mut idx = Vec::new();
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let a = (y * n + x) as u16;
                let b = a + 1;
                let c = a + n as u16;
                let d = c + 1;
                idx.extend([a, b, c, b, d, c]);
            }
        }
        (v, idx)
    }

    fn file_with(vertex_block: &[u8], index_block: &[u8], seed: u64) -> (Vec<u8>, usize, usize) {
        // buffers in game files start on aligned addresses
        let align = |f: &mut Vec<u8>, rng: &mut Rng| {
            while f.len() % 16 != 0 {
                f.push((rng.next() >> 24) as u8);
            }
        };
        let mut rng = Rng(seed);
        let mut f = rng.bytes(5000);
        f.extend(vec![0u8; 400]);
        align(&mut f, &mut rng);
        let vo = f.len();
        f.extend_from_slice(vertex_block);
        f.extend(rng.bytes(777));
        align(&mut f, &mut rng);
        let io = f.len();
        f.extend_from_slice(index_block);
        f.extend(rng.bytes(3000));
        (f, vo, io)
    }

    #[test]
    fn float_positions_with_other_attributes_between_are_found_with_their_stride() {
        let (v, idx) = sheet();
        let mut block = Vec::new();
        for p in &v {
            for c in p {
                block.extend(c.to_le_bytes());
            }
            block.extend([0x80u8, 0x80, 0xFF, 0x7F]); // a packed normal
            block.extend([0x12u8, 0x34, 0x56, 0x78]); // a packed uv
            block.extend([0u8; 12]); // padding
        }
        let ib: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        let (file, vo, io) = file_with(&block, &ib, 7);
        let runs = find_vertex_runs(&file, PosKind::F32);
        let r = runs.iter().find(|r| r.offset == vo).unwrap_or_else(|| panic!("{runs:?}"));
        assert_eq!((r.stride, r.count), (32, 1600), "{r:?}");
        assert!((r.max[0] - 0.985).abs() < 1e-4 && (r.min[0] - 0.01).abs() < 1e-6);
        assert!(r.smoothness < 0.1, "{}", r.smoothness);
        let ir = find_index_runs(&file);
        let i = ir.iter().find(|i| i.offset == io).unwrap_or_else(|| panic!("{ir:?}"));
        assert_eq!((i.bits, i.triangles, i.max_index), (16, 2 * 39 * 39, 1599));
        assert!(pair_up(i, &runs).iter().any(|p| p.offset == vo), "the index run is matched with the vertex run");
    }

    #[test]
    fn half_float_and_integer_positions_are_found_too() {
        let (v, _) = sheet();
        let to_half = |f: f32| -> u16 {
            // round-to-nearest conversion good enough for the test values
            let bits = f.to_bits();
            let sign = ((bits >> 16) & 0x8000) as u16;
            let exp = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
            let frac = (bits >> 13) & 0x3FF;
            if f == 0.0 {
                sign
            } else if exp <= 0 {
                sign
            } else {
                sign | ((exp as u16) << 10) | frac as u16
            }
        };
        let mut half = Vec::new();
        for p in &v {
            for c in p {
                half.extend(to_half(*c).to_le_bytes());
            }
            half.extend([0x11u8, 0x22]); // a w / padding
        }
        let (file, vo, _) = file_with(&half, &[], 11);
        let runs = find_vertex_runs(&file, PosKind::F16);
        // a few stray numbers beside the block may look like positions as well: the run has to contain the whole block
        let r = runs.iter().find(|r| r.offset <= vo && r.offset + r.count * r.stride >= vo + 1600 * 8).unwrap_or_else(|| panic!("{runs:?}"));
        assert_eq!(r.stride, 8);
        assert!(r.count < 1600 + 120, "{r:?}");

        let mut ints = Vec::new();
        for p in &v {
            for c in p {
                ints.extend(((c * 30000.0) as i16).to_le_bytes());
            }
            ints.extend(0x7FFFi16.to_le_bytes());
        }
        let (file, vo, _) = file_with(&ints, &[], 13);
        let runs = find_vertex_runs(&file, PosKind::I16);
        let r = runs.iter().find(|r| r.offset <= vo && r.offset + r.count * r.stride >= vo + 1600 * 8).unwrap_or_else(|| panic!("{runs:?}"));
        assert_eq!(r.stride, 8);
        assert!(r.count < 1600 + 120, "{r:?}");
    }

    #[test]
    fn indices_of_32_bits_are_found() {
        let (_, idx) = sheet();
        let ib: Vec<u8> = idx.iter().flat_map(|i| u32::from(*i).to_le_bytes()).collect();
        let (file, _, io) = file_with(&[], &ib, 17);
        let ir = find_index_runs(&file);
        let i = ir.iter().find(|i| i.offset == io).unwrap_or_else(|| panic!("{ir:?}"));
        assert_eq!((i.bits, i.triangles), (32, 2 * 39 * 39));
    }

    #[test]
    fn noise_and_padding_are_not_geometry() {
        let mut rng = Rng(99);
        let mut file = rng.bytes(200_000);
        file.extend(vec![0u8; 20_000]);
        file.extend(vec![0xFFu8; 20_000]);
        assert!(find_vertex_runs(&file, PosKind::F32).is_empty(), "{:?}", find_vertex_runs(&file, PosKind::F32));
        assert!(find_index_runs(&file).is_empty(), "{:?}", find_index_runs(&file));
        // small integers in a row (counters, tables) are not positions: they do not span a box
        let counters: Vec<u8> = (0..4000u16).flat_map(|i| (i % 7).to_le_bytes()).collect();
        assert!(find_vertex_runs(&counters, PosKind::I16).is_empty());
    }

    #[test]
    fn entropy_tells_plain_from_packed_data() {
        let mut rng = Rng(5);
        let mut d = vec![0u8; 8192];
        d.extend(rng.bytes(8192));
        let e = entropy_blocks(&d, 2);
        assert!(e[0] < 0.1 && e[1] > 7.9, "{e:?}");
        assert!(entropy_blocks(&[], 4).is_empty());
    }

    #[test]
    fn strings_and_hexdump() {
        let d = b"\x00\x01objGEOM_MNG\x00ab\x00\x00another one\x7f";
        let s = ascii_strings(d, 4, 10);
        assert_eq!(s, vec![(2, "objGEOM_MNG".to_string()), (18, "another one".to_string())]);
        assert_eq!(ascii_strings(d, 4, 1).len(), 1);
        let h = hexdump(d, 0, 20);
        assert_eq!(h.len(), 2);
        assert!(h[0].starts_with("00000000  00 01 6f 62 6a 47 45 4f 4d 5f 4d 4e 47 00 61 62") && h[0].ends_with("..objGEOM_MNG.ab"), "{}", h[0]);
        assert!(hexdump(d, 100, 10).is_empty());
    }

    #[test]
    fn half_float_decoding_matches_known_values() {
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert!(f16_to_f32(0x7C00).is_infinite() && f16_to_f32(0x7E00).is_nan());
        assert!((f16_to_f32(0x3555) - 0.333).abs() < 1e-3);
    }
}
