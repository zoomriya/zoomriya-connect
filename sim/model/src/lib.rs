//! The traffic model: stage 1 of the scale test (docs/testing.md).
//!
//! It follows 1,000 families, day by day, for 60 days: where each member is,
//! which network they're on, whether their connection to the box goes direct
//! or stays on the relay, and what they use. It reports what the relay
//! carries and costs, and how that changes when the assumptions do.
//!
//! The figures in [`Profile::default`] are guesses, the same ones as in
//! docs/testing.md. Measurements from the network lab and from real homes
//! replace them. Every run is repeatable from its seed.

/// A small, fast, repeatable random number generator (SplitMix64).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator that always gives the same numbers for the same seed.
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from 0 up to (not including) 1.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// One value from a weighted table; `fallback` if the table is empty.
    pub fn pick<T: Copy>(&mut self, table: &[(T, f64)], fallback: T) -> T {
        let total: f64 = table.iter().map(|(_, w)| w).sum();
        let mut x = self.unit() * total;
        for &(value, weight) in table {
            if x < weight {
                return value;
            }
            x -= weight;
        }
        table.last().map_or(fallback, |&(value, _)| value)
    }
}

/// The home's internet connection, as the box sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeNet {
    /// A router with easy NAT.
    EasyNat,
    /// A router with symmetric NAT.
    SymmetricNat,
    /// The provider's CGNAT, easy.
    CgnatEasy,
    /// The provider's CGNAT, symmetric.
    CgnatSymmetric,
    /// The provider's gateway and the family's own router.
    DoubleNat,
}

impl HomeNet {
    fn symmetric(self) -> bool {
        matches!(self, Self::SymmetricNat | Self::CgnatSymmetric)
    }

    /// The box can open port 443 on the router for guests (not behind CGNAT).
    fn can_open_port(self) -> bool {
        matches!(self, Self::EasyNat | Self::SymmetricNat)
    }
}

/// Where a member is when away from home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayNet {
    /// Mobile data: CGNAT, often symmetric, often IPv6.
    Mobile,
    /// Someone else's Wi-Fi.
    OtherWifi,
    /// An office or hotel network that blocks UDP.
    UdpBlocked,
}

/// The assumptions. The defaults are the figures in docs/testing.md.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Number of families (each with one box).
    pub families: usize,
    /// Days simulated.
    pub days: usize,
    /// Members per family, with weights.
    pub members: Vec<(u32, f64)>,
    /// Paired phones and laptops per member.
    pub devices_per_member: f64,
    /// The homes' networks, with weights.
    pub home_nets: Vec<(HomeNet, f64)>,
    /// Share of homes with working IPv6.
    pub home_ipv6: f64,
    /// Where members are while away, with weights.
    pub away_nets: Vec<(AwayNet, f64)>,
    /// Share of mobile networks with IPv6.
    pub mobile_ipv6: f64,
    /// Share of other Wi-Fi networks with IPv6.
    pub wifi_ipv6: f64,
    /// Share of mobile networks with symmetric NAT.
    pub mobile_symmetric: f64,
    /// Share of other Wi-Fi networks with symmetric NAT.
    pub wifi_symmetric: f64,
    /// Chance that an IPv6 path works when both ends have IPv6.
    pub ipv6_direct: f64,
    /// Chance that hole punching works when neither end is symmetric.
    pub punch_both_easy: f64,
    /// Chance that hole punching works when one end is symmetric.
    pub punch_one_symmetric: f64,
    /// Hole punching through two NATs at home, relative to one.
    pub double_nat_factor: f64,
    /// If set, every connection stays on the relay with this probability,
    /// instead of the network model above.
    pub force_relay_share: Option<f64>,
    /// Hours away on a weekday, and on a weekend day.
    pub away_hours: (f64, f64),
    /// Connections opened per weekday away (app opened, network changed).
    pub connections_per_day: f64,
    /// What a connection's first seconds carry through the relay, in MB,
    /// before it goes direct.
    pub relay_start_mb: f64,
    /// Chat, tracker and photo browsing per weekday away, in MB.
    pub everyday_mb: f64,
    /// A video watched away every this many days, of this many MB.
    pub video: (u32, f64),
    /// Photos and videos taken per member per day, in MB.
    pub captured_mb: f64,
    /// Share of photos and videos taken while away.
    pub captured_away: f64,
    /// Backups may go through the relay (docs: they wait for a direct path).
    pub backups_on_relay: bool,
    /// Albums shared per family per month, guests per share, MB per guest.
    pub shares: (f64, f64, f64),
    /// Share of guests on mobile data (the rest on Wi-Fi).
    pub guest_mobile: f64,
    /// Share of the day's relay traffic in its busiest hour.
    pub busiest_hour_share: f64,
    /// Share of paired devices holding a relay connection at the busiest time.
    pub devices_connected: f64,
    /// Relay servers, the traffic each includes (GB a month), its monthly
    /// price, and the price per GB beyond that (DigitalOcean, 2026-11).
    pub pricing: Pricing,
}

