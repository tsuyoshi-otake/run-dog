//! GPU utilisation and video-memory snapshots.
//!
//! Windows exposes these as PDH counters (`GPU Engine` and `GPU Adapter Memory`).
//! One query is opened at startup and collected on the existing 2-second CPU
//! tick, so there is no extra thread.

use std::{collections::HashMap, ptr};

use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

use crate::core::GpuStatus;

const ENGINE: &str = r"\GPU Engine(*)\Utilization Percentage";
const DEDICATED_USAGE: &str = r"\GPU Adapter Memory(*)\Dedicated Usage";
const DEDICATED_LIMIT: &str = r"\GPU Adapter Memory(*)\Dedicated Limit";
const SHARED_USAGE: &str = r"\GPU Adapter Memory(*)\Shared Usage";
const SHARED_LIMIT: &str = r"\GPU Adapter Memory(*)\Shared Limit";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct AdapterId {
    high: i32,
    low: u32,
}

pub struct GpuSampler {
    query: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    dedicated_usage: PDH_HCOUNTER,
    dedicated_limit: PDH_HCOUNTER,
    shared_usage: PDH_HCOUNTER,
    shared_limit: PDH_HCOUNTER,
    primed: bool,
}

impl GpuSampler {
    #[must_use]
    pub fn new() -> Self {
        let mut query = ptr::null_mut();
        if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
            return Self::disabled();
        }
        let engine = add_counter(query, ENGINE);
        let dedicated_usage = add_counter(query, DEDICATED_USAGE);
        let dedicated_limit = add_counter(query, DEDICATED_LIMIT);
        let shared_usage = add_counter(query, SHARED_USAGE);
        let shared_limit = add_counter(query, SHARED_LIMIT);
        if engine.is_null()
            && dedicated_usage.is_null()
            && dedicated_limit.is_null()
            && shared_usage.is_null()
            && shared_limit.is_null()
        {
            let _ = unsafe { PdhCloseQuery(query) };
            return Self::disabled();
        }
        Self {
            query,
            engine,
            dedicated_usage,
            dedicated_limit,
            shared_usage,
            shared_limit,
            primed: false,
        }
    }

    const fn disabled() -> Self {
        Self {
            query: ptr::null_mut(),
            engine: ptr::null_mut(),
            dedicated_usage: ptr::null_mut(),
            dedicated_limit: ptr::null_mut(),
            shared_usage: ptr::null_mut(),
            shared_limit: ptr::null_mut(),
            primed: false,
        }
    }

    #[must_use]
    pub fn sample(&mut self) -> Option<GpuStatus> {
        if self.query.is_null() {
            return None;
        }
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }
        if !self.primed {
            self.primed = true;
            return None;
        }

        let dedicated_limit = counter_map(self.dedicated_limit);
        let shared_limit = counter_map(self.shared_limit);
        let dedicated_usage = counter_map(self.dedicated_usage);
        let shared_usage = counter_map(self.shared_usage);
        let engines = counter_items(self.engine);
        let mut utilization = HashMap::new();
        let mut phys = HashMap::new();
        for (name, _) in engines
            .iter()
            .chain(counter_items(self.dedicated_usage).iter())
            .chain(counter_items(self.shared_usage).iter())
            .chain(counter_items(self.dedicated_limit).iter())
        {
            if let Some(id) = parse_instance_luid(name) {
                if let Some(index) = parse_phys_index(name) {
                    phys.entry(id).or_insert(index);
                }
            }
        }
        for id in adapter_ids(
            &dedicated_limit,
            &shared_limit,
            &dedicated_usage,
            &shared_usage,
            &engines,
        ) {
            if let Some(value) = adapter_engine_utilization(&engines, id) {
                utilization.insert(id, value);
            }
        }
        let adapter = select_adapter(
            &dedicated_limit,
            &shared_limit,
            &dedicated_usage,
            &shared_usage,
            &utilization,
            &phys,
        )?;
        Some(
            GpuStatus::new(
                value_for(&dedicated_limit, adapter),
                value_for(&dedicated_usage, adapter),
                value_for(&shared_limit, adapter),
                value_for(&shared_usage, adapter),
            )
            .with_utilization(utilization.get(&adapter).copied()),
        )
    }
}

impl Default for GpuSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        if !self.query.is_null() {
            let _ = unsafe { PdhCloseQuery(self.query) };
            self.query = ptr::null_mut();
        }
    }
}

fn add_counter(query: PDH_HQUERY, path: &str) -> PDH_HCOUNTER {
    let mut counter = ptr::null_mut();
    let wide = wide(path);
    let status = unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) };
    if status == 0 {
        counter
    } else {
        ptr::null_mut()
    }
}

