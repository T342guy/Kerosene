// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The engine's convars and console commands, and what becomes of the
//! requests the commands leave.
//!
//! Registered once, in [`Engine::with_game`], before the game's own. Kept
//! apart from the engine proper because it is a long list and reads as one:
//! everything a player or a config file can set or run is here.

use super::*;

/// Register the engine's convars.
pub(super) fn register_cvars(console: &mut Console) {
    console.register_cvar_ranged(
        "sv_tickrate",
        "64",
        Some(10.0),
        Some(256.0),
        ConVarFlags::NONE,
        "Server simulation steps per second.",
    );
    console.register_cvar(
        "sv_cheats",
        "0",
        ConVarFlags::NOTIFY | ConVarFlags::REPLICATED,
        "Allow cheat commands and convars.",
    );
    console.register_cvar(
        "sv_gravity",
        "800",
        ConVarFlags::REPLICATED,
        "World gravity, in kerosene units per second squared.",
    );
    console.register_cvar(
        "sv_maxspeed",
        "320",
        ConVarFlags::REPLICATED,
        "Maximum ground speed, in kerosene units per second.",
    );
    console.register_cvar(
        "sv_walkspeed",
        "150",
        ConVarFlags::REPLICATED,
        "Ground speed while the walk key (+speed) is held.",
    );
    console.register_cvar(
        "sv_accelerate",
        "10",
        ConVarFlags::REPLICATED,
        "Ground acceleration.",
    );
    console.register_cvar(
        "sv_airaccelerate",
        "10",
        ConVarFlags::REPLICATED,
        "Air acceleration.",
    );
    console.register_cvar(
        "sv_friction",
        "4",
        ConVarFlags::REPLICATED,
        "Ground friction.",
    );
    console.register_cvar(
        "sv_stopspeed",
        "100",
        ConVarFlags::REPLICATED,
        "Speed below which friction is applied as though at this speed.",
    );
    console.register_cvar(
        "sv_stepsize",
        "18",
        ConVarFlags::REPLICATED,
        "Tallest step walked up without jumping.",
    );
    console.register_cvar(
        "sv_jump_height",
        "57",
        ConVarFlags::REPLICATED,
        "Height a jump reaches, in kerosene units.",
    );
    console.register_cvar(
        "sv_air_max_wishspeed",
        "30",
        ConVarFlags::REPLICATED,
        "Air acceleration speed cap. This is what makes air strafing work.",
    );
    console.register_cvar(
        "sv_falldamage_safe",
        "580",
        ConVarFlags::REPLICATED,
        "Landing speed below which falling is harmless.",
    );
    console.register_cvar(
        "sv_falldamage_scale",
        "0.25",
        ConVarFlags::REPLICATED,
        "Damage per unit/s of landing speed above the safe threshold.",
    );
    console.register_cvar("sv_noclip", "0", ConVarFlags::CHEAT, "Fly through walls.");
    console.register_cvar_ranged(
        "r_fullscreen",
        "0",
        Some(0.0),
        Some(2.0),
        ConVarFlags::ARCHIVE,
        "0 in a window, 1 fullscreen (borderless), 2 fullscreen at the monitor's own mode.",
    );
    console.register_cvar(
        "map_autoreload",
        "0",
        ConVarFlags::NONE,
        "Reload the map, keeping your place, when it is rebuilt on disk. `play --watch` turns it on.",
    );
    console.register_cvar(
        "sv_use_range",
        "80",
        ConVarFlags::REPLICATED,
        "How far the use key reaches, in kerosene units.",
    );

    console.register_cvar(
        "sv_footstep_stride",
        "32",
        ConVarFlags::REPLICATED,
        "Distance on the ground between footstep sounds, in kerosene units.",
    );
    console.register_cvar(
        "sv_footstep_min_speed",
        "50",
        ConVarFlags::REPLICATED,
        "Horizontal speed below which footsteps are silent.",
    );
    console.register_cvar(
        "sv_footstep_trace",
        "48",
        ConVarFlags::REPLICATED,
        "How far down to trace to resolve the surface underfoot.",
    );

    console.register_cvar_ranged(
        "cl_fov",
        "90",
        Some(50.0),
        Some(130.0),
        ConVarFlags::ARCHIVE,
        "Horizontal field of view at 4:3.",
    );
    console.register_cvar_ranged(
        "sensitivity",
        "3",
        Some(0.01),
        Some(100.0),
        ConVarFlags::ARCHIVE,
        "Mouse sensitivity.",
    );
    console.register_cvar(
        "m_yaw",
        "0.022",
        ConVarFlags::ARCHIVE,
        "Yaw degrees per mouse count.",
    );
    console.register_cvar(
        "m_pitch",
        "0.022",
        ConVarFlags::ARCHIVE,
        "Pitch degrees per mouse count.",
    );
    console.register_cvar("m_invert", "0", ConVarFlags::ARCHIVE, "Invert mouse pitch.");

    console.register_cvar(
        "r_drawworld",
        "1",
        ConVarFlags::CHEAT,
        "Draw world geometry.",
    );
    console.register_cvar("r_fullbright", "0", ConVarFlags::CHEAT, "Ignore lightmaps.");
    console.register_cvar("r_lightmap", "1", ConVarFlags::CHEAT, "Apply lightmaps.");
    // Scales, not switches: 0 turns the effect off, 1 is as authored, and
    // anything above exaggerates it. Seeing what a normal map is doing is
    // most of why you would type this.
    console.register_cvar(
        "r_bumpmap",
        "1",
        ConVarFlags::CHEAT,
        "How far normal maps tilt a surface. 0 flattens them.",
    );
    console.register_cvar(
        "r_specular",
        "1",
        ConVarFlags::CHEAT,
        "Specular highlight level. 0 removes highlights.",
    );
    console.register_cvar(
        "r_novis",
        "0",
        ConVarFlags::CHEAT,
        "Ignore the PVS and draw everything.",
    );
    console.register_cvar(
        "r_speeds",
        "0",
        ConVarFlags::NONE,
        "Show per-frame render statistics.",
    );
    console.register_cvar_ranged(
        "mat_exposure",
        "1.0",
        Some(0.01),
        Some(16.0),
        ConVarFlags::ARCHIVE,
        "Overall brightness, applied by the tone-map pass.",
    );
    console.register_cvar_ranged(
        "mat_gamma",
        "1.0",
        Some(0.5),
        Some(2.5),
        ConVarFlags::ARCHIVE,
        "Display brightness: 1 as mastered, higher lifts the mid-tones. What an options menu's brightness slider sets.",
    );
    console.register_cvar(
        "r_vsync",
        "1",
        ConVarFlags::ARCHIVE,
        "Wait for the display between frames: no tearing, frame rate capped at the refresh rate.",
    );
    console.register_cvar_ranged(
        "mat_tonemap",
        "2",
        Some(0.0),
        Some(2.0),
        ConVarFlags::ARCHIVE,
        "Tone curve: 0 none (clip), 1 Reinhard, 2 ACES filmic.",
    );
    console.register_cvar(
        "r_dynamic",
        "1",
        ConVarFlags::NONE,
        "Draw dynamic lights: light_dynamic entities and the flashlight.",
    );
    console.register_cvar(
        "r_shadows",
        "1",
        ConVarFlags::ARCHIVE,
        "Real-time shadows from dynamic lights.",
    );
    console.register_cvar(
        "cl_flashlight",
        "0",
        ConVarFlags::NONE,
        "Whether the flashlight is on. The flashlight command toggles it.",
    );
    // Four samples when on: the one count every backend supports for the
    // HDR and depth formats. See `Renderer::set_msaa`.
    console.register_cvar_ranged(
        "r_msaa",
        "4",
        Some(0.0),
        Some(16.0),
        ConVarFlags::ARCHIVE,
        "Multisample anti-aliasing. 0 or 1 off; anything higher is 4x.",
    );
    console.register_cvar(
        "fps_max",
        "0",
        ConVarFlags::ARCHIVE,
        "Frame rate cap; 0 for unlimited.",
    );

    console.register_cvar(
        "r_debugdraw",
        "1",
        ConVarFlags::NONE,
        "Draw the lines a game asks for with Engine::debug_line and friends.",
    );
    console.register_cvar(
        "phys_debug",
        "0",
        ConVarFlags::CHEAT,
        "Draw physics prop collision boxes. 1 boxes, 2 boxes and bodies.",
    );
    console.register_cvar(
        "sv_stream",
        "1",
        ConVarFlags::ARCHIVE,
        "Load and unload streamed sections around the player. 0 keeps every section loaded.",
    );
    console.register_cvar(
        "sv_stream_linger",
        "3",
        ConVarFlags::ARCHIVE,
        "Seconds a section stays loaded after the player can no longer see into it.",
    );
    console.register_cvar(
        "r_stream_debug",
        "0",
        ConVarFlags::CHEAT,
        "Draw each streamed section's bounds: green loaded, yellow loading, red unloaded.",
    );
    console.register_cvar_ranged(
        "phys_hold_distance",
        "72",
        Some(32.0),
        Some(256.0),
        ConVarFlags::REPLICATED,
        "How far in front the pick-up tool carries a prop.",
    );
    console.register_cvar("phys_hold_speed", "700", ConVarFlags::REPLICATED, "How fast a carried prop is steered toward the hold point. Whatever it meets on the way still stops it.");
    console.register_cvar("phys_hold_accel", "6000", ConVarFlags::REPLICATED, "Ceiling on how hard a carried prop is accelerated toward the hold point. What it collides with can still refuse it.");
    console.register_cvar(
        "phys_hold_spin",
        "20",
        ConVarFlags::REPLICATED,
        "How fast a carried prop is turned toward the hold angle, in radians per second.",
    );
    console.register_cvar("phys_hold_spin_accel", "120", ConVarFlags::REPLICATED, "Ceiling on how hard a carried prop is turned toward the hold angle, in radians per second squared. What it is wedged against can still refuse it.");
    console.register_cvar("phys_player_push_force", "8000", ConVarFlags::REPLICATED, "How hard the player can shove a physics prop. A prop's own mass decides how far that gets it.");
    console.register_cvar(
        "phys_launch_speed",
        "650",
        ConVarFlags::REPLICATED,
        "Speed a thrown prop leaves the pick-up tool at, in kerosene units per second.",
    );

    console.register_cvar(
        "sv_pause_on_menu",
        "1",
        ConVarFlags::ARCHIVE,
        "Stop the world while the pause menu, the console or the store's overlay is open, or the window is in the background.",
    );
    console.register_cvar(
        "snd_mute_losefocus",
        "1",
        ConVarFlags::ARCHIVE,
        "Silence the game while its window is in the background.",
    );
    console.register_cvar_ranged(
        "volume",
        "0.7",
        Some(0.0),
        Some(1.0),
        ConVarFlags::ARCHIVE,
        "Master sound volume.",
    );
    console.register_cvar(
        "snd_reverb",
        "1",
        ConVarFlags::ARCHIVE,
        "Room reverb. 0 is dry everywhere.",
    );
    console.register_cvar(
        "snd_reverb_preset",
        "",
        ConVarFlags::ARCHIVE,
        "Force a room everywhere: room, hall, cave, or outdoor. Empty uses the map's.",
    );
    console.register_cvar(
        "snd_occlusion",
        "1",
        ConVarFlags::ARCHIVE,
        "Muffle sounds behind walls, and silence ones with no way through.",
    );
    console.register_cvar(
        "snd_air",
        "1",
        ConVarFlags::ARCHIVE,
        "Let distance take the highs out of a sound, the way air does.",
    );
    console.register_cvar(
        "snd_acoustics_debug",
        "0",
        ConVarFlags::CHEAT,
        "1 reports the room you are in; 2 also draws rooms and occlusion traces.",
    );
}

