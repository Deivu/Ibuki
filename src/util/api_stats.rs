use crate::models::{ApiCpu, ApiFrameStats, ApiMemory, ApiStats};
use crate::util::frame_counter::EXPECTED_FRAMES_PER_MIN;
use crate::voice::player::{GetFrameCounter, IsActive};
use crate::ws::client::GetAllPlayers;
use crate::{SCHEDULER, START};
use perf_monitor::cpu::processor_numbers;
use std::sync::atomic::Ordering;

pub async fn get_stats() -> ApiStats {
    let cores = processor_numbers().unwrap_or(1);

    let (global_cpu, process_cpu, free, reservable, used) = {
        let mut sys = crate::SYSTEM.lock().await;

        sys.refresh_cpu_usage();

        let pid = std::process::id();

        sys.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
        );

        let cpus = sys.cpus();

        let global_cpu: f32 = if cpus.is_empty() {
            0.0
        } else {
            cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32 / 100.0
        };

        let process_cpu = if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid)) {
            process.cpu_usage() as f64 / 100.0 / cores as f64
        } else {
            0.0
        };

        sys.refresh_memory();
        let free = sys.available_memory();
        let reservable = sys.total_memory();

        let used = crate::ALLOCATOR.allocated() as u64;

        (global_cpu, process_cpu, free, reservable, used)
    };

    let process_memory_info = perf_monitor::mem::get_process_memory_info().unwrap_or_default();

    let mut player_count: u64 = 0;
    let mut total_sent: u64 = 0;
    let mut total_nulled: u64 = 0;

    for client_ref in crate::CLIENTS.iter() {
        let Ok(players) = client_ref.ask(GetAllPlayers).await else {
            continue;
        };

        for (_, player_ref) in players {
            if !player_ref.ask(IsActive).await.unwrap_or(false) {
                continue;
            }

            let Ok(counter) = player_ref.ask(GetFrameCounter).await else {
                continue;
            };

            if !counter.is_data_usable() {
                continue;
            }

            player_count += 1;
            total_sent += counter.last_sent.load(Ordering::Relaxed);
            total_nulled += counter.last_nulled.load(Ordering::Relaxed);
        }
    }

    let mut frame_stats = None;

    if player_count > 0 {
        let avg_sent = total_sent / player_count;
        let avg_nulled = total_nulled / player_count;
        let avg_deficit =
            (EXPECTED_FRAMES_PER_MIN as i64) - ((total_sent + total_nulled) / player_count) as i64;

        let _ = frame_stats.insert(ApiFrameStats {
            sent: avg_sent,
            nulled: avg_nulled as u32,
            deficit: avg_deficit as i32,
        });
    }

    ApiStats {
        players: SCHEDULER.total_tasks() as u32,
        playing_players: SCHEDULER.live_tasks() as u32,
        uptime: START.elapsed().as_millis() as u64,
        memory: ApiMemory {
            free,
            used,
            allocated: process_memory_info.resident_set_size,
            reservable,
        },
        cpu: ApiCpu {
            cores: cores as u32,
            system_load: global_cpu as f64,
            lavalink_load: process_cpu,
        },
        frame_stats,
    }
}
