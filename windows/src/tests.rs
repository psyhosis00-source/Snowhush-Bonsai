use std::collections::HashSet;

use crate::geometry::{Rect, V2};
use crate::simulation::{ScatterPiece, SnowSimulation, Snowfall, Sprite, WindowSurface};

const DT: f32 = 1.0 / 60.0;
const AWAY: V2 = V2::new(-10_000.0, -10_000.0);

fn run(simulation: &mut SnowSimulation, seconds: f32, cursor: V2) {
    for _ in 0..(seconds / DT) as usize {
        simulation.step(DT, cursor);
    }
}

fn surface(id: isize, x: f32, top: f32, width: f32) -> WindowSurface {
    WindowSurface { id, frame: Rect { x, y: top - 300.0, width, height: 300.0 }, blockers: Vec::new(), corner_radius: 8.0, corner_exponent: 2.5 }
}

/// Snow area per layer, from the smoothed surfaces the renderer gets.
fn piles(simulation: &SnowSimulation) -> Vec<(V2, f32)> {
    let mut buffer = vec![0.0; simulation.column_count()];
    let layers = simulation.write_layers(&mut buffer);
    layers
        .iter()
        .map(|pile| {
            let (surface, bed) = buffer[pile.offset..pile.offset + 2 * pile.count].split_at(pile.count);
            (pile.origin, surface.iter().zip(bed).map(|(s, b)| s - b).sum::<f32>() * 2.0)
        })
        .collect()
}

fn assert_finite(simulation: &SnowSimulation) {
    let mut sprites = vec![Sprite::default(); simulation.sprite_count()];
    let count = simulation.write_sprites(&mut sprites);
    for sprite in &sprites[..count] {
        assert!(sprite.position.x.is_finite() && sprite.position.y.is_finite() && sprite.size.is_finite() && sprite.alpha.is_finite());
    }
    let mut buffer = vec![0.0; simulation.column_count()];
    simulation.write_layers(&mut buffer);
    assert!(buffer.iter().all(|h| h.is_finite()));
}

#[test]
fn snow_falls_and_settles_on_the_ground() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    run(&mut simulation, 1.0, AWAY);
    assert!(simulation.sprite_count() > 500, "flakes: {}", simulation.sprite_count());
    run(&mut simulation, 60.0, AWAY);
    assert!(simulation.has_snow());
    let piles = piles(&simulation);
    assert_eq!(piles.len(), 1);
    assert!(piles[0].1 > 1000.0, "ground area {}", piles[0].1);
    assert_finite(&simulation);
}

#[test]
fn off_stops_the_snowfall() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Off);
    run(&mut simulation, 30.0, AWAY);
    assert_eq!(simulation.sprite_count(), 0);
    assert!(!simulation.has_snow());
}

#[test]
fn cursor_sweeps_the_drift_aside() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    simulation.thaws = false;
    run(&mut simulation, 120.0, AWAY);
    simulation.set_snowfall(Snowfall::Off);
    run(&mut simulation, 15.0, AWAY);
    let before = piles(&simulation)[0].1;

    let mut sprayed = 0;
    for i in 0..120 {
        let x = 200.0 + i as f32 * 8.0;
        simulation.step(DT, V2::new(x, 10.0));
        sprayed = sprayed.max(simulation.sprite_count());
    }
    assert!(sprayed > 20, "no snow thrown up: {sprayed}");
    run(&mut simulation, 10.0, AWAY);
    let after = piles(&simulation)[0].1;
    // Swept snow lands again; it moves rather than vanishes.
    assert!((after - before).abs() / before < 0.1, "before {before}, after {after}");
    assert_finite(&simulation);
}

#[test]
fn a_gust_blows_all_the_snow_away() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    let none = HashSet::new();
    let window = [surface(7, 300.0, 600.0, 500.0)];
    simulation.update_windows(&window, 0.2, &none, &none, false);
    run(&mut simulation, 90.0, AWAY);
    assert_eq!(piles(&simulation).len(), 2);

    simulation.blow_away(1.0, false);
    run(&mut simulation, 1.5, AWAY);
    assert!(!simulation.has_snow(), "drifts left: {:?}", piles(&simulation));
    run(&mut simulation, 3.0, AWAY);
    // The new snowfall starts above the screen and hasn't reached the ground yet.
    assert!(!simulation.has_snow());
    let mut sprites = vec![Sprite::default(); 20_000];
    let count = simulation.write_sprites(&mut sprites);
    assert!(sprites[..count].iter().all(|sprite| sprite.position.y > 400.0), "snow left in the lower half");
    run(&mut simulation, 60.0, AWAY);
    assert!(simulation.has_snow(), "the snowfall didn't come back");
    assert_finite(&simulation);
}