fn counter_map(counter: PDH_HCOUNTER) -> HashMap<AdapterId, u64> {
    let mut map = HashMap::new();
    for (name, value) in counter_items(counter) {
        let Some(id) = parse_instance_luid(&name) else {
            continue;
        };
        map.insert(id, value.max(0.0).round() as u64);
    }
    map
}

fn counter_items(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    if counter.is_null() {
        return Vec::new();
    }
    let mut buffer_size = 0_u32;
    let mut item_count = 0_u32;
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut buffer_size,
            &mut item_count,
            ptr::null_mut(),
        )
    };
    if status != PDH_MORE_DATA && status != 0 {
        return Vec::new();
    }
    if buffer_size == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0_u8; buffer_size as usize];
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut buffer_size,
            &mut item_count,
            buffer.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    let items = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            item_count as usize,
        )
    };
    items
        .iter()
        .filter_map(|item| {
            if item.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA
                && item.FmtValue.CStatus != PDH_CSTATUS_NEW_DATA
            {
                return None;
            }
            let name = pwstr_to_string(item.szName)?;
            Some((name, unsafe { item.FmtValue.Anonymous.doubleValue }))
        })
        .collect()
}

fn adapter_ids(
    dedicated_limit: &HashMap<AdapterId, u64>,
    shared_limit: &HashMap<AdapterId, u64>,
    dedicated_usage: &HashMap<AdapterId, u64>,
    shared_usage: &HashMap<AdapterId, u64>,
    engines: &[(String, f64)],
) -> Vec<AdapterId> {
    let mut ids: Vec<AdapterId> = dedicated_limit.keys().copied().collect();
    ids.extend(shared_limit.keys().copied());
    ids.extend(dedicated_usage.keys().copied());
    ids.extend(shared_usage.keys().copied());
    ids.extend(
        engines
            .iter()
            .filter_map(|(name, _)| parse_instance_luid(name)),
    );
    ids.sort_by_key(|id| (id.high, id.low));
    ids.dedup();
    ids
}

fn select_adapter(
    dedicated_limit: &HashMap<AdapterId, u64>,
    shared_limit: &HashMap<AdapterId, u64>,
    dedicated_usage: &HashMap<AdapterId, u64>,
    shared_usage: &HashMap<AdapterId, u64>,
    utilization: &HashMap<AdapterId, f32>,
    phys: &HashMap<AdapterId, u32>,
) -> Option<AdapterId> {
    let mut ids = adapter_ids(
        dedicated_limit,
        shared_limit,
        dedicated_usage,
        shared_usage,
        &[],
    );
    ids.extend(utilization.keys().copied());
    ids.sort_by_key(|id| (id.high, id.low));
    ids.dedup();
    ids.retain(|id| {
        dedicated_limit.get(id).copied().unwrap_or(0) > 0
            || shared_limit.get(id).copied().unwrap_or(0) > 0
            || dedicated_usage.get(id).copied().unwrap_or(0) > 0
            || shared_usage.get(id).copied().unwrap_or(0) > 0
            || utilization.contains_key(id)
    });
    if ids.is_empty() {
        return None;
    }

    const BUSY_PERCENT: f32 = 8.0;
    let busy = ids
        .iter()
        .copied()
        .filter_map(|id| utilization.get(&id).copied().map(|value| (id, value)))
        .filter(|(_, value)| *value >= BUSY_PERCENT)
        .max_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some((id, _)) = busy {
        return Some(id);
    }

    ids.into_iter().min_by_key(|id| {
        let usage = dedicated_usage.get(id).copied().unwrap_or(0)
            + shared_usage.get(id).copied().unwrap_or(0);
        let dedicated = dedicated_limit.get(id).copied().unwrap_or(0);
        let index = phys.get(id).copied().unwrap_or(u32::MAX);
        (std::cmp::Reverse(usage), dedicated, index, id.high, id.low)
    })
}

fn value_for(map: &HashMap<AdapterId, u64>, id: AdapterId) -> u64 {
    map.get(&id).copied().unwrap_or(0)
}

fn adapter_engine_utilization(items: &[(String, f64)], id: AdapterId) -> Option<f32> {
    let mut by_engine: HashMap<String, f64> = HashMap::new();
    for (name, value) in items {
        if parse_instance_luid(name) != Some(id) {
            continue;
        }
        let Some(engine) = engine_type(name) else {
            continue;
        };
        if engine.eq_ignore_ascii_case("Idle") {
            continue;
        }
        *by_engine.entry(engine).or_insert(0.0) += *value;
    }
    if by_engine.is_empty() {
        return None;
    }
    Some(
        by_engine
            .values()
            .copied()
            .map(|value| value.clamp(0.0, 100.0))
            .fold(0.0, f64::max) as f32,
    )
}

