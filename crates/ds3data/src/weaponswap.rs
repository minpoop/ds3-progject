//! The model swap for one weapon container (`wp_a_XXXX.partsbnd.dcx` once its DCX is taken off): a BND4 that holds the
//! model (`.flver`), its textures (`.tpf`) and files for physics and animation. The model gets a new shape
//! ([`crate::modelswap`]), the texture of the colour map gets a new picture and the other maps the material uses become
//! plain colours (the average of the map they replace, so the shading stays in the same encoding); every other file of the
//! container is carried over byte for byte.
//!
//! Fail closed: the writers must reproduce the game's own model and texture files byte for byte before anything is
//! replaced (otherwise there are fields this code does not model), every step reports what it did, and the finished
//! container is read back and checked.
use crate::bnd4::{self, Bnd4};
use crate::dds::{self, Format, Image, Spec};
use crate::flver::Flver;
use crate::modelswap::{self, NewShape, SwapError};
use crate::tpf::{dds_info, Tpf};

/// What a swap made.
#[derive(Debug, Clone, Default)]
pub struct SwapResult {
    /// The new BND4 (to be wrapped in the same DCX the original had).
    pub container: Vec<u8>,
    /// What was done and checked, one fact per line.
    pub lines: Vec<String>,
}

fn cannot<T>(why: impl Into<String>) -> Result<T, SwapError> {
    Err(SwapError::CannotUse(why.into()))
}

fn leaf_stem(path: &str) -> String {
    let leaf = path.rsplit(['\\', '/']).next().unwrap_or(path);
    leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem).to_string()
}

/// The role a shader parameter name stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Colour,
    Other,
}

fn role_of(param: &str) -> Role {
    if param.to_ascii_lowercase().contains("diffuse") {
        Role::Colour
    } else {
        Role::Other
    }
}

fn file_index(bnd: &Bnd4, extension: &str) -> Result<Option<usize>, SwapError> {
    let hits: Vec<usize> = bnd.files.iter().filter(|f| f.name.as_deref().is_some_and(|n| n.to_ascii_lowercase().ends_with(extension))).map(|f| f.index).collect();
    match hits.len() {
        0 => Ok(None),
        1 => Ok(Some(hits[0])),
        n => cannot(format!("the container has {n} {extension} files (one is expected)")),
    }
}

/// The spec a new picture is written with: the one the replaced texture has when it is a format pictures can be made in,
/// else BC3 in the plain old header.
fn picture_spec(original: &Spec) -> Spec {
    match original.format {
        Format::Bc1 | Format::Bc3 => *original,
        _ => Spec { format: Format::Bc3, dxgi: None },
    }
}

