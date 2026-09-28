use std::collections::HashMap;

use crate::SorError;
use crate::constants::SPEED_OF_LIGHT_KM_US;
use crate::errors::Result;
use crate::reader::Reader;

/// Description of a single block from the Map table.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Name identifier.
    pub name: String,
    /// Version.
    pub version: u16,
    /// Size in bytes.
    pub size: u32,
    /// Offset from the start of the file.
    pub offset: usize,
}

impl BlockInfo {
    pub fn version_str(&self) -> String {
        format!("{}.{:02}", self.version / 100, self.version % 100)
    }
}

/// Map block: index of all file blocks.
#[derive(Debug, Clone)]
pub struct MapBlock {
    /// File format version.
    pub version: u16,
    /// Size of the Map block itself in bytes.
    pub map_size: u32,
    /// Descriptions of all blocks.
    pub blocks: HashMap<String, BlockInfo>,
}

impl MapBlock {
    pub fn version_str(&self) -> String {
        format!("{}.{:02}", self.version / 100, self.version % 100)
    }

    pub fn num_blocks(&self) -> usize {
        self.blocks.len()
    }

    pub(crate) fn read_from(reader: &mut Reader) -> Result<(Self, u8)> {
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

        let mut blocks = HashMap::with_capacity(data_block_count);
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

        let format = if version <= 100 { 1 } else { 2 };
        Ok((
            MapBlock {
                version,
                map_size,
                blocks,
            },
            format,
        ))
    }
}

/// General measurement parameters.
#[derive(Debug, Clone)]
pub struct GenParams {
    /// Language code (2 characters).
    pub language: String,
    /// Cable identifier.
    pub cable_id: String,
    /// Fiber identifier.
    pub fiber_id: String,
    /// Numeric fiber type code.
    pub fiber_type_code: u16,
    /// Nominal wavelength, nm.
    pub wavelength_nm: f64,
    /// Location A (near end).
    pub location_a: String,
    /// Location B (far end).
    pub location_b: String,
    /// Cable code/type.
    pub cable_code: String,
    /// Build/laying condition.
    pub build_condition: String,
    /// User-defined offset (raw value).
    pub user_offset_raw: i32,
    /// User-defined offset distance.
    pub user_offset_distance: i32,
    /// Operator name.
    pub operator: String,
    /// User comments.
    pub comments: String,
}

