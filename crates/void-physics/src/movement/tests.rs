use super::*;
use crate::world::BoxWorld;

const TICK: f32 = 1.0 / 64.0;

fn params() -> MoveParams { MoveParams::default() }

fn standing_on_floor() -> (MoveState, BoxWorld) {
    let world = BoxWorld::new().with_floor();
    let state = MoveState { origin: Vec3::new(0.0, 0.0, 0.0), on_ground: true, ..Default::default() };
    (state, world)
}

fn run(state: &mut MoveState, world: &BoxWorld, input: MoveInput, ticks: usize) {
    let p = params();
    for _ in 0..ticks {
        player_move(state, &input, &p, world, TICK);
    }
}

fn forward_input() -> MoveInput {
    MoveInput { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() }
}

#[test]
fn a_player_standing_still_stays_still() {
    let (mut state, world) = standing_on_floor();
    run(&mut state, &world, MoveInput::default(), 64);
    assert!(state.on_ground);
    assert!(state.velocity.length() < 0.5, "drifted at {:?}", state.velocity);
    assert!(state.origin.z.abs() < 0.2, "sank or floated to {}", state.origin.z);
}

#[test]
fn running_forward_reaches_max_speed_and_stops_there() {
    let (mut state, world) = standing_on_floor();
    run(&mut state, &world, forward_input(), 128);
    let speed = state.ground_speed();
    assert!(
        (speed - params().max_speed).abs() < 5.0,
        "expected to settle at {} units/s, got {speed}",
        params().max_speed
    );
    // Moving along +X, since yaw 0 looks down +X.
    assert!(state.origin.x > 100.0, "should have travelled: {:?}", state.origin);
    assert!(state.origin.y.abs() < 1.0);
}

#[test]
fn acceleration_takes_a_believable_amount_of_time() {
    // Source's acceleration of 10 adds `accel * dt * wish_speed` per tick --
    // 50 units at 64 tick -- so full speed arrives in about seven ticks.
    // Snappier than it feels in play, because friction and turning eat into
    // it constantly.
    let (mut state, world) = standing_on_floor();
    let p = params();
    let mut ticks = 0;
    while state.ground_speed() < p.max_speed * 0.99 && ticks < 200 {
        player_move(&mut state, &forward_input(), &p, &world, TICK);
        ticks += 1;
    }
    let seconds = ticks as f32 * TICK;
    assert!(ticks > 1, "reached full speed instantly, which would feel like a teleport");
    assert!(seconds < 0.3, "took {seconds:.2}s to reach full speed");
}

#[test]
fn releasing_the_key_brings_the_player_to_a_stop() {
    let (mut state, world) = standing_on_floor();
    run(&mut state, &world, forward_input(), 64);
    assert!(state.ground_speed() > 100.0);

    run(&mut state, &world, MoveInput::default(), 64);
    assert!(state.ground_speed() < 1.0, "still sliding at {}", state.ground_speed());
}

#[test]
fn friction_stops_a_slow_walk_promptly() {
    // The stop_speed floor exists for this: without it, deceleration tapers
    // off and a slow player drifts for a long time.
    let mut state = MoveState { velocity: Vec3::new(20.0, 0.0, 0.0), on_ground: true, ..Default::default() };
    let p = params();
    for _ in 0..32 { apply_friction(&mut state, &p, TICK); }
    assert!(state.velocity.length() < 1.0, "still moving at {:?}", state.velocity);
}

#[test]
fn a_jump_reaches_the_height_it_is_tuned_for() {
    let (mut state, world) = standing_on_floor();
    let p = params();
    let input = MoveInput { jump: true, ..Default::default() };

    let mut peak: f32 = 0.0;
    for _ in 0..64 {
        player_move(&mut state, &input, &p, &world, TICK);
        peak = peak.max(state.origin.z);
    }
    // Tuned for 57 units, so a player clears a 56-unit crate.
    assert!((50.0..64.0).contains(&peak), "jumped to {peak}, expected about 57");
}

#[test]
fn a_jump_lands_again() {
    let (mut state, world) = standing_on_floor();
    let input = MoveInput { jump: true, ..Default::default() };
    player_move(&mut state, &input, &params(), &world, TICK);
    assert!(!state.on_ground, "should have left the ground");

    run(&mut state, &world, MoveInput::default(), 128);
    assert!(state.on_ground, "should have landed");
    assert!(state.origin.z.abs() < 0.5, "landed at {}", state.origin.z);
}

