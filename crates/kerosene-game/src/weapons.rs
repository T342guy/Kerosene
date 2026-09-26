// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Weapons and abilities: a starting point, not a combat system.
//!
//! The engine had no weapons at all, and a HUD with nothing to show is not a
//! test of a HUD. This is the smallest thing that gives it real state to bind
//! to: three hitscan weapons in slots, a clip and a reserve each, reloading,
//! and one ability on a cooldown. Firing traces from the eye and leaves a
//! bullet hole; nothing takes damage, because nothing in the engine has health
//! but the player.
//!
//! It is pure logic, like everything in this crate: [`Arsenal::tick`] takes
//! what the player is pressing and returns [`WeaponEvent`]s, and whoever owns
//! the world -- `kerosene::game::Stock` -- does the tracing and publishes
//! [`Arsenal::state`] to the UI. A game with real weapons replaces all of it;
//! nothing else depends on it.
//!
//! Spread is random but deterministic: the generator is seeded by the shot
//! count, so a replay fires the same pellets.

/// One kind of weapon.
#[derive(Clone, PartialEq, Debug)]
pub struct WeaponDef {
    pub name: &'static str,
    /// The key that selects it: `slot1`, `slot2`...
    pub slot: u8,
    pub damage: f32,
    pub clip: u32,
    /// Rounds carried when picked up, outside the clip.
    pub reserve: u32,
    /// Seconds between shots.
    pub interval: f32,
    pub reload_time: f32,
    /// Half-angle of the cone shots land in, in degrees.
    pub spread: f32,
    /// Traces per shot.
    pub pellets: u32,
    pub range: f32,
    /// Keeps firing while the trigger is held.
    pub automatic: bool,
    pub decal: &'static str,
    pub decal_size: f32,
}

/// The three the stock game carries.
pub fn stock_weapons() -> Vec<WeaponDef> {
    vec![
        WeaponDef {
            name: "pistol",
            slot: 1,
            damage: 20.0,
            clip: 12,
            reserve: 48,
            interval: 0.2,
            reload_time: 1.2,
            spread: 0.6,
            pellets: 1,
            range: 4096.0,
            automatic: false,
            decal: "decals/bullet",
            decal_size: 6.0,
        },
        WeaponDef {
            name: "shotgun",
            slot: 2,
            damage: 9.0,
            clip: 6,
            reserve: 24,
            interval: 0.8,
            reload_time: 2.0,
            spread: 5.0,
            pellets: 7,
            range: 2048.0,
            automatic: false,
            decal: "decals/bullet",
            decal_size: 5.0,
        },
        WeaponDef {
            name: "rifle",
            slot: 3,
            damage: 14.0,
            clip: 30,
            reserve: 90,
            interval: 0.1,
            reload_time: 1.6,
            spread: 1.4,
            pellets: 1,
            range: 8192.0,
            automatic: true,
            decal: "decals/bullet",
            decal_size: 6.0,
        },
    ]
}

/// A weapon being carried.
#[derive(Clone, PartialEq, Debug)]
pub struct Weapon {
    pub def: WeaponDef,
    pub ammo: u32,
    pub reserve: u32,
}

/// A cooldown ability, such as a dash.
#[derive(Clone, PartialEq, Debug)]
pub struct Ability {
    pub name: &'static str,
    pub cooldown: f32,
    /// Seconds until it can be used again; 0 when ready.
    pub remaining: f32,
}

impl Ability {
    pub fn ready(&self) -> bool {
        self.remaining <= 0.0
    }

    /// How far through the cooldown, 1 when ready.
    pub fn charge(&self) -> f32 {
        if self.cooldown <= 0.0 {
            return 1.0;
        }
        (1.0 - self.remaining / self.cooldown).clamp(0.0, 1.0)
    }
}