impl GenParams {
    pub fn fiber_type_str(&self) -> &'static str {
        match self.fiber_type_code {
            651 => "G.651 (MMF 50/125)",
            652 => "G.652 (standard SMF)",
            653 => "G.653 (DSF)",
            654 => "G.654 (CSF)",
            655 => "G.655 (NZDSF)",
            656 => "G.656",
            657 => "G.657",
            _ => "Unknown",
        }
    }

    pub(crate) fn read_from(reader: &mut Reader, format: u8) -> Result<Self> {
        let language = reader.read_fixed_str(2)?;
        let cable_id = reader.read_cstring()?;
        let fiber_id = reader.read_cstring()?;
        let fiber_type_code = if format == 2 {
            reader.read_u16_le()?
        } else {
            0
        };
        // nominal wavelength is in whole nm here, unlike FxdParams (0.1 nm)
        let wavelength_nm = f64::from(reader.read_u16_le()?);
        let location_a = reader.read_cstring()?;
        let location_b = reader.read_cstring()?;
        let cable_code = reader.read_cstring()?;
        let build_condition = reader.read_fixed_str(2)?;
        let user_offset_raw = reader.read_i32_le()?;
        let user_offset_distance = if format == 2 {
            reader.read_i32_le()?
        } else {
            0
        };
        let operator = reader.read_cstring_opt()?;
        let comments = reader.read_cstring_opt()?;

        Ok(Self {
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
}

/// Instrument (OTDR) information.
#[derive(Debug, Clone)]
pub struct SupParams {
    /// OTDR manufacturer/supplier name.
    pub supplier: String,
    /// OTDR model name.
    pub otdr_name: String,
    /// OTDR serial number.
    pub otdr_sn: String,
    /// OTDR module name.
    pub module_name: String,
    /// OTDR module serial number.
    pub module_sn: String,
    /// OTDR software/firmware version.
    pub sw_version: String,
    /// Other proprietary supplier information.
    pub other: String,
}

impl SupParams {
    pub(crate) fn read_from(reader: &mut Reader) -> Result<Self> {
        Ok(Self {
            supplier: reader.read_cstring()?,
            otdr_name: reader.read_cstring()?,
            otdr_sn: reader.read_cstring()?,
            module_name: reader.read_cstring()?,
            module_sn: reader.read_cstring()?,
            sw_version: reader.read_cstring()?,
            other: reader.read_cstring_opt()?,
        })
    }
}

/// Fixed measurement parameters.
#[derive(Debug, Clone)]
pub struct FxdParams {
    /// Unix timestamp (seconds).
    pub timestamp: u32,
    /// Distance units: "mt", "km", "ft", "kf", "mi".
    pub unit: String,
    /// Wavelength, nm (raw * 0.1).
    pub wavelength_nm: f64,
    /// Acquisition offset (raw value).
    pub acquisition_offset: i32,
    /// Acquisition offset converted to distance.
    pub acquisition_offset_distance: i32,
    /// Pulse widths, ns.
    pub pulse_widths_ns: Vec<u16>,
    /// Sample spacing (raw uint32; unit = 1e-8 µs).
    pub sample_spacing_raw: u32,
    /// Number of data points in the trace.
    pub num_data_points: u32,
    /// Group index (raw * 1e-5).
    pub group_index: f64,
    /// Backscatter coefficient, dB.
    pub backscatter_coeff_db: f64,
    /// Number of averages performed.
    pub num_averages: u32,
    /// Averaging time, s.
    pub averaging_time_s: f64,
    /// Acquisition range (raw value).
    pub acquisition_range_raw: u32,
    /// Acquisition range coefficient.
    pub acquisition_range_coeff: i32,
    /// Front panel offset.
    pub front_panel_offset: i32,
    /// Noise floor level.
    pub noise_floor_level: u16,
    /// Noise floor scaling factor.
    pub noise_floor_scaling: i16,
    /// Power offset for the first data point.
    pub power_offset_first_point: u16,
    /// Loss threshold for event detection, dB.
    pub loss_threshold_db: f64,
    /// Reflection threshold for event detection, dB.
    pub refl_threshold_db: f64,
    /// End-of-fiber threshold for detection, dB.
    pub eof_threshold_db: f64,
    /// Trace type: "ST", "RT", "DT", "RF".
    pub trace_type: String,
    /// X1 coordinate for data windowing.
    pub x1: i32,
    /// Y1 coordinate for data windowing.
    pub y1: i32,
    /// X2 coordinate for data windowing.
    pub x2: i32,
    /// Y2 coordinate for data windowing.
    pub y2: i32,
}

impl FxdParams {
    /// Distance resolution (spacing between points), m.
    ///
    /// Formula:
    /// `dx[m] = sample_spacing_raw * 1e-8 [µs] * sol[km/µs] / n * 1000[m/km]`
    pub fn resolution_m(&self) -> f64 {
        let sample_spacing_raw = self.sample_spacing_raw as f64;
        sample_spacing_raw * 1e-8 * SPEED_OF_LIGHT_KM_US / self.group_index * 1000.0
    }

    /// Acquisition range, km.
    ///
    /// Formula:
    /// `range = acquisition_range_raw * 2e-5`
    pub fn range_km(&self) -> f64 {
        let acquisition_range_raw = self.acquisition_range_raw as f64;
        acquisition_range_raw * 2e-5
    }

    /// Conversion factor from raw prop_time → km.
    ///
    /// Formula:
    /// `factor = 1e-4 [µs/unit] * sol[km/µs] / n`
    pub fn distance_factor_km(&self) -> f64 {
        1e-4 * SPEED_OF_LIGHT_KM_US / self.group_index
    }

    /// First pulse width, ns.
    pub fn pulse_width_ns(&self) -> Option<u16> {
        self.pulse_widths_ns.first().copied()
    }

    /// Distance units as a string.
    pub fn unit_str(&self) -> &'static str {
        match self.unit.as_str() {
            "mt" => "meters",
            "km" => "kilometers",
            "ft" => "feet",
            "kf" => "kilofeet",
            "mi" => "miles",
            _ => "unknown",
        }
    }

    /// Trace type as a string.
    pub fn trace_type_str(&self) -> &'static str {
        match self.trace_type.as_str() {
            "ST" => "Standard trace",
            "RT" => "Reverse trace",
            "DT" => "Difference trace",
            "RF" => "Reference trace",
            _ => "Unknown",
        }
    }
}