#[test]
fn holding_jump_does_not_auto_bounce() {
    // Source requires releasing and pressing again; auto-bounce would make
    // bunny-hopping trivial rather than a skill.
    let (mut state, world) = standing_on_floor();
    let input = MoveInput { jump: true, ..Default::default() };
    run(&mut state, &world, input, 128);
    assert!(state.on_ground, "should have settled on the ground");
    assert!(state.jump_held);

    // Releasing and pressing again does jump.
    player_move(&mut state, &MoveInput::default(), &params(), &world, TICK);
    let result = player_move(&mut state, &input, &params(), &world, TICK);
    assert!(result.jumped);
}

#[test]
fn landing_reports_the_impact_speed() {
    let world = BoxWorld::new().with_floor();
    let mut state = MoveState { origin: Vec3::new(0.0, 0.0, 400.0), ..Default::default() };
    let p = params();
    let mut landing = None;
    for _ in 0..256 {
        let r = player_move(&mut state, &MoveInput::default(), &p, &world, TICK);
        if let Some(speed) = r.landed_at_speed { landing = Some(speed); break; }
    }
    let speed = landing.expect("should have landed");
    // v = sqrt(2 * g * h) for a 400-unit drop.
    let expected = (2.0 * p.gravity * 400.0).sqrt();
    assert!((speed - expected).abs() < expected * 0.1, "hit at {speed}, expected about {expected}");
}

#[test]
fn a_wall_stops_forward_movement_without_stopping_the_player_dead() {
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(100.0, -256.0, 0.0), Vec3::new(132.0, 256.0, 128.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };

    run(&mut state, &world, forward_input(), 128);
    assert!(state.origin.x < 100.0, "walked into the wall at {}", state.origin.x);
    assert!(state.origin.x > 60.0, "stopped far too early at {}", state.origin.x);
}

#[test]
fn a_player_slides_along_a_wall_instead_of_sticking() {
    // Running diagonally into a wall should convert into movement along it.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(100.0, -512.0, 0.0), Vec3::new(132.0, 512.0, 128.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };
    // Looking 45 degrees off the wall's normal.
    let input = MoveInput {
        forward: 1.0,
        view_angles: Angles::new(0.0, 45.0, 0.0),
        ..Default::default()
    };
    run(&mut state, &world, input, 128);
    assert!(state.origin.y > 100.0, "should have slid along the wall, y = {}", state.origin.y);
}

#[test]
fn a_low_step_is_walked_up_without_jumping() {
    // The ledge runs far enough that the player is still on it when the test
    // looks -- at full speed they cover 5 units a tick, and running off the
    // far end would land them back on the floor.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(64.0, -256.0, 0.0), Vec3::new(4096.0, 256.0, 16.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &world, forward_input(), 64);

    assert!(state.origin.x > 100.0, "did not get onto the step: x = {}", state.origin.x);
    assert!((state.origin.z - 16.0).abs() < 1.0, "should be standing on it, z = {}", state.origin.z);
    assert!(state.on_ground);
}

#[test]
fn a_step_taller_than_the_step_size_blocks() {
    // 18 is the limit; 32 is a wall you have to jump.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(64.0, -256.0, 0.0), Vec3::new(512.0, 256.0, 32.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &world, forward_input(), 128);
    assert!(state.origin.z < 8.0, "climbed a 32-unit ledge to z = {}", state.origin.z);
    assert!(state.origin.x < 64.0, "should be stopped by it, x = {}", state.origin.x);
}

#[test]
fn a_staircase_can_be_walked_up() {
    let mut world = BoxWorld::new().with_floor();
    for i in 0..8 {
        let x = 64.0 + i as f32 * 16.0;
        let z = (i + 1) as f32 * 8.0;
        // The top step runs on, so the test measures the climb rather than
        // where the player ends up after walking off the end.
        let far = if i == 7 { 4096.0 } else { x + 16.0 };
        world = world.solid(Vec3::new(x, -256.0, 0.0), Vec3::new(far, 256.0, z));
    }
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &world, forward_input(), 96);
    assert!(state.origin.z > 55.0, "only climbed to z = {}", state.origin.z);
    assert!(state.on_ground);
}