#[test]
fn a_calm_gust_melts_the_drifts_where_they_lie() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    run(&mut simulation, 90.0, AWAY);
    simulation.set_snowfall(Snowfall::Off);
    let before = piles(&simulation)[0].1;
    let falling = simulation.sprite_count();
    simulation.blow_away(-1.0, true);
    simulation.step(DT, AWAY);
    assert!(simulation.sprite_count() <= falling, "a calm gust shouldn't throw anything up");
    run(&mut simulation, 2.0, AWAY);
    // Only the last of the snowfall, still landing, is left.
    let after = piles(&simulation).first().map_or(0.0, |p| p.1);
    assert!(after < before * 0.05, "before {before}, after {after}");
}

#[test]
fn snow_settles_on_a_window_and_drops_when_it_closes() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    let none = HashSet::new();
    let window = [surface(7, 300.0, 600.0, 500.0)];
    simulation.update_windows(&window, 0.2, &none, &none, false);
    run(&mut simulation, 90.0, AWAY);
    let layers = piles(&simulation);
    assert_eq!(layers.len(), 2, "no snow on the window");
    let ledge = layers.iter().find(|(origin, _)| origin.y > 500.0).expect("ledge");
    assert!(ledge.1 > 200.0, "ledge area {}", ledge.1);

    simulation.set_snowfall(Snowfall::Off);
    simulation.update_windows(&[], 0.2, &none, &none, false);
    simulation.step(DT, AWAY);
    assert!(simulation.sprite_count() > 20, "the window's snow didn't fall");
    run(&mut simulation, 5.0, AWAY);
    assert_eq!(piles(&simulation).len(), 1);
    assert_finite(&simulation);
}

#[test]
fn dragged_window_sheds_its_snow() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
    simulation.thaws = false;
    let none = HashSet::new();
    simulation.update_windows(&[surface(3, 300.0, 600.0, 500.0)], 0.2, &none, &none, false);
    run(&mut simulation, 90.0, AWAY);
    simulation.set_snowfall(Snowfall::Off);
    run(&mut simulation, 10.0, AWAY);
    let resting = piles(&simulation).iter().find(|(origin, _)| origin.y > 500.0).map(|p| p.1).unwrap();

    // Shaken hard from side to side, as when a window is dragged back and forth.
    shake(&mut simulation, 3);
    run(&mut simulation, 3.0, AWAY);
    let shaken = piles(&simulation).iter().find(|(origin, _)| origin.y > 500.0).map_or(0.0, |p| p.1);
    assert!(shaken < resting * 0.7, "resting {resting}, after shaking {shaken}");
    assert_finite(&simulation);
}

#[test]
fn scattered_window_starts_the_held_snowfall() {
    let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Steady);
    simulation.set_snowfall_held(true);
    run(&mut simulation, 2.0, AWAY);
    assert_eq!(simulation.sprite_count(), 0, "snowfall should wait for the welcome window");

    let pieces: Vec<ScatterPiece> = (0..2000)
        .map(|i| ScatterPiece {
            position: V2::new(420.0 + (i % 70) as f32 * 6.0, 300.0 + (i / 70) as f32 * 6.0),
            velocity: V2::new(0.0, 60.0),
            color: [0.1, 0.09, 0.07],
        })
        .collect();
    simulation.scatter(&pieces);
    let mut released = false;
    for _ in 0..(10.0 / DT) as usize {
        simulation.step(DT, AWAY);
        released |= simulation.take_snowfall_released();
    }
    assert!(released, "the scattered snow never reached the ground");
    run(&mut simulation, 10.0, AWAY);
    assert!(simulation.sprite_count() > 200, "snowfall didn't start: {}", simulation.sprite_count());
    assert_finite(&simulation);
}