/// Something that happened, for the owner to act on and tell the UI about.
#[derive(Clone, PartialEq, Debug)]
pub enum WeaponEvent {
    Switched {
        from: &'static str,
        to: &'static str,
    },
    /// A shot: trace each direction (yaw and pitch offsets from the view, in
    /// degrees) out to `range`.
    Fired {
        weapon: &'static str,
        directions: Vec<(f32, f32)>,
        damage: f32,
        range: f32,
        decal: &'static str,
        decal_size: f32,
    },
    /// The trigger was pulled on an empty clip.
    Empty,
    ReloadStarted,
    Reloaded,
    AbilityUsed(&'static str),
    AbilityReady(&'static str),
}

/// A value to publish.
#[derive(Clone, PartialEq, Debug)]
pub enum StateValue {
    Flag(bool),
    Number(f64),
    Text(String),
}

/// How long `weapon.firing` stays true after a shot, for a HUD to kick the
/// crosshair.
const FLASH: f32 = 0.08;

/// Everything the player carries.
#[derive(Clone, Debug)]
pub struct Arsenal {
    pub weapons: Vec<Weapon>,
    pub active: usize,
    previous: usize,
    /// Until the next shot may fire.
    cooldown: f32,
    /// Until a reload finishes; 0 when not reloading.
    reloading: f32,
    flash: f32,
    trigger_was_down: bool,
    shots: u32,
    pub abilities: Vec<Ability>,
}

impl Default for Arsenal {
    fn default() -> Self {
        Arsenal::new(stock_weapons())
    }
}

impl Arsenal {
    pub fn new(defs: Vec<WeaponDef>) -> Arsenal {
        Arsenal {
            weapons: defs
                .into_iter()
                .map(|def| Weapon {
                    ammo: def.clip,
                    reserve: def.reserve,
                    def,
                })
                .collect(),
            active: 0,
            previous: 0,
            cooldown: 0.0,
            reloading: 0.0,
            flash: 0.0,
            trigger_was_down: false,
            shots: 0,
            abilities: vec![Ability {
                name: "dash",
                cooldown: 3.0,
                remaining: 0.0,
            }],
        }
    }

    pub fn current(&self) -> Option<&Weapon> {
        self.weapons.get(self.active)
    }

    /// What a saved game or a level change keeps: rounds, the weapon in
    /// hand, cooldowns. Weapons are named rather than numbered, so a save
    /// outlives a change to the order they are defined in.
    pub fn save(&self) -> serde_json::Value {
        let name = |i: usize| self.weapons.get(i).map(|w| w.def.name);
        serde_json::json!({
            "active": name(self.active),
            "previous": name(self.previous),
            "weapons": self.weapons.iter().map(|w| serde_json::json!({
                "name": w.def.name,
                "ammo": w.ammo,
                "reserve": w.reserve,
            })).collect::<Vec<_>>(),
            "abilities": self.abilities.iter().map(|a| serde_json::json!({
                "name": a.name,
                "remaining": a.remaining,
            })).collect::<Vec<_>>(),
        })
    }

    /// Take back what [`save`](Arsenal::save) wrote. Anything it names that
    /// this arsenal does not have is ignored; anything it leaves out stays
    /// as it is. A reload in progress is not resumed.
    pub fn load(&mut self, data: &serde_json::Value) {
        let index = |key: &str| {
            let name = data.get(key)?.as_str()?;
            self.weapons.iter().position(|w| w.def.name == name)
        };
        let (active, previous) = (index("active"), index("previous"));
        let number =
            |v: &serde_json::Value, key: &str| v.get(key).and_then(serde_json::Value::as_f64);
        for saved in data
            .get("weapons")
            .and_then(|w| w.as_array())
            .into_iter()
            .flatten()
        {
            let Some(name) = saved.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            if let Some(w) = self.weapons.iter_mut().find(|w| w.def.name == name) {
                if let Some(n) = number(saved, "ammo") {
                    w.ammo = (n.max(0.0) as u32).min(w.def.clip);
                }
                if let Some(n) = number(saved, "reserve") {
                    w.reserve = n.max(0.0) as u32;
                }
            }
        }
        for saved in data
            .get("abilities")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
        {
            let Some(name) = saved.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            if let Some(a) = self.abilities.iter_mut().find(|a| a.name == name)
                && let Some(n) = number(saved, "remaining")
            {
                a.remaining = (n as f32).clamp(0.0, a.cooldown);
            }
        }
        self.active = active.unwrap_or(self.active);
        self.previous = previous.unwrap_or(self.previous);
        self.reloading = 0.0;
        self.flash = 0.0;
    }

    pub fn is_reloading(&self) -> bool {
        self.reloading > 0.0
    }

    fn switch_to(&mut self, index: usize) -> Vec<WeaponEvent> {
        if index == self.active || index >= self.weapons.len() {
            return Vec::new();
        }
        let from = self.weapons[self.active].def.name;
        self.previous = self.active;
        self.active = index;
        // Switching cancels a reload and takes a moment, as it should.
        self.reloading = 0.0;
        self.cooldown = self.cooldown.max(0.25);
        vec![WeaponEvent::Switched {
            from,
            to: self.weapons[index].def.name,
        }]
    }

    /// Select the weapon in a slot (1-based, as the keys are).
    pub fn select_slot(&mut self, slot: u8) -> Vec<WeaponEvent> {
        match self.weapons.iter().position(|w| w.def.slot == slot) {
            Some(i) => self.switch_to(i),
            None => Vec::new(),
        }
    }

    /// Back to the weapon held before this one.
    pub fn last(&mut self) -> Vec<WeaponEvent> {
        self.switch_to(self.previous)
    }

    /// Scroll through the weapons: +1 next, -1 previous.
    pub fn cycle(&mut self, dir: i32) -> Vec<WeaponEvent> {
        if self.weapons.is_empty() {
            return Vec::new();
        }
        let n = self.weapons.len() as i32;
        self.switch_to((self.active as i32 + dir).rem_euclid(n) as usize)
    }

    pub fn reload(&mut self) -> Vec<WeaponEvent> {
        let Some(w) = self.weapons.get(self.active) else {
            return Vec::new();
        };
        if self.is_reloading() || w.ammo >= w.def.clip || w.reserve == 0 {
            return Vec::new();
        }
        self.reloading = w.def.reload_time;
        vec![WeaponEvent::ReloadStarted]
    }

    /// Use an ability by name. `true` if it was ready.
    pub fn use_ability(&mut self, name: &str) -> Option<WeaponEvent> {
        let a = self.abilities.iter_mut().find(|a| a.name == name)?;
        if !a.ready() {
            return None;
        }
        a.remaining = a.cooldown;
        Some(WeaponEvent::AbilityUsed(a.name))
    }

    /// Advance one tick. `trigger` is whether attack is held; `can_fire` is
    /// false while attack means something else -- throwing a carried prop,
    /// pressing a panel -- so the pull is still seen, and a semi-automatic
    /// weapon does not fire the moment the prop has gone.
    pub fn tick(&mut self, dt: f32, trigger: bool, can_fire: bool) -> Vec<WeaponEvent> {
        let mut events = Vec::new();
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.flash = (self.flash - dt).max(0.0);
        for a in &mut self.abilities {
            if a.remaining > 0.0 {
                a.remaining = (a.remaining - dt).max(0.0);
                if a.remaining == 0.0 {
                    events.push(WeaponEvent::AbilityReady(a.name));
                }
            }
        }

        if self.reloading > 0.0 {
            self.reloading = (self.reloading - dt).max(0.0);
            if self.reloading == 0.0
                && let Some(w) = self.weapons.get_mut(self.active)
            {
                let take = (w.def.clip - w.ammo).min(w.reserve);
                w.ammo += take;
                w.reserve -= take;
                events.push(WeaponEvent::Reloaded);
            }
        }

        let pressed = trigger && !self.trigger_was_down;
        self.trigger_was_down = trigger;
        if !can_fire {
            return events;
        }
        let Some(w) = self.weapons.get(self.active) else {
            return events;
        };
        let wants = if w.def.automatic { trigger } else { pressed };
        if !wants || self.cooldown > 0.0 || self.is_reloading() {
            return events;
        }
        if w.ammo == 0 {
            // A click, once per pull, and an automatic reload if there is
            // anything to load.
            if pressed {
                events.push(WeaponEvent::Empty);
                events.extend(self.reload());
            }
            return events;
        }

        let def = w.def.clone();
        let mut directions = Vec::with_capacity(def.pellets as usize);
        for pellet in 0..def.pellets.max(1) {
            let mut rng = Rng::new(self.shots.wrapping_mul(97).wrapping_add(pellet * 13 + 1));
            // Uniform over the cone's disc, not its square.
            let r = rng.next().sqrt() * def.spread;
            let a = rng.next() * std::f32::consts::TAU;
            directions.push((r * a.cos(), r * a.sin()));
        }
        self.shots = self.shots.wrapping_add(1);
        self.cooldown = def.interval;
        self.flash = FLASH;
        if let Some(w) = self.weapons.get_mut(self.active) {
            w.ammo -= 1;
        }
        events.push(WeaponEvent::Fired {
            weapon: def.name,
            directions,
            damage: def.damage,
            range: def.range,
            decal: def.decal,
            decal_size: def.decal_size,
        });
        events
    }

    /// Everything a HUD might bind to, as `(key, value)`.
    pub fn state(&self) -> Vec<(String, StateValue)> {
        use StateValue::*;
        let mut out = Vec::new();
        if let Some(w) = self.current() {
            out.push(("weapon.active".into(), Text(w.def.name.into())));
            out.push(("weapon.slot".into(), Number(w.def.slot.into())));
            out.push(("weapon.ammo".into(), Number(w.ammo.into())));
            out.push(("weapon.clip".into(), Number(w.def.clip.into())));
            out.push(("weapon.reserve".into(), Number(w.reserve.into())));
            out.push(("weapon.reloading".into(), Flag(self.is_reloading())));
            let progress = if self.is_reloading() {
                1.0 - self.reloading / w.def.reload_time.max(1e-3)
            } else {
                0.0
            };
            out.push(("weapon.reload_progress".into(), Number(f64::from(progress))));
            out.push(("weapon.firing".into(), Flag(self.flash > 0.0)));
        }
        out.push(("weapon.count".into(), Number(self.weapons.len() as f64)));
        for (i, w) in self.weapons.iter().enumerate() {
            out.push((format!("weapons.{i}.name"), Text(w.def.name.into())));
            out.push((format!("weapons.{i}.slot"), Number(w.def.slot.into())));
            out.push((format!("weapons.{i}.active"), Flag(i == self.active)));
        }
        for a in &self.abilities {
            let base = format!("ability.{}", a.name);
            out.push((format!("{base}.ready"), Flag(a.ready())));
            out.push((format!("{base}.charge"), Number(f64::from(a.charge()))));
            // Rounded to tenths so a countdown label changes ten times a
            // second, not every tick.
            out.push((
                format!("{base}.remaining"),
                Number((f64::from(a.remaining) * 10.0).ceil() / 10.0),
            ));
        }
        out
    }
}

/// A small xorshift generator: enough for spread, and the same everywhere.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9) | 1)
    }

