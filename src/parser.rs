use crate::checksum::crc16;
use crate::errors::{Result, SorError};
use crate::models::{
    BlockInfo, Checksum, DataPoints, FxdParams, GenParams, KeyEvents, MapBlock, RawBlock, SorFile,
    SupParams,
};
use crate::reader::Reader;

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

#[derive(Default)]
struct FxdTail {
    averaging_time_s: f64,
    acquisition_range_raw: u32,
    acquisition_range_coeff: i32,
    front_panel_offset: i32,
    noise_floor_level: u16,
    noise_floor_scaling: i16,
    power_offset_first_point: u16,
    loss_threshold_db: f64,
    refl_threshold_db: f64,
    eof_threshold_db: f64,
    trace_type: String,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl FxdTail {
    fn read_v2(reader: &mut Reader) -> Result<Self> {
        Ok(Self {
            averaging_time_s: reader.read_u16_le()? as f64 * 0.1,
            acquisition_range_raw: reader.read_u32_le()?,
            acquisition_range_coeff: reader.read_i32_le()?,
            front_panel_offset: reader.read_i32_le()?,
            noise_floor_level: reader.read_u16_le()?,
            noise_floor_scaling: reader.read_i16_le()?,
            power_offset_first_point: reader.read_u16_le()?,
            loss_threshold_db: reader.read_u16_le()? as f64 * 0.001,
            refl_threshold_db: reader.read_u16_le()? as f64 * -0.001,
            eof_threshold_db: reader.read_u16_le()? as f64 * 0.001,
            trace_type: reader.read_fixed_str(2)?.replace('\0', ""),
            x1: reader.read_i32_or()?,
            y1: reader.read_i32_or()?,
            x2: reader.read_i32_or()?,
            y2: reader.read_i32_or()?,
        })
    }

    fn read_v1(reader: &mut Reader) -> Result<Self> {
        let tt = if reader.remaining() >= 2 {
            reader.read_fixed_str(2)?.replace('\0', "")
        } else {
            "ST".to_string()
        };
        Ok(Self {
            acquisition_range_raw: reader.read_u32_le()?,
            front_panel_offset: reader.read_i32_or()?,
            noise_floor_level: reader.read_u16_or()?,
            noise_floor_scaling: reader.read_i16_or()?,
            power_offset_first_point: reader.read_u16_or()?,
            loss_threshold_db: reader.read_u16_or()? as f64 * 0.001,
            refl_threshold_db: reader.read_u16_or()? as f64 * -0.001,
            eof_threshold_db: reader.read_u16_or()? as f64 * 0.001,
            trace_type: tt,
            ..Default::default()
        })
    }
}

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

        let (map, format) = MapBlock::read_from(&mut Reader::new(self.data))?;
        self.format = format;
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

