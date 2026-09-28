# Dependency license fallbacks

Some published crate tarballs omit their workspace license file. Packaging includes these unmodified license texts, retrieved from the exact source commit recorded in each crate's `.cargo_vcs_info.json`:

| File | Crates | Source |
| --- | --- | --- |
| `rust-sdk-3.5.0.txt` | `rmcp`, `rmcp-macros` 3.5.0 | [rust-sdk LICENSE at 0cde3c5](https://github.com/modelcontextprotocol/rust-sdk/blob/0cde3c5cf3e6aff0cc852ce6045f107e95991f48/LICENSE) |
| `jsonschema-0.58.2.txt` | `jsonschema-regex`, `jsonschema-value` 0.58.2 | [jsonschema LICENSE at 3d48b90](https://github.com/Stranger6667/jsonschema/blob/3d48b9026c6518e7f3a2ecc6c9c94a9f77f6083c/LICENSE) |
| `simd-0.8.0.txt` | `uuid-simd`, `vsimd` 0.8.0 | [simd LICENSE at d74c030](https://github.com/Nugine/simd/blob/d74c030d9dc4f3cae02146d1f497ff62726ef09a/LICENSE) |

The fallback mapping is version-specific. A dependency update that lacks packaged license text must add and review its corresponding source license before packaging can succeed. Do not substitute the project's own license for a dependency's license.
