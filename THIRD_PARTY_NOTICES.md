# Third-party notices

Ashen Marine bundles or builds on the following open-source software. Their licenses permit redistribution
with the notices below.

| Component | Used for | License | Source |
|-----------|----------|---------|--------|
| ModEngine2 2.1.0 | loads the mashup into Dark Souls III (installed by Melty; bundled only in private test kits) | MIT | https://github.com/soulsmods/ModEngine2 |
| ScyllaHide (inside ModEngine2) | part of ModEngine2's release | see ModEngine2's release | https://github.com/x64dbg/ScyllaHide |
| MinHook (C library, compiled into the hook DLL) | inline function hooking | BSD-2-Clause | https://github.com/TsudaKageyu/minhook |
| Hacker Disassembler Engine (inside MinHook) | instruction length decoding | BSD-2-Clause | https://github.com/TsudaKageyu/minhook |
| `minhook` Rust crate | Rust bindings for MinHook | MIT | https://crates.io/crates/minhook |
| `windows-sys`, `windows-link` | Windows API bindings | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `serde`, `serde_json`, `sha2`, `anyhow`, `tracing`, `log`, `cc` and their small dependencies | configuration, hashing, logging, build | MIT OR Apache-2.0 (tracing: MIT) | https://crates.io |
| `darksouls3`, `fromsoftware-shared` (fromsoftware-rs) | typed Dark Souls III structures, parameter tables and game-function addresses (read-only probe in test kits) | MIT OR Apache-2.0 | https://github.com/vswarte/fromsoftware-rs |
| `pelite`, `memchr`, `bitfield`, `vtable-rs`, `windows` | PE/version-resource reading, memory search, support code of the crates above | MIT OR Apache-2.0 (memchr: MIT OR Unlicense) | https://crates.io |
| `zip`, `png`, `serde_yaml_ng` | reading Space Marine 2's archives, writing preview pictures, reading texture descriptors | MIT OR Apache-2.0 (zip: MIT) | https://crates.io |
| `bcdec_rs` | decoding block-compressed (BC1-BC7) textures to preview pictures; a Rust port of bcdec | MIT OR Unlicense (see its repository) | https://github.com/iOrange/bcdec |
| `lewton` | decoding the Vorbis audio of Space Marine 2 sound files on the player's PC | MIT OR Apache-2.0 | https://github.com/RustAudio/lewton |
| rewwise (format description only; no code copied) | the layout of Wwise sound-bank objects read by crates/sm2/src/hirc.rs | MIT OR Apache-2.0 | https://github.com/vswarte/rewwise |
| ww2ogg (algorithm ported to Rust) and its `packed_codebooks_aoTuV_603.bin` codebook library | rebuilding Wwise's stripped Vorbis streams into standard Vorbis (crates/sm2/src/wwvorbis.rs, crates/sm2/data) | BSD-3-Clause (Xiph.org Foundation; Adam Gashlin); codebooks derived from aoTuV/libvorbis (BSD-3-Clause) | https://github.com/hcs64/ww2ogg |

## MinHook

```
MinHook - The Minimalistic API Hooking Library for x64/x86
Copyright (C) 2009-2017 Tsuda Kageyu.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

 1. Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.
 2. Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Portions of this software are Copyright (c) 2008-2009, Vyacheslav Patkov
(Hacker Disassembler Engine), under the same BSD-2-Clause terms.
```

## minhook (Rust crate)

```
MIT License
Copyright (c) 2025 Jakobzs
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software ...
The above copyright notice and this permission notice shall be included in all copies or substantial portions
of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND ...
```

## ModEngine2

ModEngine2 is distributed under the MIT license (see `LICENSE-MIT` in the ModEngine2 repository, shipped with
private test kits as `modengine2/LICENSE-MIT.txt`).

## Format references (knowledge only, no code copied unless stated)

- Space Marine 2 texture format: [vash2pid/texmipper](https://github.com/vash2pid/texmipper) (MIT).
- Space Marine 2 model/serialization format: [Wildenhaus/LibSaber](https://github.com/Wildenhaus/LibSaber) and
  [Wildenhaus/IndexV2](https://github.com/Wildenhaus/IndexV2) (no license file: used only to understand the format; our parser is written independently),
  and the ResHax community thread on SM2 `.tpl` models.
- Saber's official Space Marine 2 modding documentation (spacemarine2-modding.prismray.io).
- Wwise sound banks: [vswarte/rewwise](https://github.com/vswarte/rewwise) (MIT OR Apache-2.0), vgmstream (ISC).
- Dark Souls III in-process structures: [vswarte/fromsoftware-rs](https://github.com/vswarte/fromsoftware-rs) `darksouls3` crate (MIT).

## Not included

No Dark Souls III or Space Marine 2 file is included in this project or in any release. Dark Souls III is
(c) FromSoftware / Bandai Namco. Warhammer 40,000: Space Marine 2 is (c) Games Workshop / Saber Interactive /
Focus Entertainment. This is an unofficial fan project.

## ww2ogg and the packed codebook library

```
Copyright (c) 2002, Xiph.org Foundation
Copyright (c) 2009-2016, Adam Gashlin

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

- Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

- Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

- Neither the name of the Xiph.org Foundation nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
