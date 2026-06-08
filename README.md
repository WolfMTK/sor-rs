# sor-rs

Parser for .sor OTDR traces (Telcordia SR-4731 / Bellcore). Handles format v1 and v2; unknown vendor blocks are kept as
raw bytes.

## Install

```
[dependencies]
sor-rs = { git = "https://github.com/WolfMTK/sor-rs" }
```

## Usage

```
use sor_rs::SorFile;

let sor = SorFile::from_file("trace.sor", true)?;

if let Some(dp) = &sor.data_points {
    let (distance_km, level_db) = dp.as_arrays();
}
```

`from_bytes(&bytes, verify)` does the same from memory. The parsed sections live on `SorFile`: `map_block`,
`gen_params`, `sup_params`, `fxd_params`, `key_events`, `data_points`, `raw_blocks`.

## License

[MIT](./LICENSE)
