//! A made-up Dark Souls III weapon model and its texture container, for tests (no game data): two meshes, three bones, one
//! dummy, two materials with textures, one layout.
use crate::flver::{BoundingBox, Dummy, FaceSet, Flver, GxItem, GxList, Header, Layout, LayoutMember, Material, Mesh, Node, Texture, VertexBuffer};
use crate::flver::type_size;
use crate::tpf::{Texture as TpfTexture, Tpf};

pub fn sample_layout() -> Layout {
    // position (3 floats), normal (4 bytes), tangent (4 bytes), bone indices (4 bytes), uv (4 bytes) = 12 + 4 + 4 + 4 + 4
    let mut offset = 0;
    let members = [(0x02u32, 0u32, 0i32), (0x13, 3, 0), (0x13, 6, 0), (0x11, 2, 0), (0x12, 5, 0)]
        .into_iter()
        .map(|(ty, semantic, index)| {
            let m = LayoutMember { unk00: 0, struct_offset: offset, ty, semantic, index };
            offset += type_size(ty).unwrap() as i32;
            m
        })
        .collect();
    Layout { members }
}

/// A made-up weapon: two meshes (a blade of 8 vertices, a handle of 4), three bones, one dummy, two materials with textures.
pub fn sample_flver() -> Flver {
    let layout = sample_layout();
    let size = layout.size().unwrap();
    let vertices = |n: usize, seed: u8| -> Vec<u8> {
        let mut data = Vec::new();
        for i in 0..n {
            for k in 0..3 {
                data.extend(((i * 3 + k) as f32 * 0.25 + f32::from(seed)).to_le_bytes());
            }
            data.extend([127u8, 127, 254, 255, 254, 127, 127, 255, seed, 0, 0, 0, (i * 9) as u8, 3, (i * 5) as u8, 8]);
        }
        assert_eq!(data.len(), n * size);
        data
    };
    let mk_mesh = |material: i32, n: usize, seed: u8, strip: bool| Mesh {
        dynamic: 0,
        material_index: material,
        node_index: 0,
        bone_indices: if material == 0 { vec![0, 1] } else { Vec::new() },
        bounding_box: if material == 0 { Some(BoundingBox { min: [-1.0, -1.0, -1.0], max: [1.0, 2.0, 3.0] }) } else { None },
        face_sets: vec![
            FaceSet { flags: 0, triangle_strip: strip, cull_backfaces: true, unk06: 0, indices: (0..n as u32).collect(), index_bits: 16 },
            FaceSet { flags: 0x0100_0000, triangle_strip: strip, cull_backfaces: false, unk06: 1, indices: (0..n as u32 - 1).collect(), index_bits: 16 },
        ],
        vertex_buffers: vec![VertexBuffer { layout_index: 0, vertex_size: size as i32, vertex_count: n as i32, data: vertices(n, seed) }],
    };
    let tex = |param: &str, path: &str| Texture { param_name: param.to_string(), path: path.to_string(), scale: [1.0, 1.0], tiling_u: 1, tiling_v: 1, unk14: 0.0, unk18: 0.0, unk1c: 0.0 };
    Flver {
        header: Header {
            version: 0x20014,
            bounding_box_min: [-1.0, -1.0, -1.0],
            bounding_box_max: [1.0, 2.0, 3.0],
            unicode: true,
            unk4a: false,
            unk4b: false,
            unk4c: 0,
            unk5c: 0,
            unk5d: 0,
            unk68: 4,
            special_modifier: 0,
            unk74: 0x10,
            face_count: 0,
            total_face_count: 0,
        },
        dummies: vec![Dummy { position: [0.0, 1.0, 2.0], color: [255, 255, 255, 255], forward: [0.0, 0.0, 1.0], reference_id: 100, parent_bone: 1, upward: [0.0, 1.0, 0.0], attach_bone: 1, flag1: true, use_upward_vector: true, unk30: 0, unk34: 0 }],
        materials: vec![
            Material {
                name: "blade".to_string(),
                mtd: "N:\\FDP\\mtd\\Wp\\Wp_Metal[DSB].mtd".to_string(),
                textures: vec![tex("g_Diffuse", "N:\\FDP\\data\\Other\\wp_a_9999_a.tga"), tex("g_Bumpmap", "N:\\FDP\\data\\Other\\wp_a_9999_n.tga")],
                gx_index: 0,
                index: 0,
            },
            Material { name: "handle".to_string(), mtd: "N:\\FDP\\mtd\\Wp\\Wp_Leather[DSB].mtd".to_string(), textures: vec![tex("g_Diffuse", "N:\\FDP\\data\\Other\\wp_a_9999_h_a.tga")], gx_index: -1, index: 1 },
        ],
        gx_lists: vec![GxList { items: vec![GxItem { id: "GXMD".to_string(), unk04: 100, data: vec![1, 2, 3, 4, 5, 6, 7, 8] }], terminator_id: i32::MAX, terminator_length: 4 }],
        nodes: (0..3)
            .map(|i| Node {
                name: format!("bone{i}"),
                position: [0.0, i as f32, 0.0],
                rotation: [0.0; 3],
                scale: [1.0; 3],
                parent: i as i16 - 1,
                child: if i < 2 { i as i16 + 1 } else { -1 },
                next_sibling: -1,
                previous_sibling: -1,
                bounding_box_min: [-1.0; 3],
                unk3c: 0,
                bounding_box_max: [1.0; 3],
            })
            .collect(),
        meshes: vec![mk_mesh(0, 8, 1, true), mk_mesh(1, 4, 2, false)],
        layouts: vec![layout],
    }
}


/// A made-up texture container with two DDS files (headers and zeros).
pub fn sample_tpf() -> Tpf {
    let dds = |four_cc: &[u8; 4], w: u32, h: u32, mips: u32, payload: usize| -> Vec<u8> {
        let mut d = vec![0u8; 128 + payload];
        d[..4].copy_from_slice(b"DDS ");
        d[4..8].copy_from_slice(&124u32.to_le_bytes());
        d[8..12].copy_from_slice(&0x0002_100Fu32.to_le_bytes());
        d[12..16].copy_from_slice(&h.to_le_bytes());
        d[16..20].copy_from_slice(&w.to_le_bytes());
        d[28..32].copy_from_slice(&mips.to_le_bytes());
        d[76..80].copy_from_slice(&32u32.to_le_bytes());
        d[80..84].copy_from_slice(&4u32.to_le_bytes());
        d[84..88].copy_from_slice(four_cc);
        d
    };
    Tpf {
        flag2: 3,
        encoding: 1,
        textures: vec![
            TpfTexture { name: "wp_a_9999_a".to_string(), format: 1, kind: 0, mipmaps: 3, flags1: 0, float_struct: None, bytes: dds(b"DXT5", 64, 64, 3, 5461) },
            TpfTexture { name: "wp_a_9999_n".to_string(), format: 9, kind: 0, mipmaps: 1, flags1: 0, float_struct: Some((0, vec![1.0, 2.5])), bytes: dds(b"DXT1", 16, 16, 1, 129) },
        ],
    }
}