/// A single key event of the OTDR trace.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// Event sequence number (1-indexed).
    pub number: u16,
    /// Distance to the event, km.
    pub distance_km: f64,
    /// Trace slope before the event, dB/km.
    pub slope_db_per_km: f64,
    /// Event loss, dB.
    pub loss_db: f64,
    /// Reflection (return loss), dB.
    pub refl_db: f64,
    /// Raw event type code (8 bytes ASCII).
    pub event_type: String,
    /// Distance to the end of the previous event, km.
    pub end_of_prev_km: f64,
    /// Distance to the start of the current event, km.
    pub start_of_curr_km: f64,
    /// Distance to the end of the current event, km.
    pub end_of_curr_km: f64,
    /// Distance to the start of the next event, km.
    pub start_of_next_km: f64,
    /// Distance to the peak of the event, km.
    pub peak_km: f64,
    /// User comments for the event.
    pub comments: String,
}

impl KeyEvent {
    /// First character of the event code: `0` non-reflective, `1` reflective,
    /// `2` saturated reflective.
    fn kind_code(&self) -> Option<char> {
        self.event_type.chars().next()
    }

    /// Second character of the event code: `F` found by software, `M` moved by user,
    /// `A` added by user, `E` end of fiber, `O` out of range.
    fn mode_code(&self) -> Option<char> {
        self.event_type.chars().nth(1)
    }

    /// True for reflective events, including saturated ones.
    pub fn is_reflection(&self) -> bool {
        matches!(self.kind_code(), Some('1' | '2'))
    }

    /// True if the reflection saturated the receiver.
    pub fn is_saturated(&self) -> bool {
        self.kind_code() == Some('2')
    }

    /// True if this is a non-reflective (loss/gain) event.
    pub fn is_loss_event(&self) -> bool {
        self.kind_code() == Some('0')
    }

    /// True if the event marks the end of the fiber.
    pub fn is_end_of_fiber(&self) -> bool {
        matches!(self.mode_code(), Some('E' | 'e'))
    }

    /// True if the event was detected automatically and not edited by the user.
    pub fn is_auto(&self) -> bool {
        !matches!(self.mode_code(), Some('M' | 'A'))
    }

