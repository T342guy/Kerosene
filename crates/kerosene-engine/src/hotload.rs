// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Reloading the map when it is rebuilt: `map_autoreload`.
//!
//! What `kerosene-tools play --watch` turns on, so a map saved in the editor
//! is rebuilt by Kiln and walked into without restarting the game. The
//! engine looks at the loaded map's file once a second; when it changes, the
//! map loads again and the player is put back where they stood.
//!
//! Kiln writes a map's build stamp (`.kbuild`) after the last compiler is
//! done with it, so where there is one that is what is watched: the `.kbsp`
//! itself is rewritten by each compiler in turn, and watching it would load
//! the map unlit, then lit.

use crate::engine::Engine;
use std::path::PathBuf;
use std::time::SystemTime;

/// How often the file is looked at, in seconds of real time.
const INTERVAL: f32 = 1.0;

/// The file being watched and when it was last written.
#[derive(Default, Debug)]
pub(crate) struct MapWatch {
    map: String,
    file: Option<PathBuf>,
    written: Option<SystemTime>,
    since: f32,
}

fn written(file: &std::path::Path) -> Option<SystemTime> {
    file.metadata().and_then(|m| m.modified()).ok()
}

impl Engine {
    /// Once a frame: reload the map if `map_autoreload` is on and its file
    /// has been rebuilt since it was loaded.
    pub(crate) fn watch_map(&mut self, real_dt: f32) {
        if self.console.int("map_autoreload") == 0 {
            return;
        }
        let Some(name) = self.map_name().map(str::to_string) else {
            return;
        };
        let watch = &mut self.map_watch;
        watch.since += real_dt;
        if watch.map != name {
            // A map loaded since the last look: start watching it, as it is.
            let vfs = &self.vfs;
            let stamp = vfs.disk_path(&format!("maps/{name}.{}", kerosene_vfs::ext::BUILD_STAMP));
            let file =
                stamp.or_else(|| vfs.disk_path(&format!("maps/{name}.{}", kerosene_vfs::ext::BSP)));
            *watch = MapWatch {
                written: file.as_deref().and_then(written),
                map: name,
                file,
                since: 0.0,
            };
            return;
        }
        if watch.since < INTERVAL {
            return;
        }
        watch.since = 0.0;
        let Some(file) = &watch.file else { return };
        let now = written(file);
        if now.is_none() || now == watch.written {
            return;
        }
        watch.written = now;

        let origin = self.player.movement.origin;
        let angles = self.player.view_angles;
        self.console
            .print(format!("{name} was rebuilt; reloading it"));
        match self.load_map(&name) {
            Ok(()) => self.teleport_player(origin, Some(angles)),
            Err(e) => self.console.error(format!("reloading {name}: {e:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{Engine, EngineConfig};
    use kerosene_math::{Angles, Vec3};

    #[test]
    fn a_rebuilt_map_is_reloaded_with_the_player_where_they_stood() {
        let dir = std::env::temp_dir().join(format!("kerosene-hotload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("maps")).unwrap();
        // The base content's room, as a loose file that can be rewritten.
        let name = crate::base::FALLBACK_MAP;
        let bytes = Engine::new(&EngineConfig::default())
            .vfs()
            .read(&format!("maps/{name}.kbsp"))
            .unwrap();
        let loose = dir.join(format!("maps/{name}.kbsp"));
        std::fs::write(&loose, &bytes).unwrap();

        let config = EngineConfig {
            content_paths: vec![dir.clone()],
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(&config);
        engine.console.set("map_autoreload", "1");
        engine.load_map(name).unwrap();
        engine.frame(0.0, &Default::default());
        let spot = engine.player.movement.origin + Vec3::new(16.0, 0.0, 0.0);
        engine.teleport_player(spot, Some(Angles::new(0.0, 45.0, 0.0)));
        let generation = engine.load_generation();

        // Nothing changed: nothing happens.
        engine.frame(1.5, &Default::default());
        assert_eq!(engine.load_generation(), generation);

        // Rebuilt: it loads again, and the player is still there.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&loose)
            .unwrap()
            .set_modified(later)
            .unwrap();
        engine.frame(1.5, &Default::default());
        assert_ne!(engine.load_generation(), generation);
        // Where they stood, give or take the ticks that ran after it: they
        // may have settled onto the floor, but not been sent to the start.
        let at = engine.player.movement.origin;
        assert_eq!((at.x, at.y), (spot.x, spot.y));
        assert_eq!(engine.player.view_angles.yaw, 45.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