/// What the relay servers cost.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    /// Relay servers (at least two, in two regions).
    pub servers: u32,
    /// Outgoing traffic included per server per month, in GB.
    pub included_gb: f64,
    /// Price per server per month, in dollars.
    pub server_price: f64,
    /// Price per GB beyond the included traffic, in dollars.
    pub overage_per_gb: f64,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            families: 1000,
            days: 60,
            members: vec![(2, 0.20), (3, 0.35), (4, 0.25), (5, 0.12), (6, 0.08)],
            devices_per_member: 1.5,
            home_nets: vec![
                (HomeNet::EasyNat, 0.55),
                (HomeNet::SymmetricNat, 0.10),
                (HomeNet::CgnatEasy, 0.15),
                (HomeNet::CgnatSymmetric, 0.10),
                (HomeNet::DoubleNat, 0.10),
            ],
            home_ipv6: 0.40,
            away_nets: vec![
                (AwayNet::Mobile, 0.60),
                (AwayNet::OtherWifi, 0.30),
                (AwayNet::UdpBlocked, 0.10),
            ],
            mobile_ipv6: 0.70,
            wifi_ipv6: 0.30,
            mobile_symmetric: 0.50,
            wifi_symmetric: 0.15,
            ipv6_direct: 0.90,
            punch_both_easy: 0.97,
            punch_one_symmetric: 0.50,
            double_nat_factor: 0.90,
            force_relay_share: None,
            away_hours: (9.0, 4.0),
            connections_per_day: 29.0,
            relay_start_mb: 0.1,
            // 20 chat bursts of 50 KB, 18 tracker reports of 2 KB, two
            // 20 MB photo-browsing sessions.
            everyday_mb: 20.0 * 0.05 + 18.0 * 0.002 + 2.0 * 20.0,
            video: (5, 200.0),
            // 15 photos of 3 MB and one 60 MB video.
            captured_mb: 15.0 * 3.0 + 60.0,
            captured_away: 0.5,
            backups_on_relay: false,
            shares: (2.0, 5.0, 300.0),
            guest_mobile: 0.5,
            busiest_hour_share: 0.12,
            devices_connected: 0.30,
            pricing: Pricing {
                servers: 2,
                included_gb: 2000.0,
                server_price: 12.0,
                overage_per_gb: 0.01,
            },
        }
    }
}

/// What a run found. Sizes are in MB unless the name says otherwise.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Families simulated.
    pub families: usize,
    /// Days simulated.
    pub days: usize,
    /// Paired devices.
    pub devices: usize,
    /// Family traffic away from home that went direct.
    pub family_direct: f64,
    /// Family traffic away from home that stayed on the relay.
    pub family_relayed: f64,
    /// The relay's share of connections' first seconds.
    pub relay_starts: f64,
    /// Backups carried by the relay.
    pub backups_relayed: f64,
    /// Guest share traffic carried by the relay.
    pub guests_relayed: f64,
    /// Guest share traffic that went direct.
    pub guests_direct: f64,
    /// Member-days away, and how many of them stayed on the relay.
    pub away_days: (u64, u64),
    /// The relay's traffic on each day.
    pub relay_by_day: Vec<f64>,
    /// Each family's relay traffic in each 30-day month.
    pub family_months: Vec<f64>,
}

impl Report {
    /// Everything the relay carried.
    pub fn relay_total(&self) -> f64 {
        self.family_relayed + self.relay_starts + self.backups_relayed + self.guests_relayed
    }

    /// The relay's traffic per 30 days, in GB.
    pub fn relay_gb_per_month(&self) -> f64 {
        self.relay_total() / 1000.0 * 30.0 / self.days.max(1) as f64
    }

