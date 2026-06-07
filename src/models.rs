use crate::constants::SPEED_OF_LIGHT_KM_US;
use std::collections::HashMap;

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
    /// Wavelength, nm (raw * 0.1).
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
    /// True if this is a reflection event (type starts with '1').
    pub fn is_reflection(&self) -> bool {
        self.event_type.starts_with('1')
    }

    /// True if this is a loss event (type starts with '0').
    pub fn is_loss_event(&self) -> bool {
        self.event_type.starts_with('0')
    }

    /// True if this is the end of fiber (type starts with 'E' or 'e').
    pub fn is_end_of_fiber(&self) -> bool {
        self.event_type.starts_with('E') || self.event_type.starts_with('e')
    }

    /// True if the event was detected automatically.
    pub fn is_auto(&self) -> bool {
        !self.event_type.chars().nth(1).is_some_and(|c| c == 'M')
    }

    /// Event subtype as a string.
    pub fn subtype_str(&self) -> &'static str {
        match self.event_type.chars().next() {
            Some('0') => "loss/drop/gain",
            Some('1') => "reflection",
            Some('2') => "multiple events",
            Some('E') | Some('e') => "end of fiber",
            _ => "unknown",
        }
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
    /// Initial distance (front panel offset), m.
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
}

/// File checksum (CRC-16).
#[derive(Debug, Clone, Copy)]
pub struct Checksum {
    /// Checksum algorithm identifier.
    pub algorithm: u16,
    /// Calculated checksum value.
    pub value: u16,
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
