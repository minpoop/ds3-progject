//! Made-up Space Marine 2 template files for tests (no game data anywhere): a byte writer and a one-square model. Used by this
//! crate's tests and by the tests of the programs that read templates.
use std::default::Default;

/// A small byte writer for building made-up template files in the tests (no game data anywhere).
#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.0.extend(v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend(v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }
    pub fn lps32(&mut self, s: &str) {
        self.i32(s.len() as i32);
        self.0.extend(s.as_bytes());
    }
    /// A flag set: a 16-bit bit count, then the bytes with the given bits on.
    pub fn bits16(&mut self, count: u16, on: &[usize]) {
        self.u16(count);
        let mut bytes = vec![0u8; usize::from(count).div_ceil(8)];
        for b in on {
            bytes[b / 8] |= 1 << (b % 8);
        }
        self.0.extend(bytes);
    }
    /// A chunk: id, the offset at which it ends (patched afterwards), the content.
    pub fn chunk(&mut self, id: u16, body: impl FnOnce(&mut W)) {
        self.u16(id);
        let at = self.0.len();
        self.u32(0);
        body(self);
        let end = self.0.len() as u32;
        self.0[at..at + 4].copy_from_slice(&end.to_le_bytes());
    }
    /// A section that starts with the offset at which it ends.
    pub fn section(&mut self, body: impl FnOnce(&mut W)) {
        let at = self.0.len();
        self.i32(0);
        body(self);
        let end = self.0.len() as i32;
        self.0[at..at + 4].copy_from_slice(&end.to_le_bytes());
    }
}

/// A made-up model: one square (4 vertices, 2 triangles) with a compressed position stream, an interleaved stream with
/// one tangent and one texture coordinate set, a face stream and one material. Returns (.tpl, .tpl_data).
pub fn made_up_template(with_levels: bool) -> (Vec<u8>, Vec<u8>) {
    let mut w = W::default();
    // the 0x40 bytes in front: "1SER", "tpl\0", counters, flags, a 16-character id, three numbers, no strings
    w.0.extend(b"1SERtpl\0");
    w.0.extend([0u8; 24]);
    w.u32(0);
    w.0.extend(b"S3DRESOURCE     ");
    w.i32(0);
    w.i32(0);
    w.i32(0);
    assert_eq!(w.0.len(), 0x40);
    w.0.extend(b"TPL1");
    // properties: name (bit 0), [level of detail definitions (bit 9)], geometry graph (bit 11)
    w.i32(12);
    w.u8(0b0000_0001);
    w.u8(if with_levels { 0b0000_1010 } else { 0b0000_1000 });
    w.lps32("made_up_square");
    if with_levels {
        // one definition: object 0 is detail level 0 (the three columns of the list are all present)
        w.i32(1);
        w.i32(3);
        w.u8(1);
        w.i16(0);
        w.u8(1);
        w.u8(0);
        w.u8(1);
        w.u8(0);
    }
    // the geometry graph: a header word that says 8 properties (so the flags are one byte), a version, no property lists
    w.0.extend(b"OGM1");
    w.u32(8);
    w.u16(1);
    w.u8(0);
    w.chunk(0, |w| {
        w.i16(0);
        w.i32(1); // nodes
        w.i32(4); // buffers
        w.i32(1); // meshes
        w.i32(1); // sub meshes
        w.u32(0);
        w.u32(0);
    });
    // four buffers: vertices, faces, bone numbers (none: absent), interleaved - here vertices, faces, interleaved + one spare
    w.u16(2);
    w.section(|w| {
        w.chunk(0, |w| {
            w.bits16(48, &[0, 3, 10, 11, 45]); // vertices: compressed position, packed normal in the fourth value
            w.bits16(0, &[]); // faces
            w.bits16(48, &[12, 17, 25, 30]); // interleaved: compressed tangent, compressed texture coordinates
            w.bits16(48, &[9]); // bone numbers
        });
        w.chunk(1, |w| {
            for stride in [8u16, 6, 8, 4] {
                w.u16(stride);
            }
        });
        w.chunk(2, |w| {
            for len in [32u32, 12, 32, 16] {
                w.u32(len);
            }
        });
    });
    w.u16(3);
    w.section(|w| {
        w.chunk(0, |w| w.bits16(48, &[3, 9, 30])); // compressed positions, bone numbers, compressed texture coordinates
        w.chunk(2, |w| {
            w.u8(4);
            for (id, off) in [(0i32, 0i32), (1, 0), (2, 0), (3, 0)] {
                w.i32(id);
                w.i32(off);
            }
        });
    });
    w.u16(4);
    w.section(|w| {
        w.chunk(0, |w| {
            for v in [0u16, 4, 0, 2] {
                w.u16(v);
            }
            w.i16(0);
            w.i16(-1);
        });
        w.chunk(1, |w| w.i32(0));
        w.chunk(3, |w| {
            w.u8(2);
            w.i16(7);
            w.i16(9);
        });
        w.chunk(4, |w| {
            w.u8(1);
            w.u8(0);
            w.i16(2);
        });
        w.chunk(5, |w| {
            for v in [0i16, 0, 1, 2, 2, 2] {
                w.i16(v);
            }
        });
        w.chunk(8, |w| {
            w.u16(0); // the node of the sub mesh (the materials chunk is where the node is finally set)
            w.u32(2);
            w.lps32("shadingMtl_Tex");
            w.u32(4);
            w.lps32("square_tex");
            w.lps32("tiling");
            w.u32(2);
            w.f32(1.5);
        });
    });
    w.chunk(0xFFFF, |_| {});
    // the data: vertices (x, y, z, packed normal), faces, interleaved (tangent, uv), bone numbers
    let mut d = W::default();
    for (x, y) in [(-32767i16, -32767i16), (32767, -32767), (32767, 32767), (-32767, 32767)] {
        d.i16(x);
        d.i16(y);
        d.i16(0);
        d.i16(0);
    }
    for t in [[0u16, 1, 2], [0, 2, 3]] {
        for v in t {
            d.u16(v);
        }
    }
    for (u, v) in [(0i16, 0i16), (32767, 0), (32767, 32767), (0, 32767)] {
        d.0.extend([127u8, 0, 0, 127]);
        d.i16(u);
        d.i16(v);
    }
    for b in 0..4u8 {
        d.0.extend([b, 0, 0, 0]);
    }
    (w.0, d.0)
}

/// [`made_up_template`] without level of detail definitions.
pub fn made_up_model() -> (Vec<u8>, Vec<u8>) {
    made_up_template(false)
}

/// [`made_up_template`] where object 0 (the node of the square) is detail level 0, like the weapons' full-detail mesh.
pub fn made_up_weapon() -> (Vec<u8>, Vec<u8>) {
    made_up_template(true)
}
