//! `sim-model`: runs the traffic model (docs/testing.md, stage 1) and prints
//! what the relay carries and costs, for the default assumptions and for the
//! variations that matter most.
//!
//! Usage: `sim-model [--seed N] [--families N] [--days N]`

use std::process::ExitCode;

use zoomriya_sim_model::{Profile, Report, run};

fn main() -> ExitCode {
    let mut base = Profile::default();
    let mut seed = 1;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().and_then(|v| v.parse::<u64>().ok());
        match (arg.as_str(), value) {
            ("--seed", Some(v)) => seed = v,
            ("--families", Some(v)) if v > 0 => base.families = v as usize,
            ("--days", Some(v)) if v > 0 => base.days = v as usize,
            _ => {
                eprintln!("usage: sim-model [--seed N] [--families N] [--days N]");
                return ExitCode::from(2);
            }
        }
    }

    println!(
        "ZOOMRIYA Connect traffic model: {} families, {} days, seed {seed}",
        base.families, base.days
    );
    println!("Assumptions: docs/testing.md. Sizes are per 30 days.\n");

    let baseline = run(&base, seed);
    detail(&baseline, &base);

    println!("\nWhat changes it most");
    println!(
        "{:<40} {:>9} {:>9} {:>8} {:>9}",
        "", "relay GB", "direct", "busiest", "cost $"
    );
    let row = |name: &str, profile: &Profile| {
        let report = run(profile, seed);
        println!(
            "{:<40} {:>9.0} {:>8.1}% {:>5.0} Mb {:>9.0}",
            name,
            report.relay_gb_per_month(),
            report.direct_share() * 100.0,
            report.busiest_mbit_s(profile),
            report.cost_per_month(&profile.pricing),
        );
    };
    row("Default network mix", &base);
    for share in [0.05, 0.10, 0.25] {
        row(
            &format!("{:.0}% of connections stay on the relay", share * 100.0),
            &Profile {
                force_relay_share: Some(share),
                ..base.clone()
            },
        );
    }
    row(
        "Backups allowed on the relay",
        &Profile {
            backups_on_relay: true,
            ..base.clone()
        },
    );
    row(
        "Backups on the relay, 25% stay on it",
        &Profile {
            backups_on_relay: true,
            force_relay_share: Some(0.25),
            ..base.clone()
        },
    );
    row(
        "Twice the use away from home",
        &Profile {
            everyday_mb: base.everyday_mb * 2.0,
            video: (base.video.0, base.video.1 * 2.0),
            ..base.clone()
        },
    );
    ExitCode::SUCCESS
}

fn detail(report: &Report, profile: &Profile) {
    let per_month = |mb: f64| mb / 1000.0 * 30.0 / report.days.max(1) as f64;
    println!(
        "Relay traffic                       {:>8.0} GB",
        report.relay_gb_per_month()
    );
    println!(
        "  family, staying on the relay      {:>8.0} GB",
        per_month(report.family_relayed)
    );
    println!(
        "  first seconds of each connection  {:>8.0} GB",
        per_month(report.relay_starts)
    );
    println!(
        "  guests' share links               {:>8.0} GB",
        per_month(report.guests_relayed)
    );
    println!(
        "  backups                           {:>8.0} GB",
        per_month(report.backups_relayed)
    );
    println!(
        "Days away that stayed on the relay  {:>8.1} %",
        report.relayed_day_share() * 100.0
    );
    println!(
        "Family traffic away that was direct {:>8.1} %",
        report.direct_share() * 100.0
    );
    println!(
        "Busiest hour                        {:>8.0} Mbit/s",
        report.busiest_mbit_s(profile)
    );
    println!(
        "Endpoints on the relay at once      {:>8.0} ({} boxes, {} devices)",
        report.connected_endpoints(profile),
        report.families,
        report.devices
    );
    println!(
        "Per family per month               median {:.1} GB, 90% {:.1} GB, 99% {:.1} GB, most {:.1} GB",
        report.family_month_gb(50.0),
        report.family_month_gb(90.0),
        report.family_month_gb(99.0),
        report.family_month_gb(100.0)
    );
    for limit in [20.0, 50.0, 100.0] {
        println!(
            "  family-months over {limit:>3.0} GB          {:>8}",
            report.months_over(limit)
        );
    }
    println!(
        "Cost on {} relay servers            {:>8.0} $ a month",
        profile.pricing.servers,
        report.cost_per_month(&profile.pricing)
    );
}
