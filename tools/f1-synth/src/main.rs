//! `f1-synth` — emits a synthetic 22-car race as real F1 UDP telemetry.
//!
//! ```text
//! f1-synth --format 2024 --speed 5 --laps 15
//! f1-synth --format 2022 --record captures/synth.bin --speed 0
//! ```

mod sim;
mod track;

use anyhow::Context;
use clap::Parser;
use f1_udp::{capture::Recorder, encode, Format};
use sim::{Sim, SimConfig};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(version, about = "Synthetic F1 UDP telemetry generator")]
struct Args {
    /// UDP format to emit: 2022, 2023, 2024 or 2025.
    #[arg(long, default_value = "2024")]
    format: Format,
    /// Destination for UDP datagrams.
    #[arg(long, default_value = "127.0.0.1:20777")]
    target: String,
    /// Packet send rate in Hz (in-game options are 10/15/20/30/60).
    #[arg(long, default_value_t = 20.0)]
    rate: f32,
    /// Simulation speed multiplier. 0 = as fast as possible.
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
    /// Race distance in laps.
    #[arg(long, default_value_t = 15)]
    laps: u8,
    /// Player grid slot (0-based).
    #[arg(long, default_value_t = 7)]
    player_grid: usize,
    /// Track length in metres.
    #[arg(long, default_value_t = 5200.0)]
    track_length: f32,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Also (or only, with --no-send) write a capture file.
    #[arg(long)]
    record: Option<std::path::PathBuf>,
    /// Do not send UDP (useful with --record).
    #[arg(long)]
    no_send: bool,
    /// Restart with a new session when the race ends.
    #[arg(long, name = "loop")]
    repeat: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let socket = if args.no_send {
        None
    } else {
        let s = UdpSocket::bind("0.0.0.0:0").context("bind UDP socket")?;
        s.connect(&args.target).with_context(|| format!("connect to {}", args.target))?;
        Some(s)
    };
    let mut recorder = match &args.record {
        Some(p) => Some(Recorder::create(p, args.format.packet_format()).context("create capture file")?),
        None => None,
    };
    let dt = 1.0 / args.rate.max(1.0);
    let mut seed = args.seed;
    loop {
        eprintln!(
            "f1-synth: format {} | {} laps | {} Hz | speed {}x | -> {}",
            args.format.packet_format(),
            args.laps,
            args.rate,
            args.speed,
            if args.no_send { "(no send)".to_string() } else { args.target.clone() }
        );
        let mut sim = Sim::new(SimConfig {
            format: args.format,
            laps: args.laps,
            player_grid: args.player_grid,
            seed,
            rate_hz: args.rate,
            track_length_m: args.track_length,
        });
        let start = Instant::now();
        let mut last_report = 0u32;
        while !sim.done {
            let packets = sim.step(dt);
            let header = sim.header();
            for p in &packets {
                let bytes = encode(args.format, &header, p);
                if let Some(s) = &socket {
                    // A missing listener yields ICMP "connection refused" on some OSes; ignore.
                    let _ = s.send(&bytes);
                }
                if let Some(r) = recorder.as_mut() {
                    let t_us = if args.speed > 0.0 { start.elapsed().as_micros() as u64 } else { (sim.session_time() as f64 * 1e6) as u64 };
                    r.record_at(t_us, &bytes)?;
                }
            }
            let minute = (sim.session_time() / 60.0) as u32;
            if minute != last_report {
                last_report = minute;
                eprintln!("  session time {minute} min");
            }
            if args.speed > 0.0 {
                let due = Duration::from_secs_f32(sim.session_time() / args.speed);
                if let Some(wait) = due.checked_sub(start.elapsed()) {
                    std::thread::sleep(wait);
                }
            }
        }
        eprintln!("f1-synth: race finished");
        if !args.repeat {
            break;
        }
        seed = seed.wrapping_add(1);
        std::thread::sleep(Duration::from_secs(3));
    }
    if let Some(mut r) = recorder {
        r.flush()?;
        eprintln!("f1-synth: wrote {} records", r.records());
    }
    Ok(())
}