/// Register the engine's commands.
///
/// Commands that need engine state set a request on the console for the host
/// to act on, rather than reaching into the engine: a `ConCommand` handler
/// only gets the console, and threading the whole engine through it would make
/// every command able to do anything.
/// The `pause` command's request: the engine's own, since pausing is the
/// simulation's business and a dedicated server has it too.
const PAUSE: &str = "pause";
/// The host's: it has the frame.
pub const SCREENSHOT: &str = "screenshot";
/// Written through the VFS, not to whatever path was typed: scripts can run
/// console commands, and a map's must not be able to overwrite a file of the
/// player's.
const CONDUMP: &str = "condump";

pub(super) fn register_commands(console: &mut Console) {
    console.register_command(
        "toggleconsole",
        ConVarFlags::NONE,
        "Open or close the developer console.",
        |con, _| con.request(requests::TOGGLE_CONSOLE, ""),
    );

    // Bindings are the host's -- it owns the keyboard -- so these hand the
    // words over. Typing `bind w +forward` and typing `+forward` are the same
    // thing, which is what makes a binding a binding.
    console.register_command(
        "bind",
        ConVarFlags::NONE,
        "Bind a key to a command: bind <key> <command>. With only a key, show its binding.",
        |con, args| {
            if args.count() < 2 {
                con.warn("usage: bind <key> [command]");
                return;
            }
            let payload = args.rest.clone();
            con.request(requests::BIND, &payload);
        },
    );
    console.register_command(
        "unbind",
        ConVarFlags::NONE,
        "Remove a key's binding.",
        |con, args| match args.get(1) {
            Some(key) => {
                let key = key.to_string();
                con.request(requests::UNBIND, &key)
            }
            None => con.warn("usage: unbind <key>"),
        },
    );
    console.register_command(
        "bindlist",
        ConVarFlags::NONE,
        "Print every key binding.",
        |con, _| con.request(requests::BIND_LIST, ""),
    );
    console.register_command(
        "unbindall",
        ConVarFlags::NONE,
        "Remove every key binding. config.cfg starts with this so it sets them all.",
        |con, _| con.request(requests::UNBIND_ALL, ""),
    );

    console.register_command(
        "map",
        ConVarFlags::NONE,
        "Load a map: map <name>",
        |con, args| match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::MAP, name);
            }
            None => con.warn("usage: map <name>"),
        },
    );

    console.register_command(
        "script",
        ConVarFlags::CHEAT,
        "Run script source: script <code>",
        |con, args| {
            // Everything after the command word, unsplit: script source has
            // spaces in it and tokenising it would be actively wrong.
            let source = args.rest.clone();
            if source.trim().is_empty() {
                con.warn("usage: script <code>");
                return;
            }
            con.request(requests::SCRIPT, source);
        },
    );

    console.register_command(
        "script_execute",
        ConVarFlags::CHEAT,
        "Load and run a script file: script_execute <name>",
        |con, args| match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::SCRIPT_FILE, name);
            }
            None => con.warn("usage: script_execute <name>"),
        },
    );

    console.register_command(
        "script_reload",
        ConVarFlags::CHEAT,
        "Forget every loaded script and load them again.",
        |con, _| con.request(requests::SCRIPT_RELOAD, ""),
    );

    console.register_command(
        "condump",
        ConVarFlags::NONE,
        "Write the console scrollback to a file in the player's directory: condump [name]",
        |con, args| {
            let name = args.get(1).unwrap_or("condump.txt").to_string();
            con.request(CONDUMP, name);
        },
    );

    console.register_command(
        "play",
        ConVarFlags::NONE,
        "Play a sound, heard flat: play <name>",
        |con, args| match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::PLAY_SOUND, name);
            }
            None => con.warn("usage: play <name>"),
        },
    );

    console.register_command(
        "stopsound",
        ConVarFlags::NONE,
        "Stop every sound.",
        |con, _| con.request(requests::STOP_SOUND, ""),
    );

    console.register_command(
        "snd_restart",
        ConVarFlags::NONE,
        "Forget every loaded sound and reopen the audio device.",
        |con, _| con.request(requests::SOUND_RESTART, ""),
    );

    console.register_command(
        "pause",
        ConVarFlags::NONE,
        "Stop the world, or start it again.",
        |con, _| con.request(PAUSE, ""),
    );
    console.register_command("quit", ConVarFlags::NONE, "Exit.", |con, _| {
        con.request(requests::QUIT, "");
    });
    console.register_command("exit", ConVarFlags::NONE, "Exit.", |con, _| {
        con.request(requests::QUIT, "");
    });

    console.register_command(
        "flashlight",
        ConVarFlags::NONE,
        "Toggle the flashlight.",
        |con, _| {
            let on = con.bool("cl_flashlight");
            con.set_bool("cl_flashlight", !on);
        },
    );

    console.register_command(
        "screenshot",
        ConVarFlags::NONE,
        "Save the next frame as a PNG under screenshots/ in the player's directory.",
        |con, _| con.request(SCREENSHOT, ""),
    );

    console.register_command(
        "noclip",
        ConVarFlags::CHEAT,
        "Toggle flying through walls.",
        |con, _| {
            let on = con.bool("sv_noclip");
            con.set_bool("sv_noclip", !on);
            let state = if on { "off" } else { "on" };
            con.print(format!("noclip {state}"));
        },
    );

    console.register_command(
        "version",
        ConVarFlags::NONE,
        "Show the game's version and Kerosene's.",
        |con, _| {
            let game = con.string("_game").to_string();
            match game.is_empty() {
                true => con.print(format!("Kerosene {}", crate::VERSION)),
                false => con.print(format!("{game} (Kerosene {})", crate::VERSION)),
            }
        },
    );

    console.register_command(
        "phys_stats",
        ConVarFlags::NONE,
        "Show rigid-body simulation counts.",
        |con, _| {
            con.request(requests::PHYS_STATS, "");
        },
    );

    console.register_command(
        "phys_spawn",
        ConVarFlags::CHEAT,
        "Spawn a physics cube in front of you: phys_spawn [model]",
        |con, args| {
            let model = args.get(1).unwrap_or("props/cube").to_string();
            con.request(requests::PHYS_SPAWN, model);
        },
    );
}