#[test]
fn a_window_slid_past_the_screen_edge_leaves_its_snow_heaped_against_it() {
    for (out, wall) in [(-300.0, 0.0), (1080.0, 1280.0)] {
        let mut simulation = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
        simulation.thaws = false;
        simulation.walls = (true, true);
        let none = HashSet::new();
        simulation.update_windows(&[surface(5, 400.0, 600.0, 500.0)], 0.2, &none, &none, false);
        run(&mut simulation, 90.0, AWAY);
        simulation.set_snowfall(Snowfall::Off);
        run(&mut simulation, 10.0, AWAY);
        let resting = ledge(&simulation);

        for step in 1..=180 {
            let x = 400.0 + (out - 400.0) * step as f32 / 180.0;
            simulation.update_windows(&[surface(5, x, 600.0, 500.0)], DT, &none, &none, false);
            simulation.step(DT, AWAY);
        }
        run(&mut simulation, 5.0, AWAY);
        let heaped = ledge(&simulation);
        // Three fifths of the window went past the wall, and their snow packs down once, to 0.6 of its bulk.
        assert!((heaped / resting - 0.76).abs() < 0.08, "resting {resting}, against the wall {heaped}");

        let mut buffer = vec![0.0; simulation.column_count()];
        let layers = simulation.write_layers(&mut buffer);
        let pile = layers.iter().find(|pile| pile.origin.y > 500.0).expect("ledge");
        let (surface, bed) = buffer[pile.offset..pile.offset + 2 * pile.count].split_at(pile.count);
        let depth = |i: usize| surface[i] - bed[i];
        let x = |i: usize| pile.origin.x + (i as f32 + 0.5) * 2.0;
        let past = (0..pile.count).filter(|&i| (x(i) - wall).abs() > 4.0 && (x(i) < 0.0 || x(i) > 1280.0)).map(depth).fold(0.0, f32::max);
        assert!(past < 0.01, "snow left past the edge at {wall}: {past}");
        let deepest = (0..pile.count).max_by(|&a, &b| depth(a).total_cmp(&depth(b))).unwrap();
        assert!((x(deepest) - wall).abs() < 30.0, "deepest snow at {}, the wall at {wall}", x(deepest));
        assert_finite(&simulation);
    }
}

fn shake(simulation: &mut SnowSimulation, id: isize) {
    let none = HashSet::new();
    let mut t = 0.0;
    for _ in 0..120 {
        t += DT;
        let x = 300.0 + 150.0 * (2.0 * std::f32::consts::PI * 4.0 * t).sin();
        simulation.update_windows(&[surface(id, x, 600.0, 500.0)], DT, &none, &none, false);
        simulation.step(DT, AWAY);
    }
}

fn ledge(simulation: &SnowSimulation) -> f32 {
    piles(simulation).iter().find(|(origin, _)| origin.y > 500.0).map_or(0.0, |p| p.1)
}

/// The same scenarios as the Swift harness, to compare the port against the original.
#[test]
#[ignore]
fn compare_with_swift() {
    let none = HashSet::new();
    let runs = 6;
    let (mut ground, mut ledge_area, mut dropped, mut shake_ratio, mut sweep_peak, mut sweep_ratio) = (0.0, 0.0, 0, 0.0, 0, 0.0);
    for _ in 0..runs {
        let mut a = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
        a.thaws = true;
        run(&mut a, 60.0, AWAY);
        ground += piles(&a)[0].1;

        let mut b = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
        b.update_windows(&[surface(3, 300.0, 600.0, 500.0)], 0.2, &none, &none, false);
        run(&mut b, 90.0, AWAY);
        b.set_snowfall(Snowfall::Off);
        run(&mut b, 10.0, AWAY);
        let resting = ledge(&b);
        ledge_area += resting;
        shake(&mut b, 3);
        run(&mut b, 3.0, AWAY);
        shake_ratio += ledge(&b) / resting;
        b.update_windows(&[surface(3, 300.0, 600.0, 500.0)], DT, &none, &none, false);
        run(&mut b, 3.0, AWAY);
        b.update_windows(&[], 0.2, &none, &none, false);
        b.step(DT, AWAY);
        dropped += b.sprite_count();

        let mut c = SnowSimulation::new(1280.0, 800.0, Snowfall::Heavy);
        run(&mut c, 120.0, AWAY);
        c.set_snowfall(Snowfall::Off);
        run(&mut c, 15.0, AWAY);
        let before = piles(&c)[0].1;
        let mut peak = 0;
        for i in 0..120 {
            c.step(DT, V2::new(200.0 + i as f32 * 8.0, 10.0));
            peak = peak.max(c.sprite_count());
        }
        run(&mut c, 10.0, AWAY);
        sweep_peak += peak;
        sweep_ratio += piles(&c)[0].1 / before;
    }
    let n = runs as f32;
    println!(
        "ground60 {:.0}  ledge90 {:.0}  shakeLeft {:.2}  dropped {}  sweepPeak {}  sweepKept {:.3}",
        ground / n,
        ledge_area / n,
        shake_ratio / n,
        dropped / runs,
        sweep_peak / runs,
        sweep_ratio / n
    );
}

