//! The model swap for one weapon container (`wp_a_XXXX.partsbnd.dcx` once its DCX is taken off): a BND4 that holds the
//! model (`.flver`), its textures (`.tpf`) and files for physics and animation. The model gets a new shape
//! ([`crate::modelswap`]), the texture of the colour map gets a new picture and the other maps the material uses become
//! plain colours (the average of the map they replace, so the shading stays in the same encoding); every other file of the
//! container is carried over byte for byte.
//!
//! A weapon with a scabbard has a second model in its container (the Shortsword's is `WP_A_0200.flver` and
//! `WP_A_0200_1.flver`; the left-hand version has `_L` on both). The weapon is the model named like the texture file; the
//! other one is shrunk to one tiny triangle (the Space Marine 2 chainsword has no scabbard), or left alone if that cannot be
//! done as safely as everything else here.
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

fn leaf(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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

/// The file name of an entry without its folders and extension, in lower case (`...\WP_A_0200_1.flver` -> `wp_a_0200_1`).
fn lower_stem(bnd: &Bnd4, index: usize) -> String {
    leaf_stem(bnd.files.get(index).and_then(|f| f.name.as_deref()).unwrap_or("")).to_ascii_lowercase()
}

/// The entry that holds the weapon itself and the entries of the other models. With one model that is the weapon; with
/// several it is the one named like the texture file (`WP_A_0200.tpf` -> `WP_A_0200.flver`), and when no model is named
/// like it the container is refused (it cannot be told which one is held in the hand).
pub fn host_model(bnd: &Bnd4) -> Result<(usize, Vec<usize>), SwapError> {
    let models: Vec<usize> = bnd.files.iter().filter(|f| f.name.as_deref().is_some_and(|n| n.to_ascii_lowercase().ends_with(".flver"))).map(|f| f.index).collect();
    match models.len() {
        0 => cannot("the container has no .flver model"),
        1 => Ok((models[0], Vec::new())),
        n => {
            let textures = file_index(bnd, ".tpf")?;
            let named: Vec<usize> = textures.map(|t| lower_stem(bnd, t)).map(|stem| models.iter().copied().filter(|m| lower_stem(bnd, *m) == stem).collect()).unwrap_or_default();
            match named[..] {
                [host] => Ok((host, models.into_iter().filter(|m| *m != host).collect())),
                _ => cannot(format!("the container has {n} .flver files and none of them is named like the texture file, so it cannot be told which one is the weapon")),
            }
        }
    }
}

/// A shape of one tiny triangle: what a model that is to be invisible gets (no model would also be an option, but a game that
/// loads a model it was told about would then meet a file it does not expect).
fn tiny_shape() -> NewShape {
    NewShape { positions: vec![[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.001, 0.0]], normals: vec![[0.0, 0.0, 1.0]; 3], tangents: Vec::new(), uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], triangles: vec![[0, 1, 2]] }
}