    /// Event subtype as a string.
    pub fn subtype_str(&self) -> &'static str {
        if self.is_end_of_fiber() {
            return "end of fiber";
        }
        match self.kind_code() {
            Some('0') => "loss/drop/gain",
            Some('1') => "reflection",
            Some('2') => "saturated reflection",
            _ => "unknown",
        }
    }

    pub(crate) fn read_from(reader: &mut Reader, format: u8, factor: f64) -> Result<Self> {
        let number = reader.read_u16_le()?;
        let prop_time = reader.read_u32_le()?;
        let slope_raw = reader.read_i16_le()?;
        let loss_raw = reader.read_i16_le()?;
        let refl_raw = reader.read_i32_le()?;
        let event_type = String::from_utf8_lossy(reader.read_bytes(8)?).into_owned();

        let [
            end_of_prev_km,
            start_of_curr_km,
            end_of_curr_km,
            start_of_next_km,
            peak_km,
        ] = if format == 2 {
            let mut out = [0.0f64; 5];
            for v in &mut out {
                *v = reader.read_u32_le()? as f64 * factor;
            }
            out
        } else {
            [0.0; 5]
        };

        Ok(Self {
            number,
            distance_km: prop_time as f64 * factor,
            slope_db_per_km: slope_raw as f64 * 0.001,
            loss_db: loss_raw as f64 * 0.001,
            refl_db: refl_raw as f64 * 0.001,
            event_type,
            end_of_prev_km,
            start_of_curr_km,
            end_of_curr_km,
            start_of_next_km,
            peak_km,
            comments: reader.read_cstring()?,
        })
    }
}

/// Summary characteristics of the trace.
#[derive(Debug, Clone)]
pub struct KeyEventsSummary {
    /// Total optical loss across the measured span, dB.
    pub total_loss_db: f64,
    /// Optical Return Loss (ORL) of the span, dB.
    pub orl_db: f64,
    /// Start distance for total loss calculation, km.
    pub loss_start_km: f64,
    /// End distance for total loss calculation, km.
    pub loss_end_km: f64,
    /// Start distance for ORL calculation, km.
    pub orl_start_km: f64,
    /// End distance for ORL calculation, km.
    pub orl_end_km: f64,
}

impl KeyEventsSummary {
    pub(crate) fn read_from(reader: &mut Reader, factor: f64) -> Result<KeyEventsSummary> {
        let total_loss_db = reader.read_i32_or()? as f64 * 0.001;
        let loss_start_km = reader.read_i32_or()? as f64 * factor;
        let loss_end_km = reader.read_u32_or()? as f64 * factor;
        let orl_db = reader.read_u16_or()? as f64 * 0.001;
        let orl_start_km = reader.read_i32_or()? as f64 * factor;
        let orl_end_km = reader.read_u32_or()? as f64 * factor;

        Ok(KeyEventsSummary {
            total_loss_db,
            orl_db,
            loss_start_km,
            loss_end_km,
            orl_start_km,
            orl_end_km,
        })
    }
}

/// List of trace key events.
#[derive(Debug, Clone)]
pub struct KeyEvents {
    /// List of detected key events.
    pub events: Vec<KeyEvent>,
    /// Summary of the trace characteristics.
    pub summary: KeyEventsSummary,
}

impl KeyEvents {
    pub fn num_events(&self) -> usize {
        self.events.len()
    }

    pub fn reflections(&self) -> Vec<&KeyEvent> {
        self.events.iter().filter(|e| e.is_reflection()).collect()
    }

    pub fn loss_events(&self) -> Vec<&KeyEvent> {
        self.events.iter().filter(|e| e.is_loss_event()).collect()
    }

    pub fn end_of_fiber(&self) -> Option<&KeyEvent> {
        self.events.iter().find(|e| e.is_end_of_fiber())
    }

    pub(crate) fn read_from(
        reader: &mut Reader,
        format: u8,
        fxd: Option<&FxdParams>,
    ) -> Result<Self> {
        let factor = fxd.map_or(1e-4 * SPEED_OF_LIGHT_KM_US / 1.4682, |fp| fp.distance_factor_km());
        let num_events = reader.read_u16_le()? as usize;
        let events = (0..num_events)
            .map(|_| KeyEvent::read_from(reader, format, factor))
            .collect::<Result<Vec<_>>>()?;
        let summary = KeyEventsSummary::read_from(reader, factor)?;
        Ok(Self { events, summary })
    }
}