    /// Share of family traffic away from home (backups apart) that went direct.
    pub fn direct_share(&self) -> f64 {
        let all = self.family_direct + self.family_relayed + self.relay_starts;
        if all == 0.0 {
            1.0
        } else {
            self.family_direct / all
        }
    }

    /// Share of member-days away that stayed on the relay.
    pub fn relayed_day_share(&self) -> f64 {
        if self.away_days.0 == 0 {
            0.0
        } else {
            self.away_days.1 as f64 / self.away_days.0 as f64
        }
    }

    /// The busiest hour's relay traffic, in Mbit/s.
    pub fn busiest_mbit_s(&self, profile: &Profile) -> f64 {
        let busiest_day = self.relay_by_day.iter().copied().fold(0.0, f64::max);
        busiest_day * profile.busiest_hour_share * 8.0 / 3600.0
    }

    /// Endpoints holding a relay connection at the busiest time.
    pub fn connected_endpoints(&self, profile: &Profile) -> f64 {
        self.families as f64 + self.devices as f64 * profile.devices_connected
    }

    /// The relay servers' cost per month, in dollars.
    pub fn cost_per_month(&self, pricing: &Pricing) -> f64 {
        let included = pricing.included_gb * f64::from(pricing.servers);
        let over = (self.relay_gb_per_month() - included).max(0.0);
        pricing.server_price * f64::from(pricing.servers) + over * pricing.overage_per_gb
    }

    /// A percentile (0 to 100) of the families' monthly relay traffic, in GB.
    pub fn family_month_gb(&self, percentile: f64) -> f64 {
        let mut months = self.family_months.clone();
        months.sort_by(f64::total_cmp);
        if months.is_empty() {
            return 0.0;
        }
        let at = ((percentile / 100.0) * (months.len() - 1) as f64).round() as usize;
        months.get(at).copied().unwrap_or(0.0) / 1000.0
    }

    /// Family-months over a monthly limit, in GB.
    pub fn months_over(&self, limit_gb: f64) -> usize {
        self.family_months
            .iter()
            .filter(|&&mb| mb / 1000.0 > limit_gb)
            .count()
    }
}

/// Whether a connection between the box and a device goes direct.
fn goes_direct(
    profile: &Profile,
    rng: &mut Rng,
    home: HomeNet,
    home_v6: bool,
    away: AwayNet,
) -> bool {
    if let Some(share) = profile.force_relay_share {
        return !rng.chance(share);
    }
    let (away_v6, away_symmetric) = match away {
        AwayNet::Mobile => (
            rng.chance(profile.mobile_ipv6),
            rng.chance(profile.mobile_symmetric),
        ),
        AwayNet::OtherWifi => (
            rng.chance(profile.wifi_ipv6),
            rng.chance(profile.wifi_symmetric),
        ),
        // The relay reaches it over HTTPS; nothing direct does.
        AwayNet::UdpBlocked => return false,
    };
    if home_v6 && away_v6 && rng.chance(profile.ipv6_direct) {
        return true;
    }
    let mut chance = match (home.symmetric(), away_symmetric) {
        (true, true) => 0.0,
        (false, false) => profile.punch_both_easy,
        _ => profile.punch_one_symmetric,
    };
    if home == HomeNet::DoubleNat {
        chance *= profile.double_nat_factor;
    }
    rng.chance(chance)
}