/// The scabbard (or any other model beside the weapon) as one tiny triangle; `Err` says in plain words why it was left alone.
fn shrink_model(container: &[u8], bnd: &Bnd4, index: usize) -> Result<Vec<u8>, String> {
    let bytes = bnd.file_bytes(container, index).ok_or("it lies outside the container")?;
    let model = Flver::parse(bytes).map_err(|e| format!("it cannot be read: {e}"))?;
    match model.write() {
        Ok(again) if again == bytes => {}
        Ok(_) => return Err("it written again differs from the game's file".to_string()),
        Err(e) => return Err(format!("it cannot be written again: {e}")),
    }
    let outcome = modelswap::replace_geometry(&model, tiny_shape()).map_err(|e| e.to_string())?;
    let new_model = outcome.model.ok_or("no model came out")?;
    let new_bytes = new_model.write().map_err(|e| format!("the new model cannot be written: {e}"))?;
    Flver::parse(&new_bytes).map_err(|e| format!("the new model cannot be read: {e}"))?;
    Ok(new_bytes)
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
    let (model_at, other_models) = host_model(&bnd)?;
    if !other_models.is_empty() {
        let names: Vec<String> = other_models.iter().map(|i| bnd.files[*i].name.as_deref().map_or("?", leaf).to_string()).collect();
        lines.push(format!("the container has {} models: the weapon is {} (named like the texture file); the other{} {} ({})", other_models.len() + 1, bnd.files[model_at].name.as_deref().map_or("?", leaf), if names.len() == 1 { "" } else { "s" }, names.join(", "), if names.len() == 1 { "the scabbard" } else { "the scabbards" }));
    }
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
    for &other in &other_models {
        let name = bnd.files[other].name.as_deref().map_or("?", leaf).to_string();
        match shrink_model(container, &bnd, other) {
            Ok(bytes) => {
                lines.push(format!("{name}: shrunk to one tiny triangle (the Space Marine 2 weapon has no scabbard); its textures stay as they are"));
                parts.push((other, bytes));
            }
            Err(why) => lines.push(format!("{name}: left as it is ({why})")),
        }
    }
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

    fn container_with_a_scabbard(scabbard_first: bool, scabbard_bytes: &[u8]) -> Vec<u8> {
        let base = "N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\wp_a_9999\\";
        let mut spec = Bnd4Spec::new(0x74);
        if scabbard_first {
            spec = spec.file(201, &format!("{base}WP_A_9999_1.flver"), scabbard_bytes);
        }
        spec = spec.file(100, &format!("{base}WP_A_9999.tpf"), &sample_tpf().write()).file(200, &format!("{base}WP_A_9999.flver"), &sample_flver().write().unwrap());
        if !scabbard_first {
            spec = spec.file(201, &format!("{base}WP_A_9999_1.flver"), scabbard_bytes);
        }
        spec.file(800, &format!("{base}WP_A_9999_Collidable.hkx"), &[9u8; 64]).build()
    }

    #[test]
    fn the_weapon_is_the_model_named_like_the_texture_file_and_the_scabbard_beside_it_shrinks_to_a_dot() {
        for scabbard_first in [false, true] {
            let scabbard = sample_flver().write().unwrap();
            let original = container_with_a_scabbard(scabbard_first, &scabbard);
            let before = Bnd4::parse(&original).unwrap();
            let by_name = |b: &Bnd4, name: &str| b.files.iter().find(|f| f.name.as_deref().is_some_and(|n| n.ends_with(name))).unwrap().index;
            let result = swap_container(&original, square(), Some(&picture())).unwrap_or_else(|e| panic!("{e}"));
            let after = Bnd4::parse(&result.container).unwrap();
            assert_eq!(after.files.len(), 4);
            let weapon = Flver::parse(after.file_bytes(&result.container, by_name(&after, "WP_A_9999.flver")).unwrap()).unwrap();
            assert_eq!(weapon.meshes[0].vertex_buffers[0].vertex_count, 4, "the weapon got the new shape (order {scabbard_first})");
            let sheath = Flver::parse(after.file_bytes(&result.container, by_name(&after, "WP_A_9999_1.flver")).unwrap()).unwrap();
            assert_eq!((sheath.meshes.len(), sheath.meshes[0].vertex_buffers[0].vertex_count), (1, 3), "the scabbard is one tiny triangle");
            let (lo, hi) = sheath.meshes[0].bounding_box.as_ref().map(|b| (b.min, b.max)).unwrap();
            assert!(hi[0] - lo[0] <= 0.0011 && hi[1] - lo[1] <= 0.0011, "{lo:?} {hi:?}");
            // everything else is carried over: the physics file, and the textures were swapped once for the weapon's material only
            let hkx = by_name(&after, "Collidable.hkx");
            assert_eq!(after.file_bytes(&result.container, hkx), before.file_bytes(&original, by_name(&before, "Collidable.hkx")));
            let text = result.lines.join("\n");
            assert!(text.contains("the container has 2 models: the weapon is WP_A_9999.flver (named like the texture file); the other WP_A_9999_1.flver (the scabbard)"), "{text}");
            assert!(text.contains("WP_A_9999_1.flver: shrunk to one tiny triangle"), "{text}");
            assert!(text.contains("check: the new container has the same 4 files"), "{text}");
        }
    }

    #[test]
    fn a_scabbard_that_cannot_be_shrunk_safely_is_left_alone_and_the_weapon_is_still_swapped() {
        let junk = vec![0x5Au8; 300];
        let original = container_with_a_scabbard(false, &junk);
        let result = swap_container(&original, square(), None).unwrap_or_else(|e| panic!("{e}"));
        let before = Bnd4::parse(&original).unwrap();
        let after = Bnd4::parse(&result.container).unwrap();
        let at = |b: &Bnd4| b.files.iter().find(|f| f.name.as_deref().is_some_and(|n| n.ends_with("_1.flver"))).unwrap().index;
        assert_eq!(after.file_bytes(&result.container, at(&after)), before.file_bytes(&original, at(&before)), "the scabbard is byte for byte what it was");
        let text = result.lines.join("\n");
        assert!(text.contains("WP_A_9999_1.flver: left as it is (it cannot be read"), "{text}");
    }

    #[test]
    fn two_models_and_no_texture_file_to_tell_them_apart_is_refused_and_so_is_a_name_that_fits_neither() {
        let model = sample_flver().write().unwrap();
        let none = Bnd4Spec::new(0x74).file(1, "x\\a.flver", &model).file(2, "x\\b.flver", &model).file(3, "x\\other.tpf", &sample_tpf().write()).build();
        assert!(matches!(swap_container(&none, square(), None), Err(SwapError::CannotUse(m)) if m.contains("2 .flver files and none of them is named like the texture file")));
        // the texture file's name decides, whatever the case
        let upper = Bnd4Spec::new(0x74).file(1, "x\\A_1.flver", &model).file(2, "x\\A.flver", &model).file(3, "x\\a.TPF", &sample_tpf().write()).build();
        let result = swap_container(&upper, square(), None).unwrap_or_else(|e| panic!("{e}"));
        assert!(result.lines.iter().any(|l| l.contains("the weapon is A.flver")), "{:?}", result.lines);
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