#[test]
fn walking_up_a_step_does_not_launch_the_player() {
    // The step must not add upward velocity, or stairs act as trampolines.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(64.0, -256.0, 0.0), Vec3::new(4096.0, 256.0, 16.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };
    let p = params();
    for _ in 0..64 {
        player_move(&mut state, &forward_input(), &p, &world, TICK);
        assert!(state.velocity.z <= 1.0, "step imparted upward velocity {}", state.velocity.z);
    }
}

#[test]
fn air_control_lets_a_moving_player_gain_speed() {
    // The behaviour the air-speed cap exists to allow. A player already at
    // full run speed, steering sideways in the air, should still gain.
    let p = params();
    let mut state = MoveState {
        velocity: Vec3::new(320.0, 0.0, 0.0),
        on_ground: false,
        ..Default::default()
    };
    let before = state.ground_speed();

    // Push perpendicular to the direction of travel.
    let side = Vec3::new(0.0, 1.0, 0.0);
    for _ in 0..8 {
        air_accelerate(&mut state, side, p.max_speed, p.air_accelerate, &p, TICK);
    }
    assert!(
        state.ground_speed() > before,
        "air strafing should add speed: {} -> {}",
        before,
        state.ground_speed()
    );
}

#[test]
fn air_control_is_capped_when_pushing_straight_ahead() {
    // Pushing in the direction already travelled gains nothing: the component
    // along that direction is already well past the 30-unit cap.
    let p = params();
    let mut state = MoveState {
        velocity: Vec3::new(320.0, 0.0, 0.0),
        on_ground: false,
        ..Default::default()
    };
    let before = state.velocity;
    air_accelerate(&mut state, Vec3::X, p.max_speed, p.air_accelerate, &p, TICK);
    assert_eq!(state.velocity, before, "forward air acceleration must be capped out");
}

#[test]
fn ground_acceleration_is_not_capped_the_way_air_is() {
    let p = params();
    let mut state = MoveState { on_ground: true, ..Default::default() };
    accelerate(&mut state, Vec3::X, p.max_speed, p.accelerate, &p, TICK);
    assert!(
        state.velocity.x > p.air_speed_cap,
        "ground acceleration reached only {}",
        state.velocity.x
    );
}

#[test]
fn clip_velocity_removes_motion_into_a_surface() {
    let v = Vec3::new(100.0, 0.0, -50.0);
    let out = clip_velocity(v, Vec3::Z, 1.0);
    assert_eq!(out.x, 100.0, "motion along the surface is preserved");
    assert!(out.z >= 0.0, "motion into the surface is removed, got {}", out.z);
}

#[test]
fn clip_velocity_never_leaves_motion_into_the_plane() {
    // The second pass exists because floats leave a residue that accumulates
    // into falling through the world.
    for normal in [Vec3::Z, Vec3::new(0.6, 0.0, 0.8).normalize(), -Vec3::X] {
        for v in [Vec3::new(1.0, 2.0, -300.0), Vec3::new(-50.0, 7.0, -0.001)] {
            let out = clip_velocity(v, normal, 1.0);
            assert!(out.dot(normal) >= -1e-4, "{out:?} still heads into {normal:?}");
        }
    }
}

#[test]
fn ducking_shortens_the_hull_and_slows_the_player() {
    let (mut state, world) = standing_on_floor();
    let input = MoveInput { forward: 1.0, duck: true, ..Default::default() };
    run(&mut state, &world, input, 128);

    assert!(state.ducked);
    assert_eq!(state.hull().maxs.z, 36.0);
    assert!(
        state.ground_speed() < params().max_speed * 0.5,
        "ducked speed was {}",
        state.ground_speed()
    );
}

#[test]
fn a_player_cannot_stand_up_under_a_low_ceiling() {
    // Ducked under a pipe: releasing duck must not let them stand into it.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(-256.0, -256.0, 40.0), Vec3::new(256.0, 256.0, 128.0));
    let mut state = MoveState { on_ground: true, ducked: true, ..Default::default() };
    run(&mut state, &world, MoveInput::default(), 8);
    assert!(state.ducked, "stood up into a ceiling 40 units above the floor");
}