/// Swaps the model (and, with `albedo`, the colour map) of a weapon container. `container` is the decoded BND4.
pub fn swap_container(container: &[u8], shape: NewShape, albedo: Option<&Image>) -> Result<SwapResult, SwapError> {
    let mut lines = Vec::new();
    let bnd = Bnd4::parse(container).map_err(|e| SwapError::CannotUse(format!("the container is not a usable BND4: {e}")))?;
    if !bnd.layout.verified {
        return cannot(format!("the container's layout is not the expected one ({})", bnd.layout.issues.join("; ")));
    }
    let Some(model_at) = file_index(&bnd, ".flver")? else { return cannot("the container has no .flver model") };
    let model_bytes = bnd.file_bytes(container, model_at).ok_or_else(|| SwapError::CannotUse("the model lies outside the container".to_string()))?;
    let model = Flver::parse(model_bytes).map_err(|e| SwapError::CannotUse(format!("the model cannot be read: {e}")))?;
    match model.write() {
        Ok(again) if again == model_bytes => lines.push("the model written again is byte-identical to the game's file: the writer is trusted for this file".to_string()),
        Ok(again) => return cannot(format!("the model written again differs from the game's file ({} bytes instead of {}); there are fields not modelled here", again.len(), model_bytes.len())),
        Err(e) => return cannot(format!("the model cannot be written again: {e}")),
    }
    let host = modelswap::biggest_mesh(&model).ok_or_else(|| SwapError::CannotUse("the model has no mesh".to_string()))?;
    let material = usize::try_from(model.meshes[host].material_index).ok().and_then(|i| model.materials.get(i)).cloned();

    let outcome = modelswap::replace_geometry(&model, shape)?;
    lines.extend(outcome.lines.clone());
    let new_model = outcome.model.ok_or_else(|| SwapError::CheckFailed("no model came out".to_string()))?;
    let new_model_bytes = new_model.write().map_err(|e| SwapError::CheckFailed(format!("the new model cannot be written: {e}")))?;

    // the textures the host material names
    let mut parts: Vec<(usize, Vec<u8>)> = vec![(model_at, new_model_bytes)];
    if let (Some(tpf_at), Some(material)) = (file_index(&bnd, ".tpf")?, material) {
        let tpf_bytes = bnd.file_bytes(container, tpf_at).ok_or_else(|| SwapError::CannotUse("the textures lie outside the container".to_string()))?;
        let mut tpf = Tpf::parse(tpf_bytes).map_err(|e| SwapError::CannotUse(format!("the texture container cannot be read: {e}")))?;
        if tpf.write() != tpf_bytes {
            return cannot("the texture container written again differs from the game's file; there are fields not modelled here");
        }
        lines.push("the textures written again are byte-identical to the game's file: the writer is trusted for this file".to_string());
        let mut done: Vec<usize> = Vec::new();
        let mut picture_used = false;
        for slot in &material.textures {
            let stem = leaf_stem(&slot.path);
            let Some(index) = tpf.index_of(&stem) else {
                lines.push(format!("{}: the texture {stem:?} is not in the texture container; left as it is", slot.param_name));
                continue;
            };
            if done.contains(&index) {
                continue;
            }
            done.push(index);
            let original = tpf.textures[index].bytes.clone();
            let Some(info) = dds_info(&original) else {
                lines.push(format!("{}: {stem} is not a DDS file; left as it is", slot.param_name));
                continue;
            };
            let Some(spec) = Spec::from_info(&info) else {
                lines.push(format!("{}: {stem} is stored as {} {}, a format not handled here; left as it is", slot.param_name, if info.four_cc.is_empty() { "an uncompressed format" } else { info.four_cc.as_str() }, info.dxgi_format.map_or(String::new(), |d| format!("(DXGI {d})"))));
                continue;
            };
            let (w, h, levels) = (info.width as usize, info.height as usize, info.mip_levels.max(1) as usize);
            let new_file = match (role_of(&slot.param_name), albedo, picture_used) {
                (Role::Colour, Some(picture), false) => {
                    let target = picture_spec(&spec);
                    let resized = picture.clone().with_alpha(255).resized(w, h).map_err(|e| SwapError::CheckFailed(e.to_string()))?;
                    picture_used = true;
                    lines.push(format!("{}: {stem} gets the new picture, {w}x{h}, {} with {levels} levels (it was {})", slot.param_name, target.format.name(), spec.format.name()));
                    dds::encode_dds(target, &resized, levels).map_err(|e| SwapError::CheckFailed(e.to_string()))?
                }
                _ => {
                    let colour = dds::mean_color(&original).unwrap_or(if spec.format == Format::Bc4 { [128, 0, 0, 255] } else { [128, 128, 255, 255] });
                    lines.push(format!("{}: {stem} becomes one colour {colour:?} (the average of the map it replaces), {w}x{h}, {}", slot.param_name, spec.format.name()));
                    dds::constant_dds(spec, w, h, levels, colour).map_err(|e| SwapError::CheckFailed(e.to_string()))?
                }
            };
            tpf.replace_dds(index, new_file).map_err(|e| SwapError::CheckFailed(e.to_string()))?;
        }
        parts.push((tpf_at, tpf.write()));
    } else {
        lines.push("the container has no texture file (or the mesh has no material): the textures stay as they are".to_string());
    }

    // put the new files in, one after the other (each replacement is checked by the BND4 code)
    let mut current = container.to_vec();
    for (index, data) in &parts {
        let parsed = Bnd4::parse(&current).map_err(|e| SwapError::CheckFailed(format!("the container cannot be read again: {e}")))?;
        current = bnd4::replace_file(&current, &parsed, *index, data).map_err(|e| SwapError::CheckFailed(format!("a file cannot be put into the container: {e}")))?;
    }

    // the check: the result reads back, the files are the new ones and every other file is untouched
    let back = Bnd4::parse(&current).map_err(|e| SwapError::CheckFailed(format!("the new container cannot be read: {e}")))?;
    if back.files.len() != bnd.files.len() {
        return Err(SwapError::CheckFailed("the number of files changed".to_string()));
    }
    for f in &bnd.files {
        let same = back.file_bytes(&current, f.index) == bnd.file_bytes(container, f.index);
        let replaced = parts.iter().any(|(i, _)| *i == f.index);
        if !replaced && !same {
            return Err(SwapError::CheckFailed(format!("file {} ({}) was not to be changed, but it is different now", f.index, f.name.as_deref().unwrap_or("?"))));
        }
    }
    for (index, data) in &parts {
        if back.file_bytes(&current, *index) != Some(data.as_slice()) {
            return Err(SwapError::CheckFailed("a replaced file does not read back as written".to_string()));
        }
    }
    Flver::parse(back.file_bytes(&current, model_at).unwrap_or(&[])).map_err(|e| SwapError::CheckFailed(format!("the new model cannot be read from the new container: {e}")))?;
    lines.push(format!("check: the new container has the same {} files, the replaced ones read back as written and the others are untouched ({} bytes, was {})", back.files.len(), current.len(), container.len()));
    Ok(SwapResult { container: current, lines })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::bnd4::Bnd4Spec;
    use crate::testing::flver::{sample_flver, sample_tpf};

    fn container() -> Vec<u8> {
        Bnd4Spec::new(0x74)
            .file(100, "N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\wp_a_9999\\wp_a_9999.flver", &sample_flver().write().unwrap())
            .file(101, "N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\wp_a_9999\\wp_a_9999.tpf", &sample_tpf().write())
            .file(102, "N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\wp_a_9999\\wp_a_9999.hkx", &[7u8; 120])
            .build()
    }

    fn square() -> NewShape {
        NewShape {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: Vec::new(),
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }
    }

    fn picture() -> Image {
        let mut px = Vec::new();
        for y in 0..32usize {
            for x in 0..32usize {
                px.extend([(x * 8) as u8, (y * 8) as u8, 100, 77]);
            }
        }
        Image::new(32, 32, px).unwrap()
    }

    #[test]
    fn a_container_gets_the_new_shape_the_new_picture_and_keeps_its_other_files() {
        let original = container();
        let result = swap_container(&original, square(), Some(&picture())).unwrap_or_else(|e| panic!("{e}"));
        let before = Bnd4::parse(&original).unwrap();
        let after = Bnd4::parse(&result.container).unwrap();
        assert_eq!(after.files.len(), 3);
        // the physics file is carried over byte for byte
        assert_eq!(after.file_bytes(&result.container, 2), before.file_bytes(&original, 2));
        // the model: one mesh, four vertices
        let model = Flver::parse(after.file_bytes(&result.container, 0).unwrap()).unwrap();
        assert_eq!((model.meshes.len(), model.meshes[0].vertex_buffers[0].vertex_count), (1, 4));
        // the textures: the colour map is a new DXT5 picture of the old size and level count, the normal map one colour
        let tpf = Tpf::parse(after.file_bytes(&result.container, 1).unwrap()).unwrap();
        let colour = dds_info(&tpf.textures[0].bytes).unwrap();
        assert_eq!((colour.width, colour.height, colour.mip_levels, colour.four_cc.as_str()), (64, 64, 3, "DXT5"));
        assert_eq!(tpf.textures[0].mipmaps, 3);
        let normal = dds_info(&tpf.textures[1].bytes).unwrap();
        assert_eq!((normal.width, normal.height, normal.four_cc.as_str()), (16, 16, "DXT1"));
        assert_eq!((tpf.textures[1].format, tpf.textures[1].float_struct.clone()), (9, Some((0, vec![1.0, 2.5]))), "the game's own fields of the entry stay");
        // the picture decodes to something that looks like the one given (alpha is made opaque)
        let first = &tpf.textures[0].bytes[128..];
        let alpha = dds::decode_bc4_block(&first[..8]);
        assert!(alpha.iter().all(|a| *a == 255), "{alpha:?}");
        let text = result.lines.join("\n");
        for needle in ["the model written again is byte-identical", "the textures written again are byte-identical", "g_Diffuse: wp_a_9999_a gets the new picture, 64x64, BC3 (DXT5) with 3 levels", "g_Bumpmap: wp_a_9999_n becomes one colour", "check: the new container has the same 3 files"] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
    }

    #[test]
    fn without_a_picture_every_map_becomes_a_plain_colour() {
        let result = swap_container(&container(), square(), None).unwrap();
        let after = Bnd4::parse(&result.container).unwrap();
        let tpf = Tpf::parse(after.file_bytes(&result.container, 1).unwrap()).unwrap();
        assert!(result.lines.iter().any(|l| l.contains("g_Diffuse: wp_a_9999_a becomes one colour")));
        assert_eq!(dds_info(&tpf.textures[0].bytes).unwrap().mip_levels, 3);
    }

    #[test]
    fn a_container_that_cannot_be_trusted_is_not_touched() {
        // a model with a byte the writer does not reproduce (the header's face counts are recomputed on writing, so a wrong
        // stated count shows up as a difference)
        let mut model_bytes = sample_flver().write().unwrap();
        model_bytes[0x40..0x44].copy_from_slice(&999i32.to_le_bytes());
        let bad = Bnd4Spec::new(0x74).file(100, "x\\wp.flver", &model_bytes).file(101, "x\\wp.tpf", &sample_tpf().write()).build();
        match swap_container(&bad, square(), None) {
            Err(SwapError::CannotUse(m)) => assert!(m.contains("differs from the game's file"), "{m}"),
            other => panic!("{other:?}"),
        }
        // no model at all, or two
        let none = Bnd4Spec::new(0x74).file(1, "x\\a.hkx", &[1u8; 40]).build();
        assert!(matches!(swap_container(&none, square(), None), Err(SwapError::CannotUse(m)) if m.contains("no .flver")));
        let two = Bnd4Spec::new(0x74).file(1, "a.flver", &sample_flver().write().unwrap()).file(2, "b.flver", &sample_flver().write().unwrap()).build();
        assert!(matches!(swap_container(&two, square(), None), Err(SwapError::CannotUse(m)) if m.contains("2 .flver files")));
        // not a container
        assert!(matches!(swap_container(b"nothing", square(), None), Err(SwapError::CannotUse(_))));
        // a bad shape is named
        let mut s = square();
        s.triangles.push([0, 1, 40]);
        assert!(matches!(swap_container(&container(), s, None), Err(SwapError::BadShape(_))));
    }

    #[test]
    fn a_texture_format_that_is_not_handled_is_left_alone_and_said() {
        let mut tpf = sample_tpf();
        tpf.textures[1].bytes[84..88].copy_from_slice(b"XXXX");
        let c = Bnd4Spec::new(0x74).file(100, "x\\wp.flver", &sample_flver().write().unwrap()).file(101, "x\\wp.tpf", &tpf.write()).build();
        let result = swap_container(&c, square(), None).unwrap();
        assert!(result.lines.iter().any(|l| l.contains("a format not handled here; left as it is")), "{:?}", result.lines);
        let after = Bnd4::parse(&result.container).unwrap();
        let out = Tpf::parse(after.file_bytes(&result.container, 1).unwrap()).unwrap();
        assert_eq!(out.textures[1].bytes, tpf.textures[1].bytes);
    }
}