    fn parse_block(&self, info: &BlockInfo, sor: &mut SorFile) -> Result<()> {
        let mut reader = self.block_reader(info)?;
        match info.name.as_str() {
            "GenParams" => sor.gen_params = Some(GenParams::read_from(&mut reader, self.format)?),
            "SupParams" => sor.sup_params = Some(SupParams::read_from(&mut reader)?),
            "FxdParams" => sor.fxd_params = Some(self.parse_fxd_params(&mut reader)?),
            "KeyEvents" => {
                sor.key_events =
                    Some(KeyEvents::read_from(&mut reader, self.format, sor.fxd_params.as_ref())?)
            }
            "DataPts" => sor.data_points = Some(DataPoints::read_from(&mut reader)?),
            "Cksum" => sor.checksum = Some(Checksum::read_from(&mut reader)?),
            _ => {
                let data = reader.read_bytes(reader.remaining())?.to_vec();
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

    fn block_reader(&self, info: &BlockInfo) -> Result<Reader<'a>> {
        let name_size = info.name.len() + 1;
        Reader::with_bounds(
            self.data,
            info.offset + name_size,
            (info.size as usize).saturating_sub(name_size),
        )
    }

    fn parse_fxd_params(&self, reader: &mut Reader) -> Result<FxdParams> {
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
        let pulse_widths_ns = (0..num_pw)
            .map(|_| reader.read_u16_le())
            .collect::<Result<Vec<_>>>()?;

        let sample_spacing_raw = reader.read_u32_le()?;
        let num_data_points = reader.read_u32_le()?;
        let group_index = reader.read_u32_le()? as f64 * 1e-5;
        let backscatter_coeff_db = reader.read_u16_le()? as f64 * -0.1;
        let num_averages = reader.read_u32_le()?;

        let tail = if self.format == 2 {
            FxdTail::read_v2(reader)?
        } else {
            FxdTail::read_v1(reader)?
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
            averaging_time_s: tail.averaging_time_s,
            acquisition_range_raw: tail.acquisition_range_raw,
            acquisition_range_coeff: tail.acquisition_range_coeff,
            front_panel_offset: tail.front_panel_offset,
            noise_floor_level: tail.noise_floor_level,
            noise_floor_scaling: tail.noise_floor_scaling,
            power_offset_first_point: tail.power_offset_first_point,
            loss_threshold_db: tail.loss_threshold_db,
            refl_threshold_db: tail.refl_threshold_db,
            eof_threshold_db: tail.eof_threshold_db,
            trace_type: tail.trace_type,
            x1: tail.x1,
            y1: tail.y1,
            x2: tail.x2,
            y2: tail.y2,
        })
    }

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

    fn link_data_points(&self, sor: &mut SorFile) {
        if let (Some(dp), Some(fp)) = (&mut sor.data_points, &sor.fxd_params) {
            let user_offset = sor.gen_params.as_ref().map_or(0, |gp| gp.user_offset_raw);
            let offset_units = f64::from(fp.acquisition_offset) - f64::from(user_offset);
            dp.resolution_m = fp.resolution_m();
            dp.x_offset_m = offset_units * fp.distance_factor_km() * 1000.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use crate::errors::SorError;
    use crate::models::SorFile;

    fn sample(name: &str) -> String {
        format!("{}/data/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn strongest_reflection_km(sor: &SorFile, near_km: f64, window_km: f64) -> f64 {
        let dp = sor.data_points.as_ref().unwrap();
        let (distances, levels) = dp.as_arrays();
        distances
            .iter()
            .zip(&levels)
            .filter(|(d, _)| (**d - near_km).abs() < window_km)
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(d, _)| *d)
            .unwrap()
    }

    #[fixture]
    fn noyes() -> String {
        sample("example1-noyes-ofl280.sor")
    }

    #[fixture]
    fn noyes_fast() -> String {
        sample("example1-noyes-ofl280-fastreporter-save.sor")
    }

    #[fixture]
    fn exfo_max() -> String {
        sample("example2-exfo-maxtester730c.sor")
    }

    #[fixture]
    fn anritsu() -> String {
        sample("example3-anritsu-accessmastermt9085.sor")
    }

    #[fixture]
    fn exfo_1310() -> String {
        sample("example4-exfo-ftb4ftbx730c-mfdgainer-1310nm.sor")
    }

    #[fixture]
    fn exfo_1550() -> String {
        sample("example4-exfo-ftb4ftbx730c-mfdgainer-1550nm.sor")
    }

    #[fixture]
    fn exfo_rtu() -> String {
        sample("example5-exfo-rtu2ftbx735c-sm7r-ea-hrd.sor")
    }

    #[fixture]
    fn sor_file(#[from(noyes)] path: String) -> SorFile {
        SorFile::from_file(&path, false).unwrap()
    }

    #[fixture]
    fn all_files(
        noyes: String,
        noyes_fast: String,
        exfo_max: String,
        anritsu: String,
        exfo_1310: String,
        exfo_1550: String,
        exfo_rtu: String,
    ) -> Vec<String> {
        vec![
            noyes, noyes_fast, exfo_max, anritsu, exfo_1310, exfo_1550, exfo_rtu,
        ]
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
    fn parse_without_error(all_files: Vec<String>) {
        for path in &all_files {
            SorFile::from_file(path, false)
                .unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
        }
    }

    #[rstest]
    fn from_bytes(all_files: Vec<String>) {
        for path in &all_files {
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
        for key in &[
            "GenParams",
            "SupParams",
            "FxdParams",
            "KeyEvents",
            "DataPts",
            "Cksum",
        ] {
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
    fn map_all_files(all_files: Vec<String>) {
        for path in &all_files {
            let sor = SorFile::from_file(path, false).unwrap();
            assert!(sor.map_block.as_ref().unwrap().num_blocks() > 0);
        }
    }

    #[rstest]
    fn gen_params_noyes(sor_file: SorFile) {
        let gp = sor_file.gen_params.as_ref().unwrap();
        assert_eq!(gp.language, "EN");
        assert!(gp.fiber_type_code > 0);
        let wl = gp.wavelength_nm;
        assert!((1000.0..=1700.0).contains(&wl), "Unrealistic wavelength: {wl}");
    }

    #[rstest]
    fn gen_params_fiber_type_str(sor_file: SorFile) {
        let gp = sor_file.gen_params.as_ref().unwrap();
        assert!(gp.fiber_type_str().contains("G.652"));
    }

    #[rstest]
    fn gen_params_all_files(all_files: Vec<String>) {
        for path in &all_files {
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
    fn sup_params_all_files(all_files: Vec<String>) {
        for path in &all_files {
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
    fn fxd_params_resolution_realistic(all_files: Vec<String>) {
        for path in &all_files {
            let sor = SorFile::from_file(path, false).unwrap();
            let fp = sor.fxd_params.as_ref().unwrap();
            let r = fp.resolution_m();
            assert!(r > 0.0 && r < 100.0, "Unrealistic resolution {r:.4} in {path}");
        }
    }

    #[rstest]
    fn fxd_params_group_index_realistic(all_files: Vec<String>) {
        for path in &all_files {
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
    fn key_events_distances_non_negative(all_files: Vec<String>) {
        for path in &all_files {
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
    fn key_events_summary_orl_realistic(all_files: Vec<String>) {
        for path in &all_files {
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
        assert_eq!(ke.reflections().len(), 2);
        assert_eq!(ke.loss_events().len(), 1);
    }

    #[rstest]
    #[case::noyes(sample("example1-noyes-ofl280.sor"), 3.734)]
    #[case::exfo_ghosts_after_end(sample("example2-exfo-maxtester730c.sor"), 3.739)]
    #[case::anritsu(sample("example3-anritsu-accessmastermt9085.sor"), 7.985)]
    #[case::exfo_1310(sample("example4-exfo-ftb4ftbx730c-mfdgainer-1310nm.sor"), 3.629)]
    fn end_of_fiber_is_found(#[case] path: String, #[case] expected_km: f64) {
        let sor = SorFile::from_file(&path, false).unwrap();
        let end = sor
            .key_events
            .as_ref()
            .unwrap()
            .end_of_fiber()
            .expect("no end of fiber");
        assert!((end.distance_km - expected_km).abs() < 0.001, "end at {} km", end.distance_km);
        assert_eq!(end.subtype_str(), "end of fiber");
    }

    #[rstest]
    fn saturated_reflection_is_reflective(sor_file: SorFile) {
        let end = sor_file.key_events.as_ref().unwrap().events.last().unwrap();
        assert!(end.is_saturated());
        assert!(end.is_reflection());
    }

    #[rstest]
    fn key_events_all_files_positive_count(all_files: Vec<String>) {
        for path in &all_files {
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
    fn data_points_noyes_offset(sor_file: SorFile) {
        let dp = sor_file.data_points.as_ref().unwrap();
        assert!((dp.x_offset_m - -547.25).abs() < 0.5, "x_offset_m = {}", dp.x_offset_m);
    }

    #[rstest]
    #[case::noyes_with_user_offset(sample("example1-noyes-ofl280.sor"), 3.734)]
    #[case::exfo_with_user_offset(sample("example4-exfo-ftb4ftbx730c-mfdgainer-1310nm.sor"), 3.629)]
    #[case::exfo_without_offsets(sample("example2-exfo-maxtester730c.sor"), 3.739)]
    fn reflection_peak_matches_event_distance(#[case] path: String, #[case] event_km: f64) {
        let sor = SorFile::from_file(&path, false).unwrap();
        let peak_km = strongest_reflection_km(&sor, event_km, 0.3);
        assert!(
            (peak_km - event_km).abs() < 0.005,
            "peak at {peak_km:.4} km, event at {event_km} km"
        );
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
    fn data_points_all_files(all_files: Vec<String>) {
        for path in &all_files {
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
        let has_exfo = sor.raw_blocks.keys().any(|k| k.contains("Exfo"));
        assert!(has_exfo, "Expected EXFO proprietary blocks: {:?}", sor.raw_blocks.keys());
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
}
