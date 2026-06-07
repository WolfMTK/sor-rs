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