#[test]
fn survives_a_long_chaotic_session() {
    let mut simulation = SnowSimulation::new(1920.0, 1080.0, Snowfall::Variable);
    let none = HashSet::new();
    let mut seed = 12345u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u32 << 24) as f32
    };
    let mut windows: Vec<(isize, f32, f32, f32)> = Vec::new();
    let mut cursor = V2::new(500.0, 100.0);
    for frame in 0..40_000 {
        if frame % 30 == 0 {
            let roll = next();
            if roll < 0.2 && windows.len() < 6 {
                windows.push((frame as isize + 1, next() * 1600.0 - 100.0, 200.0 + next() * 850.0, 60.0 + next() * 900.0));
            } else if roll < 0.3 && !windows.is_empty() {
                windows.remove((next() * windows.len() as f32) as usize % windows.len());
            }
        }
        for window in &mut windows {
            if next() < 0.05 {
                window.1 += (next() - 0.5) * 400.0;
                window.2 += (next() - 0.5) * 200.0;
            }
            if next() < 0.01 {
                window.3 = 20.0 + next() * 1200.0;
            }
        }
        let surfaces: Vec<WindowSurface> = windows.iter().map(|&(id, x, top, width)| surface(id, x, top, width)).collect();
        simulation.update_windows(&surfaces, DT, &none, &none, false);
        if next() < 0.3 {
            cursor = cursor + V2::new((next() - 0.5) * 300.0, (next() - 0.5) * 200.0);
        }
        if frame % 5000 == 0 {
            simulation.thaws = !simulation.thaws;
            simulation.accumulates = frame % 15000 != 0;
        }
        if frame == 20_000 {
            simulation.resize(1280.0, 720.0);
        }
        let dt = if next() < 0.01 { 0.05 } else { DT };
        simulation.step(dt, cursor);
    }
    assert_finite(&simulation);
}

#[test]
#[ignore]
fn step_cost() {
    let none = HashSet::new();
    let mut simulation = SnowSimulation::new(1920.0, 1080.0, Snowfall::Steady);
    simulation.thaws = true;
    let window = [WindowSurface {
        id: 1,
        frame: Rect { x: 200.0, y: 300.0, width: 900.0, height: 500.0 },
        blockers: Vec::new(),
        corner_radius: 8.0,
        corner_exponent: 2.5,
    }];
    simulation.update_windows(&window, 0.2, &none, &none, false);
    for _ in 0..60 * 30 {
        simulation.step(1.0 / 30.0, AWAY);
    }
    let steps = 30 * 120;
    let start = std::time::Instant::now();
    for i in 0..steps {
        simulation.step(1.0 / 30.0, V2::new((i % 1900) as f32, 20.0));
    }
    let us = start.elapsed().as_secs_f64() * 1e6 / steps as f64;
    println!("rust: {us:.1} µs per step, {} sprites", simulation.sprite_count());
}

// --- Weather abstraction & Rain unit tests ---

use crate::rain::{Rain, fall_angle_degrees};
use crate::weather::{Effect, Snow, Weather};

fn assert_weather_finite(weather: &dyn Weather) {
    let mut sprites = vec![Sprite::default(); weather.sprite_count().max(1)];
    let mut heights = vec![0.0; weather.column_count().max(1)];
    let out = weather.draw(&mut sprites, &mut heights);
    for sprite in &sprites[..out.sprites] {
        assert!(
            sprite.position.x.is_finite()
                && sprite.position.y.is_finite()
                && sprite.velocity.x.is_finite()
                && sprite.velocity.y.is_finite()
                && sprite.size.is_finite()
                && sprite.alpha.is_finite()
        );
    }
}