fn parse_instance_luid(name: &str) -> Option<AdapterId> {
    let rest = name.split("luid_").nth(1)?;
    let mut parts = rest.split('_');
    let high = parse_hex_i32(parts.next()?)?;
    let low = parse_hex_u32(parts.next()?)?;
    Some(AdapterId { high, low })
}

fn engine_type(name: &str) -> Option<String> {
    name.split("engtype_")
        .nth(1)
        .map(|value| value.split(['/', '\\']).next().unwrap_or(value).to_owned())
}

fn parse_phys_index(name: &str) -> Option<u32> {
    name.split("phys_").nth(1)?.split('_').next()?.parse().ok()
}

fn parse_hex_u32(value: &str) -> Option<u32> {
    u32::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

fn parse_hex_i32(value: &str) -> Option<i32> {
    i32::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

fn pwstr_to_string(ptr: windows_sys::core::PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
            ptr, len,
        )))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        adapter_engine_utilization, parse_instance_luid, parse_phys_index, select_adapter,
        AdapterId,
    };
    use std::collections::HashMap;

    #[test]
    fn component_pdh_instance_luid_and_max_engine_are_parsed() {
        let id = AdapterId {
            high: 0,
            low: 0x0001_7a41,
        };
        assert_eq!(
            parse_instance_luid("pid_12_luid_0x00000000_0x00017A41_phys_0_eng_0_engtype_3D"),
            Some(id)
        );
        let items = [
            (
                "pid_1_luid_0x00000000_0x00017A41_phys_0_eng_0_engtype_3D".to_owned(),
                40.0,
            ),
            (
                "pid_2_luid_0x00000000_0x00017A41_phys_0_eng_0_engtype_3D".to_owned(),
                25.0,
            ),
            (
                "pid_2_luid_0x00000000_0x00017A41_phys_0_eng_1_engtype_Copy".to_owned(),
                90.0,
            ),
            (
                "pid_2_luid_0x00000000_0xDEADBEEF_phys_0_eng_0_engtype_3D".to_owned(),
                100.0,
            ),
        ];
        assert_eq!(
            parse_phys_index("pid_12_luid_0x00000000_0x00017A41_phys_0_eng_0_engtype_3D"),
            Some(0)
        );
        assert_eq!(adapter_engine_utilization(&items, id), Some(90.0));
    }

    #[test]
    fn component_adapter_choice_prefers_integrated_gpu_when_idle() {
        let discrete = AdapterId { high: 0, low: 2 };
        let integrated = AdapterId { high: 0, low: 1 };
        let mut dedicated = HashMap::new();
        dedicated.insert(discrete, 8 << 30);
        dedicated.insert(integrated, 128 << 20);
        let mut shared = HashMap::new();
        shared.insert(discrete, 16 << 30);
        shared.insert(integrated, 16 << 30);
        let utilization = HashMap::new();
        let mut phys = HashMap::new();
        phys.insert(integrated, 0);
        phys.insert(discrete, 1);
        let usage = HashMap::new();
        assert_eq!(
            select_adapter(&dedicated, &shared, &usage, &usage, &utilization, &phys),
            Some(integrated)
        );
    }

    #[test]
    fn component_adapter_choice_follows_the_busy_gpu() {
        let discrete = AdapterId { high: 0, low: 2 };
        let integrated = AdapterId { high: 0, low: 1 };
        let mut dedicated = HashMap::new();
        dedicated.insert(discrete, 8 << 30);
        dedicated.insert(integrated, 128 << 20);
        let mut shared = HashMap::new();
        shared.insert(discrete, 16 << 30);
        shared.insert(integrated, 16 << 30);
        let mut utilization = HashMap::new();
        utilization.insert(integrated, 4.0);
        utilization.insert(discrete, 72.0);
        let phys = HashMap::new();
        let usage = HashMap::new();
        assert_eq!(
            select_adapter(&dedicated, &shared, &usage, &usage, &utilization, &phys),
            Some(discrete)
        );
    }

    #[test]
    fn component_adapter_choice_prefers_the_gpu_holding_memory() {
        let discrete = AdapterId {
            high: 0,
            low: 0x949E,
        };
        let integrated = AdapterId {
            high: 0,
            low: 0x90EC,
        };
        let limits = HashMap::new();
        let dedicated_usage = HashMap::new();
        let mut shared_usage = HashMap::new();
        shared_usage.insert(integrated, 834_043_904);
        shared_usage.insert(discrete, 8192);
        let utilization = HashMap::new();
        let phys = HashMap::new();
        assert_eq!(
            select_adapter(
                &limits,
                &limits,
                &dedicated_usage,
                &shared_usage,
                &utilization,
                &phys
            ),
            Some(integrated)
        );
    }
}