#[test]
fn a_player_stands_up_when_there_is_room() {
    let (mut state, world) = standing_on_floor();
    state.ducked = true;
    run(&mut state, &world, MoveInput::default(), 8);
    assert!(!state.ducked);
    assert_eq!(state.hull().maxs.z, 72.0);
}

#[test]
fn water_is_detected_at_three_depths() {
    let world = BoxWorld::new()
        .with_floor()
        .volume(
            Vec3::new(-256.0, -256.0, 0.0),
            Vec3::new(256.0, 256.0, 40.0),
            contents::WATER,
        );
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &world, MoveInput::default(), 2);
    assert_eq!(state.water_level, WaterLevel::Waist, "40 units is waist deep on a 72-unit player");

    // Deeper water covers the eyes.
    let deep = BoxWorld::new().with_floor().volume(
        Vec3::new(-256.0, -256.0, 0.0),
        Vec3::new(256.0, 256.0, 200.0),
        contents::WATER,
    );
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &deep, MoveInput::default(), 2);
    assert_eq!(state.water_level, WaterLevel::Eyes);
}

#[test]
fn a_submerged_player_cannot_jump() {
    let world = BoxWorld::new().with_floor().volume(
        Vec3::new(-256.0, -256.0, 0.0),
        Vec3::new(256.0, 256.0, 200.0),
        contents::WATER,
    );
    let mut state = MoveState { on_ground: true, ..Default::default() };
    let input = MoveInput { jump: true, ..Default::default() };
    let r = player_move(&mut state, &input, &params(), &world, TICK);
    assert!(!r.jumped, "you cannot jump underwater");
}

#[test]
fn the_eye_position_follows_the_stance() {
    let mut state = MoveState { origin: Vec3::new(0.0, 0.0, 10.0), ..Default::default() };
    assert_eq!(state.eye_position().z, 74.0);
    state.ducked = true;
    assert_eq!(state.eye_position().z, 38.0);
}

#[test]
fn noclip_moves_freely_through_geometry() {
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(-512.0, -512.0, -512.0), Vec3::new(512.0, 512.0, 512.0));
    let mut state = MoveState { noclip: true, ..Default::default() };
    run(&mut state, &world, forward_input(), 32);
    assert!(state.origin.x > 100.0, "noclip should pass straight through, x = {}", state.origin.x);
}

#[test]
fn a_zero_length_tick_changes_nothing() {
    let (mut state, world) = standing_on_floor();
    let before = state;
    player_move(&mut state, &forward_input(), &params(), &world, 0.0);
    assert_eq!(state.origin, before.origin);
    assert_eq!(state.velocity, before.velocity);
}

#[test]
fn velocity_is_capped_at_terminal_speed() {
    let world = BoxWorld::new();
    let p = params();
    let mut state = MoveState { origin: Vec3::new(0.0, 0.0, 10000.0), ..Default::default() };
    for _ in 0..2000 {
        player_move(&mut state, &MoveInput::default(), &p, &world, TICK);
    }
    assert!(
        state.velocity.length() <= p.max_velocity + 1.0,
        "fell to {} units/s",
        state.velocity.length()
    );
}

#[test]
fn jump_height_tracks_gravity() {
    let p = MoveParams { gravity: 1600.0, ..Default::default() };
    let impulse = p.jump_for_height(57.0);
    // h = v^2 / 2g
    let height = impulse * impulse / (2.0 * p.gravity);
    assert!((height - 57.0).abs() < 0.01);
}

#[test]
fn walking_off_a_ledge_drops_the_player() {
    // The complement of the step tests: geometry that ends should not hold
    // the player up.
    let world = BoxWorld::new()
        .with_floor()
        .solid(Vec3::new(64.0, -256.0, 0.0), Vec3::new(256.0, 256.0, 16.0));
    let mut state = MoveState { on_ground: true, ..Default::default() };
    run(&mut state, &world, forward_input(), 128);
    assert!(state.origin.x > 256.0, "should have run off the end");
    assert!(state.origin.z < 1.0, "should have fallen back to the floor, z = {}", state.origin.z);
    assert!(state.on_ground);
}