#[test]
fn effect_keys_roundtrip_and_default_is_snow() {
    assert_eq!(Effect::default(), Effect::Snow);
    for effect in Effect::ALL {
        assert_eq!(Effect::from_key(effect.key()), Some(effect));
    }
    assert_eq!(Effect::from_key("RAIN"), Some(Effect::Rain));
    assert_eq!(Effect::from_key("unknown"), None);
}

#[test]
fn weather_trait_wraps_snow_without_changing_behavior() {
    let mut snow = Snow::new(1280.0, 800.0, Snowfall::Heavy);
    let weather: &mut dyn Weather = &mut snow;
    for _ in 0..180 {
        weather.step(DT, &[], None, AWAY);
    }
    assert!(weather.sprite_count() > 400);
    for _ in 0..3600 {
        weather.step(DT, &[], None, AWAY);
    }
    assert!(weather.has_pile());
    assert_weather_finite(weather);
}

#[test]
fn rain_drops_fall_fast_and_slant_with_wind() {
    let mut calm_rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 7);
    calm_rain.spawn_drop_at(V2::new(640.0, 700.0), V2::new(0.0, -1000.0), 0.5);
    calm_rain.step(0.1, &[], Some(0.0), AWAY);

    let mut sprites = [Sprite::default(); 8];
    let mut heights = [];
    let calm_out = calm_rain.draw(&mut sprites, &mut heights);
    assert_eq!(calm_out.sprites, 1);
    assert!(calm_out.piles.is_empty());
    assert!(sprites[0].lump < 0.0, "rain streak should carry negative lump marker");
    assert!((sprites[0].position.x - 640.0).abs() < 1e-3);
    assert!((sprites[0].position.y - 600.0).abs() < 1e-2);
    assert!(fall_angle_degrees(sprites[0].velocity).abs() < 0.1);

    let mut windy_right = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 7);
    windy_right.spawn_drop_at(V2::new(640.0, 700.0), V2::new(0.0, -1000.0), 0.5);
    for _ in 0..30 {
        windy_right.step(DT, &[], Some(260.0), AWAY);
    }
    let right_out = windy_right.draw(&mut sprites, &mut heights);
    assert_eq!(right_out.sprites, 1);
    assert!(sprites[0].position.x > 690.0, "positive wind should push drop right: {}", sprites[0].position.x);
    assert!(fall_angle_degrees(sprites[0].velocity) > 8.0, "angle should lean right");

    let mut windy_left = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 7);
    windy_left.spawn_drop_at(V2::new(640.0, 700.0), V2::new(0.0, -1000.0), 0.5);
    for _ in 0..30 {
        windy_left.step(DT, &[], Some(-260.0), AWAY);
    }
    windy_left.draw(&mut sprites, &mut heights);
    assert!(sprites[0].position.x < 590.0, "negative wind should push drop left: {}", sprites[0].position.x);
    assert!(fall_angle_degrees(sprites[0].velocity) < -8.0, "angle should lean left");
}

#[test]
fn rain_collides_with_window_top_edge_and_spawns_short_splashes() {
    let mut rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 99);
    let win = [surface(1, 300.0, 500.0, 400.0)];
    // Place drop at y = 510 moving at -1000 DIP/s: in one 1/60s step it travels to 493.3, crossing top edge y = 500.
    rain.spawn_drop_at(V2::new(500.0, 510.0), V2::new(0.0, -1000.0), 0.6);
    assert_eq!(rain.drop_count(), 1);
    assert_eq!(rain.splash_count(), 0);

    rain.step(DT, &win, Some(0.0), AWAY);
    assert_eq!(rain.drop_count(), 0, "drop should vanish upon hitting window top edge");
    assert!((3..=5).contains(&rain.splash_count()), "expected 3..=5 splash droplets, got {}", rain.splash_count());
    assert!(!rain.has_pile(), "rain must not accumulate");
    assert_eq!(rain.column_count(), 0);

    // Splashes must live strictly within 100..=250 ms: still present at ~50 ms, all gone after 260 ms.
    for _ in 0..3 {
        rain.step(0.01, &win, Some(0.0), AWAY);
    }
    assert!(rain.splash_count() > 0, "splashes should still be visible ~46 ms after impact");

    for _ in 0..23 {
        rain.step(0.01, &win, Some(0.0), AWAY);
    }
    assert_eq!(rain.splash_count(), 0, "all splashes must expire by 250 ms");
}