    /// 0..1.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fired(events: &[WeaponEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, WeaponEvent::Fired { .. }))
            .count()
    }

    #[test]
    fn a_semi_automatic_weapon_fires_once_per_pull() {
        let mut a = Arsenal::default();
        assert_eq!(fired(&a.tick(0.016, true, true)), 1);
        for _ in 0..60 {
            assert_eq!(fired(&a.tick(0.016, true, true)), 0);
        }
        a.tick(0.016, false, true);
        assert_eq!(fired(&a.tick(0.016, true, true)), 1);
        assert_eq!(a.current().unwrap().ammo, 10);
    }

    #[test]
    fn an_automatic_weapon_fires_at_its_rate() {
        let mut a = Arsenal::default();
        a.select_slot(3);
        let mut shots = 0;
        // One second, after the switch delay.
        for _ in 0..16 {
            a.tick(1.0 / 64.0, false, true);
        }
        for _ in 0..64 {
            shots += fired(&a.tick(1.0 / 64.0, true, true));
        }
        assert!((9..=11).contains(&shots), "{shots}");
    }

    #[test]
    fn switching_reports_both_ends_and_lastinv_goes_back() {
        let mut a = Arsenal::default();
        assert_eq!(
            a.select_slot(2),
            vec![WeaponEvent::Switched {
                from: "pistol",
                to: "shotgun"
            }]
        );
        assert!(a.select_slot(2).is_empty(), "already held");
        a.last();
        assert_eq!(a.current().unwrap().def.name, "pistol");
    }

    #[test]
    fn an_empty_clip_clicks_and_reloads() {
        let mut a = Arsenal::default();
        a.weapons[0].ammo = 0;
        let events = a.tick(0.016, true, true);
        assert!(
            events.contains(&WeaponEvent::Empty) && events.contains(&WeaponEvent::ReloadStarted)
        );
        let mut done = false;
        for _ in 0..200 {
            done |= a.tick(0.016, false, true).contains(&WeaponEvent::Reloaded);
        }
        assert!(done);
        assert_eq!((a.weapons[0].ammo, a.weapons[0].reserve), (12, 36));
    }

    #[test]
    fn a_shotgun_fires_its_pellets_within_the_cone_the_same_way_every_time() {
        let mut a = Arsenal::default();
        a.select_slot(2);
        let mut b = a.clone();
        let shot = |x: &mut Arsenal| {
            for _ in 0..20 {
                x.tick(0.016, false, true);
            }
            x.tick(0.016, true, true)
        };
        let (ea, eb) = (shot(&mut a), shot(&mut b));
        assert_eq!(ea, eb);
        let Some(WeaponEvent::Fired { directions, .. }) =
            ea.iter().find(|e| matches!(e, WeaponEvent::Fired { .. }))
        else {
            panic!("{ea:?}")
        };
        assert_eq!(directions.len(), 7);
        assert!(
            directions
                .iter()
                .all(|(y, p)| (y * y + p * p).sqrt() <= 5.0 + 1e-4)
        );
    }

    #[test]
    fn abilities_cool_down() {
        let mut a = Arsenal::default();
        assert!(a.use_ability("dash").is_some());
        assert!(a.use_ability("dash").is_none());
        assert_eq!(a.abilities[0].charge(), 0.0);
        let mut ready = false;
        for _ in 0..200 {
            ready |= a
                .tick(0.016, false, true)
                .contains(&WeaponEvent::AbilityReady("dash"));
        }
        assert!(ready && a.abilities[0].ready());
    }

    #[test]
    fn the_state_names_what_the_hud_binds_to() {
        let a = Arsenal::default();
        let state = a.state();
        let get = |k: &str| {
            state
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            get("weapon.active"),
            Some(StateValue::Text("pistol".into()))
        );
        assert_eq!(get("weapon.ammo"), Some(StateValue::Number(12.0)));
        assert_eq!(get("ability.dash.ready"), Some(StateValue::Flag(true)));
    }

    #[test]
    fn an_arsenal_saves_and_loads_by_name() {
        let mut a = Arsenal::default();
        a.weapons[0].ammo = 3;
        a.weapons[1].reserve = 5;
        a.select_slot(2);
        a.use_ability("dash");
        let text = a.save().to_string();

        let mut b = Arsenal::default();
        b.load(&serde_json::from_str(&text).unwrap());
        assert_eq!(b.weapons[0].ammo, 3);
        assert_eq!(b.weapons[1].reserve, 5);
        assert_eq!(b.current().unwrap().def.slot, 2);
        assert!(!b.abilities[0].ready(), "the cooldown came along");
        // `last` goes back to what was held before, as before the save.
        b.last();
        assert_eq!(b.current().unwrap().def.slot, 1);

        // Rubbish is ignored rather than trusted.
        let mut c = Arsenal::default();
        c.load(&serde_json::json!({"active": "nope", "weapons": [{"name": "nope", "ammo": 9}, {"ammo": 1}]}));
        assert_eq!(c.weapons, Arsenal::default().weapons);
    }
}
