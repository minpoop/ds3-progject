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

## Not included

No Dark Souls III or Space Marine 2 file is included in this project or in any release. Dark Souls III is
(c) FromSoftware / Bandai Namco. Warhammer 40,000: Space Marine 2 is (c) Games Workshop / Saber Interactive /
Focus Entertainment. This is an unofficial fan project.