/// Runs the model.
pub fn run(profile: &Profile, seed: u64) -> Report {
    let mut rng = Rng::new(seed);
    let months = profile.days.div_ceil(30).max(1);
    let mut report = Report {
        families: profile.families,
        days: profile.days,
        relay_by_day: vec![0.0; profile.days],
        family_months: Vec::with_capacity(profile.families * months),
        ..Report::default()
    };

    for _ in 0..profile.families {
        let members = rng.pick(&profile.members, 3);
        report.devices += (f64::from(members) * profile.devices_per_member).round() as usize;
        let home = rng.pick(&profile.home_nets, HomeNet::EasyNat);
        let home_v6 = rng.chance(profile.home_ipv6);
        let mut by_month = vec![0.0; months];

        for day in 0..profile.days {
            let weekday = day % 7 < 5;
            let hours = if weekday {
                profile.away_hours.0
            } else {
                profile.away_hours.1
            };
            let scale = hours / profile.away_hours.0.max(f64::MIN_POSITIVE);
            let mut relayed_today = 0.0;

            for member in 0..members {
                if hours <= 0.0 {
                    continue;
                }
                let away = rng.pick(&profile.away_nets, AwayNet::Mobile);
                let direct = goes_direct(profile, &mut rng, home, home_v6, away);
                report.away_days.0 += 1;

                let mut used = profile.everyday_mb * scale;
                let (every, video_mb) = profile.video;
                if every > 0 && (day as u32 + member).is_multiple_of(every) {
                    used += video_mb;
                }
                let starts = profile.connections_per_day * scale * profile.relay_start_mb;
                report.relay_starts += starts;
                relayed_today += starts;

                let backup = profile.captured_mb * profile.captured_away;
                if direct {
                    report.family_direct += used;
                } else {
                    report.away_days.1 += 1;
                    report.family_relayed += used;
                    relayed_today += used;
                    if profile.backups_on_relay {
                        report.backups_relayed += backup;
                        relayed_today += backup;
                    }
                }
            }

            if let Some(slot) = report.relay_by_day.get_mut(day) {
                *slot += relayed_today;
            }
            if let Some(slot) = by_month.get_mut(day / 30) {
                *slot += relayed_today;
            }
        }

        // Guests: views spread over the run.
        let (shares, guests, guest_mb) = profile.shares;
        let views = (shares * guests * profile.days as f64 / 30.0).round() as usize;
        let open_port = home.can_open_port();
        for _ in 0..views {
            let day = (rng.unit() * profile.days as f64) as usize;
            let away = if rng.chance(profile.guest_mobile) {
                AwayNet::Mobile
            } else {
                AwayNet::OtherWifi
            };
            let direct = open_port || goes_direct(profile, &mut rng, home, home_v6, away);
            if direct {
                report.guests_direct += guest_mb;
            } else {
                report.guests_relayed += guest_mb;
                if let Some(slot) = report.relay_by_day.get_mut(day) {
                    *slot += guest_mb;
                }
                if let Some(slot) = by_month.get_mut(day / 30) {
                    *slot += guest_mb;
                }
            }
        }

        // Only whole months count towards a family's monthly figure.
        let whole = profile.days / 30;
        report
            .family_months
            .extend(by_month.into_iter().take(whole.max(1)));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_answer() {
        let profile = Profile::default();
        let a = run(&profile, 7);
        let b = run(&profile, 7);
        assert_eq!(a.relay_total().to_bits(), b.relay_total().to_bits());
    }

    #[test]
    fn backups_never_use_the_relay_by_default() {
        let report = run(&Profile::default(), 1);
        assert_eq!(report.backups_relayed, 0.0);
    }

    #[test]
    fn backups_on_the_relay_add_traffic() {
        let profile = Profile {
            backups_on_relay: true,
            ..Profile::default()
        };
        let with = run(&profile, 1);
        let without = run(&Profile::default(), 1);
        assert!(with.backups_relayed > 0.0);
        assert!(with.relay_total() > without.relay_total());
    }

    #[test]
    fn more_relayed_connections_more_relay_traffic() {
        let at = |share| {
            let profile = Profile {
                force_relay_share: Some(share),
                ..Profile::default()
            };
            run(&profile, 3).relay_total()
        };
        assert!(at(0.05) < at(0.10));
        assert!(at(0.10) < at(0.25));
    }

    #[test]
    fn forced_share_is_respected() {
        let profile = Profile {
            force_relay_share: Some(0.10),
            ..Profile::default()
        };
        let share = run(&profile, 5).relayed_day_share();
        assert!((share - 0.10).abs() < 0.01, "share {share}");
    }

    #[test]
    fn the_default_mix_mostly_goes_direct() {
        let report = run(&Profile::default(), 9);
        let share = report.relayed_day_share();
        assert!(share > 0.05 && share < 0.40, "share {share}");
    }

    #[test]
    fn small_traffic_costs_only_the_servers() {
        let report = run(&Profile::default(), 2);
        let pricing = Profile::default().pricing;
        if report.relay_gb_per_month() < pricing.included_gb * f64::from(pricing.servers) {
            assert_eq!(report.cost_per_month(&pricing), 24.0);
        }
    }

    #[test]
    fn rng_picks_from_the_table() {
        let mut rng = Rng::new(4);
        for _ in 0..100 {
            let v = rng.pick(&[(1, 1.0), (2, 1.0)], 0);
            assert!(v == 1 || v == 2);
        }
        assert_eq!(rng.pick::<u8>(&[], 9), 9);
    }
}
