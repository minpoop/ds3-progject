//! Developer tool: reads a Space Marine 2 `.tpl` file (pass its path) and prints what the reader makes of it.
//!   cargo run -p ashen-sm2 --example tpl_dump -- path/to/wpn_chainsword_01.tpl
use ashen_sm2::tpl::Template;

fn main() {
    let path = std::env::args().nth(1).expect("usage: tpl_dump <file.tpl>");
    let bytes = std::fs::read(&path).expect("read the file");
    println!("{} bytes", bytes.len());
    let (t, error) = Template::parse_partial(&bytes);
    if let Some(e) = &error {
        println!("ERROR: {e}");
    }
    {
        {
            println!("name {:?}  state {:?}  properties {:#x}  header strings {}", t.name, t.state, t.property_bits, t.header_strings.len());
            if let Some(s) = &t.skin {
                println!("skin: {} bones, lod bone counts {:?}, inverse bind matrices {}", s.bone_count, s.lod_bone_counts, s.inverse_bind.as_ref().map_or(0, |m| m.len()));
            }
            println!("{} animation sequences: {}", t.anim_sequences.len(), t.anim_sequences.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "));
            println!("bbox {:?}", t.bbox);
            println!("{} lod definitions: {:?}", t.lods.len(), t.lods.iter().map(|l| (l.object, l.index, l.last_lod_up_to_infinity)).collect::<Vec<_>>());
            println!("{} textures: {:?}", t.textures.len(), t.textures);
            println!("geometry at {:?}, reader stopped at {:#x} of {:#x}", t.geometry_at, t.end_at, bytes.len());
            if let Some(g) = &t.geometry {
                println!("root {} nodes {} buffers {} meshes {} submeshes {}; inline buffers: {}", g.root_node, g.node_count, g.buffer_count, g.mesh_count, g.sub_mesh_count, g.buffers_inline);
                for o in g.objects.iter().take(200) {
                    println!("  object {:3} {:<40} parent {:3} next {:3} child {:3} splits {:?}+{:?}", o.id, o.name.as_deref().unwrap_or("-"), o.parent, o.next, o.child, o.split_index, o.num_splits);
                }
                println!("stream offsets {:?} sizes {:?}", g.stream_offsets, g.stream_sizes);
                for (i, b) in g.buffers.iter().enumerate() {
                    println!("  buffer {i}: stride {} length {} start {} flags {:#x}", b.stride, b.length, b.start, b.flags.low64());
                }
                for (i, m) in g.meshes.iter().enumerate() {
                    println!("  mesh {i}: flags {:#x} buffers {:?}", m.flags.low64(), m.buffers);
                }
                for (i, s) in g.sub_meshes.iter().enumerate() {
                    println!("  submesh {i}: mesh {} verts {}+{} faces {}+{} node {} skin {} bones {} uv {:?} tf {:?}", s.mesh, s.vertex_offset, s.vertex_count, s.face_offset, s.face_count, s.node, s.skin_compound, s.bone_ids.len(), s.uv_scaling, s.transform);
                }
            }
        }
    }
}