/// Digitized OTDR trace.
#[derive(Debug, Clone)]
pub struct DataPoints {
    /// Total number of data points in the trace.
    pub num_points: u32,
    /// Number of traces (usually 1 for standard OTDR).
    pub num_traces: u16,
    /// Scaling factor for converting raw data to dB.
    pub scaling_factor: u16,
    /// Raw amplitude data array.
    pub raw_data: Vec<u16>,
    /// Distance between points (populated from FxdParams), m.
    pub resolution_m: f64,
    /// Distance of the first point from the start of the fiber under test
    /// (acquisition offset minus user offset), m. Usually negative.
    pub x_offset_m: f64,
}

impl DataPoints {
    /// Trace levels in dB.
    pub fn levels_db(&self) -> Vec<f64> {
        let scale = self.scaling_factor as f64 * 1e-6;
        self.raw_data.iter().map(|&v| v as f64 * scale).collect()
    }

    /// Distances from the start of the fiber, m.
    pub fn distances_m(&self) -> Vec<f64> {
        (0..self.num_points as usize)
            .map(|i| self.x_offset_m + i as f64 * self.resolution_m)
            .collect()
    }

    /// Distances from the start of the fiber, km.
    pub fn distances_km(&self) -> Vec<f64> {
        self.distances_m().into_iter().map(|d| d / 1000.0).collect()
    }

    /// Tuple `(distances_km, levels_db)` for plotting.
    pub fn as_arrays(&self) -> (Vec<f64>, Vec<f64>) {
        (self.distances_km(), self.levels_db())
    }

    /// Maximum level (excluding saturation 65535), dB.
    pub fn max_level_db(&self) -> f64 {
        let scale = self.scaling_factor as f64 * 1e-6;
        self.raw_data
            .iter()
            .filter(|&&v| v < 65535)
            .map(|&v| v as f64 * scale)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Minimum level (excluding saturation), dB.
    pub fn min_level_db(&self) -> f64 {
        let scale = self.scaling_factor as f64 * 1e-6;
        self.raw_data
            .iter()
            .filter(|&&v| v < 65535)
            .map(|&v| v as f64 * scale)
            .fold(f64::INFINITY, f64::min)
    }

    /// Trace dynamic range, dB.
    pub fn dynamic_range_db(&self) -> f64 {
        let max = self.max_level_db();
        let min = self.min_level_db();
        if max.is_finite() && min.is_finite() {
            max - min
        } else {
            0.0
        }
    }

    pub(crate) fn read_from(reader: &mut Reader) -> Result<Self> {
        let num_points = reader.read_u32_le()?;
        let num_traces = reader.read_u16_le()?;
        let _num_points_2 = reader.read_u32_le()?;
        let scaling_factor = reader.read_u16_le()?;

        let raw_bytes = reader.read_bytes(num_points as usize * 2)?;
        let raw_data: Vec<u16> = raw_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();

        Ok(Self {
            num_points,
            num_traces,
            scaling_factor,
            raw_data,
            resolution_m: 0.0,
            x_offset_m: 0.0,
        })
    }
}

/// File checksum (CRC-16).
#[derive(Debug, Clone, Copy)]
pub struct Checksum {
    /// Checksum algorithm identifier.
    pub algorithm: u16,
    /// Calculated checksum value.
    pub value: u16,
}

impl Checksum {
    pub(crate) fn read_from(reader: &mut Reader) -> Result<Self> {
        Ok(Self {
            algorithm: 1,
            value: reader.read_u16_le()?,
        })
    }
}

/// Contains raw bytes.
#[derive(Debug, Clone)]
pub struct RawBlock {
    /// Name identifier.
    pub name: String,
    /// Raw block payload bytes.
    pub data: Vec<u8>,
}

impl RawBlock {
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

#[derive(Debug, Default)]
pub struct SorFile {
    pub map_block: Option<MapBlock>,
    pub gen_params: Option<GenParams>,
    pub sup_params: Option<SupParams>,
    pub fxd_params: Option<FxdParams>,
    pub key_events: Option<KeyEvents>,
    pub data_points: Option<DataPoints>,
    pub checksum: Option<Checksum>,
    pub raw_blocks: HashMap<String, RawBlock>,
}
