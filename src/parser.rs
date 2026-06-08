use crate::constants::SPEED_OF_LIGHT_KM_US;
use crate::errors::{Result, SorError};
use crate::models::{
    BlockInfo, Checksum, DataPoints, FxdParams, GenParams, KeyEvent, KeyEvents, KeyEventsSummary,
    MapBlock, RawBlock, SorFile, SupParams,
};
use crate::reader::Reader;
use std::collections::HashMap;

impl SorFile {
    /// Parses a SOR file from disk.
    pub fn from_file(path: impl AsRef<std::path::Path>, verify_checksum: bool) -> Result<SorFile> {
        let data = std::fs::read(path)?;
        Self::from_bytes(&data, verify_checksum)
    }

    /// Parses a SOR file from a byte slice.
    pub fn from_bytes(data: &[u8], verify_checksum: bool) -> Result<SorFile> {
        SorParser::new(data).parse(verify_checksum)
    }
}

// Parses a SOR file one block at a time.
struct SorParser<'a> {
    data: &'a [u8],
    format: u8,
}

impl<'a> SorParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        SorParser { data, format: 2 }
    }

    fn parse(&mut self, verify_checksum: bool) -> Result<SorFile> {
        let mut sor = SorFile::default();

        let map = self.parse_map()?;
        sor.map_block = Some(map.clone());

        let mut sorted_blocks: Vec<BlockInfo> = map.blocks.values().cloned().collect();
        sorted_blocks.sort_by_key(|block| block.offset);

        for info in &sorted_blocks {
            self.parse_block(info, &mut sor)?;
        }

        if verify_checksum {
            self.verify_checksum(&sor)?;
        }

        self.link_data_points(&mut sor);

        Ok(sor)
    }

    /// Parses the Map block (the block directory) and detects the format version (1 or 2).
    fn parse_map(&mut self) -> Result<MapBlock> {
        let mut reader = Reader::new(self.data);

        let magic = reader.read_bytes(4)?;
        if magic != b"Map\x00" {
            return Err(SorError::parse(format!(
                "Map marker not found: expected b\"Map\\0\", got {:?}",
                magic
            )));
        }

        let version = reader.read_u16_le()?;
        let map_size = reader.read_u32_le()?;
        let num_blocks_raw = reader.read_u16_le()?;
        let data_block_count = (num_blocks_raw as usize).saturating_sub(1);

        self.format = if version <= 100 { 1 } else { 2 };

        let mut blocks = HashMap::new();
        let mut running_offset = map_size as usize;

        for _ in 0..data_block_count {
            let name = reader.read_cstring()?;
            let block_ver = reader.read_u16_le()?;
            let block_size = reader.read_u32_le()?;
            blocks.insert(
                name.clone(),
                BlockInfo {
                    name,
                    version: block_ver,
                    size: block_size,
                    offset: running_offset,
                },
            );
            running_offset += block_size as usize;
        }

        Ok(MapBlock {
            version,
            map_size,
            blocks,
        })
    }

    /// Dispatches a block to its parser by name.
    fn parse_block(&self, info: &BlockInfo, sor: &mut SorFile) -> Result<()> {
        match info.name.as_str() {
            "GenParams" => sor.gen_params = Some(self.parse_gen_params(info)?),
            "SupParams" => sor.sup_params = Some(self.parse_sup_params(info)?),
            "FxdParams" => sor.fxd_params = Some(self.parse_fxd_params(info)?),
            "KeyEvents" => {
                sor.key_events = Some(self.parse_key_events(info, sor.fxd_params.as_ref())?)
            }
            "DataPts" => sor.data_points = Some(self.parse_data_pts(info)?),
            "Cksum" => sor.checksum = Some(self.parse_cksum(info)?),
            _ => {
                let mut r = self.block_reader(info)?;
                let remaining = r.remaining();
                let data = r.read_bytes(remaining)?.to_vec();
                sor.raw_blocks.insert(
                    info.name.clone(),
                    RawBlock {
                        name: info.name.clone(),
                        data,
                    },
                );
            }
        }
        Ok(())
    }

    /// A reader positioned at the block payload, just past the name string.
    fn block_reader(&self, info: &BlockInfo) -> Result<Reader<'a>> {
        let name_size = info.name.len() + 1;
        let content_offset = info.offset + name_size;
        let content_size = (info.size as usize).saturating_sub(name_size);
        Reader::new(self.data).slice(content_offset, content_size)
    }

    /// Parses GenParams (general measurement parameters).
    fn parse_gen_params(&self, info: &BlockInfo) -> Result<GenParams> {
        let mut reader = self.block_reader(info)?;

        let language = reader.read_fixed_str(2)?;
        let cable_id = reader.read_cstring()?;
        let fiber_id = reader.read_cstring()?;
        let fiber_type_code = reader.read_u16_le()?;
        let wavelength_nm = reader.read_u16_le()? as f64 * 0.1;
        let location_a = reader.read_cstring()?;
        let cable_code = reader.read_cstring()?;
        let build_condition = reader.read_cstring()?;
        let user_offset_raw = reader.read_i32_le()?;

        let (location_b, user_offset_distance, operator, comments) = if self.format == 2 {
            let location_b = reader.read_cstring()?;
            let user_offset_distance = reader.read_i32_le()?;
            let operator = reader.read_cstring()?;
            let comments = read_cstring_opt(&mut reader);
            (location_b, user_offset_distance, operator, comments)
        } else {
            let operator = read_cstring_opt(&mut reader);
            (String::new(), 0, operator, String::new())
        };

        Ok(GenParams {
            language,
            cable_id,
            fiber_id,
            fiber_type_code,
            wavelength_nm,
            location_a,
            location_b,
            cable_code,
            build_condition,
            user_offset_raw,
            user_offset_distance,
            operator,
            comments,
        })
    }

    /// Parses SupParams (supplier and instrument info).
    fn parse_sup_params(&self, info: &BlockInfo) -> Result<SupParams> {
        let mut reader = self.block_reader(info)?;
        Ok(SupParams {
            supplier: reader.read_cstring()?,
            otdr_name: reader.read_cstring()?,
            otdr_sn: reader.read_cstring()?,
            module_name: reader.read_cstring()?,
            module_sn: reader.read_cstring()?,
            sw_version: reader.read_cstring()?,
            other: read_cstring_opt(&mut reader),
        })
    }

    /// Parses FxdParams (fixed acquisition parameters).
    fn parse_fxd_params(&self, info: &BlockInfo) -> Result<FxdParams> {
        let mut reader = self.block_reader(info)?;

        let timestamp = reader.read_u32_le()?;
        let unit = reader.read_fixed_str(2)?;
        let wavelength_nm = reader.read_u16_le()? as f64 * 0.1;
        let acquisition_offset = reader.read_i32_le()?;

        let acquisition_offset_distance = if self.format == 2 {
            reader.read_i32_le()?
        } else {
            0
        };

        let num_pw = reader.read_u16_le()? as usize;
        let mut pulse_widths_ns = Vec::with_capacity(num_pw);
        for _ in 0..num_pw {
            pulse_widths_ns.push(reader.read_u16_le()?);
        }

        let sample_spacing_raw = reader.read_u32_le()?;
        let num_data_points = reader.read_u32_le()?;
        let group_index = reader.read_u32_le()? as f64 * 1e-5;
        let backscatter_coeff_db = reader.read_u16_le()? as f64 * -0.1;
        let num_averages = reader.read_u32_le()?;

        let (
            averaging_time_s,
            acquisition_range_raw,
            acquisition_range_coeff,
            front_panel_offset,
            noise_floor_level,
            noise_floor_scaling,
            power_offset_first_point,
            loss_threshold_db,
            refl_threshold_db,
            eof_threshold_db,
            trace_type,
            x1,
            y1,
            x2,
            y2,
        ) = if self.format == 2 {
            let avg_time = reader.read_u16_le()? as f64 * 0.1;
            let acq_range = reader.read_u32_le()?;
            let acq_coeff = reader.read_i32_le()?;
            let fpo = reader.read_i32_le()?;
            let nfl = reader.read_u16_le()?;
            let nfs = reader.read_i16_le()?;
            let pofp = reader.read_u16_le()?;
            let loss_thr = reader.read_u16_le()? as f64 * 0.001;
            let refl_thr = reader.read_u16_le()? as f64 * -0.001;
            let eof_thr = reader.read_u16_le()? as f64 * 0.001;
            let tt = reader.read_fixed_str(2)?.replace('\0', "");
            (
                avg_time,
                acq_range,
                acq_coeff,
                fpo,
                nfl,
                nfs,
                pofp,
                loss_thr,
                refl_thr,
                eof_thr,
                tt,
                read_i32_or(&mut reader, 0)?,
                read_i32_or(&mut reader, 0)?,
                read_i32_or(&mut reader, 0)?,
                read_i32_or(&mut reader, 0)?,
            )
        } else {
            let acq_range = reader.read_u32_le()?;
            let fpo = read_i32_or(&mut reader, 0)?;
            let nfl = read_u16_or(&mut reader, 0)?;
            let nfs = read_i16_or(&mut reader, 0)?;
            let pofp = read_u16_or(&mut reader, 0)?;
            let loss_thr = read_u16_or(&mut reader, 0)? as f64 * 0.001;
            let refl_thr = read_u16_or(&mut reader, 0)? as f64 * -0.001;
            let eof_thr = read_u16_or(&mut reader, 0)? as f64 * 0.001;
            let tt = if reader.remaining() >= 2 {
                reader.read_fixed_str(2)?.replace('\0', "")
            } else {
                "ST".to_string()
            };
            (
                0.0, acq_range, 0, fpo, nfl, nfs, pofp, loss_thr, refl_thr, eof_thr, tt, 0, 0, 0, 0,
            )
        };

        Ok(FxdParams {
            timestamp,
            unit,
            wavelength_nm,
            acquisition_offset,
            acquisition_offset_distance,
            pulse_widths_ns,
            sample_spacing_raw,
            num_data_points,
            group_index,
            backscatter_coeff_db,
            num_averages,
            averaging_time_s,
            acquisition_range_raw,
            acquisition_range_coeff,
            front_panel_offset,
            noise_floor_level,
            noise_floor_scaling,
            power_offset_first_point,
            loss_threshold_db,
            refl_threshold_db,
            eof_threshold_db,
            trace_type,
            x1,
            y1,
            x2,
            y2,
        })
    }

    /// Parses KeyEvents (detected trace events).
    fn parse_key_events(&self, info: &BlockInfo, fxd: Option<&FxdParams>) -> Result<KeyEvents> {
        let mut reader = self.block_reader(info)?;

        let factor = match fxd {
            Some(fp) => fp.distance_factor_km(),
            None => 1e-4 * SPEED_OF_LIGHT_KM_US / 1.4682,
        };

        let num_events = reader.read_u16_le()? as usize;
        let mut events = Vec::with_capacity(num_events);

        for _ in 0..num_events {
            let ev_num = reader.read_u16_le()?;
            let prop_time = reader.read_u32_le()?;
            let slope_raw = reader.read_i16_le()?;
            let loss_raw = reader.read_i16_le()?;
            let refl_raw = reader.read_i32_le()?;
            let event_type = String::from_utf8_lossy(reader.read_bytes(8)?).into_owned();

            let distance_km = prop_time as f64 * factor;

            let (end_prev, start_curr, end_curr, start_next, peak) = if self.format == 2 {
                (
                    reader.read_u32_le()? as f64 * factor,
                    reader.read_u32_le()? as f64 * factor,
                    reader.read_u32_le()? as f64 * factor,
                    reader.read_u32_le()? as f64 * factor,
                    reader.read_u32_le()? as f64 * factor,
                )
            } else {
                (0.0, 0.0, 0.0, 0.0, 0.0)
            };

            let comments = reader.read_cstring()?;

            events.push(KeyEvent {
                number: ev_num,
                distance_km,
                slope_db_per_km: slope_raw as f64 * 0.001,
                loss_db: loss_raw as f64 * 0.001,
                refl_db: refl_raw as f64 * 0.001,
                event_type,
                end_of_prev_km: end_prev,
                start_of_curr_km: start_curr,
                end_of_curr_km: end_curr,
                start_of_next_km: start_next,
                peak_km: peak,
                comments,
            });
        }

        let mut summary = KeyEventsSummary {
            total_loss_db: 0.0,
            orl_db: 0.0,
            loss_start_km: 0.0,
            loss_end_km: 0.0,
            orl_start_km: 0.0,
            orl_end_km: 0.0,
        };

        if reader.remaining() >= 4 {
            summary.total_loss_db = reader.read_i32_le()? as f64 * 0.001;
        }
        if reader.remaining() >= 4 {
            summary.loss_start_km = reader.read_i32_le()? as f64 * factor;
        }
        if reader.remaining() >= 4 {
            summary.loss_end_km = reader.read_u32_le()? as f64 * factor;
        }
        if reader.remaining() >= 2 {
            summary.orl_db = reader.read_u16_le()? as f64 * 0.001;
        }
        if reader.remaining() >= 4 {
            summary.orl_start_km = reader.read_i32_le()? as f64 * factor;
        }
        if reader.remaining() >= 4 {
            summary.orl_end_km = reader.read_u32_le()? as f64 * factor;
        }

        Ok(KeyEvents { events, summary })
    }

    /// Parses DataPts (raw OTDR trace samples).
    fn parse_data_pts(&self, info: &BlockInfo) -> Result<DataPoints> {
        let mut reader = self.block_reader(info)?;

        let num_points = reader.read_u32_le()?;
        let num_traces = reader.read_u16_le()?;
        let _num_points_2 = reader.read_u32_le()?;
        let scaling_factor = reader.read_u16_le()?;

        let raw_bytes = reader.read_bytes(num_points as usize * 2)?;
        let mut raw_data = Vec::with_capacity(num_points as usize);
        for chunk in raw_bytes.chunks_exact(2) {
            raw_data.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }

        Ok(DataPoints {
            num_points,
            num_traces,
            scaling_factor,
            raw_data,
            resolution_m: 0.0,
            x_offset_m: 0.0,
        })
    }

    /// Parses the Cksum block.
    fn parse_cksum(&self, info: &BlockInfo) -> Result<Checksum> {
        let mut r = self.block_reader(info)?;
        Ok(Checksum {
            algorithm: 1,
            value: r.read_u16_le()?,
        })
    }

    /// Verifies the file's CRC-16/CCITT-FALSE checksum.
    fn verify_checksum(&self, sor: &SorFile) -> Result<()> {
        let cksum_info = sor.map_block.as_ref().and_then(|m| m.blocks.get("Cksum"));

        if let (Some(ci), Some(stored)) = (cksum_info, sor.checksum) {
            let crc_end = ci.offset + ci.name.len() + 1;
            if crc_end <= self.data.len() {
                let computed = crc16(&self.data[..crc_end]);
                if computed != stored.value {
                    return Err(SorError::ChecksumError {
                        expected: stored.value,
                        actual: computed,
                    });
                }
            }
        }
        Ok(())
    }

    /// Copies resolution and x-offset from FxdParams into DataPoints.
    fn link_data_points(&self, sor: &mut SorFile) {
        if let (Some(dp), Some(fp)) = (&mut sor.data_points, &sor.fxd_params) {
            dp.resolution_m = fp.resolution_m();
            dp.x_offset_m = fp.acquisition_offset as f64 * fp.resolution_m();
        }
    }
}