/// Poll the console for requests engine commands left behind.
pub fn take_console_requests(engine: &mut Engine) -> Vec<(String, String)> {
    let mut unhandled = Vec::new();
    for (kind, payload) in engine.console.take_requests() {
        match kind.as_str() {
            requests::MAP => engine.request_map(&payload),
            requests::QUIT => engine.should_quit = true,
            kerosene_console::requests::BIND => {
                let mut words = payload.splitn(2, char::is_whitespace);
                let key = words.next().unwrap_or("").trim_matches('"');
                match words.next().map(|c| c.trim().trim_matches('"')) {
                    Some(command) if !command.is_empty() => engine.input.bind(key, command),
                    _ => {
                        let line = match engine.input.binding(key) {
                            Some(command) => format!("\"{key}\" = \"{command}\""),
                            None => format!("\"{key}\" is not bound"),
                        };
                        engine.console.print(line);
                    }
                }
            }
            kerosene_console::requests::UNBIND => engine.input.unbind(payload.trim()),
            kerosene_console::requests::UNBIND_ALL => engine.input.unbind_all(),
            kerosene_console::requests::BIND_LIST => {
                let listing = engine.input.to_config();
                if listing.is_empty() {
                    engine.console.print("no keys are bound");
                } else {
                    engine.console.print(listing);
                }
            }
            PAUSE => {
                engine.paused_by_command = !engine.paused_by_command;
                let word = if engine.paused_by_command {
                    "paused"
                } else {
                    "unpaused"
                };
                engine.console.print(word);
            }
            requests::SCRIPT => match engine.run_script(&payload) {
                Ok(Some(value)) => engine.console.echo(value),
                Ok(None) => {}
                Err(e) => engine.console.error(format!("script: {e}")),
            },
            requests::SCRIPT_FILE => {
                if let Err(e) = engine.load_script(&payload) {
                    engine.console.error(format!("script_execute: {e}"));
                }
            }
            requests::SCRIPT_RELOAD => engine.reload_scripts(),
            CONDUMP => {
                let text: String = engine
                    .console
                    .log()
                    .map(|line| format!("{}\n", line.text))
                    .collect();
                let lines = engine.console.log_len();
                match engine.vfs.write(&payload, text.as_bytes()) {
                    Ok(full) => engine
                        .console
                        .print(format!("wrote {lines} lines to {}", full.display())),
                    Err(e) => engine.console.error(format!("condump: {e}")),
                }
            }
            requests::PLAY_SOUND => {
                let vfs = engine.vfs.clone();
                if engine.audio.play(&vfs, &payload, None, 1.0).is_none() {
                    engine.console.warn(format!("could not play `{payload}`"));
                }
            }
            requests::STOP_SOUND => engine.audio.stop_all(),
            requests::SOUND_RESTART => {
                engine.audio = if engine.wants_audio {
                    crate::audio::AudioSystem::open()
                } else {
                    crate::audio::AudioSystem::silent()
                };
                engine.entity_voices.clear();
                let vfs = engine.vfs.clone();
                engine.audio.load_scripts(&vfs);
                let status = engine.audio.status.clone();
                engine.console.print(format!("audio: {status}"));
            }
            requests::PHYS_SPAWN => {
                // A cube a little in front of the player's eye, at chest
                // height, so the drop is visible immediately.
                let forward = engine.player.view_angles.forward();
                let eye = engine.player.movement.eye_position();
                let origin = eye + forward * 96.0;
                let model = if payload.is_empty() {
                    "props/cube".to_string()
                } else {
                    payload
                };
                engine.spawn_prop(&model, origin);
                engine
                    .console
                    .print(format!("spawned prop_physics ({model}) at {origin:.1}",));
            }
            requests::PHYS_STATS => {
                engine.console.print(format!(
                    "physics: {} props, {} static hulls, {} movers, {} bodies",
                    engine.physics.prop_count(),
                    engine.physics.static_body_count(),
                    engine.physics.mover_count(),
                    engine.physics.body_count(),
                ));
            }
            kind if engine.debug_console_request(kind, &payload) => {}
            kind if engine.frontend_console_request(kind) => {}
            kind if engine.ui_console_request(kind, &payload) => {}
            kind if engine.platform_console_request(kind, &payload) => {}
            kind if engine.save_console_request(kind, &payload) => {}
            // Not ours. The console can ask for things the *host* owns --
            // opening the console itself, most obviously -- and the engine
            // has no business knowing a window exists. Handing them back
            // beats teaching it.
            _ => unhandled.push((kind, payload)),
        }
    }
    unhandled
}

/// Report requests nobody claimed.
///
/// For a caller with nothing to add -- a headless server has no console to
/// open -- so that an unrecognised request is still said out loud rather than
/// dropped on the floor.
pub fn report_unhandled(engine: &mut Engine, requests: Vec<(String, String)>) {
    for (kind, payload) in requests {
        // The game's own commands leave requests of their own; it gets the
        // first refusal on anything the engine and the host did not know.
        let claimed = engine
            .with_game_mut(|game, engine| game.console_request(engine, &kind, &payload))
            .unwrap_or(false);
        if !claimed {
            engine
                .console
                .warn(format!("unknown host request `{kind}`"));
        }
    }
}