#[test]
fn rain_ground_collision_spawns_splashes_at_work_area_bottom() {
    let mut rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 123);
    rain.spawn_drop_at(V2::new(400.0, 8.0), V2::new(40.0, -900.0), 0.7);
    rain.step(DT, &[], Some(40.0), AWAY);
    assert_eq!(rain.drop_count(), 0);
    assert!(rain.splash_count() >= 3);
    assert_weather_finite(&rain);
}

#[test]
fn rain_respects_occluding_front_windows() {
    let front_blocker = Rect { x: 350.0, y: 420.0, width: 220.0, height: 260.0 }; // covers x in 350..570, y in 420..680
    let mut back_window = surface(2, 300.0, 500.0, 500.0); // top edge at y = 500, x in 300..800
    back_window.blockers.push(front_blocker);
    let surfaces = [back_window];

    // Drop falling onto the occluded section (x = 450, y crosses 500) must NOT splash on the hidden back edge.
    let mut occluded_rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 42);
    occluded_rain.spawn_drop_at(V2::new(450.0, 510.0), V2::new(0.0, -1000.0), 0.5);
    occluded_rain.step(DT, &surfaces, Some(0.0), AWAY);
    assert_eq!(occluded_rain.drop_count(), 1, "drop should pass past occluded back-window edge");
    assert_eq!(occluded_rain.splash_count(), 0);

    // Drop falling onto the exposed section (x = 700, y crosses 500) MUST hit and splash.
    let mut exposed_rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 42);
    exposed_rain.spawn_drop_at(V2::new(700.0, 510.0), V2::new(0.0, -1000.0), 0.5);
    exposed_rain.step(DT, &surfaces, Some(0.0), AWAY);
    assert_eq!(exposed_rain.drop_count(), 0, "drop should hit visible section of window edge");
    assert!(exposed_rain.splash_count() >= 3);
}

#[test]
fn rain_optional_trickle_is_off_by_default_and_works_when_enabled() {
    let win = [surface(10, 200.0, 520.0, 800.0)];
    let mut rain = Rain::with_seed(1280.0, 800.0, Snowfall::Heavy, 2026);
    assert!(!rain.trickle(), "trickle flag must default to false");

    for _ in 0..180 {
        rain.step(DT, &win, Some(40.0), AWAY);
    }
    assert_eq!(rain.trickle_count(), 0, "no trickles may spawn while disabled");

    rain.set_trickle(true);
    let mut max_trickles = 0;
    for _ in 0..240 {
        rain.step(DT, &win, Some(60.0), AWAY);
        max_trickles = max_trickles.max(rain.trickle_count());
    }
    assert!(max_trickles > 0, "expected trickling drops along window top edge when enabled");
    assert_weather_finite(&rain);

    rain.set_trickle(false);
    assert_eq!(rain.trickle_count(), 0, "disabling trickle should clear active ledge beads");
}