/// Reads a C-string, or returns an empty one if no bytes are left.
fn read_cstring_opt(reader: &mut Reader<'_>) -> String {
    if reader.remaining() > 0 {
        return reader.read_cstring().unwrap_or_default();
    }
    String::new()
}

/// Reads an i32, or `default` if fewer than 4 bytes remain.
fn read_i32_or(reader: &mut Reader<'_>, default: i32) -> Result<i32> {
    if reader.remaining() >= 4 {
        return reader.read_i32_le();
    }
    Ok(default)
}

/// Reads a u16, or `default` if fewer than 2 bytes remain.
fn read_u16_or(reader: &mut Reader<'_>, default: u16) -> Result<u16> {
    if reader.remaining() >= 2 {
        return reader.read_u16_le();
    }
    Ok(default)
}

/// Reads an i16, or `default` if fewer than 2 bytes remain.
fn read_i16_or(reader: &mut Reader<'_>, default: i16) -> Result<i16> {
    if reader.remaining() >= 2 {
        return reader.read_i16_le();
    }
    Ok(default)
}

/// CRC-16/CCITT-FALSE digest of `data`.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use crate::errors::SorError;
    use crate::models::SorFile;
    use crate::parser::crc16;
    use rstest::{fixture, rstest};

    fn sample(name: &str) -> String {
        format!("{}/data/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[fixture]
    fn noyes() -> String { sample("example1-noyes-ofl280.sor") }

    #[fixture]
    fn noyes_fast() -> String { sample("example1-noyes-ofl280-fastreporter-save.sor") }

    #[fixture]
    fn exfo_max() -> String { sample("example2-exfo-maxtester730c.sor") }

    #[fixture]
    fn anritsu() -> String { sample("example3-anritsu-accessmastermt9085.sor") }

    #[fixture]
    fn exfo_1310() -> String { sample("example4-exfo-ftb4ftbx730c-mfdgainer-1310nm.sor") }

    #[fixture]
    fn exfo_1550() -> String { sample("example4-exfo-ftb4ftbx730c-mfdgainer-1550nm.sor") }

    #[fixture]
    fn exfo_rtu() -> String { sample("example5-exfo-rtu2ftbx735c-sm7r-ea-hrd.sor") }

    #[fixture]
    fn sor_file(#[from(noyes)] path: String) -> SorFile {
        SorFile::from_file(&path, false).unwrap()
    }

    #[rstest]
    fn invalid_magic_raises() {
        let bad = b"NOTSOR\x00"
            .iter()
            .chain(b"\x00".iter().cycle().take(100))
            .copied()
            .collect::<Vec<_>>();
        assert!(matches!(SorFile::from_bytes(&bad, false), Err(SorError::ParseError(_))));
    }

    #[rstest]
    fn file_not_found_raises() {
        let res = SorFile::from_file("/no/such/file.sor", false);
        assert!(matches!(res, Err(SorError::IOError(_))));
    }

    #[rstest]
    fn parse_without_error(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            SorFile::from_file(path, false)
                .unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
        }
    }

    #[rstest]
    fn from_bytes(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let data = std::fs::read(path).unwrap();
            SorFile::from_bytes(&data, false)
                .unwrap_or_else(|e| panic!("Failed from_bytes {path}: {e}"));
        }
    }

    #[rstest]
    fn map_block_present(sor_file: SorFile) {
        let map = sor_file.map_block.as_ref().unwrap();
        assert_eq!(map.version, 200);
        assert_eq!(map.version_str(), "2.00");
    }

    #[rstest]
    fn map_block_required_keys(sor_file: SorFile) {
        let map = sor_file.map_block.as_ref().unwrap();
        for key in &["GenParams", "SupParams", "FxdParams", "KeyEvents", "DataPts", "Cksum"] {
            assert!(map.blocks.contains_key(*key), "Missing block: {key}");
        }
    }

    #[rstest]
    fn map_offsets_sequential(sor_file: SorFile) {
        let map = sor_file.map_block.as_ref().unwrap();
        let mut sorted: Vec<_> = map.blocks.values().collect();
        sorted.sort_by_key(|b| b.offset);
        for w in sorted.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert_eq!(
                a.offset + a.size as usize,
                b.offset,
                "Blocks {} and {} are not contiguous",
                a.name,
                b.name
            );
        }
    }

    #[rstest]
    fn map_all_files(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            assert!(sor.map_block.as_ref().unwrap().num_blocks() > 0);
        }
    }

    #[rstest]
    fn gen_params_noyes(sor_file: SorFile) {
        let gp = sor_file.gen_params.as_ref().unwrap();
        assert_eq!(gp.language, "EN");
        assert_eq!(gp.language.len(), 2);
        assert!(gp.fiber_type_code > 0);
        let wl = gp.wavelength_nm;
        assert!(
            (100.0..=200.0).contains(&wl) || (1000.0..=1700.0).contains(&wl),
            "Unrealistic wavelength: {wl}"
        );
    }

    #[rstest]
    fn gen_params_fiber_type_str(sor_file: SorFile) {
        let gp = sor_file.gen_params.as_ref().unwrap();
        assert!(gp.fiber_type_str().contains("G.652"));
    }

    #[rstest]
    fn gen_params_all_files(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let gp = sor.gen_params.as_ref().expect("GenParams missing");
            assert_eq!(gp.language.len(), 2, "Invalid language in {path}");
        }
    }

    #[rstest]
    fn sup_params_noyes_supplier(sor_file: SorFile) {
        let sp = sor_file.sup_params.as_ref().unwrap();
        assert!(sp.supplier.contains("Noyes"), "Supplier: {:?}", sp.supplier);
        assert!(sp.otdr_name.contains("OFL280"), "Model: {:?}", sp.otdr_name);
    }

    #[rstest]
    fn sup_params_anritsu(anritsu: String) {
        let sor = SorFile::from_file(anritsu, false).unwrap();
        let sp = sor.sup_params.as_ref().unwrap();
        let has_info =
            !sp.supplier.is_empty() || !sp.otdr_name.is_empty() || !sp.module_name.is_empty();
        assert!(has_info);
    }

    #[rstest]
    fn sup_params_all_files(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            assert!(sor.sup_params.is_some(), "SupParams missing in {path}");
        }
    }

    #[rstest]
    fn fxd_params_noyes(sor_file: SorFile) {
        let fp = sor_file.fxd_params.as_ref().unwrap();

        assert!(fp.timestamp > 946_684_800, "Timestamp is too early");
        assert!(fp.timestamp < 2_208_988_800, "Timestamp is too late");

        assert_eq!(fp.unit, "mt");

        assert!((1.4..=1.6).contains(&fp.group_index), "n = {}", fp.group_index);

        assert_eq!(fp.num_data_points, 30000);

        assert_eq!(fp.pulse_widths_ns, vec![30u16]);

        let sol = 0.299_792_458_f64;
        let expected_dx = fp.sample_spacing_raw as f64 * 1e-8 * sol / fp.group_index * 1000.0;
        assert!((fp.resolution_m() - expected_dx).abs() < 1e-9);

        let expected_range = fp.acquisition_range_raw as f64 * 2e-5;
        assert!((fp.range_km() - expected_range).abs() < 1e-9);
    }

    #[rstest]
    fn fxd_params_resolution_realistic(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let fp = sor.fxd_params.as_ref().unwrap();
            let r = fp.resolution_m();
            assert!(r > 0.0 && r < 100.0, "Unrealistic resolution {r:.4} in {path}");
        }
    }

    #[rstest]
    fn fxd_params_group_index_realistic(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let n = sor.fxd_params.as_ref().unwrap().group_index;
            assert!((1.4..=1.6).contains(&n), "n={n} in {path}");
        }
    }

    #[rstest]
    fn fxd_params_distance_factor_formula(sor_file: SorFile) {
        let fp = sor_file.fxd_params.as_ref().unwrap();
        let sol = 0.299_792_458_f64;
        let expected = 1e-4 * sol / fp.group_index;
        assert!((fp.distance_factor_km() - expected).abs() < 1e-15);
    }

    #[rstest]
    fn key_events_noyes_count(sor_file: SorFile) {
        let ke = sor_file.key_events.as_ref().unwrap();
        assert_eq!(ke.num_events(), 3);
    }

    #[rstest]
    fn key_events_first_is_reflection(sor_file: SorFile) {
        let ke = sor_file.key_events.as_ref().unwrap();
        let first = &ke.events[0];
        assert!(
            first.is_reflection(),
            "First event must be a reflection: {:?}",
            first.event_type
        );
    }

    #[rstest]
    fn key_events_type_length(sor_file: SorFile) {
        let ke = sor_file.key_events.as_ref().unwrap();
        for ev in &ke.events {
            assert_eq!(ev.event_type.len(), 8, "Event type length: {:?}", ev.event_type);
        }
    }

    #[rstest]
    fn key_events_distances_non_negative(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let ke = sor.key_events.as_ref().unwrap();
            for ev in &ke.events {
                assert!(ev.distance_km >= 0.0, "Negative distance in {path}");
            }
        }
    }

    #[rstest]
    fn key_events_noyes_distances(sor_file: SorFile) {
        let ke = sor_file.key_events.as_ref().unwrap();
        let ev1 = &ke.events[0];
        let ev2 = &ke.events[1];
        let ev3 = &ke.events[2];
        assert!((ev1.distance_km - 0.0).abs() < 0.001);
        assert!((ev2.distance_km - 0.011).abs() < 0.001, "ev2 dist = {:.4}", ev2.distance_km);
        assert!((ev3.distance_km - 3.734).abs() < 0.01, "ev3 dist = {:.4}", ev3.distance_km);
    }

    #[rstest]
    fn key_events_summary_orl_realistic(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let orl = sor.key_events.as_ref().unwrap().summary.orl_db;
            assert!((-100.0..=100.0).contains(&orl), "ORL={orl} in {path}");
        }
    }

    #[rstest]
    fn key_events_noyes_summary(sor_file: SorFile) {
        let s = &sor_file.key_events.as_ref().unwrap().summary;
        assert!((s.total_loss_db - 0.576).abs() < 0.01, "total_loss={}", s.total_loss_db);
        assert!((s.orl_db - 24.516).abs() < 0.01, "orl={}", s.orl_db);
    }

    #[rstest]
    fn key_events_filter_methods(sor_file: SorFile) {
        let ke = sor_file.key_events.as_ref().unwrap();
        assert_eq!(ke.reflections().len(), 1);
        assert_eq!(ke.loss_events().len(), 1);
    }

    #[rstest]
    fn key_events_all_files_positive_count(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            assert!(sor.key_events.as_ref().unwrap().num_events() > 0, "{path}");
        }
    }

    #[rstest]
    fn data_points_noyes(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let fp = sor_file.fxd_params.as_ref().unwrap();

        assert_eq!(dp.num_points, fp.num_data_points);
        assert_eq!(dp.raw_data.len(), 30000);
        assert_eq!(dp.scaling_factor, 1000);
    }

    #[rstest]
    fn data_points_levels_db(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let levels = dp.levels_db();
        assert_eq!(levels.len(), dp.num_points as usize);
        for (i, (&raw, &lvl)) in dp.raw_data.iter().zip(levels.iter()).enumerate() {
            if raw < 65535 {
                assert!(lvl >= 0.0, "Negative level [{i}]: {lvl}");
            }
        }
    }

    #[rstest]
    fn data_points_distances_monotonic(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let dists = dp.distances_km();
        for w in dists.windows(2) {
            assert!(w[1] >= w[0], "Distances are not monotonic: {} >= {}", w[0], w[1]);
        }
    }

    #[rstest]
    fn data_points_as_arrays(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let (dists, levels) = dp.as_arrays();
        assert_eq!(dists.len(), levels.len());
        assert_eq!(dists.len(), dp.num_points as usize);
    }

    #[rstest]
    fn data_points_resolution_linked(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let fp = sor_file.fxd_params.as_ref().unwrap();
        assert!((dp.resolution_m - fp.resolution_m()).abs() < 1e-9);
    }

    #[rstest]
    fn data_points_dynamic_range_positive(sor_file: SorFile) {
        assert!(sor_file.data_points.as_ref().unwrap().dynamic_range_db() > 0.0);
    }

    #[rstest]
    fn data_points_start_near_zero(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        let first = dp.distances_km()[0];
        assert!(first.abs() < 1.0, "First distance is too far from 0: {first}");
    }

    #[rstest]
    fn data_points_all_files(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) {
        for path in &[noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu] {
            let sor = SorFile::from_file(path, false).unwrap();
            let dp = sor.data_points.as_ref().expect("DataPts missing");
            assert!(dp.num_points > 0);
            assert_eq!(dp.raw_data.len(), dp.num_points as usize);
            let (d, l) = dp.as_arrays();
            assert_eq!(d.len(), l.len());
        }
    }

    #[rstest]
    fn checksum_present(sor_file: SorFile) {
        assert!(sor_file.checksum.is_some());
        assert_ne!(sor_file.checksum.unwrap().value, 0);
    }

    #[rstest]
    fn checksum_verify_noyes(noyes: String) {
        SorFile::from_file(noyes, true).expect("CRC should match");
    }

    #[rstest]
    fn checksum_wrong_raises(noyes: String) {
        let mut data = std::fs::read(noyes).unwrap();
        data[100] ^= 0xFF;
        let res = SorFile::from_bytes(&data, true);
        assert!(matches!(res, Err(SorError::ChecksumError { .. })));
    }

    #[rstest]
    fn raw_blocks_exfo(exfo_max: String) {
        let sor = SorFile::from_file(exfo_max, false).unwrap();
        let has_exfo = sor.raw_blocks.keys().any(|k| k.contains("Exfo") || k.contains("exfo"));
        assert!(has_exfo || !sor.raw_blocks.is_empty(), "Expected EXFO proprietary blocks");
    }

    #[rstest]
    fn raw_blocks_anritsu(anritsu: String) {
        let sor = SorFile::from_file(anritsu, false).unwrap();
        assert!(!sor.raw_blocks.is_empty(), "Expected Anritsu proprietary blocks");
        for (_, rb) in &sor.raw_blocks {
            assert_eq!(rb.size(), rb.data.len());
        }
    }

    #[rstest]
    fn wavelength_ratio_1310_vs_1550(exfo_1310: String, exfo_1550: String) {
        let sor1 = SorFile::from_file(exfo_1310, false).unwrap();
        let sor2 = SorFile::from_file(exfo_1550, false).unwrap();
        let wl1 = sor1.fxd_params.as_ref().unwrap().wavelength_nm;
        let wl2 = sor2.fxd_params.as_ref().unwrap().wavelength_nm;
        assert!(wl1 < wl2, "wl1={wl1} >= wl2={wl2}");
        let ratio = wl1 / wl2;
        assert!((0.80..=0.90).contains(&ratio), "ratio={ratio:.3}");
    }

    #[rstest]
    fn noyes_vs_fastreporter_group_index(noyes: String, noyes_fast: String) {
        let sor1 = SorFile::from_file(noyes, false).unwrap();
        let sor2 = SorFile::from_file(noyes_fast, false).unwrap();
        let n1 = sor1.fxd_params.as_ref().unwrap().group_index;
        let n2 = sor2.fxd_params.as_ref().unwrap().group_index;
        assert!((n1 - n2).abs() < 0.01, "n1={n1} vs n2={n2}");
    }

    #[rstest]
    fn crc16_known_value() {
        let val = crc16(b"123456789");
        assert_eq!(val, 0x29B1, "CRC-16/CCITT-FALSE: expected 0x29B1, got {val:#06x}");
    }

    #[rstest]
    fn crc16_empty() {
        assert_eq!(crc16(b""), 0xFFFF);
    }

    #[rstest]
    fn checksum_verify_selected_files(noyes: String) {
        SorFile::from_file(noyes, true).expect("Noyes OFL280 file CRC should match");
    }
}