#[test]
fn rain_kinematics_and_collisions_are_dt_independent() {
    // 1. Exact drop motion & wind relaxation across 120 Hz, 60 Hz, and 30 Hz.
    let run_single = |steps: usize, dt: f32| -> Sprite {
        let mut rain = Rain::with_seed(1920.0, 1080.0, Snowfall::Off, 55);
        rain.spawn_drop_at(V2::new(500.0, 950.0), V2::new(20.0, -960.0), 0.6);
        for _ in 0..steps {
            rain.step(dt, &[], Some(210.0), AWAY);
        }
        let mut sprites = [Sprite::default(); 4];
        let mut heights = [];
        let out = rain.draw(&mut sprites, &mut heights);
        assert_eq!(out.sprites, 1);
        sprites[0]
    };

    let s120 = run_single(60, 1.0 / 120.0); // 0.5 s total
    let s60 = run_single(30, 1.0 / 60.0); // 0.5 s total
    let s30 = run_single(15, 1.0 / 30.0); // 0.5 s total

    assert!((s120.position.x - s60.position.x).abs() < 1e-2, "x 120Hz={} vs 60Hz={}", s120.position.x, s60.position.x);
    assert!((s120.position.x - s30.position.x).abs() < 1e-2, "x 120Hz={} vs 30Hz={}", s120.position.x, s30.position.x);
    assert!((s120.position.y - s30.position.y).abs() < 1e-2, "y 120Hz={} vs 30Hz={}", s120.position.y, s30.position.y);
    assert!((s120.velocity.x - s30.velocity.x).abs() < 1e-2);

    // 2. Window collision + post-impact splash positions across 120 Hz vs 30 Hz.
    let win = [surface(1, 200.0, 500.0, 800.0)];
    let run_impact = |steps: usize, dt: f32| -> Vec<V2> {
        let mut rain = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 777);
        // Set initial vx equal to target wind * wind_scale so path is straight across impact.
        let depth: f32 = 0.5;
        let wind_scale = 0.65 + depth * 0.35;
        let wind = 100.0;
        rain.spawn_drop_at(V2::new(500.0, 620.0), V2::new(wind * wind_scale, -1000.0), depth);
        for _ in 0..steps {
            rain.step(dt, &win, Some(wind), AWAY);
        }
        let mut sprites = vec![Sprite::default(); 16];
        let mut heights = [];
        let out = rain.draw(&mut sprites, &mut heights);
        sprites[..out.sprites].iter().map(|s| s.position).collect()
    };

    // Impact occurs at t = (620 - 500) / 1000 = 0.12 s; evaluate at T = 0.20 s (80 ms into splash life).
    let impact_120 = run_impact(24, 1.0 / 120.0);
    let impact_30 = run_impact(6, 1.0 / 30.0);
    assert_eq!(impact_120.len(), impact_30.len());
    assert!(!impact_120.is_empty());
    for (a, b) in impact_120.iter().zip(&impact_30) {
        assert!((a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() < 0.05, "splash mismatch: {a:?} vs {b:?}");
    }

    // 3. Steady-state active raindrop density across 120 Hz, 60 Hz, and 30 Hz.
    let steady_count = |fps: usize| -> usize {
        let dt = 1.0 / fps as f32;
        let mut rain = Rain::with_seed(1920.0, 1080.0, Snowfall::Steady, 31415);
        for _ in 0..(fps * 5) {
            rain.step(dt, &win, Some(80.0), AWAY);
        }
        assert_weather_finite(&rain);
        rain.drop_count()
    };
    let n120 = steady_count(120) as f32;
    let n60 = steady_count(60) as f32;
    let n30 = steady_count(30) as f32;
    assert!((n120 - n60).abs() / n120 < 0.12, "steady drops 120Hz={n120} vs 60Hz={n60}");
    assert!((n120 - n30).abs() / n120 < 0.12, "steady drops 120Hz={n120} vs 30Hz={n30}");
}

#[test]
fn rain_intensities_and_natural_gusts_remain_stable() {
    let win = [surface(1, 250.0, 480.0, 700.0)];
    let mut off = Rain::with_seed(1280.0, 800.0, Snowfall::Off, 11);
    for _ in 0..120 {
        off.step(DT, &win, None, AWAY);
    }
    assert_eq!(off.sprite_count(), 0);

    let mut counts = Vec::new();
    for level in [Snowfall::Light, Snowfall::Steady, Snowfall::Heavy] {
        let mut rain = Rain::with_seed(1280.0, 800.0, level, 19);
        for _ in 0..180 {
            rain.step(DT, &win, None, AWAY);
        }
        assert_weather_finite(&rain);
        counts.push(rain.drop_count());
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2], "expected Light < Steady < Heavy: {counts:?}");

    // Variable intensity + natural gusts over 45 simulated seconds.
    let mut variable = Rain::with_seed(1920.0, 1080.0, Snowfall::Variable, 23);
    variable.set_trickle(true);
    let mut min_wind = f32::MAX;
    let mut max_wind = f32::MIN;
    for _ in 0..(45 * 60) {
        variable.step(DT, &win, None, AWAY);
        min_wind = min_wind.min(variable.current_wind());
        max_wind = max_wind.max(variable.current_wind());
    }
    assert!(max_wind - min_wind > 120.0, "gusts should vary wind angle noticeably: {min_wind}..{max_wind}");
    assert_weather_finite(&variable);
}

